#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute test-plan stage 01 (artifact and fixture integrity checks).
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
tp_stage_begin 1 "integrity"

tp_run_cmd "cargo-lockfile" cargo metadata --locked --no-deps
tp_run_cmd "test-plan-hash-check" "${TEST_PLAN_ROOT}/scripts/ci/check_test_plan.sh"
if [[ "${TP_SKIP_GENERATED_CHECK:-0}" == "1" ]]; then
  tp_mark_incomplete \
    "generated-artifacts" \
    "TP_SKIP_GENERATED_CHECK=1" \
    "Generated artifact drift is not checked; docs-as-built and manifest/codegen alignment may be invalid."
else
  tp_run_cmd "generated-artifacts" "${TEST_PLAN_ROOT}/scripts/check-generated.sh"
fi

tp_stage_complete 1
