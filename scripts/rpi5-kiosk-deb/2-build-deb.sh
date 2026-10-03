#!/usr/bin/env bash
# Cross-compiles hpsdr-rs for aarch64 and builds the kiosk .deb.
# Run from a checkout WITH its submodules (vendor/rade_c, vendor/rnnoise),
# ideally on the Linux filesystem (on WSL: copy the repo under /root, not
# /mnt/c -- builds are many times faster). Usage:
#   scripts/rpi5-kiosk-deb/2-build-deb.sh [deb-revision]
set -euo pipefail
. "$HOME/.cargo/env"
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
REV="${1:-1}"

export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig
export PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig
# build.rs adds -march=native for the C code: that would be the BUILD
# machine's CPU, not the Pi's.
export HPSDR_RS_NO_MARCH_NATIVE=1

cargo build --release --target aarch64-unknown-linux-gnu
cargo deb --target aarch64-unknown-linux-gnu --variant kiosk --no-build --deb-revision "$REV"
ls -la target/aarch64-unknown-linux-gnu/debian/
