#!/usr/bin/env python3
"""
setup_env.py - Prepara um Windows para compilar o hpsdr-rs via MSYS2/MinGW-w64.

Idempotente: verifica o que já está instalado e salta esses passos.

O que faz:
  1. Localiza o MSYS2 já instalado, ou instala-o (silenciosamente)
  2. Actualiza a base de dados de pacotes do MSYS2
  3. Instala os pacotes MinGW-w64 em falta:
       mingw-w64-x86_64-toolchain
       mingw-w64-x86_64-fftw
       mingw-w64-x86_64-pkg-config
  4. Localiza o rustup já instalado, ou instala-o
  5. Garante que a toolchain default é a variante GNU (stable-x86_64-pc-windows-gnu),
     não a msvc -- assim nunca é preciso instalar o Visual Studio Build Tools,
     nem para o binário final nem para compilar build scripts/proc-macros

Corre a partir de uma consola normal do Windows (cmd ou PowerShell),
NÃO de dentro de uma shell do MSYS2. É preciso ligação à internet.
Se o MSYS2 ainda não estiver instalado, pode ser preciso correr como
Administrador (depende da pasta de destino escolhida).

Uso:
    python setup_env.py
    python setup_env.py --msys2-dir "C:\\msys64"
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path
from typing import Optional, Set

# Instalador do MSYS2, versão fixa e conhecida (evita depender de um nome
# de ficheiro "latest" que pode mudar de formato entre releases).
MSYS2_INSTALLER_URL = (
    "https://github.com/msys2/msys2-installer/releases/download/"
    "2024-01-13/msys2-x86_64-20240113.exe"
)

RUSTUP_INIT_URL = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"

REQUIRED_PACKAGES = [
    "mingw-w64-x86_64-toolchain",
    "mingw-w64-x86_64-fftw",
    "mingw-w64-x86_64-pkg-config",
]

RUST_TARGET = "x86_64-pc-windows-gnu"


def run(cmd, **kwargs):
    print(f"$ {' '.join(str(c) for c in cmd)}")
    return subprocess.run(cmd, check=True, **kwargs)


def find_msys2(preferred_dir: Path) -> Optional[Path]:
    for candidate in (preferred_dir, Path(r"C:\msys64"), Path(r"C:\msys2")):
        if (candidate / "usr" / "bin" / "pacman.exe").exists():
            return candidate
    return None


def install_msys2(target_dir: Path) -> None:
    print(f"MSYS2 não encontrado. A instalar em {target_dir} ...")
    with tempfile.TemporaryDirectory() as tmp:
        installer = Path(tmp) / "msys2-installer.exe"
        print("A transferir o instalador do MSYS2 ...")
        urllib.request.urlretrieve(MSYS2_INSTALLER_URL, installer)
        run([
            str(installer), "install",
            "--root", str(target_dir),
            "--confirm-command",
        ])
    print("MSYS2 instalado.")


def msys2_installed_packages(bash_exe: Path) -> Set[str]:
    result = subprocess.run(
        [str(bash_exe), "-lc", "pacman -Qq"],
        capture_output=True, text=True, check=True,
    )
    return set(result.stdout.split())


def ensure_packages(msys2_dir: Path) -> None:
    bash_exe = msys2_dir / "usr" / "bin" / "bash.exe"

    print("A actualizar a base de dados de pacotes do MSYS2 ...")
    subprocess.run([str(bash_exe), "-lc", "pacman -Sy --noconfirm"], check=True)

    installed = msys2_installed_packages(bash_exe)
    missing = [p for p in REQUIRED_PACKAGES if p not in installed]

    if not missing:
        print("Todos os pacotes MinGW-w64 necessários já estão instalados. A saltar.")
        return

    print(f"A instalar pacotes em falta: {', '.join(missing)}")
    subprocess.run(
        [str(bash_exe), "-lc", f"pacman -S --noconfirm {' '.join(missing)}"],
        check=True,
    )


# Onde o rustup-init instala o toolchain por omissão. O instalador acrescenta
# esta pasta ao PATH do Windows via registo, mas isso não chega a este
# processo Python já em execução — por isso é preciso lidar com o PATH
# manualmente já a seguir a instalar.
CARGO_BIN = Path.home() / ".cargo" / "bin"


def find_rustup() -> Optional[str]:
    found = shutil.which("rustup")
    if found:
        return found
    candidate = CARGO_BIN / "rustup.exe"
    if candidate.exists():
        return str(candidate)
    return None


def install_rustup() -> None:
    print("rustup não encontrado. A instalar o Rust ...")
    with tempfile.TemporaryDirectory() as tmp:
        installer = Path(tmp) / "rustup-init.exe"
        urllib.request.urlretrieve(RUSTUP_INIT_URL, installer)
        # --default-host gnu: instala logo a toolchain GNU como default,
        # em vez da msvc (a normal por omissão no Windows). Isto evita por
        # completo precisar do Visual Studio Build Tools -- tudo, incluindo
        # build scripts e proc-macros (que compilam sempre para o "host",
        # nunca para o --target pedido no cargo build), passa a usar o gcc
        # do MSYS2 em vez do link.exe da MSVC.
        run([
            str(installer), "-y",
            "--default-toolchain", "stable",
            "--default-host", RUST_TARGET,
        ])
    # Torna o cargo/rustup logo utilizáveis NESTE processo, sem esperar por
    # uma consola nova (o instalador só actualiza o PATH do Windows, não o
    # deste processo Python que já estava a correr).
    os.environ["PATH"] = str(CARGO_BIN) + os.pathsep + os.environ.get("PATH", "")
    print(f"Rust instalado em {CARGO_BIN} (host GNU por omissão).")


def ensure_gnu_host_toolchain(rustup_exe: str) -> None:
    """Garante que a toolchain default é a variante GNU (stable-x86_64-pc-windows-gnu).

    Isto importa mesmo quando o rustup já estava instalado antes (ex: com a
    toolchain msvc por omissão, como o rustup-init costuma escolher no
    Windows) -- nesse caso 'rustup target add' sozinho NÃO chega, porque
    build scripts e proc-macros continuam a ser compilados para o host
    (msvc), exigindo o linker do Visual Studio Build Tools mesmo que o
    binário final seja compilado --target x86_64-pc-windows-gnu.
    """
    active = subprocess.run(
        [rustup_exe, "show", "active-toolchain"],
        capture_output=True, text=True, check=True,
    ).stdout

    if RUST_TARGET in active:
        print(f"Toolchain stable-{RUST_TARGET} já é o default. A saltar.")
        return

    toolchains = subprocess.run(
        [rustup_exe, "toolchain", "list"],
        capture_output=True, text=True, check=True,
    ).stdout

    if f"stable-{RUST_TARGET}" not in toolchains:
        print(f"A instalar a toolchain stable-{RUST_TARGET} ...")
        run([rustup_exe, "toolchain", "install", f"stable-{RUST_TARGET}"])

    print(f"A definir stable-{RUST_TARGET} como toolchain default ...")
    run([rustup_exe, "default", f"stable-{RUST_TARGET}"])


def main() -> None:
    if sys.platform != "win32":
        sys.exit("Este script destina-se a correr no Windows.")

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--msys2-dir", default=r"C:\msys64",
                         help=r"Pasta onde o MSYS2 está ou deve ficar instalado (default: C:\msys64)")
    args = parser.parse_args()
    preferred_dir = Path(args.msys2_dir)

    print("== 1/3: MSYS2 + MinGW-w64 ==")
    msys2_dir = find_msys2(preferred_dir)
    if msys2_dir is None:
        install_msys2(preferred_dir)
        msys2_dir = preferred_dir
    else:
        print(f"MSYS2 já instalado em {msys2_dir}. A saltar instalação.")
    ensure_packages(msys2_dir)

    print("\n== 2/3: Rust (rustup) ==")
    rustup_exe = find_rustup()
    if rustup_exe is None:
        install_rustup()
        rustup_exe = find_rustup()
    else:
        print("rustup já está instalado. A saltar instalação.")
    if rustup_exe is None:
        sys.exit(
            "Instalei o rustup mas não o encontro a seguir.\n"
            "Fecha esta consola, abre uma nova (para o PATH actualizar) e volta a correr este script."
        )

    print("\n== 3/3: Toolchain GNU (evita precisar do Visual Studio) ==")
    ensure_gnu_host_toolchain(rustup_exe)

    mingw_bin = msys2_dir / "mingw64" / "bin"
    print("\n" + "=" * 60)
    print("Tudo pronto.")
    print(f"IMPORTANTE: o gcc/pkg-config do MinGW-w64 têm de estar no PATH")
    print(f"para o cargo os encontrar. Essa pasta é:\n  {mingw_bin}")
    print("O build_hpsdr.py já trata disto automaticamente por ti.")
    print("=" * 60)


if __name__ == "__main__":
    main()
