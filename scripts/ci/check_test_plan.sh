#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Validate the canonical test catalog, documentation projection, and runner integration.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "${repo_root}"

python3 scripts/ci/test_plan_catalog.py validate
python3 scripts/ci/test_plan_catalog.py check-doc --doc docs/TEST_PLAN.md
python3 scripts/ci/test_plan_integrity.py

shell_scripts=(
  scripts/ci/check_test_plan.sh
  scripts/ci/due_diligence_gate.sh
  scripts/ci/host_hermetic_gate.sh
  scripts/ci/python_test_gate.sh
  scripts/ci/swarmui_ui_gate.sh
  scripts/ci/test_plan_common.sh
  scripts/ci/test_plan_converge.sh
  scripts/ci/test_plan_resources.sh
  scripts/ci/test_plan_run.sh
  scripts/ci/test_plan_stage_01_integrity.sh
  scripts/ci/test_plan_stage_02_host_fast.sh
  scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh
  scripts/ci/test_plan_stage_04_rest_multiplexer.sh
  scripts/ci/test_plan_stage_05_due_diligence.sh
  scripts/ci/test_plan_target_canary.sh
  scripts/cohsh/REST_regression_batch.sh
  scripts/cohsh/run_regression_batch.sh
)
bash -n "${shell_scripts[@]}"

printf "test plan integrity checks ok\n"
