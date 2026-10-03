#!/usr/bin/env bash
# One-time setup of an Ubuntu 24.04 (or WSL2 Ubuntu 24.04) machine for
# cross-compiling hpsdr-rs to Linux/arm64 and packaging it as a .deb for a
# Raspberry Pi 5 running Debian 13 "trixie". Run as root.
# See docs/packaging/rpi5-kiosk-deb.md for the full explanation.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

dpkg --add-architecture arm64
# Keep the stock sources amd64-only and add Ubuntu's arm64 "ports" archive.
f=/etc/apt/sources.list.d/ubuntu.sources
if [ -f "$f" ] && ! grep -q "Architectures: amd64" "$f"; then
    sed -i '/^Types: deb/a Architectures: amd64' "$f"
fi
cat > /etc/apt/sources.list.d/arm64-ports.sources <<'EOS'
Types: deb
URIs: http://ports.ubuntu.com/ubuntu-ports
Suites: noble noble-updates noble-security
Components: main universe
Architectures: arm64
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOS
apt-get update -qq
apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu \
    pkg-config cmake make dpkg-dev libclang-dev git curl autoconf automake libtool \
    libfftw3-dev:arm64 libasound2-dev:arm64 libx11-dev:arm64 libxkbcommon-dev:arm64 \
    libudev-dev:arm64 libgl-dev:arm64 libegl-dev:arm64 libssl-dev:arm64

# Rust (skip if already installed) + the arm64 target + cargo-deb.
if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
. "$HOME/.cargo/env"
rustup target add aarch64-unknown-linux-gnu
cargo install cargo-deb --locked
echo "Setup done."
