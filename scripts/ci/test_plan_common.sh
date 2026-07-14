#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Shared helpers for staged Cohesix test-plan execution scripts.
# Copyright 2026 Lukas Bower

set -euo pipefail

tp_incomplete_dir() {
  printf "%s/incomplete" "${TEST_PLAN_STATE_DIR}"
}

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

tp_stage_iteration_marker() {
  local stage="$1"
  printf "%s/stage_%02d.iteration" "${TEST_PLAN_STATE_DIR}" "${stage}"
}

tp_stage_fingerprint_file() {
  local stage="$1"
  printf "%s/stage_%02d.inputs.sha256" "${TEST_PLAN_STATE_DIR}" "${stage}"
}

tp_stage_input_fingerprint() {
  local stage="$1"
  python3 - "${TEST_PLAN_ROOT}" "${stage}" <<'PY'
import hashlib
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
stage = sys.argv[2]

stage_scripts = {
    "1": ["scripts/ci/test_plan_stage_01_integrity.sh"],
    "2": ["scripts/ci/test_plan_stage_02_host_fast.sh"],
    "3": ["scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh"],
    "4": ["scripts/ci/test_plan_stage_04_rest_multiplexer.sh"],
    "5": ["scripts/ci/test_plan_stage_05_due_diligence.sh"],
}

paths = {
    "docs/TEST_PLAN.md",
    "scripts/ci/test_plan_run.sh",
    "scripts/ci/test_plan_common.sh",
    *stage_scripts.get(stage, []),
}

if stage == "1":
    paths.update(
        {
            "scripts/ci/check_test_plan.sh",
            "scripts/check-generated.sh",
        }
    )
elif stage == "3":
    paths.add("scripts/cohsh/run_regression_batch.sh")
    paths.update(
        str(path.relative_to(root))
        for path in sorted((root / "scripts/cohsh").glob("*.coh"))
        if path.is_file()
    )
elif stage == "4":
    paths.update(
        {
            "scripts/cohsh/REST_regression_batch.sh",
            "tools/cohesix-py/cohesix/backends.py",
        }
    )
    paths.update(
        str(path.relative_to(root))
        for path in sorted((root / "scripts/cohsh").glob("*.coh"))
        if path.is_file()
    )
elif stage == "5":
    paths.update(
        {
            "scripts/ci/due_diligence_gate.sh",
            "scripts/ci/check_test_plan.sh",
            "scripts/cohsh/run_regression_batch.sh",
            "scripts/check-generated.sh",
        }
    )

digest = hashlib.sha256()
for rel in sorted(paths):
    path = root / rel
    digest.update(rel.encode("utf-8"))
    digest.update(b"\0")
    if path.is_file():
        digest.update(hashlib.sha256(path.read_bytes()).hexdigest().encode("ascii"))
    else:
        digest.update(b"MISSING")
    digest.update(b"\n")
print(digest.hexdigest())
PY
}

tp_write_stage_fingerprint() {
  local stage="$1"
  local path
  path="$(tp_stage_fingerprint_file "${stage}")"
  local fingerprint
  fingerprint="$(tp_stage_input_fingerprint "${stage}")"
  {
    printf "stage=%02d\n" "${stage}"
    printf "target=%s\n" "${TEST_PLAN_TARGET:-unknown}"
    printf "fingerprint=%s\n" "${fingerprint}"
    printf "written_at_utc=%s\n" "$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  } >"${path}"
}

tp_stage_fingerprint_value() {
  local path="$1"
  sed -n 's/^fingerprint=//p' "${path}" | tail -n 1
}

tp_assert_stage_fingerprint_fresh() {
  local stage="$1"
  local path
  path="$(tp_stage_fingerprint_file "${stage}")"
  if [[ ! -f "${path}" ]]; then
    printf "warning: missing input fingerprint for stage %02d: %s\n" "${stage}" "${path}" >&2
    return 0
  fi
  local recorded
  recorded="$(tp_stage_fingerprint_value "${path}")"
  local current
  current="$(tp_stage_input_fingerprint "${stage}")"
  if [[ -z "${recorded}" || "${recorded}" != "${current}" ]]; then
    printf "stale input fingerprint for stage %02d: %s\n" "${stage}" "${path}" >&2
    printf "  recorded: %s\n" "${recorded:-missing}" >&2
    printf "  current:  %s\n" "${current}" >&2
    printf "rerun stage %02d in this state dir before reusing later evidence\n" "${stage}" >&2
    return 1
  fi
}

tp_stage_begin() {
  local stage="$1"
  local name="$2"
  TP_STAGE="${stage}"
  TP_STAGE_NAME="${name}"
  TP_STAGE_TAG="$(tp_stage_tag "${stage}")"
  TP_LOG_FILE="${TEST_PLAN_LOG_DIR}/${TP_STAGE_TAG}-${name}.log"
  TP_HAS_INCOMPLETE=0
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

tp_mark_incomplete() {
  local step="$1"
  local reason="$2"
  local impact="$3"

  TP_HAS_INCOMPLETE=1
  local dir
  dir="$(tp_incomplete_dir)"
  mkdir -p "${dir}"

  local timestamp
  timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  local path="${dir}/${TP_STAGE_TAG}-${step}.md"
  cat >"${path}" <<EOF
# INCOMPLETE: ${step}

- timestamp: ${timestamp}
- stage: ${TP_STAGE_TAG} (${TP_STAGE_NAME})
- step: ${step}
- reason: ${reason}
- impact: ${impact}

Remediation: run the skipped step(s) and re-run this stage in the same state dir.
EOF

  tp_log "INCOMPLETE step=${step} reason=${reason}"
  tp_log "INCOMPLETE file=${path}"
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
  tp_run_cmd "${name}" bash -lc "cd \"${TEST_PLAN_ROOT}\" && ${command}"
}

tp_stage_complete() {
  local stage="$1"
  local marker
  marker="$(tp_stage_marker "${stage}")"
  tp_write_stage_fingerprint "${stage}"
  if [[ "${TP_HAS_INCOMPLETE:-0}" != "0" ]]; then
    local incomplete_marker="${marker%.done}.incomplete"
    date -u +"%Y-%m-%dT%H:%M:%SZ" >"${incomplete_marker}"
    tp_log "FAIL  incomplete marker=${incomplete_marker}"
    tp_log "FAIL  see $(tp_incomplete_dir)/ for details"
    return 1
  fi
  if [[ "${TEST_PLAN_ITERATION:-0}" == "1" ]]; then
    local iteration_marker
    iteration_marker="$(tp_stage_iteration_marker "${stage}")"
    date -u +"%Y-%m-%dT%H:%M:%SZ" >"${iteration_marker}"
    tp_log "ITER  marker=${iteration_marker}"
    tp_log "ITER  no PASS marker written"
    return 0
  fi
  date -u +"%Y-%m-%dT%H:%M:%SZ" >"${marker}"
  tp_log "DONE  marker=${marker}"
}
