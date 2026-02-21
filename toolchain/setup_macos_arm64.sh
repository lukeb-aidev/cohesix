#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Install Cohesix macOS ARM64 host toolchain dependencies and pinned Rust toolchain.
# Copyright 2026 Lukas Bower

set -euo pipefail

BREW_PACKAGES=(git cmake ninja llvm@17 python@3 qemu coreutils jq)
RUST_TOOLCHAIN_VERSION="1.93.1"
RUST_TARGET="aarch64-unknown-none"

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
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain "$RUST_TOOLCHAIN_VERSION"
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
else
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi

echo "Ensuring Rust toolchain $RUST_TOOLCHAIN_VERSION is installed..."
rustup toolchain install "$RUST_TOOLCHAIN_VERSION"
rustup override set "$RUST_TOOLCHAIN_VERSION"

echo "Ensuring rustfmt and clippy are installed for $RUST_TOOLCHAIN_VERSION..."
rustup component add rustfmt clippy --toolchain "$RUST_TOOLCHAIN_VERSION"

echo "Ensuring target $RUST_TARGET is installed for $RUST_TOOLCHAIN_VERSION..."
rustup target add "$RUST_TARGET" --toolchain "$RUST_TOOLCHAIN_VERSION"

echo "Rust version: $(rustc --version)"

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
    echo "QEMU installation failed; qemu-system-aarch64 not in PATH." >&2
    exit 1
fi

echo "QEMU version: $(qemu-system-aarch64 --version | head -n1)"

echo "Toolchain setup complete."
