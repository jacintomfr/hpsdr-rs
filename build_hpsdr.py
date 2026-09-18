#!/usr/bin/env python3
"""
build_hpsdr.py - Compila o hpsdr-rs para Windows via MinGW-w64.

Corre isto depois do setup_env.py já ter instalado o MSYS2/MinGW-w64,
o rustup e o target x86_64-pc-windows-gnu.

Uso (a partir da pasta do projecto -- a que tem o Cargo.toml):
    cd C:\\caminho\\para\\o\\teu\\hpsdr-rs
    python build_hpsdr.py

Ou, indicando a pasta explicitamente (corre de onde quiseres):
    python build_hpsdr.py --project-dir "C:\\caminho\\para\\hpsdr-rs" --msys2-dir "D:\\msys64"

Por omissão compila em modo release (otimizado). Usa --debug para um
build de debug (mais rápido a compilar, binário maior e mais lento).
"""
import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

TARGET = "x86_64-pc-windows-gnu"

# Onde o rustup-init instala o toolchain por omissão. Numa consola que
# esteve aberta desde antes da instalação do Rust, o PATH dela ainda não
# inclui esta pasta -- por isso vamos à procura do cargo aqui também, em
# vez de assumir que "cargo" já está no PATH.
CARGO_BIN = Path.home() / ".cargo" / "bin"


def find_cargo() -> str:
    found = shutil.which("cargo")
    if found:
        return found
    candidate = CARGO_BIN / "cargo.exe"
    if candidate.exists():
        return str(candidate)
    sys.exit(
        "Não encontro o cargo (nem no PATH, nem em "
        f"{candidate}).\nCorre primeiro o setup_env.py, "
        "ou fecha e reabre a consola e tenta outra vez."
    )


# Onde a maior parte das pessoas extrai o Npcap SDK, se seguirem a sugestão
# mais óbvia. Ajustável via --npcap-sdk-dir se tiveres posto noutro sítio.
DEFAULT_NPCAP_SDK_DIR = r"C:\npcap-sdk"


def main() -> None:
    if sys.platform != "win32":
        sys.exit("Este script destina-se a correr no Windows.")

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--msys2-dir", default=r"C:\msys64",
                         help=r"Onde está instalado o MSYS2 (default: C:\msys64)")
    # ROOT CAUSE FIX: isto costumava ter um caminho fixo (a primeira pasta
    # onde o projecto tinha sido descarregado) como valor por omissão --
    # o que significava que, depois de passar a trabalhar noutra cópia
    # (ex: um clone Git à parte, feito para acompanhar o repositório
    # original), correr "python build_hpsdr.py" sem argumentos continuava,
    # silenciosamente, a compilar a pasta antiga, mesmo estando dentro da
    # nova. O valor por omissão passa a ser a PASTA ACTUAL (onde o script é
    # corrido) -- corres de dentro do projecto que queres compilar, tal
    # como cargo/git/etc. já fazem, e não precisas de --project-dir de
    # todo no caso normal.
    parser.add_argument("--project-dir", default=os.getcwd(),
                         help="Pasta do projecto hpsdr-rs (default: a pasta actual)")
    parser.add_argument("--debug", action="store_true",
                         help="Compilar em modo debug em vez de release")
    parser.add_argument("--npcap-sdk-dir", default=DEFAULT_NPCAP_SDK_DIR,
                         help=f"Onde extraíste o Npcap SDK (default: {DEFAULT_NPCAP_SDK_DIR})")
    args = parser.parse_args()

    project_dir = Path(args.project_dir)
    if not (project_dir / "Cargo.toml").exists():
        sys.exit(
            f"Não encontrei um Cargo.toml em {project_dir}.\n"
            "Corre este script de dentro da pasta do projecto (a que tem o "
            "Cargo.toml), ou usa --project-dir para apontar para lá."
        )

    mingw_bin = Path(args.msys2_dir) / "mingw64" / "bin"
    if not mingw_bin.exists():
        sys.exit(
            f"Não encontrei {mingw_bin}.\n"
            f"Corre primeiro o setup_env.py (ou confirma --msys2-dir)."
        )

    cargo_exe = find_cargo()

    # Garante que o gcc/pkg-config do MinGW-w64 e o próprio cargo são
    # encontrados, mesmo que o PATH desta consola ainda não tenha sido
    # actualizado (ex: consola aberta antes de o Rust ser instalado).
    env = os.environ.copy()
    env["PATH"] = (
        str(mingw_bin) + os.pathsep + str(CARGO_BIN) + os.pathsep + env.get("PATH", "")
    )

    # NPCAP_SDK_DIR: build.rs procura Lib\x64\Packet.lib aqui dentro para
    # a funcionalidade de upload de firmware do Ozy (pnet_datalink). Sem
    # isto o link falha com "cannot find -lPacket". Só definimos a
    # variável se a pasta existir de facto -- caso contrário deixamos o
    # cargo falhar com a mensagem de erro normal, para não mascarar um
    # --npcap-sdk-dir errado com um caminho que não existe.
    npcap_dir = Path(args.npcap_sdk_dir)
    if npcap_dir.exists():
        env["NPCAP_SDK_DIR"] = str(npcap_dir)
        print(f"NPCAP_SDK_DIR={npcap_dir}")
    else:
        print(
            f"Aviso: {npcap_dir} não existe -- se o link falhar com "
            "'cannot find -lPacket', descarrega o Npcap SDK em "
            "https://npcap.com/#download, extrai-o para essa pasta "
            "(ou usa --npcap-sdk-dir) e corre outra vez."
        )

    release = not args.debug
    cmd = [cargo_exe, "build", "--target", TARGET]
    if release:
        cmd.append("--release")

    print(f"A compilar em {project_dir} ...")
    print(f"$ {' '.join(cmd)}")
    result = subprocess.run(cmd, cwd=project_dir, env=env)
    if result.returncode != 0:
        sys.exit(result.returncode)

    exe_dir = "release" if release else "debug"
    exe_path = project_dir / "target" / TARGET / exe_dir / "hpsdr-rs.exe"
    print("\n" + "=" * 60)
    if exe_path.exists():
        print(f"Compilação concluída: {exe_path}")
    else:
        print("Cargo terminou sem erro, mas não encontrei o .exe no sítio esperado:")
        print(f"  {exe_path}")
    print("=" * 60)


if __name__ == "__main__":
    main()
