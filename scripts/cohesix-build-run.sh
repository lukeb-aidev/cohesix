#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/cohesix-build-run.sh [options] [-- <extra-qemu-args>]

Build the Cohesix Rust workspace, assemble the seL4 payload CPIO archive, and
boot the system under QEMU. The script expects an existing seL4 build tree that
already produced `elfloader`, `kernel.elf`, and support artefacts. By default it
looks for that tree at `$HOME/seL4/build`.

Options:
  --sel4-build <dir>    Path to the seL4 build output (default: $HOME/seL4/build)
  --out-dir <dir>       Directory for generated artefacts (default: out/cohesix)
  --clean               Remove existing contents of the output directory before building
  --profile <name>      Cargo profile to build (release|debug|custom; default: release)
  --cargo-target <triple>  Target triple used for seL4 component builds (required)
  --qemu <path>         QEMU binary to execute (default: qemu-system-aarch64)
  --no-run              Skip launching QEMU after building the artefacts
  -h, --help            Show this help message

Any arguments following `--` are forwarded directly to QEMU.
USAGE
}

log() {
    echo "[cohesix-build] $*"
}

fail() {
    echo "[cohesix-build] error: $*" >&2
    exit 1
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
print(f"[cohesix-build] {label}: {path} ({size} bytes, sha256={digest})")
PY
}

