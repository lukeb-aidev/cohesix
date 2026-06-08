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
export CARGO_INCREMENTAL=0
tp_log "INFO  CARGO_INCREMENTAL=0 for deterministic macOS host-fast cargo commands"

host_matrix=(
  "cargo test -p coh --features mock"
  "cargo test -p cohesix-rest"
  "cargo test -p gpu-bridge-host"
  "cargo test -p cohsh-core"
  "cargo test -p cohsh --test ticket_mint"
  "cargo test -p cohsh --test transcripts"
  "cargo test -p cohsh --test control_plane"
  "cargo test -p cohsh --test pooling"
  "cargo test -p cohsh"
  "cargo test -p secure9p-core --test session_limits"
  "CARGO_INCREMENTAL=0 cargo check -p swarmui --bin swarmui"
  "python3 scripts/ci/check_swarmui_dependencies.py"
  "CARGO_INCREMENTAL=0 cargo test -p swarmui --test dependency_policy"
  "CARGO_INCREMENTAL=0 cargo test -p swarmui --test transcript"
  "CARGO_INCREMENTAL=0 cargo test -p swarmui --test console_parity"
  "CARGO_INCREMENTAL=0 cargo test -p swarmui --test security"
  "CARGO_INCREMENTAL=0 cargo test -p swarmui --test tauri2_config"
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
  "CARGO_INCREMENTAL=0 cargo test -p swarmui --test trace"
  "cargo run -p coh --features mock -- doctor --mock"
  "cargo test -p hive-gateway"
  "cargo test -p tests"
  "python3 scripts/ci/check_driver_test_coverage.py"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart"
  "cargo test -p pi4-driver-abi"
  "cargo test -p pi4-driver-runtime"
  "cargo check -p pi4-driver-runtime --target aarch64-unknown-none"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib ninedoor::tests"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::driver_task_net"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib net::stack"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::poll_io_obeys_driver_task_budget"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::flush_tx_backpressure_does_not_count_as_budget_overrun"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_write_moves_bytes_without_root_port_pointer"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_poll_moves_rx_bytes_without_root_port_pointer"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_skips_ready_network_data_poll_for_driver_task_turn"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_defers_buffered_network_console_lines_for_driver_task_turn"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4,net-console --lib event::tests::nettest_reports_wifi_host_eapol_pending_detail"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4,net-console --lib event::tests::netstats_emits_compact_status_line"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::"
  "cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance"
  "cargo test -p sel4-sys --lib"
  "SEL4_BUILD_DIR=\"${TEST_PLAN_ROOT}/seL4/SMP_build\" cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu"
  "SEL4_BUILD_DIR=\"${TEST_PLAN_ROOT}/seL4/build_UBOOT\" cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4"
  "cargo test -p root-task --no-default-features --features net-console --lib net:: -- --nocapture"
  "CARGO_INCREMENTAL=0 cargo test --workspace --exclude swarmui"
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
    "\"${python_bin}\" -m pytest tests/test_pi4_gate_proof.py"
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
