// CLASSIFICATION: COMMUNITY
// Filename: setup_build_env.sh v0.1
// Author: Lukas Bower
// Date Modified: 2026-02-12
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ARCH="$(uname -m)"

msg(){ printf "\e[32m==>\e[0m %s\n" "$*"; }
die(){ printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

pkgs=(gcc cmake ninja-build python3-venv python3-pip)
if [[ "$ARCH" == "aarch64" || "$ARCH" == "arm64" ]]; then
    pkgs+=(aarch64-linux-gnu-gcc)
fi

if command -v apt-get >/dev/null 2>&1; then
    msg "Installing build dependencies"
    sudo apt-get update -y >/dev/null
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y ${pkgs[*]} >/dev/null
fi

VENV="$ROOT/.venv"
if [ -d "$VENV" ]; then
    rm -rf "$VENV"
fi

python3 -m venv "$VENV"
source "$VENV/bin/activate"
python3 -m pip install --upgrade pip >/dev/null
python3 -m pip install jinja2 ply pyyaml >/dev/null

deactivate
