#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute test-plan stage 04 (REST multiplexer regression batch).
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
tp_require_stage_done 3
tp_stage_begin 4 "rest-multiplexer"
tp_resolve_python_311
python_bin="${TP_PYTHON_RESOLVED}"

qemu_pid=0
gateway_pid=0
local_qemu_out=""
cohsh_bin="${COHSH_BIN:-${TEST_PLAN_ROOT}/out/cohesix/host-tools/cohsh}"
coh_bin="${TP_COH_BIN:-${TEST_PLAN_ROOT}/out/cohesix/host-tools/coh}"

stage4_kill_tree() {
  local pid="$1"
  local child
  while read -r child; do
    [[ -n "${child}" ]] || continue
    stage4_kill_tree "${child}"
  done < <(pgrep -P "${pid}" 2>/dev/null || true)
  kill "${pid}" >/dev/null 2>&1 || true
}

stage4_cleanup() {
  local status=$?
  if (( gateway_pid > 0 )) && kill -0 "${gateway_pid}" >/dev/null 2>&1; then
    stage4_kill_tree "${gateway_pid}"
    wait "${gateway_pid}" >/dev/null 2>&1 || true
  fi
  if (( qemu_pid > 0 )) && kill -0 "${qemu_pid}" >/dev/null 2>&1; then
    stage4_kill_tree "${qemu_pid}"
    wait "${qemu_pid}" >/dev/null 2>&1 || true
  fi
  exit "${status}"
}
trap stage4_cleanup EXIT

stage4_resolve_manifest_auth_token() {
  local manifest_path="$1"
  "${python_bin}" - "${manifest_path}" <<'PY'
import pathlib
import sys

manifest = pathlib.Path(sys.argv[1])
if not manifest.is_file():
    print("bootstrap")
    raise SystemExit(0)

try:
    import tomllib
except ModuleNotFoundError:
    print("bootstrap")
    raise SystemExit(0)

data = tomllib.loads(manifest.read_text(encoding="utf-8"))
for ticket in data.get("tickets", []):
    if str(ticket.get("role", "")).strip() == "queen":
        secret = str(ticket.get("secret", "")).strip()
        if secret:
            print(secret)
            raise SystemExit(0)
print("bootstrap")
PY
}

stage4_check_port_open() {
  local host="$1"
  local port="$2"
  "${python_bin}" - "${host}" "${port}" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
try:
    with socket.create_connection((host, port), timeout=0.5):
        raise SystemExit(0)
except OSError:
    raise SystemExit(1)
PY
}

stage4_find_free_port() {
  local host="$1"
  "${python_bin}" - "${host}" <<'PY'
import socket
import sys

host = sys.argv[1]
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind((host, 0))
    print(sock.getsockname()[1])
PY
}

stage4_parse_bind() {
  local bind="$1"
  local host="${bind%:*}"
  local port="${bind##*:}"
  if [[ -z "${host}" || "${host}" == "${bind}" || ! "${port}" =~ ^[0-9]+$ ]]; then
    tp_log "FAIL  invalid Stage 04 gateway bind: ${bind}"
    exit 1
  fi
  printf '%s\n%s\n' "${host}" "${port}"
}

stage4_wait_port_ready() {
  local host="$1"
  local port="$2"
  local timeout="$3"
  local pid="$4"
  local deadline=$((SECONDS + timeout))
  while (( SECONDS < deadline )); do
    if ! kill -0 "${pid}" >/dev/null 2>&1; then
      return 1
    fi
    if stage4_check_port_open "${host}" "${port}"; then
      return 0
    fi
    sleep 0.2
  done
  return 2
}

stage4_check_auth_ready() {
  local host="$1"
  local port="$2"
  local token="$3"
  "${python_bin}" - "${host}" "${port}" "${token}" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
token = sys.argv[3]
payload = f"AUTH {token}".encode()
frame = (len(payload) + 4).to_bytes(4, "little") + payload
try:
    with socket.create_connection((host, port), timeout=0.5) as sock:
        sock.settimeout(0.8)
        sock.sendall(frame)
        header = sock.recv(4)
        if len(header) != 4:
            raise SystemExit(1)
        total = int.from_bytes(header, "little")
        if total < 4 or total > 4096:
            raise SystemExit(1)
        body = bytearray()
        while len(body) < total - 4:
            chunk = sock.recv(total - 4 - len(body))
            if not chunk:
                break
            body.extend(chunk)
except OSError:
    raise SystemExit(1)

if b"OK AUTH" in body:
    raise SystemExit(0)
if b"ERR AUTH" in body:
    raise SystemExit(3)
raise SystemExit(1)
PY
}

