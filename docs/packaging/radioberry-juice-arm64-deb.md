# radioberry-juice for arm64 (Raspberry Pi 5)

`radioberry-juice` is PA3GSB's USB-to-network bridge for the Radioberry 2.x
board (it talks to the board's FTDI FT2232H over USB with the FTDI D2XX library
and exposes it as a normal openHPSDR Protocol 1 radio, which hpsdr-rs then
discovers). It already builds as an amd64 `.deb` with
`scripts/build-juice-deb.sh`; this page records how the **arm64** package was
built and checked.

## Why a native (emulated) build, not a cross-compile

The juice sources already support aarch64 (`linux-Makefile` detects the
compiler's target and bundles FTDI's aarch64 `libftd2xx.so`). But its
`install-linux.sh`, which `build-juice-deb.sh` uses to stage the package, checks
the *host's* `dpkg --print-architecture` and the ELF machine of the binaries, so
it refuses to stage an arm64 build on an amd64 machine. Rather than modify the
author's scripts, the package is built **inside an arm64 Debian 13 "trixie"
chroot** (debootstrap + `qemu-user-static`, the same one used to verify the
kiosk package), where the host *is* arm64:

```bash
# one-time: build tools in the chroot
chroot /root/trixie apt-get install -y build-essential dpkg-dev binutils file
# copy the juice tree (without dist/, build/, *.exe) to /build/juice in the chroot, and
# scripts/build-juice-deb.sh to /build, then:
chroot /root/trixie /bin/sh -c 'cd /build && USER=pi JUICE_DEB_VERSION=1.0 \
    JUICE_DEB_REVISION=1 bash build-juice-deb.sh /build/juice'
# -> /build/juice/dist/radioberry-juice_1.0-1_arm64.deb
```

`USER=pi` is needed because the installer wants a non-root login name even for
staging (the package's data still goes to `/home/pi/.radioberry`, as on amd64).

## What was checked

* `Architecture: arm64`, `Depends: libc6`; the program and `libftd2xx.so` are
  ELF AArch64; needed glibc symbol versions are at most 2.34 (Debian 12/13,
  Ubuntu 22.04+).
* Installed with `apt install` in a **pristine** arm64 trixie chroot: the
  dependency resolved from the distribution alone, `ldd` found every library
  (the D2XX library through `$ORIGIN/lib`), and the program starts and prints its
  Radioberry V2.0 banner.
* Not checked: running against a real Radioberry board on a Raspberry Pi 5.

## Installing on the Pi

```bash
sudo apt install -y /tmp/radioberry-juice_1.0-1_arm64.deb
sudo usermod -aG radioberry $USER      # then log out and back in
nano ~/.radioberry/radioberry.props    # fpga=CL016 or CL025
radioberry-juice
```

The package installs a udev rule that grants the `radioberry` group access to
the FTDI device (0403:6010) and releases it from the kernel's `ftdi_sio` driver,
so reconnect the board after installing. FTDI's licence notice is shipped in
`/usr/share/doc/radioberry-juice/copyright` (the D2XX library is for use with the
Radioberry's genuine FTDI chip only).
