#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.10
# Author: Lukas Bower
# Date Modified: 2026-12-31

set -euo pipefail
set -x

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
LOG_DIR="${LOG_DIR:-$ROOT/log}"
mkdir -p "$LOG_DIR"
LOG_FILE="${LOG_FILE:-$LOG_DIR/make_iso.log}"
: > "$LOG_FILE"
exec 3>&1
log() {
    echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG_FILE" >&3
}

ISO_ROOT="$ROOT/out/iso"
ISO_OUT="$ROOT/out/cohesix.iso"
ROLE="${1:-${COHROLE:-QueenPrimary}}"

cleanup() {
    [ -d "$ISO_ROOT" ] && rm -rf "$ISO_ROOT"
}
trap cleanup EXIT

log "Preparing ISO root at $ISO_ROOT"
mkdir -p "$ISO_ROOT/EFI/BOOT" "$ISO_ROOT/boot" "$ISO_ROOT/bin" "$ISO_ROOT/usr/bin" \
         "$ISO_ROOT/usr/share/man" "$ISO_ROOT/usr/share/cohesix/man" \
         "$ISO_ROOT/etc/cohesix" "$ISO_ROOT/roles" "$ISO_ROOT/srv" \
         "$ISO_ROOT/home/cohesix" "$ISO_ROOT/upgrade" "$ISO_ROOT/log"

KERNEL_SRC="$ROOT/out/bin/kernel.efi"
ROOT_SRC="$ROOT/out/cohesix_root.elf"

[ -f "$KERNEL_SRC" ] || { log "kernel.efi missing at $KERNEL_SRC"; exit 1; }
[ -f "$ROOT_SRC" ] || { log "userland.elf missing at $ROOT_SRC"; exit 1; }

log "Copying kernel and userland binaries..."
cp "$KERNEL_SRC" "$ISO_ROOT/boot/kernel.efi"
cp "$ROOT_SRC" "$ISO_ROOT/boot/userland.elf"
sha256sum "$ISO_ROOT/boot/kernel.efi" | tee -a "$LOG_FILE" >&3
sha256sum "$ISO_ROOT/boot/userland.elf" | tee -a "$LOG_FILE" >&3
ls -lh "$ISO_ROOT/boot" | tee -a "$LOG_FILE" >&3

log "Ensuring config.yaml exists..."
if [ -f "$ROOT/out/etc/cohesix/config.yaml" ]; then
    cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_ROOT/etc/cohesix/config.yaml"
    log "Creating default role.conf..."
    echo "CohRole=DroneWorker" > "$ISO_ROOT/etc/role.conf"
else
    log "ERROR: config.yaml missing in build output"
    exit 1
fi

# BusyBox and shell
if [ -x "$ROOT/out/bin/busybox" ]; then
    log "Installing BusyBox..."
    cp "$ROOT/out/bin/busybox" "$ISO_ROOT/bin/busybox"
    for a in ash sh ls cp mv echo mount cat ps kill; do
        ln -sf busybox "$ISO_ROOT/bin/$a"
    done
else
    log "WARNING: BusyBox binary not found, shell tools may be incomplete"
fi

# CLI tools
log "Staging CLI tools..."
for t in cohcli cohcap cohtrace cohrun cohbuild cohcc cohshell.sh \
         demo_bee_learns demo_cloud_queen demo_cuda_edge \
         demo_secure_relay demo_sensor_world demo_multi_duel \
         demo_trace_validation; do
    if [ -f "$ROOT/bin/$t" ]; then
        dest="$t"
        [ "$t" = "cohshell.sh" ] && dest="cohesix-shell"
        cp "$ROOT/bin/$t" "$ISO_ROOT/usr/bin/$dest"
        chmod +x "$ISO_ROOT/usr/bin/$dest"
    else
        log "WARNING: CLI tool $t not found"
    fi
done
ln -sf cohcli "$ISO_ROOT/usr/bin/cohesix"

# Demos (CUDA and Rapier)
if [ -d "$ROOT/src/demos" ]; then
    log "Ignoring legacy Python demo assets"
else
    log "WARNING: demo sources not found"
