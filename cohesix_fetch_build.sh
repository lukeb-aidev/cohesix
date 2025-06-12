#!/bin/bash
# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-22

set -euo pipefail

timestamp=$(date +%Y%m%d_%H%M%S)
cd ~

echo "📦 Cloning Git repo via SSH..."

if [ -d "cohesix" ]; then
  mv cohesix "cohesix_backup_$timestamp"
  echo "🗂️ Moved existing repo to cohesix_backup_$timestamp"
fi

git clone git@github.com:lukeb-aidev/cohesix.git
cd cohesix

echo "📦 Updating submodules (if any)..."
git submodule update --init --recursive

echo "🐍 Setting up Python venv..."
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip setuptools wheel
if [ -f requirements.txt ]; then
  pip install -r requirements.txt
fi

echo "🦀 Building Rust components..."
cargo build --release

echo "🧪 Running Rust tests..."
RUST_BACKTRACE=1 cargo test --release || true

echo "🐹 Building Go components..."
if [ -f go.mod ]; then
  go build ./...
  go test ./... || true
fi

echo "🐍 Running Python tests (pytest)..."
if command -v pytest &> /dev/null; then
  pytest -v || true
fi

echo "🧱 CMake config (if present)..."
if [ -f CMakeLists.txt ]; then
  mkdir -p build && cd build
  cmake ..
  make -j$(nproc)
  cd ..
fi

echo "✅ All builds complete."
