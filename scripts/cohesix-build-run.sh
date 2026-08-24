#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build and stage Cohesix artefacts, including rootfs payloads, for QEMU runs.
# Copyright 2026 Lukas Bower

set -euo pipefail
SEL4_LD="${SEL4_LD:-}"
declare -a EXTRA_QEMU_ARGS=()

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GENERATED_CONFIG_DIR="$PROJECT_ROOT/configs/generated"
CANONICAL_QEMU_PROFILE="qemu_smp_production"
CANONICAL_QEMU_BUILD_DIR="$PROJECT_ROOT/out/sel4/profile-v2/qemu-smp-production"
CANONICAL_QEMU_KVM_PROFILE="qemu_smp_kvm_production"
CANONICAL_QEMU_KVM_BUILD_DIR="$PROJECT_ROOT/out/sel4/profile-v2/qemu-smp-kvm-production"
CANONICAL_QEMU_EMULATED_CPU="cortex-a57"
CANONICAL_QEMU_KVM_CPU="host"
CANONICAL_QEMU_TIMER_CLOCK_HZ="24000000"
QEMU_TIMER_CLOCK_HZ="$CANONICAL_QEMU_TIMER_CLOCK_HZ"
CANONICAL_QEMU_VIRT="off"
HOST_OS="$(uname -s)"
QEMU_MACHINE_EXTRA="${COHESIX_QEMU_MACHINE_EXTRA:-${QEMU_MACHINE_EXTRA:-}}"
if [[ -z "$QEMU_MACHINE_EXTRA" && "$HOST_OS" == "Darwin" ]]; then
    QEMU_MACHINE_EXTRA="kernel-irqchip=off"
fi
QEMU_VIRT_RAW="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-}}"
CROSS_HOST_REPLAY="${COHESIX_QEMU_CROSS_HOST_REPLAY:-0}"
if [[ -z "$QEMU_VIRT_RAW" ]]; then
    QEMU_VIRT_RAW="$CANONICAL_QEMU_VIRT"
fi

usage() {
    cat <<'USAGE'
Usage: scripts/cohesix-build-run.sh [options] [-- <extra-qemu-args>]

Build the Cohesix Rust workspace, assemble the seL4 payload CPIO archive, and
boot the system under QEMU. The script expects an existing seL4 build tree that
already produced `elfloader`, `kernel.elf`, and support artefacts. By default it
uses the validated `qemu_smp_production` contract at
`$PROJECT_ROOT/out/sel4/profile-v2/qemu-smp-production`.
That contract must report GICv3. Launch uses the profile-owned
`virt,gic-version=3,virtualization=off` machine with `cortex-a57` under macOS
HVF or the KVM `host` CPU under Linux, and rejects command-line or environment
attempts to replace that host-specific truth.

Options:
  --sel4-build <dir>    Path to the seL4 build output
                        (default: $PROJECT_ROOT/out/sel4/profile-v2/qemu-smp-production)
  --out-dir <dir>       Directory for generated artefacts (default: out/cohesix)
  --clean               Remove existing contents of the output directory before building
  --profile <name>      Cargo profile to build (release|debug|custom; default: release)
  --cargo-target <triple>  Target triple used for seL4 component builds (required)
  --root-task-features <list>
                        Comma-separated feature set used for the root-task seL4 build
                        (default: release-qemu,bootstrap-trace for both transports)
  --features <name>      Enable additional root-task feature (bootstrap-trace|serial-console|cohesix-dev).
                         May be specified multiple times.
  --qemu <path>         QEMU binary to execute (default: qemu-system-aarch64)
  --transport <kind>    Console transport to launch (tcp|qemu, default: tcp)
                        tcp: run QEMU here with PL011 serial console and TCP console listener;
                             connect from another terminal via cohsh --transport tcp.
                        qemu: run cohsh using its QEMU transport; cohsh manages QEMU and no
                              TCP console is exposed to the host by default.
  --tcp-port <port>     TCP port exposed by QEMU for the remote console (default: 31337)
  --raw-qemu            Launch QEMU directly in this terminal after building (bypasses cohsh)
  --launch-existing     Validate and launch the immutable artefacts from one prior build;
                        do not rebuild, restage, or repack the QEMU inputs
  --no-run              Skip launching QEMU after building the artefacts
  --dtb <path>          Override the device tree blob passed to QEMU
  -h, --help            Show this help message

Any arguments following `--` are forwarded directly to QEMU (or passed through
to cohsh via --qemu-arg when --transport qemu is selected).

Env overrides:
  COHESIX_QEMU_SMP / QEMU_SMP (default: 4; ignored when *_QEMU_SMP_TOPO is set)
  COHESIX_QEMU_SMP_TOPO / QEMU_SMP_TOPO (default: 4,cores=4,threads=1,sockets=1)
  COHESIX_QEMU_VIRT / QEMU_VIRT (must be off; profile-owned machine contract)
  COHESIX_QEMU_ACCEL / QEMU_ACCEL (default: hvf on macOS, kvm on Linux;
                        explicit tcg is diagnostic and claim-ineligible)
  COHESIX_QEMU_MACHINE_EXTRA / QEMU_MACHINE_EXTRA (appended to -machine)
                        (must not override machine type, GIC, or virtualization)
  COHESIX_QEMU_CROSS_HOST_REPLAY (0|1; default: 0)
                        Allow Linux to replay immutable Mac-built guest inputs;
                        valid only with --launch-existing and a rebound record
  COHESIX_SEL4_PROFILE (qemu_smp_production|qemu_smp_kvm_production|
                        qemu_smp_diagnostic; validates an explicitly selected
                        build tree against that contract)
  COHESIX_DRIVER_CLASSIC_COMPARATOR_RECORD (immutable comparator record;
                        defaults to configs/driver_runtime_classic_comparator.toml)
USAGE
}

log() {
    echo "[cohesix-build] $*"
}

fail() {
    echo "[cohesix-build] error: $*" >&2
    exit 1
}

validate_selected_qemu_profile() {
    local profile_name="$1"
    local profile_python="$PROJECT_ROOT/out/toolchain/sel4-profile-venv/bin/python"
    local profile_tool="$SCRIPT_DIR/sel4_profile.py"

    case "$profile_name" in
        qemu_smp_production|qemu_smp_kvm_production|qemu_smp_diagnostic)
            ;;
        *)
            fail "Unsupported QEMU seL4 profile contract: $profile_name"
            ;;
    esac
    [[ -x "$profile_python" ]] || fail \
        "canonical seL4 profile Python is missing: $profile_python (run toolchain/setup_macos_arm64.sh)"
    [[ -f "$profile_tool" ]] || fail "seL4 profile validator is missing: $profile_tool"

    log "Validating seL4 profile contract: $profile_name"
    "$profile_python" "$profile_tool" validate \
        --profile "$profile_name" \
        --build-dir "$SEL4_BUILD_DIR" \
        --require-source \
        --require-artifacts \
        --for-runtime \
        || fail "seL4 build does not satisfy profile contract $profile_name"
}

qemu_args_have_accel() {
    local arg
    for arg in "$@"; do
        if [[ "$arg" == "-accel" ]]; then
            return 0
        fi
        if [[ "$arg" == *"accel="* ]]; then
            return 0
        fi
    done
    return 1
}

qemu_args_accel_value() {
    local expect_value=0
    local arg
    local value
    for arg in "$@"; do
        if [[ "$expect_value" -eq 1 ]]; then
            value="${arg%%,*}"
            [[ -n "$value" ]] || fail "QEMU -accel requires a non-empty value"
            echo "$value"
            return 0
        fi
        case "$arg" in
            -accel)
                expect_value=1
                ;;
            -accel=*)
                value="${arg#-accel=}"
                value="${value%%,*}"
                [[ -n "$value" ]] || fail "QEMU -accel requires a non-empty value"
                echo "$value"
                return 0
                ;;
        esac
    done
    if [[ "$expect_value" -eq 1 ]]; then
        fail "QEMU -accel requires a value"
    fi
    return 1
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
            echo "kvm"
            ;;
        *)
            echo "unsupported"
            ;;
    esac
}

