#!/usr/bin/env bash
# Builds PA3GSB's radioberry-juice (the USB<->network bridge for the
# Radioberry) for Linux, from its own separate source tree -- juice isn't
# part of this repo, it's a standalone C project this project's author
# also maintains a local checkout of. This script just wraps its
# `linux-Makefile` so the WSL2 test flow (build here, run under WSL2 with
# the Radioberry's USB passed through via usbipd-win, verify the P1
# discovery reply) is a one-liner instead of a many-step manual recipe.
#
# Confirmed working end-to-end on WSL2 Ubuntu: `make -f linux-Makefile`
# links against the bundled FTDI D2XX 1.4.35 library (no separate FTDI
# download needed), `install-linux.sh` sets up the udev rule + `radioberry`
# group, and the resulting binary answers real HPSDR Protocol 1 discovery
# packets identically to the Windows build.
set -euo pipefail

JUICE_SRC="${1:-$HOME/Documents/juice/firmware-extended}"

if [ ! -f "$JUICE_SRC/linux-Makefile" ]; then
    echo "error: $JUICE_SRC/linux-Makefile not found" >&2
    echo "usage: $0 [path-to-juice-firmware-extended]" >&2
    exit 1
fi

echo "Building radioberry-juice from $JUICE_SRC..."
make -C "$JUICE_SRC" -f linux-Makefile -j"$(nproc)"

ARCH=$(gcc -dumpmachine | grep -oE '^(x86_64|aarch64|arm|i386)' | sed 's/i386/x86_32/;s/^arm$/armhf/')
DIST_DIR="$JUICE_SRC/dist/linux-$ARCH"

echo
echo "Built: $DIST_DIR/radioberry-juice"
echo
echo "To install (creates udev rule + 'radioberry' group, needs sudo):"
echo "  cd '$DIST_DIR' && sudo bash install-linux.sh"
echo
echo "Inside WSL2, the Radioberry's USB device needs passthrough first"
echo "(run as Administrator in Windows PowerShell, once per boot):"
echo "  usbipd bind --busid <busid> --force   # find <busid> via 'usbipd list'"
echo "  usbipd attach --wsl --busid <busid>"
