# Raspberry Pi 5 kiosk `.deb` (arm64, Debian 13 "trixie")

A self-contained package of hpsdr-rs in **1024x600 kiosk mode** for a Raspberry
Pi 5 running Debian 13 "trixie" (Raspberry Pi OS). It installs the binary, the
RADE (FreeDV neural voice) engine, a menu entry and a **desktop shortcut**, and
pulls every system library it needs through `apt`.

The package is **built on a PC, not on the Pi** (cross-compiled): a Pi 5 with
4 GB needs 30+ minutes to build this project, while a PC does it in about
3 minutes, and the Pi then needs no Rust toolchain, no build dependencies and
no access to the private `rade_c` submodule.

## What you get

| | |
|---|---|
| Package name | `hpsdr-rs-kiosk` (conflicts with / replaces / provides `hpsdr-rs`) |
| Architecture | `arm64` |
| Binary | `/usr/bin/hpsdr-rs` (one binary; kiosk mode is `HPSDR_LCD_1024X600=1`) |
| RADE | statically linked into the binary, model weights built in, **no extra files** |
| Launcher | `/usr/bin/hpsdr-rs-kiosk` (just sets `HPSDR_LCD_1024X600=1` and starts the app) and the menu entry `/usr/share/applications/hpsdr-rs-kiosk.desktop` |
| Desktop shortcut | copied to `~/Desktop` of every user that has one (postinst); `/etc/skel/Desktop` covers future users; removed on uninstall |
| Docs / firmware | manual, Ozy and RX-888 firmware, udev rule examples under `/usr/share/...` (same as the normal package) |

## Installing on the Pi

Copy the `.deb` to the Pi and install it (the `./` matters, it makes `apt`
treat the argument as a file and resolve its dependencies):

```bash
scp hpsdr-rs-kiosk_0.8.7-1_arm64.deb pi@<pi-address>:/tmp/
ssh pi@<pi-address> "sudo apt install -y /tmp/hpsdr-rs-kiosk_0.8.7-1_arm64.deb"
```

(Using `/tmp` avoids apt's harmless "couldn't be accessed by user `_apt`"
notice you get when installing from inside `/home`.)

Uninstall: `sudo apt remove hpsdr-rs-kiosk`.

## How it was built

Everything lives in the repository:

| File | Purpose |
|---|---|
| `Cargo.toml` -> `[package.metadata.deb.variants.kiosk]` | the package definition (name, dependencies, files) |
| `assets/deb-kiosk/hpsdr-rs-kiosk`, `assets/deb-kiosk/hpsdr-rs-kiosk.desktop` | the launcher script and the menu/desktop entry |
| `assets/deb-kiosk/postinst`, `postrm` | create / remove the desktop shortcut |
| `build_rade.rs` | cross-compile fix for the RADE/Opus build (see below) |
| `scripts/rpi5-kiosk-deb/1-setup-cross.sh` | one-time setup of the build machine |
| `scripts/rpi5-kiosk-deb/2-build-deb.sh` | cross-compile + `cargo deb` |
| `scripts/rpi5-kiosk-deb/3-verify-trixie.sh` | install + smoke test in a clean trixie arm64 chroot |

The build machine used was **WSL2 Ubuntu 24.04** on Windows (any Ubuntu 24.04
x86-64 works).

### 1. One-time setup (as root)

`1-setup-cross.sh` adds the `arm64` architecture and Ubuntu's `ports.ubuntu.com`
archive, installs `gcc-aarch64-linux-gnu`, cmake, autotools and the **arm64
development libraries** hpsdr-rs links against (`libfftw3-dev`,
`libasound2-dev`, X11/xkbcommon/GL/EGL headers, ...), then Rust, the
`aarch64-unknown-linux-gnu` target and `cargo-deb`.

Building against Ubuntu 24.04's arm64 libraries is fine for a trixie target:
the binary only needs glibc >= 2.39 (trixie has 2.41) and the library
sonames (`libasound.so.2`, `libfftw3.so.3`) are the same.

### 2. Build

Use a checkout **with submodules** (`vendor/rade_c` is a private repository;
if the machine has no GitHub credentials, copy the folder over from a machine
that has it). On WSL, copy the repo to the Linux filesystem (`/root/...`)
first, building under `/mnt/c` is much slower.

```bash
scripts/rpi5-kiosk-deb/2-build-deb.sh        # optional argument: deb revision
# -> target/aarch64-unknown-linux-gnu/debian/hpsdr-rs-kiosk_<version>-<rev>_arm64.deb
```

It sets the cross toolchain environment, runs
`cargo build --release --target aarch64-unknown-linux-gnu`, then
`cargo deb --target aarch64-unknown-linux-gnu --variant kiosk --no-build`.

Two things in the build needed fixing for cross-compiling, both already in the
repo:

* **`HPSDR_RS_NO_MARCH_NATIVE=1`**: `build.rs` passes `-march=native` to the C
  code, which would target the build machine's CPU rather than the Pi's.
