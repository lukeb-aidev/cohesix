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
tp_resolve_python_311
python_bin="${TP_PYTHON_RESOLVED}"

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
  "\"${python_bin}\" scripts/ci/check_swarmui_dependencies.py"
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
  "\"${python_bin}\" scripts/ci/check_driver_test_coverage.py"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio"
  "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart"
  "cargo test -p pi4-driver-abi"
  "cargo test -p pi4-driver-runtime -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_backplane_alp_timing_matches_linux_wall_clock_window -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_backplane_attach_clears_extra_pullups_once_before_chipcommon_window -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_generation_reprobe_trace_clears_extra_pullups_once -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_pullup_clear_consumes_one_retained_turn_with_one_exact_sdio_operation -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_pullup_clear_failure_poison_prevents_same_generation_replay -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib sdio_owner_reciprocal_pullup_clear_is_exactly_once_per_generation -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_pullup_clear_crosses_real_runtime_ring_controller_seam_once -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_card_init_uses_linux_command_and_ready_bounds -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib sdio_short_busy_timeout_is_post_issue_and_never_retryable -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_supervisor_display_status_is_concise_and_machine_record_free -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_hdmi_milestone_uses_a_distinct_later_operator_turn -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib delayed_cyw43_hdmi_fifo_retains_full_queue_and_terminal_release -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_function_ready_deadlines_preserve_linux_default_and_f2_window -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib cyw43_data_tx_never_replays_ambiguous_function2_write -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib production_loop_routes_idle_and_retained_commands_to_blocking_receive -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib pending_quantum_requires_one_exact_endpoint_rendezvous -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib pending_quantum_coalesced_peer_irq_arbitration_is_fair_and_durable -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib pending_quantum_accepts_only_the_exact_one_way_endpoint_rendezvous -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib pending_command_dpc_arbitration_requires_separate_endpoint_rendezvous -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib reciprocal_sdio_child_submit_and_polls_require_separate_root_rendezvous -- --test-threads=1"
  "cargo test -p pi4-driver-runtime --lib retained_256_poll_drain_spends_256_root_endpoint_rendezvous -- --test-threads=1"
  "cargo check -p pi4-driver-runtime --target aarch64-unknown-none"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib ninedoor::tests"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::tests::runtime_"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::driver_task_net -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib linked_runtime_service_badges_exclude_the_reserved_root_bit -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_bootstrap_operator_turn -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_one_way_turn_keeps_a_demoted_pi_runtime_schedulable -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_ring_sequence_is_invisible_until_the_dedicated_issue_turn -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_poll_miss_arms_one_endpoint_rendezvous_or_quarantines_the_request -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_priority_lease_identity_rejects_request_fingerprint_and_generation_aliases -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib non_pair_retained_faults_never_request_cyw43_sdio_recovery -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_service_turn_preserves_pending_and_rejects_aliases -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_engine_deadline_poisons_an_issued_request_without_replay -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib sdio_engine_deadline_poisons_an_issued_request_without_replay -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib descriptor_issued_deadlines_poison_both_linked_runtime_generations -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib pre_bundle_pair_recovery_reacquires_firmware_before_continuation -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_engine_active_request_requires_the_complete_immutable_fingerprint -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_maintenance_deadline_poisons_only_an_issued_action -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_turn_status_reports_transitions_and_power_of_two_repeats -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib linked_runtime_tx_ -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_rx_terminal_transport_failure_is_not_reported_as_pending -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_tx_idle_terminal_transport_failure_poison_is_idempotent -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_staged_tx_terminal_transport_failure_is_not_backpressure -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib ordinary_linked_dispatch_echoes_buffered_keyboard_without_usb_or_display_turn -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib ordinary_linked_dispatch_routes_arrows_to_echo_before_later_display_phase -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_pair_restart_executes_one_operation_per_turn_in_canonical_order -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_pair_restart_every_operation_cut_uses_the_same_outer_fence -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib retained_pair_restart_deadline_failure_fences_before_recovery_mmio -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib cyw43_supervisor_post_up_drain_consumes_256_reciprocal_ring_turns -- --test-threads=1"
  "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib accepted_reboot_blocks_cyw43_bootstrap_until_backend_dispatch -- --test-threads=1"
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
  "\"${TEST_PLAN_ROOT}/out/toolchain/sel4-profile-venv/bin/python\" scripts/sel4_profile.py validate --profile qemu_smp_production --build-dir \"${TEST_PLAN_ROOT}/out/sel4/profile-v2/qemu-smp-production\" --require-source --require-artifacts --for-runtime"
  "SEL4_BUILD_DIR=\"${TEST_PLAN_ROOT}/out/sel4/profile-v2/qemu-smp-production\" cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu"
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
  if ! "${python_bin}" -c "import pytest" >/dev/null 2>&1; then
    venv_dir="${TEST_PLAN_STATE_DIR}/.venv"
    tp_log "INFO  pytest not available via ${python_bin}; provisioning ${venv_dir}"
    venv_python="${venv_dir}/bin/python3"
    if [[ -x "${venv_python}" ]] && ! "${venv_python}" -c \
      'import os, sys; expected = os.path.realpath(sys.argv[1]); actual = os.path.realpath(sys._base_executable); raise SystemExit(0 if sys.version_info >= (3, 11) and actual == expected else 1)' \
      "${python_bin}"; then
      tp_log "INFO  existing Python venv does not match ${python_bin}; recreating ${venv_dir}"
      tp_run_cmd "python-venv-recreate" "${python_bin}" -m venv --clear "${venv_dir}"
    elif [[ ! -x "${venv_python}" ]]; then
      tp_run_cmd "python-venv-create" "${python_bin}" -m venv "${venv_dir}"
    fi
    python_bin="${venv_python}"
    tp_run_cmd \
      "${python_bin} version >= 3.11" \
      "${python_bin}" \
      -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)'
    if ! "${python_bin}" -c "import pytest" >/dev/null 2>&1; then
      tp_run_shell "python-venv-install-pytest" "\"${python_bin}\" -m pip install pytest"
    fi
  fi
  python_matrix=(
    "\"${python_bin}\" -m pytest tests/test_sel4_profile.py"
    "\"${python_bin}\" -m pytest tests/test_pi4_image_build.py"
    "\"${python_bin}\" -m pytest tests/test_pi4_image_identity.py"
    "\"${python_bin}\" -m pytest tests/test_pi4_wifi_repeatability.py"
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
