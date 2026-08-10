#!/usr/bin/env python3
# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Validate driver/HAL coverage gates against docs/DRIVERS.md and release target bundles.
# Author: Lukas Bower

"""Static coverage guard for Cohesix driver and HAL test alignment."""

from __future__ import annotations

import pathlib
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised by macOS system Python.
    tomllib = None


ROOT = pathlib.Path(__file__).resolve().parents[2]


def read_text(rel_path: str) -> str:
    return (ROOT / rel_path).read_text(encoding="utf-8")


def require_tokens(errors: list[str], rel_path: str, tokens: list[str]) -> None:
    text = read_text(rel_path)
    for token in tokens:
        if token not in text:
            errors.append(f"{rel_path}: missing `{token}`")


def require_absent_tokens(errors: list[str], rel_path: str, tokens: list[str]) -> None:
    text = read_text(rel_path)
    for token in tokens:
        if token in text:
            errors.append(f"{rel_path}: forbidden retired driver token `{token}`")


def require_path_absent(errors: list[str], rel_path: str) -> None:
    if (ROOT / rel_path).exists():
        errors.append(f"{rel_path}: retired root-owned driver path must be absent")


def require_feature(
    errors: list[str], features: dict[str, list[str]], name: str, required: set[str]
) -> None:
    actual = set(features.get(name, []))
    missing = sorted(required - actual)
    if missing:
        errors.append(f"apps/root-task/Cargo.toml: feature `{name}` missing {missing}")


def load_root_task_cargo() -> dict[str, object]:
    """Load the root-task manifest on Python versions before `tomllib` exists."""

    text = read_text("apps/root-task/Cargo.toml")
    if tomllib is not None:
        return tomllib.loads(text)
    return parse_root_task_manifest_subset(text)


def check_test_plan_action_catalog(errors: list[str]) -> None:
    """Require broad driver suites without duplicating filtered test runs."""

    if tomllib is None:
        errors.append(
            "scripts/ci/check_driver_test_coverage.py: Python 3.11 or newer "
            "is required to validate configs/test_plan_actions.toml"
        )
        return
    catalog = tomllib.loads(read_text("configs/test_plan_actions.toml"))
    raw_actions = catalog.get("action", [])
    actions = {
        action.get("id"): action
        for action in raw_actions
        if isinstance(action, dict) and isinstance(action.get("id"), str)
    }
    expected_commands = {
        "host.root-task-qemu-features": (
            "cargo test -p root-task --no-default-features "
            "--features driver-tests-qemu --lib -- --test-threads=1 "
            "--skip drivers::driver_task_net"
        ),
        "host.root-task-pi4-features": (
            "cargo test -p root-task --no-default-features "
            "--features driver-tests-pi4 --lib -- --test-threads=1"
        ),
        "host.root-task-net-console": (
            "cargo test -p root-task --no-default-features "
            "--features net-console --lib -- --test-threads=1"
        ),
        "host.pi4-runtime-tests": (
            "cargo test -p pi4-driver-runtime -- --test-threads=1"
        ),
        "host.pi4-runtime-target-check": (
            "cargo check -p pi4-driver-runtime "
            "--target aarch64-unknown-none"
        ),
        "host.cache-maintenance-tests": (
            "cargo test -p root-task --no-default-features "
            "--features cache-maintenance --test cache_maintenance"
        ),
        "target.root-task-qemu-release": "--features release-qemu",
        "target.root-task-pi4-release": "--features release-pi4",
    }
    host_action_ids = {
        action_id
        for action_id in expected_commands
        if action_id.startswith("host.")
    }
    for action_id, expected in expected_commands.items():
        action = actions.get(action_id)
        if action is None:
            errors.append(
                "configs/test_plan_actions.toml: missing driver coverage "
                f"action `{action_id}`"
            )
            continue
        command = action.get("command")
        if not isinstance(command, str) or expected not in command:
            errors.append(
                "configs/test_plan_actions.toml: action "
                f"`{action_id}` must contain `{expected}`"
            )
        expected_stage = 1 if action_id in host_action_ids else 2
        expected_scope = (
            "common"
            if action_id in host_action_ids
            else "provisioned-target"
        )
        if action.get("stage") != expected_stage:
            errors.append(
                "configs/test_plan_actions.toml: "
                f"`{action_id}` must be Stage {expected_stage}"
            )
        if action.get("scope") != expected_scope:
            errors.append(
                "configs/test_plan_actions.toml: "
                f"`{action_id}` must use scope={expected_scope}"
            )
        if action.get("test_policy") == "nonzero":
            minimum = action.get("minimum_test_count")
            if not isinstance(minimum, int) or minimum < 1:
                errors.append(
                    "configs/test_plan_actions.toml: "
                    f"`{action_id}` must reject zero-test execution"
                )


