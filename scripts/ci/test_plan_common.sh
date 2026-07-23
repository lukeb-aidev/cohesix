#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Provide provenance-bound helpers for staged Cohesix test-plan execution.
# Copyright 2026 Lukas Bower

set -euo pipefail

tp_common_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_resources.sh
source "${tp_common_dir}/test_plan_resources.sh"

tp_incomplete_dir() {
  printf "%s/incomplete" "${TEST_PLAN_STATE_DIR}"
}

tp_state_lock_dir() {
  printf "%s/.test-plan.lock" "${TEST_PLAN_STATE_DIR}"
}

tp_release_state_lock() {
  local exit_code="${1:-0}"
  if [[ "${TP_STATE_LOCK_OWNED:-0}" == "1" ]]; then
    local lock_dir
    lock_dir="$(tp_state_lock_dir)"
    local recorded_owner=""
    if [[ -f "${lock_dir}/owner" ]]; then
      recorded_owner="$(sed -n '1p' "${lock_dir}/owner")"
    fi
    if [[ -n "${TEST_PLAN_LOCK_OWNER_ID:-}" &&
          "${recorded_owner}" == "${TEST_PLAN_LOCK_OWNER_ID}" ]]; then
      rm -f "${lock_dir}/owner"
      rmdir "${lock_dir}" 2>/dev/null || true
    fi
    TP_STATE_LOCK_OWNED=0
  fi
  return "${exit_code}"
}

tp_acquire_state_lock() {
  local lock_dir
  lock_dir="$(tp_state_lock_dir)"
  if [[ "${TP_STATE_LOCK_OWNED:-0}" == "1" &&
        -n "${TEST_PLAN_LOCK_OWNER_ID:-}" &&
        -f "${lock_dir}/owner" &&
        "$(sed -n '1p' "${lock_dir}/owner")" == "${TEST_PLAN_LOCK_OWNER_ID}" ]]; then
    return 0
  fi
  TP_STATE_LOCK_OWNED=0

  if [[ "${TEST_PLAN_RUNNER_LOCK_HELD:-0}" == "1" ]]; then
    if [[ -z "${TEST_PLAN_LOCK_OWNER_ID:-}" ||
          ! -f "${lock_dir}/owner" ||
          "$(sed -n '1p' "${lock_dir}/owner")" != "${TEST_PLAN_LOCK_OWNER_ID}" ]]; then
      printf "runner lock ownership is missing or mismatched: %s\n" \
        "${lock_dir}" >&2
      return 1
    fi
    return 0
  fi

  TEST_PLAN_LOCK_OWNER_ID="${TEST_PLAN_LOCK_OWNER_ID:-$(tp_new_attempt_id)}"
  if ! mkdir "${lock_dir}" 2>/dev/null; then
    printf "test-plan state directory is already active: %s\n" \
      "${TEST_PLAN_STATE_DIR}" >&2
    if [[ -s "${lock_dir}/owner" ]]; then
      printf "lock owner: %s\n" "$(sed -n '1p' "${lock_dir}/owner")" >&2
    fi
    printf "use a different --state-dir, or inspect and remove a stale lock only after confirming no writer is active\n" >&2
    return 1
  fi
  printf "%s\n" "${TEST_PLAN_LOCK_OWNER_ID}" >"${lock_dir}/owner"
  TP_STATE_LOCK_OWNED=1
  export TEST_PLAN_LOCK_OWNER_ID
  trap 'tp_release_state_lock "$?"' EXIT
}

tp_stage_scope() {
  case "$1" in
    1) printf "common" ;;
    2|3|4|5) printf "target" ;;
    *)
      printf "invalid test-plan stage: %s\n" "$1" >&2
      return 1
      ;;
  esac
}

tp_init() {
  local script_dir
  script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  TEST_PLAN_ROOT="${TEST_PLAN_ROOT:-$(cd "${script_dir}/../.." && pwd)}"
  TEST_PLAN_STATE_DIR="${TEST_PLAN_STATE_DIR:-${TEST_PLAN_ROOT}/out/test-plan/manual}"
  TEST_PLAN_LOG_DIR="${TEST_PLAN_STATE_DIR}/logs"
  TP_EVIDENCE_TOOL="${TEST_PLAN_ROOT}/scripts/ci/test_plan_evidence.py"
  if [[ ! -f "${TP_EVIDENCE_TOOL}" ]]; then
    printf "missing test-plan evidence tool: %s\n" "${TP_EVIDENCE_TOOL}" >&2
    return 1
  fi
  mkdir -p \
    "${TEST_PLAN_LOG_DIR}" \
    "${TEST_PLAN_STATE_DIR}/evidence/attempts" \
    "${TEST_PLAN_STATE_DIR}/evidence/iterations" \
    "${TEST_PLAN_STATE_DIR}/evidence/qualifications"
  tp_configure_resource_limits
  tp_acquire_state_lock
}