fi

# Python CLI modules
log "Skipping deprecated Python CLI modules"

# Python modules
log "Skipping deprecated Python runtime"

# Man pages
if [ -d "$ROOT/docs/man" ]; then
    log "Installing man pages..."
    cp "$ROOT"/docs/man/*.1 "$ISO_ROOT/usr/share/man/" 2>/dev/null || true
    cp "$ROOT"/docs/man/*.8 "$ISO_ROOT/usr/share/man/" 2>/dev/null || true
else
    log "WARNING: No man page sources found"
fi
[ -f "$ROOT/bin/mandoc" ] && cp "$ROOT/bin/mandoc" "$ISO_ROOT/bin/mandoc" && chmod +x "$ISO_ROOT/bin/mandoc"
[ -f "$ROOT/bin/man" ] && cp "$ROOT/bin/man" "$ISO_ROOT/bin/man" && chmod +x "$ISO_ROOT/bin/man"

# Plan9 namespace and boot scripts
[ -f "$ROOT/config/plan9.ns" ] && cp "$ROOT/config/plan9.ns" "$ISO_ROOT/etc/plan9.ns"
[ -f "$ROOT/etc/test_boot.sh" ] && cp "$ROOT/etc/test_boot.sh" "$ISO_ROOT/etc/test_boot.sh"
[ -f "$ISO_ROOT/etc/plan9.ns" ] || { log "ERROR: /etc/plan9.ns missing"; exit 1; }
[ -f "$ISO_ROOT/etc/cohesix/config.yaml" ] || { log "ERROR: config.yaml missing in ISO"; exit 1; }

# Roles
if [ -d "$ROOT/out/roles" ]; then
    log "Copying role definitions..."
    cp -a "$ROOT/out/roles/." "$ISO_ROOT/roles/"
fi

# Miniroot
[ -d "$ROOT/userland/miniroot" ] && cp -a "$ROOT/userland/miniroot" "$ISO_ROOT/miniroot"

ARCH="${COH_ARCH:-$(uname -m)}"
UEFI_DIR="$ISO_ROOT/EFI/BOOT"
case "$ARCH" in
  x86_64|amd64)
    BOOT_EFI="BOOTX64.EFI"
    ;;
  aarch64|arm64)
    BOOT_EFI="BOOTAA64.EFI"
    ;;
  *)
    log "❌ Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

log "Staging EFI binary as $BOOT_EFI"
cp "$KERNEL_SRC" "$UEFI_DIR/$BOOT_EFI"

command -v xorriso >/dev/null 2>&1 || { log "xorriso not found"; exit 1; }

log "Creating ISO image at $ISO_OUT..."
xorriso -as mkisofs -R -J -joliet -V Cohesix -o "$ISO_OUT" \
    -efi-boot "EFI/BOOT/$BOOT_EFI" -no-emul-boot "$ISO_ROOT" || {
    log "xorriso failed"; exit 1;
}

# Validation
fail=0
for t in cohesix cohcap cohtrace cohrun cohbuild cohcc cohesix-shell; do
    if [ ! -x "$ISO_ROOT/usr/bin/$t" ]; then
        log "Missing executable: $t"
        fail=1
    fi
    if [ ! -f "$ISO_ROOT/usr/share/man/${t%.sh}.1" ]; then
        log "Man page missing for $t"
        fail=1
    fi
done
[ -x "$ISO_ROOT/bin/busybox" ] || { log "busybox missing"; fail=1; }
[ $fail -eq 0 ] || { log "ISO validation failed"; exit 1; }

log "ISO validation passed"

if command -v tree >/dev/null 2>&1; then
    tree "$ISO_ROOT"
else
    find "$ISO_ROOT"
fi

log "QEMU x86_64 test: qemu-system-x86_64 -bios OVMF.fd -cdrom $ISO_OUT -serial mon:stdio -nographic"
log "QEMU aarch64 test: qemu-system-aarch64 -M virt -cpu cortex-a57 -bios QEMU_EFI.fd -cdrom $ISO_OUT -serial mon:stdio -nographic"

log "DEBUG: Finished make_iso.sh execution."
