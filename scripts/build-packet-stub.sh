#!/bin/sh
# Builds wix/packet-stub/packet.dll (the Npcap Packet.dll forwarding stub the MSI
# installs next to hpsdr-rs.exe) with MinGW-w64. Run from the repository root in an
# MSYS2 shell, or with C:\msys64\mingw64\bin on PATH.
set -e
cd "$(dirname "$0")/.."
gcc -shared -O2 -o wix/packet-stub/packet.dll wix/packet-stub/packet_stub.c wix/packet-stub/packet.def -Wl,--kill-at
gcc -shared -O2 -DSTUB_TEST_NO_REAL -o wix/packet-stub/packet_noreal_test.dll wix/packet-stub/packet_stub.c wix/packet-stub/packet.def -Wl,--kill-at
echo "built wix/packet-stub/packet.dll (and packet_noreal_test.dll, test-only)"