tp_evidence() {
  python3 "${TP_EVIDENCE_TOOL}" "$@"
}

tp_stage_tag() {
  printf "stage-%02d" "$1"
}

tp_stage_marker() {
  printf "%s/stage_%02d.done" "${TEST_PLAN_STATE_DIR}" "$1"
}

tp_stage_iteration_marker() {
  printf "%s/stage_%02d.iteration" "${TEST_PLAN_STATE_DIR}" "$1"
}

tp_stage_attestation_ref() {
  printf "%s/stage_%02d.attestation.json" "$1" "$2"
}

tp_stage_target_attestation_ref() {
  printf "%s/stage_%02d.%s.attestation.json" "$1" "$2" "$3"
}

tp_stage_iteration_attestation_ref() {
  printf "%s/stage_%02d.iteration.attestation.json" "$1" "$2"
}

tp_stage_pending_ref() {
  local state_dir="$1"
  local stage="$2"
  local mode="${3:-full}"
  if [[ "${mode}" == "iteration" ]]; then
    printf "%s/stage_%02d.iteration.pending.json" "${state_dir}" "${stage}"
  else
    printf "%s/stage_%02d.pending.json" "${state_dir}" "${stage}"
  fi
}

tp_stage_fingerprint_file() {
  printf "%s/stage_%02d.inputs.sha256" "${TEST_PLAN_STATE_DIR}" "$1"
}

tp_capture_context() {
  tp_evidence \
    capture-context \
    --root "${TEST_PLAN_ROOT}" \
    --state-dir "${TEST_PLAN_STATE_DIR}" \
    --stage "$1" \
    --target "${TEST_PLAN_TARGET:-unknown}" \
    --output "$2"
}

tp_context_field() {
  tp_evidence field --path "$1" --name "$2"
}

tp_sha256_file() {
  tp_evidence sha256 --path "$1"
}

tp_tree_digest() {
  tp_evidence tree-digest --path "$1"
}

tp_resolve_state_pointer() {
  tp_evidence \
    resolve-state-pointer \
    --state-dir "$1" \
    --pointer "$2"
}

tp_now_epoch_ms() {
  tp_evidence now-ms
}

tp_new_attempt_id() {
  tp_evidence new-id
}

tp_atomic_ref_write() {
  tp_evidence \
    write-ref \
    --state-dir "$1" \
    --ref "$2" \
    --manifest "$3"
}

tp_atomic_marker_write() {
  tp_evidence write-marker --path "$1" --manifest "$2"
}

tp_ref_manifest_path() {
  tp_evidence \
    ref-manifest \
    --state-dir "$1" \
    --ref "$2"
}

tp_stage_input_fingerprint() {
  local stage="$1"
  local temporary
  temporary="$(mktemp "${TEST_PLAN_STATE_DIR}/.stage-context.XXXXXX")"
  tp_capture_context "${stage}" "${temporary}"
  tp_context_field "${temporary}" context_digest
  rm -f "${temporary}"
}

tp_write_stage_fingerprint() {
  local stage="$1"
  local fingerprint="${TEST_PLAN_CONTEXT_DIGEST:-}"
  if [[ -z "${fingerprint}" ]]; then
    fingerprint="$(tp_stage_input_fingerprint "${stage}")"
  fi
  tp_evidence \
    write-fingerprint \
    --path "$(tp_stage_fingerprint_file "${stage}")" \
    --stage "${stage}" \
    --target "${TEST_PLAN_TARGET:-unknown}" \
    --digest "${fingerprint}"
}

tp_stage_fingerprint_value() {
  sed -n 's/^fingerprint=//p' "$1" | tail -n 1
}

