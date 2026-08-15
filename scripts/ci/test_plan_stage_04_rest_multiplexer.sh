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

target="${TEST_PLAN_TARGET:-qemu}"
case "${target}" in
  qemu)
    action_id="qemu.rest-regression"
    claim_tier="qemu-integration"
    prior_action_id="qemu.tcp-regression"
    ;;
  pi4)
    action_id="pi4.rest-regression"
    claim_tier="pi4-transport"
    prior_action_id="pi4.tcp-regression"
    ;;
  *)
    tp_log "FAIL  Stage 04 target must be qemu or pi4, got ${target}"
    exit 2
    ;;
esac
catalog_tool="${TEST_PLAN_ROOT}/scripts/ci/test_plan_catalog.py"
artifact_helper="${TEST_PLAN_ROOT}/scripts/ci/qemu_artifact.py"
catalog_digest="sha256:$(
  python3 "${catalog_tool}" action --id "${action_id}" --field digest
)"
prior_catalog_digest="sha256:$(
  python3 "${catalog_tool}" action --id "${prior_action_id}" --field digest
)"
source_digest="sha256:${TEST_PLAN_SOURCE_DIGEST#sha256:}"
stage3_pointer="${TEST_PLAN_STATE_DIR}/stage_03_artifact_root.path"
stage3_root=""
if [[ -s "${stage3_pointer}" ]]; then
  stage3_root="$(
    "${artifact_helper}" \
      resolve-root \
      --state-dir "${TEST_PLAN_STATE_DIR}" \
      --pointer "${stage3_pointer}"
  )"
elif [[ "${TEST_PLAN_STAGED_RUN:-0}" == "1" ]]; then
  tp_log "FAIL  staged Stage 04 requires the Stage 03 artifact-root pointer"
  tp_log "FAIL  missing=${stage3_pointer}"
  exit 1
fi
stage3_artifact=""
if [[ -n "${stage3_root}" ]]; then
  stage3_artifact="${stage3_root}/qemu-artifacts/base/qemu-artifact.json"
elif [[ -n "${TP_STAGE4_QEMU_ARTIFACT_MANIFEST:-}" ]]; then
  stage3_artifact="${TP_STAGE4_QEMU_ARTIFACT_MANIFEST}"
fi
if [[ -n "${TP_STAGE4_QEMU_ARTIFACT_MANIFEST:-}" && -n "${stage3_root}" ]]; then
  explicit_stage3_artifact="$(
    python3 -c \
      'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve(strict=False))' \
      "${TP_STAGE4_QEMU_ARTIFACT_MANIFEST}"
  )"
  canonical_stage3_artifact="$(
    python3 -c \
      'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve(strict=False))' \
      "${stage3_artifact}"
  )"
  if [[ "${explicit_stage3_artifact}" != "${canonical_stage3_artifact}" ]]; then
    tp_log "FAIL  TP_STAGE4_QEMU_ARTIFACT_MANIFEST cannot replace staged Stage 03 evidence"
    exit 1
  fi
fi
stage4_root="${TP_ATTEMPT_DIR}/rest"
rest_log_root="${stage4_root}/regression-logs"
stage4_runtime_root="${stage4_root}/runtime"
stage4_result_dir="${stage4_root}/results"
stage4_result="${stage4_result_dir}/stage-04.json"
target_evidence_input="${TEST_PLAN_TARGET_EVIDENCE_FILE:-${TP_PI4_TARGET_EVIDENCE_FILE:-}}"
target_evidence=""
stage4_boot_id=""
stage4_artifact_manifest=""
mkdir -p \
  "${stage4_root}" \
  "${rest_log_root}" \
  "${stage4_runtime_root}" \
  "${stage4_result_dir}"

qemu_pid=0
gateway_pid=0
cohsh_bin="${COHSH_BIN:-${TEST_PLAN_ROOT}/out/cohesix/host-tools/cohsh}"
coh_bin="${TP_COH_BIN:-${TEST_PLAN_ROOT}/out/cohesix/host-tools/coh}"
readonly stage4_gateway_queue_wait_limit_ms=5000
readonly stage4_rest_response_delivery_grace_ms=5000
readonly stage4_default_gateway_control_response_timeout_ms=120000
readonly stage4_default_gateway_telemetry_response_timeout_ms=120000
readonly stage4_max_gateway_response_timeout_ms=1200000
readonly stage4_max_rest_client_response_timeout_ms=1210000
readonly stage4_context_environment_names=(
  COHESIX_GATEWAY_URL
  COHSH_REST_RESPONSE_TIMEOUT_MS
  COHSH_REST_URL
  COH_REST_URL
  HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS
  HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN
  HIVE_GATEWAY_URL
  TP_STAGE4_FUSE_COH_BIN
  TP_STAGE4_FUSE_MOUNT_DIR
  TP_STAGE4_FUSE_MOUNT_LOG
)
stage4_context_environment_present=()
stage4_context_environment_values=()

