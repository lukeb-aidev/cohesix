#!/usr/bin/env bash
set -euo pipefail
set -x

log() {
    echo "[$(date +%H:%M:%S)] $*"
}

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
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

# Roles
if [ -d "$ROOT/out/roles" ]; then
    log "Copying role definitions..."
    cp -a "$ROOT/out/roles/." "$ISO_ROOT/roles/"
fi

# Miniroot
[ -d "$ROOT/userland/miniroot" ] && cp -a "$ROOT/userland/miniroot" "$ISO_ROOT/miniroot"

# GRUB config
log "Creating GRUB configuration..."
cat >"$ISO_ROOT/boot/grub/grub.cfg" <<CFG
set default=0
set timeout=0
set CohRole=${ROLE}
menuentry "Cohesix" {
  multiboot2 /boot/kernel.elf
  module /boot/userland.elf CohRole=\${CohRole}
}
CFG

command -v grub-mkrescue >/dev/null 2>&1 || { log "grub-mkrescue not found"; exit 1; }

log "Creating ISO image at $ISO_OUT..."
grub-mkrescue -o "$ISO_OUT" "$ISO_ROOT" || { log "grub-mkrescue failed"; exit 1; }

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
