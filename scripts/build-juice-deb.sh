#!/usr/bin/env bash
# Packages PA3GSB's radioberry-juice (the USB<->network bridge for the
# Radioberry) as a standalone .deb -- so installing it on a fresh Linux
# box is `sudo apt install ./radioberry-juice_*.deb`, same one-liner as
# hpsdr-rs itself, rather than a manual build-juice-linux.sh + sudo
# install-linux.sh dance. juice isn't part of this repo (it's PA3GSB's
# own project, this project's author separately maintains a local
# checkout of it) -- this script only wraps its own build (linux-Makefile)
# and its own installer (install-linux.sh, run in --destdir staging mode
# to produce a package root instead of touching the live system).
#
# NOTE ON THE BUNDLED FTDI D2XX LIBRARY: juice's own source tree already
# bundles FTDI's proprietary libftd2xx.so (see
# ftdi/linux/1.4.35/<arch>/lib/ in the juice source), which this .deb
# ends up redistributing as-is. FTDI's own license terms (the notice at
# the top of ftd2xx.h, also published at
# https://ftdichip.com/driver-licence-terms/) permit this under two
# conditions this script deliberately satisfies:
#   1. "FTDI DRIVERS MAY BE DISTRIBUTED IN ANY FORM AS LONG AS LICENSE
#      INFORMATION IS NOT MODIFIED" -- ftd2xx.h/README.pdf/release-notes.txt/
#      SOURCE.txt (which together carry that notice) are bundled
#      unmodified below, per install-linux.sh's own required-files list,
#      and the notice is additionally copied verbatim into
#      usr/share/doc/radioberry-juice/copyright, the standard place a
#      .deb documents third-party license terms.
#   2. "FTDI DRIVERS MAY BE USED ONLY IN CONJUNCTION WITH PRODUCTS BASED
#      ON FTDI PARTS" -- the Radioberry's own FT2232H is a genuine FTDI
#      part (this is what juice talks to; there is no other use for this
#      library here), and the package Description below says so
#      explicitly.
set -euo pipefail

JUICE_SRC="${1:-$HOME/Documents/juice/firmware-extended}"
VERSION="${JUICE_DEB_VERSION:-1.0}"
REVISION="${JUICE_DEB_REVISION:-1}"

if [ ! -f "$JUICE_SRC/linux-Makefile" ]; then
    echo "error: $JUICE_SRC/linux-Makefile not found" >&2
    echo "usage: $0 [path-to-juice-firmware-extended]" >&2
    exit 1
fi

case "$(dpkg --print-architecture)" in
    amd64) arch=x86_64 ;;
    i386) arch=x86_32 ;;
    arm64) arch=aarch64 ;;
    armhf) arch=armhf ;;
    *) echo "error: unsupported Debian/Ubuntu architecture: $(dpkg --print-architecture)" >&2; exit 1 ;;
esac
deb_arch=$(dpkg --print-architecture)

echo "Building radioberry-juice from $JUICE_SRC..."
make -C "$JUICE_SRC" -f linux-Makefile -j"$(nproc)"

WORK_DIR=$(mktemp -d)
trap 'rm -rf -- "$WORK_DIR"' EXIT
PKG_ROOT="$WORK_DIR/pkgroot"
mkdir -p "$PKG_ROOT"

echo "Staging install layout via install-linux.sh --destdir..."
bash "$JUICE_SRC/install-linux.sh" \
    --source "$JUICE_SRC/dist/linux-$arch" \
    --destdir "$PKG_ROOT"

mkdir -p "$PKG_ROOT/DEBIAN"
cat > "$PKG_ROOT/DEBIAN/control" <<EOF
Package: radioberry-juice
Version: $VERSION-$REVISION
Section: hamradio
Priority: optional
Architecture: $deb_arch
Maintainer: PA3GSB <pa3gsb@gmail.com>
Depends: libc6
Description: USB-to-network bridge for the Radioberry SDR board
 Talks to a Radioberry 2.x board over USB (FTDI D2XX), loads its FPGA
 gateware, and exposes it over the network via the normal openHPSDR
 discovery/UDP protocol, same as any Metis/Hermes-family board --
 for use with hpsdr-rs, piHPSDR, or any other openHPSDR Protocol 1
 client. Includes the required FTDI D2XX runtime library and a udev
 rule that grants USB access to members of the 'radioberry' group and
 automatically releases the device from the kernel's ftdi_sio serial
 driver so this program can use it instead.
 .
 The bundled FTDI D2XX library is for use with the Radioberry's own
 genuine FTDI FT2232H USB interface chip only, per FTDI's own driver
 license terms (see /usr/share/doc/radioberry-juice/copyright).
EOF

# See this script's own top-of-file comment for why this exists and
# what it has to contain: FTDI's license permits redistribution only
# with its license notice kept intact, so that notice (extracted
# verbatim, not retyped) is republished here at the standard Debian
# path for a package's third-party license terms.
FTDI_HEADER="$JUICE_SRC/dist/linux-$arch/ftdi/ftd2xx.h"
mkdir -p "$PKG_ROOT/usr/share/doc/radioberry-juice"
{
    echo "This package is not affiliated with or endorsed by Future Technology"
    echo "Devices International (FTDI). It bundles FTDI's unmodified libftd2xx.so"
    echo "(D2XX) runtime library and header, redistributed under FTDI's own"
    echo "driver license terms, reproduced verbatim below from ftd2xx.h"
    echo "(also published at https://ftdichip.com/driver-licence-terms/)."
    echo "This library is for use with the Radioberry's genuine FTDI FT2232H"
    echo "USB interface chip only."
    echo
    awk '/^Copyright \(c\)/{p=1} p{print} /^Module Name:/{exit}' "$FTDI_HEADER"
} > "$PKG_ROOT/usr/share/doc/radioberry-juice/copyright"

# The staged install-linux.sh run above only lays out files at their
# real system paths -- it deliberately skips the group/udev-reload steps
# in --destdir mode (see its own --destdir doc comment), since those are
# live-system actions that don't belong in a package's data.tar. Do them
# here instead, in postinst, the normal place for a .deb to touch the
# running system rather than just install files.
cat > "$PKG_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/bash
set -e
getent group radioberry >/dev/null || groupadd radioberry
target_user="${SUDO_USER:-}"
if [ -n "$target_user" ] && [ "$target_user" != root ]; then
    if ! id -nG "$target_user" | grep -qw radioberry; then
        usermod -aG radioberry "$target_user"
        echo "Added $target_user to the 'radioberry' group -- log out and back in for it to take effect."
    fi
else
    echo "Add your login user to the 'radioberry' group manually: sudo usermod -aG radioberry \$USER"
fi
udevadm control --reload-rules 2>/dev/null || true
udevadm trigger --subsystem-match=usb 2>/dev/null || true
echo "radioberry-juice installed. Edit /home/pi/.radioberry/radioberry.props (fpga=CL016 or CL025), reconnect the Radioberry, then run: radioberry-juice"
EOF
chmod 755 "$PKG_ROOT/DEBIAN/postinst"

OUT_DIR="$JUICE_SRC/dist"
OUT_DEB="$OUT_DIR/radioberry-juice_${VERSION}-${REVISION}_${deb_arch}.deb"
dpkg-deb --build --root-owner-group "$PKG_ROOT" "$OUT_DEB"

echo
echo "Built: $OUT_DEB"
echo "Install with: sudo apt install '$OUT_DEB'"