has_kvm_device() {
    [[ -c /dev/kvm && -r /dev/kvm && -w /dev/kvm ]]
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
    [[ -n "$accel" && "$accel" != "unsupported" ]] || \
        fail "QEMU acceleration is unsupported on $HOST_OS"
    if [[ "$accel" == "kvm" && "$HOST_OS" == "Linux" ]]; then
        if ! has_kvm_device; then
            fail "Linux QEMU requires usable /dev/kvm; select tcg explicitly only for diagnostic evidence"
        fi
    fi
    if ! qemu_accel_supported "$accel"; then
        if [[ "$HOST_OS" == "Darwin" && "$accel" == "hvf" ]]; then
            fail "canonical Darwin QEMU requires HVF, but $QEMU_BIN does not advertise it; set COHESIX_QEMU_ACCEL=tcg only for a claim-ineligible diagnostic run"
        fi
        fail "Requested QEMU accelerator '$accel' is not supported by $QEMU_BIN"
    fi
    echo "$accel"
}

resolve_qemu_cpu_model() {
    local accel="$1"
    if [[ "$HOST_OS" == "Linux" && "$accel" == "kvm" ]]; then
        echo "$CANONICAL_QEMU_KVM_CPU"
        return
    fi
    echo "$CANONICAL_QEMU_EMULATED_CPU"
}

resolve_qemu_cpu_arg() {
    local accel="$1"
    local cpu_model
    cpu_model="$(resolve_qemu_cpu_model "$accel")"
    if [[ "$accel" == "tcg" ]]; then
        cpu_model="${cpu_model},cntfrq=${QEMU_TIMER_CLOCK_HZ}"
    fi
    echo "$cpu_model"
}

read_selected_timer_clock_hz() {
    local platform_header="$1"
    python3 - "$platform_header" <<'PY'
from pathlib import Path
import re
import sys

header_path = Path(sys.argv[1])
try:
    header = header_path.read_text(encoding="utf-8")
except (OSError, UnicodeError) as error:
    raise SystemExit(f"cannot read selected seL4 timer header: {error}") from error
matches = re.findall(
    r"^\s*#define\s+TIMER_CLOCK_HZ\s+(?:ULL_CONST\(\s*)?([0-9]+)(?:\s*\))?\s*$",
    header,
    flags=re.MULTILINE,
)
if len(matches) != 1 or int(matches[0]) < 1:
    raise SystemExit(
        "selected seL4 timer header must define one positive TIMER_CLOCK_HZ"
    )
print(int(matches[0]))
PY
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

remove_root_task_feature() {
    local feature="$1"
    if [[ -z "${ROOT_TASK_FEATURES:-}" ]]; then
        return
    fi

    local padded=",${ROOT_TASK_FEATURES},"
    padded="${padded//,$feature,/}"
    while [[ "$padded" == *",,"* ]]; do
        padded="${padded//,,/,}"
    done
    padded="${padded#,}"
    padded="${padded%,}"
    ROOT_TASK_FEATURES="$padded"
}

has_root_task_feature() {
    local feature="$1"
    if [[ -z "${ROOT_TASK_FEATURES:-}" ]]; then
        return 1
    fi

    case ",${ROOT_TASK_FEATURES}," in
        *,"$feature",*) return 0 ;;
        *) return 1 ;;
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

resolve_qemu_smp_arg() {
    if [[ -n "$QEMU_SMP_TOPO_RAW" ]]; then
        echo "$QEMU_SMP_TOPO_RAW"
        return
    fi
    if [[ -n "$QEMU_SMP_RAW" ]]; then
        echo "$QEMU_SMP_RAW"
        return
    fi
    echo "$DEFAULT_QEMU_SMP_TOPO"
}

resolve_qemu_virt_arg() {
    if [[ -n "$QEMU_VIRT_RAW" ]]; then
        echo "$QEMU_VIRT_RAW"
        return
    fi
    echo "$CANONICAL_QEMU_VIRT"
}

validate_qemu_virt_arg() {
    local arg="$1"

    [[ "$arg" == "$CANONICAL_QEMU_VIRT" ]] || \
        fail "selected QEMU profile requires virtualization=off; got ${arg}"
}

validate_generated_timer_clock() {
    local resolved_manifest="$1"
    local platform_header="$2"
    local timer_clock_hz

    if ! timer_clock_hz="$(python3 - "$resolved_manifest" "$platform_header" <<'PY'
import json
from pathlib import Path
import re
import sys

manifest_path = Path(sys.argv[1])
header_path = Path(sys.argv[2])
try:
    document = json.loads(manifest_path.read_text(encoding="utf-8"))
except (OSError, UnicodeError, json.JSONDecodeError) as error:
    raise SystemExit(f"cannot read resolved root-task manifest: {error}") from error

service = document.get("console_network_service")
if not isinstance(service, dict):
    raise SystemExit("resolved manifest has no console_network_service object")
manifest_hz = service.get("timer_clock_hz")
if (
    not isinstance(manifest_hz, int)
    or isinstance(manifest_hz, bool)
    or manifest_hz < 1
):
    raise SystemExit("resolved manifest timer_clock_hz must be a positive integer")

try:
    header = header_path.read_text(encoding="utf-8")
except (OSError, UnicodeError) as error:
    raise SystemExit(f"cannot read selected seL4 timer header: {error}") from error
matches = re.findall(
    r"^\s*#define\s+TIMER_CLOCK_HZ\s+(?:ULL_CONST\(\s*)?([0-9]+)(?:\s*\))?\s*$",
    header,
    flags=re.MULTILINE,
)
if len(matches) != 1:
    raise SystemExit(
        "selected seL4 timer header must define TIMER_CLOCK_HZ exactly once"
    )
kernel_hz = int(matches[0])
if manifest_hz != kernel_hz:
    raise SystemExit(
        "timer clock mismatch: "
        f"console_network_service.timer_clock_hz={manifest_hz}, "
        f"selected seL4 TIMER_CLOCK_HZ={kernel_hz}"
    )
print(kernel_hz)
PY
)"; then
        fail "generated console-network timer clock does not match selected seL4"
    fi
    log "Validated console-network/seL4 timer clock: ${timer_clock_hz} Hz"
}

validate_qemu_smp_arg() {
    local arg="$1"

    if [[ -z "$arg" ]]; then
        fail "QEMU SMP setting is empty"
    fi

    if [[ "$arg" =~ ^[0-9]+$ ]]; then
        if [[ "$arg" -lt 1 ]]; then
            fail "QEMU_SMP must be a positive integer (got ${arg})"
        fi
        return
    fi

    if [[ "$arg" == *" "* ]]; then
        fail "QEMU SMP topology may not contain spaces (${arg})"
    fi

    local token
    IFS=',' read -r -a tokens <<< "$arg"
    for token in "${tokens[@]}"; do
        if [[ "$token" =~ ^[0-9]+$ ]]; then
            if [[ "$token" -lt 1 ]]; then
                fail "QEMU SMP topology token must be >= 1 (${token})"
            fi
            continue
        fi
        if [[ "$token" =~ ^[A-Za-z][A-Za-z0-9_-]*=[0-9]+$ ]]; then
            local value="${token#*=}"
            if [[ "$value" -lt 1 ]]; then
                fail "QEMU SMP topology token must be >= 1 (${token})"
            fi
            continue
        fi
        fail "Invalid QEMU SMP topology token (${token})"
    done
}

wait_for_port() {
    local host="$1"
    local port="$2"
    local timeout="${3:-30}"

    python3 - "$host" "$port" "$timeout" <<'PY'
import socket
import sys
import time

host = sys.argv[1]
port = int(sys.argv[2])
deadline = time.time() + float(sys.argv[3])

while time.time() < deadline:
    try:
        with socket.create_connection((host, port), timeout=1):
            sys.exit(0)
    except OSError:
        time.sleep(0.1)

print(f"[cohesix-build] error: timed out waiting for {host}:{port}", file=sys.stderr)
sys.exit(1)
PY
}

wait_for_port_or_exit() {
    local host="$1"
    local port="$2"
    local timeout="$3"
    local pid="$4"

    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if python3 - "$host" "$port" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
try:
    with socket.create_connection((host, port), timeout=0.5):
        sys.exit(0)
except OSError:
    sys.exit(1)
PY
        then
            return 0
        fi
        sleep 0.2
    done

    return 2
}

build_network_args() {
    local smoke_port="$1"

    NETWORK_ARGS=(
        -netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${TCP_PORT}-:31337,hostfwd=udp:127.0.0.1:${UDP_ECHO_PORT}-:31338,hostfwd=tcp:127.0.0.1:${smoke_port}-:31339"
    )

    if [[ "${NET_BACKEND}" == "virtio" ]]; then
        NETWORK_ARGS+=(
            -device "virtio-net-device,netdev=net0,mac=52:55:00:d1:55:01,bus=virtio-mmio-bus.0"
        )
    else
        NETWORK_ARGS+=(
            -device "rtl8139,netdev=net0,mac=52:55:00:d1:55:01"
        )
    fi
}

