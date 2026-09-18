#!/usr/bin/env python3
"""
update-github.py - Grava e envia as TUAS proprias alteracoes desta
pasta (hpsdr-rs-git) para o teu fork no GitHub (origin).

Isto NAO traz as novidades do g0orx (upstream) -- isso e uma operacao
a parte (git fetch upstream + git rebase/merge upstream/master), que
vale a pena fazer com calma porque pode gerar conflitos nos ficheiros
que ja personalizaste (Settings/Add Receiver, waterfall, RX Gain,
etc. ja tiveram conflitos reais). Este script serve so para: "fiz
mudancas nesta pasta, quero-as no meu GitHub" -- nada mais.

Uso:
    python update-github.py
    python update-github.py -m "mensagem do commit"
    python update-github.py --no-push      (so grava local, nao envia)
"""
import argparse
import subprocess
import sys
from pathlib import Path

REPO_DIR = Path(__file__).resolve().parent
BRANCH = "master"


def run(cmd, check=True):
    print(f"$ {' '.join(cmd)}")
    result = subprocess.run(cmd, cwd=REPO_DIR, text=True)
    if check and result.returncode != 0:
        sys.exit(f"[ERRO] Falhou: {' '.join(cmd)} (codigo {result.returncode})")
    return result.returncode


def git_output(cmd):
    return subprocess.run(cmd, cwd=REPO_DIR, text=True, capture_output=True).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("-m", "--message", help="Mensagem do commit (se omitida, e pedida no momento)")
    parser.add_argument("--no-push", action="store_true", help="So faz commit local, nao envia para o GitHub")
    args = parser.parse_args()

    if not (REPO_DIR / ".git").exists():
        sys.exit(f"[ERRO] {REPO_DIR} nao parece ser um repositorio git.")

    status = git_output(["git", "status", "--porcelain"])

    if not status:
        print("Nada por gravar -- a pasta ja esta identica ao ultimo commit.")
        if not args.no_push:
            # Pode haver commits ja feitos localmente que ainda nao
            # chegaram ao GitHub (ex: correste isto so com --no-push da
            # ultima vez) -- tenta enviar-los na mesma.
            run(["git", "push", "origin", BRANCH])
        return

    print("Alteracoes detectadas nesta pasta:\n")
    print(git_output(["git", "status", "-s"]))
    print()

    message = args.message
    if not message:
        message = input("Mensagem do commit (descreve o que mudou): ").strip()
        if not message:
            sys.exit("Cancelado -- e preciso uma mensagem de commit.")

    run(["git", "add", "-A"])
    run(["git", "commit", "-m", message])

    if not args.no_push:
        run(["git", "push", "origin", BRANCH])
        print("\nEnviado para o teu GitHub.")
    else:
        print("\n--no-push indicado -- gravado localmente, mas nao enviado ainda.")
        print("Corre sem --no-push mais tarde para enviar este e quaisquer outros commits pendentes.")


if __name__ == "__main__":
    main()
