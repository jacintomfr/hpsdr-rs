#!/usr/bin/env python3
"""
update_hpsdr.py

Automatiza a atualizacao do fork jacintomfr/hpsdr-rs a partir do
repositorio original g0orx/hpsdr-rs, preservando as modificacoes
pessoais, e depois compila o projeto correndo build_hpsdr.py.

Passos:
  1. git fetch upstream
  2. git merge upstream/master
     - Se houver conflitos, o script para e informa quais ficheiros
       resolver manualmente (nao faz merge automatico "as cegas").
  3. git push origin master
  4. Corre build_hpsdr.py (o teu script de compilacao existente)

Uso:
  python update_hpsdr.py
  python update_hpsdr.py --no-push       (nao envia para o GitHub)
  python update_hpsdr.py --no-build      (nao compila no final)
"""

import subprocess
import sys
import argparse
from pathlib import Path

REPO_DIR = Path(__file__).resolve().parent
BUILD_SCRIPT = REPO_DIR / "build_hpsdr.py"
BRANCH = "master"


def run(cmd, check=True):
    """Corre um comando e mostra o output em tempo real."""
    print(f"\n$ {' '.join(cmd)}")
    result = subprocess.run(cmd, cwd=REPO_DIR, text=True)
    if check and result.returncode != 0:
        print(f"\n[ERRO] Comando falhou (codigo {result.returncode}): {' '.join(cmd)}")
        sys.exit(result.returncode)
    return result.returncode


def git_output(cmd):
    """Corre um comando git e devolve o output como texto (sem mostrar)."""
    result = subprocess.run(cmd, cwd=REPO_DIR, text=True, capture_output=True)
    return result.stdout.strip()


def has_uncommitted_changes():
    status = git_output(["git", "status", "--porcelain"])
    return bool(status)


def main():
    parser = argparse.ArgumentParser(description="Atualiza o fork hpsdr-rs e compila.")
    parser.add_argument("--no-push", action="store_true", help="Nao faz git push no final")
    parser.add_argument("--no-build", action="store_true", help="Nao corre o build_hpsdr.py no final")
    args = parser.parse_args()

    print("=" * 60)
    print("Atualizacao automatica: hpsdr-rs (fork <- upstream)")
    print("=" * 60)

    if not (REPO_DIR / ".git").exists():
        print(f"[ERRO] {REPO_DIR} nao parece ser um repositorio git.")
        sys.exit(1)

    # 0. Verificar se ha alteracoes locais por gravar
    if has_uncommitted_changes():
        print("\n[AVISO] Tens alteracoes locais nao commitadas nesta pasta.")
        print("Faz commit ou stash antes de continuar, para evitar perdas.")
        resposta = input("Continuar mesmo assim? (s/N): ").strip().lower()
        if resposta != "s":
            print("Cancelado pelo utilizador.")
            sys.exit(0)

    # 1. Garantir que estamos na branch certa
    current_branch = git_output(["git", "branch", "--show-current"])
    if current_branch != BRANCH:
        print(f"\n[INFO] Estavas na branch '{current_branch}', a mudar para '{BRANCH}'...")
        run(["git", "checkout", BRANCH])

    # 2. Fetch do upstream
    run(["git", "fetch", "upstream"])

    # 3. Merge
    print(f"\n[INFO] A fazer merge de upstream/{BRANCH} para {BRANCH}...")
    merge_rc = run(["git", "merge", f"upstream/{BRANCH}", "--no-edit"], check=False)

    if merge_rc != 0:
        # Verificar se e um conflito real
        status = git_output(["git", "status", "--porcelain"])
        conflitos = [l for l in status.splitlines() if l.startswith("UU") or l.startswith("AA") or l.startswith("DD")]
        if conflitos:
            print("\n" + "=" * 60)
            print("[CONFLITO] Ha ficheiros em conflito que precisam de resolucao manual:")
            for c in conflitos:
                print(f"   - {c}")
            print("\nAbre esses ficheiros, resolve os marcadores <<<<<<< / ======= / >>>>>>>,")
            print("depois faz:")
            print("   git add <ficheiro>")
            print("   git commit")
            print("e volta a correr este script (ou so o push/build manualmente).")
            print("=" * 60)
            sys.exit(1)
        else:
            print("\n[ERRO] O merge falhou por outro motivo. Verifica o output acima.")
            sys.exit(1)

    print("\n[OK] Merge concluido sem conflitos.")

    # 4. Push
    if not args.no_push:
        run(["git", "push", "origin", BRANCH])
        print("\n[OK] Push feito para origin/" + BRANCH)
    else:
        print("\n[INFO] --no-push indicado, a saltar o push.")

    # 5. Build
    if not args.no_build:
        if BUILD_SCRIPT.exists():
            print(f"\n[INFO] A correr {BUILD_SCRIPT.name}...")
            run([sys.executable, str(BUILD_SCRIPT)])
        else:
            print(f"\n[AVISO] Nao encontrei {BUILD_SCRIPT}, salta-se a compilacao.")
    else:
        print("\n[INFO] --no-build indicado, a saltar a compilacao.")

    print("\n" + "=" * 60)
    print("Tudo concluido com sucesso.")
    print("=" * 60)


if __name__ == "__main__":
    main()
