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
        --no-run)
            RUN_QEMU=0
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
DTB_PATH="$SEL4_BUILD_DIR/qemu-arm-virt.dtb"

[[ -f "$ELFLOADER_PATH" ]] || fail "elfloader binary not found at $ELFLOADER_PATH"
[[ -f "$KERNEL_PATH" ]] || fail "kernel.elf not found at $KERNEL_PATH"

if [[ ! -f "$DTB_PATH" ]]; then
    log "DTB not found at $DTB_PATH; continuing without explicit -dtb"
    DTB_PATH=""
else
    describe_file "Device tree" "$DTB_PATH"
fi

PROFILE_FLAG=()
PROFILE_DIR="$PROFILE"
case "$PROFILE" in
    release)
        PROFILE_FLAG=(--release)
        PROFILE_DIR="release"
        ;;
    dev|debug)
        PROFILE_FLAG=()
        PROFILE_DIR="debug"
        ;;
    *)
        PROFILE_FLAG=(--profile "$PROFILE")
        ;;
 esac

if [[ -z "$CARGO_TARGET" ]]; then
    fail "--cargo-target must be provided to build seL4 components"
fi

SEL4_COMPONENT_PACKAGES=(root-task nine-door worker-heart worker-gpu)
HOST_TOOL_PACKAGES=(cohsh gpu-bridge-host)

HOST_BUILD_ARGS=(build)
HOST_BUILD_ARGS+=("${PROFILE_FLAG[@]}")
for pkg in "${HOST_TOOL_PACKAGES[@]}"; do
    HOST_BUILD_ARGS+=(-p "$pkg")
done

log "Building host tooling via: cargo ${HOST_BUILD_ARGS[*]}"
cargo "${HOST_BUILD_ARGS[@]}"

SEL4_BUILD_ARGS=(build --target "$CARGO_TARGET")
SEL4_BUILD_ARGS+=("${PROFILE_FLAG[@]}")
for pkg in "${SEL4_COMPONENT_PACKAGES[@]}"; do
    SEL4_BUILD_ARGS+=(-p "$pkg")
done

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

DTB_LOAD_ADDR=0x4f000000
KERNEL_LOAD_ADDR=0x70000000
ROOTSERVER_LOAD_ADDR=0x80000000

GIC_VERSION="3"
GIC_CONFIG_SOURCE=""
SEL4_CONFIG_CANDIDATES=(
    "$SEL4_BUILD_DIR/.config"
    "$SEL4_BUILD_DIR/kernel/.config"
)

for cfg in "${SEL4_CONFIG_CANDIDATES[@]}"; do
    if [[ -f "$cfg" ]]; then
        if grep -q '^CONFIG_ARM_GIC_V3_SUPPORT=y' "$cfg"; then
            GIC_VERSION="3"
            GIC_CONFIG_SOURCE="$cfg"
            break
        fi
        if grep -q '^# CONFIG_ARM_GIC_V3_SUPPORT is not set' "$cfg"; then
            GIC_VERSION="2"
            GIC_CONFIG_SOURCE="$cfg"
            break
        fi
    fi
done

if [[ -n "$GIC_CONFIG_SOURCE" ]]; then
    if [[ "$GIC_VERSION" == "3" ]]; then
        log "Detected GICv3 support in $GIC_CONFIG_SOURCE"
    else
        log "Detected GICv3 support disabled in $GIC_CONFIG_SOURCE; using gic-version=2"
    fi
else
    log "Unable to infer GIC version from seL4 build configuration; defaulting to gic-version=$GIC_VERSION"
fi

QEMU_MACHINE_OPTS="virt,gic-version=$GIC_VERSION"
QEMU_DTB_ADDR_SUPPORTED=0

if command -v "$QEMU_BIN" >/dev/null 2>&1; then
    if "$QEMU_BIN" -machine virt,help 2>&1 | grep -q 'dtb-addr'; then
        QEMU_MACHINE_OPTS+="\,dtb-addr=$DTB_LOAD_ADDR"
        QEMU_DTB_ADDR_SUPPORTED=1
    else
        log "QEMU binary $QEMU_BIN does not advertise virt.dtb-addr; using default device tree placement"
    fi
fi

QEMU_CMD=("$QEMU_BIN" -machine "$QEMU_MACHINE_OPTS" -cpu cortex-a57 -m 1024 -serial mon:stdio -display none -kernel "$ELFLOADER_PATH" -initrd "$CPIO_PATH" -device loader,file="$KERNEL_STAGE_PATH",addr=$KERNEL_LOAD_ADDR,force-raw=on -device loader,file="$ROOTSERVER_STAGE_PATH",addr=$ROOTSERVER_LOAD_ADDR,force-raw=on)

if [[ -n "$DTB_PATH" ]]; then
    if [[ "$QEMU_DTB_ADDR_SUPPORTED" -eq 1 ]]; then
        log "Device tree will load at $DTB_LOAD_ADDR"
    else
        log "Device tree provided via -dtb; load address determined by QEMU"
    fi
    QEMU_CMD+=(-dtb "$DTB_PATH")
fi

if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
    QEMU_CMD+=("${EXTRA_QEMU_ARGS[@]}")
fi

log "Prepared QEMU command: ${QEMU_CMD[*]}"

if [[ "$RUN_QEMU" -eq 0 ]]; then
    log "--no-run supplied; build artefacts ready at $OUT_DIR"
    exit 0
fi

exec "${QEMU_CMD[@]}"
