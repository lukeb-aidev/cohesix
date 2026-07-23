#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the cohsh .coh regression pack against QEMU or a live Pi 4 TCP console.
# Copyright 2026 Lukas Bower

# Note: select a target with COHSH_BATCH_TARGET=qemu|pi4; qemu is the default.
# Note: override auth/timeouts via env vars, e.g. COHSH_AUTH_TOKEN=... READY_TIMEOUT=300 PORT_TIMEOUT=60 QUIT_CLOSE_TIMEOUT=60 scripts/cohsh/run_regression_batch.sh
# Note: live Pi runs use COHSH_BATCH_TARGET=pi4 COHSH_TCP_HOST=<pi-ip> COHSH_TCP_PORT=31337.
# Note: archive root defaults to out/regression-logs; override via COHSH_LOG_ROOT=/path/to/logs.
# Note: set COHSH_BATCH_CLEAN_TARGET=1 for a forced clean rebuild before batch execution.
# Note: set COHSH_BATCH_GROUPS=base,base-telemetry,base-shard,gated to run a subset.
# ** Note: typical end-to-end runtime is ~25 minutes; plan for >= 30 minutes to avoid repeated retries.
#
# Operator note for live Pi final reboot proof:
# The Pi 4 batch uses TCP while Cohesix is running, but the final reboot/fresh-boot
# proof crosses a reset and the U-Boot menu. Use the active minicom UART capture
# for that section instead of trying to drive the Terminal UI. macOS may deny
# scripted Terminal keystrokes; direct writes to /dev/cu.usbserial-* still reach
# the same UART and minicom continues to capture the evidence.
#
# Repeatable flow:
#   1. Identify the active minicom session and capture log:
#        ps -ax -o pid=,tty=,command= | rg '[m]inicom'
#        lsof -p <minicom-pid> | rg 'pi4-serial|usbserial'
#      Use the lsof output as the source of truth for both the serial device and
#      the log file; for example:
#        serial_dev=/dev/cu.usbserial-0001
#        capture_log=/Users/lukasbower/pi4-serial-YYYYMMDD-HHMMSS.log
#      Ignore stale or zero-byte ~/pi4-serial-*.log files.
#   2. Prove the serial injection lane before resetting:
#        python3 -c 'import os, sys; fd=os.open(sys.argv[1], os.O_WRONLY | os.O_NOCTTY); os.write(fd, b"smp activity\r"); os.close(fd)' "$serial_dev"
#      Then grep the same capture log for a new "smp activity" block with
#      "OK SMP mode=activity" and, for Genet, "backend=bcmgenet-v5" plus the
#      expected DHCP lease.
#   3. Request reboot over authenticated cohsh while TCP is still up:
#        tmp_script="$(mktemp /tmp/coh-reboot.XXXXXX.coh)"
#        printf 'reboot\n' > "$tmp_script"
#        "${COHSH_BIN:-target/debug/cohsh}" --transport tcp --tcp-host "$COHSH_TCP_HOST" --tcp-port "${COHSH_TCP_PORT:-31337}" --auth-token "$COHSH_AUTH_TOKEN" --role queen --script "$tmp_script"
#        rc=$?; rm -f "$tmp_script"; test "$rc" -eq 0
#      Expected console evidence is "OK REBOOT detail=scheduled" followed by a
#      reset in the minicom log.
#   4. When U-Boot reaches the menu, press Enter over the UART device:
#        python3 -c 'import os, sys; fd=os.open(sys.argv[1], os.O_WRONLY | os.O_NOCTTY); os.write(fd, b"\r"); os.close(fd)' "$serial_dev"
#      If U-Boot reports "boot marker diagnostics", that is expected; pressing
#      Enter should continue the normal boot from the interactive menu.
#   5. Wait for fresh Cohesix proof in the same capture log: DHCP bound,
#      root prompt, then a new clean "smp activity" block. If USB/local-seat
#      boot chatter interleaves with a partial command, send a blank line,
#      retry "smp activity" after owner-state ready, and only count the full
#      block ending in "OK SMP mode=activity".
#   6. For scripts that require clean boot-local state, run one selected group
#      per fresh boot, for example:
#        COHSH_BATCH_TARGET=pi4 COHSH_BATCH_GROUPS=base ...
#        <reboot using the UART flow above>
#        COHSH_BATCH_TARGET=pi4 COHSH_BATCH_GROUPS=base-telemetry ...
#      The default remains the full sequence for QEMU parity and quick live
#      smoke runs; selected groups make the fresh-boot proof repeatable.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=scripts/ci/test_plan_resources.sh
source "${PROJECT_ROOT}/scripts/ci/test_plan_resources.sh"
tp_configure_resource_limits
GENERATED_CONFIG_DIR="$PROJECT_ROOT/configs/generated"
QEMU_ARTIFACT_HELPER="$PROJECT_ROOT/scripts/ci/qemu_artifact.py"
TEST_PLAN_CATALOG="$PROJECT_ROOT/scripts/ci/test_plan_catalog.py"
BUILD_RUN_BIN="${COHESIX_BUILD_RUN_BIN:-$PROJECT_ROOT/scripts/cohesix-build-run.sh}"
cd "$PROJECT_ROOT"