stage4_wait_auth_ready() {
  local host="$1"
  local port="$2"
  local token="$3"
  local timeout="$4"
  local pid="$5"
  local deadline=$((SECONDS + timeout))
  local status
  while (( SECONDS < deadline )); do
    if ! kill -0 "${pid}" >/dev/null 2>&1; then
      return 1
    fi
    set +e
    stage4_check_auth_ready "${host}" "${port}" "${token}"
    status=$?
    set -e
    case "${status}" in
      0)
        return 0
        ;;
      3)
        return 3
        ;;
    esac
    sleep 0.2
  done
  return 2
}

stage4_wait_gateway_ready() {
  local url="$1"
  local token="$2"
  local bin="$3"
  local timeout="$4"
  local ready_script="${TEST_PLAN_STATE_DIR}/stage4-gateway-ready.coh"
  local deadline=$((SECONDS + timeout))

  cat >"${ready_script}" <<'EOF'
ping
EXPECT OK
quit
EOF

  while (( SECONDS < deadline )); do
    if (( gateway_pid > 0 )) && ! kill -0 "${gateway_pid}" >/dev/null 2>&1; then
      return 1
    fi
    if COHSH_REST_URL="${url}" \
      HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${token}" \
      "${bin}" --transport rest --rest-url "${url}" --rest-auth-token "${token}" --role queen --script "${ready_script}" \
      >>"${TP_LOG_FILE}" 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 2
}