def parse_root_task_manifest_subset(text: str) -> dict[str, object]:
    """Parse the manifest subset needed by this guard without external deps."""

    features: dict[str, list[str]] = {}
    dependencies: dict[str, dict[str, bool]] = {}
    lines = text.splitlines()
    index = 0
    section = ""
    while index < len(lines):
        raw = lines[index]
        stripped = raw.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped.strip("[]")
            index += 1
            continue
        if section == "features":
            match = re.match(r"^([A-Za-z0-9_-]+)\s*=\s*(.*)$", stripped)
            if match:
                name, value = match.groups()
                payload = value
                while "[" in payload and "]" not in payload and index + 1 < len(lines):
                    index += 1
                    payload += "\n" + lines[index]
                features[name] = re.findall(r'"([^"]+)"', payload)
        elif section == "dependencies":
            match = re.match(r"^([A-Za-z0-9_-]+)\s*=\s*(.*)$", stripped)
            if match:
                name, value = match.groups()
                if value.strip().startswith("{"):
                    dependencies[name] = {
                        "optional": "optional = true" in value,
                    }
        index += 1
    return {"features": features, "dependencies": dependencies}


def check_feature_bundles(errors: list[str]) -> None:
    cargo = load_root_task_cargo()
    features = cargo["features"]
    old_usb_dependency = "usb" + "-oxide"
    require_feature(
        errors,
        features,
        "release-qemu",
        {
            "kernel",
            "serial-console",
            "net-console",
            "net-backend-virtio",
            "cache-maintenance",
            "usb",
        },
    )
    require_feature(
        errors,
        features,
        "release-pi4",
        {"kernel", "serial-console", "net-console", "cache-maintenance", "usb"},
    )
    require_feature(errors, features, "driver-tests-qemu", {"release-qemu"})
    require_feature(errors, features, "driver-tests-pi4", {"release-pi4"})
    retired_usb_dependency = "cohesix" + "-usb"
    if f"dep:{retired_usb_dependency}" in set(features.get("usb", [])):
        errors.append("apps/root-task/Cargo.toml: `usb` must not select the retired root-owned USB crate")
    if retired_usb_dependency in cargo["dependencies"]:
        errors.append("apps/root-task/Cargo.toml: retired root-owned USB crate must not be a dependency")
    if f"dep:{old_usb_dependency}" in set(features.get("usb", [])):
        errors.append("apps/root-task/Cargo.toml: old USB package must not be selected")
    if old_usb_dependency in cargo["dependencies"]:
        errors.append("apps/root-task/Cargo.toml: old USB package must not be a dependency")
    if "net-backend-virtio" in set(features.get("release-pi4", [])):
        errors.append("apps/root-task/Cargo.toml: `release-pi4` must not select QEMU VirtIO")


