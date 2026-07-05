# Author: Lukas Bower
# Purpose: Regression tests for the Pi 4 pyserial reboot helper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_serial_reboot.py."""

from __future__ import annotations

import importlib.util
import pathlib
from collections.abc import Iterable

import pytest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4_serial_reboot.py"

SPEC = importlib.util.spec_from_file_location("pi4_serial_reboot", SCRIPT_PATH)
assert SPEC is not None
pi4_serial_reboot = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(pi4_serial_reboot)


ROOT_MENU_SAVED = b"""
[cohesix] Cohesix boot options
[cohesix] Saved network settings detected
[cohesix] mode=dhcp
[cohesix] interface=wifi
[cohesix] wifi-ssid=<redacted>
  1. Continue with existing config
  2. Configure networking
Select option [1]:
"""

ROOT_MENU_SAVED_WIRED = b"""
[cohesix] Cohesix boot options
[cohesix] Saved network settings detected
[cohesix] mode=dhcp
[cohesix] interface=wired
  1. Continue with existing config
  2. Configure networking
Select option [1]:
"""

ROOT_MENU_DEFAULTS = b"""
[cohesix] Cohesix boot options
[cohesix] No saved network settings; manifest defaults remain active
  1. Boot with manifest defaults
  2. Configure networking
Select option [1]:
"""

DHCP_MENU = b"""
[cohesix] Guided network setup
[cohesix] Select address acquisition mode
  1. DHCP ON (automatic address)
  2. DHCP OFF (static IPv4)
  3. Back to boot options
Select option [1]:
"""

INTERFACE_MENU = b"""
[cohesix] Guided network setup
[cohesix] Select active interface
  1. Wired Ethernet (GENET)
  2. Wi-Fi (CYW43455)
  3. Back to DHCP selection
Select option [1]:
"""

REVIEW_WIRED_DHCP = b"""
[cohesix] Review network settings
[cohesix] mode=dhcp
[cohesix] interface=wired
  1. Boot with these settings
  2. Save settings and reboot
  3. Edit settings
Select option [1]:
"""

REVIEW_WIFI_DHCP = b"""
[cohesix] Review network settings
[cohesix] mode=dhcp
[cohesix] interface=wifi
  1. Boot with these settings
  2. Save settings and reboot
  3. Edit settings
Select option [1]:
"""


class FakeController:
    """Small serial-controller test double for menu-selection logic."""

    def __init__(self, reads: Iterable[bytes] = ()) -> None:
        self.reads = list(reads)
        self.sent: list[str] = []
        self.public_sent: list[str] = []
        self.reinforced: list[bool] = []
        self.notes: list[str] = []
        self.drains: list[tuple[float, str]] = []
        self.redactions: list[tuple[str, str]] = []

    def note(self, text: str) -> None:
        self.notes.append(text)

    def add_redaction(self, secret: str, replacement: str) -> None:
        self.redactions.append((secret, replacement))

    def send_line(
        self,
        line: str,
        *,
        public_line: str | None = None,
        reinforce_terminator: bool = False,
    ) -> None:
        self.sent.append(line)
        self.public_sent.append(public_line if public_line is not None else line)
        self.reinforced.append(reinforce_terminator)

    def read_until(
        self,
        markers: Iterable[bytes],
        timeout_s: float,
        *,
        label: str,
    ) -> bytes:
        del timeout_s, label
        assert self.reads, f"unexpected read for markers {tuple(markers)!r}"
        snapshot = self.reads.pop(0)
        assert any(marker in snapshot for marker in markers)
        return snapshot

    def drain_for(self, duration_s: float, *, label: str) -> None:
        self.drains.append((duration_s, label))


def test_saved_wifi_uses_old_root_menu_option_one() -> None:
    """Saved WiFi proof must use the old continue-with-existing-config path."""

    controller = FakeController()

    pi4_serial_reboot.select_lane(controller, "wifi", ROOT_MENU_SAVED)

    assert controller.sent == ["1"]
    assert controller.reinforced == [False]


def test_serial_commands_use_cr_line_ending() -> None:
    """Root serial diagnostics require CR; LF-only input can leave commands buffered."""

    assert pi4_serial_reboot.serial_line_bytes("netstats") == b"netstats\r"
    assert pi4_serial_reboot.serial_line_bytes("wifi diag") == b"wifi diag\r"


