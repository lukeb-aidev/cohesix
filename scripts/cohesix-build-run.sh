
#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HOST_OS="$(uname -s)"

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
  --root-task-features <list>
                        Comma-separated feature set used for the root-task seL4 build
                        (default: kernel,net)
  --features <name>      Enable additional root-task feature (bootstrap-trace|serial-console).
                         May be specified multiple times.
  --qemu <path>         QEMU binary to execute (default: qemu-system-aarch64)
  --transport <kind>    Console transport to launch (tcp|qemu, default: tcp)
  --tcp-port <port>     TCP port exposed by QEMU for the remote console (default: 31337)
  --cohsh-launch <mode> Launch cohsh inline, in a macOS background session, or auto-detect (auto|inline|macos-terminal)
  --no-run              Skip launching QEMU after building the artefacts
  --raw-qemu            Launch QEMU directly instead of cohsh (disables interactive CLI)
  --dtb <path>          Override the device tree blob passed to QEMU
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

append_root_task_feature() {
    local feature="$1"
    if [[ -z "$feature" ]]; then
        return
    fi

    if [[ "$ROOT_TASK_FEATURES" == "none" || -z "$ROOT_TASK_FEATURES" ]]; then
        ROOT_TASK_FEATURES="$feature"
        return
    fi

    case ",$ROOT_TASK_FEATURES," in
        *,"$feature",*) ;;
        *) ROOT_TASK_FEATURES="$ROOT_TASK_FEATURES,$feature" ;;
    esac
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

    [[ -n "$cfg_file" ]] || fail "cannot find seL4 config to infer GIC"

    local detect_script="$SCRIPT_DIR/lib/detect_gic_version.py"
    if [[ ! -x "$detect_script" ]]; then
        fail "helper missing or not executable: $detect_script"
    fi

    local result
    if ! result=$("$detect_script" "$cfg_file"); then
        fail "cannot infer GIC version from $cfg_file"
    fi

    if [[ -z "$result" ]]; then
        fail "cannot infer GIC version from $cfg_file"
    fi

    echo "$result"
}

wait_for_port() {
    local host="$1"
    local port="$2"
    local attempts="${3:-30}"
    for ((i = 0; i < attempts; i++)); do
        if python3 - "$host" "$port" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
try:
    with socket.create_connection((host, port), timeout=1):
        pass
except OSError:
    raise SystemExit(1)
PY
            then
            return 0
        fi
        sleep 1
    done
    fail "Timed out waiting for TCP port ${host}:${port}"
}

launch_cohsh_macos_terminal() {
    local host_tools_dir="$1"
    local cohsh_tcp_port="$2"

    if [[ "$HOST_OS" != "Darwin" ]]; then
        fail "macOS Terminal launch requested but host OS is not macOS"
    fi

    local cohsh_bin="$host_tools_dir/cohsh"
    if [[ ! -x "$cohsh_bin" ]]; then
        fail "cohsh binary not found or not executable: $cohsh_bin"
    fi

    export COHSH_TCP_PORT="$cohsh_tcp_port"
    "$cohsh_bin" --transport tcp --tcp-port "$cohsh_tcp_port" --role queen >/dev/null 2>&1 &

    log "Launched cohsh (tcp://127.0.0.1:${cohsh_tcp_port}) in background."
}

