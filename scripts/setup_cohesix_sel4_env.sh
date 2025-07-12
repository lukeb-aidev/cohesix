# CLASSIFICATION: COMMUNITY
# Filename: setup_cohesix_sel4_env.sh v0.1
# Author: Lukas Bower
# Date Modified: 2027-12-31
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="$ROOT/logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/setup_sel4_$(date +%Y%m%d_%H%M%S).log"
exec > >(tee -a "$LOG_FILE") 2>&1

echo "🔧 Cohesix seL4 environment setup starting..."

SEL4_DIR="$ROOT/third_party/seL4"
SEL4_COMMIT="$(cat "$SEL4_DIR/COMMIT")"
WORKSPACE="$SEL4_DIR/workspace"
BUILD_DIR="$WORKSPACE/build_release"

echo "🌐 Removing old workspaces..."
rm -rf "$HOME/sel4_workspace" "$ROOT/sel4_workspace" "$WORKSPACE"

echo "📥 Cloning seL4 workspace..."
mkdir -p "$WORKSPACE"
cd "$WORKSPACE"
repo init -u https://github.com/seL4/sel4test-manifest.git --depth=1
repo sync
cd "$WORKSPACE/sel4"
git fetch origin "$SEL4_COMMIT" --depth 1
git checkout -q "$SEL4_COMMIT"

ln -sfn "$WORKSPACE" "$HOME/sel4_workspace"
ln -sfn "$WORKSPACE" "$ROOT/sel4_workspace"

echo "🔧 Installing toolchains..."
DEPS=(cmake ninja-build gcc-aarch64-linux-gnu g++-aarch64-linux-gnu python3-venv repo)
if command -v sudo >/dev/null 2>&1; then SUDO=sudo; else SUDO=""; fi
$SUDO apt-get update -y
$SUDO apt-get install -y "${DEPS[@]}"

if ! command -v rustup >/dev/null 2>&1; then
    curl https://sh.rustup.rs -sSf | sh -s -- -y
    source "$HOME/.cargo/env"
fi
rustup target add aarch64-unknown-linux-gnu
rustup component add rust-src

cd "$ROOT"
VENV_DIR=".venv_sel4"
if [ ! -d "$VENV_DIR" ]; then
    python3 -m venv "$VENV_DIR"
fi
source "$VENV_DIR/bin/activate"
pip install --upgrade pip

echo "🏗️ Building seL4 kernel and elfloader..."
cd "$WORKSPACE"
rm -rf build_release
mkdir build_release
cd build_release
../init-build.sh -C ../easy-settings.cmake -GNinja
ninja kernel.elf elfloader libsel4.a

file kernel/kernel.elf | grep -q "AArch64"
file elfloader/elfloader | grep -q "AArch64"
readelf -h kernel/kernel.elf | grep -q "AArch64"

mkdir -p "$SEL4_DIR/lib" "$SEL4_DIR/include"
cp libsel4/libsel4.a "$SEL4_DIR/lib/"
cp -r libsel4/include/* "$SEL4_DIR/include/"

echo "✅ seL4 environment ready"
