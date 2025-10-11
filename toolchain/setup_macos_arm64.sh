#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

BREW_PACKAGES=(git cmake ninja llvm@17 python@3 qemu coreutils jq)

if ! command -v brew >/dev/null 2>&1; then
    echo "Homebrew is required. Install it from https://brew.sh/ and re-run this script." >&2
    exit 1
fi

echo "Updating Homebrew formulas..."
brew update

echo "Ensuring required Homebrew packages are present..."
for pkg in "${BREW_PACKAGES[@]}"; do
    if ! brew list --formula "$pkg" >/dev/null 2>&1; then
        echo "Installing $pkg"
        brew install "$pkg"
    else
        echo "Package $pkg already installed"
    fi
done

if [[ -d /opt/homebrew/opt/llvm/bin ]]; then
    export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
fi

if ! command -v rustup >/dev/null 2>&1; then
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
else
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi

echo "Ensuring rustfmt and clippy are installed..."
rustup component add rustfmt clippy --toolchain stable

echo "Rust version: $(rustc --version)"

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "QEMU installation failed; qemu-system-aarch64 not in PATH." >&2
    exit 1
fi

echo "QEMU version: $(qemu-system-aarch64 --version | head -n1)"

echo "Toolchain setup complete."
