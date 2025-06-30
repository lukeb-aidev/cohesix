#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: make_iso.sh v0.5
# Author: Lukas Bower
# Date Modified: 2026-11-21

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
mkdir -p "$ISO_ROOT/boot/grub" "$ISO_ROOT/bin" "$ISO_ROOT/usr/bin" \
         "$ISO_ROOT/usr/cli" "$ISO_ROOT/usr/share/man" "$ISO_ROOT/usr/share/cohesix/man" \
         "$ISO_ROOT/etc/cohesix" "$ISO_ROOT/roles" "$ISO_ROOT/srv" \
         "$ISO_ROOT/home/cohesix" "$ISO_ROOT/upgrade" "$ISO_ROOT/log"

KERNEL_SRC="$ROOT/out/bin/kernel.elf"
ROOT_SRC="$ROOT/out/cohesix_root.elf"

[ -f "$KERNEL_SRC" ] || { log "kernel.elf missing at $KERNEL_SRC"; exit 1; }
[ -f "$ROOT_SRC" ] || { log "userland.elf missing at $ROOT_SRC"; exit 1; }

log "Copying kernel and userland binaries..."
cp "$KERNEL_SRC" "$ISO_ROOT/boot/kernel.elf"
cp "$ROOT_SRC" "$ISO_ROOT/boot/userland.elf"
sha256sum "$ISO_ROOT/boot/kernel.elf" | tee -a "$LOG_FILE" >&3
sha256sum "$ISO_ROOT/boot/userland.elf" | tee -a "$LOG_FILE" >&3
ls -lh "$ISO_ROOT/boot" | tee -a "$LOG_FILE" >&3

log "Ensuring config.yaml exists..."
if [ -f "$ROOT/out/etc/cohesix/config.yaml" ]; then
    cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_ROOT/etc/cohesix/config.yaml"
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
for t in cohcli cohcap cohtrace cohrun cohbuild cohcc cohshell.sh; do
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

# Go helpers
if [ -d "$ROOT/go/bin" ]; then
    log "Copying Go CLI helpers..."
    cp -r "$ROOT/go/bin/." "$ISO_ROOT/usr/cli/" 2>/dev/null || true
    if [ -f "$ROOT/go/bin/coh-9p-helper" ]; then
        mkdir -p "$ISO_ROOT/srv/9p"
        cp "$ROOT/go/bin/coh-9p-helper" "$ISO_ROOT/srv/9p/"
    fi
else
    log "WARNING: No Go helpers found"
fi

# Python modules
if [ -d "$ROOT/python" ]; then
    log "Copying Python modules..."
    cp -r "$ROOT/python" "$ISO_ROOT/home/cohesix" 2>/dev/null || true
else
    log "WARNING: No Python modules directory found"
fi

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
case "$ARCH" in
  x86_64|amd64)
    GRUB_TARGET="i386-pc-efi"
    GRUB_MODULE_PATH="/usr/lib/grub/i386-pc"
    GRUB_ENTRY="  multiboot2 /boot/kernel.elf\n  module /boot/userland.elf CohRole=\${CohRole}"
    ;;
  aarch64|arm64)
    GRUB_TARGET="arm64-efi"
    GRUB_MODULE_PATH="/usr/lib/grub/arm64-efi"
    GRUB_ENTRY="  linux /boot/kernel.elf CohRole=\${CohRole}"
    ;;
  *)
    log "❌ Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

# GRUB config
log "Creating GRUB configuration for $ARCH..."
cat >"$ISO_ROOT/boot/grub/grub.cfg" <<CFG
set default=0
set timeout=5
if [ "\${CohRole}" = "" ]; then
    set CohRole=${ROLE}
fi
menuentry "Cohesix (Role: \${CohRole})" {
$GRUB_ENTRY
}
CFG

log "Detected arch: $ARCH, using GRUB target: $GRUB_TARGET"

log "DEBUG: ROOT=$ROOT"
log "DEBUG: ISO_ROOT=$ISO_ROOT"
log "DEBUG: ISO_OUT=$ISO_OUT"
log "DEBUG: GRUB_MODULE_PATH=$GRUB_MODULE_PATH"
log "DEBUG: checking if GRUB module dir exists: [ -d \"$GRUB_MODULE_PATH\" ]"

if [ ! -d "$GRUB_MODULE_PATH" ]; then
    log "ERROR: GRUB modules for $GRUB_TARGET not found at $GRUB_MODULE_PATH"
    exit 1
fi
module_count=$(find "$GRUB_MODULE_PATH" -name '*.mod' | wc -l)
log "GRUB modules detected: $module_count in $GRUB_MODULE_PATH"
if [ "$ARCH" = "x86_64" ] || [ "$ARCH" = "amd64" ]; then
    [ -f "$GRUB_MODULE_PATH/multiboot2.mod" ] || { log "ERROR: multiboot2.mod missing"; exit 1; }
elif [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
    [ -f "$GRUB_MODULE_PATH/efi_gop.mod" ] || { log "ERROR: efi_gop.mod missing"; exit 1; }
fi

command -v grub-mkrescue >/dev/null 2>&1 || { log "grub-mkrescue not found"; exit 1; }
command -v xorriso >/dev/null 2>&1 || { log "xorriso not found (required by grub-mkrescue)"; exit 1; }

log "Creating ISO image at $ISO_OUT..."
MODULES="part_gpt efi_gop ext2 fat normal iso9660 configfile linux"
if [ "$ARCH" = "x86_64" ] || [ "$ARCH" = "amd64" ]; then
    MODULES="$MODULES multiboot2"
fi

log "Using GRUB modules: $MODULES"
log "DRY-RUN: grub-mkrescue -o $ISO_OUT $ISO_ROOT --modules=\"$MODULES\""
grub-mkrescue -o "$ISO_OUT" "$ISO_ROOT" \
    --modules="$MODULES" \
    || { log "grub-mkrescue failed"; exit 1; }

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

log "QEMU x86_64 test: qemu-system-x86_64 -cdrom $ISO_OUT -boot d -m 1024"
log "QEMU aarch64 test: qemu-system-aarch64 -M virt -cpu cortex-a57 -bios QEMU_EFI.fd -cdrom $ISO_OUT -m 1024"

log "DEBUG: Finished make_iso.sh execution."
