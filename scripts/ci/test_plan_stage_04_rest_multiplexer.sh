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

gateway_url="${COHESIX_GATEWAY_URL:-${HIVE_GATEWAY_URL:-${COHSH_REST_URL:-${COH_REST_URL:-}}}}"
gateway_auth_token="${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:-${COHSH_REST_AUTH_TOKEN:-${COH_REST_AUTH_TOKEN:-}}}"
if [[ -z "${gateway_url}" ]]; then
  tp_log "FAIL  missing gateway URL"
  tp_log "set COHESIX_GATEWAY_URL (or HIVE_GATEWAY_URL/COHSH_REST_URL/COH_REST_URL) before running stage 04"
  exit 1
fi
tp_log "INFO  gateway-url=${gateway_url}"
if [[ -z "${gateway_auth_token}" ]]; then
  tp_log "FAIL  missing gateway request auth token"
  tp_log "set HIVE_GATEWAY_REQUEST_AUTH_TOKEN (or COHSH_REST_AUTH_TOKEN/COH_REST_AUTH_TOKEN) before running stage 04"
  exit 1
fi
tp_log "INFO  gateway-auth-token=present"

core_scripts="boot_v0.coh observe_watch.coh busy_backpressure.coh session_pool.coh"
parity_scripts="policy_gate.coh rest_control_plane_smoke.coh"

tp_run_cmd \
  "cohsh-rest-regression-core" \
  env \
  COHESIX_GATEWAY_URL="${gateway_url}" \
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${gateway_auth_token}" \
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
  python_bin="${TP_PYTHON_BIN:-python3}"
  tp_run_shell "python-rest-smoke" \
    "COHESIX_GATEWAY_URL=\"${gateway_url}\" HIVE_GATEWAY_REQUEST_AUTH_TOKEN=\"${gateway_auth_token}\" \"${python_bin}\" - <<'PY'
import os
import sys
from pathlib import Path

repo_root = Path.cwd()
sys.path.insert(0, str(repo_root / \"tools\" / \"cohesix-py\"))

from cohesix.backends import RestBackend

gateway_url = os.environ[\"COHESIX_GATEWAY_URL\"]
backend = RestBackend(gateway_url)

root_entries = backend.list_dir(\"/\")
if not root_entries:
    raise SystemExit(\"REST smoke failed: root listing is empty\")
log_payload = backend.read_file(\"/log/queen.log\", 4096)
print(f\"python-rest-smoke root_entries={len(root_entries)} log_bytes={len(log_payload)}\")
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
  coh_bin="${TP_COH_BIN:-${TEST_PLAN_ROOT}/out/cohesix/host-tools/coh}"
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
