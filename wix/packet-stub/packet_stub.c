/*
 * packet.dll forwarding stub for the MinGW build of hpsdr-rs.
 *
 * The MinGW-built hpsdr-rs.exe links Npcap's Packet.dll (for the firmware-update
 * feature and network-interface enumeration) as a normal import, and MinGW has no
 * delay-load. On a clean Windows PC without Npcap the executable then refuses to
 * start at all (STATUS_DLL_NOT_FOUND). This stub is installed next to the
 * executable under the same name: it loads the real Packet.dll (Npcap or WinPcap,
 * from System32) when there is one and forwards every call to it, and otherwise
 * makes the 11 functions return 0/NULL so the application starts normally and
 * only the features that need Npcap report that it is missing.
 *
 * Build: scripts/build-packet-stub.sh
 */
#include <windows.h>
#include <string.h>

#define N_FUNCS 11
FARPROC real_tbl[N_FUNCS];

static const char *names[N_FUNCS] = {
    "PacketOpenAdapter",  "PacketSendPacket",  "PacketAllocatePacket", "PacketInitPacket",
    "PacketFreePacket",   "PacketReceivePacket", "PacketCloseAdapter", "PacketSetHwFilter",
    "PacketGetAdapterNames", "PacketSetBuff",  "PacketSetMinToCopy",
};

static ULONG_PTR fail_stub(void) { return 0; }

BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, LPVOID reserved) {
    (void)inst;
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        HMODULE h = NULL;
#ifndef STUB_TEST_NO_REAL
        char path[MAX_PATH];
        UINT n = GetSystemDirectoryA(path, MAX_PATH);
        const char *cands[] = {"\\Npcap\\Packet.dll", "\\Packet.dll"};
        for (int c = 0; c < 2 && !h && n > 0 && n < MAX_PATH - 32; c++) {
            strcpy(path + n, cands[c]);
            h = LoadLibraryExA(path, NULL, LOAD_WITH_ALTERED_SEARCH_PATH);
        }
#endif
        for (int i = 0; i < N_FUNCS; i++) {
            FARPROC p = h ? GetProcAddress(h, names[i]) : NULL;
            real_tbl[i] = p ? p : (FARPROC)fail_stub;
        }
    }
    return TRUE;
}

#define TRAMP(i, name)                                   \
    __asm__(".text\n.globl " #name "\n" #name ":\n"      \
            "\tjmp *real_tbl+" #i "*8(%rip)\n");

TRAMP(0, PacketOpenAdapter)
TRAMP(1, PacketSendPacket)
TRAMP(2, PacketAllocatePacket)
TRAMP(3, PacketInitPacket)
TRAMP(4, PacketFreePacket)
TRAMP(5, PacketReceivePacket)
TRAMP(6, PacketCloseAdapter)
TRAMP(7, PacketSetHwFilter)
TRAMP(8, PacketGetAdapterNames)
TRAMP(9, PacketSetBuff)
TRAMP(10, PacketSetMinToCopy)
