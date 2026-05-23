# Author: Lukas Bower
# Purpose: Guard Pi 4 manifest defaults required for DHCP driver-task bring-up.
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
