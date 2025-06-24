# CLASSIFICATION: COMMUNITY
# Filename: setup_build_env.sh v0.5
# Author: Lukas Bower
# Date Modified: 2026-07-25
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ARCH="$(uname -m)"
# Load or prompt for persistent architecture configuration
if [ -f "$ROOT/scripts/load_arch_config.sh" ]; then
    source "$ROOT/scripts/load_arch_config.sh" --prompt
else
    echo "⚠️  load_arch_config.sh not found. Skipping architecture config."
fi

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
        local numeric_dist
        case "$dist" in
            noble) numeric_dist="ubuntu2404" ;;
            jammy) numeric_dist="ubuntu2204" ;;
            focal) numeric_dist="ubuntu2004" ;;
            bionic) numeric_dist="ubuntu1804" ;;
            *) numeric_dist="$dist" ;;
        esac
        local narch="$arch"
        if [[ "$arch" == "amd64" ]]; then
            narch="x86_64"
        fi
        local keyfile=/usr/share/keyrings/nvidia-cuda-keyring.gpg
        sudo mkdir -p /usr/share/keyrings
        if [ ! -f "$keyfile" ]; then
            curl -fsSL "https://developer.download.nvidia.com/compute/cuda/repos/${numeric_dist}/${narch}/3bf863cc.pub" \
                | gpg --dearmor | sudo tee "$keyfile" >/dev/null
        fi
        echo "deb [signed-by=$keyfile] https://developer.download.nvidia.com/compute/cuda/repos/${numeric_dist}/${narch}/ /" \
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

# Ensure ~/.cohesix_config exists
CONFIG_FILE="$HOME/.cohesix_config"
if [ ! -f "$CONFIG_FILE" ]; then
    echo "🔧 Creating default Cohesix config at $CONFIG_FILE"
    cat > "$CONFIG_FILE" <<EOF
# Cohesix Architecture Configuration
COHESIX_ARCH=$(uname -m)
EOF
fi

echo "✅ Build environment setup complete."
