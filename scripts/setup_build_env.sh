#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: setup_build_env.sh v0.8
# Author: Lukas Bower
# Date Modified: 2030-07-06
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ARCH="$(uname -m)"
OS_NAME="$(uname -s)"

if [[ "$OS_NAME" != "Darwin" ]]; then
    printf '\e[31m[ERR]\e[0m This setup only supports macOS (Darwin). Detected %s.\n' "$OS_NAME" >&2
    exit 1
fi

if [[ "$ARCH" != "arm64" && "$ARCH" != "aarch64" ]]; then
    printf '\e[31m[ERR]\e[0m This setup only supports Apple Silicon (arm64/aarch64). Detected %s.\n' "$ARCH" >&2
    exit 1
fi
# Load or prompt for persistent architecture configuration
normalize_arch() {
    case "$1" in
        arm64) echo "aarch64" ;;
        amd64) echo "x86_64" ;;
        *) echo "$1" ;;
    esac
}

if [ -f "$ROOT/scripts/load_arch_config.sh" ]; then
    # shellcheck source=./scripts/load_arch_config.sh
    source "$ROOT/scripts/load_arch_config.sh" --prompt
else
    echo "⚠️  load_arch_config.sh not found. Skipping architecture config."
    COHESIX_ARCH="$(normalize_arch "$(uname -m)")"
    export COHESIX_ARCH
    echo "🔧 Fallback: setting COHESIX_ARCH to $COHESIX_ARCH"
    CONFIG_FILE="$HOME/.cohesix_config"
    echo "COHESIX_ARCH=$COHESIX_ARCH" > "$CONFIG_FILE"
    echo "✅ Wrote fallback config to $CONFIG_FILE"
fi

msg(){ printf "\e[32m==>\e[0m %s\n" "$*"; }
die(){ printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

ensure_homebrew_shellenv() {
    if command -v brew >/dev/null 2>&1; then
        eval "$(brew shellenv)"
        return
    fi

    if [ -x "/opt/homebrew/bin/brew" ]; then
        eval "$(/opt/homebrew/bin/brew shellenv)"
        return
    fi

    if [ -x "/usr/local/bin/brew" ]; then
        eval "$(/usr/local/bin/brew shellenv)"
        return
    fi

    msg "Installing Homebrew package manager …"
    NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    if [ -x "/opt/homebrew/bin/brew" ]; then
        eval "$(/opt/homebrew/bin/brew shellenv)"
    elif [ -x "/usr/local/bin/brew" ]; then
        eval "$(/usr/local/bin/brew shellenv)"
    else
        die "Homebrew installation did not provide a usable brew binary."
    fi
}

ensure_brew_packages() {
    local manager_script="$ROOT/scripts/manage_homebrew_packages.sh"
    if [ ! -x "$manager_script" ]; then
        die "Missing package manager helper at $manager_script"
    fi
    "$manager_script" install "$@"
}

ensure_python_bin() {
    local python_bin=""
    for candidate in python3.12 python3.11 python3.10 python3; do
        if command -v "$candidate" >/dev/null 2>&1; then
            local bin_path
            bin_path="$(command -v "$candidate")"
            if [[ "$OS_NAME" == "Darwin" && "$bin_path" == "/usr/bin/python3" ]]; then
                continue
            fi
            python_bin="$bin_path"
            break
        fi
    done

    if [[ -z "$python_bin" ]]; then
        die "Unable to locate a usable python3 interpreter."
    fi

    printf '%s\n' "$python_bin"
}

msg "Detected macOS host (architecture: $ARCH)."

MACOS_VERSION="$(sw_vers -productVersion 2>/dev/null || echo "0")"
MACOS_MAJOR="${MACOS_VERSION%%.*}"
if [[ "$MACOS_MAJOR" =~ ^[0-9]+$ && "$MACOS_MAJOR" -lt 26 ]]; then
    msg "WARNING: macOS $MACOS_VERSION detected. This setup is validated for macOS 26 or newer."
fi

ensure_homebrew_shellenv

brew_pkgs=(
    qemu
    aarch64-unknown-linux-gnu
    llvm
    cmake
    ninja
    python@3.12
    pkg-config
    coreutils
    gnu-tar
)
ensure_brew_packages "${brew_pkgs[@]}"

PYTHON_BIN="$(ensure_python_bin)"

if ! command -v aarch64-unknown-linux-gnu-gcc >/dev/null 2>&1; then
    die "aarch64-unknown-linux-gnu-gcc not found on PATH after installation. Ensure the Homebrew LLVM toolchain is linked."
fi

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    die "qemu-system-aarch64 not found on PATH after installation. Confirm Homebrew's qemu package is correctly installed."
fi

VENV="$ROOT/.venv"
if [ -d "$VENV" ]; then
    rm -rf "$VENV"
fi

"$PYTHON_BIN" -m venv "$VENV"
VENV_PYTHON="$VENV/bin/python3"
if [ ! -x "$VENV_PYTHON" ]; then
    VENV_PYTHON="$VENV/bin/python"
fi

"$VENV_PYTHON" -m pip install --upgrade pip >/dev/null
"$VENV_PYTHON" -m pip install jinja2 ply pyyaml >/dev/null

# Ensure ~/.cohesix_config exists
CONFIG_FILE="$HOME/.cohesix_config"
if [ ! -f "$CONFIG_FILE" ]; then
    echo "🔧 Creating default Cohesix config at $CONFIG_FILE"
    cat > "$CONFIG_FILE" <<EOF
# Cohesix Architecture Configuration
COHESIX_ARCH=$(normalize_arch "$(uname -m)")
EOF
fi

echo "✅ Build environment setup complete."
