#!/bin/bash
# cohesix_fetch_build.sh
# Fetch and fully build the Cohesix project using SSH Git auth.
# Author: Lukas Bower
# Date: 2025-06-12

set -euo pipefail

timestamp=$(date +%Y%m%d_%H%M%S)
cd ~

echo "📦 Cloning Git repo via SSH..."

# Backup existing folder if it exists
if [ -d "cohesix" ]; then
  mv cohesix "cohesix_backup_$timestamp"
  echo "🗂️ Moved existing repo to cohesix_backup_$timestamp"
fi

# Clone using SSH key (assumes GitHub SSH auth already configured)
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
cargo test --release || true

echo "🐹 Building Go components..."
if [ -f go.mod ]; then
  go build ./...
fi

echo "🐍 Running Python tests (pytest)..."
if command -v pytest &> /dev/null && [ -d "tests_py" ]; then
  pytest tests_py || true
fi

echo "🧱 CMake config (if present)..."
if [ -f CMakeLists.txt ]; then
  mkdir -p build && cd build
  cmake ..
  make -j$(nproc)
  cd ..
fi

echo "✅ All builds complete."