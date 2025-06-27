# CLASSIFICATION: COMMUNITY
# Filename: tools/make_iso.sh v0.6
# Author: Lukas Bower
# Date Modified: 2026-10-10
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO_ROOT="$ROOT/out/iso_root"
ISO_OUT="$ROOT/out/cohesix.iso"
KERNEL_SRC="$ROOT/out/BOOTX64.EFI"

error(){ echo "[make_iso] $1" >&2; exit 1; }

if command -v xorriso >/dev/null 2>&1; then
    MKISO=(xorriso -as mkisofs)
elif command -v mkisofs >/dev/null 2>&1; then
    MKISO=(mkisofs)
else
    error "xorriso or mkisofs required"
fi

[ -f "$KERNEL_SRC" ] || error "Missing kernel $KERNEL_SRC"

rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT"/{{bin,usr/bin,usr/share/cohesix/man,etc,roles,srv,home/cohesix,EFI/BOOT}}
# Include optional miniroot for early shell testing
if [ -d "$ROOT/userland/miniroot" ]; then
    cp -a "$ROOT/userland/miniroot" "$ISO_ROOT/miniroot"
fi

# Kernel and bootloader
cp "$KERNEL_SRC" "$ISO_ROOT/EFI/BOOT/bootx64.efi"
cp "$KERNEL_SRC" "$ISO_ROOT/kernel.efi"

# Copy runtime binaries
if [ -d "$ROOT/out/bin" ]; then
    cp -a "$ROOT/out/bin/." "$ISO_ROOT/bin/"
fi

# CLI wrappers
for tool in cohcli cohcap cohtrace cohrun cohbuild cohcc cohshell.sh; do
    if [ -f "$ROOT/bin/$tool" ]; then
        dest="$tool"
        [ "$tool" = "cohshell.sh" ] && dest="cohesix-shell"
        cp "$ROOT/bin/$tool" "$ISO_ROOT/usr/bin/$dest"
        chmod +x "$ISO_ROOT/usr/bin/$dest"
    fi
done
ln -sf cohcli "$ISO_ROOT/usr/bin/cohesix"

# BusyBox and shell
if [ -x "$ROOT/out/bin/busybox" ]; then
    cp "$ROOT/out/bin/busybox" "$ISO_ROOT/bin/busybox"
    for app in ash sh ls cp mv echo mount cat ps kill; do
        ln -sf busybox "$ISO_ROOT/bin/$app"
    done
fi

command -v bash >/dev/null 2>&1 && ln -sf "$(command -v bash)" "$ISO_ROOT/bin/bash"
ln -sf /usr/bin/python3 "$ISO_ROOT/usr/bin/python3"

# Man pages
if [ -d "$ROOT/docs/man" ]; then
    cp "$ROOT"/docs/man/*.1 "$ISO_ROOT/usr/share/cohesix/man/"
fi
if [ -f "$ROOT/bin/man" ]; then
    cp "$ROOT/bin/man" "$ISO_ROOT/usr/bin/man" && chmod +x "$ISO_ROOT/usr/bin/man"
fi
if [ -f "$ROOT/bin/mandoc" ]; then
    cp "$ROOT/bin/mandoc" "$ISO_ROOT/bin/mandoc" && chmod +x "$ISO_ROOT/bin/mandoc"
fi

# Configuration files
cp -a "$ROOT/etc/." "$ISO_ROOT/etc/" 2>/dev/null || true
if [ -f "$ROOT/out/etc/cohesix/config.yaml" ]; then
    mkdir -p "$ISO_ROOT/etc/cohesix"
    cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_ROOT/etc/cohesix/config.yaml"
fi
if [ -d "$ROOT/out/roles" ]; then
    cp -a "$ROOT/out/roles/." "$ISO_ROOT/roles/"
fi
[ -f "$ISO_ROOT/etc/cohesix/config.yaml" ] || error "config.yaml missing"

# Optional role file
[ -f "$ROOT/out/srv/cohrole" ] && cp "$ROOT/out/srv/cohrole" "$ISO_ROOT/srv/cohrole"

# Cloud hook configuration
if [ -f "$ROOT/etc/cloud.toml" ]; then
    cp "$ROOT/etc/cloud.toml" "$ISO_ROOT/etc/cloud.toml"
fi
if [ -n "${CLOUD_HOOK_URL:-}" ]; then
    echo "$CLOUD_HOOK_URL" > "$ISO_ROOT/etc/cloud_hook"
fi

"${MKISO[@]}" -R -J -o "$ISO_OUT" "$ISO_ROOT"

# Validation step
validate(){
    local root="$1"; local ok=0; local fail=0
    check(){ [ -e "$root/$1" ]; }
    exec_check(){ [ -x "$root/$1" ]; }

    for t in cohesix cohcap cohtrace cohrun cohbuild cohcc cohesix-shell; do
        exec_check "usr/bin/$t" || { echo "Missing $t"; fail=1; }
        check "usr/share/cohesix/man/${t%.sh}.1" || { echo "Man page missing for $t"; fail=1; }
    done
    exec_check "usr/bin/python3" || { echo "python3 missing"; fail=1; }
    exec_check "bin/busybox" || { echo "busybox missing"; fail=1; }
    exec_check "usr/bin/man" || { echo "man tool missing"; fail=1; }
    check "etc/cohesix/config.yaml" || { echo "config.yaml missing"; fail=1; }
    if ! check "srv/cohrole" && ! check "etc/cohrole"; then
        echo "cohrole missing"; fail=1
    fi
    check "miniroot/bin/echo" || { echo "miniroot missing"; fail=1; }
    check "etc/test_boot.sh" || { echo "test_boot.sh missing"; fail=1; }
    [ $fail -eq 0 ] || { echo "ISO validation failed"; exit 1; }
    echo "ISO validation passed"
}

validate "$ISO_ROOT"

# Boot test hint
# Run: qemu-system-x86_64 -cdrom "$ISO_OUT" -m 512M -nographic -no-reboot