canonical_path() {
    python3 - "$1" <<'PY'
from pathlib import Path
import sys

print(Path(sys.argv[1]).resolve(strict=False))
PY
}

validate_output_roots() {
    python3 - \
        "$PROJECT_ROOT" \
        "$ARCHIVE_ROOT" \
        "$QEMU_ARTIFACT_ROOT" \
        "$TRANSPORT_RESULT_ROOT" \
        "$TRANSPORT_EVIDENCE_ROOT" <<'PY'
from pathlib import Path
import tempfile
import sys

repo, archive, artifact, result, evidence = (
    Path(value).resolve() for value in sys.argv[1:]
)
repo_out = (repo / "out").resolve()
temp_root = Path(tempfile.gettempdir()).resolve()
home = Path.home().resolve()


def within(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True


for label, path in (
    ("archive", archive),
    ("artifact", artifact),
    ("result", result),
    ("evidence", evidence),
):
    if path in {Path("/"), repo, repo_out, home, temp_root}:
        raise SystemExit(f"unsafe {label} root: {path}")
    if not within(path, repo_out) and not within(path, temp_root):
        raise SystemExit(
            f"{label} root must be below {repo_out} or temporary root "
            f"{temp_root}: {path}"
        )

if artifact == archive or result == archive:
    raise SystemExit("artifact/result roots must not alias the archive root")
if within(archive, artifact) or within(archive, result):
    raise SystemExit("archive root must not be nested below artifact/result roots")
if artifact == result or within(artifact, result) or within(result, artifact):
    raise SystemExit("artifact and result roots must not alias or overlap")
for label, path in (
    ("archive", archive),
    ("artifact", artifact),
    ("result", result),
):
    if not within(path, evidence):
        raise SystemExit(f"{label} root escapes transport evidence root: {path}")
PY
}

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
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-${SEL4_BUILD:-${PROJECT_ROOT}/out/sel4/profile-v2/qemu-smp-production}}"
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
if [[ "$BATCH_TARGET" == "qemu" ]]; then
    TEST_ACTION_ID="${TEST_PLAN_ACTION_ID:-qemu.tcp-regression}"
    TEST_CLAIM_TIER="qemu-integration"
else
    TEST_ACTION_ID="${TEST_PLAN_ACTION_ID:-pi4.tcp-regression}"
    TEST_CLAIM_TIER="pi4-transport"
fi
if [[ ! -x "$QEMU_ARTIFACT_HELPER" ]]; then
    echo "Missing QEMU artifact helper: $QEMU_ARTIFACT_HELPER" >&2
    exit 1
fi
if [[ ! -x "$TEST_PLAN_CATALOG" ]]; then
    echo "Missing test-plan action catalog helper: $TEST_PLAN_CATALOG" >&2
    exit 1
fi
if [[ ! -x "$BUILD_RUN_BIN" ]]; then
    echo "Missing Cohesix build wrapper: $BUILD_RUN_BIN" >&2
    exit 1
fi
TEST_ACTION_DIGEST="sha256:$(
    "$TEST_PLAN_CATALOG" action --id "$TEST_ACTION_ID" --field digest
)"
TEST_SOURCE_DIGEST="${TEST_PLAN_SOURCE_DIGEST:-$(
    "$QEMU_ARTIFACT_HELPER" source-digest --repo-root "$PROJECT_ROOT"
)}"
if [[ "$TEST_SOURCE_DIGEST" != sha256:* ]]; then
    TEST_SOURCE_DIGEST="sha256:${TEST_SOURCE_DIGEST}"
fi
TEST_ATTEMPT_MANIFEST="${TEST_PLAN_ATTEMPT_MANIFEST:-}"
QEMU_ARTIFACT_ROOT="${COHSH_QEMU_ARTIFACT_ROOT:-}"
TRANSPORT_RESULT_ROOT="${COHSH_TRANSPORT_RESULT_ROOT:-}"
REQUIRE_RESULT_EVIDENCE="${COHSH_REQUIRE_RESULT_EVIDENCE:-0}"
TARGET_EVIDENCE_FILE="${TEST_PLAN_TARGET_EVIDENCE_FILE:-${PI4_TARGET_EVIDENCE_FILE:-}}"
RUN_ID="${COHSH_BATCH_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
if [[ "$BATCH_TARGET" == "pi4" && -z "${COHSH_LOG_ROOT:-}" ]]; then
    ARCHIVE_ROOT="${PROJECT_ROOT}/out/regression-logs/pi4-full-${RUN_ID}"
