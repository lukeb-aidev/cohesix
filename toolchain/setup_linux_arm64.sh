#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Install Cohesix Linux host toolchain dependencies (Ubuntu, ARM64).

set -euo pipefail

log() {
  printf "[toolchain] %s\n" "$*"
}

warn() {
  printf "[toolchain] warning: %s\n" "$*" >&2
}

fail() {
  printf "[toolchain] error: %s\n" "$*" >&2
  exit 1
}

if [[ -f /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
else
  fail "/etc/os-release not found; unable to detect Linux distribution"
fi

if [[ "${ID:-}" != "ubuntu" ]]; then
  warn "expected Ubuntu; detected ${ID:-unknown}"
fi

if [[ "${VERSION_ID:-}" != 22.* && "${VERSION_ID:-}" != 24.* ]]; then
  warn "expected Ubuntu 22.04 or 24.04; detected ${VERSION_ID:-unknown}"
fi

if ! command -v apt-get >/dev/null 2>&1; then
  fail "apt-get not found; unsupported Linux distribution"
fi

APT_PREFIX=(apt-get)
if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  if command -v sudo >/dev/null 2>&1; then
    APT_PREFIX=(sudo apt-get)
  else
    fail "sudo is required to install packages"
  fi
fi

PACKAGES=(
  build-essential
  git
  cmake
  ninja-build
  clang
  lld
  llvm
  python3
  python3-venv
  python3-pip
  curl
  pkg-config
  jq
  qemu-system-aarch64
  qemu-utils
  libfuse3-dev
)

log "Updating apt indices..."
DEBIAN_FRONTEND=noninteractive "${APT_PREFIX[@]}" update -y

log "Installing packages: ${PACKAGES[*]}"
DEBIAN_FRONTEND=noninteractive "${APT_PREFIX[@]}" install -y "${PACKAGES[@]}"

if ! command -v rustup >/dev/null 2>&1; then
  log "Installing rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
else
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

log "Ensuring rustfmt and clippy are installed..."
rustup component add rustfmt clippy --toolchain stable

log "Rust version: $(rustc --version)"

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
  fail "qemu-system-aarch64 not in PATH after install"
fi

log "QEMU version: $(qemu-system-aarch64 --version | head -n1)"

log "Toolchain setup complete."
