#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.1
# Author: Lukas Bower
# Date Modified: 2027-12-31
set -euo pipefail

WORKSPACE="${WORKSPACE:-$HOME/sel4_workspace}"
COHESIX_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

check_deps() {
  for dep in dtc repo cmake ninja aarch64-linux-gnu-gcc aarch64-linux-gnu-g++; do
    if ! command -v "$dep" >/dev/null 2>&1; then
      echo "❌ Missing dependency: $dep" >&2
      exit 1
    fi
  done
}

sync_sel4() {
  mkdir -p "$WORKSPACE"
  cd "$WORKSPACE"
  if [ ! -d .repo ]; then
    repo init -u https://github.com/seL4/sel4test-manifest.git --depth=1
  fi
  repo sync
}

init_submodules() {
  cd "$COHESIX_DIR"
  git submodule update --init --recursive
}

build_cargo() {
  cd "$COHESIX_DIR"
  cargo build --target aarch64-unknown-none --release
}

main() {
  check_deps
  sync_sel4
  init_submodules
  build_cargo
  echo "✅  Cohesix fetch and build completed successfully."
}

main "$@"
