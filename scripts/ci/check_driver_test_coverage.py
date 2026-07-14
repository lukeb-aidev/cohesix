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
            "driver-tests-qemu --lib drivers::rtl8139",
            "driver-tests-qemu --lib drivers::virtio",
            "driver-tests-qemu --lib hal::pci",
            "driver-tests-qemu --lib hal::virtio_mmio",
            "driver-tests-qemu --lib hal::uart",
            "driver-tests-pi4 --lib hal::pi4_pcie",
            "driver-tests-pi4 --lib hal::pi4_wifi",
            "driver-tests-pi4 --lib local_seat::",
            "--features release-qemu",
            "--features release-pi4",
            "--features cache-maintenance --test cache_maintenance",
        ],
    )
    require_tokens(
        errors,
        "scripts/ci/test_plan_stage_02_host_fast.sh",
        [
            'python_bin="${TP_PYTHON_RESOLVED}"',
            '\\"${python_bin}\\" scripts/ci/check_driver_test_coverage.py',
            "driver-tests-qemu --lib drivers::rtl8139",
            "driver-tests-qemu --lib drivers::virtio",
            "driver-tests-qemu --lib hal::pci",
            "driver-tests-qemu --lib hal::virtio_mmio",
            "driver-tests-qemu --lib hal::uart",
            "driver-tests-pi4 --lib hal::pi4_pcie",
            "driver-tests-pi4 --lib hal::pi4_wifi",
            "driver-tests-pi4 --lib local_seat::",
            "--features release-qemu",
            "--features release-pi4",
            "--features cache-maintenance --test cache_maintenance",
        ],
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
        ],
        "apps/root-task/src/hal/cache.rs": [
            "cache_labels_use_sel4_aarch64_vspace_invocations",
        ],
        "apps/root-task/src/hal/driver_task.rs": [
            "builtin_driver_task_contracts_are_valid_and_dedicated",
            "priority_order_matches_sel4_and_cooperative_service_rules",
            "builtin_isolation_summary_requires_runtime_proof_for_acceptance",
            "wired_nic_steady_dataplane_trace_is_suppressed_for_benchmarks",
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
        ],
        "apps/root-task/src/local_seat.rs": [
            "local_seat_usb_init_and_enum_use_prompt_slice_only_after_prompt",
            "linked_local_seat_usb_keyboard_ready",
            "register_driver_task_runtime_owner_state",
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
            "usb_keyboard_payload_decoder_matches_former_hid_broad_layouts",
            "usb_keyboard_interrupt_trbs_request_bounded_endpoint_packet_len",
            "usb_command_wait_can_preserve_keyboard_transfer_event",
            "usb_hub_topology_helpers_encode_route_tt_and_speed",
            "usb_command_doorbell_flush_is_barrier_only",
            "usb_runtime_declares_fixed_pcie_owner_link",
            "usb_xhci_64bit_register_publication_keeps_distinct_flush_stages",
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
