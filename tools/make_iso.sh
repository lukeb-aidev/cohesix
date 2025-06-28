// CLASSIFICATION: COMMUNITY
// Filename: tools/make_iso.sh v0.7
// Author: Lukas Bower
// Date Modified: 2026-10-16
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO_ROOT="$ROOT/out/iso"
ISO_OUT="$ROOT/out/cohesix.iso"
ROLE="${1:-${COHROLE:-QueenPrimary}}"

cleanup() {
    [ -d "$ISO_ROOT" ] && rm -rf "$ISO_ROOT"
}
trap cleanup EXIT

mkdir -p "$ISO_ROOT/boot/grub" "$ISO_ROOT/bin" "$ISO_ROOT/usr/bin" \
         "$ISO_ROOT/usr/cli" "$ISO_ROOT/usr/share/man" "$ISO_ROOT/usr/share/cohesix/man" \
         "$ISO_ROOT/etc" "$ISO_ROOT/roles" "$ISO_ROOT/srv" "$ISO_ROOT/home/cohesix"

KERNEL_SRC="$ROOT/out/bin/kernel.elf"
ROOT_SRC="$ROOT/out/cohesix_root.elf"

[ -f "$KERNEL_SRC" ] || { echo "kernel.elf missing at $KERNEL_SRC" >&2; exit 1; }
[ -f "$ROOT_SRC" ] || { echo "userland.elf missing at $ROOT_SRC" >&2; exit 1; }

cp "$KERNEL_SRC" "$ISO_ROOT/boot/kernel.elf"
cp "$ROOT_SRC" "$ISO_ROOT/boot/userland.elf"

if [ -f "$ROOT/out/boot/config.yaml" ]; then
    cp "$ROOT/out/boot/config.yaml" "$ISO_ROOT/boot/config.yaml"
fi
[ -f "$ROOT/bootargs.txt" ] && cp "$ROOT/bootargs.txt" "$ISO_ROOT/boot/bootargs.txt"

# BusyBox and shell
if [ -x "$ROOT/out/bin/busybox" ]; then
    cp "$ROOT/out/bin/busybox" "$ISO_ROOT/bin/busybox"
    for a in ash sh ls cp mv echo mount cat ps kill; do
        ln -sf busybox "$ISO_ROOT/bin/$a"
    done
fi

# CLI tools
for t in cohcli cohcap cohtrace cohrun cohbuild cohcc cohshell.sh; do
    if [ -f "$ROOT/bin/$t" ]; then
        dest="$t"
        [ "$t" = "cohshell.sh" ] && dest="cohesix-shell"
        cp "$ROOT/bin/$t" "$ISO_ROOT/usr/bin/$dest"
        chmod +x "$ISO_ROOT/usr/bin/$dest"
    fi
done
ln -sf cohcli "$ISO_ROOT/usr/bin/cohesix"

# Go helpers
if [ -d "$ROOT/go/bin" ]; then
    cp -r "$ROOT/go/bin/." "$ISO_ROOT/usr/cli/" 2>/dev/null || true
    if [ -f "$ROOT/go/bin/coh-9p-helper" ]; then
        mkdir -p "$ISO_ROOT/srv/9p"
        cp "$ROOT/go/bin/coh-9p-helper" "$ISO_ROOT/srv/9p/"
    fi
fi

# Python runtime modules
[ -d "$ROOT/python" ] && cp -r "$ROOT/python" "$ISO_ROOT/home/cohesix" 2>/dev/null || true

# Man pages
if [ -d "$ROOT/docs/man" ]; then
    cp "$ROOT"/docs/man/*.1 "$ISO_ROOT/usr/share/man/" 2>/dev/null || true
    cp "$ROOT"/docs/man/*.8 "$ISO_ROOT/usr/share/man/" 2>/dev/null || true
fi
[ -f "$ROOT/bin/mandoc" ] && cp "$ROOT/bin/mandoc" "$ISO_ROOT/bin/mandoc" && chmod +x "$ISO_ROOT/bin/mandoc"
[ -f "$ROOT/bin/man" ] && cp "$ROOT/bin/man" "$ISO_ROOT/bin/man" && chmod +x "$ISO_ROOT/bin/man"

# plan9 namespace and test boot script
if [ -f "$ROOT/config/plan9.ns" ]; then
    cp "$ROOT/config/plan9.ns" "$ISO_ROOT/etc/plan9.ns"
fi
[ -f "$ROOT/etc/test_boot.sh" ] && cp "$ROOT/etc/test_boot.sh" "$ISO_ROOT/etc/test_boot.sh"

# roles and config
if [ -d "$ROOT/out/roles" ]; then
    cp -a "$ROOT/out/roles/." "$ISO_ROOT/roles/"
fi
if [ -f "$ROOT/out/etc/cohesix/config.yaml" ]; then
    mkdir -p "$ISO_ROOT/etc/cohesix"
    cp "$ROOT/out/etc/cohesix/config.yaml" "$ISO_ROOT/etc/cohesix/config.yaml"
fi
[ -f "$ISO_ROOT/etc/cohesix/config.yaml" ] || { echo "config.yaml missing" >&2; exit 1; }

# Optional miniroot for early shell testing
[ -d "$ROOT/userland/miniroot" ] && cp -a "$ROOT/userland/miniroot" "$ISO_ROOT/miniroot"

cat >"$ISO_ROOT/boot/grub/grub.cfg" <<CFG
set default=0
set timeout=0
set CohRole=${ROLE}
menuentry "Cohesix" {
  multiboot2 /boot/kernel.elf
  module /boot/userland.elf CohRole=\${CohRole}
  module /boot/config.yaml
  module /boot/bootargs.txt
}
CFG

command -v grub-mkrescue >/dev/null 2>&1 || { echo "grub-mkrescue not found" >&2; exit 1; }

grub-mkrescue -o "$ISO_OUT" "$ISO_ROOT" >/dev/null 2>&1

validate(){
    local r="$1"; local fail=0
    for t in cohesix cohcap cohtrace cohrun cohbuild cohcc cohesix-shell; do
        [ -x "$r/usr/bin/$t" ] || { echo "Missing $t"; fail=1; }
        [ -f "$r/usr/share/man/${t%.sh}.1" ] || { echo "Man page missing for $t"; fail=1; }
    done
    [ -x "$r/bin/busybox" ] || { echo "busybox missing"; fail=1; }
    [ -f "$r/etc/cohesix/config.yaml" ] || { echo "config.yaml missing"; fail=1; }
    [ $fail -eq 0 ] || { echo "ISO validation failed"; exit 1; }
    echo "ISO validation passed"
}

validate "$ISO_ROOT"
