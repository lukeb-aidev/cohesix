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
    require_feature(errors, features, "usb", {"dep:usb-oxide"})
    usb_oxide = cargo["dependencies"].get("usb-oxide", {})
    if not isinstance(usb_oxide, dict) or not usb_oxide.get("optional"):
        errors.append("apps/root-task/Cargo.toml: `usb-oxide` must be optional behind `usb`")
    if "net-backend-virtio" in set(features.get("release-pi4", [])):
        errors.append("apps/root-task/Cargo.toml: `release-pi4` must not select QEMU VirtIO")


def main() -> int:
    errors: list[str] = []
    check_feature_bundles(errors)

    require_tokens(
        errors,
        "docs/DRIVERS.md",
        [
            "release-qemu",
            "release-pi4",
            "driver-tests-qemu",
            "driver-tests-pi4",
            "MMIO, IRQ, DMA, PCI, SDIO, power/reset, and firmware service calls are HAL",
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
            "driver-tests-pi4 --lib drivers::bcmgenet",
            "driver-tests-pi4 --lib drivers::cyw43",
            "driver-tests-pi4 --lib hal::bcmgenet",
            "driver-tests-pi4 --lib hal::pi4_pcie",
            "driver-tests-pi4 --lib hal::pi4_wifi",
            "driver-tests-pi4,net-console --lib event::tests::nettest_reports_wifi_host_eapol_pending_detail",
            "driver-tests-pi4,net-console --lib event::tests::netstats_emits_compact_status_line",
            "driver-tests-pi4 --lib local_seat::",
            "driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::",
            "--features release-qemu",
            "--features release-pi4",
            "--features cache-maintenance --test cache_maintenance",
        ],
    )
    require_tokens(
        errors,
        "scripts/ci/test_plan_stage_02_host_fast.sh",
        [
            "python3 scripts/ci/check_driver_test_coverage.py",
            "driver-tests-qemu --lib drivers::rtl8139",
            "driver-tests-qemu --lib drivers::virtio",
            "driver-tests-qemu --lib hal::pci",
            "driver-tests-qemu --lib hal::virtio_mmio",
            "driver-tests-qemu --lib hal::uart",
            "driver-tests-pi4 --lib drivers::bcmgenet",
            "driver-tests-pi4 --lib drivers::cyw43",
            "driver-tests-pi4 --lib hal::bcmgenet",
            "driver-tests-pi4 --lib hal::pi4_pcie",
            "driver-tests-pi4 --lib hal::pi4_wifi",
            "driver-tests-pi4 --lib local_seat::",
            "driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::",
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
        "apps/root-task/src/drivers/bcmgenet.rs": [
            "dma_phys_addresses_use_pi4_bus_alias_window",
            "tx_len_status_sets_required_bits",
            "rx_len_status_round_trip",
        ],
        "apps/root-task/src/drivers/cyw43.rs": [
            "startup_link_reply_failure_retry_stays_startup_safe_until_f2_succeeds",
            "direct_function2_reply_blockers_do_not_trigger_startup_link_reply_rescue",
            "sdpcm_credit_window_respects_sequence_bounds",
        ],
        "apps/root-task/src/hal/bcmgenet.rs": [
            "genet_dma_policy_is_physical_uncached_for_pi4",
            "genet_candidate_requires_all_register_pages_covered",
        ],
        "apps/root-task/src/hal/cache.rs": [
            "cache_labels_use_sel4_aarch64_vspace_invocations",
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
            'feature = "usb"',
            'any(target_os = "none", test)',
            "mod local_seat_pi4",
        ],
        "apps/root-task/src/local_seat_pi4.rs": [
            "driver_coverage_pi4_local_seat_usb_vl805_dma_contracts",
            "pre_reset_irq_quiesce_requires_hal_source_clear_before_ack",
            "pi4_pcie_dma_window_uses_linux_captured_bcm2711_dma_range",
            "pi4_xhci_dma_policy_never_tries_raw_phys_after_pcie_alias",
            "xhci_high_bar_runtime_runs_polling_only",
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
