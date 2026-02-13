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
if [[ -z "${gateway_url}" ]]; then
  tp_log "FAIL  missing gateway URL"
  tp_log "set COHESIX_GATEWAY_URL (or HIVE_GATEWAY_URL/COHSH_REST_URL/COH_REST_URL) before running stage 04"
  exit 1
fi
tp_log "INFO  gateway-url=${gateway_url}"

tp_run_cmd "cohsh-rest-regression-batch" "${TEST_PLAN_ROOT}/scripts/cohsh/REST_regression_batch.sh"

tp_stage_complete 4
