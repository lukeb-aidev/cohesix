# CLASSIFICATION: COMMUNITY
# Filename: full_fetch_and_build.sh v0.9
# Author: Lukas Bower
# Date Modified: 2025-09-21
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

ISO_ROOT="${ISO_ROOT:-out}"
mkdir -p "$ISO_ROOT/bin" "$ISO_ROOT/roles" "$ISO_ROOT/etc/cohesix" "$ISO_ROOT/setup"

msg(){ printf "\e[32m[build]\e[0m %s\n" "$*"; }
fail(){ printf "\e[31m[error]\e[0m %s\n" "$*" >&2; exit 1; }

if [ ! -f setup/config.yaml ]; then
  fail "setup/config.yaml missing"
fi
cp setup/config.yaml "$ISO_ROOT/etc/cohesix/config.yaml" || fail "copy setup/config.yaml"
msg "Copied setup/config.yaml to $ISO_ROOT/etc/cohesix/config.yaml"

if ls setup/roles/*.yaml >/dev/null 2>&1; then
  for cfg in setup/roles/*.yaml; do
    role="$(basename "$cfg" .yaml)"
    mkdir -p "$ISO_ROOT/roles/$role"
    cp "$cfg" "$ISO_ROOT/roles/$role/config.yaml" || fail "copy $cfg"
    msg "Copied $cfg to $ISO_ROOT/roles/$role/config.yaml"
  done
else
  fail "No role configs found in setup/roles"
fi
for shf in setup/init.sh setup/*.sh; do
  [ -f "$shf" ] || continue
  cp "$shf" "$ISO_ROOT/setup/" || fail "copy $shf"
done

msg "Building workspace (secure9p)"
cargo build --release --all-targets --no-default-features --features "secure9p"
msg "Running library tests"
cargo test --release --lib || true

TARGET="x86_64-unknown-uefi"

msg "Building kernel EFI…"
cargo build --release --target "$TARGET" --bin kernel \
  --no-default-features --features minimal_uefi,kernel_bin
KERNEL_EFI="target/${TARGET}/release/kernel.efi"
[ -f "$KERNEL_EFI" ] || fail "Kernel EFI missing at $KERNEL_EFI"
cp "$KERNEL_EFI" out/BOOTX64.EFI

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

[ -f out/bin/init.efi ] || fail "out/bin/init.efi missing"
[ -f out/etc/cohesix/config.yaml ] || fail "out/etc/cohesix/config.yaml missing"

msg "Creating bootable ISO…"
./scripts/make_iso.sh
[ -f out/cohesix.iso ] || fail "ISO not created"

if grep -q "-cdrom out/cohesix.iso" test_boot_efi.sh 2>/dev/null; then
  msg "ISO boot configuration verified in QEMU boot script"
else
  fail "ISO boot option missing in QEMU boot script"
fi

msg "Full build complete. Artifacts in out/"