gateway_url="${COHESIX_GATEWAY_URL:-${HIVE_GATEWAY_URL:-${COHSH_REST_URL:-${COH_REST_URL:-}}}}"
gateway_auth_token="${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:-${COHSH_REST_AUTH_TOKEN:-${COH_REST_AUTH_TOKEN:-}}}"
if [[ -z "${gateway_url}" ]]; then
  if [[ -n "${TP_STAGE4_QEMU_TCP_PORT:-}" ]]; then
    qemu_tcp_port="${TP_STAGE4_QEMU_TCP_PORT}"
  else
    qemu_tcp_port="$(stage4_find_free_port 127.0.0.1)"
  fi
  if [[ -n "${TP_STAGE4_GATEWAY_BIND:-}" ]]; then
    gateway_bind="${TP_STAGE4_GATEWAY_BIND}"
  else
    gateway_bind="127.0.0.1:$(stage4_find_free_port 127.0.0.1)"
  fi
  gateway_bind_parsed="$(stage4_parse_bind "${gateway_bind}")"
  gateway_bind_host="${gateway_bind_parsed%%$'\n'*}"
  gateway_bind_port="${gateway_bind_parsed##*$'\n'}"
  gateway_url="http://${gateway_bind}"
  gateway_auth_token="${TP_STAGE4_GATEWAY_AUTH_TOKEN:-test-plan-stage4-rest-token}"
  console_auth_token="${COHSH_AUTH_TOKEN:-${COH_AUTH_TOKEN:-$(stage4_resolve_manifest_auth_token "${TEST_PLAN_ROOT}/configs/root_task.toml")}}"
  local_qemu_out="${TEST_PLAN_STATE_DIR}/rest-qemu"
  cohsh_bin="${COHSH_BIN:-${local_qemu_out}/host-tools/cohsh}"
  coh_bin="${TP_COH_BIN:-${local_qemu_out}/host-tools/coh}"
  qemu_log="${TEST_PLAN_LOG_DIR}/stage4-local-qemu.log"
  gateway_log="${TEST_PLAN_LOG_DIR}/stage4-hive-gateway.log"
  sel4_build_dir="${TP_STAGE4_SEL4_BUILD_DIR:-${SEL4_BUILD_DIR:-${TEST_PLAN_ROOT}/seL4/SMP_build}}"

  if stage4_check_port_open 127.0.0.1 "${qemu_tcp_port}"; then
    tp_log "FAIL  local QEMU TCP port already in use: ${qemu_tcp_port}"
    tp_log "set COHESIX_GATEWAY_URL for an existing gateway, or free the port before running Stage 04"
    exit 1
  fi
  if stage4_check_port_open "${gateway_bind_host}" "${gateway_bind_port}"; then
    tp_log "FAIL  local hive-gateway bind port already in use: ${gateway_bind}"
    tp_log "set TP_STAGE4_GATEWAY_BIND to a free host:port, or set COHESIX_GATEWAY_URL for an existing gateway"
    exit 1
  fi

  tp_log "INFO  no gateway URL supplied; starting local QEMU + hive-gateway"
  tp_log "INFO  local-qemu-port=${qemu_tcp_port}"
  tp_log "INFO  local-qemu-out=${local_qemu_out}"
  tp_log "INFO  local-gateway-url=${gateway_url}"

  SEL4_BUILD_DIR="${sel4_build_dir}" COH_RTC_MANIFEST="${TEST_PLAN_ROOT}/configs/root_task.toml" \
    "${TEST_PLAN_ROOT}/scripts/cohesix-build-run.sh" \
    --sel4-build "${sel4_build_dir}" \
    --out-dir "${local_qemu_out}" \
    --profile release \
    --root-task-features cohesix-dev \
    --cargo-target aarch64-unknown-none \
    --raw-qemu \
    --transport tcp \
    --tcp-port "${qemu_tcp_port}" \
    >"${qemu_log}" 2>&1 &
  qemu_pid=$!

  ready_timeout="${TP_STAGE4_READY_TIMEOUT:-900}"
  auth_ready_timeout="${TP_STAGE4_AUTH_READY_TIMEOUT:-120}"
  if ! stage4_wait_port_ready 127.0.0.1 "${qemu_tcp_port}" "${ready_timeout}" "${qemu_pid}"; then
    tp_log "FAIL  local QEMU TCP console did not become reachable"
    tail -n 80 "${qemu_log}" >&2 || true
    exit 1
  fi
  if stage4_wait_auth_ready 127.0.0.1 "${qemu_tcp_port}" "${console_auth_token}" "${auth_ready_timeout}" "${qemu_pid}"; then
    :
  else
    auth_status=$?
    if [[ "${auth_status}" -eq 3 ]]; then
      tp_log "FAIL  local QEMU TCP auth rejected the selected console token"
    else
      tp_log "FAIL  local QEMU TCP auth endpoint did not become responsive"
    fi
    tail -n 80 "${qemu_log}" >&2 || true
    exit 1
  fi

  if [[ ! -x "${local_qemu_out}/host-tools/hive-gateway" ]]; then
    tp_log "FAIL  hive-gateway binary missing after local QEMU build: ${local_qemu_out}/host-tools/hive-gateway"
    exit 1
  fi

  "${local_qemu_out}/host-tools/hive-gateway" \
    --bind "${gateway_bind}" \
    --tcp-host 127.0.0.1 \
    --tcp-port "${qemu_tcp_port}" \
    --auth-token "${console_auth_token}" \
    --request-auth-token "${gateway_auth_token}" \
    >"${gateway_log}" 2>&1 &
  gateway_pid=$!

  gateway_ready_timeout="${TP_STAGE4_GATEWAY_READY_TIMEOUT:-120}"
  if ! stage4_wait_gateway_ready "${gateway_url}" "${gateway_auth_token}" "${cohsh_bin}" "${gateway_ready_timeout}"; then
    tp_log "FAIL  local hive-gateway did not become REST-ready"
    tail -n 80 "${gateway_log}" >&2 || true
    exit 1
  fi
fi
tp_log "INFO  gateway-url=${gateway_url}"
if [[ -z "${gateway_auth_token}" ]]; then
  tp_log "FAIL  missing gateway request auth token"
  tp_log "set HIVE_GATEWAY_REQUEST_AUTH_TOKEN (or COHSH_REST_AUTH_TOKEN/COH_REST_AUTH_TOKEN) before running stage 04"
  exit 1
fi
tp_log "INFO  gateway-auth-token=present"