log_tcp_hostfwd() {
    local smoke_port="$1"

    log "Hostfwd: tcp 127.0.0.1:${TCP_PORT} -> 10.0.2.15:31337"
    log "Hostfwd: udp 127.0.0.1:${UDP_ECHO_PORT} -> 10.0.2.15:31338"
    log "Hostfwd: tcp 127.0.0.1:${smoke_port} -> 10.0.2.15:31339"
    log "Note: 10.0.2.15 is not directly reachable from the host under slirp"
    log "sudo tcpdump -i lo0 -n 'tcp port ${TCP_PORT} or udp port ${UDP_ECHO_PORT} or tcp port ${smoke_port}'"
}

print_tcp_summary() {
    local smoke_port="$1"

    log "Using smoke host port: ${smoke_port} (guest :31339)"
    log "TCP console: nc -v 127.0.0.1 ${TCP_PORT}"
    log "UDP echo: echo -n \"ping\" | nc -u -w1 127.0.0.1 ${UDP_ECHO_PORT}"
    log "TCP smoke: printf \"hi\" | nc -v 127.0.0.1 ${smoke_port}"
}

run_qemu_attempt() {
    local smoke_port="$1"
    local log_file="$2"
    local fifo_path
    local tee_pid

    QEMU_ARGS=("${BASE_QEMU_ARGS[@]}")
    if [[ "$TRANSPORT" == "tcp" ]]; then
        build_network_args "$smoke_port"
        QEMU_ARGS+=("${NETWORK_ARGS[@]}")
        log_tcp_hostfwd "$smoke_port"
    fi

    if [[ -n "$DTB_OVERRIDE" ]]; then
        [[ -f "$DTB_OVERRIDE" ]] || fail "Specified DTB override not found: $DTB_OVERRIDE"
        describe_file "DTB override" "$DTB_OVERRIDE"
        QEMU_ARGS+=(-dtb "$DTB_OVERRIDE")
    fi

    if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
        QEMU_ARGS+=("${EXTRA_QEMU_ARGS[@]}")
    fi

    log "Prepared QEMU command: ${QEMU_ARGS[*]}"

    fifo_path="$(mktemp -t cohesix-qemu.fifo)"
    rm -f "$fifo_path"
    mkfifo "$fifo_path"
    tee "$log_file" < "$fifo_path" &
    tee_pid=$!
    "$QEMU_BIN" "${QEMU_ARGS[@]}" > "$fifo_path" 2>&1 &
    QEMU_PID=$!
    trap 'kill $QEMU_PID 2>/dev/null || true' EXIT

    if wait_for_port_or_exit "127.0.0.1" "$TCP_PORT" 60 "$QEMU_PID"; then
        rm -f "$fifo_path"
        return 0
    fi

    local wait_status=$?
    if ! kill -0 "$QEMU_PID" 2>/dev/null; then
        wait "$QEMU_PID" || true
    fi
    wait "$tee_pid" 2>/dev/null || true
    rm -f "$fifo_path"

    case "$wait_status" in
        2)
            log "TCP console did not become ready on port $TCP_PORT"
            ;;
    esac
    return 1
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

qemu_arg_replaces_immutable_input() {
    local arg="$1"
    case "$arg" in
        -kernel|--kernel|-initrd|--initrd|-bios|--bios|-dtb|--dtb|-append|--append|-smp|--smp|-cpu|--cpu|-m|--m)
            return 0
            ;;
        -kernel=*|--kernel=*|-initrd=*|--initrd=*|-bios=*|--bios=*|-dtb=*|--dtb=*|-append=*|--append=*|-smp=*|--smp=*|-cpu=*|--cpu=*|-m=*|--m=*|loader,*|-device=loader,*|--device=loader,*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

validate_gicv3_override_safety() {
    local arg
    local -a cohsh_args=()
    if [[ "$QEMU_MACHINE_EXTRA" == *"gic-version"* \
        || "$QEMU_MACHINE_EXTRA" == *"virtualization"* \
        || "$QEMU_MACHINE_EXTRA" == *"virt,"* \
        || "$QEMU_MACHINE_EXTRA" == *"machine="* \
        || "$QEMU_MACHINE_EXTRA" == *"type="* ]]; then
        fail "QEMU machine extras must not override virt, gic-version, or virtualization; the machine is profile-owned"
    fi
    # Bash 3.2 on macOS treats an empty array as unset under `set -u`; the
    # `+` expansion preserves zero iterations while retaining argument bounds.
    for arg in "${EXTRA_QEMU_ARGS[@]+"${EXTRA_QEMU_ARGS[@]}"}"; do
        if [[ "${LAUNCH_EXISTING:-0}" -eq 1 ]] && \
            qemu_arg_replaces_immutable_input "$arg"; then
            fail "--launch-existing extra arguments must not replace immutable QEMU inputs or topology"
        fi
        case "$arg" in
            -machine|--machine|-machine=*|--machine=*|-M|-M*|*gic-version*|*virtualization*|-cpu|--cpu|-cpu=*|--cpu=*)
                fail "extra QEMU arguments must not override the profile-owned virt,gic-version=3 machine or CPU"
                ;;
        esac
    done
    if [[ -n "${COHSH_QEMU_ARGS:-}" ]]; then
        read -r -a cohsh_args <<< "${COHSH_QEMU_ARGS}"
        for arg in "${cohsh_args[@]+"${cohsh_args[@]}"}"; do
            if [[ "${LAUNCH_EXISTING:-0}" -eq 1 ]] && \
                qemu_arg_replaces_immutable_input "$arg"; then
                fail "--launch-existing COHSH_QEMU_ARGS must not replace immutable QEMU inputs or topology"
            fi
            case "$arg" in
                -machine|--machine|-machine=*|--machine=*|-M|-M*|*gic-version*|*virtualization*|-cpu|--cpu|-cpu=*|--cpu=*)
                    fail "COHSH_QEMU_ARGS must not override the profile-owned virt,gic-version=3 machine or CPU"
                    ;;
            esac
        done
    fi
}