* **Opus built for the wrong CPU** (`build_rade.rs`): RADE's Opus/FARGAN code
  is built by `rade_c`'s CMake, whose `BuildOpus.cmake` only passes `--host` to
  Opus's `./configure` from `CMAKE_C_COMPILER_TARGET` (a Clang-only variable).
  With GCC it was empty, so Opus was silently built for x86 and the final link
  failed with `skipping incompatible libopus.a`. The generated CMake wrapper
  now sets `CMAKE_C_COMPILER_TARGET=aarch64-linux-gnu` when cross-compiling to
  aarch64.

### 3. Dependencies

`cargo deb` fills `$auto` from the libraries the binary is linked against
(`libasound2t64`, `libfftw3-double3`, `libc6`, ...). The GUI stack loads several
libraries with `dlopen` at runtime, which `$auto` cannot see, so the variant
lists them by hand: `libxkbcommon-x11-0`, `libxkbcommon0`, `libgl1`, `libegl1`,
`libx11-6`, `libx11-xcb1`, `libxcb1`, `libxcursor1`, `libxrandr2`, `libxi6`,
`libxinerama1`, `libxrender1`, `libwayland-client0`, `libwayland-egl1`. The list
was produced by searching the binary for `lib*.so*` strings:

```bash
strings -a hpsdr-rs | grep -o 'lib[A-Za-z0-9_+-]*\.so[.0-9]*' | sort -u
```

`libasound2-plugins` is only a *recommendation* (it pulls in a large chain of
audio/video libraries and is not needed for ALSA on a Pi).

### 4. Verification

`3-verify-trixie.sh` creates a clean **Debian 13 arm64** root with `debootstrap`
(run through `qemu-user-static`) and checks, in order:

1. `apt install ./package.deb` resolves every dependency from trixie's own repos;
2. the desktop shortcut appears in `/home/pi/Desktop`, owned by `pi`, executable;
3. `ldd /usr/bin/hpsdr-rs` shows no missing library;
4. the app starts under a virtual 1024x600 X server in kiosk mode and stays
   alive (the main window is 1024x600 and "Discover HPSDR Radios" is 1000x580);
5. `apt remove` deletes the binary and the shortcut.

This was **not** run on real Pi hardware by the script (emulated CPU, software
OpenGL, no sound card, no radio), so audio, the real display and RADE decoding
with a signal still need a check on the device.

## Notes

* The kiosk layout is fixed at 1024x600 and renders 1:1 in physical pixels, so
  on a desktop monitor with display scaling it looks small. That is by design
  for the real panel.
* Release policy for this repository: only the no-AVX2 Windows `.exe` is
  published on GitHub Releases; this `.deb` is built locally and not uploaded.

## CPU cores on the Pi: do not use `isolcpus` for this app

A Pi set up for real-time SDR work often has `isolcpus=2,3` on its kernel
command line (`/boot/firmware/cmdline.txt`). The scheduler never puts a normal
program on isolated cores, so `nproc` reports 2 and the app shares cores 0-1
with the desktop and the VNC server. Measured on a Pi 5 with RADE running:
cores 0 and 1 at ~87% each, cores 2 and 3 idle.

**Pinning the app to the isolated cores does not fix it.** Running it with
`taskset -c 2,3 hpsdr-rs` moved the desktop's load away, but the kernel does
**not** load-balance between isolated cores: every thread of the app (UI,
spectrum/DSP, RADE, WDSP and the `cpal_alsa_in`/`cpal_alsa_out` audio threads,
checked with `ps -L -o tid,psr,pcpu,comm`) stayed on the one core it started
on, which sat at 100%. The result was audible clicks and dropouts in SSB, even
though the app's own "Audio glitches" counter and PipeWire's xrun counters
(`pw-top`) both stayed at zero, because the starved threads were upstream of
those checks. Moving the heaviest thread to the other isolated core by hand
(`taskset -cp 2 <tid>`) brought the worst core from 100% to 70%.

The fix is to remove the isolation and let the scheduler use all four cores:

```bash
sudo cp /boot/firmware/cmdline.txt /boot/firmware/cmdline.txt.bak
sudo sed -i 's/ isolcpus=2,3//' /boot/firmware/cmdline.txt
sudo reboot
```

(`nohz_full=` and `rcu_nocbs=` can stay; they are harmless without
`isolcpus`.) The `hpsdr-rs-kiosk` launcher therefore does no CPU pinning. If
you need the isolated cores for something else, pin *that* program instead.

## Line endings

Build from a Linux checkout (or a copy exported with
`git -c core.autocrlf=false archive`), not from a Windows working tree: with
`core.autocrlf=true` the text files there have CRLF line endings, and a
`.desktop` file with CRLF is rejected by `desktop-file-validate` and its `Exec`
line ends in a carriage return. `.gitattributes` forces LF for
`assets/deb-kiosk/*` and the packaging scripts. The Ozy firmware
`assets/ozy/ozyfw-sdr1k.hex` is stored with CRLF in git and ships that way.
