#!/bin/bash
set -euxo pipefail

ROOT=$(git rev-parse --show-toplevel)
LOG_DIR="$ROOT/log"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/make_iso.log"
: > "$LOG_FILE"
exec > >(tee -a "$LOG_FILE") 2>&1

ISO_ROOT="$ROOT/out/iso"
ISO_OUT="$ROOT/out/cohesix.iso"
ROLE="QueenPrimary"

log() {
  echo "[$(date +%H:%M:%S)] $*"
}

trap cleanup EXIT
cleanup() {
  [ -d "$ISO_ROOT" ] && rm -rf "$ISO_ROOT"
}

log "Preparing ISO root at $ISO_ROOT"
mkdir -p "$ISO_ROOT/EFI/BOOT" "$ISO_ROOT/boot" "$ISO_ROOT/bin" \
         "$ISO_ROOT/usr/bin" "$ISO_ROOT/usr/share/man" "$ISO_ROOT/usr/share/cohesix/man" \
         "$ISO_ROOT/etc/cohesix" "$ISO_ROOT/roles" "$ISO_ROOT/srv" \
         "$ISO_ROOT/home/cohesix" "$ISO_ROOT/upgrade" "$ISO_ROOT/log"

cp "$ROOT/out/bin/kernel.elf" "$ISO_ROOT/boot/kernel.elf"
cp "$ROOT/out/cohesix_root.elf" "$ISO_ROOT/boot/userland.elf"

cp "$ROOT/out/bin/busybox" "$ISO_ROOT/bin/busybox"
for app in ash sh ls cp mv echo mount cat ps kill; do
  ln -sf busybox "$ISO_ROOT/bin/$app"
done

cli_tools="cohesix cohcap cohtrace cohrun cohbuild cohcc"
missing_tools=()
for tool in $cli_tools; do
  if [ ! -f "$ROOT/bin/$tool" ]; then
    if [ -f "$ROOT/target/release/$tool" ]; then
      log "Copying $tool from target/release to bin/"
      cp "$ROOT/target/release/$tool" "$ROOT/bin/$tool"
      chmod +x "$ROOT/bin/$tool"
    else
      missing_tools+=("$tool")
    fi
  fi
done

for tool in $cli_tools; do
  if [ -f "$ROOT/bin/$tool" ]; then
    cp "$ROOT/bin/$tool" "$ISO_ROOT/usr/bin/$tool"
    chmod +x "$ISO_ROOT/usr/bin/$tool"
  else
    log "Missing CLI tool: $tool"
  fi
done

if [ -f "$ROOT/bin/cohshell.sh" ]; then
  cp "$ROOT/bin/cohshell.sh" "$ISO_ROOT/usr/bin/cohesix-shell"
  chmod +x "$ISO_ROOT/usr/bin/cohesix-shell"
else
  log "Missing cohshell.sh"
fi

demos="demo_bee_learns demo_cloud_queen demo_cuda_edge demo_secure_relay demo_sensor_world demo_multi_duel demo_trace_validation"
for demo in $demos; do
  if [ -f "$ROOT/bin/$demo" ]; then
    cp "$ROOT/bin/$demo" "$ISO_ROOT/usr/bin/$demo"
    chmod +x "$ISO_ROOT/usr/bin/$demo"
  else
    log "Skipping missing demo binary: $demo"
  fi
done

cp -r "$ROOT/userland/miniroot" "$ISO_ROOT/miniroot"
cp -r "$ROOT/out/roles" "$ISO_ROOT/roles"
cp "$ROOT/config/plan9.ns" "$ISO_ROOT/etc/plan9.ns"
cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_ROOT/etc/cohesix/config.yaml"

mkdir -p "$ISO_ROOT/usr/share/cohesix/man"
cp $ROOT/docs/man/* "$ISO_ROOT/usr/share/cohesix/man/"
cp $ROOT/docs/man/* "$ISO_ROOT/usr/share/man/"

cp "$ROOT/out/bin/kernel.elf" "$ISO_ROOT/EFI/BOOT/BOOTAA64.EFI"

log "Creating ISO image at $ISO_OUT..."
xorriso -as mkisofs -R -J -joliet -V Cohesix \
  -o "$ISO_OUT" \
  -eltorito-alt-boot -e EFI/BOOT/BOOTAA64.EFI -no-emul-boot \
  "$ISO_ROOT"

log "✅ ISO creation complete."
log "Installed CLI tools:"
for tool in $cli_tools; do
  if [ -f "$ISO_ROOT/usr/bin/$tool" ]; then
    echo " - $tool"
  else
    echo " - $tool (missing)"
  fi
done

log "Optional demos skipped if missing."
exit 0
