# CLASSIFICATION: COMMUNITY
# Filename: setup_build_env.sh v0.2
# Author: Lukas Bower
# Date Modified: 2026-02-13
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ARCH="$(uname -m)"

msg(){ printf "\e[32m==>\e[0m %s\n" "$*"; }
die(){ printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

pkgs=(gcc cmake ninja-build python3-venv python3-pip)
if [[ "$ARCH" == "aarch64" || "$ARCH" == "arm64" ]]; then
    pkgs+=(gcc-aarch64-linux-gnu)
fi

if command -v apt-get >/dev/null 2>&1; then
    add_cuda_repo() {
        local dist=$(lsb_release -cs)
        local arch=$(dpkg --print-architecture)
        local keyring=/etc/apt/keyrings/cuda-archive-keyring.gpg
        sudo mkdir -p /etc/apt/keyrings
        if [ ! -f "$keyring" ]; then
            curl -fsSL "https://developer.download.nvidia.com/compute/cuda/repos/${dist}/${arch}/cuda-archive-keyring.gpg" \
                | sudo gpg --dearmor -o "$keyring"
        fi
        echo "deb [signed-by=$keyring] https://developer.download.nvidia.com/compute/cuda/repos/${dist}/${arch}/ /" \
            | sudo tee /etc/apt/sources.list.d/cuda.list >/dev/null
    }

    msg "Installing build dependencies"
    add_cuda_repo
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
