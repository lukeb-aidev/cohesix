#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the cohsh .coh regression pack against QEMU or a live Pi 4 TCP console.
# Copyright 2026 Lukas Bower

# Note: select a target with COHSH_BATCH_TARGET=qemu|pi4; qemu is the default.
# Note: override auth/timeouts via env vars, e.g. COHSH_AUTH_TOKEN=... READY_TIMEOUT=300 PORT_TIMEOUT=60 QUIT_CLOSE_TIMEOUT=60 scripts/cohsh/run_regression_batch.sh
# Note: live Pi runs use COHSH_BATCH_TARGET=pi4 COHSH_TCP_HOST=<pi-ip> COHSH_TCP_PORT=31337.
# Note: archive root defaults to out/regression-logs; override via COHSH_LOG_ROOT=/path/to/logs.
# Note: set COHSH_BATCH_CLEAN_TARGET=1 for a forced clean rebuild before batch execution.
# ** Note: typical end-to-end runtime is ~25 minutes; plan for >= 30 minutes to avoid repeated retries.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GENERATED_CONFIG_DIR="$PROJECT_ROOT/configs/generated"
cd "$PROJECT_ROOT"

BASE_SCRIPTS=(
    "boot_v0.coh"
    "9p_batch.coh"
    "host_absent.coh"
    "host_sidecar_mock.coh"
    "observe_watch.coh"
    "root_cut_basic.coh"
    "session_lifecycle.coh"
    "busy_backpressure.coh"
    "cas_roundtrip.coh"
    "tcp_basic.coh"
    "session_pool.coh"
)

BASE_TELEMETRY_SCRIPTS=(
    "telemetry_ring.coh"
    "telemetry_push_create.coh"
)

BASE_SHARD_SCRIPTS=(
    "shard_1k.coh"
)

GATED_SCRIPTS=(
    "replay_journal.coh"
    "policy_gate.coh"
    "model_cas_bind.coh"
    "sidecar_integration.coh"
)

BASE_MANIFEST="${PROJECT_ROOT}/configs/root_task.toml"
GATED_MANIFEST="${PROJECT_ROOT}/configs/root_task_regression.toml"
READY_MARKER="Cohesix console ready"
READY_TIMEOUT="${READY_TIMEOUT:-180}"
PORT_TIMEOUT="${PORT_TIMEOUT:-30}"
QUIT_CLOSE_TIMEOUT="${QUIT_CLOSE_TIMEOUT:-30}"
AUTH_READY_TIMEOUT="${AUTH_READY_TIMEOUT:-60}"
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-${SEL4_BUILD:-${PROJECT_ROOT}/seL4/SMP_build}}"
BATCH_TARGET="${COHSH_BATCH_TARGET:-qemu}"
case "$BATCH_TARGET" in
    qemu)
        ;;
    pi|pi4|live|external)
        BATCH_TARGET="pi4"
        ;;
    *)
        echo "Unknown COHSH_BATCH_TARGET=${BATCH_TARGET}; expected qemu or pi4" >&2
        exit 2
        ;;
esac
RUN_ID="${COHSH_BATCH_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
if [[ "$BATCH_TARGET" == "pi4" && -z "${COHSH_LOG_ROOT:-}" ]]; then
    ARCHIVE_ROOT="${PROJECT_ROOT}/out/regression-logs/pi4-full-${RUN_ID}"
else
    ARCHIVE_ROOT="${COHSH_LOG_ROOT:-${PROJECT_ROOT}/out/regression-logs}"
fi
TCP_HOST="${COHSH_TCP_HOST:-${COHSH_HOST:-127.0.0.1}}"
TCP_PORT="${COHSH_TCP_PORT:-${COHSH_PORT:-31337}}"
QEMU_TCP_HOST="127.0.0.1"
QEMU_TCP_PORT="31337"
COHSH_RUN_TCP_HOST="$QEMU_TCP_HOST"
COHSH_RUN_TCP_PORT="$QEMU_TCP_PORT"
if [[ "$BATCH_TARGET" == "pi4" ]]; then
    BATCH_CONTINUE_ON_FAIL="${COHSH_BATCH_CONTINUE_ON_FAIL:-1}"
else
    BATCH_CONTINUE_ON_FAIL="${COHSH_BATCH_CONTINUE_ON_FAIL:-0}"
fi