launch_qemu_artifacts() {
    local size_guard_path="$PROJECT_ROOT/scripts/ci/size_guard.sh"
    [[ -x "$size_guard_path" ]] || fail \
        "Mandatory payload size guard is missing or not executable: $size_guard_path"
    "$size_guard_path" "$CPIO_PATH"

    KERNEL_LOAD_ADDR=0x70000000
    ROOTSERVER_LOAD_ADDR=0x80000000
    [[ "$GIC_VER" == "3" ]] || fail \
        "selected operational QEMU build must use GICv3, detected gic-version=$GIC_VER"
    validate_gicv3_override_safety
    log "Validated immutable QEMU launch record: $LAUNCH_ARTIFACT_RECORD"
    log "Auto-detected GIC version: gic-version=$GIC_VER"
    log "Using QEMU SMP: $QEMU_SMP_ARG"
    log "Using QEMU virtualization: ${QEMU_VIRT_ARG}"
    local machine_arg="virt,gic-version=${GIC_VER},virtualization=${QEMU_VIRT_ARG}"
    if [[ -n "$QEMU_MACHINE_EXTRA" ]]; then
        machine_arg="${machine_arg},${QEMU_MACHINE_EXTRA}"
    fi
    log "Using QEMU machine: ${machine_arg}"

    # Serial output from the PL011 console and root-task logger is expected on
    # stdio via -serial mon:stdio; keep this wiring intact for every launch.
    local cpu_arg
    cpu_arg="$(resolve_qemu_cpu_arg "$QEMU_ACCEL")"
    log "Using QEMU CPU: ${cpu_arg}"
    BASE_QEMU_ARGS=("${ACCEL_ARGS[@]}" -machine "${machine_arg}" -cpu "$cpu_arg" -m 1024 -smp "$QEMU_SMP_ARG" -serial mon:stdio -display none -kernel "$ELFLOADER_STAGE_PATH" -initrd "$CPIO_PATH" -device loader,file="$KERNEL_STAGE_PATH",addr=$KERNEL_LOAD_ADDR,force-raw=on -device loader,file="$ROOTSERVER_STAGE_PATH",addr=$ROOTSERVER_LOAD_ADDR,force-raw=on)

    if [[ "$TRANSPORT" == "tcp" ]]; then
        if [[ "$NET_BACKEND" == "virtio" ]]; then
            log "Wiring virtio-net MMIO NIC for TCP console"
            BASE_QEMU_ARGS+=(-global virtio-mmio.force-legacy=off)
        else
            log "Wiring RTL8139 NIC for TCP console"
        fi
    fi

    if [[ "$RUN_QEMU" -eq 0 ]]; then
        log "--no-run supplied; immutable build artefacts verified at $OUT_DIR"
        return 0
    fi

    if [[ "$DIRECT_QEMU" -eq 1 ]]; then
        QEMU_ARGS=("${BASE_QEMU_ARGS[@]}")
        if [[ "$TRANSPORT" == "tcp" ]]; then
            build_network_args "$TCP_SMOKE_PORT"
            QEMU_ARGS+=("${NETWORK_ARGS[@]}")
        fi
        if [[ -n "$DTB_OVERRIDE" ]]; then
            [[ -f "$DTB_OVERRIDE" ]] || fail "Specified DTB override not found: $DTB_OVERRIDE"
            describe_file "DTB override" "$DTB_OVERRIDE"
            QEMU_ARGS+=(-dtb "$DTB_OVERRIDE")
        fi
        if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
            QEMU_ARGS+=("${EXTRA_QEMU_ARGS[@]}")
        fi
        exec "$QEMU_BIN" "${QEMU_ARGS[@]}"
    fi

    if [[ "$TRANSPORT" == "tcp" ]]; then
        local local_log
        local_log="$(mktemp -t cohesix-qemu.log)"
        if ! run_qemu_attempt "$TCP_SMOKE_PORT" "$local_log"; then
            if grep -q "Could not set up host forwarding rule" "$local_log" && grep -q "31339" "$local_log"; then
                log "Retrying QEMU with fallback smoke port ${HOST_SMOKE_PORT_FALLBACK}"
                TCP_SMOKE_PORT="$HOST_SMOKE_PORT_FALLBACK"
                local_log="$(mktemp -t cohesix-qemu.log)"
                if ! run_qemu_attempt "$TCP_SMOKE_PORT" "$local_log"; then
                    log "QEMU failed to start after retry; last log lines:"
                    tail -n 50 "$local_log" >&2 || true
                    return 1
                fi
            else
                log "QEMU failed to start; last log lines:"
                tail -n 50 "$local_log" >&2 || true
                return 1
            fi
        fi

        print_tcp_summary "$TCP_SMOKE_PORT"
        log "QEMU is running with serial console and TCP console on port $TCP_PORT"
        log "Run: ./cohsh --transport tcp --tcp-port $TCP_PORT    in another terminal."

        wait "$QEMU_PID"
        trap - EXIT
        return 0
    fi

    if [[ "$TRANSPORT" == "qemu" ]]; then
        log "Launching cohsh (QEMU transport) for interactive session"
        COHSH_BIN="$HOST_TOOLS_DIR/cohsh"
        if [[ ! -x "$COHSH_BIN" ]]; then
            fail "cohsh CLI not found: $COHSH_BIN"
        fi

        CLI_CMD=(
            "$COHSH_BIN"
            --transport qemu
            --qemu-bin "$QEMU_BIN"
            --qemu-out-dir "$OUT_DIR"
            --qemu-gic-version "$GIC_VER"
            --role queen
        )

        if [[ ${#ACCEL_ARGS[@]} -gt 0 ]]; then
            for arg in "${ACCEL_ARGS[@]}"; do
                CLI_CMD+=(--qemu-arg "$arg")
            done
        fi

        if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
            for arg in "${EXTRA_QEMU_ARGS[@]}"; do
                CLI_CMD+=(--qemu-arg "$arg")
            done
        fi

        exec "${CLI_CMD[@]}"
    fi
}

main() {
    cd "$PROJECT_ROOT" || fail "cannot enter repository root: $PROJECT_ROOT"
    SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-${SEL4_BUILD:-$CANONICAL_QEMU_BUILD_DIR}}"
    SEL4_PROFILE="${COHESIX_SEL4_PROFILE:-}"
    OUT_DIR="out/cohesix"
    PROFILE="release"
    CARGO_TARGET=""
    QEMU_BIN="qemu-system-aarch64"
    RUN_QEMU=1
    DIRECT_QEMU=0
    LAUNCH_EXISTING=0
    declare -a EXTRA_QEMU_ARGS=()
    declare -a ACCEL_ARGS=()
    CLEAN_OUT_DIR=0
    DTB_OVERRIDE=""
    TRANSPORT="tcp"
    HOST_CONSOLE_PORT=31337
    HOST_UDP_ECHO_PORT=31338
    HOST_SMOKE_PORT=31339
    HOST_SMOKE_PORT_FALLBACK=31349
    TCP_PORT="$HOST_CONSOLE_PORT"
    UDP_ECHO_PORT="$HOST_UDP_ECHO_PORT"
    TCP_SMOKE_PORT="$HOST_SMOKE_PORT"
    DEFAULT_QEMU_SMP_TOPO="4,cores=4,threads=1,sockets=1"
    QEMU_SMP_RAW="${COHESIX_QEMU_SMP:-${QEMU_SMP:-}}"
    QEMU_SMP_TOPO_RAW="${COHESIX_QEMU_SMP_TOPO:-${QEMU_SMP_TOPO:-}}"
    QEMU_SMP_ARG="$(resolve_qemu_smp_arg)"
    VIRTIO_MMIO_FORCE_LEGACY=${VIRTIO_MMIO_FORCE_LEGACY:-0}
    ROOT_TASK_FEATURES=""
    ROOT_TASK_FEATURES_OVERRIDE=0
    ROOT_TASK_FEATURE_EXTRAS=()

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
                ROOT_TASK_FEATURES_OVERRIDE=1
                shift 2
                ;;
            --features)
                [[ $# -ge 2 ]] || fail "--features requires a value"
                case "$2" in
                    bootstrap-trace|serial-console|cohesix-dev)
                        ROOT_TASK_FEATURE_EXTRAS+=("$2")
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
            --launch-existing)
                LAUNCH_EXISTING=1
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

    if [[ "$LAUNCH_EXISTING" -eq 1 && "$CLEAN_OUT_DIR" -eq 1 ]]; then
        fail "--launch-existing cannot be combined with --clean"
    fi
    if [[ "$LAUNCH_EXISTING" -eq 1 && -n "$DTB_OVERRIDE" ]]; then
        fail "--launch-existing does not permit an unbound DTB override"
    fi
    [[ "$CROSS_HOST_REPLAY" == "0" || "$CROSS_HOST_REPLAY" == "1" ]] || \
        fail "COHESIX_QEMU_CROSS_HOST_REPLAY must be 0 or 1"
    if [[ "$CROSS_HOST_REPLAY" == "1" ]]; then
        [[ "$LAUNCH_EXISTING" -eq 1 ]] || \
            fail "cross-host replay requires --launch-existing"
        [[ "$HOST_OS" == "Linux" ]] || \
            fail "cross-host replay is supported only on Linux"
    fi

    command -v python3 >/dev/null 2>&1 || fail "Required command not found in PATH: python3"
    if [[ -L "$OUT_DIR" ]]; then
        fail "output directory must not be a symlink: $OUT_DIR"
    fi
    OUT_DIR="$(python3 - "$OUT_DIR" <<'PY'
import pathlib
import sys

print(pathlib.Path(sys.argv[1]).resolve(strict=False))
PY
)"
    case "$OUT_DIR" in
        "$PROJECT_ROOT"/out/*)
            ;;
        *)
            fail "output directory must be a resolved child of $PROJECT_ROOT/out: $OUT_DIR"
            ;;
    esac

    CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target}"
    if [[ -L "$CARGO_TARGET_DIR" ]]; then
        fail "Cargo target directory must not be a symlink: $CARGO_TARGET_DIR"
    fi
    CARGO_TARGET_DIR="$(python3 - "$CARGO_TARGET_DIR" <<'PY'
import pathlib
import sys

print(pathlib.Path(sys.argv[1]).resolve(strict=False))
PY
)"
    [[ "$CARGO_TARGET_DIR" == "$PROJECT_ROOT/target" ]] || fail \
        "CARGO_TARGET_DIR must resolve to the repository target directory: $PROJECT_ROOT/target"
    export CARGO_TARGET_DIR

    if [[ "$ROOT_TASK_FEATURES" == "none" ]]; then
        ROOT_TASK_FEATURES=""
    fi

    if [[ "$ROOT_TASK_FEATURES_OVERRIDE" -eq 0 ]]; then
        ROOT_TASK_FEATURES="release-qemu,bootstrap-trace"
    else
        # Preserve explicit --root-task-features verbatim so hardware profile
        # parity checks (e.g. Pi4 no-NIC baselines) are not transport-mutated.
        :
    fi

    for feature in "${ROOT_TASK_FEATURE_EXTRAS[@]-}"; do
        append_root_task_feature "$feature"
    done

    remove_root_task_feature "untyped-debug"
    remove_root_task_feature "trace-heavy-init"
    remove_root_task_feature "dtb-dump"

    NET_BACKEND="rtl8139"
    # Feature selection here drives the matching QEMU device model. Cargo
    # expands `release-qemu` to `net-backend-virtio`, but this shell script sees
    # only the operator's top-level feature names, so account for that bundle
    # explicitly and never pair a virtio-enabled root task with an RTL8139.
    if has_root_task_feature "release-qemu" \
        || has_root_task_feature "net-backend-virtio" \
        || has_root_task_feature "dev-virt" \
        || has_root_task_feature "cohesix-dev"; then
        NET_BACKEND="virtio"
    fi

    if [[ -n "$ROOT_TASK_FEATURES" ]]; then
        log "Final root-task feature set: $ROOT_TASK_FEATURES"
    else
        log "Final root-task feature set: <none>"
    fi

    if [[ "$TRANSPORT" == "tcp" ]]; then
        log "TCP console NIC backend: ${NET_BACKEND}"
    fi

    if matches=$(rg -n "\\[untyped:" apps/root-task/src 2>/dev/null); then
        if printf '%s\n' "$matches" | grep -v "bootstrap/untyped.rs" >/dev/null; then
            echo "[cohesix-build] ERROR: found untyped prints outside feature gate" >&2
            exit 1
        fi
    fi

    if [[ "$TRANSPORT" == "tcp" && "$TCP_PORT" -le 0 ]]; then
        fail "TCP port must be a positive integer"
    fi

    validate_qemu_smp_arg "$QEMU_SMP_ARG"
    QEMU_VIRT_ARG="$(resolve_qemu_virt_arg)"
    validate_qemu_virt_arg "$QEMU_VIRT_ARG"

    if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
        fail "seL4 build directory not found: $SEL4_BUILD_DIR"
    fi
    SEL4_BUILD_DIR="$(cd "$SEL4_BUILD_DIR" && pwd)"

    if [[ -z "$SEL4_PROFILE" && "$SEL4_BUILD_DIR" == "$CANONICAL_QEMU_BUILD_DIR" ]]; then
        SEL4_PROFILE="$CANONICAL_QEMU_PROFILE"
    elif [[ -z "$SEL4_PROFILE" \
        && "$SEL4_BUILD_DIR" == "$CANONICAL_QEMU_KVM_BUILD_DIR" ]]; then
        SEL4_PROFILE="$CANONICAL_QEMU_KVM_PROFILE"
    fi
    if [[ "$CROSS_HOST_REPLAY" == "1" ]]; then
        [[ "$SEL4_PROFILE" == "$CANONICAL_QEMU_PROFILE" \
            || "$SEL4_PROFILE" == "$CANONICAL_QEMU_KVM_PROFILE" ]] || \
            fail "cross-host replay requires a production QEMU profile"
        log "Cross-host replay: trusting the immutable guest hashes and rebound Linux launch record"
    elif [[ -n "$SEL4_PROFILE" ]]; then
        validate_selected_qemu_profile "$SEL4_PROFILE"
    else
        log "Explicit non-canonical seL4 build selected; this run is claim-ineligible unless COHESIX_SEL4_PROFILE names a passing contract"
    fi

    export SEL4_BUILD_DIR
    export SEL4_BUILD="$SEL4_BUILD_DIR"
    QEMU_TIMER_CLOCK_HZ="$(read_selected_timer_clock_hz \
        "$SEL4_BUILD_DIR/kernel/gen_headers/plat/platform_gen.h")" || \
        fail "cannot resolve selected seL4 TIMER_CLOCK_HZ"
    export QEMU_TIMER_CLOCK_HZ

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

    declare -a REQUIRED_COMMANDS=(python3 "$QEMU_BIN")
    if [[ "$LAUNCH_EXISTING" -eq 0 ]]; then
        REQUIRED_COMMANDS+=(cargo cpio)
    fi
    for cmd in "${REQUIRED_COMMANDS[@]}"; do
        command -v "$cmd" >/dev/null 2>&1 || fail "Required command not found in PATH: $cmd"
    done

    if command -v "$QEMU_BIN" >/dev/null 2>&1; then
        QEMU_VERSION="$($QEMU_BIN --version | head -n1)"
        log "Using QEMU binary: $QEMU_BIN ($QEMU_VERSION)"
    fi

    local extra_has_accel=0
    if [[ ${#EXTRA_QEMU_ARGS[@]} -gt 0 ]]; then
        if qemu_args_have_accel "${EXTRA_QEMU_ARGS[@]}"; then
            extra_has_accel=1
        fi
    fi

    if [[ "$extra_has_accel" -eq 0 ]]; then
        if [[ "$TRANSPORT" == "qemu" && -n "${COHSH_QEMU_ARGS:-}" ]]; then
            read -r -a COHSH_QEMU_ARGS_ARR <<< "${COHSH_QEMU_ARGS}"
            if qemu_args_have_accel "${COHSH_QEMU_ARGS_ARR[@]}"; then
                QEMU_ACCEL="$(qemu_args_accel_value "${COHSH_QEMU_ARGS_ARR[@]}")"
                log "QEMU accel override detected in COHSH_QEMU_ARGS; this launch is claim-ineligible"
            else
                QEMU_ACCEL="$(resolve_qemu_accel)"
                ACCEL_ARGS=(-accel "$QEMU_ACCEL")
                log "Using QEMU accel: $QEMU_ACCEL"
            fi
        else
            QEMU_ACCEL="$(resolve_qemu_accel)"
            ACCEL_ARGS=(-accel "$QEMU_ACCEL")
            log "Using QEMU accel: $QEMU_ACCEL"
        fi
    else
        QEMU_ACCEL="$(qemu_args_accel_value "${EXTRA_QEMU_ARGS[@]}")"
        log "QEMU accel overridden via extra QEMU args; this launch is claim-ineligible"
    fi

    if [[ "${QEMU_ACCEL:-}" == "tcg" ]]; then
        log "TCG is an explicit diagnostic envelope; this run is claim-ineligible"
    elif [[ "$HOST_OS" == "Darwin" \
        && -n "${QEMU_ACCEL:-}" \
        && "$QEMU_ACCEL" != "hvf" ]]; then
        log "Non-HVF Darwin acceleration is outside the production envelope; this run is claim-ineligible"
    fi
    QEMU_CPU_MODEL="$(resolve_qemu_cpu_model "$QEMU_ACCEL")"

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

    OUT_DIR_ABS="$OUT_DIR"
    STAGING_DIR="$OUT_DIR_ABS/staging"
    HOST_OUT_DIR="$OUT_DIR_ABS/host-tools"
    HOST_TOOLS_DIR="$HOST_OUT_DIR"
    ELFLOADER_STAGE_PATH="$STAGING_DIR/elfloader"
    KERNEL_STAGE_PATH="$STAGING_DIR/kernel.elf"
    ROOTSERVER_STAGE_PATH="$STAGING_DIR/rootserver"
    CPIO_PATH="$OUT_DIR_ABS/cohesix-system.cpio"
    LAUNCH_ARTIFACT_TOOL="$SCRIPT_DIR/lib/qemu_launch_artifacts.py"
    LAUNCH_ARTIFACT_RECORD="$OUT_DIR_ABS/cohesix-qemu-launch-artifacts.json"
    [[ -f "$LAUNCH_ARTIFACT_TOOL" ]] || fail \
        "QEMU launch artifact helper is missing: $LAUNCH_ARTIFACT_TOOL"

    if [[ "$LAUNCH_EXISTING" -eq 1 ]]; then
        [[ -d "$OUT_DIR_ABS" ]] || fail \
            "--launch-existing output directory is missing: $OUT_DIR_ABS"
        GIC_VER="$(detect_gic_version)"
        python3 "$LAUNCH_ARTIFACT_TOOL" verify \
            --out-dir "$OUT_DIR_ABS" \
            --sel4-build "$SEL4_BUILD_DIR" \
            --profile "$PROFILE_DIR" \
            --cargo-target "$CARGO_TARGET" \
            --root-task-features "$ROOT_TASK_FEATURES" \
            --gic-version "$GIC_VER" \
            --sel4-profile "${SEL4_PROFILE:-unselected}" \
            --qemu "$QEMU_BIN" \
            --accelerator "$QEMU_ACCEL" \
            --virtualization "$QEMU_VIRT_ARG" \
            --machine-extra "$QEMU_MACHINE_EXTRA" \
            --cpu "$QEMU_CPU_MODEL" \
            --smp "$QEMU_SMP_ARG" \
            --net-backend "$NET_BACKEND" >/dev/null || \
            fail "immutable QEMU launch artifact verification failed"
        launch_qemu_artifacts
        return $?
    fi

    RTC_MANIFEST="${COH_RTC_MANIFEST:-$PROJECT_ROOT/configs/root_task.toml}"
    mkdir -p "$GENERATED_CONFIG_DIR"
    log "Regenerating the complete canonical coh-rtc output set from ${RTC_MANIFEST}"
    cargo run -p coh-rtc -- \
        "$RTC_MANIFEST" \
        --timer-clock-hz "$QEMU_TIMER_CLOCK_HZ" \
        --out "$PROJECT_ROOT/apps/root-task/src/generated" \
        --manifest "$GENERATED_CONFIG_DIR/root_task_resolved.json" \
        --cas-manifest-template "$GENERATED_CONFIG_DIR/cas_manifest_template.json" \
        --cli-script "$PROJECT_ROOT/scripts/cohsh/boot_v0.coh" \
        --doc-snippet "$PROJECT_ROOT/docs/snippets/root_task_manifest.md" \
        --gpu-breadcrumbs-snippet "$PROJECT_ROOT/docs/snippets/gpu_breadcrumbs.md" \
        --observability-interfaces-snippet "$PROJECT_ROOT/docs/snippets/observability_interfaces.md" \
        --observability-security-snippet "$PROJECT_ROOT/docs/snippets/observability_security.md" \
        --ticket-quotas-snippet "$PROJECT_ROOT/docs/snippets/ticket_quotas.md" \
        --trace-policy-snippet "$PROJECT_ROOT/docs/snippets/trace_policy.md" \
        --cas-interfaces-snippet "$PROJECT_ROOT/docs/snippets/cas_interfaces.md" \
        --cas-security-snippet "$PROJECT_ROOT/docs/snippets/cas_security.md" \
        --cbor-snippet "$PROJECT_ROOT/docs/snippets/telemetry_cbor_schema.md" \
        --cohsh-policy "$GENERATED_CONFIG_DIR/cohsh_policy.toml" \
        --cohsh-policy-rust "$PROJECT_ROOT/apps/cohsh/src/generated/policy.rs" \
        --cohsh-policy-doc "$PROJECT_ROOT/docs/snippets/cohsh_policy.md" \
        --cohsh-client-rust "$PROJECT_ROOT/apps/cohsh/src/generated/client.rs" \
        --cohsh-client-doc "$PROJECT_ROOT/docs/snippets/cohsh_client.md" \
        --cohsh-grammar-doc "$PROJECT_ROOT/docs/snippets/cohsh_grammar.md" \
        --cohsh-ticket-policy-doc "$PROJECT_ROOT/docs/snippets/cohsh_ticket_policy.md" \
        --coh-policy "$GENERATED_CONFIG_DIR/coh_policy.toml" \
        --coh-policy-rust "$PROJECT_ROOT/apps/coh/src/generated/policy.rs" \
        --coh-policy-doc "$PROJECT_ROOT/docs/snippets/coh_policy.md" \
        --swarmui-defaults "$GENERATED_CONFIG_DIR/swarmui_defaults.toml" \
        --swarmui-defaults-rust "$PROJECT_ROOT/apps/swarmui/src/generated.rs" \
        --swarmui-defaults-doc "$PROJECT_ROOT/docs/snippets/swarmui_defaults.md" \
        --implementation-surfaces "$PROJECT_ROOT/configs/implementation_surfaces.toml" \
        --implementation-surface-inventory "$GENERATED_CONFIG_DIR/implementation_surface_inventory.json" \
        --host-integration-source "$PROJECT_ROOT/configs/host_integration_acceptance.toml" \
        --host-integration-graph "$GENERATED_CONFIG_DIR/host_integration_dependency.json" \
        --host-integration-doc "$PROJECT_ROOT/docs/snippets/host_integration_dependency.md" \
        --cohesix-py-defaults "$PROJECT_ROOT/tools/cohesix-py/cohesix/generated.py" \
        --cohesix-py-doc "$PROJECT_ROOT/docs/snippets/cohesix_py_defaults.md" \
        --coh-doctor-doc "$PROJECT_ROOT/docs/snippets/coh_doctor_checks.md"

    validate_generated_timer_clock \
        "$GENERATED_CONFIG_DIR/root_task_resolved.json" \
        "$SEL4_BUILD_DIR/kernel/gen_headers/plat/platform_gen.h"

    log "Regenerating target-qualified Python projection contracts"
    cargo run -p coh-rtc --bin coh-rtc-python-profile -- \
        "$PROJECT_ROOT/configs/root_task.toml" \
        --sel4-profiles "$PROJECT_ROOT/configs/sel4/profiles.toml" \
        --profile qemu_smp_production \
        --out "$GENERATED_CONFIG_DIR/cohesix_python_qemu_smp_production.json"
    cargo run -p coh-rtc --bin coh-rtc-python-profile -- \
        "$PROJECT_ROOT/configs/root_task_pi4_uboot_aarch64.toml" \
        --sel4-profiles "$PROJECT_ROOT/configs/sel4/profiles.toml" \
        --profile pi4_production \
        --out "$GENERATED_CONFIG_DIR/cohesix_python_pi4_production.json"

    SEL4_COMPONENT_PACKAGES=(nine-door-runtime console-network-runtime worker-heart worker-gpu worker-lora pi4-driver-runtime)
    HOST_TOOL_PACKAGES=(gpu-bridge-host cas-tool hive-gateway host-ticket-agent swarmui)

    HOST_BUILD_ARGS=(build)
    if (( ${#PROFILE_ARGS[@]} > 0 )); then
        HOST_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
    fi
    for pkg in "${HOST_TOOL_PACKAGES[@]}"; do
        HOST_BUILD_ARGS+=(-p "$pkg")
    done

    log "Building host tooling via: cargo ${HOST_BUILD_ARGS[*]}"
    cargo "${HOST_BUILD_ARGS[@]}"

    COH_BUILD_ARGS=(build)
    if (( ${#PROFILE_ARGS[@]} > 0 )); then
        COH_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
    fi
    COH_BUILD_ARGS+=(-p coh)
    # On macOS, `coh mount` requires the optional FUSE backend (MacFUSE runtime).
    # Build with `--features fuse` by default so operators can mount without a manual rebuild.
    if [[ "$HOST_OS" == "Darwin" ]]; then
        COH_BUILD_ARGS+=(--features fuse)
    fi
    log "Building coh CLI via: cargo ${COH_BUILD_ARGS[*]}"
    cargo "${COH_BUILD_ARGS[@]}"

    HOST_SIDECAR_ARGS=(build)
    if (( ${#PROFILE_ARGS[@]} > 0 )); then
        HOST_SIDECAR_ARGS+=("${PROFILE_ARGS[@]}")
    fi
    HOST_SIDECAR_ARGS+=(-p host-sidecar-bridge --features tcp)
    log "Building host-sidecar-bridge with TCP support via: cargo ${HOST_SIDECAR_ARGS[*]}"
    cargo "${HOST_SIDECAR_ARGS[@]}"

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
    if has_root_task_feature release-qemu && has_root_task_feature bootstrap-trace; then
        SEL4_BUILD_ARGS+=(--features "nine-door-runtime/qemu-evidence,console-network-runtime/qemu-evidence,worker-heart/qemu-evidence,worker-gpu/qemu-evidence,worker-lora/qemu-evidence")
        log "Enabling external QEMU/GDB service and Worker evidence symbols"
    fi

    ROOT_TASK_BUILD_ARGS=(build --target "$CARGO_TARGET")
    if (( ${#PROFILE_ARGS[@]} > 0 )); then
        ROOT_TASK_BUILD_ARGS+=("${PROFILE_ARGS[@]}")
    fi
    ROOT_TASK_BUILD_ARGS+=(-p root-task --no-default-features)
    if [[ -n "$ROOT_TASK_FEATURES" ]]; then
        ROOT_TASK_BUILD_ARGS+=(--features "$ROOT_TASK_FEATURES")
    fi

    if [[ -n "$SEL4_LD" ]]; then
        ROOT_TASK_LINKER_SCRIPT="$SEL4_LD"
    else
        ROOT_TASK_LINKER_SCRIPT="$PROJECT_ROOT/apps/root-task/sel4.ld"
        if [[ ! -f "$ROOT_TASK_LINKER_SCRIPT" ]]; then
            fail "root-task linker script not found: $ROOT_TASK_LINKER_SCRIPT"
        fi
    fi

    log "Using root-task linker script: $ROOT_TASK_LINKER_SCRIPT"
    log "Building seL4 components via: cargo ${SEL4_BUILD_ARGS[*]}"
    cargo "${SEL4_BUILD_ARGS[@]}"

    HOST_ARTIFACT_DIR="$CARGO_TARGET_DIR/$PROFILE_DIR"
    SEL4_ARTIFACT_DIR="$CARGO_TARGET_DIR/$CARGO_TARGET/$PROFILE_DIR"

    [[ -d "$HOST_ARTIFACT_DIR" ]] || fail "Cargo artefact directory not found: $HOST_ARTIFACT_DIR"
    [[ -d "$SEL4_ARTIFACT_DIR" ]] || fail "Cargo artefact directory not found: $SEL4_ARTIFACT_DIR"

    mkdir -p "$OUT_DIR"
    OUT_DIR_ABS="$(cd "$OUT_DIR" && pwd)"
    WORKER_OUTPUT_DIR="$OUT_DIR_ABS/worker-images"
    WORKER_CANONICAL_DIR="$WORKER_OUTPUT_DIR/canonical"
    WORKER_ARCHIVE_PATH="$WORKER_OUTPUT_DIR/cohesix-worker-images.cpio"
    WORKER_MANIFEST_PATH="$WORKER_OUTPUT_DIR/cohesix-worker-image-manifest.json"
    WORKER_MANIFEST_TOOL="$SCRIPT_DIR/worker_image_manifest.py"
    [[ -f "$WORKER_MANIFEST_TOOL" ]] || fail "Worker image manifest tool is missing: $WORKER_MANIFEST_TOOL"
    log "Validating and packaging target Worker images before root compilation"
    python3 "$WORKER_MANIFEST_TOOL" build \
        --image-dir "$SEL4_ARTIFACT_DIR" \
        --output-dir "$WORKER_CANONICAL_DIR" \
        --archive "$WORKER_ARCHIVE_PATH" \
        --manifest "$WORKER_MANIFEST_PATH" \
        --target "$CARGO_TARGET" \
        --profile "$PROFILE_DIR"

    DRIVER_OUTPUT_DIR="$OUT_DIR_ABS/driver-runtimes"
    DRIVER_ARCHIVE_PATH="$DRIVER_OUTPUT_DIR/cohesix-driver-runtimes.cpio"
    DRIVER_MANIFEST_PATH="$DRIVER_OUTPUT_DIR/cohesix-driver-runtime-manifest.json"
    DRIVER_MANIFEST_TOOL="$SCRIPT_DIR/driver_runtime_manifest.py"
    DRIVER_CLASSIC_COMPARATOR_RECORD="${COHESIX_DRIVER_CLASSIC_COMPARATOR_RECORD:-$PROJECT_ROOT/configs/driver_runtime_classic_comparator.toml}"
    [[ -f "$DRIVER_MANIFEST_TOOL" ]] || fail \
        "Driver runtime manifest tool is missing: $DRIVER_MANIFEST_TOOL"
    [[ -f "$DRIVER_CLASSIC_COMPARATOR_RECORD" ]] || fail \
        "Immutable classic driver comparator record is missing: $DRIVER_CLASSIC_COMPARATOR_RECORD"
    log "Packaging deterministic MCS linked-driver archive before root compilation"
    python3 "$DRIVER_MANIFEST_TOOL" build \
        --image-dir "$SEL4_ARTIFACT_DIR" \
        --archive "$DRIVER_ARCHIVE_PATH" \
        --manifest "$DRIVER_MANIFEST_PATH" \
        --target "$CARGO_TARGET" \
        --profile "$PROFILE_DIR" \
        --classic-comparator-record "$DRIVER_CLASSIC_COMPARATOR_RECORD"

    log "Building root-task against separate target-qualified Worker and driver identities"
    COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD="$DRIVER_ARCHIVE_PATH" \
        COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE="$SEL4_ARTIFACT_DIR/console-network-runtime" \
        COHESIX_NINEDOOR_RUNTIME_IMAGE="$SEL4_ARTIFACT_DIR/nine-door-runtime" \
        COHESIX_WORKER_IMAGE_ARCHIVE="$WORKER_ARCHIVE_PATH" \
        COHESIX_WORKER_IMAGE_MANIFEST="$WORKER_MANIFEST_PATH" \
        SEL4_LD="$ROOT_TASK_LINKER_SCRIPT" \
        cargo "${ROOT_TASK_BUILD_ARGS[@]}"

    describe_file "Built root-task" "$SEL4_ARTIFACT_DIR/root-task"

    ROOTFS_COMPONENT_BINS=(
        nine-door-runtime
    )
    HOST_ONLY_BINS=(cohsh coh gpu-bridge-host host-sidecar-bridge cas-tool hive-gateway host-ticket-agent swarmui)

    STAGING_DIR="$OUT_DIR/staging"
    ROOTFS_DIR="$STAGING_DIR/cohesix/bin"
    ARTIFACTS_DIR="$STAGING_DIR/cohesix/artifacts"
    HOST_OUT_DIR="$OUT_DIR/host-tools"
    CPIO_PATH="$OUT_DIR_ABS/cohesix-system.cpio"

    if [[ -d "$STAGING_DIR" ]]; then
        find "$STAGING_DIR" -depth -mindepth 1 -delete
    fi
    mkdir -p "$ROOTFS_DIR" "$ARTIFACTS_DIR" "$HOST_OUT_DIR"
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

    for bin in "${ROOTFS_COMPONENT_BINS[@]}"; do
        SRC="$SEL4_ARTIFACT_DIR/$bin"
        [[ -f "$SRC" ]] || fail "Expected binary not found: $SRC"
        install -m 0755 "$SRC" "$ROOTFS_DIR/$bin"
        log "Packaged component binary: $ROOTFS_DIR/$bin"
    done

    CONSOLE_NETWORK_RUNTIME_PATH="$ARTIFACTS_DIR/console-network-runtime"
    install -m 0644 \
        "$SEL4_ARTIFACT_DIR/console-network-runtime" \
        "$CONSOLE_NETWORK_RUNTIME_PATH"
    cmp -s \
        "$SEL4_ARTIFACT_DIR/console-network-runtime" \
        "$CONSOLE_NETWORK_RUNTIME_PATH" || \
        fail "Packaged console-network runtime differs from the target artifact"
    log "Packaged exact console-network runtime: $CONSOLE_NETWORK_RUNTIME_PATH"

    install -m 0644 "$WORKER_ARCHIVE_PATH" "$ARTIFACTS_DIR/cohesix-worker-images.cpio"
    install -m 0644 "$WORKER_MANIFEST_PATH" "$ARTIFACTS_DIR/cohesix-worker-image-manifest.json"
    python3 "$WORKER_MANIFEST_TOOL" verify \
        --archive "$ARTIFACTS_DIR/cohesix-worker-images.cpio" \
        --manifest "$ARTIFACTS_DIR/cohesix-worker-image-manifest.json"
    log "Packaged target-qualified Worker archive and manifest under cohesix/artifacts"

    install -m 0644 "$DRIVER_MANIFEST_PATH" "$ARTIFACTS_DIR/cohesix-driver-runtime-manifest.json"
    python3 "$DRIVER_MANIFEST_TOOL" verify \
        --archive "$DRIVER_ARCHIVE_PATH" \
        --manifest "$ARTIFACTS_DIR/cohesix-driver-runtime-manifest.json"
    log "Recorded the rootserver-embedded MCS driver archive without duplicating it in the rootfs"

    for bin in "${HOST_ONLY_BINS[@]}"; do
        SRC="$HOST_ARTIFACT_DIR/$bin"
        [[ -f "$SRC" ]] || fail \
            "Selected-profile host tool is missing after its source build: $SRC"
        install -m 0755 "$SRC" "$HOST_OUT_DIR/$bin"
        log "Copied host-side tool: $HOST_OUT_DIR/$bin"
    done

    PROC_TESTS_DIR="$STAGING_DIR/cohesix/proc/tests"
    mkdir -p "$PROC_TESTS_DIR"
    for script in selftest_quick.coh selftest_full.coh selftest_negative.coh selftest_smp.coh; do
        SRC="$PROJECT_ROOT/resources/proc_tests/$script"
        [[ -f "$SRC" ]] || fail "Missing selftest script: $SRC"
        install -m 0644 "$SRC" "$PROC_TESTS_DIR/$script"
        log "Packaged selftest script: $PROC_TESTS_DIR/$script"
    done

    KERNEL_STAGE_PATH="$STAGING_DIR/kernel.elf"
    ROOTSERVER_STAGE_PATH="$STAGING_DIR/rootserver"

    install -m 0755 "$KERNEL_PATH" "$KERNEL_STAGE_PATH"
    rm -f "$ROOTSERVER_STAGE_PATH"
    install -m 0755 "$SEL4_ARTIFACT_DIR/root-task" "$ROOTSERVER_STAGE_PATH"
    log "Packaged component binary: $ROOTSERVER_STAGE_PATH"
    if [[ -f "$ROOTSERVER_STAGE_PATH" ]]; then
        python3 - "$ROOTSERVER_STAGE_PATH" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
size = path.stat().st_size
print(f"[cohesix-build] Staged rootserver size: {path} ({size} bytes)")
PY
    fi
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
    for bin in "${ROOTFS_COMPONENT_BINS[@]}"; do
        MANIFEST_INPUTS+=("cohesix/bin/$bin")
    done
    MANIFEST_INPUTS+=("cohesix/artifacts/console-network-runtime")

    python3 - "$STAGING_DIR" "$PROFILE" "$RESOLVED_TARGET" "$SEL4_ARTIFACT_DIR" \
        "$WORKER_MANIFEST_PATH" "$WORKER_ARCHIVE_PATH" \
        "$DRIVER_MANIFEST_PATH" "$DRIVER_ARCHIVE_PATH" "${MANIFEST_INPUTS[@]}" <<'PY'
import hashlib
import json
import pathlib
import sys

if len(sys.argv) < 10:
    raise SystemExit(
        "manifest generation requires staging dir, profile, target, "
        "artifact dir, Worker manifest/archive, driver manifest/archive, "
        "and at least one binary"
    )

staging = pathlib.Path(sys.argv[1])
profile = sys.argv[2]
target = sys.argv[3]
artifact_dir = pathlib.Path(sys.argv[4])
worker_manifest_source = pathlib.Path(sys.argv[5])
worker_archive_source = pathlib.Path(sys.argv[6])
driver_manifest_source = pathlib.Path(sys.argv[7])
driver_archive_source = pathlib.Path(sys.argv[8])
rootserver = staging / "rootserver"
root_task = artifact_dir / "root-task"
if rootserver.read_bytes() != root_task.read_bytes():
    raise SystemExit("staged rootserver differs from the target root-task artifact")
if (staging / "cohesix" / "bin" / "root-task").exists():
    raise SystemExit("root-task must not be duplicated inside the CPIO payload")

entries = []
for rel_path in sys.argv[9:]:
    path = staging / rel_path
    data = path.read_bytes()
    target_path = artifact_dir / path.name
    if data != target_path.read_bytes():
        raise SystemExit(f"packaged component differs from target artifact: {path.name}")
    entries.append({
        "name": path.name,
        "path": rel_path,
        "size": path.stat().st_size,
        "sha256": hashlib.sha256(data).hexdigest(),
    })
worker_manifest_path = staging / "cohesix" / "artifacts" / "cohesix-worker-image-manifest.json"
worker_archive_path = staging / "cohesix" / "artifacts" / "cohesix-worker-images.cpio"
if worker_manifest_path.read_bytes() != worker_manifest_source.read_bytes():
    raise SystemExit("staged Worker manifest differs from its validated source")
if worker_archive_path.read_bytes() != worker_archive_source.read_bytes():
    raise SystemExit("staged Worker archive differs from its validated source")
worker_archive_bytes = worker_archive_source.read_bytes()
worker_manifest_bytes = worker_manifest_source.read_bytes()
rootserver_bytes = rootserver.read_bytes()
if rootserver_bytes.count(worker_archive_bytes) != 1:
    raise SystemExit("rootserver must embed the exact validated Worker archive once")
if rootserver_bytes.count(worker_manifest_bytes) != 1:
    raise SystemExit("rootserver must embed the exact validated Worker manifest once")
worker_manifest = json.loads(worker_manifest_path.read_text(encoding="utf-8"))
driver_manifest_path = staging / "cohesix" / "artifacts" / "cohesix-driver-runtime-manifest.json"
if driver_manifest_path.read_bytes() != driver_manifest_source.read_bytes():
    raise SystemExit("staged driver manifest differs from its validated source")
driver_archive_path = driver_archive_source
driver_archive_bytes = driver_archive_path.read_bytes()
if not driver_archive_bytes:
    raise SystemExit("validated driver archive is empty")
if driver_archive_bytes not in rootserver.read_bytes():
    raise SystemExit("rootserver does not embed the exact validated driver archive")
if (staging / "cohesix" / "artifacts" / "cohesix-driver-runtimes.cpio").exists():
    raise SystemExit("rootfs must not duplicate the rootserver-embedded driver archive")
driver_manifest = json.loads(driver_manifest_path.read_text(encoding="utf-8"))
console_runtime = next(
    (entry for entry in entries if entry["path"] == "cohesix/artifacts/console-network-runtime"),
    None,
)
if console_runtime is None:
    raise SystemExit("payload inventory omitted the compiler-declared console-network runtime")
manifest = {
    "profile": profile,
    "target": target,
    "binaries": entries,
    "console_network_runtime": {
        "image_id": "console-network-runtime",
        "path": console_runtime["path"],
        "size": console_runtime["size"],
        "sha256": console_runtime["sha256"],
        "entry_symbol": "_start",
        "listener_port": 31337,
    },
    "worker_images": {
        "archive_path": "cohesix/artifacts/cohesix-worker-images.cpio",
        "archive_sha256": hashlib.sha256(worker_archive_bytes).hexdigest(),
        "manifest_path": "cohesix/artifacts/cohesix-worker-image-manifest.json",
        "manifest_sha256": hashlib.sha256(worker_manifest_bytes).hexdigest(),
        "embedded_in_rootserver": True,
        "target_load_source": "rootserver-embedded",
        "images": worker_manifest["images"],
    },
    "driver_runtimes": {
        "archive_path": "driver-runtimes/cohesix-driver-runtimes.cpio",
        "archive_path_scope": "build-output",
        "archive_sha256": hashlib.sha256(driver_archive_bytes).hexdigest(),
        "embedded_in_rootserver": True,
        "manifest_path": "cohesix/artifacts/cohesix-driver-runtime-manifest.json",
        "manifest_sha256": hashlib.sha256(driver_manifest_path.read_bytes()).hexdigest(),
        "classic_comparator": driver_manifest["classic_comparator"],
        "components": driver_manifest["components"],
    },
}
manifest_path = staging / "cohesix" / "manifest.json"
manifest_path.parent.mkdir(parents=True, exist_ok=True)
manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
print(
    "[cohesix-build] Payload inventory verified: "
    f"rootserver=separate components={len(entries)}"
)
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

    GIC_VER="$(detect_gic_version)"
    [[ "$GIC_VER" == "3" ]] || fail \
        "selected operational QEMU build must use GICv3, detected gic-version=$GIC_VER"
    python3 "$LAUNCH_ARTIFACT_TOOL" write \
        --out-dir "$OUT_DIR_ABS" \
        --sel4-build "$SEL4_BUILD_DIR" \
        --profile "$PROFILE_DIR" \
        --cargo-target "$CARGO_TARGET" \
        --root-task-features "$ROOT_TASK_FEATURES" \
        --gic-version "$GIC_VER" \
        --sel4-profile "${SEL4_PROFILE:-unselected}" \
        --qemu "$QEMU_BIN" \
        --accelerator "$QEMU_ACCEL" \
        --virtualization "$QEMU_VIRT_ARG" \
        --machine-extra "$QEMU_MACHINE_EXTRA" \
        --cpu "$QEMU_CPU_MODEL" \
        --smp "$QEMU_SMP_ARG" \
        --net-backend "$NET_BACKEND" >/dev/null || \
        fail "could not bind immutable QEMU launch artifacts"
    log "Bound immutable QEMU launch artifacts: $LAUNCH_ARTIFACT_RECORD"

    launch_qemu_artifacts
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