def main() -> int:
    errors: list[str] = []
    check_feature_bundles(errors)
    check_test_plan_action_catalog(errors)
    require_path_absent(errors, "apps/root-task/src/local_seat_pi4.rs")
    require_path_absent(errors, "crates/cohesix-usb")
    require_absent_tokens(
        errors,
        "Cargo.toml",
        [
            "crates/cohesix-usb",
            "usb-oxide",
        ],
    )
    require_absent_tokens(
        errors,
        "apps/root-task/src/local_seat.rs",
        [
            "LocalSeatUsbOwnerRuntimeRecord",
            "LocalSeatHdmiOwnerRuntimeRecord",
            "root-runtime-pointer",
        ],
    )
    require_absent_tokens(
        errors,
        "apps/root-task/src/hal/pi4_wifi.rs",
        [
            "pub struct Pi4WifiState",
            "Cyw43HostEapolRxSource",
        ],
    )
    require_absent_tokens(
        errors,
        "apps/root-task/src/hal/mod.rs",
        [
            "fn wifi_set_power",
            "fn wifi_set_reset",
            "fn sdio_reset_host",
            "fn sdio_io_direct_read",
            "fn sdio_io_extended",
        ],
    )

    require_tokens(
        errors,
        "docs/DRIVERS.md",
        [
            "release-qemu",
            "release-pi4",
            "driver-tests-qemu",
            "driver-tests-pi4",
            "HAL owns resource admission for MMIO, IRQ, DMA, PCI, SDIO, board-level",
            "Tests cover touched logic paths",
        ],
    )
    require_tokens(
        errors,
        "docs/TEST_PLAN.md",
        [
            "python3 scripts/ci/check_driver_test_coverage.py",
            "`host.root-task-qemu-features`",
            "`host.root-task-pi4-features`",
            "`host.pi4-runtime-tests`",
            "`target.root-task-qemu-release`",
            "`target.root-task-pi4-release`",
            "one broad",
        ],
    )
    require_tokens(
        errors,
        "scripts/ci/test_plan_stage_01_integrity.sh",
        [
            "--stage 1",
            "--scope common",
            "host.python-*",
            "tp_run_catalog_action",
        ],
    )
    require_tokens(
        errors,
        "scripts/ci/test_plan_stage_02_host_fast.sh",
        [
            "--stage 2",
            "--scope provisioned-target",
            "tp_run_catalog_action",
            "ACTION_SET scope=provisioned-target",
        ],
    )
    stage_two = read_text("scripts/ci/test_plan_stage_02_host_fast.sh")
    for forbidden in ("--scope common", "ACTION_SET scope=common"):
        if forbidden in stage_two:
            errors.append(
                "scripts/ci/test_plan_stage_02_host_fast.sh: "
                f"provisioned Stage 2 must not contain `{forbidden}`"
            )

    source_tokens = {
        "apps/root-task/src/drivers/rtl8139.rs": [
            "locate_pci_device_accepts_only_hal_reported_rtl8139_tuple",
            "rtl8139_rejects_io_port_bar_before_mapping_or_dma_allocation",
            "rtl8139_register_and_ring_bounds_match_hal_mmio_contract",
        ],
        "apps/root-task/src/drivers/virtio/net.rs": [
            "accepts_modern_v2",
            "tx_head_reuse_prevented",
            "cache_ops_called_in_right_places",
            "publish_guard_rejects_zero_len_descriptor",
        ],
        "apps/root-task/src/drivers/driver_task_net.rs": [
            "driver_task_nic_tx_token_stages_or_counts_drop_without_mmio",
            "driver_task_nic_receive_is_ring_driven",
            "cyw43_driver_task_firmware_ready_is_not_dhcp_ready",
            "runtime_ring_service_admits_frame_bearing_tx_commands",
            "cyw43_transport_recovery_is_limited_to_owner_backplane_faults",
            "cyw43_firmware_streaming_uses_bounded_boot_chunks",
            "sdio_dpc_diagnostic_derives_exact_ring_service_totals",
            "gate7_diagnostic_requires_one_complete_current_host_eapol_session",
            "cyw43_oldgood_set_ssid_then_m1_commits_association_before_message",
            "cyw43_oldgood_prefix_advances_once_in_exact_generation_order",
            "cyw43_oldgood_prefix_poison_is_sticky_across_skips_and_new_join_attempts",
            "cyw43_oldgood_prefix_is_cleared_by_recovery_and_pair_restart_boundaries",
        ],
        "apps/root-task/src/hal/cache.rs": [
            "cache_labels_use_sel4_aarch64_vspace_invocations",
        ],
        "apps/root-task/src/hal/driver_task.rs": [
            "builtin_driver_task_contracts_are_valid_and_dedicated",
            "priority_order_matches_sel4_and_cooperative_service_rules",
            "builtin_isolation_summary_requires_runtime_proof_for_acceptance",
            "wired_nic_steady_dataplane_trace_is_suppressed_for_benchmarks",
            "cyw43_dpc_client_reader_is_stable_passive_and_sequence_last",
            "usb_engine_init_controller_reset_stage_uses_extended_bounded_timeout",
            "compact_owner_state_rows_preserve_canonical_contract_within_serial_bound",
            "usb_oldgood_stable_reader_requires_two_cache_invalidated_identical_samples",
            "usb_oldgood_current_binding_rejects_partial_poisoned_or_stale_identity",
            "usb_oldgood_context_rejects_descriptor_ring_or_root_writer_change",
            "usb_oldgood_receipt_range_is_disjoint_from_root_ring_housekeeping",
            "deferred_descriptor_status_distinguishes_retained_pending_from_bounded_no_reply",
        ],
        "apps/root-task/src/hal/pci.rs": [
            "topology_find_by_id_matches_expected_device",
            "topology_returns_none_for_missing_device",
        ],
        "apps/root-task/src/hal/pi4_pcie.rs": [
            "bcm2711_pcie_register_pages_are_mapped_in_sel4_cursor_order",
            "vl805_poll_only_command_requires_mem_master_and_intx_disabled",
            "vl805_msi_control_disable_clears_enable_bit_only",
            "bcm2711_dma_window_values_match_pi4_dma_ranges",
        ],
        "apps/root-task/src/hal/pi4_wifi.rs": [
            "firmware_function2_gate_reports_disabled_until_ht_proof",
            "sdio_function_enable_sequence_brings_up_f1_then_f2",
            "cmd53_r5_error_is_reported_only_for_extended_io",
            "armcr4_cpuhalt_matches_upstream_handoff_sequence",
        ],
        "apps/root-task/src/hal/uart.rs": [
            "uart_addresses_match_qemu_and_pi4_platform_windows",
        ],
        "apps/root-task/src/hal/virtio_mmio.rs": [
            "slot_paddr_enforces_bounded_qemu_virt_window",
        ],
        "apps/root-task/src/lib.rs": [
            "pub mod local_seat;",
        ],
        "apps/root-task/src/event/mod.rs": [
            "serial_input_skips_ready_network_data_poll_for_driver_task_turn",
            "serial_tx_backlog_skips_ready_network_data_poll_for_driver_task_turn",
            "local_seat_input_skips_ready_network_poll_for_keyboard_turn",
            "usb_console_startup_feedback_reports_stage_changes_heartbeat_and_timing",
            "usb_console_startup_feedback_uses_bounded_linked_serial_projection",
            "usb_console_ready_projects_canonical_receipt_once_and_first",
            "linked_usb_runtime_queue_fields_decode_first_report_telemetry",
            "hdmi_completion_status_uses_current_active_request_not_cumulative_gap",
            "wifi_diag_authority_lines_fit_the_untruncated_console_budget",
            "wifi_diag_essential_proof_is_contiguous_and_identity_bound",
            "wifi_oldgood_owner_order_is_canonical_for_the_retained_parser",
            "wifi_oldgood_retained_prefix_emits_exact_ordered_legacy_grammar",
            "wifi_oldgood_retained_prefix_fits_maximum_integer_width_and_console_capacity",
            "wifi_oldgood_retained_prefix_rejects_incomplete_or_cross_generation_evidence",
            "wifi_oldgood_retained_batch_enqueue_is_atomic_and_reserves_smp_tail",
            "usb_oldgood_retained_pair_has_strict_bounded_grammar",
            "usb_oldgood_retained_pair_enqueue_is_adjacent_and_atomic_under_backlog",
            "usb_oldgood_projection_call_sites_are_passive_adjacent_and_active_excluded",
        ],
        "apps/root-task/src/local_seat.rs": [
            "local_seat_usb_init_and_enum_use_prompt_slice_only_after_prompt",
            "linked_local_seat_usb_keyboard_ready",
            "usb_command_ready_receipt_and_details_fit_bounded_log_records",
            "register_driver_task_runtime_owner_state",
            "runtime_hdmi_input_row_waits_for_matching_completion_receipt",
            "runtime_hdmi_input_row_preserves_fifo_output_before_and_after_it",
            "runtime_hdmi_input_row_extends_existing_priority_fifo_boundary",
            "runtime_hdmi_input_after_retry_exhaustion_rearms_canonical_redraw",
            "runtime_hdmi_usb_readiness_retracts_and_rereleases_ready_banner",
            "runtime_hdmi_usb_readiness_invalidation_retracts_and_rereleases_prompt",
            "runtime_hdmi_backspace_stops_at_prompt_floor",
            "runtime_hdmi_rapid_arrows_chase_completed_viewport_one_row_per_frame",
            "physical_pi_hdmi_prompt_visibility_requires_usb_command_readiness",
            "idle_first_report_before_replay_keeps_attach_pending_without_controller_reinit",
            "linked_usb_endpoint_cache_lifecycle_is_runtime_owned_and_coherent",
            "linked_usb_preproof_input_survives_and_releases_exactly_once_after_proof",
            "linked_usb_complete_shortcuts_preserve_retained_attach_ticket",
            "terminal_usb_readiness_requires_both_descriptor_and_owner_proof_chains",
            "pre_prompt_usb_retry_does_not_starve_missing_descriptor_or_owner_proof",
        ],
        "apps/pi4-driver-runtime/src/lib.rs": [
            "genet_tx_len_status",
            "genet_decode_rx_len",
            "genet_runtime_submit_tx",
            "genet_runtime_init_hw",
            "genet_rx_queue_preserves_burst_order_between_service_turns",
            "genet_rx_queue_full_rejects_without_overwriting_preserved_frames",
            "genet_rx_drain_budget_caps_one_service_turn",
            "genet_tx_completion_reclaim_budget_caps_one_service_turn",
            "genet_service_reports_budget_exhaustion_before_dataplane_work",
            "cyw43_data_tx_is_credit_gated_and_preserves_sequence_on_no_credit",
            "cyw43_control_tx_is_credit_gated_and_preserves_sequence_on_no_credit",
            "cyw43_rx_glom_and_deferred_queue_caps_match_pi4_stability_envelope",
            "cyw43_glom_rx_deaggregates_first_frame_and_queues_followup",
            "cyw43_rx_queue_removes_matching_channel_without_reordering_data",
            "cyw43_dpc_client_record_is_current_sequence_last_and_passive",
            "cyw43_owner_rearm_checkpoints_client_record_after_the_existing_hint",
            "usb_keyboard_payload_decoder_matches_former_hid_broad_layouts",
            "usb_keyboard_interrupt_trbs_request_bounded_endpoint_packet_len",
            "usb_command_wait_can_preserve_keyboard_transfer_event",
            "usb_hub_topology_helpers_encode_route_tt_and_speed",
            "usb_command_doorbell_flush_is_barrier_only",
            "usb_runtime_declares_fixed_pcie_owner_link",
            "usb_xhci_64bit_register_publication_keeps_distinct_flush_stages",
            "usb_keyboard_held_arrow_repeat_is_separate_from_report_edges",
            "usb_keyboard_steady_poll_emits_due_arrow_repeat_without_new_report",
            "hdmi_runtime_csi_scroll_steps_preserve_cursor_and_dirty_one_edge_row",
        ],
        "crates/pi4-driver-abi/src/lib.rs": [
            "cyw43_dpc_client_layout_is_bounded_and_sequence_last",
            "usb_oldgood_receipt_is_fixed_commit_last_identity_bound_and_fail_closed",
        ],
        "apps/root-task/src/net/stack.rs": [
            "cyw43_oldgood_dhcp_receipt_is_exact_generation_bound_and_resettable",
        ],
        "apps/root-task/src/serial/mod.rs": [
            "poll_io_obeys_driver_task_budget",
            "flush_tx_obeys_driver_task_budget",
            "flush_tx_backpressure_does_not_count_as_budget_overrun",
        ],
        "apps/root-task/tests/cache_maintenance.rs": [
            "cache_maintenance_dma_audit_logs_flush_before_share_ready",
            "cache_maintenance_dma_sync_for_cpu_invalidates_before_ready",
        ],
        "tests/test_pi4_trace_normalize.py": [
            "test_gate_summary_rejects_latest_malformed_wifi_dpc_accounting",
            "test_gate_summary_rejects_latest_orphan_malformed_wifi_dpc_scope",
            "test_gate_summary_rejects_wifi_dpc_u32_overflow",
            "test_gate_summary_does_not_infer_first_byte_from_usb_gate10",
            "test_gate_summary_uses_linked_runtime_first_byte_for_post_input_health",
            "test_gate_summary_uses_current_active_no_progress_not_cumulative_keep_active",
            "test_gate_summary_quarantines_invalid_usb_queue_enumeration_snapshot",
            "test_gate_summary_rejects_non_one_deep_queue_before_first_byte",
            "test_gate_summary_keeps_invalid_queue_depth_after_later_first_byte",
            "test_boot_evidence_requires_current_one_deep_usb_queue",
            "test_hdmi_passive_status_requires_driver_completion_receipt",
            "test_hdmi_passive_status_rejects_missing_or_inconsistent_driver_receipt",
            "test_retained_gate7_requires_one_exact_current_diag_transaction",
            "test_retained_gate7_rejects_standalone_or_prior_truncated_row",
            "test_retained_gate7_full_match_fails_closed",
            "test_wifi_diag_complete_rejects_scrubbed_gate8_summary",
            "test_wifi_diag_complete_recovers_exact_current_clipped_gate8",
            "test_wifi_diag_complete_rejects_mismatched_clipped_gate8_identity",
            "test_wifi_diag_complete_revokes_old_gate8_on_latest_invalid_summary",
            "test_wifi_diag_context_requires_exact_matching_nonzero_id",
            "test_bare_reserved_wifi_dpc_row_revokes_older_triplet",
            "test_later_healthy_wifi_dpc_triplet_supersedes_transient_stale_sample",
            "test_newer_incomplete_wifi_diag_revokes_older_current_snapshot",
            "test_gate8_recovery_after_diag_terminal_revokes_older_pass",
            "test_compact_wifi_diag_terminal_requires_exact_emitted_grammar",
            "test_attempt_after_ready_revokes_gate8_authority",
            "test_current_wifi_diag_does_not_reuse_an_older_dpc_sample",
            "test_malformed_current_wifi_diag_command_cannot_reuse_dpc",
            "test_current_wifi_diag_accepts_only_its_fresh_dpc_triplet",
            "test_current_wifi_diag_complete_without_begin_cannot_reuse_dpc",
            "test_current_wifi_diag_complete_cannot_stitch_to_prior_command",
            "test_current_wifi_diag_rejects_complete_before_transaction_terminal",
            "test_gate_summary_accepts_identity_bound_wifi_oldgood_retained_prefix",
            "test_wifi_oldgood_retained_prefix_rejects_noncontiguous_or_reordered_rows",
            "test_wifi_oldgood_retained_prefix_rejects_wrong_owner_or_identity",
            "test_newer_incomplete_wifi_oldgood_retained_prefix_revokes_older_complete",
            "test_wifi_oldgood_retained_prefix_is_revoked_by_later_join_or_recovery",
            "test_wifi_oldgood_retained_prefix_requires_fresh_tcp_and_dpc_tail",
            "test_wifi_oldgood_retained_prefix_rejects_cross_generation_network_tail",
            "test_truncated_wifi_oldgood_retained_prefix_cannot_supply_live_gate7",
            "test_malformed_wifi_oldgood_retained_prefix_revokes_older_live_authority",
            "test_wifi_oldgood_retained_prefix_rejects_impossible_producer_widths",
            "test_gate_summary_accepts_identity_bound_usb_oldgood_retained_pair",
            "test_usb_oldgood_retained_pair_rejects_incomplete_or_noncurrent_proof",
            "test_usb_oldgood_retained_pair_requires_latest_physical_adjacency",
            "test_newer_invalid_usb_oldgood_pair_revokes_older_complete_pair",
        ],
        "tests/test_pi4_gate_proof.py": [
            "test_gate_proof_accepts_identity_bound_usb_oldgood_retained_pair",
            "test_gate_proof_ignores_uncommitted_dormant_usb_oldgood_receipt",
        ],
        "tests/test_pi4_serial_reboot.py": [
            "test_serial_read_matches_prompt_across_prior_stream_tail",
            "test_serial_read_rejects_noncontiguous_prompt_tail",
            "test_diagnostic_barrier_accepts_prompt_split_after_first_byte",
        ],
    }
    for rel_path, tokens in source_tokens.items():
        require_tokens(errors, rel_path, tokens)

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("driver/HAL coverage guard ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
