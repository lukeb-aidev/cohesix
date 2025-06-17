// CLASSIFICATION: COMMUNITY
// Filename: full_fetch_and_build.sh v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-29
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

mkdir -p out/bin
mkdir -p out/etc
if [ ! -f out/etc/init.conf ] && [ -f etc/init.conf ]; then
  cp etc/init.conf out/etc/init.conf
fi

msg(){ printf "\e[32m[build]\e[0m %s\n" "$*"; }
fail(){ printf "\e[31m[error]\e[0m %s\n" "$*" >&2; exit 1; }

TARGET="x86_64-unknown-uefi"

msg "Building kernel EFI…"
cargo build --release --target "$TARGET" --bin kernel \
  --no-default-features --features minimal_uefi,kernel_bin
KERNEL_EFI="target/${TARGET}/release/kernel.efi"
[ -f "$KERNEL_EFI" ] || fail "Kernel EFI missing at $KERNEL_EFI"
cp "$KERNEL_EFI" out/kernel.efi

# Build primary userland binary
msg "Building init EFI…"
cargo build --release --target "$TARGET" --bin init \
  --no-default-features --features minimal_uefi
INIT_EFI="target/${TARGET}/release/init.efi"
[ -f "$INIT_EFI" ] || fail "init EFI missing at $INIT_EFI"
cp "$INIT_EFI" out/bin/init.efi

# Build additional userland binaries
msg "Scanning for userland binaries…"
META=$(mktemp)
cargo metadata --format-version 1 --no-deps > "$META"
jq -r '.packages[].targets[] | select(.kind[]=="bin") | .name' "$META" | sort -u | while read -r bin; do
  case "$bin" in
    kernel|init|cohcc|cohcap|cohbuild|cohfuzz|scenario_compiler|cohtrace|cohrun_cli|cohagent|cohrun|cohup|cohesix|cohrole)
      continue;;
  esac
  msg "Building $bin EFI…"
  cargo build --release --target "$TARGET" --bin "$bin" \
    --no-default-features --features minimal_uefi || fail "Build failed for $bin"
  BIN_PATH="target/${TARGET}/release/${bin}.efi"
  if [ -f "$BIN_PATH" ]; then
    cp "$BIN_PATH" "out/bin/${bin}.efi"
  else
    fail "Expected $BIN_PATH not found"
  fi
 done
rm -f "$META"

msg "Creating bootable ISO…"
./make_iso.sh
[ -f out/cohesix.iso ] || fail "ISO not created"

if grep -q "-cdrom out/cohesix.iso" test_boot_efi.sh 2>/dev/null; then
  msg "ISO boot configuration verified in QEMU boot script"
else
  fail "ISO boot option missing in QEMU boot script"
fi

msg "Full build complete. Artifacts in out/"
