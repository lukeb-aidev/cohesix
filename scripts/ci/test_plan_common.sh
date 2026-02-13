#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Shared helpers for staged Cohesix test-plan execution scripts.
# Copyright 2026 Lukas Bower

set -euo pipefail

tp_init() {
  local script_dir
  script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  TEST_PLAN_ROOT="${TEST_PLAN_ROOT:-$(cd "${script_dir}/../.." && pwd)}"
  TEST_PLAN_STATE_DIR="${TEST_PLAN_STATE_DIR:-${TEST_PLAN_ROOT}/out/test-plan/manual}"
  TEST_PLAN_LOG_DIR="${TEST_PLAN_STATE_DIR}/logs"
  mkdir -p "${TEST_PLAN_LOG_DIR}"
}

tp_stage_tag() {
  local stage="$1"
  printf "stage-%02d" "${stage}"
}

tp_stage_marker() {
  local stage="$1"
  printf "%s/stage_%02d.done" "${TEST_PLAN_STATE_DIR}" "${stage}"
}

tp_stage_begin() {
  local stage="$1"
  local name="$2"
  TP_STAGE="${stage}"
  TP_STAGE_NAME="${name}"
  TP_STAGE_TAG="$(tp_stage_tag "${stage}")"
  TP_LOG_FILE="${TEST_PLAN_LOG_DIR}/${TP_STAGE_TAG}-${name}.log"
  : >"${TP_LOG_FILE}"
  {
    printf "[%s] BEGIN %s\n" "${TP_STAGE_TAG}" "${name}"
    printf "[%s] ROOT  %s\n" "${TP_STAGE_TAG}" "${TEST_PLAN_ROOT}"
    printf "[%s] STATE %s\n" "${TP_STAGE_TAG}" "${TEST_PLAN_STATE_DIR}"
    printf "[%s] LOG   %s\n" "${TP_STAGE_TAG}" "${TP_LOG_FILE}"
  } | tee -a "${TP_LOG_FILE}"
}

tp_require_stage_done() {
  local stage="$1"
  local marker
  marker="$(tp_stage_marker "${stage}")"
  if [[ ! -f "${marker}" ]]; then
    printf "missing required stage marker: %s\n" "${marker}" >&2
    printf "run stage %02d first with TEST_PLAN_STATE_DIR=%s\n" "${stage}" "${TEST_PLAN_STATE_DIR}" >&2
    exit 1
  fi
}

tp_log() {
  local line="$1"
  printf "[%s] %s\n" "${TP_STAGE_TAG}" "${line}" | tee -a "${TP_LOG_FILE}"
}

tp_run_cmd() {
  local name="$1"
  shift
  tp_log "START ${name}"
  tp_log "CMD   $*"
  if "$@" >>"${TP_LOG_FILE}" 2>&1; then
    tp_log "PASS  ${name}"
    return 0
  fi
  tp_log "FAIL  ${name}"
  tail -n 40 "${TP_LOG_FILE}" >&2 || true
  return 1
}

tp_run_shell() {
  local name="$1"
  shift
  local command="$*"
  tp_run_cmd "${name}" bash -lc "cd \"${TEST_PLAN_ROOT}\" && ${command}"
}

tp_stage_complete() {
  local stage="$1"
  local marker
  marker="$(tp_stage_marker "${stage}")"
  date -u +"%Y-%m-%dT%H:%M:%SZ" >"${marker}"
  tp_log "DONE  marker=${marker}"
}