main() {
    SEL4_BUILD_DIR="${SEL4_BUILD:-$HOME/seL4/build}"
    OUT_DIR="out/cohesix"
    PROFILE="release"
    CARGO_TARGET=""
    QEMU_BIN="qemu-system-aarch64"
    RUN_QEMU=1
    DIRECT_QEMU=0
    declare -a EXTRA_QEMU_ARGS=()
    CLEAN_OUT_DIR=0
    DTB_OVERRIDE=""
    TRANSPORT="tcp"
    TCP_PORT=31337
    ROOT_TASK_FEATURES="kernel,net"

    COHSH_LAUNCH_MODE="auto"

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
            --root-task-features)
                [[ $# -ge 2 ]] || fail "--root-task-features requires a list"
                ROOT_TASK_FEATURES="$2"
                shift 2
                ;;
            --features)
                [[ $# -ge 2 ]] || fail "--features requires a value"
                case "$2" in
                    bootstrap-trace|serial-console)
                        append_root_task_feature "$2"
                        ;;
                    *)
                        fail "Unsupported feature requested via --features: $2"
                        ;;
                esac
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
            --raw-qemu)
                DIRECT_QEMU=1
                shift
                ;;
            --transport)
                [[ $# -ge 2 ]] || fail "--transport requires a value (tcp|qemu)"
                case "$2" in
                    tcp|qemu)
                        TRANSPORT="$2"
                        ;;
                    *)
                        fail "Unsupported transport: $2"
                        ;;
                esac
                shift 2
                ;;
            --tcp-port)
                [[ $# -ge 2 ]] || fail "--tcp-port requires a value"
                if ! [[ "$2" =~ ^[0-9]+$ ]]; then
                    fail "--tcp-port expects a numeric value"
                fi
                TCP_PORT="$2"
                shift 2
                ;;
            --cohsh-launch)
                [[ $# -ge 2 ]] || fail "--cohsh-launch requires a mode"
                case "$2" in
                    auto|inline|macos-terminal)
                        COHSH_LAUNCH_MODE="$2"
                        ;;
                    *)
                        fail "Unsupported cohsh launch mode: $2"
                        ;;
                esac
                shift 2
                ;;
            --clean)
                CLEAN_OUT_DIR=1
                shift
                ;;
            -h|--help)
                usage
                return 0
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

    if [[ "$ROOT_TASK_FEATURES" == "none" ]]; then
        ROOT_TASK_FEATURES=""
    fi

    if [[ "$TRANSPORT" == "qemu" || "$DIRECT_QEMU" -eq 1 ]]; then
        append_root_task_feature "serial-console"
    fi

    if [[ -n "$ROOT_TASK_FEATURES" ]]; then
        log "Final root-task feature set: $ROOT_TASK_FEATURES"
    else
        log "Final root-task feature set: <none>"
    fi

    if [[ "$TRANSPORT" == "tcp" && "$TCP_PORT" -le 0 ]]; then
        fail "TCP port must be a positive integer"
    fi

    if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
        fail "seL4 build directory not found: $SEL4_BUILD_DIR"
    fi

    export SEL4_BUILD_DIR
    export SEL4_BUILD="$SEL4_BUILD_DIR"

    local EFFECTIVE_COHSH_MODE="$COHSH_LAUNCH_MODE"
    if [[ "$EFFECTIVE_COHSH_MODE" == "auto" ]]; then
        if [[ "$TRANSPORT" == "tcp" && "$HOST_OS" == "Darwin" ]]; then
            EFFECTIVE_COHSH_MODE="macos-terminal"
        else
            EFFECTIVE_COHSH_MODE="inline"
        fi
    fi

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
    HOST_TOOL_PACKAGES=(gpu-bridge-host)

    HOST_BUILD_ARGS=(build)
    if (( ${#PROFILE_ARGS[@]} > 0 )); then
        HOST_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
    fi
    for pkg in "${HOST_TOOL_PACKAGES[@]}"; do
        HOST_BUILD_ARGS+=(-p "$pkg")
    done

    log "Building host tooling via: cargo ${HOST_BUILD_ARGS[*]}"
    cargo "${HOST_BUILD_ARGS[@]}"

    COHSH_BUILD_ARGS=(build)
    if (( ${#PROFILE_ARGS[@]} > 0 )); then
        COHSH_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
    fi
    COHSH_BUILD_ARGS+=(-p cohsh --features tcp)
    log "Building cohsh CLI with TCP transport via: cargo ${COHSH_BUILD_ARGS[*]}"
    cargo "${COHSH_BUILD_ARGS[@]}"

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
    ROOT_TASK_BUILD_ARGS+=(-p root-task --no-default-features)
    if [[ -n "$ROOT_TASK_FEATURES" ]]; then
        ROOT_TASK_BUILD_ARGS+=(--features "$ROOT_TASK_FEATURES")
    fi

    ROOT_TASK_LINKER_SCRIPT="$PROJECT_ROOT/apps/root-task/sel4.ld"
    if [[ ! -f "$ROOT_TASK_LINKER_SCRIPT" ]]; then
        fail "root-task linker script not found: $ROOT_TASK_LINKER_SCRIPT"
    fi

    log "Using root-task linker script: $ROOT_TASK_LINKER_SCRIPT"
    log "Building root-task via: cargo ${ROOT_TASK_BUILD_ARGS[*]}"
    SEL4_LD="$ROOT_TASK_LINKER_SCRIPT" cargo "${ROOT_TASK_BUILD_ARGS[@]}"

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
    HOST_TOOLS_DIR="$(cd "$HOST_OUT_DIR" && pwd)"
    HOST_TOOLS="$HOST_TOOLS_DIR"

    ELFLOADER_STAGE_PATH="$STAGING_DIR/elfloader"
    if [[ ! -f "$SCRIPT_DIR/lib/strip_elfloader_modules.py" ]]; then
        fail "helper missing: $SCRIPT_DIR/lib/strip_elfloader_modules.py"
    fi
    python3 "$SCRIPT_DIR/lib/strip_elfloader_modules.py" \
        --rootserver "$SEL4_ARTIFACT_DIR/root-task" \
        "$ELFLOADER_PATH" \
        "$ELFLOADER_STAGE_PATH"
    describe_file "Sanitised elfloader" "$ELFLOADER_STAGE_PATH"

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
    log "Packaged component binary: $ROOTSERVER_STAGE_PATH"
    if [[ -f "$ROOTSERVER_STAGE_PATH" ]]; then
        shasum -a 256 "$ROOTSERVER_STAGE_PATH" | awk '{print "[cohesix-build] rootserver sha256=" $1}'
    fi

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
    if [[ ! -d cohesix ]]; then
        fail "Rootfs directory missing from staging area: $STAGING_DIR/cohesix"
    fi
    find cohesix -print | LC_ALL=C sort | cpio -o -H newc > "$CPIO_PATH"
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

    QEMU_ARGS=(-machine "virt,gic-version=${GIC_VER}" -cpu cortex-a57 -m 1024 -smp 1 -serial mon:stdio -display none -kernel "$ELFLOADER_STAGE_PATH" -initrd "$CPIO_PATH" -device loader,file="$KERNEL_STAGE_PATH",addr=$KERNEL_LOAD_ADDR,force-raw=on -device loader,file="$ROOTSERVER_STAGE_PATH",addr=$ROOTSERVER_LOAD_ADDR,force-raw=on)
    declare -a CLI_EXTRA_ARGS=()

    if [[ "$TRANSPORT" == "tcp" ]]; then
        NETWORK_ARGS=(-netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${TCP_PORT}-10.0.2.15:${TCP_PORT}" -device virtio-net-device,netdev=net0)
        QEMU_ARGS+=("${NETWORK_ARGS[@]}")
    fi

    if [[ -n "$DTB_OVERRIDE" ]]; then
        [[ -f "$DTB_OVERRIDE" ]] || fail "Specified DTB override not found: $DTB_OVERRIDE"
        describe_file "DTB override" "$DTB_OVERRIDE"
        QEMU_ARGS+=(-dtb "$DTB_OVERRIDE")
        CLI_EXTRA_ARGS+=(-dtb "$DTB_OVERRIDE")
    fi

    if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
        CLI_EXTRA_ARGS+=("${EXTRA_QEMU_ARGS[@]}")
        QEMU_ARGS+=("${EXTRA_QEMU_ARGS[@]}")
    fi

    log "Prepared QEMU command: ${QEMU_ARGS[*]}"

    if [[ "$RUN_QEMU" -eq 0 ]]; then
        log "--no-run supplied; build artefacts ready at $OUT_DIR"
        return 0
    fi

    if [[ "$DIRECT_QEMU" -eq 1 ]]; then
        exec "$QEMU_BIN" "${QEMU_ARGS[@]}"
    fi

    COHSH_BIN="$HOST_TOOLS_DIR/cohsh"

    if [[ "$TRANSPORT" == "tcp" ]]; then
        log "Launching QEMU with TCP console bridge on port $TCP_PORT"
        "$QEMU_BIN" "${QEMU_ARGS[@]}" &
        QEMU_PID=$!
        trap 'kill $QEMU_PID 2>/dev/null || true' EXIT

        wait_for_port "127.0.0.1" "$TCP_PORT" 60
        export COHSH_TCP_PORT="$TCP_PORT"
        if [[ ! -x "$COHSH_BIN" ]]; then
            fail "cohsh CLI not found: $COHSH_BIN"
        fi

        CLI_CMD=("$COHSH_BIN" --transport tcp --tcp-port "$TCP_PORT" --role queen)
        case "$EFFECTIVE_COHSH_MODE" in
            inline)
                log "Launching cohsh inline (TCP transport) for interactive session"
                "${CLI_CMD[@]}"
                STATUS=$?
                kill "$QEMU_PID" 2>/dev/null || true
                wait "$QEMU_PID" 2>/dev/null || true
                trap - EXIT
                exit $STATUS
                ;;
            macos-terminal)
                log "Launching cohsh in background (TCP transport)"
                launch_cohsh_macos_terminal "$HOST_TOOLS_DIR" "$TCP_PORT"
                log "cohsh started in a background session. Press Ctrl+C here to stop QEMU when finished."
                local QEMU_EXIT=0
                if ! wait "$QEMU_PID" 2>/dev/null; then
                    QEMU_EXIT=$?
                fi
                trap - EXIT
                return $QEMU_EXIT
                ;;
            *)
                fail "Unknown cohsh launch mode: $EFFECTIVE_COHSH_MODE"
                ;;
        esac
    else
        log "Launching cohsh (QEMU transport) for interactive session"
        if [[ ! -x "$COHSH_BIN" ]]; then
            fail "cohsh CLI not found: $COHSH_BIN"
        fi
        CLI_CMD=("$COHSH_BIN" --transport qemu --qemu-bin "$QEMU_BIN" --qemu-out-dir "$OUT_DIR" --qemu-gic-version "$GIC_VER" --role queen)
        if [[ ${#CLI_EXTRA_ARGS[@]} -gt 0 ]]; then
            for arg in "${CLI_EXTRA_ARGS[@]}"; do
                CLI_CMD+=(--qemu-arg "$arg")
            done
        fi
        exec "${CLI_CMD[@]}"
    fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