else
    ARCHIVE_ROOT="${COHSH_LOG_ROOT:-${PROJECT_ROOT}/out/regression-logs}"
fi
ARCHIVE_ROOT="$(canonical_path "$ARCHIVE_ROOT")"
if [[ -z "$QEMU_ARTIFACT_ROOT" ]]; then
    QEMU_ARTIFACT_ROOT="${ARCHIVE_ROOT}/qemu-artifacts"
fi
QEMU_ARTIFACT_ROOT="$(canonical_path "$QEMU_ARTIFACT_ROOT")"
if [[ -z "$TRANSPORT_RESULT_ROOT" ]]; then
    TRANSPORT_RESULT_ROOT="${ARCHIVE_ROOT}/transport-results"
fi
TRANSPORT_RESULT_ROOT="$(canonical_path "$TRANSPORT_RESULT_ROOT")"
TRANSPORT_EVIDENCE_ROOT="$(
    python3 - \
        "$ARCHIVE_ROOT" \
        "$QEMU_ARTIFACT_ROOT" \
        "$TRANSPORT_RESULT_ROOT" <<'PY'
from pathlib import Path
import os
import sys

print(Path(os.path.commonpath(sys.argv[1:])).resolve())
PY
)"
validate_output_roots
case "${COHSH_BATCH_PRINT_PATHS:-0}" in
    0)
        ;;
    1)
        printf 'ARCHIVE_ROOT=%s\n' "$ARCHIVE_ROOT"
        printf 'QEMU_ARTIFACT_ROOT=%s\n' "$QEMU_ARTIFACT_ROOT"
        printf 'TRANSPORT_RESULT_ROOT=%s\n' "$TRANSPORT_RESULT_ROOT"
        printf 'TRANSPORT_EVIDENCE_ROOT=%s\n' "$TRANSPORT_EVIDENCE_ROOT"
        exit 0
        ;;
    *)
        echo "COHSH_BATCH_PRINT_PATHS must be 0 or 1" >&2
        exit 2
        ;;
esac
TCP_HOST="${COHSH_TCP_HOST:-${COHSH_HOST:-127.0.0.1}}"
TCP_PORT="${COHSH_TCP_PORT:-${COHSH_PORT:-31337}}"
QEMU_TCP_HOST="127.0.0.1"
QEMU_TCP_PORT="${COHSH_QEMU_TCP_PORT:-}"
QEMU_UDP_PORT="${COHSH_QEMU_UDP_PORT:-}"
QEMU_SMOKE_PORT="${COHSH_QEMU_SMOKE_PORT:-}"
COHSH_RUN_TCP_HOST="$QEMU_TCP_HOST"
COHSH_RUN_TCP_PORT="$QEMU_TCP_PORT"
if [[ "$BATCH_TARGET" == "pi4" ]]; then
    BATCH_CONTINUE_ON_FAIL="${COHSH_BATCH_CONTINUE_ON_FAIL:-1}"
else
    BATCH_CONTINUE_ON_FAIL="${COHSH_BATCH_CONTINUE_ON_FAIL:-0}"
fi

GENERATED_OUTPUT_PATHS=(
    "apps/root-task/src/generated"
    "configs/generated/root_task_resolved.json"
    "configs/generated/root_task_resolved.json.sha256"
    "configs/generated/cas_manifest_template.json"
    "configs/generated/cas_manifest_template.json.sha256"
    "scripts/cohsh/boot_v0.coh"
    "docs/snippets/root_task_manifest.md"
    "docs/snippets/gpu_breadcrumbs.md"
    "docs/snippets/observability_interfaces.md"
    "docs/snippets/observability_security.md"
    "docs/snippets/ticket_quotas.md"
    "docs/snippets/trace_policy.md"
    "docs/snippets/cas_interfaces.md"
    "docs/snippets/cas_security.md"
    "docs/snippets/telemetry_cbor_schema.md"
    "tools/cohesix-py/cohesix/generated.py"
    "docs/snippets/cohesix_py_defaults.md"
    "docs/snippets/coh_doctor_checks.md"
    "apps/cohsh/src/generated/policy.rs"
    "docs/snippets/cohsh_policy.md"
    "apps/cohsh/src/generated/client.rs"
    "docs/snippets/cohsh_client.md"
    "docs/snippets/cohsh_grammar.md"
    "docs/snippets/cohsh_ticket_policy.md"
    "apps/coh/src/generated/policy.rs"
    "docs/snippets/coh_policy.md"
    "apps/swarmui/src/generated.rs"
    "docs/snippets/swarmui_defaults.md"
    "out/cohsh_policy.toml"
    "out/cohsh_policy.toml.sha256"
    "out/coh_policy.toml"
    "out/coh_policy.toml.sha256"
    "out/swarmui_defaults.toml"
    "out/swarmui_defaults.toml.sha256"
)
generated_snapshot_dir=""
generated_snapshot_ready=0

