#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the cohsh .coh regression pack in two QEMU boots (base + gated).

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

BASE_SCRIPTS=(
    "boot_v0.coh"
    "9p_batch.coh"
    "host_absent.coh"
    "telemetry_ring.coh"
    "shard_1k.coh"
    "observe_watch.coh"
    "cas_roundtrip.coh"
    "tcp_basic.coh"
    "session_pool.coh"
)

GATED_SCRIPTS=(
    "replay_journal.coh"
    "policy_gate.coh"
    "model_cas_bind.coh"
    "sidecar_integration.coh"
)

BASE_MANIFEST="${PROJECT_ROOT}/configs/root_task.toml"
GATED_MANIFEST="${PROJECT_ROOT}/configs/root_task_regression.toml"
ARCHIVE_ROOT="out/regression-logs"
READY_MARKER="Cohesix console ready"
READY_TIMEOUT="${READY_TIMEOUT:-180}"
PORT_TIMEOUT="${PORT_TIMEOUT:-30}"
QUIT_CLOSE_TIMEOUT="${QUIT_CLOSE_TIMEOUT:-30}"

check_port_open() {
    local host="$1"
    local port="$2"
    python3 - "$host" "$port" <<'PY'
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
}

wait_port_ready() {
    local host="$1"
    local port="$2"
    local timeout="$3"
    local pid="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if check_port_open "$host" "$port"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

log_has() {
    local file="$1"
    local pattern="$2"
    python3 - "$file" "$pattern" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
pattern = sys.argv[2]
if not path.exists():
    sys.exit(1)
data = path.read_bytes()
try:
    text = data.decode(errors="ignore")
except Exception:
    sys.exit(1)
sys.exit(0 if pattern in text else 1)
PY
}

wait_log_marker() {
    local file="$1"
    local pattern="$2"
    local timeout="$3"
    local pid="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if log_has "$file" "$pattern"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

count_log_pattern() {
    local file="$1"
    local pattern="$2"
    python3 - "$file" "$pattern" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
pattern = sys.argv[2].encode()
if not path.exists():
    print(0)
    sys.exit(0)
data = path.read_bytes()
print(data.count(pattern))
PY
}

wait_log_count_increase() {
    local file="$1"
    local pattern="$2"
    local start_count="$3"
    local timeout="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        local current
        current=$(count_log_pattern "$file" "$pattern")
        if (( current > start_count )); then
            return 0
        fi
        sleep 0.2
    done
    return 1
}

run_cohsh() {
    local script="$1"
    local bin="${COHSH_BIN:-./out/cohesix/host-tools/cohsh}"
    case "$script" in
        boot_v0.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/boot_v0.coh
            ;;
        9p_batch.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/9p_batch.coh
            ;;
        host_absent.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/host_absent.coh
            ;;
        telemetry_ring.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/telemetry_ring.coh
            ;;
        shard_1k.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/shard_1k.coh
            ;;
        observe_watch.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/observe_watch.coh
            ;;
        cas_roundtrip.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/cas_roundtrip.coh
            ;;
        tcp_basic.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/tcp_basic.coh
            ;;
        session_pool.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/session_pool.coh
            ;;
        policy_gate.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/policy_gate.coh
            ;;
        model_cas_bind.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/model_cas_bind.coh
            ;;
        replay_journal.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/replay_journal.coh
            ;;
        sidecar_integration.coh)
            "$bin" --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --script scripts/cohsh/sidecar_integration.coh
            ;;
        *)
            echo "Unknown script: $script" >&2
            return 2
            ;;
    esac
}

