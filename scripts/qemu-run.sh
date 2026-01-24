#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Package a minimal rootfs and launch Cohesix under QEMU.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

log() {
    echo "[qemu-run] $*"
}

describe_file() {
    local label="$1"
    local path="$2"

    if [[ ! -f "$path" ]]; then
        log "$label missing: $path"
        return
    fi

    python3 - "$label" "$path" <<'PY'
import hashlib
import pathlib
import sys

label = sys.argv[1]
path = pathlib.Path(sys.argv[2])
data = path.read_bytes()
size = path.stat().st_size
digest = hashlib.sha256(data).hexdigest()
print(f"[qemu-run] {label}: {path} ({size} bytes, sha256={digest})")
PY
}

usage() {
    cat <<'USAGE'
Usage: scripts/qemu-run.sh --elfloader <path> --kernel <path> --root-task <path> [--out-dir <dir>] [--qemu <bin>] [--tcp-port <port>]

Boot seL4 in QEMU using externally built artefacts while packaging the Cohesix
root task into a CPIO archive. The script mirrors the expectations in
`docs/BUILD_PLAN.md` Milestone 0 and assumes that all binaries have already been
built for aarch64.
USAGE
}

ELFLOADER=""
KERNEL=""
ROOT_TASK=""
OUT_DIR="out"
QEMU_BIN="qemu-system-aarch64"
SEL4_BUILD_DIR="${SEL4_BUILD:-$HOME/seL4/build}"
DTB_OVERRIDE=""
DEFAULT_TCP_PORT=31337
TCP_PORT=""
SELFTEST_TCP_PORT=31339

while [[ $# -gt 0 ]]; do
    case "$1" in
        --elfloader)
            ELFLOADER="$2"
            shift 2
            ;;
        --kernel)
            KERNEL="$2"
            shift 2
            ;;
        --root-task)
            ROOT_TASK="$2"
            shift 2
            ;;
        --out-dir)
            OUT_DIR="$2"
            shift 2
            ;;
        --qemu)
            QEMU_BIN="$2"
            shift 2
            ;;
        --sel4-build)
            SEL4_BUILD_DIR="$2"
            shift 2
            ;;
        --dtb)
            DTB_OVERRIDE="$2"
            shift 2
            ;;
        --tcp-port)
            TCP_PORT="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

if [[ -z "$ELFLOADER" || -z "$KERNEL" || -z "$ROOT_TASK" ]]; then
    echo "Missing required arguments." >&2
    usage
    exit 1
fi

if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
    log "seL4 build directory not found: $SEL4_BUILD_DIR"
    exit 1
fi

detect_gic_version() {
    local cfg_file=""
    local candidate
    for candidate in \
        "$SEL4_BUILD_DIR/kernel/gen_config/kernel_config.h" \
        "$SEL4_BUILD_DIR/kernel/gen_config/kernel/gen_config.h" \
        "$SEL4_BUILD_DIR/kernel/include/autoconf.h" \
        "$SEL4_BUILD_DIR/kernel/autoconf/autoconf.h"; do
        if [[ -f "$candidate" ]]; then
            cfg_file="$candidate"
            break
        fi
    done

    if [[ -z "$cfg_file" ]]; then
        echo "[qemu-run] ERROR: cannot find seL4 config to infer GIC" >&2
        exit 2
    fi

    local detect_script="$SCRIPT_DIR/lib/detect_gic_version.py"
    if [[ ! -x "$detect_script" ]]; then
        echo "[qemu-run] ERROR: helper missing or not executable: $detect_script" >&2
        exit 2
    fi

    local result
    if ! result=$("$detect_script" "$cfg_file"); then
        echo "[qemu-run] ERROR: cannot infer GIC version from $cfg_file" >&2
        exit 2
    fi

    if [[ -z "$result" ]]; then
        echo "[qemu-run] ERROR: cannot infer GIC version from $cfg_file" >&2
        exit 2
    fi

    echo "$result"
}

detect_qemu_accel() {
    local accel="${COHESIX_QEMU_ACCEL:-${QEMU_ACCEL:-}}"
    if [[ -n "$accel" ]]; then
        echo "$accel"
        return
    fi

    local host_os
    host_os="$(uname -s 2>/dev/null || true)"
    case "$host_os" in
        Darwin)
            echo "hvf"
            ;;
        Linux)
            if [[ -c /dev/kvm && -r /dev/kvm && -w /dev/kvm ]]; then
                echo "kvm"
            else
                echo "tcg"
            fi
            ;;
        *)
            echo "tcg"
            ;;
    esac
}

qemu_accel_supported() {
    local accel="$1"
    local help
    help="$("$QEMU_BIN" -accel help 2>/dev/null || true)"
    if [[ -z "$help" ]]; then
        return 0
    fi
    echo "$help" | grep -Eiq "(^|[ ,])${accel}([ ,]|$)"
}

