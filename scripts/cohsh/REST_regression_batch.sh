#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run concurrent cohsh regression scripts through the REST multiplexer.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SCRIPT_ROOT="${COHSH_SCRIPT_ROOT:-${ROOT_DIR}/scripts/cohsh}"
COHSH_BIN="${COHSH_BIN:-${ROOT_DIR}/out/cohesix/host-tools/cohsh}"

GATEWAY_URL="${COHESIX_GATEWAY_URL:-${HIVE_GATEWAY_URL:-${COHSH_REST_URL:-${COH_REST_URL:-}}}}"
GATEWAY_AUTH_TOKEN="${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:-${COHSH_REST_AUTH_TOKEN:-${COH_REST_AUTH_TOKEN:-}}}"
PARALLELISM="${COHSH_PARALLELISM:-3}"
REPEAT="${COHSH_REPEAT:-1}"
BATCH_NAME="${COHSH_BATCH_NAME:-rest-regression}"
LOG_ROOT="${COHSH_LOG_ROOT:-${ROOT_DIR}/out/regression-logs}"

usage() {
  cat <<'USAGE'
Usage: scripts/cohsh/REST_regression_batch.sh

Runs a concurrent cohsh regression batch over the hive-gateway REST multiplexer.

Required env:
  COHESIX_GATEWAY_URL (or HIVE_GATEWAY_URL/COHSH_REST_URL/COH_REST_URL)
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN (or COHSH_REST_AUTH_TOKEN/COH_REST_AUTH_TOKEN)

Optional env:
  COHSH_BIN (default: out/cohesix/host-tools/cohsh)
  COHSH_SCRIPT_ROOT (default: scripts/cohsh)
  COHSH_PARALLELISM (default: 3)
  COHSH_REPEAT (default: 1)
  COHSH_BATCH_NAME (default: rest-regression)
  COHSH_LOG_ROOT (default: out/regression-logs)
  COHSH_SCRIPT_LIST (space-delimited list of .coh scripts)

Notes:
  - Uses REST transport only (`--transport rest`).
  - No IP addresses or SSH keys are hardcoded; set the gateway URL explicitly.
USAGE
}

fail() {
  echo "$1" >&2
  exit 1
}

wait_for_gateway_ready() {
  local attempts=30
  local delay=1
  local tmp_script
  tmp_script="$(mktemp -t cohsh-rest-ready.XXXXXX.coh)"
  cat >"${tmp_script}" <<'EOF'
ping
EXPECT OK
quit
EOF
  for ((i = 0; i < attempts; i += 1)); do
    if COHSH_REST_URL="${GATEWAY_URL}" \
      HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${GATEWAY_AUTH_TOKEN}" \
      "${COHSH_BIN}" --transport rest --role queen --script "${tmp_script}" \
      >/dev/null 2>&1; then
      rm -f "${tmp_script}"
      return 0
    fi
    sleep "${delay}"
  done
  rm -f "${tmp_script}"
  fail "gateway did not become ready after ${attempts} attempts (${GATEWAY_URL})"
}

if [[ -z "${GATEWAY_URL}" ]]; then
  usage >&2
  fail "Gateway URL is required (set COHESIX_GATEWAY_URL or HIVE_GATEWAY_URL)"
fi

if [[ -z "${GATEWAY_AUTH_TOKEN}" ]]; then
  usage >&2
  fail "Gateway request auth token is required (set HIVE_GATEWAY_REQUEST_AUTH_TOKEN or COHSH_REST_AUTH_TOKEN)"
fi

if [[ ! -x "${COHSH_BIN}" ]]; then
  fail "cohsh binary not found or not executable: ${COHSH_BIN}"
fi

if [[ ! -d "${SCRIPT_ROOT}" ]]; then
  fail "script root not found: ${SCRIPT_ROOT}"
fi

DEFAULT_SCRIPTS=(
  "boot_v0.coh"
  "observe_watch.coh"
  "busy_backpressure.coh"
  "session_pool.coh"
  "tcp_basic.coh"
)

if [[ -n "${COHSH_SCRIPT_LIST:-}" ]]; then
  read -r -a SCRIPTS <<< "${COHSH_SCRIPT_LIST}"
else
  SCRIPTS=("${DEFAULT_SCRIPTS[@]}")
fi

wait_for_gateway_ready

OUT_DIR="${LOG_ROOT}/${BATCH_NAME}"
mkdir -p "${OUT_DIR}"

JOB_LIST=()
for script in "${SCRIPTS[@]}"; do
  if [[ ! -f "${SCRIPT_ROOT}/${script}" ]]; then
    fail "script not found: ${SCRIPT_ROOT}/${script}"
  fi
  for ((i = 0; i < REPEAT; i += 1)); do
    JOB_LIST+=("${script}")
  done
done

run_one() {
  local script="$1"
  local index="$2"
  local log_path="${OUT_DIR}/${script%.coh}.run${index}.log"
  printf "[rest-batch] start %s (#%s)\n" "$script" "$index"
  COHSH_REST_URL="${GATEWAY_URL}" \
    HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${GATEWAY_AUTH_TOKEN}" \
    "${COHSH_BIN}" --transport rest --script "${SCRIPT_ROOT}/${script}" \
    >"${log_path}" 2>&1
}

if [[ "${#JOB_LIST[@]}" -eq 0 ]]; then
  fail "no scripts selected"
fi

if [[ "${PARALLELISM}" -lt 1 ]]; then
  fail "COHSH_PARALLELISM must be >= 1"
fi

failures=0
index=0
pids=()
scripts_for_pid=()

for script in "${JOB_LIST[@]}"; do
  index=$((index + 1))
  run_one "${script}" "${index}" &
  pids+=("$!")
  scripts_for_pid+=("${script}")
  if [[ "${#pids[@]}" -ge "${PARALLELISM}" ]]; then
    for i in "${!pids[@]}"; do
      if ! wait "${pids[$i]}"; then
        echo "[rest-batch] FAIL: ${scripts_for_pid[$i]}" >&2
        failures=$((failures + 1))
      else
        echo "[rest-batch] PASS: ${scripts_for_pid[$i]}"
      fi
    done
    pids=()
    scripts_for_pid=()
  fi
done

if [[ "${#pids[@]}" -gt 0 ]]; then
  for i in "${!pids[@]}"; do
    if ! wait "${pids[$i]}"; then
      echo "[rest-batch] FAIL: ${scripts_for_pid[$i]}" >&2
      failures=$((failures + 1))
    else
      echo "[rest-batch] PASS: ${scripts_for_pid[$i]}"
    fi
  done
fi

if [[ "${failures}" -ne 0 ]]; then
  fail "REST regression batch failed: ${failures} script(s) failed"
fi

printf "[rest-batch] complete: %s runs passed\n" "${#JOB_LIST[@]}"