clear_generated_path() {
    local relative="$1"
    local absolute="${PROJECT_ROOT}/${relative}"
    case "$absolute" in
        "${PROJECT_ROOT}/"*)
            ;;
        *)
            echo "Refusing to clear generated path outside repository: $absolute" >&2
            return 1
            ;;
    esac
    if [[ -d "$absolute" && ! -L "$absolute" ]]; then
        find "$absolute" -mindepth 1 -delete
        rmdir "$absolute"
    else
        rm -f "$absolute"
    fi
}

snapshot_generated_outputs() {
    if [[ "$generated_snapshot_ready" == "1" ]]; then
        return 0
    fi
    generated_snapshot_dir="$(mktemp -d "${TMPDIR:-/tmp}/cohesix-generated.XXXXXX")"
    : >"${generated_snapshot_dir}/present"
    : >"${generated_snapshot_dir}/missing"
    local relative
    for relative in "${GENERATED_OUTPUT_PATHS[@]}"; do
        local source="${PROJECT_ROOT}/${relative}"
        if [[ -e "$source" || -L "$source" ]]; then
            mkdir -p "${generated_snapshot_dir}/files/$(dirname "$relative")"
            cp -pR "$source" "${generated_snapshot_dir}/files/${relative}"
            printf '%s\n' "$relative" >>"${generated_snapshot_dir}/present"
        else
            printf '%s\n' "$relative" >>"${generated_snapshot_dir}/missing"
        fi
    done
    generated_snapshot_ready=1
}

restore_generated_outputs() {
    if [[ "$generated_snapshot_ready" != "1" ]]; then
        return 0
    fi
    local relative
    for relative in "${GENERATED_OUTPUT_PATHS[@]}"; do
        if [[ -e "${PROJECT_ROOT}/${relative}" || -L "${PROJECT_ROOT}/${relative}" ]]; then
            clear_generated_path "$relative" || return 1
        fi
        if grep -Fx "$relative" "${generated_snapshot_dir}/present" >/dev/null; then
            mkdir -p "${PROJECT_ROOT}/$(dirname "$relative")"
            cp -pR \
                "${generated_snapshot_dir}/files/${relative}" \
                "${PROJECT_ROOT}/${relative}" || return 1
        fi
    done
    for relative in $(<"${generated_snapshot_dir}/present"); do
        if ! diff -qr \
            "${generated_snapshot_dir}/files/${relative}" \
            "${PROJECT_ROOT}/${relative}" >/dev/null; then
            echo "Failed to restore generated output exactly: ${relative}" >&2
            return 1
        fi
    done
    for relative in $(<"${generated_snapshot_dir}/missing"); do
        if [[ -e "${PROJECT_ROOT}/${relative}" || -L "${PROJECT_ROOT}/${relative}" ]]; then
            echo "Generated output should have been removed during restore: ${relative}" >&2
            return 1
        fi
    done
    find "$generated_snapshot_dir" -mindepth 1 -delete
    rmdir "$generated_snapshot_dir"
    generated_snapshot_dir=""
    generated_snapshot_ready=0
}

group_selected() {
    local group="$1"
    local selected="${COHSH_BATCH_GROUPS:-all}"
    if [[ -z "$selected" || "$selected" == "all" ]]; then
        return 0
    fi
    selected="${selected//,/ }"
    local item
    for item in $selected; do
        if [[ "$item" == "$group" ]]; then
            return 0
        fi
    done
    return 1
}

log_skip_group() {
    local group="$1"
    if [[ -n "${SUMMARY_LOG:-}" ]]; then
        printf "SKIP group=%s reason=COHSH_BATCH_GROUPS\n" "$group" | tee -a "$SUMMARY_LOG"
    else
        printf "SKIP group=%s reason=COHSH_BATCH_GROUPS\n" "$group"
    fi
}

