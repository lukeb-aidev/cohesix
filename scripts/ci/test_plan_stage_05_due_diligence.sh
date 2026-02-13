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

tp_run_cmd "due-diligence-gate" "${TEST_PLAN_ROOT}/scripts/ci/due_diligence_gate.sh"

tp_stage_complete 5
