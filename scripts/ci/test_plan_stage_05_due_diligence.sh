#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute test-plan stage 05 (full due-diligence release gate).
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
tp_require_stage_done 4
tp_stage_begin 5 "due-diligence"

reuse_regression="${TP_STAGE5_REUSE_REGRESSION:-1}"
reuse_regression_log_root=""
if [[ "${reuse_regression}" == "1" && "${DD_SKIP_REGRESSION_BATCH:-0}" != "1" && -z "${DD_REUSE_REGRESSION_BATCH_FROM:-}" ]]; then
  stage3_fingerprint="$(tp_stage_fingerprint_file 3)"
  regression_log_root="${TEST_PLAN_STATE_DIR}/qemu-regression-logs"
  if [[ ! -f "${stage3_fingerprint}" ]]; then
    tp_log "INFO  Stage 03 input fingerprint is missing; running exhaustive due-diligence regression batch"
  elif [[ ! -d "${regression_log_root}" ]]; then
    tp_log "FAIL  missing reusable Stage 03 regression logs: ${regression_log_root}"
    exit 1
  else
    tp_assert_stage_fingerprint_fresh 3
    reuse_regression_log_root="${regression_log_root}"
  fi
fi

if [[ -n "${reuse_regression_log_root}" ]]; then
  tp_log "INFO  reusing Stage 03 regression batch evidence: ${regression_log_root}"
  tp_run_cmd \
    "due-diligence-gate" \
    env \
    DD_REUSE_REGRESSION_BATCH_FROM="${reuse_regression_log_root}" \
    "${TEST_PLAN_ROOT}/scripts/ci/due_diligence_gate.sh"
else
  tp_run_cmd "due-diligence-gate" "${TEST_PLAN_ROOT}/scripts/ci/due_diligence_gate.sh"
fi

tp_stage_complete 5
