#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

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
Usage: scripts/qemu-run.sh --elfloader <path> --kernel <path> --root-task <path> [--out-dir <dir>] [--qemu <bin>]

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

QEMU_CMD=("$QEMU_BIN" \
    -machine virt,gic-version=3 \
    -cpu cortex-a57 \
    -m 1024 \
    -serial mon:stdio \
    -display none \
    -kernel "$ELFLOADER" \
    -initrd "$ROOTFS_CPIO" \
    -device loader,file="$KERNEL",addr=0x70000000 \
    -device loader,file="$ROOT_TASK",addr=0x80000000)

log "Prepared QEMU command: ${QEMU_CMD[*]}"

exec "${QEMU_CMD[@]}"
