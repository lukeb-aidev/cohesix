#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute the reusable common-hermetic test-plan closure once.
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
export CARGO_INCREMENTAL=0
export TEST_PLAN_ROOT TEST_PLAN_STATE_DIR TEST_PLAN_TARGET
export TP_PYTHON_PLAYBOOK_OUT="${TEST_PLAN_STATE_DIR}/python-playbooks"
tp_stage_begin 1 "integrity"
catalog_path="${TEST_PLAN_ROOT}/scripts/ci/test_plan_catalog.py"

if [[ "${TP_SKIP_PYTHON:-0}" == "1" ]]; then
  tp_mark_incomplete \
    "python-matrix" \
    "TP_SKIP_PYTHON=1" \
    "Repository Python discovery and client example smokes were not executed; host-tool parity and release-bundle correctness are unproven."
fi

ACTION_IDS=()
while IFS= read -r action_id; do
  [[ -n "${action_id}" ]] && ACTION_IDS+=("${action_id}")
done < <(
  python3 "${catalog_path}" list \
    --stage 1 \
    --scope common
)

for action_id in "${ACTION_IDS[@]}"; do
  if [[ "${action_id}" == "integrity.generated-contracts" &&
        "${TP_SKIP_GENERATED_CHECK:-0}" == "1" ]]; then
    tp_mark_incomplete \
      "generated-artifacts" \
      "TP_SKIP_GENERATED_CHECK=1" \
      "Generated artifact and test-plan catalog drift are not checked; docs-as-built and manifest/codegen alignment may be invalid."
    continue
  fi
  if [[ "${TP_SKIP_PYTHON:-0}" == "1" &&
        "${action_id}" == host.python-* ]]; then
    tp_log "SKIP action=${action_id} reason=TP_SKIP_PYTHON=1"
    continue
  fi
  tp_run_catalog_action "${action_id}"
done

if [[ "${TP_WRITE_TRACE_FIXTURES:-0}" == "1" ]]; then
  tp_run_shell \
    "rewrite-cohsh-trace-fixtures" \
    "COHESIX_WRITE_TRACE=1 cargo test -p cohsh --test trace"
  tp_run_shell \
    "rewrite-swarmui-trace-fixtures" \
    "COHESIX_WRITE_TRACE=1 cargo test -p swarmui --test trace"
fi

tp_stage_complete 1
