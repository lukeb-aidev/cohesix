#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Install Cohesix host-build and diagnostic-QEMU dependencies on Ubuntu ARM64.
# Copyright 2026 Lukas Bower

set -euo pipefail

RUST_TOOLCHAIN_VERSION="1.97.1"
RUST_TARGET="aarch64-unknown-none"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SETUP_REPO_VENV="${SCRIPT_DIR}/setup_repo_venv.sh"
SKIP_REPO_VENV=0

usage() {
  cat <<'EOF'
Usage: toolchain/setup_linux_arm64.sh [--skip-venv]

Install the Cohesix host-tool build dependencies, Rust 1.97.1, and diagnostic
QEMU support on Ubuntu 22.04, 24.04, or 26.04 ARM64. By default the script also
creates the repository .venv and installs the Cohesix Python client.

This Linux lane does not create the pinned macOS seL4 compiler/profile inputs
and does not by itself establish QEMU or release acceptance.
EOF
}

log() {
  printf "[toolchain] %s\n" "$*"
}

fail() {
  printf "[toolchain] error: %s\n" "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-venv)
      SKIP_REPO_VENV=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

[[ "$(uname -s)" == "Linux" ]] || fail "this installer requires Linux"
case "$(uname -m)" in
  aarch64|arm64) ;;
  *) fail "this installer requires an ARM64 host; detected $(uname -m)" ;;
esac

if [[ -f /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
else
  fail "/etc/os-release not found; unable to detect Linux distribution"
fi

[[ "${ID:-}" == "ubuntu" ]] || \
  fail "unsupported Linux distribution: ${ID:-unknown} (expected Ubuntu)"
case "${VERSION_ID:-}" in
  22.04|24.04|26.04) ;;
  *)
    fail "unsupported Ubuntu release: ${VERSION_ID:-unknown} (expected 22.04, 24.04, or 26.04)"
    ;;
esac

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
  python3-pip
  curl
  pkg-config
  jq
  ripgrep
  qemu-system-arm
  qemu-utils
  libfuse3-dev
  libssl-dev
  libwebkit2gtk-4.1-dev
  libjavascriptcoregtk-4.1-dev
  libgtk-3-dev
  libayatana-appindicator3-dev
  librsvg2-dev
  libxdo-dev
  binutils
  protobuf-compiler
  cpio
  make
)

if [[ "${VERSION_ID}" == "22.04" ]]; then
  PACKAGES+=(python3.11 python3.11-venv)
  VENV_PYTHON="python3.11"
else
  PACKAGES+=(python3 python3-venv)
  VENV_PYTHON="python3"
fi

log "Updating apt indices..."
DEBIAN_FRONTEND=noninteractive "${APT_PREFIX[@]}" update -y

if ! command -v add-apt-repository >/dev/null 2>&1; then
  DEBIAN_FRONTEND=noninteractive \
    "${APT_PREFIX[@]}" install -y software-properties-common
fi
ADD_REPOSITORY=(add-apt-repository)
if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  ADD_REPOSITORY=(sudo add-apt-repository)
fi
log "Ensuring Ubuntu Universe is enabled..."
DEBIAN_FRONTEND=noninteractive "${ADD_REPOSITORY[@]}" -y universe
DEBIAN_FRONTEND=noninteractive "${APT_PREFIX[@]}" update -y

log "Installing packages: ${PACKAGES[*]}"
DEBIAN_FRONTEND=noninteractive "${APT_PREFIX[@]}" install -y "${PACKAGES[@]}"

if ! command -v rustup >/dev/null 2>&1; then
  log "Installing rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
    -y --profile minimal --default-toolchain "$RUST_TOOLCHAIN_VERSION"
fi
if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi
command -v rustup >/dev/null 2>&1 || \
  fail "rustup is unavailable after installation"

log "Ensuring Rust toolchain ${RUST_TOOLCHAIN_VERSION} is installed..."
rustup toolchain install "$RUST_TOOLCHAIN_VERSION" --profile minimal
rustup override set "$RUST_TOOLCHAIN_VERSION"

log "Ensuring rustfmt and clippy are installed for ${RUST_TOOLCHAIN_VERSION}..."
rustup component add rustfmt clippy --toolchain "$RUST_TOOLCHAIN_VERSION"

log "Ensuring target ${RUST_TARGET} is installed for ${RUST_TOOLCHAIN_VERSION}..."
rustup target add "$RUST_TARGET" --toolchain "$RUST_TOOLCHAIN_VERSION"

log "Rust version: $(rustc --version)"

if ! command -v qemu-system-aarch64 >/dev/null 2>&1; then
  fail "qemu-system-aarch64 not in PATH after install"
fi

log "QEMU version: $(qemu-system-aarch64 --version | head -n1)"

if ! qemu-system-aarch64 -accel help 2>/dev/null |
  grep -Eq '(^|[[:space:]])tcg($|[[:space:]])'
then
  fail "QEMU does not report the required TCG accelerator"
fi

if [[ "${SKIP_REPO_VENV}" -eq 0 ]]; then
  "${SETUP_REPO_VENV}" --python "$(command -v "${VENV_PYTHON}")"
else
  log "Skipping repository .venv setup by request."
fi

log "Linux host-tool and diagnostic-QEMU setup complete in ${REPO_ROOT}."