tp_verify_stage_attestation() {
  local state_dir="$1"
  local stage="$2"
  local target="${3:-}"
  local -a arguments=(
    verify
    --root "${TEST_PLAN_ROOT}"
    --state-dir "${state_dir}"
    --stage "${stage}"
  )
  if [[ -n "${target}" ]]; then
    arguments+=(--target "${target}")
  fi
  tp_evidence "${arguments[@]}" >/dev/null
}

tp_assert_stage_fingerprint_fresh() {
  local stage="$1"
  if ! tp_verify_stage_attestation \
    "${TEST_PLAN_STATE_DIR}" \
    "${stage}" \
    "${TEST_PLAN_TARGET:-}"; then
    printf "stage %02d has no fresh, provenance-bound attestation\n" "${stage}" >&2
    printf "rerun stage %02d in this state dir before reusing its evidence\n" "${stage}" >&2
    return 1
  fi
}

tp_require_stage_done() {
  local stage="$1"
  if ! tp_verify_stage_attestation \
    "${TEST_PLAN_STATE_DIR}" \
    "${stage}" \
    "${TEST_PLAN_TARGET:-}"; then
    printf "missing or stale required stage attestation for stage %02d\n" "${stage}" >&2
    printf "run stage %02d first with TEST_PLAN_STATE_DIR=%s\n" \
      "${stage}" \
      "${TEST_PLAN_STATE_DIR}" >&2
    exit 1
  fi
}

tp_log() {
  printf "[%s] %s\n" "${TP_STAGE_TAG}" "$1" | tee -a "${TP_LOG_FILE}"
}

tp_redact_command() {
  tp_evidence redact -- "$@"
}

tp_redact_stream() {
  tp_evidence redact-stream
}

tp_log_segment_sha256() {
  tp_evidence \
    log-segment-sha \
    --path "$1" \
    --start "$2" \
    --end "$3"
}

tp_write_action_manifest() {
  local path="$1"
  local ordinal="$2"
  local name="$3"
  local command="$4"
  local started_at="$5"
  local finished_at="$6"
  local duration_ms="$7"
  local exit_code="$8"
  local log_start="$9"
  local log_end="${10}"
  local log_segment_sha="${11}"
  local -a arguments=(
    write-action
    --path "${path}"
    --ordinal "${ordinal}"
    --name "${name}"
    --command "${command}"
    --started-at "${started_at}"
    --finished-at "${finished_at}"
    --duration-ms "${duration_ms}"
    --exit-code "${exit_code}"
    --log-start "${log_start}"
    --log-end "${log_end}"
    --log-sha256 "${log_segment_sha}"
  )
  if [[ -n "${TP_CURRENT_CATALOG_ACTION_ID:-}" ]]; then
    arguments+=(
      --catalog-action-id "${TP_CURRENT_CATALOG_ACTION_ID}"
      --catalog-action-digest "${TP_CURRENT_CATALOG_ACTION_DIGEST}"
      --catalog-timeout-seconds "${TP_CURRENT_CATALOG_TIMEOUT_SECONDS}"
      --catalog-test-policy "${TP_CURRENT_CATALOG_TEST_POLICY}"
    )
    if [[ -n "${TP_CURRENT_CATALOG_MINIMUM_TEST_COUNT:-}" ]]; then
      arguments+=(
        --catalog-minimum-test-count "${TP_CURRENT_CATALOG_MINIMUM_TEST_COUNT}"
      )
    fi
  fi
  tp_evidence "${arguments[@]}"
}