# Keep Stage 04 on scripts that are parity-safe over the REST file projection.
# `busy_backpressure.coh` and `policy_gate.coh` depend on console-parser semantics
# and remain covered in the TCP regression matrix (Stage 03).
core_scripts="boot_v0.coh observe_watch.coh session_pool.coh"
parity_scripts="rest_control_plane_smoke.coh"

tp_run_cmd \
  "cohsh-rest-regression-core" \
  env \
  COHESIX_GATEWAY_URL="${gateway_url}" \
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${gateway_auth_token}" \
  COHSH_BIN="${cohsh_bin}" \
  COHSH_LOG_ROOT="${TEST_PLAN_STATE_DIR}/rest-regression-logs" \
  COHSH_BATCH_NAME="rest-regression-core" \
  COHSH_PARALLELISM=3 \
  COHSH_SCRIPT_LIST="${core_scripts}" \
  "${TEST_PLAN_ROOT}/scripts/cohsh/REST_regression_batch.sh"

tp_run_cmd \
  "cohsh-rest-regression-parity" \
  env \
  COHESIX_GATEWAY_URL="${gateway_url}" \
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${gateway_auth_token}" \
  COHSH_BIN="${cohsh_bin}" \
  COHSH_LOG_ROOT="${TEST_PLAN_STATE_DIR}/rest-regression-logs" \
  COHSH_BATCH_NAME="rest-regression-parity" \
  COHSH_PARALLELISM=1 \
  COHSH_SCRIPT_LIST="${parity_scripts}" \
  "${TEST_PLAN_ROOT}/scripts/cohsh/REST_regression_batch.sh"

if [[ "${TP_SKIP_PYTHON:-0}" == "1" ]]; then
  tp_mark_incomplete \
    "python-rest-smoke" \
    "TP_SKIP_PYTHON=1" \
    "REST smoke via cohesix-py was not executed; Python REST parity is unproven."
else
  tp_run_shell "python-rest-smoke" \
    "COHESIX_GATEWAY_URL=\"${gateway_url}\" HIVE_GATEWAY_REQUEST_AUTH_TOKEN=\"${gateway_auth_token}\" \"${python_bin}\" - <<'PY'
import os
import sys
import time
from pathlib import Path

