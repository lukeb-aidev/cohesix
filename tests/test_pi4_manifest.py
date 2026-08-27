# Author: Lukas Bower
# Purpose: Guard Pi 4 networking, scheduling, and timer manifest defaults.
# Copyright 2026 Lukas Bower

"""Tests for the Pi 4 U-Boot root-task manifest defaults."""

from __future__ import annotations

import tomllib
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
PI4_MANIFEST = REPO_ROOT / "configs" / "root_task_pi4_uboot_aarch64.toml"


def load_pi4_manifest() -> dict[str, object]:
    """Load the Pi 4 U-Boot manifest as TOML."""
    return tomllib.loads(PI4_MANIFEST.read_text(encoding="utf-8"))


def test_pi4_manifest_defaults_to_dhcp_auto_networking() -> None:
    """The no-saved-policy Pi 4 boot must exercise DHCP, not static IPv4."""
    manifest = load_pi4_manifest()
    network = manifest["hw"]["network"]

    assert network["enabled"] is True
    assert network["backend"] == "bcmgenet-v5"
    assert network["mode"] == "dhcp"
    assert network["interface"] == "auto"


def test_pi4_manifest_enables_local_seat_and_fourth_core_net_drivers() -> None:
    """Pi 4 boots must keep HDMI/USB enabled and put both NIC drivers on core 3."""
    manifest = load_pi4_manifest()
    local_seat = manifest["hw"]["local_seat"]
    driver_affinity = manifest["root_task"]["affinity"]["drivers"]

    assert local_seat["enabled"] is True
    assert local_seat["required"] is True
    assert local_seat["keyboard_device"] == "usb-kbd0"
    assert local_seat["display_device"] == "hdmi0"
    assert driver_affinity["bcmgenet-v5"] == 3
    assert driver_affinity["cyw43455"] == 3


def test_pi4_root_preempts_console_with_exact_admitted_response_bounds() -> None:
    """Pi retains pre-auth root priority while deriving the console lane."""

    manifest = load_pi4_manifest()
    temporal = manifest["temporal_authority"]
    tasks = temporal["tasks"]
    root = next(
        task
        for task in tasks
        if task["id"] == "root-control"
    )
    console_task = next(
        task for task in tasks if task["id"] == "console-network-service"
    )
    console = manifest["console_network_service"]
    core_zero = next(
        admission
        for admission in temporal["core_admission"]
        if admission["core"] == 0
    )

    assert root["timeout_policy"] == "natural-postpone"
    assert root["budget_us"] == 2_750
    assert root["period_us"] == 10_000
    assert root["max_refills"] == 2
    assert root["wcet_us"] == 2_500
    assert root["response_time_us"] == 5_100
    assert root["priority"] == 200
    assert root["mcp"] == 200
    assert (
        root["wcet_provenance"]
        == "m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24"
    )
    assert console_task["core"] == 0
    assert console_task["budget_us"] == 3_000
    assert console_task["period_us"] == 10_000
    assert console_task["max_refills"] == 2
    assert console_task["wcet_us"] == 3_000
    assert console_task["response_time_us"] == 8_100
    assert console_task["priority"] == 180
    assert console_task["mcp"] == 200
    assert console_task["priority"] < root["priority"]
    assert console["abi_version"] == 5
    assert console["priority"] == 180
    assert console["mcp"] == 200
    assert console["timer_clock_hz"] == 54_000_000
    core_zero_demand = sum(
        task["budget_us"]
        for task in tasks
        if task["core"] == 0 and task["execution"] == "active"
    )
    assert core_zero_demand == 9_000
    assert core_zero["capacity_us"] - core_zero["reserve_us"] == 9_000


def test_pi4_genet_object_delta_is_backend_derived_without_a_source_toggle() -> None:
    """The BCM GENET backend carries exactly 32 reused-page child cap slots."""

    manifest = load_pi4_manifest()
    network = manifest["hw"]["network"]
    console = manifest["console_network_service"]
    fixed = manifest["worker_resource_admission"]["fixed_objects"]

    assert network["backend"] == "bcmgenet-v5"
    assert "direct_genet" not in console
    assert console.get("direct_virtio", False) is False
    assert console["objects"]["frames"] == 103
    assert console["objects"]["cspace_slots"] == 160
    assert fixed["frames"] == 4_077
    assert fixed["cspace_slots"] == 9_267


def test_pi4_serial_tracks_one_frame_of_fifo_empty_refills() -> None:
    """Pi serial admits every bounded FIFO episode without raising its budget."""

    manifest = load_pi4_manifest()
    serial = next(
        task
        for task in manifest["temporal_authority"]["tasks"]
        if task["id"] == "driver-serial"
    )

    assert serial["scheduling_context_bits"] == 9
    assert serial["max_refills"] == 18
    assert serial["budget_us"] == 500
    assert serial["period_us"] == 10_000
    assert serial["wcet_us"] == 400


def test_pi4_hdmi_damage_compositor_uses_admitted_core_two_burst() -> None:
    """Pi HDMI gets a bounded burst while core two retains its reserve."""

    manifest = load_pi4_manifest()
    temporal = manifest["temporal_authority"]
    tasks = temporal["tasks"]
    hdmi = next(task for task in tasks if task["id"] == "driver-hdmi")
    gpu = next(
        task for task in tasks if task["id"] == "root-worker-executor-gpu"
    )
    core_two = next(
        admission
        for admission in temporal["core_admission"]
        if admission["core"] == 2
    )
    core_two_demand = sum(
        task["budget_us"] for task in tasks if task["core"] == 2
    )

    assert hdmi["budget_us"] == 2_000
    assert hdmi["period_us"] == 10_000
    assert hdmi["wcet_us"] == 1_800
    assert hdmi["response_time_us"] == 2_100
    assert (
        hdmi["wcet_provenance"]
        == "m26e-pi4-hdmi-write-only-candidate-v1"
    )
    assert gpu["budget_us"] == 5_000
    assert gpu["response_time_us"] == 7_100
    assert core_two_demand == 7_400
    assert core_two["capacity_us"] - core_two["reserve_us"] == 9_000