SEL4_BUILD_DIR="${SEL4_BUILD:-$HOME/seL4/build}"
OUT_DIR="out/cohesix"
PROFILE="release"
CARGO_TARGET=""
QEMU_BIN="qemu-system-aarch64"
RUN_QEMU=1
EXTRA_QEMU_ARGS=()
CLEAN_OUT_DIR=0
DTB_OVERRIDE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --sel4-build)
            [[ $# -ge 2 ]] || fail "--sel4-build requires a directory"
            SEL4_BUILD_DIR="$2"
            shift 2
            ;;
        --out-dir)
            [[ $# -ge 2 ]] || fail "--out-dir requires a directory"
            OUT_DIR="$2"
            shift 2
            ;;
        --profile)
            [[ $# -ge 2 ]] || fail "--profile requires a value"
            PROFILE="$2"
            shift 2
            ;;
        --cargo-target)
            [[ $# -ge 2 ]] || fail "--cargo-target requires a triple"
            CARGO_TARGET="$2"
            shift 2
            ;;
        --qemu)
            [[ $# -ge 2 ]] || fail "--qemu requires a binary path"
            QEMU_BIN="$2"
            shift 2
            ;;
        --dtb)
            [[ $# -ge 2 ]] || fail "--dtb requires a path"
            DTB_OVERRIDE="$2"
            shift 2
            ;;
        --no-run)
            RUN_QEMU=0
            shift
            ;;
        --clean)
            CLEAN_OUT_DIR=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            EXTRA_QEMU_ARGS=("$@")
            break
            ;;
        *)
            fail "Unknown argument: $1"
            ;;
    esac
done

if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
    fail "seL4 build directory not found: $SEL4_BUILD_DIR"
fi

export SEL4_BUILD_DIR
export SEL4_BUILD="$SEL4_BUILD_DIR"

detect_gic_version() {
    local cfg_file
    for cfg_file in \
        "$SEL4_BUILD_DIR/kernel/gen_config/kernel_config.h" \
        "$SEL4_BUILD_DIR/kernel/include/autoconf.h"; do
        [[ -f "$cfg_file" ]] && break
    done

    [[ -f "$cfg_file" ]] || { echo "[cohesix-build] ERROR: cannot find seL4 config to infer GIC"; exit 2; }

    if grep -qE 'CONFIG_ARM_GIC_V3[= ]1' "$cfg_file"; then
        echo 3
    elif grep -qE 'CONFIG_ARM_GIC_V2[= ]1' "$cfg_file"; then
        echo 2
    else
        echo "[cohesix-build] ERROR: cannot infer GIC version from $cfg_file"
        exit 2
    fi
}

if [[ "$CLEAN_OUT_DIR" -eq 1 ]]; then
    if [[ -d "$OUT_DIR" ]]; then
        if [[ "$OUT_DIR" == "/" ]]; then
            fail "Refusing to clean the filesystem root"
        fi
        log "Cleaning output directory before build: $OUT_DIR"
        find "$OUT_DIR" -mindepth 1 -delete
    else
        log "Output directory $OUT_DIR does not exist; nothing to clean"
    fi
fi

for cmd in cargo cpio python3 "$QEMU_BIN"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        if [[ "$cmd" == "$QEMU_BIN" && "$RUN_QEMU" -eq 0 ]]; then
            log "Skipping QEMU availability check because --no-run was provided"
            break
        fi
        fail "Required command not found in PATH: $cmd"
    fi
    [[ "$cmd" == "$QEMU_BIN" ]] && break
done

if command -v "$QEMU_BIN" >/dev/null 2>&1; then
    QEMU_VERSION="$($QEMU_BIN --version | head -n1)"
    log "Using QEMU binary: $QEMU_BIN ($QEMU_VERSION)"
fi

ELFLOADER_PATH="$SEL4_BUILD_DIR/elfloader/elfloader"
KERNEL_PATH="$SEL4_BUILD_DIR/kernel/kernel.elf"
[[ -f "$ELFLOADER_PATH" ]] || fail "elfloader binary not found at $ELFLOADER_PATH"
[[ -f "$KERNEL_PATH" ]] || fail "kernel.elf not found at $KERNEL_PATH"

declare -a PROFILE_ARGS=()
PROFILE_DIR="$PROFILE"
case "$PROFILE" in
    release)
        PROFILE_ARGS=(--release)
        PROFILE_DIR="release"
        ;;
    dev|debug)
        PROFILE_DIR="debug"
        ;;
    *)
        PROFILE_ARGS=(--profile "$PROFILE")
        ;;
 esac

if [[ -z "$CARGO_TARGET" ]]; then
    fail "--cargo-target must be provided to build seL4 components"
fi

SEL4_COMPONENT_PACKAGES=(nine-door worker-heart worker-gpu)
HOST_TOOL_PACKAGES=(cohsh gpu-bridge-host)

HOST_BUILD_ARGS=(build)
if (( ${#PROFILE_ARGS[@]} > 0 )); then
    HOST_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
fi
for pkg in "${HOST_TOOL_PACKAGES[@]}"; do
    HOST_BUILD_ARGS+=(-p "$pkg")
done

log "Building host tooling via: cargo ${HOST_BUILD_ARGS[*]}"
cargo "${HOST_BUILD_ARGS[@]}"

SEL4_BUILD_ARGS=(build --target "$CARGO_TARGET")
if (( ${#PROFILE_ARGS[@]} > 0 )); then
    SEL4_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
fi
for pkg in "${SEL4_COMPONENT_PACKAGES[@]}"; do
    SEL4_BUILD_ARGS+=(-p "$pkg")
done

ROOT_TASK_BUILD_ARGS=(build --target "$CARGO_TARGET")
if (( ${#PROFILE_ARGS[@]} > 0 )); then
    ROOT_TASK_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
fi
ROOT_TASK_BUILD_ARGS+=(-p root-task -F root-task/sel4-console)

log "Building root-task with console support via: cargo ${ROOT_TASK_BUILD_ARGS[*]}"
cargo "${ROOT_TASK_BUILD_ARGS[@]}"

log "Building seL4 components via: cargo ${SEL4_BUILD_ARGS[*]}"
cargo "${SEL4_BUILD_ARGS[@]}"

HOST_ARTIFACT_DIR="target/$PROFILE_DIR"
SEL4_ARTIFACT_DIR="target/$CARGO_TARGET/$PROFILE_DIR"

[[ -d "$HOST_ARTIFACT_DIR" ]] || fail "Cargo artefact directory not found: $HOST_ARTIFACT_DIR"
[[ -d "$SEL4_ARTIFACT_DIR" ]] || fail "Cargo artefact directory not found: $SEL4_ARTIFACT_DIR"

COMPONENT_BINS=(root-task nine-door worker-heart worker-gpu)
HOST_ONLY_BINS=(cohsh gpu-bridge-host)

mkdir -p "$OUT_DIR"
OUT_DIR_ABS="$(cd "$OUT_DIR" && pwd)"
STAGING_DIR="$OUT_DIR/staging"
ROOTFS_DIR="$STAGING_DIR/cohesix/bin"
HOST_OUT_DIR="$OUT_DIR/host-tools"
CPIO_PATH="$OUT_DIR_ABS/cohesix-system.cpio"

rm -rf "$STAGING_DIR"
mkdir -p "$ROOTFS_DIR" "$HOST_OUT_DIR"

for bin in "${COMPONENT_BINS[@]}"; do
    SRC="$SEL4_ARTIFACT_DIR/$bin"
    [[ -f "$SRC" ]] || fail "Expected binary not found: $SRC"
    install -m 0755 "$SRC" "$ROOTFS_DIR/$bin"
    log "Packaged component binary: $ROOTFS_DIR/$bin"
done

for bin in "${HOST_ONLY_BINS[@]}"; do
    SRC="$HOST_ARTIFACT_DIR/$bin"
    if [[ -f "$SRC" ]]; then
        install -m 0755 "$SRC" "$HOST_OUT_DIR/$bin"
        log "Copied host-side tool: $HOST_OUT_DIR/$bin"
    else
        log "Host tool not built for target $HOST_ARTIFACT_DIR: $bin (skipping)"
    fi
done

KERNEL_STAGE_PATH="$STAGING_DIR/kernel.elf"
ROOTSERVER_STAGE_PATH="$STAGING_DIR/rootserver"

install -m 0755 "$KERNEL_PATH" "$KERNEL_STAGE_PATH"
install -m 0755 "$ROOTFS_DIR/root-task" "$ROOTSERVER_STAGE_PATH"

describe_file "seL4 kernel" "$KERNEL_STAGE_PATH"
describe_file "Root server" "$ROOTSERVER_STAGE_PATH"

RESOLVED_TARGET="$CARGO_TARGET"
if [[ -z "$RESOLVED_TARGET" ]]; then
    RESOLVED_TARGET=$(rustc -vV 2>/dev/null | awk '/host:/ {print $2}')
fi

MANIFEST_INPUTS=()
for bin in "${COMPONENT_BINS[@]}"; do
    MANIFEST_INPUTS+=("cohesix/bin/$bin")
done

python3 - "$STAGING_DIR" "$PROFILE" "$RESOLVED_TARGET" "${MANIFEST_INPUTS[@]}" <<'PY'
import hashlib
import json
import pathlib
import sys

if len(sys.argv) < 5:
    raise SystemExit("manifest generation requires staging dir, profile, target, and at least one binary")

staging = pathlib.Path(sys.argv[1])
profile = sys.argv[2]
target = sys.argv[3]
entries = []
for rel_path in sys.argv[4:]:
    path = staging / rel_path
    data = path.read_bytes()
    entries.append({
        "name": path.name,
        "path": rel_path,
        "size": path.stat().st_size,
        "sha256": hashlib.sha256(data).hexdigest(),
    })
manifest = {
    "profile": profile,
    "target": target,
    "binaries": entries,
}
manifest_path = staging / "cohesix" / "manifest.json"
manifest_path.parent.mkdir(parents=True, exist_ok=True)
manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
PY

log "Manifest written to $STAGING_DIR/cohesix/manifest.json"

pushd "$STAGING_DIR" >/dev/null
log "Creating payload archive at $CPIO_PATH"
find . -print | LC_ALL=C sort | cpio -o -H newc > "$CPIO_PATH"
popd >/dev/null

describe_file "Payload CPIO" "$CPIO_PATH"

if [[ -f scripts/ci/size_guard.sh ]]; then
    scripts/ci/size_guard.sh "$CPIO_PATH"
else
    log "Size guard script not found; skipping payload size check"
fi

KERNEL_LOAD_ADDR=0x70000000
ROOTSERVER_LOAD_ADDR=0x80000000

GIC_VER="$(detect_gic_version)"
log "Auto-detected GIC version: gic-version=$GIC_VER"

QEMU_ARGS=(-machine "virt,gic-version=${GIC_VER}" -cpu cortex-a57 -m 1024 -smp 1 -serial mon:stdio -display none -kernel "$ELFLOADER_PATH" -initrd "$CPIO_PATH" -device loader,file="$KERNEL_STAGE_PATH",addr=$KERNEL_LOAD_ADDR,force-raw=on -device loader,file="$ROOTSERVER_STAGE_PATH",addr=$ROOTSERVER_LOAD_ADDR,force-raw=on)

if [[ -n "$DTB_OVERRIDE" ]]; then
    [[ -f "$DTB_OVERRIDE" ]] || fail "Specified DTB override not found: $DTB_OVERRIDE"
    describe_file "DTB override" "$DTB_OVERRIDE"
    QEMU_ARGS+=(-dtb "$DTB_OVERRIDE")
fi

if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
    QEMU_ARGS+=("${EXTRA_QEMU_ARGS[@]}")
fi

log "Prepared QEMU command: ${QEMU_ARGS[*]}"

if [[ "$RUN_QEMU" -eq 0 ]]; then
    log "--no-run supplied; build artefacts ready at $OUT_DIR"
    exit 0
fi

exec "$QEMU_BIN" "${QEMU_ARGS[@]}"
