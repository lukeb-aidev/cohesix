# CLASSIFICATION: COMMUNITY
# Filename: make_grub_iso.sh v0.13
# Author: Lukas Bower
# Date Modified: 2026-08-04
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ISO_ROOT="$ROOT/out/iso"
ISO_OUT="$ROOT/out/cohesix_grub.iso"
ROLE="${1:-${COHROLE:-QueenPrimary}}"

ensure_plan9_ns() {
    local ns_path="$HOME/cohesix/config/plan9.ns"
    if [ ! -f "$ns_path" ]; then
        echo "⚠️ $ns_path missing. Generating default..."
        mkdir -p "$(dirname "$ns_path")"
        cat > "$ns_path" <<'EOF'
// CLASSIFICATION: COMMUNITY
// Filename: config/plan9.ns v0.1
// Author: Lukas Bower
// Date Modified: 2026-08-04
mount -b /dev /dev
mount -b /proc /proc
bind -a /bin /bin
bind -a /usr/py /usr/py
bind -a /srv /srv
bind -a /mnt/9root /
EOF
    fi
    mkdir -p "$ISO_ROOT/etc"
    if cp "$ns_path" "$ISO_ROOT/etc/plan9.ns"; then
        echo "✅ plan9.ns staged"
    else
        echo "⚠️ plan9.ns staging failed" >&2
    fi
}

success=0
cleanup() {
    if [ $success -ne 1 ]; then
        rm -rf "$ISO_ROOT"
    fi
}
trap cleanup EXIT

# Create stage directory if missing
mkdir -p "$ISO_ROOT/boot/grub"

# Ensure kernel and root task ELFs exist
KERNEL_ELF="$ISO_ROOT/boot/kernel.elf"
ROOT_ELF="$ISO_ROOT/boot/userland.elf"
INIT_EFI="$ISO_ROOT/bin/init.efi"
if [ ! -s "$KERNEL_ELF" ]; then
    echo "ERROR: kernel ELF missing at $KERNEL_ELF" >&2
    exit 1
fi
if [ ! -s "$ROOT_ELF" ]; then
    echo "ERROR: root ELF missing at $ROOT_ELF" >&2
    exit 1
fi
if [ ! -x "$INIT_EFI" ]; then
    echo "WARNING: init EFI not executable or missing at $INIT_EFI" >&2
else
    echo "✅ init EFI present"
fi

 # Kernel and root task already staged in $ISO_ROOT/boot/
CONFIG_YAML="$ROOT/config/config.yaml"
if [ ! -f "$CONFIG_YAML" ]; then
    echo "Generating default config.yaml" >&2
    mkdir -p "$ROOT/config"
    cat > "$CONFIG_YAML" <<EOF
# Auto-generated fallback config
system:
  role: worker
  trace: true
EOF
fi
cp "$CONFIG_YAML" "$ISO_ROOT/boot/config.yaml"
if [ -f "$INIT_EFI" ]; then
    chmod +x "$INIT_EFI"
fi
ensure_plan9_ns

# BusyBox utilities
mkdir -p "$ISO_ROOT/bin"
if [ -x "$ROOT/out/bin/busybox" ]; then
    cp "$ROOT/out/bin/busybox" "$ISO_ROOT/bin/busybox"
    for app in ash sh ls cp mv echo mount cat ps kill; do
        ln -sf busybox "$ISO_ROOT/bin/$app"
    done
fi

# Demo launchers and assets
for f in "$ROOT"/bin/demo_*; do
    if [ -f "$f" ]; then
        cp "$f" "$ISO_ROOT/bin/" && chmod +x "$ISO_ROOT/bin/$(basename "$f")"
    fi
done
if [ -d "$ROOT/src/demos" ]; then
    mkdir -p "$ISO_ROOT/usr/share/cohesix/src"
    cp -r "$ROOT/src/demos" "$ISO_ROOT/usr/share/cohesix/src/" 2>/dev/null || true
fi

