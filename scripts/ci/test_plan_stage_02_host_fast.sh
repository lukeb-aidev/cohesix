#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute test-plan stage 02 (host-side fast unit and integration matrix).
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/ci/test_plan_common.sh
source "${script_dir}/test_plan_common.sh"

tp_init
tp_require_stage_done 1
tp_stage_begin 2 "host-fast"

host_matrix=(
  "cargo test -p coh --features mock"
  "cargo test -p cohesix-rest"
  "cargo test -p gpu-bridge-host"
  "cargo test -p cohsh-core"
  "cargo test -p cohsh --test ticket_mint"
  "cargo test -p cohsh --test transcripts"
  "cargo test -p cohsh --test control_plane"
  "cargo test -p cohsh"
  "cargo test -p swarmui --test transcript"
  "cargo test -p swarmui --test console_parity"
  "cargo test -p swarmui --test security"
  "cargo test -p host-sidecar-bridge"
  "cargo test -p host-ticket-agent"
  "cargo test -p nine-door --test ui_security"
  "cargo test -p nine-door --test session_state"
  "cargo test -p nine-door --test pressure_counters"
  "cargo test -p nine-door --test schedule_create"
  "cargo test -p nine-door --test schedule_bounds"
  "cargo test -p nine-door --test lease_bounds"
  "cargo test -p nine-door --test policy_ctl"
  "cargo test -p nine-door --test export_ctl"
  "cargo test -p nine-door --test telemetry_create"
  "cargo test -p nine-door --test telemetry_quotas"
  "cargo test -p nine-door --test telemetry_envelope"
  "cargo test -p nine-door --test integration"
  "cargo test -p cohsh-core --test trace"
  "cargo test -p cohsh --test trace"
  "cargo test -p swarmui --test trace"
  "cargo run -p coh --features mock -- doctor --mock"
  "cargo test -p hive-gateway"
  "cargo test -p tests"
  "cargo test -p root-task --no-default-features --features net-console net:: -- --nocapture"
  "cargo test --workspace"
)

for command in "${host_matrix[@]}"; do
  tp_run_shell "${command}" "${command}"
done

if [[ "${TP_SKIP_PYTHON:-0}" == "1" ]]; then
  tp_mark_incomplete \
    "python-matrix" \
    "TP_SKIP_PYTHON=1" \
    "Python client tests and examples were not executed; host tool parity and release-bundle correctness are unproven."
else
  python_bin="${TP_PYTHON_BIN:-python3}"
  if ! "${python_bin}" -c "import pytest" >/dev/null 2>&1; then
    venv_dir="${TEST_PLAN_STATE_DIR}/.venv"
    tp_log "INFO  pytest not available via ${python_bin}; provisioning ${venv_dir}"
    if [[ ! -x "${venv_dir}/bin/python3" ]]; then
      tp_run_shell "python-venv-create" "python3 -m venv \"${venv_dir}\""
    fi
    python_bin="${venv_dir}/bin/python3"
    if ! "${python_bin}" -c "import pytest" >/dev/null 2>&1; then
      tp_run_shell "python-venv-install-pytest" "\"${python_bin}\" -m pip install pytest"
    fi
  fi
  python_matrix=(
    "\"${python_bin}\" -m pytest tests/test_pi4_trace_normalize.py"
    "\"${python_bin}\" -m pytest tools/cohesix-py/tests"
    "\"${python_bin}\" tools/cohesix-py/examples/lease_run.py --mock"
    "\"${python_bin}\" tools/cohesix-py/examples/peft_roundtrip.py --mock"
    "\"${python_bin}\" tools/cohesix-py/examples/telemetry_write_pull.py --mock"
    "\"${python_bin}\" tools/cohesix-py/examples/use_case_playbook.py --playbook mixed-closed-loop-ai-factory --dry-run --mock --no-proc-snapshot --no-host-snapshot --no-push-host-snapshot --out out/test-plan/python-playbooks"
  )
  for command in "${python_matrix[@]}"; do
    tp_run_shell "${command}" "${command}"
  done
fi

if [[ "${TP_WRITE_TRACE_FIXTURES:-0}" == "1" ]]; then
  tp_run_shell "rewrite-cohsh-trace-fixtures" "COHESIX_WRITE_TRACE=1 cargo test -p cohsh --test trace"
  tp_run_shell "rewrite-swarmui-trace-fixtures" "COHESIX_WRITE_TRACE=1 cargo test -p swarmui --test trace"
fi

tp_stage_complete 2
