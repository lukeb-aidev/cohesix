#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: setup_cohesix_sel4_env.sh v0.8
# Author: Lukas Bower
# Date Modified: 2027-12-31
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE="$ROOT/third_party/seL4/workspace"
LOG_DIR="$ROOT/logs"
mkdir -p "$LOG_DIR" "$WORKSPACE"

if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
else
    SUDO=""
fi

msg(){ printf "\e[32m==>\e[0m %s\n" "$*"; }
fail(){ printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

command -v apt-get >/dev/null 2>&1 || fail "apt-get not found"
msg "Installing seL4 prerequisites"

for tool in dtc cmake ninja aarch64-linux-gnu-gcc aarch64-linux-gnu-g++ pip3; do
  case $tool in
    dtc) pkg="device-tree-compiler" ;;
    cmake) pkg="cmake" ;;
    ninja) pkg="ninja-build" ;;
    aarch64-linux-gnu-gcc) pkg="gcc-aarch64-linux-gnu" ;;
    aarch64-linux-gnu-g++) pkg="g++-aarch64-linux-gnu" ;;
    pip3) pkg="python3-pip" ;;
  esac

  if command -v "$tool" >/dev/null 2>&1; then
    echo "✅ $tool already installed"
  else
    echo "📦 Installing $pkg..."
    $SUDO apt update
    $SUDO apt install -y "$pkg" || { echo "❌ Failed to install $pkg"; exit 1; }
  fi
done

echo "dtc version: $(dtc --version)"
echo "✅ All required packages are installed."

[ -x /usr/bin/repo ] || fail "repo not installed at /usr/bin/repo"

for cmd in dtc cmake ninja aarch64-linux-gnu-gcc aarch64-linux-gnu-g++ python3 repo curl git; do
    command -v "$cmd" >/dev/null 2>&1 || fail "$cmd not found in PATH"
done

msg "Syncing seL4 workspace at $WORKSPACE"
cd "$WORKSPACE"
if [ ! -d .repo ]; then
    repo init -u https://github.com/seL4/sel4test-manifest.git --depth=1
fi
repo sync

for d in kernel projects tools; do
    [ -d "$WORKSPACE/$d" ] || fail "Missing $d after repo sync"
done

msg "✅ Cohesix seL4 environment is ready."
exit 0