stage4_capture_context_environment() {
  local index
  local name
  for ((index = 0; index < ${#stage4_context_environment_names[@]}; index += 1)); do
    name="${stage4_context_environment_names[index]}"
    if [[ -n "${!name+x}" ]]; then
      stage4_context_environment_present[index]=1
      stage4_context_environment_values[index]="${!name}"
    else
      stage4_context_environment_present[index]=0
      stage4_context_environment_values[index]=""
    fi
  done
}

stage4_restore_context_environment() {
  local index
  local name
  for ((index = 0; index < ${#stage4_context_environment_names[@]}; index += 1)); do
    name="${stage4_context_environment_names[index]}"
    if [[ "${stage4_context_environment_present[index]}" == "1" ]]; then
      printf -v "${name}" '%s' "${stage4_context_environment_values[index]}"
      export "${name}"
    else
      unset "${name}"
    fi
  done
}

# Stage 04 exports its resolved endpoint and timeout contract for child tools.
# Preserve the inherited input context so those runner-owned values cannot be
# misclassified as source/configuration drift when the attempt is finalized.
stage4_capture_context_environment

stage4_validate_timeout_ms() {
  local label="$1"
  local value="$2"
  local minimum="$3"
  local maximum="$4"
  if [[ ! "${value}" =~ ^[0-9]+$ ]] || [[ "${#value}" -gt 10 ]]; then
    tp_log "FAIL  ${label} must be a decimal millisecond value" >&2
    return 1
  fi
  value="$((10#${value}))"
  if (( value < minimum || value > maximum )); then
    tp_log "FAIL  ${label} must be within ${minimum}..${maximum}ms, got ${value}ms" >&2
    return 1
  fi
  printf '%s\n' "${value}"
}

stage4_resolve_timeout_override() {
  local label="$1"
  local harness_name="$2"
  local product_name="$3"
  local default_value="$4"
  local require_explicit="$5"
  local minimum="$6"
  local maximum="$7"
  local harness_value="${!harness_name-}"
  local product_value="${!product_name-}"
  local normalized_harness=""
  local normalized_product=""

  if [[ -n "${harness_value}" ]]; then
    normalized_harness="$(
      stage4_validate_timeout_ms \
        "${harness_name}" \
        "${harness_value}" \
        "${minimum}" \
        "${maximum}"
    )" || return 1
  fi
  if [[ -n "${product_value}" ]]; then
    normalized_product="$(
      stage4_validate_timeout_ms \
        "${product_name}" \
        "${product_value}" \
        "${minimum}" \
        "${maximum}"
    )" || return 1
  fi
  if [[ -n "${normalized_harness}" \
    && -n "${normalized_product}" \
    && "${normalized_harness}" != "${normalized_product}" ]]; then
    tp_log "FAIL  conflicting ${label}: ${harness_name}=${normalized_harness} ${product_name}=${normalized_product}" >&2
    return 1
  fi
  if [[ -n "${normalized_harness}" ]]; then
    printf '%s\n' "${normalized_harness}"
    return 0
  fi
  if [[ -n "${normalized_product}" ]]; then
    printf '%s\n' "${normalized_product}"
    return 0
  fi
  if [[ "${require_explicit}" == "1" ]]; then
    tp_log "FAIL  external gateway requires explicit ${label} via ${harness_name} or ${product_name}" >&2
    return 1
  fi
  stage4_validate_timeout_ms \
    "default ${label}" \
    "${default_value}" \
    "${minimum}" \
    "${maximum}"
}

stage4_resolve_timeout_contract() {
  local external_gateway="$1"
  local require_gateway_declaration=0
  local maximum_gateway_timeout_ms
  local minimum_client_timeout_ms
  if [[ "${external_gateway}" == "1" ]]; then
    require_gateway_declaration=1
    stage4_gateway_timeout_declaration="external-explicit"
  else
    stage4_gateway_timeout_declaration="local-configured"
  fi

  stage4_gateway_control_response_timeout_ms="$(
    stage4_resolve_timeout_override \
      "gateway control broker response timeout" \
      TP_STAGE4_GATEWAY_CONTROL_RESPONSE_TIMEOUT_MS \
      HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS \
      "${stage4_default_gateway_control_response_timeout_ms}" \
      "${require_gateway_declaration}" \
      "${stage4_gateway_queue_wait_limit_ms}" \
      "${stage4_max_gateway_response_timeout_ms}"
  )" || return 1
  stage4_gateway_telemetry_response_timeout_ms="$(
    stage4_resolve_timeout_override \
      "gateway telemetry broker response timeout" \
      TP_STAGE4_GATEWAY_TELEMETRY_RESPONSE_TIMEOUT_MS \
      HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS \
      "${stage4_default_gateway_telemetry_response_timeout_ms}" \
      "${require_gateway_declaration}" \
      "${stage4_gateway_queue_wait_limit_ms}" \
      "${stage4_max_gateway_response_timeout_ms}"
  )" || return 1
  maximum_gateway_timeout_ms="${stage4_gateway_control_response_timeout_ms}"
  if (( stage4_gateway_telemetry_response_timeout_ms > maximum_gateway_timeout_ms )); then
    maximum_gateway_timeout_ms="${stage4_gateway_telemetry_response_timeout_ms}"
  fi
  minimum_client_timeout_ms=$((
    stage4_gateway_queue_wait_limit_ms
    + maximum_gateway_timeout_ms
    + stage4_rest_response_delivery_grace_ms
  ))
  stage4_rest_client_response_timeout_ms="$(
    stage4_resolve_timeout_override \
      "REST client response timeout" \
      TP_STAGE4_REST_CLIENT_TIMEOUT_MS \
      COHSH_REST_RESPONSE_TIMEOUT_MS \
      "${minimum_client_timeout_ms}" \
      0 \
      "${minimum_client_timeout_ms}" \
      "${stage4_max_rest_client_response_timeout_ms}"
  )" || return 1

  export \
    HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS="${stage4_gateway_control_response_timeout_ms}" \
    HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS="${stage4_gateway_telemetry_response_timeout_ms}" \
    COHSH_REST_RESPONSE_TIMEOUT_MS="${stage4_rest_client_response_timeout_ms}"
}

stage4_process_tree() {
  local pid="$1"
  local child
  while read -r child; do
    [[ -n "${child}" ]] || continue
    stage4_process_tree "${child}"
  done < <(pgrep -P "${pid}" 2>/dev/null || true)
  printf '%s\n' "${pid}"
}

stage4_signal_processes() {
  local processes="$1"
  local signal="$2"
  local process
  for process in ${processes}; do
    kill "-${signal}" "${process}" >/dev/null 2>&1 || true
  done
}

stage4_wait_processes() {
  local processes="$1"
  local attempts="$2"
  local process
  local running
  local attempt
  for ((attempt = 0; attempt < attempts; attempt += 1)); do
    running=0
    for process in ${processes}; do
      if kill -0 "${process}" >/dev/null 2>&1; then
        running=1
        break
      fi
    done
    if [[ "${running}" == "0" ]]; then
      return 0
    fi
    sleep 0.1
  done
  return 1
}

stage4_stop_process_tree() {
  local pid="$1"
  local label="$2"
  if (( pid <= 0 )) || ! kill -0 "${pid}" >/dev/null 2>&1; then
    return 0
  fi

  local processes
  processes="$(stage4_process_tree "${pid}")"
  stage4_signal_processes "${processes}" TERM
  if ! stage4_wait_processes "${processes}" 50; then
    tp_log "WARN  ${label} did not exit after TERM; sending KILL"
    stage4_signal_processes "${processes}" KILL
    if ! stage4_wait_processes "${processes}" 20; then
      tp_log "FAIL  ${label} remained alive after bounded TERM/KILL cleanup"
      return 1
    fi
  fi

  # The root is our direct child. It is no longer live, so this reap cannot
  # block even when descendants were already reparented.
  wait "${pid}" >/dev/null 2>&1 || true
}

stage4_cleanup() {
  local status=$?
  local cleanup_status=0
  trap - EXIT
  set +e
  stage4_stop_local_services
  cleanup_status=$?
  if [[ "${status}" -eq 0 && "${cleanup_status}" -ne 0 ]]; then
    status="${cleanup_status}"
  fi
  stage4_restore_context_environment
  tp_stage_exit_trap "${status}" || true
  exit "${status}"
}
trap stage4_cleanup EXIT

stage4_stop_local_services() {
  local cleanup_status=0
  if ! stage4_stop_process_tree "${gateway_pid}" "local hive-gateway"; then
    cleanup_status=1
  fi
  gateway_pid=0
  if ! stage4_stop_process_tree "${qemu_pid}" "local QEMU"; then
    cleanup_status=1
  fi
  qemu_pid=0
  return "${cleanup_status}"
}

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

stage4_find_free_udp_port() {
  local host="$1"
  "${python_bin}" - "${host}" <<'PY'
import socket
import sys

host = sys.argv[1]
with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
    sock.bind((host, 0))
    print(sock.getsockname()[1])
PY
}

stage4_allocate_distinct_tcp_port() {
  local host="$1"
  shift
  local attempt
  local candidate
  local excluded
  local conflict
  for ((attempt = 0; attempt < 64; attempt += 1)); do
    candidate="$(stage4_find_free_port "${host}")"
    conflict=0
    for excluded in "$@"; do
      if [[ "${candidate}" == "${excluded}" ]]; then
        conflict=1
        break
      fi
    done
    if [[ "${conflict}" == "0" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done
  return 1
}

stage4_allocate_distinct_udp_port() {
  local host="$1"
  shift
  local attempt
  local candidate
  local excluded
  local conflict
  for ((attempt = 0; attempt < 64; attempt += 1)); do
    candidate="$(stage4_find_free_udp_port "${host}")"
    conflict=0
    for excluded in "$@"; do
      if [[ "${candidate}" == "${excluded}" ]]; then
        conflict=1
        break
      fi
    done
    if [[ "${conflict}" == "0" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done
  return 1
}

stage4_validate_port() {
  local label="$1"
  local port="$2"
  if [[ ! "${port}" =~ ^[0-9]+$ ]] \
    || [[ "${#port}" -gt 5 ]] \
    || (( 10#${port} < 1 || 10#${port} > 65535 )); then
    tp_log "FAIL  invalid ${label} port: ${port}"
    exit 1
  fi
}

stage4_parse_bind() {
  local bind="$1"
  local host="${bind%:*}"
  local port="${bind##*:}"
  if [[ -z "${host}" || "${host}" == "${bind}" ]]; then
    tp_log "FAIL  invalid Stage 04 gateway bind: ${bind}"
    exit 1
  fi
  stage4_validate_port "Stage 04 gateway bind" "${port}"
  port="$((10#${port}))"
  printf '%s\n%s\n' "${host}" "${port}"
}

stage4_gateway_origin() {
  local value="$1"
  "${python_bin}" - "${value}" <<'PY'
import sys
from urllib.parse import urlsplit

parsed = urlsplit(sys.argv[1])
if (
    parsed.scheme.lower() not in {"http", "https"}
    or not parsed.hostname
    or parsed.username
    or parsed.password
    or parsed.query
    or parsed.fragment
):
    raise SystemExit("invalid credential-free Stage 04 gateway URL")
try:
    port = parsed.port or (443 if parsed.scheme.lower() == "https" else 80)
except ValueError:
    raise SystemExit("invalid Stage 04 gateway URL port")
host = parsed.hostname
if ":" in host:
    host = f"[{host}]"
print(f"{parsed.scheme.lower()}://{host}:{port}")
PY
}

stage4_wait_log_marker() {
  local path="$1"
  local marker="$2"
  local timeout="$3"
  local pid="$4"
  local deadline=$((SECONDS + timeout))
  while (( SECONDS < deadline )); do
    if ! kill -0 "${pid}" >/dev/null 2>&1; then
      return 1
    fi
    if [[ -f "${path}" ]] && grep -Fq -- "${marker}" "${path}"; then
      return 0
    fi
    sleep 0.2
  done
  return 2
}

stage4_wait_gateway_ready() {
  local url="$1"
  local token="$2"
  local bin="$3"
  local timeout="$4"
  local ready_script="${stage4_runtime_root}/gateway-ready.coh"
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
      COHSH_REST_RESPONSE_TIMEOUT_MS="${stage4_rest_client_response_timeout_ms}" \
      "${bin}" --transport rest --role queen --script "${ready_script}" \
      >>"${TP_LOG_FILE}" 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 2
}

gateway_url="${COHESIX_GATEWAY_URL:-${HIVE_GATEWAY_URL:-${COHSH_REST_URL:-${COH_REST_URL:-}}}}"
gateway_auth_token="${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:-${COHSH_REST_AUTH_TOKEN:-${COH_REST_AUTH_TOKEN:-}}}"
external_gateway=0
if [[ -n "${gateway_url}" ]]; then
  external_gateway=1
  export COHESIX_GATEWAY_URL="${gateway_url}"
fi
stage4_resolve_timeout_contract "${external_gateway}"
tp_log "INFO  gateway-timeout-declaration=${stage4_gateway_timeout_declaration}"
tp_log "INFO  gateway-broker-queue-wait-limit-ms=${stage4_gateway_queue_wait_limit_ms}"
tp_log "INFO  gateway-broker-control-response-timeout-ms=${stage4_gateway_control_response_timeout_ms}"
tp_log "INFO  gateway-broker-telemetry-response-timeout-ms=${stage4_gateway_telemetry_response_timeout_ms}"
tp_log "INFO  rest-response-delivery-grace-ms=${stage4_rest_response_delivery_grace_ms}"
tp_log "INFO  cohsh-rest-response-timeout-ms=${stage4_rest_client_response_timeout_ms}"
if [[ "${target}" == "pi4" ]]; then
  if [[ -z "${gateway_url}" ]]; then
    tp_log "FAIL  Pi 4 Stage 04 requires an existing REST gateway"
    tp_log "FAIL  local QEMU fallback cannot produce Pi 4 evidence"
    exit 1
  fi
  stage3_target_evidence="${stage3_root}/target-evidence.json"
  if [[ -z "${stage3_root}" || ! -s "${stage3_target_evidence}" ]]; then
    tp_log "FAIL  Pi 4 Stage 04 requires immutable Stage 03 target evidence"
    tp_log "FAIL  missing=${stage3_target_evidence}"
    exit 1
  fi
  if [[ -n "${target_evidence_input}" ]] \
    && ! cmp -s "${target_evidence_input}" "${stage3_target_evidence}"; then
    tp_log "FAIL  supplied Pi 4 target evidence differs from Stage 03"
    tp_log "FAIL  Stage 04 must continue against the exact Stage 03 boot"
    exit 1
  fi
  target_evidence="${stage4_root}/target-evidence.json"
  tp_run_cmd \
    "copy-pi4-stage4-target-evidence" \
    "${artifact_helper}" \
    copy-evidence \
    --source "${stage3_target_evidence}" \
    --output "${target_evidence}"
  tp_run_cmd \
    "verify-pi4-stage3-stage4-continuity" \
    "${artifact_helper}" \
    verify-pi4-continuity \
    --target-evidence "${target_evidence}" \
    --prior-result "${stage3_root}/transport-results/base.json" \
    --source-digest "${source_digest}" \
    --prior-evidence-root "${stage3_root}" \
    --prior-action-id "${prior_action_id}" \
    --prior-catalog-action-digest "${prior_catalog_digest}"
  cohsh_bin="${COHSH_BIN:-${TEST_PLAN_ROOT}/target/debug/cohsh}"
  coh_bin="${TP_COH_BIN:-${TEST_PLAN_ROOT}/target/debug/coh}"
else
  if [[ ! -s "${stage3_artifact}" ]]; then
    if [[ "${TEST_PLAN_STAGED_RUN:-0}" == "1" ]]; then
      tp_log "FAIL  staged Stage 04 requires the validated Stage 03 base artifact"
      tp_log "FAIL  missing=${stage3_artifact}"
      exit 1
    fi
    fallback_log_root="${stage4_root}/standalone-build"
    fallback_artifact_root="${stage4_root}/standalone-qemu-artifacts"
    tp_log "INFO  Stage 03 artifact absent; preparing one standalone canonical base artifact"
    tp_run_cmd \
      "stage4-standalone-qemu-artifact" \
      env \
      COHSH_BATCH_TARGET=qemu \
      COHSH_BATCH_GROUPS=base \
      COHSH_BATCH_PREPARE_ONLY=1 \
      COHSH_LOG_ROOT="${fallback_log_root}" \
      COHSH_QEMU_ARTIFACT_ROOT="${fallback_artifact_root}" \
      COHSH_TRANSPORT_RESULT_ROOT="${fallback_log_root}/transport-results" \
      TEST_PLAN_ACTION_ID="${prior_action_id}" \
      TEST_PLAN_SOURCE_DIGEST="${source_digest}" \
      TEST_PLAN_ATTEMPT_MANIFEST= \
      "${TEST_PLAN_ROOT}/scripts/cohsh/run_regression_batch.sh"
    stage3_artifact="${fallback_artifact_root}/base/qemu-artifact.json"
  fi
  tp_run_cmd \
    "verify-stage3-base-artifact-for-rest" \
    "${artifact_helper}" \
    verify \
    --artifact-manifest "${stage3_artifact}" \
    --source-digest "${source_digest}" \
    --action-id "${prior_action_id}" \
    --catalog-action-digest "${prior_catalog_digest}"
  stage4_artifact_manifest="${stage3_artifact}"
  stage3_artifact_dir="$(dirname "${stage3_artifact}")"
  cohsh_bin="${COHSH_BIN:-${stage3_artifact_dir}/host-tools/cohsh}"
  coh_bin="${TP_COH_BIN:-${stage3_artifact_dir}/host-tools/coh}"
  if [[ -n "${gateway_url}" && -z "${target_evidence_input}" ]]; then
    tp_log "FAIL  external QEMU gateway requires machine-generated target evidence"
    tp_log "FAIL  set TEST_PLAN_TARGET_EVIDENCE_FILE with boot and Stage 03 artifact binding"
    exit 1
  fi
  if [[ -n "${gateway_url}" ]]; then
    target_evidence="${stage4_root}/target-evidence.json"
    tp_run_cmd \
      "copy-qemu-stage4-target-evidence" \
      "${artifact_helper}" \
      copy-evidence \
      --source "${target_evidence_input}" \
      --output "${target_evidence}"
    tp_run_cmd \
      "verify-external-qemu-target" \
      "${artifact_helper}" \
      verify-qemu-target \
      --target-evidence "${target_evidence}" \
      --artifact-manifest "${stage4_artifact_manifest}" \
      --source-digest "${source_digest}" \
      --artifact-action-id "${prior_action_id}" \
      --artifact-catalog-action-digest "${prior_catalog_digest}"
  fi
fi
if [[ -z "${gateway_url}" ]]; then
  qemu_tcp_port_explicit=0
  if [[ -n "${TP_STAGE4_QEMU_TCP_PORT:-}" ]]; then
    qemu_tcp_port="${TP_STAGE4_QEMU_TCP_PORT}"
    qemu_tcp_port_explicit=1
  else
    qemu_tcp_port="$(stage4_allocate_distinct_tcp_port 127.0.0.1)"
  fi
  stage4_validate_port "local QEMU TCP" "${qemu_tcp_port}"
  qemu_tcp_port="$((10#${qemu_tcp_port}))"
  gateway_bind_explicit=0
  if [[ -n "${TP_STAGE4_GATEWAY_BIND:-}" ]]; then
    gateway_bind="${TP_STAGE4_GATEWAY_BIND}"
    gateway_bind_explicit=1
  else
    gateway_bind="127.0.0.1:$(
      stage4_allocate_distinct_tcp_port 127.0.0.1 "${qemu_tcp_port}"
    )"
  fi
  gateway_bind_parsed="$(stage4_parse_bind "${gateway_bind}")"
  gateway_bind_host="${gateway_bind_parsed%%$'\n'*}"
  gateway_bind_port="${gateway_bind_parsed##*$'\n'}"
  gateway_bind="${gateway_bind_host}:${gateway_bind_port}"
  if [[ "${qemu_tcp_port}" == "${gateway_bind_port}" ]]; then
    if [[ "${qemu_tcp_port_explicit}" == "1" \
      && "${gateway_bind_explicit}" == "1" ]]; then
      tp_log "FAIL  local QEMU and hive-gateway ports must be distinct"
      tp_log "FAIL  conflicting explicit port=${qemu_tcp_port}"
      exit 1
    fi
    if [[ "${qemu_tcp_port_explicit}" == "1" ]]; then
      gateway_bind_port="$(
        stage4_allocate_distinct_tcp_port \
          "${gateway_bind_host}" \
          "${qemu_tcp_port}"
      )"
      gateway_bind="${gateway_bind_host}:${gateway_bind_port}"
    else
      qemu_tcp_port="$(
        stage4_allocate_distinct_tcp_port \
          127.0.0.1 \
          "${gateway_bind_port}"
      )"
    fi
  fi
  if [[ "${qemu_tcp_port}" == "${gateway_bind_port}" ]]; then
    tp_log "FAIL  unable to allocate distinct local QEMU and hive-gateway ports"
    exit 1
  fi
  gateway_url="http://${gateway_bind}"
  export COHESIX_GATEWAY_URL="${gateway_url}"
  gateway_auth_token="${TP_STAGE4_GATEWAY_AUTH_TOKEN:-test-plan-stage4-rest-token}"
  console_auth_token="${COHSH_AUTH_TOKEN:-${COH_AUTH_TOKEN:-$(stage4_resolve_manifest_auth_token "${TEST_PLAN_ROOT}/configs/root_task.toml")}}"
  artifact_dir="$(dirname "${stage4_artifact_manifest}")"
  cohsh_bin="${COHSH_BIN:-${artifact_dir}/host-tools/cohsh}"
  coh_bin="${TP_COH_BIN:-${artifact_dir}/host-tools/coh}"
  qemu_log="${stage4_runtime_root}/local-qemu.log"
  gateway_log="${stage4_runtime_root}/hive-gateway.log"
  qemu_smoke_port="$(
    stage4_allocate_distinct_tcp_port \
      127.0.0.1 \
      "${qemu_tcp_port}" \
      "${gateway_bind_port}"
  )"
  qemu_udp_port="$(
    stage4_allocate_distinct_udp_port \
      127.0.0.1 \
      "${qemu_tcp_port}" \
      "${gateway_bind_port}" \
      "${qemu_smoke_port}"
  )"

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
  tp_log "INFO  local-qemu-artifact=${stage4_artifact_manifest}"

  stage4_boot_id="${TEST_PLAN_ATTEMPT_ID}-rest-qemu-${RANDOM}"
  "${artifact_helper}" launch \
    --artifact-manifest "${stage4_artifact_manifest}" \
    --source-digest "${source_digest}" \
    --action-id "${prior_action_id}" \
    --catalog-action-digest "${prior_catalog_digest}" \
    --console-port "${qemu_tcp_port}" \
    --udp-port "${qemu_udp_port}" \
    --smoke-port "${qemu_smoke_port}" \
    >"${qemu_log}" 2>&1 &
  qemu_pid=$!

  ready_timeout="${TP_STAGE4_READY_TIMEOUT:-900}"
  if ! stage4_wait_log_marker \
    "${qemu_log}" \
    "[mark] root-console.start.ok" \
    "${ready_timeout}" \
    "${qemu_pid}"; then
    tp_log "FAIL  local QEMU root-console ready marker not observed"
    tail -n 80 "${qemu_log}" >&2 || true
    exit 1
  fi

  if [[ ! -x "${artifact_dir}/host-tools/hive-gateway" ]]; then
    tp_log "FAIL  hive-gateway binary missing from Stage 03 artifact: ${artifact_dir}/host-tools/hive-gateway"
    exit 1
  fi

  COHSH_AUTH_TOKEN="${console_auth_token}" \
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${gateway_auth_token}" \
  "${artifact_dir}/host-tools/hive-gateway" \
    --bind "${gateway_bind}" \
    --tcp-host 127.0.0.1 \
    --tcp-port "${qemu_tcp_port}" \
    --broker-control-response-timeout-ms "${stage4_gateway_control_response_timeout_ms}" \
    --broker-telemetry-response-timeout-ms "${stage4_gateway_telemetry_response_timeout_ms}" \
    >"${gateway_log}" 2>&1 &
  gateway_pid=$!

  gateway_ready_timeout="${TP_STAGE4_GATEWAY_READY_TIMEOUT:-120}"
  if ! stage4_wait_gateway_ready "${gateway_url}" "${gateway_auth_token}" "${cohsh_bin}" "${gateway_ready_timeout}"; then
    tp_log "FAIL  local hive-gateway did not become REST-ready"
    tail -n 80 "${gateway_log}" >&2 || true
    exit 1
  fi
fi
gateway_origin="$(stage4_gateway_origin "${gateway_url}")"
tp_log "INFO  gateway-origin=${gateway_origin}"
if [[ -z "${gateway_auth_token}" ]]; then
  tp_log "FAIL  missing gateway request auth token"
  tp_log "set HIVE_GATEWAY_REQUEST_AUTH_TOKEN (or COHSH_REST_AUTH_TOKEN/COH_REST_AUTH_TOKEN) before running stage 04"
  exit 1
fi
tp_log "INFO  gateway-auth-token=present"
export \
  COHESIX_GATEWAY_URL="${gateway_url}" \
  HIVE_GATEWAY_REQUEST_AUTH_TOKEN="${gateway_auth_token}" \
  COHSH_REST_URL="${gateway_url}" \
  COH_REST_URL="${gateway_url}" \
  HIVE_GATEWAY_URL="${gateway_url}"

# Keep Stage 04 on scripts that are parity-safe over the REST file projection.
# `busy_backpressure.coh` and `policy_gate.coh` depend on console-parser semantics
# and remain covered in the TCP regression matrix (Stage 03).
core_scripts="boot_v0.coh observe_watch.coh session_pool.coh"
parity_scripts="rest_control_plane_smoke.coh"
core_parallelism=3
if ((TP_HOST_JOBS < core_parallelism)); then
  core_parallelism="${TP_HOST_JOBS}"
fi

tp_run_cmd \
  "cohsh-rest-regression-core" \
  env \
  COHSH_BIN="${cohsh_bin}" \
  COHSH_LOG_ROOT="${rest_log_root}" \
  COHSH_BATCH_NAME="rest-regression-core" \
  COHSH_PARALLELISM="${core_parallelism}" \
  COHSH_REST_RESPONSE_TIMEOUT_MS="${stage4_rest_client_response_timeout_ms}" \
  COHSH_SCRIPT_LIST="${core_scripts}" \
  "${TEST_PLAN_ROOT}/scripts/cohsh/REST_regression_batch.sh"

tp_run_cmd \
  "cohsh-rest-regression-parity" \
  env \
  COHSH_BIN="${cohsh_bin}" \
  COHSH_LOG_ROOT="${rest_log_root}" \
  COHSH_BATCH_NAME="rest-regression-parity" \
  COHSH_PARALLELISM=1 \
  COHSH_REST_RESPONSE_TIMEOUT_MS="${stage4_rest_client_response_timeout_ms}" \
  COHSH_SCRIPT_LIST="${parity_scripts}" \
  "${TEST_PLAN_ROOT}/scripts/cohsh/REST_regression_batch.sh"

if [[ "${TP_SKIP_PYTHON:-0}" == "1" ]]; then
  tp_mark_incomplete \
    "python-rest-smoke" \
    "TP_SKIP_PYTHON=1" \
    "REST smoke via cohesix-py was not executed; Python REST parity is unproven."
else
  tp_run_shell "python-rest-smoke" \
    "\"${python_bin}\" - <<'PY'
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
    timeout_s=float(os.environ[\"COHSH_REST_RESPONSE_TIMEOUT_MS\"]) / 1000.0,
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
    export \
      TP_STAGE4_FUSE_COH_BIN="${coh_bin}" \
      TP_STAGE4_FUSE_MOUNT_DIR="${stage4_runtime_root}/coh-mount-rest" \
      TP_STAGE4_FUSE_MOUNT_LOG="${stage4_runtime_root}/coh-rest-mount.log"
    fuse_regression_command="$(
      cat <<'BASH'
set -euo pipefail

mount_dir="${TP_STAGE4_FUSE_MOUNT_DIR}"
mount_log="${TP_STAGE4_FUSE_MOUNT_LOG}"
coh_bin="${TP_STAGE4_FUSE_COH_BIN}"
coh_mount_pid=0
mkdir -p "$(dirname "${mount_log}")" "${mount_dir}"

mount_is_active() {
  grep -F " ${mount_dir} " /proc/mounts >/dev/null 2>&1
}

wait_for_exit() {
  local pid="$1"
  local attempts="$2"
  local attempt
  for ((attempt = 0; attempt < attempts; attempt += 1)); do
    if ! kill -0 "${pid}" >/dev/null 2>&1; then
      wait "${pid}" >/dev/null 2>&1 || true
      return 0
    fi
    sleep 0.1
  done
  return 1
}

bounded_unmount() {
  if ! mount_is_active; then
    return 0
  fi

  fusermount3 -u "${mount_dir}" >/dev/null 2>&1 &
  local unmount_pid=$!
  if ! wait_for_exit "${unmount_pid}" 50; then
    kill -TERM "${unmount_pid}" >/dev/null 2>&1 || true
    if ! wait_for_exit "${unmount_pid}" 10; then
      kill -KILL "${unmount_pid}" >/dev/null 2>&1 || true
      wait_for_exit "${unmount_pid}" 10 || true
    fi
  fi
  if mount_is_active; then
    echo "bounded FUSE unmount failed: ${mount_dir}" >&2
    return 1
  fi
}

stop_mount_process() {
  if (( coh_mount_pid <= 0 )) \
    || ! kill -0 "${coh_mount_pid}" >/dev/null 2>&1; then
    return 0
  fi

  kill -TERM "${coh_mount_pid}" >/dev/null 2>&1 || true
  if ! wait_for_exit "${coh_mount_pid}" 50; then
    kill -KILL "${coh_mount_pid}" >/dev/null 2>&1 || true
    if ! wait_for_exit "${coh_mount_pid}" 20; then
      echo "coh mount remained alive after bounded TERM/KILL cleanup" >&2
      return 1
    fi
  fi
}

cleanup() {
  local status=$?
  local cleanup_status=0
  trap - EXIT
  set +e
  bounded_unmount || cleanup_status=1
  stop_mount_process || cleanup_status=1
  bounded_unmount || cleanup_status=1
  if [[ "${status}" -eq 0 && "${cleanup_status}" -ne 0 ]]; then
    status="${cleanup_status}"
  fi
  exit "${status}"
}
trap cleanup EXIT

if mount_is_active; then
  bounded_unmount
fi

"${coh_bin}" mount --at "${mount_dir}" >"${mount_log}" 2>&1 &
coh_mount_pid=$!

for ((i = 0; i < 50; i += 1)); do
  if mount_is_active; then
    break
  fi
  if ! kill -0 "${coh_mount_pid}" >/dev/null 2>&1; then
    echo "coh mount exited early"
    tail -n 80 "${mount_log}" || true
    exit 1
  fi
  sleep 0.2
done
if ! mount_is_active; then
  echo "coh mount did not become active"
  tail -n 80 "${mount_log}" || true
  exit 1
fi

state_path="${mount_dir}/proc/lifecycle/state"
if [[ ! -f "${state_path}" ]]; then
  echo "missing mount file: ${state_path}"
  exit 1
fi
state_bytes="$(wc -c <"${state_path}" | tr -d ' ')"
if [[ "${state_bytes}" -lt 4 ]]; then
  echo "expected non-empty lifecycle state; got bytes=${state_bytes}"
  exit 1
fi

log_path="${mount_dir}/log/queen.log"
if [[ ! -f "${log_path}" ]]; then
  echo "missing mount file: ${log_path}"
  exit 1
fi
log_bytes="$(wc -c <"${log_path}" | tr -d ' ')"
if [[ "${log_bytes}" -lt 1 ]]; then
  echo "expected non-empty queen log; got bytes=${log_bytes}"
  exit 1
fi

dev_id="tp-mount-$(date +%s)"
echo '{"new":"segment","mime":"text/plain"}' \
  >>"${mount_dir}/queen/telemetry/${dev_id}/ctl"
echo "hello-from-mount ts_ms=$(date +%s000)" \
  >>"${mount_dir}/queen/telemetry/${dev_id}/seg/seg-000001"

latest_path="${mount_dir}/queen/telemetry/${dev_id}/latest"
if [[ ! -f "${latest_path}" ]]; then
  echo "missing latest pointer: ${latest_path}"
  exit 1
fi
latest="$(tr -d '\r\n' <"${latest_path}")"
if [[ "${latest}" != "seg-000001" ]]; then
  echo "expected latest=seg-000001; got latest=${latest}"
  exit 1
fi
if ! grep -q "hello-from-mount" \
  "${mount_dir}/queen/telemetry/${dev_id}/seg/seg-000001"; then
  echo "expected appended telemetry record missing"
  exit 1
fi
BASH
    )"
    tp_run_shell "coh-rest-mount-regression" "${fuse_regression_command}"
  fi
fi

stage4_stop_local_services
mkdir -p "${stage4_result_dir}"
stage4_summary="${stage4_result_dir}/summary.log"
{
  printf 'schema=cohesix.test-plan-rest-summary/v1\n'
  printf 'status=pass\n'
  printf 'target=%s\n' "${target}"
  printf 'claim_tier=%s\n' "${claim_tier}"
  printf 'action_id=%s\n' "${action_id}"
  printf 'source_digest=%s\n' "${source_digest}"
  printf 'gateway_timeout_declaration=%s\n' "${stage4_gateway_timeout_declaration}"
  printf 'gateway_broker_queue_wait_limit_ms=%s\n' "${stage4_gateway_queue_wait_limit_ms}"
  printf 'gateway_broker_control_response_timeout_ms=%s\n' "${stage4_gateway_control_response_timeout_ms}"
  printf 'gateway_broker_telemetry_response_timeout_ms=%s\n' "${stage4_gateway_telemetry_response_timeout_ms}"
  printf 'rest_response_delivery_grace_ms=%s\n' "${stage4_rest_response_delivery_grace_ms}"
  printf 'cohsh_rest_response_timeout_ms=%s\n' "${stage4_rest_client_response_timeout_ms}"
  printf 'gateway_kind=%s\n' "$(
    if [[ "${external_gateway}" == "1" ]]; then
      printf 'external'
    else
      printf 'local-qemu'
    fi
  )"
} >"${stage4_summary}"

declare -a result_arguments=(
  result
  --output "${stage4_result}"
  --action-id "${action_id}"
  --catalog-action-digest "${catalog_digest}"
  --claim-tier "${claim_tier}"
  --target "${target}"
  --source-digest "${source_digest}"
  --evidence-root "${stage4_root}"
  --group rest-multiplexer
  --status pass
  --log "${stage4_summary}"
)
for script in ${core_scripts} ${parity_scripts}; do
  result_arguments+=(--script "${script}")
  script_stem="${script%.coh}"
  declare -a matching_logs=()
  while IFS= read -r -d '' result_log; do
    matching_logs+=("${result_log}")
  done < <(
    find "${rest_log_root}" \
      -type f \
      -name "${script_stem}.run*.log" \
      -print0
  )
  if [[ "${#matching_logs[@]}" -ne 1 ]]; then
    tp_log "FAIL  expected one REST result log for ${script}, found ${#matching_logs[@]}"
    exit 1
  fi
  result_arguments+=(--log "${matching_logs[0]}")
done

if [[ "${target}" == "qemu" ]]; then
  result_arguments+=(
    --artifact-manifest "${stage4_artifact_manifest}"
    --artifact-action-id "${prior_action_id}"
    --artifact-catalog-action-digest "${prior_catalog_digest}"
  )
  if [[ "${external_gateway}" == "1" ]]; then
    result_arguments+=(--target-evidence "${target_evidence}")
  else
    result_arguments+=(--boot-id "${stage4_boot_id}")
    result_arguments+=(--log "${qemu_log}" --log "${gateway_log}")
  fi
else
  result_arguments+=(--target-evidence "${target_evidence}")
fi

tp_run_cmd \
  "write-${action_id}-result" \
  "${artifact_helper}" \
  "${result_arguments[@]}"
tp_run_cmd \
  "verify-${action_id}-result" \
  "${artifact_helper}" \
  verify-result \
  --result "${stage4_result}" \
  --action-id "${action_id}" \
  --catalog-action-digest "${catalog_digest}" \
  --claim-tier "${claim_tier}" \
  --target "${target}" \
  --source-digest "${source_digest}" \
  --evidence-root "${stage4_root}"

if [[ "${TEST_PLAN_ITERATION:-0}" != "1" ]]; then
  "${artifact_helper}" \
    publish-root \
    --state-dir "${TEST_PLAN_STATE_DIR}" \
    --root "${stage4_root}" \
    --pointer "${TEST_PLAN_STATE_DIR}/stage_04_artifact_root.path"
fi
stage4_restore_context_environment
tp_stage_complete 4