resolve_qemu_accel() {
    local accel
    accel="$(detect_qemu_accel)"
    if [[ -z "$accel" ]]; then
        accel="tcg"
    fi
    if ! qemu_accel_supported "$accel"; then
        log "Requested QEMU accelerator '$accel' not supported by $QEMU_BIN; falling back to tcg"
        accel="tcg"
    fi
    echo "$accel"
}

for artefact in "$ELFLOADER" "$KERNEL" "$ROOT_TASK"; do
    if [[ ! -f "$artefact" ]]; then
        log "Artefact not found: $artefact"
        exit 1
    fi
done

if ! command -v "$QEMU_BIN" >/dev/null 2>&1; then
    log "QEMU binary not found: $QEMU_BIN"
    exit 1
fi

if ! command -v cpio >/dev/null 2>&1; then
    log "cpio is required to package the rootfs."
    exit 1
fi

mkdir -p "$OUT_DIR/rootfs/bin"
cp "$ROOT_TASK" "$OUT_DIR/rootfs/bin/root-task"
mkdir -p "$OUT_DIR/rootfs/proc/tests"
for script in selftest_quick.coh selftest_full.coh selftest_negative.coh; do
    SRC="$SCRIPT_DIR/../resources/proc_tests/$script"
    if [[ ! -f "$SRC" ]]; then
        log "Selftest script missing: $SRC"
        exit 1
    fi
    cp "$SRC" "$OUT_DIR/rootfs/proc/tests/$script"
done

pushd "$OUT_DIR/rootfs" >/dev/null
find . -print | cpio -o -H newc > ../rootfs.cpio
popd >/dev/null

ROOTFS_CPIO="$OUT_DIR/rootfs.cpio"

describe_file "Elfloader" "$ELFLOADER"
describe_file "Kernel" "$KERNEL"
describe_file "Root task" "$ROOT_TASK"
describe_file "Rootfs CPIO" "$ROOTFS_CPIO"

QEMU_VERSION="$($QEMU_BIN --version | head -n1)"
log "Using QEMU binary: $QEMU_BIN ($QEMU_VERSION)"

GIC_VER="$(detect_gic_version)"
log "Auto-detected GIC version: gic-version=$GIC_VER"
QEMU_ACCEL="$(resolve_qemu_accel)"
log "Using QEMU accel: $QEMU_ACCEL"

QEMU_ARGS=(-accel "$QEMU_ACCEL" \
    -machine "virt,gic-version=${GIC_VER}" \
    -cpu cortex-a57 \
    -m 1024 \
    -smp 1 \
    -serial stdio \
    -monitor none \
    -display none \
    -kernel "$ELFLOADER" \
    -initrd "$ROOTFS_CPIO" \
    -device loader,file="$KERNEL",addr=0x70000000,force-raw=on \
    -device loader,file="$ROOT_TASK",addr=0x80000000,force-raw=on \
    -global virtio-mmio.force-legacy=off)

if [[ -z "$TCP_PORT" ]]; then
    TCP_PORT="$DEFAULT_TCP_PORT"
fi

if ! [[ "$TCP_PORT" =~ ^[0-9]+$ ]]; then
    log "Invalid TCP port: $TCP_PORT"
    exit 1
fi

NETWORK_ARGS=(
    -netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${TCP_PORT}-10.0.2.15:${TCP_PORT},hostfwd=tcp:127.0.0.1:${SELFTEST_TCP_PORT}-10.0.2.15:${SELFTEST_TCP_PORT}"
    -device virtio-net-device,netdev=net0,bus=virtio-mmio-bus.0
)
log "Forwarding TCP console on 127.0.0.1:${TCP_PORT} (QEMU user networking)"
log "Connect using: nc 127.0.0.1 ${TCP_PORT}"
log "Forwarding net self-test on 127.0.0.1:${SELFTEST_TCP_PORT} (QEMU user networking)"
log "Self-test check: nc 127.0.0.1 ${SELFTEST_TCP_PORT}"

if [[ -n "$DTB_OVERRIDE" ]]; then
    if [[ ! -f "$DTB_OVERRIDE" ]]; then
        log "DTB override not found: $DTB_OVERRIDE"
        exit 1
    fi
    describe_file "DTB override" "$DTB_OVERRIDE"
    QEMU_ARGS+=(-dtb "$DTB_OVERRIDE")
fi

log "Prepared QEMU command: ${QEMU_ARGS[*]} ${NETWORK_ARGS[*]}"

exec "$QEMU_BIN" "${QEMU_ARGS[@]}" "${NETWORK_ARGS[@]}"