tp_run_cmd() {
  local name="$1"
  shift
  TP_ACTION_COUNTER=$((TP_ACTION_COUNTER + 1))
  local ordinal="${TP_ACTION_COUNTER}"
  local action_path
  action_path="$(printf "%s/actions/%04d.json" "${TP_ATTEMPT_DIR}" "${ordinal}")"
  local redacted_command
  redacted_command="$(tp_redact_command "$@")"
  local started_at
  started_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  local started_ms
  started_ms="$(tp_now_epoch_ms)"
  local log_start
  log_start="$(wc -c <"${TP_LOG_FILE}" | tr -d ' ')"

  tp_log "START ${name}"
  tp_log "CMD   ${redacted_command}"
  local exit_code=0
  local -a pipeline_status
  if "$@" 2>&1 | tp_redact_stream >>"${TP_LOG_FILE}"; then
    pipeline_status=("${PIPESTATUS[@]}")
  else
    pipeline_status=("${PIPESTATUS[@]}")
  fi
  exit_code="${pipeline_status[0]}"
  local redactor_exit_code="${pipeline_status[1]}"
  if [[ "${redactor_exit_code}" -ne 0 && "${exit_code}" -eq 0 ]]; then
    exit_code=125
  fi
  if [[ "${exit_code}" -eq 0 ]]; then
    tp_log "PASS  ${name}"
  else
    tp_log "FAIL  ${name} exit=${exit_code}"
  fi

  local finished_at
  finished_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  local finished_ms
  finished_ms="$(tp_now_epoch_ms)"
  local duration_ms=$((finished_ms - started_ms))
  local log_end
  log_end="$(wc -c <"${TP_LOG_FILE}" | tr -d ' ')"
  local log_segment_sha
  log_segment_sha="$(tp_log_segment_sha256 "${TP_LOG_FILE}" "${log_start}" "${log_end}")"
  tp_write_action_manifest \
    "${action_path}" \
    "${ordinal}" \
    "${name}" \
    "${redacted_command}" \
    "${started_at}" \
    "${finished_at}" \
    "${duration_ms}" \
    "${exit_code}" \
    "${log_start}" \
    "${log_end}" \
    "${log_segment_sha}"

  if [[ "${exit_code}" -ne 0 ]]; then
    tail -n 40 "${TP_LOG_FILE}" >&2 || true
    return "${exit_code}"
  fi
}

tp_catalog_field() {
  local action_id="$1"
  local field="$2"
  python3 "${TEST_PLAN_ROOT}/scripts/ci/test_plan_catalog.py" \
    --catalog "${TEST_PLAN_ROOT}/configs/test_plan_actions.toml" \
    action \
    --id "${action_id}" \
    --field "${field}"
}

tp_catalog_execute() {
  local -a arguments=(
    run-catalog-command
    --root "${TEST_PLAN_ROOT}"
    --timeout-seconds "$1"
    --test-policy "$2"
  )
  if [[ -n "$3" ]]; then
    arguments+=(--minimum-test-count "$3")
  fi
  arguments+=(--command "$4")
  tp_evidence "${arguments[@]}"
}

tp_run_catalog_action() {
  local action_id="$1"
  local action_json
  action_json="$(tp_catalog_field "${action_id}" json)"
  local action_stage
  local action_scope
  local action_targets
  local action_manual
  action_stage="$(
    python3 -c 'import json,sys; print(json.load(sys.stdin)["stage"])' \
      <<<"${action_json}"
  )"
  action_scope="$(
    python3 -c 'import json,sys; print(json.load(sys.stdin)["scope"])' \
      <<<"${action_json}"
  )"
  action_targets="$(
    python3 -c \
      'import json,sys; print("\n".join(json.load(sys.stdin)["targets"]))' \
      <<<"${action_json}"
  )"
  action_manual="$(
    python3 -c \
      'import json,sys; print("1" if json.load(sys.stdin).get("manual", False) else "0")' \
      <<<"${action_json}"
  )"
  if [[ "${action_stage}" != "${TP_STAGE}" ]]; then
    tp_log "FAIL  catalog action ${action_id} belongs to stage ${action_stage}, not ${TP_STAGE}"
    return 1
  fi
  if [[ "${action_manual}" == "1" ]]; then
    tp_log "FAIL  manual catalog action ${action_id} cannot run as a staged command"
    return 1
  fi
  if ! grep -Fx "${TEST_PLAN_TARGET:-unknown}" <<<"${action_targets}" >/dev/null; then
    tp_log "FAIL  catalog action ${action_id} does not apply to target ${TEST_PLAN_TARGET:-unknown}"
    return 1
  fi
  case "${action_scope}" in
    common|provisioned-target|target)
      ;;
    *)
      tp_log "FAIL  catalog action ${action_id} has non-executable scope ${action_scope}"
      return 1
      ;;
  esac

  local command
  local timeout_seconds
  local test_policy
  local minimum_test_count
  command="$(
    python3 -c 'import json,sys; print(json.load(sys.stdin)["command"])' \
      <<<"${action_json}"
  )"
  timeout_seconds="$(
    python3 -c \
      'import json,sys; print(json.load(sys.stdin)["timeout_seconds"])' \
      <<<"${action_json}"
  )"
  test_policy="$(
    python3 -c \
      'import json,sys; print(json.load(sys.stdin)["test_policy"])' \
      <<<"${action_json}"
  )"
  minimum_test_count="$(
    python3 -c \
      'import json,sys; print(json.load(sys.stdin).get("minimum_test_count", ""))' \
      <<<"${action_json}"
  )"
  TP_CURRENT_CATALOG_ACTION_ID="${action_id}"
  TP_CURRENT_CATALOG_ACTION_DIGEST="$(tp_catalog_field "${action_id}" digest)"
  TP_CURRENT_CATALOG_TIMEOUT_SECONDS="${timeout_seconds}"
  TP_CURRENT_CATALOG_TEST_POLICY="${test_policy}"
  TP_CURRENT_CATALOG_MINIMUM_TEST_COUNT="${minimum_test_count}"
  local exit_code=0
  if tp_run_cmd \
    "${action_id}" \
    tp_catalog_execute \
    "${timeout_seconds}" \
    "${test_policy}" \
    "${minimum_test_count}" \
    "${command}"; then
    exit_code=0
  else
    exit_code=$?
  fi
  unset \
    TP_CURRENT_CATALOG_ACTION_ID \
    TP_CURRENT_CATALOG_ACTION_DIGEST \
    TP_CURRENT_CATALOG_TIMEOUT_SECONDS \
    TP_CURRENT_CATALOG_TEST_POLICY \
    TP_CURRENT_CATALOG_MINIMUM_TEST_COUNT
  return "${exit_code}"
}