def test_diagnostics_reinforce_root_command_terminators() -> None:
    """Root diagnostics resend a guarded CR so commands cannot remain buffered."""

    controller = FakeController(
        [
            b"cohesix>",
            b"OK NETSTATS\n",
            b"OK NETTEST\n",
            b"OK WIFI\n",
            b"ERR WIFI\n",
            b"OK USB\n",
            b"OK USB\n",
            b"OK SMP\n",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "wifi", prompt_ready=True)

    assert controller.sent == [
        "",
        "netstats",
        "nettest",
        "wifi diag",
        "wifi probe-ht",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.public_sent[0] == "<diagnostic-prime>"
    assert controller.reinforced == [False, True, True, True, True, True, True, True]
    assert controller.drains == [(1.0, "post-root-prompt-before-diagnostics")]


def test_diagnostics_do_not_wait_for_consumed_prompt_after_ok() -> None:
    """OK and prompt can arrive in the same read; the next command must proceed."""

    controller = FakeController(
        [
            b"cohesix>",
            b"OK NETSTATS\ncohesix>",
            b"OK NETTEST\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "",
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.reads == []
    assert controller.drains == [(1.0, "post-root-prompt-before-diagnostics")]


def test_diagnostics_require_command_specific_result_marker() -> None:
    """A stale OK from another command must not satisfy the next diagnostic."""

    controller = FakeController(
        [
            b"cohesix>",
            b"OK WIFI\ncohesix>",
        ]
    )

    with pytest.raises(AssertionError):
        pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)


def test_reboot_from_root_clears_line_and_pings_before_auth(monkeypatch: pytest.MonkeyPatch) -> None:
    """Authenticated reboot must prove a clean root shell before sending a ticket."""

    controller = FakeController(
        [
            b"cohesix>",
            b"OK PING\ncohesix>",
            b"OK ATTACH\nrole=queen\ncohesix>",
            b"U-Boot 2026\n" + ROOT_MENU_SAVED,
        ]
    )
    monkeypatch.setattr(pi4_serial_reboot, "mint_ticket", lambda *_args: "secret-ticket")

    snapshot = pi4_serial_reboot.reboot_from_root(
        controller,
        REPO_ROOT,
        pathlib.Path("cohsh"),
        pathlib.Path("config.toml"),
        30,
    )

    assert controller.sent == ["", "ping", "attach queen secret-ticket", "reboot"]
    assert controller.public_sent == ["<clear-line>", "ping", "attach queen <ticket>", "reboot"]
    assert controller.redactions == [("secret-ticket", "<queen-ticket>")]
    assert ROOT_MENU_SAVED in snapshot


def test_reboot_from_root_accepts_interleaved_attach_role_proof(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Local-seat mirroring can split OK ATTACH; role proof is enough to continue."""

    controller = FakeController(
        [
            b"cohesix>",
            b"OK PING\ncohesix>",
            b"OK ATTAC[local-seat] redraw\nH role=queen\ncohesix>",
            b"U-Boot 2026\n" + ROOT_MENU_SAVED,
        ]
    )
    monkeypatch.setattr(pi4_serial_reboot, "mint_ticket", lambda *_args: "secret-ticket")

    snapshot = pi4_serial_reboot.reboot_from_root(
        controller,
        REPO_ROOT,
        pathlib.Path("cohsh"),
        pathlib.Path("config.toml"),
        30,
    )

    assert controller.sent == ["", "ping", "attach queen secret-ticket", "reboot"]
    assert ROOT_MENU_SAVED in snapshot


def test_saved_wifi_refuses_manifest_defaults_when_policy_missing() -> None:
    """WiFi proof must not silently boot manifest defaults after policy loss."""

    controller = FakeController()

    with pytest.raises(RuntimeError, match="saved WiFi policy is not visible"):
        pi4_serial_reboot.select_lane(controller, "wifi", ROOT_MENU_DEFAULTS)

    assert controller.sent == []


def test_saved_wifi_refuses_saved_wired_policy() -> None:
    """WiFi proof must not use option 1 when the saved policy is wired."""

    controller = FakeController()

    with pytest.raises(RuntimeError, match="saved network policy is not WiFi"):
        pi4_serial_reboot.select_lane(controller, "wifi", ROOT_MENU_SAVED_WIRED)

    assert controller.sent == []


def test_menu_state_classifier_uses_latest_visible_page() -> None:
    """Stale root menu bytes must not hide a later guided setup page."""

    snapshot = ROOT_MENU_SAVED + b"\x1b[2J" + DHCP_MENU

    assert pi4_serial_reboot.classify_menu_state(snapshot) == "dhcp"


def test_initial_menu_state_reads_current_menu_before_selecting_genet() -> None:
    """Menu-start proof must not assume option 2 is safe until the current page is read."""

    controller = FakeController([ROOT_MENU_SAVED, DHCP_MENU, INTERFACE_MENU, REVIEW_WIRED_DHCP])

    pi4_serial_reboot.select_lane(controller, "genet", None)

    assert controller.sent == ["2", "1", "1", "1"]


def test_genet_uses_old_guided_menu_without_saving_policy() -> None:
    """Genet proof uses DHCP and wired choices, then boots without saving."""

    controller = FakeController([DHCP_MENU, INTERFACE_MENU, REVIEW_WIRED_DHCP])

    pi4_serial_reboot.select_lane(controller, "genet", ROOT_MENU_SAVED)

    assert controller.sent == ["2", "1", "1", "1"]


def test_wifi_backs_out_of_dhcp_submenu_to_saved_root_menu() -> None:
    """If the old menu is already in setup, WiFi returns to saved policy."""

    controller = FakeController([ROOT_MENU_SAVED])

    pi4_serial_reboot.select_lane(controller, "wifi", DHCP_MENU)

    assert controller.sent == ["3", "1"]


def test_genet_edits_wifi_review_before_booting_wired() -> None:
    """A stale WiFi review page is edited before selecting Genet DHCP."""

    controller = FakeController([DHCP_MENU, INTERFACE_MENU, REVIEW_WIRED_DHCP])

    pi4_serial_reboot.select_lane(controller, "genet", REVIEW_WIFI_DHCP)

    assert controller.sent == ["3", "1", "1", "1"]
