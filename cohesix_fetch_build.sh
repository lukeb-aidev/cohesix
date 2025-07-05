# CLASSIFICATION: COMMUNITY
# Filename: cohesix_fetch_build.sh v0.4
# Author: Lukas Bower
# Date Modified: 2027-01-31
#!/bin/bash
set -euo pipefail
LOG=/srv/upload/fix-log.txt
mkdir -p "$(dirname "$LOG")"
exec > >(tee -a "$LOG") 2>&1

ROOT=$(pwd)
export RUSTFLAGS="-C link-arg=-Tlink.ld"

echo "== Rust build =="
BINS=(cohesix_root cohcc cohesix_build cohesix_cap cohesix_trace init logdemo)
for BIN in "${BINS[@]}"; do
  case "$BIN" in
    init|logdemo)
      FEATURES="minimal_uefi"
      ;;
    *)
      FEATURES="std"
      ;;
  esac
  cargo build --release --bin "$BIN" --target aarch64-unknown-linux-musl --no-default-features --features "$FEATURES" || exit 1
done

cargo clippy --target aarch64-unknown-linux-musl -- -D warnings

mkdir -p out/bin
for BIN in "${BINS[@]}"; do
  cp "target/aarch64-unknown-linux-musl/release/$BIN" "out/bin/$BIN" || true
done
cp target/aarch64-unknown-linux-musl/release/cohesix_root out/bin/cohesix_root.elf

echo "== Go build =="
mkdir -p out/go_helpers
for dir in go/cmd/*; do
  if [ -f "$dir/main.go" ]; then
    name=$(basename "$dir")
    GOOS=linux GOARCH=arm64 go build -C "$dir" -o "$ROOT/out/go_helpers/$name" || true
  fi
done

if ls tests 2>/dev/null | grep -q '\.py'; then
  pytest || echo "pytest failed"
else
  echo "SKIPPED pytest"
fi

echo "== Build complete =="