repo_root = Path.cwd()
sys.path.insert(0, str(repo_root / \"tools\" / \"cohesix-py\"))

from cohesix.backends import RestBackend

gateway_url = os.environ[\"COHESIX_GATEWAY_URL\"]
backend = RestBackend(
    gateway_url,
    timeout_s=10.0,
    max_attempts=6,
    backoff_ms=200,
    backoff_ceiling_ms=2000,
)

def with_retry(label, fn):
    delay_s = 0.5
    for attempt in range(1, 13):
        try:
            return fn()
        except Exception as exc:
            message = str(exc).lower()
            transient = (
                \"429\" in message
                or \"backpressure\" in message
                or \"timed out\" in message
            )
            if (not transient) or attempt == 12:
                raise
            time.sleep(delay_s)
            delay_s = min(delay_s * 2, 5.0)

root_entries = with_retry(\"list_dir\", lambda: backend.list_dir(\"/\"))
if not root_entries:
    raise SystemExit(\"REST smoke failed: root listing is empty\")
state_payload = with_retry(
    \"read_file\",
    lambda: backend.read_file(\"/proc/lifecycle/state\", 128),
)
print(
    f\"python-rest-smoke root_entries={len(root_entries)} state_bytes={len(state_payload)}\"
)
PY"
fi

if [[ "${TP_SKIP_FUSE:-0}" == "1" ]]; then
  tp_mark_incomplete \
    "coh-rest-mount-regression" \
    "TP_SKIP_FUSE=1" \
    "REST FUSE mount regression was skipped; mount semantics and exclusivity are unproven."
elif [[ "$(uname -s)" != "Linux" ]]; then
  tp_log "NA    coh-rest-mount-regression (Linux-only)"
elif [[ ! -e /dev/fuse ]]; then
  tp_mark_incomplete \
    "coh-rest-mount-regression" \
    "/dev/fuse missing" \
    "FUSE is not available on the Linux host; REST mount correctness cannot be validated."
elif ! command -v fusermount3 >/dev/null 2>&1; then
  tp_mark_incomplete \
    "coh-rest-mount-regression" \
    "fusermount3 missing" \
    "FUSE tooling is missing on the Linux host; REST mount correctness cannot be validated."
else
  if [[ ! -x "${coh_bin}" ]]; then
    tp_mark_incomplete \
      "coh-rest-mount-regression" \
      "coh binary missing: ${coh_bin}" \
      "REST mount regression requires the host coh binary; rebuild host tools or set TP_COH_BIN."
  else
    tp_run_shell "coh-rest-mount-regression" \
      "set -euo pipefail
mount_dir=\"${TEST_PLAN_STATE_DIR}/coh-mount-rest\"
mount_log=\"${TEST_PLAN_LOG_DIR}/coh-rest-mount.log\"
mkdir -p \"${TEST_PLAN_LOG_DIR}\"
mkdir -p \"${mount_dir}\"
if grep -F \" ${mount_dir} \" /proc/mounts >/dev/null 2>&1; then
  fusermount3 -u \"${mount_dir}\" || true
fi

cleanup() {
  if grep -F \" ${mount_dir} \" /proc/mounts >/dev/null 2>&1; then
    fusermount3 -u \"${mount_dir}\" || true
  fi
  if [[ -n \"${coh_mount_pid:-}\" ]]; then
    wait \"${coh_mount_pid}\" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

COH_REST_URL=\"${gateway_url}\" HIVE_GATEWAY_REQUEST_AUTH_TOKEN=\"${gateway_auth_token}\" \\
  \"${coh_bin}\" mount --rest-url \"${gateway_url}\" --rest-auth-token \"${gateway_auth_token}\" \\
  --at \"${mount_dir}\" >\"${mount_log}\" 2>&1 &
coh_mount_pid=$!

for i in \$(seq 1 50); do
  if grep -F \" ${mount_dir} \" /proc/mounts >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 \"${coh_mount_pid}\" >/dev/null 2>&1; then
    echo \"coh mount exited early\"
    tail -n 80 \"${mount_log}\" || true
    exit 1
  fi
  sleep 0.2
done
if ! grep -F \" ${mount_dir} \" /proc/mounts >/dev/null 2>&1; then
  echo \"coh mount did not become active\"
  tail -n 80 \"${mount_log}\" || true
  exit 1
fi

state_path=\"${mount_dir}/proc/lifecycle/state\"
if [[ ! -f \"${state_path}\" ]]; then
  echo \"missing mount file: ${state_path}\"
  exit 1
fi
state_bytes=\$(wc -c <\"${state_path}\" | tr -d ' ')
if [[ \"${state_bytes}\" -lt 4 ]]; then
  echo \"expected non-empty lifecycle state; got bytes=${state_bytes}\"
  exit 1
fi

log_path=\"${mount_dir}/log/queen.log\"
if [[ ! -f \"${log_path}\" ]]; then
  echo \"missing mount file: ${log_path}\"
  exit 1
fi
log_bytes=\$(wc -c <\"${log_path}\" | tr -d ' ')
if [[ \"${log_bytes}\" -lt 1 ]]; then
  echo \"expected non-empty queen log; got bytes=${log_bytes}\"
  exit 1
fi

dev_id=\"tp-mount-\$(date +%s)\"
echo '{\"new\":\"segment\",\"mime\":\"text/plain\"}' >>\"${mount_dir}/queen/telemetry/${dev_id}/ctl\"
echo \"hello-from-mount ts_ms=\$(date +%s000)\" >>\"${mount_dir}/queen/telemetry/${dev_id}/seg/seg-000001\"

latest_path=\"${mount_dir}/queen/telemetry/${dev_id}/latest\"
if [[ ! -f \"${latest_path}\" ]]; then
  echo \"missing latest pointer: ${latest_path}\"
  exit 1
fi
latest=\$(cat \"${latest_path}\" | tr -d '\\r\\n')
if [[ \"${latest}\" != \"seg-000001\" ]]; then
  echo \"expected latest=seg-000001; got latest=${latest}\"
  exit 1
fi
if ! grep -q \"hello-from-mount\" \"${mount_dir}/queen/telemetry/${dev_id}/seg/seg-000001\"; then
  echo \"expected appended telemetry record missing\"
  exit 1
fi"
  fi
fi

tp_stage_complete 4