resolve_manifest_auth_token() {
    local manifest_path="$1"
    python3 - "$manifest_path" <<'PY'
import pathlib
import sys

manifest = pathlib.Path(sys.argv[1])
if not manifest.is_file():
    print("bootstrap")
    raise SystemExit(0)

try:
    import tomllib  # Python 3.11+
except ModuleNotFoundError:
    print("bootstrap")
    raise SystemExit(0)

data = tomllib.loads(manifest.read_text(encoding="utf-8"))
tickets = data.get("tickets", [])
for ticket in tickets:
    if str(ticket.get("role", "")).strip() == "queen":
        secret = str(ticket.get("secret", "")).strip()
        if secret:
            print(secret)
            raise SystemExit(0)
print("bootstrap")
PY
}

DEFAULT_MANIFEST_AUTH_TOKEN="$(resolve_manifest_auth_token "${BASE_MANIFEST}")"
COHSH_AUTH_TOKEN="${COHSH_AUTH_TOKEN:-${COH_AUTH_TOKEN:-${DEFAULT_MANIFEST_AUTH_TOKEN}}}"

is_local_tcp_host() {
    case "$1" in
        127.0.0.1|localhost|::1)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

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
    local status=$?
    if [ "$status" -eq 0 ]; then
        return 0
    fi
    if is_local_tcp_host "$host" && command -v lsof >/dev/null 2>&1; then
        if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
            return 0
        fi
    fi
    return 1
}

wait_port_ready() {
    local host="$1"
    local port="$2"
    local timeout="$3"
    local pid="$4"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if (( pid > 0 )) && ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if check_port_open "$host" "$port"; then
            return 0
        fi
        sleep 0.2
    done
    return 2
}

check_auth_ready() {
    local host="$1"
    local port="$2"
    local token="$3"
    python3 - "$host" "$port" "$token" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
token = sys.argv[3]
payload = f"AUTH {token}".encode()
frame_len = len(payload) + 4
frame = frame_len.to_bytes(4, "little") + payload
try:
    with socket.create_connection((host, port), timeout=0.5) as sock:
        sock.settimeout(0.8)
        sock.sendall(frame)
        header = sock.recv(4)
        if len(header) != 4:
            sys.exit(1)
        total = int.from_bytes(header, "little")
        if total < 4 or total > 4096:
            sys.exit(1)
        remaining = total - 4
        chunks = bytearray()
        while len(chunks) < remaining:
            part = sock.recv(remaining - len(chunks))
            if not part:
                break
            chunks.extend(part)
        data = bytes(chunks)
except OSError:
    sys.exit(1)

if b"OK AUTH" in data or b"ERR AUTH" in data:
    sys.exit(0)
sys.exit(1)
PY
}

wait_auth_ready() {
    local host="$1"
    local port="$2"
    local token="$3"
    local timeout="$4"
    local pid="$5"
    local deadline=$((SECONDS + timeout))
    while (( SECONDS < deadline )); do
        if (( pid > 0 )) && ! kill -0 "$pid" 2>/dev/null; then
            return 1
        fi
        if check_auth_ready "$host" "$port" "$token"; then
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

script_path_for() {
    local script="$1"
    case "$script" in
        boot_v0.coh)
            echo "scripts/cohsh/boot_v0.coh"
            ;;
        9p_batch.coh)
            echo "scripts/cohsh/9p_batch.coh"
            ;;
        host_absent.coh)
            echo "scripts/cohsh/host_absent.coh"
            ;;
        host_sidecar_mock.coh)
            echo "scripts/cohsh/host_sidecar_mock.coh"
            ;;
        telemetry_ring.coh)
            echo "scripts/cohsh/telemetry_ring.coh"
            ;;
        telemetry_push_create.coh)
            echo "scripts/cohsh/telemetry_push_create.coh"
            ;;
        shard_1k.coh)
            echo "scripts/cohsh/shard_1k.coh"
            ;;
        observe_watch.coh)
            echo "scripts/cohsh/observe_watch.coh"
            ;;
        root_cut_basic.coh)
            echo "scripts/cohsh/root_cut_basic.coh"
            ;;
        session_lifecycle.coh)
            echo "scripts/cohsh/session_lifecycle.coh"
            ;;
        busy_backpressure.coh)
            echo "scripts/cohsh/busy_backpressure.coh"
            ;;
        cas_roundtrip.coh)
            echo "scripts/cohsh/cas_roundtrip.coh"
            ;;
        tcp_basic.coh)
            echo "scripts/cohsh/tcp_basic.coh"
            ;;
        session_pool.coh)
            echo "scripts/cohsh/session_pool.coh"
            ;;
        policy_gate.coh)
            echo "scripts/cohsh/policy_gate.coh"
            ;;
        model_cas_bind.coh)
            echo "scripts/cohsh/model_cas_bind.coh"
            ;;
        replay_journal.coh)
            echo "scripts/cohsh/replay_journal.coh"
            ;;
        sidecar_integration.coh)
            echo "scripts/cohsh/sidecar_integration.coh"
            ;;
        *)
            echo "Unknown script: $script" >&2
            return 2
            ;;
    esac
}