tp_resolve_python_311() {
  local requested="${TP_PYTHON_BIN:-python3}"
  local command_path
  if ! command_path="$(command -v "${requested}")"; then
    tp_log "FAIL  Python interpreter not found: ${requested}"
    return 1
  fi
  if ! TP_PYTHON_RESOLVED="$(
    "${command_path}" -c \
      'import os, sys; print(os.path.realpath(sys.executable))'
  )"; then
    tp_log "FAIL  unable to resolve Python interpreter: ${command_path}"
    return 1
  fi
  if [[ "${TP_PYTHON_RESOLVED}" != /* || ! -x "${TP_PYTHON_RESOLVED}" ]]; then
    tp_log "FAIL  resolved Python interpreter is not an absolute executable: ${TP_PYTHON_RESOLVED}"
    return 1
  fi
  tp_run_cmd \
    "${TP_PYTHON_RESOLVED} version >= 3.11" \
    "${TP_PYTHON_RESOLVED}" \
    -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)'
}

tp_run_shell() {
  local name="$1"
  shift
  local command="$*"
  tp_run_cmd \
    "${name}" \
    env \
    -u BASH_ENV \
    -u ENV \
    bash \
    --noprofile \
    --norc \
    -c \
    "cd \"${TEST_PLAN_ROOT}\" && ${command}"
}

tp_archive_active_incomplete() {
  local attempt_dir="$1"
  local stage_tag="$2"
  local incomplete_dir
  incomplete_dir="$(tp_incomplete_dir)"
  [[ -d "${incomplete_dir}" ]] || return 0
  local destination="${attempt_dir}/superseded-incomplete"
  local path
  while IFS= read -r -d '' path; do
    mkdir -p "${destination}"
    mv "${path}" "${destination}/"
  done < <(
    find "${incomplete_dir}" \
      -maxdepth 1 \
      -type f \
      -name "${stage_tag}-*" \
      -print0
  )
}

tp_invalidate_active_stage() {
  local stage="$1"
  local target="${TEST_PLAN_TARGET:-unknown}"
  rm -f \
    "$(tp_stage_marker "${stage}")" \
    "${TEST_PLAN_STATE_DIR}/stage_$(printf "%02d" "${stage}").${target}.done" \
    "$(tp_stage_attestation_ref "${TEST_PLAN_STATE_DIR}" "${stage}")" \
    "$(tp_stage_target_attestation_ref "${TEST_PLAN_STATE_DIR}" "${stage}" "${target}")" \
    "$(tp_stage_fingerprint_file "${stage}")" \
    "${TEST_PLAN_STATE_DIR}/stage_$(printf "%02d" "${stage}").incomplete" \
    "${TEST_PLAN_STATE_DIR}/stage_$(printf "%02d" "${stage}")_artifact_root.path"
}

tp_finalize_pending_attempt() {
  local state_dir="$1"
  local stage="$2"
  local mode="${3:-full}"
  local status="${4:-abandoned}"
  local pending
  pending="$(tp_stage_pending_ref "${state_dir}" "${stage}" "${mode}")"
  [[ -s "${pending}" ]] || return 0
  tp_evidence \
    finalize-pending \
    --state-dir "${state_dir}" \
    --pending "${pending}" \
    --status "${status}" >/dev/null
}

tp_stage_begin() {
  local stage="$1"
  local name="$2"
  TP_STAGE="${stage}"
  TP_STAGE_NAME="${name}"
  TP_STAGE_SCOPE="$(tp_stage_scope "${stage}")"
  TP_STAGE_TAG="$(tp_stage_tag "${stage}")"
  TP_HAS_INCOMPLETE=0
  TP_ACTION_COUNTER=0
  TP_STAGE_FINALIZED=0
  TP_STAGE_STARTED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  TP_STAGE_STARTED_MS="$(tp_now_epoch_ms)"
  TEST_PLAN_ATTEMPT_ID="$(tp_new_attempt_id)"
  local mode="full"
  local evidence_kind="attempts"
  if [[ "${TEST_PLAN_ITERATION:-0}" == "1" ]]; then
    mode="iteration"
    evidence_kind="iterations"
  fi
  TP_PENDING_REF="$(tp_stage_pending_ref "${TEST_PLAN_STATE_DIR}" "${stage}" "${mode}")"
  if [[ -e "${TP_PENDING_REF}" ]]; then
    if [[ "${TEST_PLAN_FORCE:-0}" != "1" && "${mode}" == "full" ]]; then
      printf "unfinished stage attempt exists: %s\n" "${TP_PENDING_REF}" >&2
      printf "inspect it or rerun with --force to record and supersede it\n" >&2
      return 1
    fi
    tp_finalize_pending_attempt "${TEST_PLAN_STATE_DIR}" "${stage}" "${mode}" abandoned
  fi

  TP_ATTEMPT_DIR="${TEST_PLAN_STATE_DIR}/evidence/${evidence_kind}/${TP_STAGE_TAG}/${TEST_PLAN_ATTEMPT_ID}"
  mkdir -p "${TP_ATTEMPT_DIR}/actions"
  TP_LOG_FILE="${TP_ATTEMPT_DIR}/stage.log"
  TP_CONTEXT_FILE="${TP_ATTEMPT_DIR}/context.json"
  TEST_PLAN_ATTEMPT_MANIFEST="${TP_ATTEMPT_DIR}/attempt.json"
  TP_STAGE_MANIFEST="${TP_ATTEMPT_DIR}/stage.json"
  if [[ "${mode}" == "full" ]]; then
    tp_invalidate_active_stage "${stage}"
    tp_archive_active_incomplete "${TP_ATTEMPT_DIR}" "${TP_STAGE_TAG}"
  else
    rm -f \
      "$(tp_stage_iteration_marker "${stage}")" \
      "$(tp_stage_iteration_attestation_ref "${TEST_PLAN_STATE_DIR}" "${stage}")"
  fi

  : >"${TP_LOG_FILE}"
  tp_capture_context "${stage}" "${TP_CONTEXT_FILE}"
  TEST_PLAN_SOURCE_DIGEST="$(tp_context_field "${TP_CONTEXT_FILE}" source_digest)"
  TEST_PLAN_CONTEXT_DIGEST="$(tp_context_field "${TP_CONTEXT_FILE}" context_digest)"
  export \
    TEST_PLAN_ATTEMPT_ID \
    TEST_PLAN_ATTEMPT_MANIFEST \
    TEST_PLAN_SOURCE_DIGEST \
    TEST_PLAN_CONTEXT_DIGEST
  local -a attempt_arguments=(
    write-attempt
    --path "${TEST_PLAN_ATTEMPT_MANIFEST}"
    --attempt-id "${TEST_PLAN_ATTEMPT_ID}"
    --stage "${stage}"
    --stage-name "${name}"
    --scope "${TP_STAGE_SCOPE}"
    --target "${TEST_PLAN_TARGET:-unknown}"
    --started-at "${TP_STAGE_STARTED_AT}"
    --started-ms "${TP_STAGE_STARTED_MS}"
    --log "stage.log"
    --context "context.json"
  )
  local -a pending_arguments=(
    write-pending
    --state-dir "${TEST_PLAN_STATE_DIR}"
    --path "${TP_PENDING_REF}"
    --attempt-dir "${TP_ATTEMPT_DIR}"
    --stage "${stage}"
    --target "${TEST_PLAN_TARGET:-unknown}"
  )
  if [[ "${mode}" == "iteration" ]]; then
    attempt_arguments+=(--iteration)
    pending_arguments+=(--iteration)
  fi
  tp_evidence "${attempt_arguments[@]}"
  tp_evidence "${pending_arguments[@]}"
  {
    printf "[%s] BEGIN %s\n" "${TP_STAGE_TAG}" "${name}"
    printf "[%s] ROOT  %s\n" "${TP_STAGE_TAG}" "${TEST_PLAN_ROOT}"
    printf "[%s] STATE %s\n" "${TP_STAGE_TAG}" "${TEST_PLAN_STATE_DIR}"
    printf "[%s] LOG   %s\n" "${TP_STAGE_TAG}" "${TP_LOG_FILE}"
    printf "[%s] ATTEMPT %s\n" "${TP_STAGE_TAG}" "${TEST_PLAN_ATTEMPT_ID}"
    printf "[%s] SOURCE %s\n" "${TP_STAGE_TAG}" "${TEST_PLAN_SOURCE_DIGEST}"
    printf "[%s] RESOURCE jobs=%s ui_workers=%s\n" \
      "${TP_STAGE_TAG}" \
      "${TP_HOST_JOBS}" \
      "${TP_UI_WORKERS}"
  } | tee -a "${TP_LOG_FILE}"
  trap 'tp_stage_exit_trap "$?"' EXIT
}

tp_mark_incomplete() {
  local step="$1"
  local reason="$2"
  local impact="$3"
  TP_HAS_INCOMPLETE=1
  local slug
  slug="$(sed -E 's/[^A-Za-z0-9_.-]+/-/g; s/^[.-]+//; s/[.-]+$//' <<<"${step}")"
  slug="${slug:-unnamed}"
  local active_dir
  active_dir="$(tp_incomplete_dir)"
  mkdir -p "${active_dir}" "${TP_ATTEMPT_DIR}/incomplete"
  local active_path="${active_dir}/${TP_STAGE_TAG}-${slug}.md"
  tp_evidence \
    write-incomplete \
    --active "${active_path}" \
    --attempt "${TP_ATTEMPT_DIR}/incomplete/${slug}.md" \
    --timestamp "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
    --stage-tag "${TP_STAGE_TAG}" \
    --stage-name "${TP_STAGE_NAME}" \
    --step "${step}" \
    --reason "${reason}" \
    --impact "${impact}"
  tp_log "INCOMPLETE step=${step} reason=${reason}"
  tp_log "INCOMPLETE file=${active_path}"
}

tp_link_current_log() {
  tp_evidence \
    link-log \
    --link "${TEST_PLAN_LOG_DIR}/$2" \
    --target "$1"
}

tp_stage_exit_trap() {
  local exit_code="$1"
  trap - EXIT
  if [[ "${TP_STAGE_FINALIZED:-0}" != "1" && -n "${TP_ATTEMPT_DIR:-}" ]]; then
    if [[ -f "${TP_LOG_FILE:-}" ]]; then
      printf "[%s] ABORT exit=%s\n" "${TP_STAGE_TAG:-stage}" "${exit_code}" \
        >>"${TP_LOG_FILE}" || true
    fi
    local mode="full"
    [[ "${TEST_PLAN_ITERATION:-0}" == "1" ]] && mode="iteration"
    tp_finalize_pending_attempt \
      "${TEST_PLAN_STATE_DIR}" \
      "${TP_STAGE}" \
      "${mode}" \
      failed || true
  fi
  tp_release_state_lock "${exit_code}"
  return "${exit_code}"
}

tp_stage_complete() {
  local stage="$1"
  local mode="full"
  [[ "${TEST_PLAN_ITERATION:-0}" == "1" ]] && mode="iteration"
  local final_context="${TP_ATTEMPT_DIR}/final-context.json"
  tp_capture_context "${stage}" "${final_context}"
  local final_digest
  final_digest="$(tp_context_field "${final_context}" context_digest)"
  local status="pass"
  if [[ "${final_digest}" != "${TEST_PLAN_CONTEXT_DIGEST}" ]]; then
    status="stale-inputs"
    tp_log "FAIL  inputs changed during stage"
    tp_log "FAIL  initial=${TEST_PLAN_CONTEXT_DIGEST}"
    tp_log "FAIL  final=${final_digest}"
  elif [[ "${TP_HAS_INCOMPLETE:-0}" != "0" ]]; then
    status="incomplete"
    tp_log "FAIL  incomplete evidence; see $(tp_incomplete_dir)/"
  elif [[ "${mode}" == "iteration" ]]; then
    tp_log "ITER  immutable iteration evidence will be recorded"
  else
    tp_log "DONE  immutable PASS evidence will be recorded"
  fi

  tp_evidence \
    finalize-attempt \
    --attempt-dir "${TP_ATTEMPT_DIR}" \
    --status "${status}" \
    --final-context "${final_context}" >/dev/null
  TP_STAGE_FINALIZED=1
  rm -f "${TP_PENDING_REF}"
  if [[ "${status}" != "pass" ]]; then
    if [[ "${status}" == "incomplete" ]]; then
      tp_atomic_marker_write \
        "${TEST_PLAN_STATE_DIR}/stage_$(printf "%02d" "${stage}").incomplete" \
        "${TP_STAGE_MANIFEST}"
    fi
    trap - EXIT
    tp_release_state_lock 1
    return 1
  fi

  if [[ "${mode}" == "iteration" ]]; then
    tp_atomic_ref_write \
      "${TEST_PLAN_STATE_DIR}" \
      "$(tp_stage_iteration_attestation_ref "${TEST_PLAN_STATE_DIR}" "${stage}")" \
      "${TP_STAGE_MANIFEST}"
    tp_atomic_marker_write \
      "$(tp_stage_iteration_marker "${stage}")" \
      "${TP_STAGE_MANIFEST}"
    trap - EXIT
    tp_release_state_lock 0
    return 0
  fi

  tp_atomic_ref_write \
    "${TEST_PLAN_STATE_DIR}" \
    "$(tp_stage_attestation_ref "${TEST_PLAN_STATE_DIR}" "${stage}")" \
    "${TP_STAGE_MANIFEST}"
  tp_write_stage_fingerprint "${stage}"
  tp_atomic_marker_write \
    "$(tp_stage_marker "${stage}")" \
    "${TP_STAGE_MANIFEST}"
  tp_link_current_log \
    "${TP_LOG_FILE}" \
    "${TP_STAGE_TAG}-${TP_STAGE_NAME}.log"
  trap - EXIT
  tp_release_state_lock 0
}

tp_qualify_stage_attestation() {
  local state_dir="$1"
  local stage="$2"
  local target="$3"
  shift 3
  local -a arguments=(
    qualify
    --root "${TEST_PLAN_ROOT}"
    --state-dir "${state_dir}"
    --stage "${stage}"
    --target "${target}"
  )
  local artifact
  for artifact in "$@"; do
    arguments+=(--artifact "${artifact}")
  done
  tp_evidence "${arguments[@]}" >/dev/null
}

tp_import_common_stage_attestation() {
  local source_state="$1"
  local destination_state="$2"
  local stage="$3"
  local target="$4"
  local context_path
  context_path="$(
    tp_evidence \
      import-common \
      --root "${TEST_PLAN_ROOT}" \
      --source-state "${source_state}" \
      --destination-state "${destination_state}" \
      --stage "${stage}" \
      --target "${target}"
  )"
  local saved_state="${TEST_PLAN_STATE_DIR}"
  local saved_target="${TEST_PLAN_TARGET:-}"
  local saved_context_digest="${TEST_PLAN_CONTEXT_DIGEST:-}"
  TEST_PLAN_STATE_DIR="${destination_state}"
  TEST_PLAN_TARGET="${target}"
  TEST_PLAN_CONTEXT_DIGEST="$(tp_context_field "${context_path}" context_digest)"
  tp_write_stage_fingerprint "${stage}"
  TEST_PLAN_STATE_DIR="${saved_state}"
  TEST_PLAN_TARGET="${saved_target}"
  TEST_PLAN_CONTEXT_DIGEST="${saved_context_digest}"
}
