#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute only the selected provisioned-target checks after reusable common evidence.
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
export CARGO_INCREMENTAL=0
export TEST_PLAN_ROOT TEST_PLAN_STATE_DIR TEST_PLAN_TARGET
tp_require_stage_done 1
tp_stage_begin 2 "host-and-target"
tp_log "INFO  common-hermetic actions are attested by reusable Stage 01 evidence"
catalog_path="${TEST_PLAN_ROOT}/scripts/ci/test_plan_catalog.py"

PROVISIONED_ACTION_IDS=()
while IFS= read -r action_id; do
  [[ -n "${action_id}" ]] && PROVISIONED_ACTION_IDS+=("${action_id}")
done < <(
  python3 "${catalog_path}" list \
    --stage 2 \
    --scope provisioned-target \
    --target "${TEST_PLAN_TARGET}"
)

for action_id in "${PROVISIONED_ACTION_IDS[@]}"; do
  tp_log "ACTION_SET scope=provisioned-target target=${TEST_PLAN_TARGET} action=${action_id}"
done

for action_id in "${PROVISIONED_ACTION_IDS[@]}"; do
  tp_run_catalog_action "${action_id}"
done

tp_stage_complete 2