run_cohsh_file() {
    local script_path="$1"
    local bin="${COHSH_BIN:-./out/cohesix/host-tools/cohsh}"
    local tcp_host="${COHSH_RUN_TCP_HOST:-127.0.0.1}"
    local tcp_port="${COHSH_RUN_TCP_PORT:-31337}"
    "$bin" \
        --transport tcp \
        --tcp-host "$tcp_host" \
        --tcp-port "$tcp_port" \
        --auth-token "${COHSH_AUTH_TOKEN}" \
        --script "$script_path"
}

run_cohsh() {
    local script="$1"
    local script_path
    if ! script_path="$(script_path_for "$script")"; then
        return 2
    fi
    run_cohsh_file "$script_path"
}

run_batch() {
    local name="$1"
    local manifest="$2"
    local out_dir="$3"
    shift 3
    local scripts=("$@")

    if check_port_open "$QEMU_TCP_HOST" "$QEMU_TCP_PORT"; then
        echo "Port ${QEMU_TCP_PORT} already in use; stop the running QEMU TCP console and retry." >&2
        return 1
    fi

    local log_root="${out_dir}/logs"
    local archive_root="${ARCHIVE_ROOT}/${name}"
    local qemu_log="${log_root}/regression_batch.qemu.log"

    rm -rf "$out_dir"
    mkdir -p "$log_root" "$archive_root"
    mkdir -p "$GENERATED_CONFIG_DIR"

    cargo run -p coh-rtc -- \
        "$manifest" \
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
        --cohsh-grammar-doc "$PROJECT_ROOT/docs/snippets/cohsh_grammar.md" \
        --cohsh-ticket-policy-doc "$PROJECT_ROOT/docs/snippets/cohsh_ticket_policy.md"

    COH_RTC_MANIFEST="$manifest" SEL4_BUILD_DIR="$SEL4_BUILD_DIR" ./scripts/cohesix-build-run.sh \
        --sel4-build "$SEL4_BUILD_DIR" \
        --out-dir "$out_dir" \
        --profile release \
        --root-task-features cohesix-dev \
        --cargo-target aarch64-unknown-none \
        --raw-qemu \
        --transport tcp \
        > "$qemu_log" 2>&1 &
    qemu_pid=$!

    if ! wait_port_ready "$QEMU_TCP_HOST" "$QEMU_TCP_PORT" "$READY_TIMEOUT" "$qemu_pid"; then
        echo "FAIL: TCP console not ready" >&2
        tail -n 50 "$qemu_log" >&2 || true
        return 1
    fi
    echo "INFO: TCP console reachable on ${QEMU_TCP_HOST}:${QEMU_TCP_PORT}"

    if ! log_has "$qemu_log" "$READY_MARKER"; then
        echo "WARN: console ready marker not seen; proceeding because TCP console is reachable" >&2
    fi

    echo "INFO: waiting for TCP auth handshake readiness"
    if ! wait_auth_ready "$QEMU_TCP_HOST" "$QEMU_TCP_PORT" "$COHSH_AUTH_TOKEN" "$AUTH_READY_TIMEOUT" "$qemu_pid"; then
        echo "FAIL: TCP console auth endpoint not ready" >&2
        tail -n 50 "$qemu_log" >&2 || true
        return 1
    fi
    echo "INFO: TCP auth handshake is responsive"

    COHSH_BIN="${out_dir}/host-tools/cohsh"
    COHSH_RUN_TCP_HOST="$QEMU_TCP_HOST"
    COHSH_RUN_TCP_PORT="$QEMU_TCP_PORT"

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

ensure_live_cohsh_bin() {
    if [[ -n "${COHSH_BIN:-}" ]]; then
        if [[ ! -x "$COHSH_BIN" ]]; then
            echo "Configured COHSH_BIN is not executable: $COHSH_BIN" >&2
            return 1
        fi
        return 0
    fi

    COHSH_BIN="${PROJECT_ROOT}/target/debug/cohsh"
    if [[ -x "$COHSH_BIN" ]]; then
        return 0
    fi

    cargo build -p cohsh
    if [[ ! -x "$COHSH_BIN" ]]; then
        echo "cohsh build completed but binary is missing: $COHSH_BIN" >&2
        return 1
    fi
}

write_lifecycle_resume_script() {
    local path="$1"
    cat > "$path" <<'EOF'
# Author: Lukas Bower
# Purpose: Resume a live Pi 4 regression target before continuing batch execution.
# Copyright 2026 Lukas Bower
attach queen
EXPECT OK
lifecycle resume
EXPECT OK
quit
EXPECT OK
EOF
}

write_lifecycle_touch_script() {
    local path="$1"
    cat > "$path" <<'EOF'
# Author: Lukas Bower
# Purpose: Refresh live Pi 4 root reachability after an idempotent lifecycle check.
# Copyright 2026 Lukas Bower
attach queen
EXPECT OK
quit
EXPECT OK
EOF
}

run_lifecycle_touch() {
    local label="$1"
    local log_path="${ARCHIVE_ROOT}/touch-${label}.out.log"
    if run_cohsh_file "$LIFECYCLE_TOUCH_SCRIPT" > "$log_path" 2>&1; then
        printf "TOUCH %s status=0\n" "$label" | tee -a "$SUMMARY_LOG"
        return 0
    fi
    local status=$?
    printf "TOUCH %s status=%s\n" "$label" "$status" | tee -a "$SUMMARY_LOG"
    sed 's/^/  /' "$log_path" | tail -n 20 | tee -a "$SUMMARY_LOG"
    return "$status"
}

run_lifecycle_resume() {
    local label="$1"
    local log_path="${ARCHIVE_ROOT}/resume-${label}.out.log"
    local status
    mkdir -p "$ARCHIVE_ROOT"
    set +e
    run_cohsh_file "$LIFECYCLE_RESUME_SCRIPT" > "$log_path" 2>&1
    status=$?
    set -e
    if [[ "$status" -eq 0 ]]; then
        printf "RESUME %s status=0\n" "$label" | tee -a "$SUMMARY_LOG"
        return 0
    fi
    if grep -q 'state=ONLINE reason=invalid-transition' "$log_path"; then
        printf "RESUME %s status=0 already-online\n" "$label" | tee -a "$SUMMARY_LOG"
        run_lifecycle_touch "$label" || true
        return 0
    fi
    printf "RESUME %s status=%s\n" "$label" "$status" | tee -a "$SUMMARY_LOG"
    sed 's/^/  /' "$log_path" | tail -n 20 | tee -a "$SUMMARY_LOG"
    return "$status"
}

run_live_group() {
    local name="$1"
    shift
    local scripts=("$@")
    local group_dir="${ARCHIVE_ROOT}/${name}"
    mkdir -p "$group_dir"

    run_lifecycle_resume "before-${name}" || true
    for script in "${scripts[@]}"; do
        local script_name="${script%.coh}"
        local coh_log="${group_dir}/${script_name}.out.log"

        pi_total=$((pi_total + 1))
        printf "=== Running %s/%s ===\n" "$name" "$script" | tee -a "$SUMMARY_LOG"
        if run_cohsh "$script" > "$coh_log" 2>&1; then
            pi_pass=$((pi_pass + 1))
            printf "PASS %s/%s\n" "$name" "$script" | tee -a "$SUMMARY_LOG"
        else
            local status=$?
            pi_fail=$((pi_fail + 1))
            printf "FAIL %s/%s status=%s\n" "$name" "$script" "$status" | tee -a "$SUMMARY_LOG"
            sed 's/^/  /' "$coh_log" | tail -n 40 | tee -a "$SUMMARY_LOG"
            if [[ "$BATCH_CONTINUE_ON_FAIL" != "1" ]]; then
                run_lifecycle_resume "after-${name}-${script_name}" || true
                return "$status"
            fi
        fi
        run_lifecycle_resume "after-${name}-${script_name}" || true
    done
}

run_pi4_batch() {
    rm -rf "$ARCHIVE_ROOT"
    mkdir -p "$ARCHIVE_ROOT"
    SUMMARY_LOG="${ARCHIVE_ROOT}/summary.log"
    : > "$SUMMARY_LOG"
    LIFECYCLE_RESUME_SCRIPT="${ARCHIVE_ROOT}/lifecycle_resume.coh"
    LIFECYCLE_TOUCH_SCRIPT="${ARCHIVE_ROOT}/lifecycle_touch.coh"
    write_lifecycle_resume_script "$LIFECYCLE_RESUME_SCRIPT"
    write_lifecycle_touch_script "$LIFECYCLE_TOUCH_SCRIPT"

    COHSH_RUN_TCP_HOST="$TCP_HOST"
    COHSH_RUN_TCP_PORT="$TCP_PORT"
    ensure_live_cohsh_bin

    printf "INFO target=pi4 tcp=%s:%s log_root=%s\n" "$TCP_HOST" "$TCP_PORT" "$ARCHIVE_ROOT" | tee -a "$SUMMARY_LOG"
    if ! wait_port_ready "$TCP_HOST" "$TCP_PORT" "$PORT_TIMEOUT" 0; then
        printf "FAIL: TCP console not reachable on %s:%s within %ss\n" "$TCP_HOST" "$TCP_PORT" "$PORT_TIMEOUT" | tee -a "$SUMMARY_LOG" >&2
        return 1
    fi
    printf "INFO: TCP console reachable on %s:%s\n" "$TCP_HOST" "$TCP_PORT" | tee -a "$SUMMARY_LOG"

    if ! wait_auth_ready "$TCP_HOST" "$TCP_PORT" "$COHSH_AUTH_TOKEN" "$AUTH_READY_TIMEOUT" 0; then
        printf "FAIL: TCP console auth endpoint not ready on %s:%s within %ss\n" "$TCP_HOST" "$TCP_PORT" "$AUTH_READY_TIMEOUT" | tee -a "$SUMMARY_LOG" >&2
        return 1
    fi
    printf "INFO: TCP auth handshake is responsive\n" | tee -a "$SUMMARY_LOG"

    pi_pass=0
    pi_fail=0
    pi_total=0
    if ! run_live_group "base" "${BASE_SCRIPTS[@]}"; then
        return 1
    fi
    if ! run_live_group "base-telemetry" "${BASE_TELEMETRY_SCRIPTS[@]}"; then
        return 1
    fi
    if ! run_live_group "base-shard" "${BASE_SHARD_SCRIPTS[@]}"; then
        return 1
    fi
    if ! run_live_group "gated" "${GATED_SCRIPTS[@]}"; then
        return 1
    fi

    printf "RESULT pass=%s fail=%s total=%s log_root=%s\n" "$pi_pass" "$pi_fail" "$pi_total" "$ARCHIVE_ROOT" | tee -a "$SUMMARY_LOG"
    if (( pi_fail > 0 )); then
        return 1
    fi
    return 0
}

qemu_pid=0
SUMMARY_LOG=""
LIFECYCLE_RESUME_SCRIPT=""
LIFECYCLE_TOUCH_SCRIPT=""
pi_pass=0
pi_fail=0
pi_total=0

cleanup() {
    if (( qemu_pid > 0 )); then
        if kill -0 "$qemu_pid" 2>/dev/null; then
            kill "$qemu_pid" || true
        fi
    fi
}
trap cleanup EXIT

if [[ "$BATCH_TARGET" == "pi4" ]]; then
    run_pi4_batch
    exit $?
fi

if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
    echo "Missing seL4 build directory: $SEL4_BUILD_DIR" >&2
    echo "Set SEL4_BUILD_DIR (or SEL4_BUILD) to your kernel build, e.g. ${PROJECT_ROOT}/seL4/SMP_build" >&2
    exit 1
fi

if [[ "${COHSH_BATCH_CLEAN_TARGET:-0}" == "1" ]]; then
    rm -rf "${PROJECT_ROOT}/target"
fi
rm -rf \
    "${PROJECT_ROOT}/out/cohesix" \
    "${PROJECT_ROOT}/out/cohesix-gated" \
    "$ARCHIVE_ROOT"
mkdir -p "$ARCHIVE_ROOT"

if ! run_batch "base" "$BASE_MANIFEST" "${PROJECT_ROOT}/out/cohesix" "${BASE_SCRIPTS[@]}"; then
    exit 1
fi

if ! run_batch "base-telemetry" "$BASE_MANIFEST" "${PROJECT_ROOT}/out/cohesix" "${BASE_TELEMETRY_SCRIPTS[@]}"; then
    exit 1
fi

if ! run_batch "base-shard" "$BASE_MANIFEST" "${PROJECT_ROOT}/out/cohesix" "${BASE_SHARD_SCRIPTS[@]}"; then
    exit 1
fi

if ! run_batch "gated" "$GATED_MANIFEST" "${PROJECT_ROOT}/out/cohesix-gated" "${GATED_SCRIPTS[@]}"; then
    exit 1
fi

cargo run -p coh-rtc -- \
    "$PROJECT_ROOT/configs/root_task.toml" \
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
    --cohsh-grammar-doc "$PROJECT_ROOT/docs/snippets/cohsh_grammar.md" \
    --cohsh-ticket-policy-doc "$PROJECT_ROOT/docs/snippets/cohsh_ticket_policy.md"

echo "regression batch complete: $(( ${#BASE_SCRIPTS[@]} + ${#BASE_TELEMETRY_SCRIPTS[@]} + ${#BASE_SHARD_SCRIPTS[@]} + ${#GATED_SCRIPTS[@]} )) scripts passed"
