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

stage5_root="${TP_ATTEMPT_DIR}/governance"
audit_root="${stage5_root}/audit"

tp_run_cmd \
  "due-diligence-gate" \
  env -u DD_REUSE_REGRESSION_BATCH_FROM \
  DD_GATE_LOG_DIR="${audit_root}" \
  DD_REUSE_STAGED_EVIDENCE_FROM="${TEST_PLAN_STATE_DIR}" \
  DD_REUSE_STAGED_EVIDENCE_TARGET="${TEST_PLAN_TARGET}" \
  "${TEST_PLAN_ROOT}/scripts/ci/due_diligence_gate.sh"

tp_run_cmd \
  "publish-stage-05-artifact-root" \
  "${TEST_PLAN_ROOT}/scripts/ci/qemu_artifact.py" \
  publish-root \
  --state-dir "${TEST_PLAN_STATE_DIR}" \
  --root "${stage5_root}" \
  --pointer "${TEST_PLAN_STATE_DIR}/stage_05_artifact_root.path"

tp_stage_complete 5
