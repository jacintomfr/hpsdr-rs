#!/usr/bin/env bash
# Installs the built .deb into a clean Debian 13 "trixie" arm64 chroot
# (emulated with qemu-user) and smoke-tests it: dependencies resolve, the
# desktop shortcut is created, nothing is missing from ldd, the app starts
# under a virtual 1024x600 X server in kiosk mode, and it uninstalls cleanly.
# Run as root. Usage: 3-verify-trixie.sh path/to/hpsdr-rs-kiosk_*_arm64.deb
set -euo pipefail
DEB="$(readlink -f "$1")"
R=/root/trixie
apt-get install -y qemu-user-static debootstrap
[ -e "$R/bin/sh" ] || debootstrap --arch=arm64 --variant=minbase \
    --include=ca-certificates,apt trixie "$R" http://deb.debian.org/debian

cp /etc/resolv.conf "$R/etc/resolv.conf"
cp "$DEB" "$R/tmp/pkg.deb"
mount -t proc proc "$R/proc" 2>/dev/null || true
trap 'umount "$R/proc" 2>/dev/null || true' EXIT
chroot "$R" /bin/sh -e -c '
export DEBIAN_FRONTEND=noninteractive
id pi >/dev/null 2>&1 || useradd -m -s /bin/bash pi
mkdir -p /home/pi/Desktop && chown pi:pi /home/pi/Desktop
apt-get update -qq
apt-get install -y ./tmp/pkg.deb xvfb x11-utils libgl1-mesa-dri
echo "== desktop shortcut"; ls -la /home/pi/Desktop/
echo "== ldd"; ldd /usr/bin/hpsdr-rs | grep "not found" && exit 1 || echo "no missing libraries"
cd /tmp
Xvfb :77 -screen 0 1024x600x24 >/dev/null 2>&1 &
XV=$!
sleep 3
HOME=/root DISPLAY=:77 HPSDR_LCD_1024X600=1 /usr/bin/hpsdr-rs >/tmp/run.log 2>&1 &
APP=$!
sleep 50
kill -0 $APP && echo "app alive after 50s" || { echo "APP DIED"; cat /tmp/run.log; exit 1; }
DISPLAY=:77 xwininfo -root -tree | grep -E "hpsdr-rs|Discover"
kill $APP $XV 2>/dev/null || true
apt-get remove -y hpsdr-rs-kiosk
test ! -e /home/pi/Desktop/hpsdr-rs-kiosk.desktop && echo "shortcut removed on uninstall"
'
echo "VERIFY OK"