selected_script_count() {
    local total=0
    if group_selected "base"; then
        total=$((total + ${#BASE_SCRIPTS[@]}))
    fi
    if group_selected "base-telemetry"; then
        total=$((total + ${#BASE_TELEMETRY_SCRIPTS[@]}))
    fi
    if group_selected "base-shard"; then
        total=$((total + ${#BASE_SHARD_SCRIPTS[@]}))
    fi
    if group_selected "gated"; then
        total=$((total + ${#GATED_SCRIPTS[@]}))
    fi
    printf "%s\n" "$total"
}

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

check_port_available() {
    local host="$1"
    local port="$2"
    local kind="$3"
    python3 - "$host" "$port" "$kind" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
kind = sys.argv[3]
sock_type = socket.SOCK_DGRAM if kind == "udp" else socket.SOCK_STREAM
try:
    with socket.socket(socket.AF_INET, sock_type) as sock:
        sock.bind((host, port))
        if sock_type == socket.SOCK_STREAM:
            sock.listen(1)
except (OSError, ValueError):
    raise SystemExit(1)
raise SystemExit(0)
PY
}

find_free_host_port() {
    local kind="$1"
    python3 - "$kind" <<'PY'
import socket
import sys

kind = sys.argv[1]
sock_type = socket.SOCK_DGRAM if kind == "udp" else socket.SOCK_STREAM
with socket.socket(socket.AF_INET, sock_type) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
}

resolve_qemu_host_ports() {
    if [[ -z "$QEMU_TCP_PORT" ]]; then
        QEMU_TCP_PORT="$(find_free_host_port tcp)"
    fi
    if [[ -z "$QEMU_UDP_PORT" ]]; then
        QEMU_UDP_PORT="$(find_free_host_port udp)"
    fi
    if [[ -z "$QEMU_SMOKE_PORT" ]]; then
        QEMU_SMOKE_PORT="$(find_free_host_port tcp)"
    fi
    while [[ "$QEMU_UDP_PORT" == "$QEMU_TCP_PORT" ]]; do
        QEMU_UDP_PORT="$(find_free_host_port udp)"
    done
    while [[ "$QEMU_SMOKE_PORT" == "$QEMU_TCP_PORT" \
        || "$QEMU_SMOKE_PORT" == "$QEMU_UDP_PORT" ]]; do
        QEMU_SMOKE_PORT="$(find_free_host_port tcp)"
    done
    printf \
        'INFO: QEMU host ports console=%s udp=%s smoke=%s\n' \
        "$QEMU_TCP_PORT" \
        "$QEMU_UDP_PORT" \
        "$QEMU_SMOKE_PORT"
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
    local args=(
        "$bin"
        --transport tcp
        --tcp-host "$tcp_host"
        --tcp-port "$tcp_port"
        --auth-token "${COHSH_AUTH_TOKEN}"
    )
    if [[ -n "${COHSH_RUN_POLICY:-}" ]]; then
        args+=(--policy "$COHSH_RUN_POLICY")
    fi
    args+=(--script "$script_path")
    "${args[@]}"
}

run_cohsh() {
    local script="$1"
    local script_path
    if ! script_path="$(script_path_for "$script")"; then
        return 2
    fi
    run_cohsh_file "$script_path"
}

reset_scoped_directory() {
    local directory="$1"
    case "$directory" in
        "$ARCHIVE_ROOT"|"$QEMU_ARTIFACT_ROOT"|\
        "${PROJECT_ROOT}/"*|"${ARCHIVE_ROOT}/"*|"${QEMU_ARTIFACT_ROOT}/"*)
            ;;
        *)
            echo "Refusing to reset directory outside scoped roots: $directory" >&2
            return 1
            ;;
    esac
    if [[ -d "$directory" ]]; then
        find "$directory" -mindepth 1 -delete
    else
        mkdir -p "$directory"
    fi
}

prepare_qemu_artifact() {
    local variant="$1"
    local manifest="$2"
    local artifact_dir="${QEMU_ARTIFACT_ROOT}/${variant}"
    local artifact_manifest="${artifact_dir}/qemu-artifact.json"
    local build_log="${ARCHIVE_ROOT}/build/${variant}.log"
    local selected_profile="${COHESIX_SEL4_PROFILE:-qemu_smp_production}"
    local smp="${COHESIX_QEMU_SMP_TOPO:-${QEMU_SMP_TOPO:-4,cores=4,threads=1,sockets=1}}"
    local virtualization="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-on}}"
    local machine_extra="${COHESIX_QEMU_MACHINE_EXTRA:-${QEMU_MACHINE_EXTRA:-}}"
    if [[ -z "$machine_extra" && "$(uname -s)" == "Darwin" ]]; then
        machine_extra="kernel-irqchip=off"
    fi

    snapshot_generated_outputs
    reset_scoped_directory "$artifact_dir"
    mkdir -p "$artifact_dir" "$(dirname "$build_log")"
    echo "INFO: building QEMU artifact variant=${variant} manifest=${manifest}"
    COHESIX_SEL4_PROFILE="$selected_profile" \
        COH_RTC_MANIFEST="$manifest" \
        SEL4_BUILD_DIR="$SEL4_BUILD_DIR" \
        "$BUILD_RUN_BIN" \
        --sel4-build "$SEL4_BUILD_DIR" \
        --out-dir "$artifact_dir" \
        --profile release \
        --root-task-features cohesix-dev \
        --cargo-target aarch64-unknown-none \
        --transport tcp \
        --no-run \
        >"$build_log" 2>&1

    local record_args=(
        record
        --artifact-dir "$artifact_dir"
        --output "$artifact_manifest"
        --manifest "$manifest"
        --resolved-manifest "$GENERATED_CONFIG_DIR/root_task_resolved.json"
        --policy "$PROJECT_ROOT/out/cohsh_policy.toml"
        --source-digest "$TEST_SOURCE_DIGEST"
        --sel4-build "$SEL4_BUILD_DIR"
        --sel4-profile "$selected_profile"
        --root-task-features cohesix-dev
        --cargo-target aarch64-unknown-none
        --smp "$smp"
        --virtualization "$virtualization"
        --machine-extra "$machine_extra"
        --net-backend virtio
        --detect-gic-script "$PROJECT_ROOT/scripts/lib/detect_gic_version.py"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
    )
    if [[ -n "$TEST_ATTEMPT_MANIFEST" && -f "$TEST_ATTEMPT_MANIFEST" ]]; then
        record_args+=(--attempt-manifest "$TEST_ATTEMPT_MANIFEST")
    elif [[ -n "$TEST_ATTEMPT_MANIFEST" ]]; then
        echo "WARN: attempt manifest is not yet available; omitting from artifact evidence: ${TEST_ATTEMPT_MANIFEST}" \
            >>"$build_log"
    fi
    "$QEMU_ARTIFACT_HELPER" "${record_args[@]}" >>"$build_log"
    echo "INFO: QEMU artifact ready variant=${variant} manifest=${artifact_manifest}"

    case "$variant" in
        base)
            BASE_QEMU_ARTIFACT_MANIFEST="$artifact_manifest"
            ;;
        gated)
            GATED_QEMU_ARTIFACT_MANIFEST="$artifact_manifest"
            ;;
        *)
            echo "Unknown QEMU artifact variant: $variant" >&2
            return 1
            ;;
    esac
}

write_qemu_result() {
    local name="$1"
    local artifact_manifest="$2"
    local boot_id="$3"
    local qemu_log="$4"
    shift 4
    local scripts=("$@")
    local result_path="${TRANSPORT_RESULT_ROOT}/${name}.json"
    local arguments=(
        result
        --output "$result_path"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
        --claim-tier "$TEST_CLAIM_TIER"
        --target qemu
        --source-digest "$TEST_SOURCE_DIGEST"
        --evidence-root "$TRANSPORT_EVIDENCE_ROOT"
        --artifact-manifest "$artifact_manifest"
        --artifact-action-id "$TEST_ACTION_ID"
        --artifact-catalog-action-digest "$TEST_ACTION_DIGEST"
        --boot-id "$boot_id"
        --group "$name"
        --status pass
        --log "$qemu_log"
    )
    local script
    for script in "${scripts[@]}"; do
        arguments+=(--script "$script")
        arguments+=(--log "${ARCHIVE_ROOT}/runtime/${name}/${script%.coh}.out.log")
    done
    mkdir -p "$TRANSPORT_RESULT_ROOT"
    "$QEMU_ARTIFACT_HELPER" "${arguments[@]}" >"${result_path}.id"
}

run_batch() {
    local name="$1"
    local artifact_manifest="$2"
    shift 2
    local scripts=("$@")

    if ! check_port_available "$QEMU_TCP_HOST" "$QEMU_TCP_PORT" tcp; then
        echo "QEMU console port already in use: ${QEMU_TCP_PORT}" >&2
        return 1
    fi
    if ! check_port_available "$QEMU_TCP_HOST" "$QEMU_UDP_PORT" udp; then
        echo "QEMU UDP echo port already in use: ${QEMU_UDP_PORT}" >&2
        return 1
    fi
    if ! check_port_available "$QEMU_TCP_HOST" "$QEMU_SMOKE_PORT" tcp; then
        echo "QEMU smoke port already in use: ${QEMU_SMOKE_PORT}" >&2
        return 1
    fi

    "$QEMU_ARTIFACT_HELPER" verify \
        --artifact-manifest "$artifact_manifest" \
        --source-digest "$TEST_SOURCE_DIGEST" \
        --action-id "$TEST_ACTION_ID" \
        --catalog-action-digest "$TEST_ACTION_DIGEST" >/dev/null

    local artifact_dir
    artifact_dir="$(dirname "$artifact_manifest")"
    local log_root="${ARCHIVE_ROOT}/runtime/${name}"
    local archive_root="${ARCHIVE_ROOT}/${name}"
    local qemu_log="${log_root}/regression_batch.qemu.log"
    local boot_id="${RUN_ID}-${name}-$$-${RANDOM}"

    mkdir -p "$log_root" "$archive_root"
    "$QEMU_ARTIFACT_HELPER" launch \
        --artifact-manifest "$artifact_manifest" \
        --source-digest "$TEST_SOURCE_DIGEST" \
        --action-id "$TEST_ACTION_ID" \
        --catalog-action-digest "$TEST_ACTION_DIGEST" \
        --console-port "$QEMU_TCP_PORT" \
        --udp-port "$QEMU_UDP_PORT" \
        --smoke-port "$QEMU_SMOKE_PORT" \
        >"$qemu_log" 2>&1 &
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

    COHSH_BIN="${artifact_dir}/host-tools/cohsh"
    COHSH_RUN_TCP_HOST="$QEMU_TCP_HOST"
    COHSH_RUN_TCP_PORT="$QEMU_TCP_PORT"
    COHSH_RUN_POLICY="${artifact_dir}/evidence/cohsh_policy.toml"

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
    write_qemu_result \
        "$name" \
        "$artifact_manifest" \
        "$boot_id" \
        "$qemu_log" \
        "${scripts[@]}"
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

write_pi4_result() {
    local name="$1"
    shift
    local scripts=("$@")
    if [[ -z "$TARGET_EVIDENCE_FILE" ]]; then
        if [[ "$REQUIRE_RESULT_EVIDENCE" == "1" ]]; then
            echo "Pi 4 transport evidence requires TEST_PLAN_TARGET_EVIDENCE_FILE" >&2
            return 1
        fi
        printf \
            "NO_CLAIM group=%s tier=pi4-transport reason=target-evidence-missing\n" \
            "$name" | tee -a "$SUMMARY_LOG"
        return 0
    fi

    local result_path="${TRANSPORT_RESULT_ROOT}/${name}.json"
    local arguments=(
        result
        --output "$result_path"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
        --claim-tier pi4-transport
        --target pi4
        --source-digest "$TEST_SOURCE_DIGEST"
        --evidence-root "$TRANSPORT_EVIDENCE_ROOT"
        --target-evidence "$TARGET_EVIDENCE_FILE"
        --group "$name"
        --status pass
        --log "$SUMMARY_LOG"
    )
    local script
    for script in "${scripts[@]}"; do
        arguments+=(--script "$script")
        arguments+=(--log "${ARCHIVE_ROOT}/${name}/${script%.coh}.out.log")
    done
    mkdir -p "$TRANSPORT_RESULT_ROOT"
    "$QEMU_ARTIFACT_HELPER" "${arguments[@]}" >"${result_path}.id"
}

write_transport_aggregate() {
    local target="$1"
    local claim_tier="$2"
    if [[ "$target" == "pi4" && -z "$TARGET_EVIDENCE_FILE" ]]; then
        return 0
    fi
    local aggregate_path="${TRANSPORT_RESULT_ROOT}/stage-03.json"
    local arguments=(
        aggregate
        --output "$aggregate_path"
        --action-id "$TEST_ACTION_ID"
        --catalog-action-digest "$TEST_ACTION_DIGEST"
        --claim-tier "$claim_tier"
        --target "$target"
        --source-digest "$TEST_SOURCE_DIGEST"
        --evidence-root "$TRANSPORT_EVIDENCE_ROOT"
    )
    local group
    for group in base base-telemetry base-shard gated; do
        if group_selected "$group"; then
            arguments+=(--result "${TRANSPORT_RESULT_ROOT}/${group}.json")
        fi
    done
    mkdir -p "$TRANSPORT_RESULT_ROOT"
    "$QEMU_ARTIFACT_HELPER" "${arguments[@]}" >"${aggregate_path}.id"
}

run_pi4_batch() {
    reset_scoped_directory "$ARCHIVE_ROOT"
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
    if group_selected "base"; then
        if ! run_live_group "base" "${BASE_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "base"
    fi
    if group_selected "base-telemetry"; then
        if ! run_live_group "base-telemetry" "${BASE_TELEMETRY_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "base-telemetry"
    fi
    if group_selected "base-shard"; then
        if ! run_live_group "base-shard" "${BASE_SHARD_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "base-shard"
    fi
    if group_selected "gated"; then
        if ! run_live_group "gated" "${GATED_SCRIPTS[@]}"; then
            return 1
        fi
    else
        log_skip_group "gated"
    fi

    # Fresh-boot proof after this point is intentionally UART/operator-driven;
    # TCP disappears during reset. Follow the top-of-file minicom notes.
    printf "RESULT pass=%s fail=%s total=%s log_root=%s\n" "$pi_pass" "$pi_fail" "$pi_total" "$ARCHIVE_ROOT" | tee -a "$SUMMARY_LOG"
    if (( pi_fail > 0 )); then
        return 1
    fi
    if group_selected "base"; then
        write_pi4_result "base" "${BASE_SCRIPTS[@]}"
    fi
    if group_selected "base-telemetry"; then
        write_pi4_result "base-telemetry" "${BASE_TELEMETRY_SCRIPTS[@]}"
    fi
    if group_selected "base-shard"; then
        write_pi4_result "base-shard" "${BASE_SHARD_SCRIPTS[@]}"
    fi
    if group_selected "gated"; then
        write_pi4_result "gated" "${GATED_SCRIPTS[@]}"
    fi
    write_transport_aggregate "pi4" "pi4-transport"
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
    local status=$?
    local cleanup_status=0
    set +e
    if (( qemu_pid > 0 )); then
        if kill -0 "$qemu_pid" 2>/dev/null; then
            kill "$qemu_pid" || true
            wait "$qemu_pid" 2>/dev/null || true
        fi
    fi
    if ! restore_generated_outputs; then
        cleanup_status=1
    fi
    if (( cleanup_status != 0 )); then
        status=1
    fi
    trap - EXIT
    exit "$status"
}
trap cleanup EXIT

if [[ "$BATCH_TARGET" == "pi4" ]]; then
    if [[ -n "$TARGET_EVIDENCE_FILE" ]]; then
        "$QEMU_ARTIFACT_HELPER" verify-pi4-evidence \
            --target-evidence "$TARGET_EVIDENCE_FILE" \
            --source-digest "$TEST_SOURCE_DIGEST" >/dev/null
    elif [[ "$REQUIRE_RESULT_EVIDENCE" == "1" ]]; then
        echo "Pi 4 transport evidence requires TEST_PLAN_TARGET_EVIDENCE_FILE" >&2
        exit 1
    fi
    run_pi4_batch
    exit $?
fi

if [[ ! -d "$SEL4_BUILD_DIR" ]]; then
    echo "Missing seL4 build directory: $SEL4_BUILD_DIR" >&2
    echo "Build the canonical profile or set SEL4_BUILD_DIR (or SEL4_BUILD) explicitly; default: ${PROJECT_ROOT}/out/sel4/profile-v2/qemu-smp-production" >&2
    exit 1
fi

resolve_qemu_host_ports
if [[ "${COHSH_BATCH_CLEAN_TARGET:-0}" == "1" ]]; then
    if [[ -d "${PROJECT_ROOT}/target" ]]; then
        find "${PROJECT_ROOT}/target" -mindepth 1 -delete
        rmdir "${PROJECT_ROOT}/target"
    fi
fi
reset_scoped_directory "$ARCHIVE_ROOT"
BASE_QEMU_ARTIFACT_MANIFEST=""
GATED_QEMU_ARTIFACT_MANIFEST=""

if group_selected "base" \
    || group_selected "base-telemetry" \
    || group_selected "base-shard"; then
    prepare_qemu_artifact "base" "$BASE_MANIFEST"
fi
if group_selected "gated"; then
    prepare_qemu_artifact "gated" "$GATED_MANIFEST"
fi
# The built artifacts carry private copies of their resolved manifest and
# client policy, so tracked/default generated outputs can be restored before
# any long-running QEMU boot begins.
restore_generated_outputs

case "${COHSH_BATCH_PREPARE_ONLY:-0}" in
    0)
        ;;
    1)
        if [[ -n "$BASE_QEMU_ARTIFACT_MANIFEST" ]]; then
            printf 'BASE_QEMU_ARTIFACT_MANIFEST=%s\n' "$BASE_QEMU_ARTIFACT_MANIFEST"
        fi
        if [[ -n "$GATED_QEMU_ARTIFACT_MANIFEST" ]]; then
            printf 'GATED_QEMU_ARTIFACT_MANIFEST=%s\n' "$GATED_QEMU_ARTIFACT_MANIFEST"
        fi
        exit 0
        ;;
    *)
        echo "COHSH_BATCH_PREPARE_ONLY must be 0 or 1" >&2
        exit 2
        ;;
esac

if group_selected "base"; then
    if ! run_batch "base" "$BASE_QEMU_ARTIFACT_MANIFEST" "${BASE_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "base"
fi

if group_selected "base-telemetry"; then
    if ! run_batch "base-telemetry" "$BASE_QEMU_ARTIFACT_MANIFEST" "${BASE_TELEMETRY_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "base-telemetry"
fi

if group_selected "base-shard"; then
    if ! run_batch "base-shard" "$BASE_QEMU_ARTIFACT_MANIFEST" "${BASE_SHARD_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "base-shard"
fi

if group_selected "gated"; then
    if ! run_batch "gated" "$GATED_QEMU_ARTIFACT_MANIFEST" "${GATED_SCRIPTS[@]}"; then
        exit 1
    fi
else
    log_skip_group "gated"
fi

write_transport_aggregate "qemu" "qemu-integration"
echo "regression batch complete: $(selected_script_count) scripts passed"