# Man pages and mandoc
if [ -d "$ROOT/docs/man" ]; then
    mkdir -p "$ISO_ROOT/usr/share/man"
    cp "$ROOT"/docs/man/*.1 "$ISO_ROOT/usr/share/man/" 2>/dev/null || true
    cp "$ROOT"/docs/man/*.8 "$ISO_ROOT/usr/share/man/" 2>/dev/null || true
fi
if [ -f "$ROOT/bin/mandoc" ]; then
    cp "$ROOT/bin/mandoc" "$ISO_ROOT/bin/mandoc" && chmod +x "$ISO_ROOT/bin/mandoc"
fi
if [ -d "$ROOT/prebuilt/mandoc" ]; then
    mkdir -p "$ISO_ROOT/prebuilt/mandoc"
    cp "$ROOT"/prebuilt/mandoc/mandoc.* "$ISO_ROOT/prebuilt/mandoc/" 2>/dev/null || true
fi
if [ -f "$ROOT/bin/man" ]; then
    cp "$ROOT/bin/man" "$ISO_ROOT/bin/man" && chmod +x "$ISO_ROOT/bin/man"
fi

# Validate man pages using mandoc
if [ -x "$ISO_ROOT/bin/mandoc" ]; then
    for m in "$ISO_ROOT/usr/share/man"/*.[18]; do
        "$ISO_ROOT/bin/mandoc" -Tascii "$m" >/dev/null || {
            echo "mandoc parse failed for $m" >&2
            exit 1
        }
    done
fi

# Optional demo libraries
mkdir -p "$ISO_ROOT/lib"
for lib in "$ROOT"/prebuilt/lib/*.so; do
    [ -f "$lib" ] && cp "$lib" "$ISO_ROOT/lib/" || true
done

# Ensure GPU and physics directories exist
mkdir -p "$ISO_ROOT/srv/cuda" "$ISO_ROOT/sim"

# Stage optional CUDA and physics assets if available
if [ -d "$ROOT/srv/cuda" ]; then
    cp -r "$ROOT/srv/cuda/." "$ISO_ROOT/srv/cuda/" 2>/dev/null || true
else
    echo "WARNING: $ROOT/srv/cuda missing; skipping CUDA staging" >&2
fi

if [ -d "$ROOT/sim" ]; then
    cp -r "$ROOT/sim/." "$ISO_ROOT/sim/" 2>/dev/null || true
else
    echo "WARNING: $ROOT/sim missing; skipping physics assets" >&2
fi

# Generate grub.cfg
cat >"$ISO_ROOT/boot/grub/grub.cfg" <<CFG
set default=0
set timeout=0
set CohRole=${ROLE}
menuentry "Cohesix" {
  multiboot2 /boot/kernel.elf
  module /boot/userland.elf CohRole=${ROLE}
  module /boot/config.yaml
}
CFG

# Build ISO using grub-mkrescue
if ! command -v grub-mkrescue >/dev/null 2>&1; then
    echo "ERROR: grub-mkrescue not found" >&2
    exit 1
fi


grub-mkrescue -o "$ISO_OUT" "$ISO_ROOT" >/dev/null 2>&1

# Ensure summary directories exist before scanning
mkdir -p "$ISO_ROOT/bin" "$ISO_ROOT/roles"

if [ -f "$ISO_OUT" ] && [ -s "$ISO_OUT" ]; then
    BIN_COUNT=0
    if [ -d "$ISO_ROOT/bin" ]; then
        BIN_COUNT=$(find "$ISO_ROOT/bin" -type f -perm -111 | wc -l)
    fi
    ROLE_COUNT=0
    if [ -d "$ISO_ROOT/roles" ]; then
        ROLE_COUNT=$(find "$ISO_ROOT/roles" -name '*.yaml' | wc -l)
    fi
    SIZE_MB=$(du -m "$ISO_OUT" | awk '{print $1}')
    echo "ISO BUILD OK: ${BIN_COUNT} binaries, ${ROLE_COUNT} roles, ${SIZE_MB}MB total"
else
    echo "ERROR: ISO build failed" >&2
    exit 1
fi

if command -v qemu-system-x86_64 >/dev/null 2>&1; then
    OVMF_CODE=""
    for p in /usr/share/OVMF/OVMF_CODE.fd /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF.fd /usr/share/qemu/OVMF.fd /usr/share/edk2/ovmf/OVMF_CODE.fd; do
        if [ -f "$p" ]; then
            OVMF_CODE="$p"
            break
        fi
    done
    OVMF_VARS=""
    for p in /usr/share/OVMF/OVMF_VARS.fd /usr/share/OVMF/OVMF_VARS_4M.fd /usr/share/edk2/ovmf/OVMF_VARS.fd; do
        if [ -f "$p" ]; then
            OVMF_VARS="$p"
            break
        fi
    done
    if [ -n "$OVMF_CODE" ] && [ -n "$OVMF_VARS" ]; then
        TMP_VARS="$(mktemp)"
        cp "$OVMF_VARS" "$TMP_VARS"
        timeout 20 qemu-system-x86_64 -bios "$OVMF_CODE" \
            -drive if=pflash,format=raw,file="$TMP_VARS" \
            -cdrom "$ISO_OUT" -net none -M q35 -m 256M \
            -nographic -no-reboot -serial mon:stdio >/dev/null 2>&1
        QEMU_STATUS=$?
        rm -f "$TMP_VARS"
        if [ $QEMU_STATUS -ne 0 ]; then
            echo "ERROR: QEMU boot failed" >&2
            exit 1
        fi
    else
        echo "WARNING: OVMF firmware not found; skipping boot test" >&2
    fi
else
    echo "WARNING: qemu-system-x86_64 not available; skipping boot test" >&2
fi

success=1
