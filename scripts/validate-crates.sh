# CLASSIFICATION: COMMUNITY
# Filename: validate-crates.sh v0.1
# Author: Lukas Bower
# Date Modified: 2028-11-26

set -euo pipefail

# Verify each crate manifest parses
for crate in $(grep -R "path =" workspace/Cargo.toml | cut -d '"' -f2); do
  echo "🔍 Checking $crate"
  test -f "workspace/$crate/Cargo.toml" || { echo "ERROR: missing workspace/$crate/Cargo.toml"; exit 1; }
  cargo +nightly metadata --manifest-path "workspace/$crate/Cargo.toml" >/dev/null
done

# Build entire workspace
echo "🔨 Building workspace"
cargo +nightly build --manifest-path workspace/Cargo.toml --workspace --release \
  -Z build-std=core,alloc,compiler_builtins \
  -Z build-std-features=compiler-builtins-mem
