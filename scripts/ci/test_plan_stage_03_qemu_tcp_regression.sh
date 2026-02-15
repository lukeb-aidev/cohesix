#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute test-plan stage 03 (QEMU TCP regression batch).
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
tp_require_stage_done 2
tp_stage_begin 3 "qemu-tcp-regression"

ready_timeout="${TP_STAGE3_READY_TIMEOUT:-900}"
port_timeout="${TP_STAGE3_PORT_TIMEOUT:-60}"
quit_close_timeout="${TP_STAGE3_QUIT_CLOSE_TIMEOUT:-60}"
auth_ready_timeout="${TP_STAGE3_AUTH_READY_TIMEOUT:-120}"

tp_run_cmd \
  "cohsh-qemu-regression-batch" \
  env \
  COHSH_LOG_ROOT="${TEST_PLAN_STATE_DIR}/qemu-regression-logs" \
  READY_TIMEOUT="${ready_timeout}" \
  PORT_TIMEOUT="${port_timeout}" \
  QUIT_CLOSE_TIMEOUT="${quit_close_timeout}" \
  AUTH_READY_TIMEOUT="${auth_ready_timeout}" \
  "${TEST_PLAN_ROOT}/scripts/cohsh/run_regression_batch.sh"

tp_stage_complete 3