run_batch() {
    local name="$1"
    local manifest="$2"
    local out_dir="$3"
    shift 3
    local scripts=("$@")

    if check_port_open 127.0.0.1 31337; then
        echo "Port 31337 already in use; stop the running QEMU TCP console and retry." >&2
        return 1
    fi

    local log_root="${out_dir}/logs"
    local archive_root="${ARCHIVE_ROOT}/${name}"
    local qemu_log="${log_root}/regression_batch.qemu.log"

    rm -rf "$out_dir"
    mkdir -p "$log_root" "$archive_root"

    cargo run -p coh-rtc -- \
        "$manifest" \
        --out "$PROJECT_ROOT/apps/root-task/src/generated" \
        --manifest "$PROJECT_ROOT/out/manifests/root_task_resolved.json" \
        --cas-manifest-template "$PROJECT_ROOT/out/cas_manifest_template.json" \
        --cli-script "$PROJECT_ROOT/scripts/cohsh/boot_v0.coh" \
        --doc-snippet "$PROJECT_ROOT/docs/snippets/root_task_manifest.md" \
        --observability-interfaces-snippet "$PROJECT_ROOT/docs/snippets/observability_interfaces.md" \
        --observability-security-snippet "$PROJECT_ROOT/docs/snippets/observability_security.md" \
        --cas-interfaces-snippet "$PROJECT_ROOT/docs/snippets/cas_interfaces.md" \
        --cas-security-snippet "$PROJECT_ROOT/docs/snippets/cas_security.md"

    COH_RTC_MANIFEST="$manifest" SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh \
        --sel4-build "$HOME/seL4/build" \
        --out-dir "$out_dir" \
        --profile release \
        --root-task-features cohesix-dev \
        --cargo-target aarch64-unknown-none \
        --raw-qemu \
        --transport tcp \
        > "$qemu_log" 2>&1 &
    qemu_pid=$!

    if ! wait_log_marker "$qemu_log" "$READY_MARKER" "$READY_TIMEOUT" "$qemu_pid"; then
        echo "FAIL: console ready marker not seen" >&2
        tail -n 50 "$qemu_log" >&2 || true
        return 1
    fi

    if ! wait_port_ready 127.0.0.1 31337 "$PORT_TIMEOUT" "$qemu_pid"; then
        echo "FAIL: TCP console not ready" >&2
        tail -n 50 "$qemu_log" >&2 || true
        return 1
    fi

    COHSH_BIN="${out_dir}/host-tools/cohsh"

    for script in "${scripts[@]}"; do
        local script_name="${script%.coh}"
        echo "=== Running ${name}/${script} ==="

        local close_count_before
        close_count_before=$(count_log_pattern "$qemu_log" "audit tcp.conn.close")
        local coh_log="${log_root}/${script_name}.out.log"

        if ! run_cohsh "$script" > "$coh_log" 2>&1; then
            echo "FAIL: cohsh script ${script}" >&2
            cp "$qemu_log" "${archive_root}/${script_name}.qemu.log" || true
            cp "$coh_log" "${archive_root}/${script_name}.out.log" || true
            return 1
        fi

        if ! wait_log_count_increase "$qemu_log" "audit tcp.conn.close" "$close_count_before" "$QUIT_CLOSE_TIMEOUT"; then
            echo "FAIL: connection did not close after ${script} within ${QUIT_CLOSE_TIMEOUT}s" >&2
            cp "$qemu_log" "${archive_root}/${script_name}.qemu.log" || true
            cp "$coh_log" "${archive_root}/${script_name}.out.log" || true
            return 1
        fi

        cp "$qemu_log" "${archive_root}/${script_name}.qemu.log"
        cp "$coh_log" "${archive_root}/${script_name}.out.log"
        echo "PASS: ${script}"
    done

    if kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
    qemu_pid=0
    return 0
}

qemu_pid=0

cleanup() {
    if (( qemu_pid > 0 )); then
        if kill -0 "$qemu_pid" 2>/dev/null; then
            kill "$qemu_pid" || true
        fi
    fi
}
trap cleanup EXIT

rm -rf target out/cohesix out/cohesix-gated "$ARCHIVE_ROOT"
mkdir -p "$ARCHIVE_ROOT"

if ! run_batch "base" "$BASE_MANIFEST" "out/cohesix" "${BASE_SCRIPTS[@]}"; then
    exit 1
fi

if ! run_batch "gated" "$GATED_MANIFEST" "out/cohesix-gated" "${GATED_SCRIPTS[@]}"; then
    exit 1
fi

cargo run -p coh-rtc -- \
    "$PROJECT_ROOT/configs/root_task.toml" \
    --out "$PROJECT_ROOT/apps/root-task/src/generated" \
    --manifest "$PROJECT_ROOT/out/manifests/root_task_resolved.json" \
    --cas-manifest-template "$PROJECT_ROOT/out/cas_manifest_template.json" \
    --cli-script "$PROJECT_ROOT/scripts/cohsh/boot_v0.coh" \
    --doc-snippet "$PROJECT_ROOT/docs/snippets/root_task_manifest.md" \
    --observability-interfaces-snippet "$PROJECT_ROOT/docs/snippets/observability_interfaces.md" \
    --observability-security-snippet "$PROJECT_ROOT/docs/snippets/observability_security.md" \
    --cas-interfaces-snippet "$PROJECT_ROOT/docs/snippets/cas_interfaces.md" \
    --cas-security-snippet "$PROJECT_ROOT/docs/snippets/cas_security.md"

echo "regression batch complete: $(( ${#BASE_SCRIPTS[@]} + ${#GATED_SCRIPTS[@]} )) scripts passed"
