# Author: Lukas Bower
# Purpose: Regression tests for the Pi 4 pyserial reboot helper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_serial_reboot.py."""

from __future__ import annotations

import importlib.util
import io
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
[cohesix] Cohesix boot menu
[cohesix] Saved network settings loaded
[cohesix] IPv4: Automatic (DHCP)
[cohesix] Network: Wi-Fi
[cohesix] Wi-Fi network: Configured (name hidden)
  1. Boot with saved settings
  2. Change network settings
Select option [1]:
"""

ROOT_MENU_SAVED_WIRED = b"""
[cohesix] Cohesix boot menu
[cohesix] Saved network settings loaded
[cohesix] IPv4: Automatic (DHCP)
[cohesix] Network: Ethernet
  1. Boot with saved settings
  2. Change network settings
Select option [1]:
"""

ROOT_MENU_DEFAULTS = b"""
[cohesix] Cohesix boot menu
[cohesix] Default network settings active
  1. Boot with default settings
  2. Change network settings
Select option [1]:
"""

DHCP_MENU = b"""
[cohesix] Network setup (step 1 of 3)
[cohesix] Choose IPv4 configuration
  1. Automatic (DHCP)
  2. Manual (static IPv4)
  0. Back to boot menu
Select option [1]:
"""

INTERFACE_MENU = b"""
[cohesix] Network setup (step 2 of 3)
[cohesix] Choose network connection
  1. Ethernet (wired)
  2. Wi-Fi (wireless)
  0. Back
Select option [1]:
"""

REVIEW_WIRED_DHCP = b"""
[cohesix] Review network settings
[cohesix] IPv4: Automatic (DHCP)
[cohesix] Network: Ethernet
  1. Boot once without saving
  2. Save settings and restart
  3. Edit network settings
Select option [1]:
"""

REVIEW_WIFI_DHCP = b"""
[cohesix] Review network settings
[cohesix] IPv4: Automatic (DHCP)
[cohesix] Network: Wi-Fi
  1. Boot once without saving
  2. Save settings and restart
  3. Edit network settings
Select option [1]:
"""

WIFI_MENU = b"""
[cohesix] Network setup (step 3 of 3)
[cohesix] Choose Wi-Fi network
  1. Keep current Wi-Fi settings
  0. Back
Select option [1]:
"""

STATIC_MENU = b"""
[cohesix] Network setup: manual IPv4
  1. Enter manual IPv4 settings
  0. Back
Select option [1]:
"""

RESET_MENU = b"""
[cohesix] Reset saved settings?
  1. Confirm reset
  0. Cancel
Select option [0]:
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
        self.drain_reads: list[bytes] = []
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
        assert any(pi4_serial_reboot.serial_marker_seen(snapshot, marker) for marker in markers)
        return snapshot

    def drain_for(self, duration_s: float, *, label: str) -> bytes:
        self.drains.append((duration_s, label))
        if self.drain_reads:
            return self.drain_reads.pop(0)
        return b""


class TimeoutOnceController(FakeController):
    """Fake controller that times out once, then returns scripted reads."""

    def __init__(self, reads: list[bytes]) -> None:
        super().__init__(reads)
        self._timed_out = False

    def read_until(
        self,
        markers: Iterable[bytes],
        timeout_s: float,
        *,
        label: str,
    ) -> bytes:
        if not self._timed_out:
            del markers, timeout_s
            self._timed_out = True
            raise pi4_serial_reboot.SerialMarkerTimeout(f"timeout for {label}")
        return super().read_until(markers, timeout_s, label=label)


class NoReadyThenController(FakeController):
    """Fake controller that rejects a below-ready snapshot once, then proceeds."""

    def read_until(
        self,
        markers: Iterable[bytes],
        timeout_s: float,
        *,
        label: str,
    ) -> bytes:
        if not self.notes:
            del timeout_s
            snapshot = self.reads.pop(0)
            assert not any(
                pi4_serial_reboot.serial_marker_seen(snapshot, marker)
                for marker in markers
            )
            self.notes.append(f"below-ready snapshot rejected for {label}")
            raise pi4_serial_reboot.SerialMarkerTimeout(f"timeout for {label}")
        return super().read_until(markers, timeout_s, label=label)


def redaction_controller(
    secret: bytes,
    replacement: bytes = b"<queen-ticket>",
) -> tuple[pi4_serial_reboot.RedactingSerialController, io.BytesIO]:
    """Build a serial-free controller instance for streaming-redaction tests."""

    output = io.BytesIO()
    controller = object.__new__(pi4_serial_reboot.RedactingSerialController)
    controller._redactions = [(secret, replacement)]
    controller._redaction_carry = b""
    controller._pending_annotations = []
    controller._write_safe = output.write
    return controller, output


def multi_redaction_controller(
    redactions: list[tuple[bytes, bytes]],
) -> tuple[pi4_serial_reboot.RedactingSerialController, io.BytesIO]:
    """Build a controller with several possibly overlapping secrets."""

    controller, output = redaction_controller(*redactions[0])
    controller._redactions = redactions
    return controller, output


@pytest.mark.parametrize("split", range(len(b"secret-ticket") + 1))
def test_serial_redaction_hides_ticket_at_every_two_chunk_boundary(split: int) -> None:
    """No split point may expose any byte of a complete Queen ticket."""

    secret = b"secret-ticket"
    controller, output = redaction_controller(secret)
    controller._record(b"before:" + secret[:split])
    controller._record(secret[split:] + b":after")
    controller._flush_redaction_carry()

    assert output.getvalue() == b"before:<queen-ticket>:after"
    assert secret not in output.getvalue()


def test_serial_redaction_hides_ticket_with_single_byte_chunks() -> None:
    """Adversarial one-byte reads must remain both lossless and secret-free."""

    secret = b"secret-ticket"
    controller, output = redaction_controller(secret)
    for byte in b"prefix-" + secret + b"-suffix":
        controller._record(bytes([byte]))
    controller._flush_redaction_carry()

    assert output.getvalue() == b"prefix-<queen-ticket>-suffix"
    assert secret not in output.getvalue()


def test_serial_redaction_conceals_incomplete_secret_on_close() -> None:
    """A truncated serial echo must not publish a recognizable ticket prefix."""

    secret = b"secret-ticket"
    controller, output = redaction_controller(secret)
    controller._record(b"prefix-" + secret[:7])
    controller._flush_redaction_carry()

    assert output.getvalue() == b"prefix-<redacted-partial>"
    assert secret[:7] not in output.getvalue()


@pytest.mark.parametrize("split", range(len(b"abab") + 1))
def test_serial_redaction_handles_self_overlapping_secret(split: int) -> None:
    """A complete secret followed by its own prefix must never leak."""

    controller, output = redaction_controller(b"aba", b"<ticket>")
    payload = b"abab"
    controller._record(payload[:split])
    controller._record(payload[split:])
    controller._flush_redaction_carry()

    assert output.getvalue() == b"<ticket>b"
    assert b"aba" not in output.getvalue()


def test_serial_redaction_prefers_longest_of_overlapping_secrets() -> None:
    """A shorter configured prefix must not expose a longer secret suffix."""

    controller, output = multi_redaction_controller(
        [(b"ab", b"<short>"), (b"aba", b"<long>")]
    )
    for byte in b"xabayab":
        controller._record(bytes([byte]))
    controller._flush_redaction_carry()

    assert output.getvalue() == b"x<long>y<short>"
    assert b"aba" not in output.getvalue()


def test_host_note_cannot_bisect_streaming_serial_secret() -> None:
    """Host annotations must preserve a pending serial-secret prefix."""

    secret = b"secret-ticket"
    controller, output = redaction_controller(secret)
    controller._record(b"secret-")
    controller.note("interleave")
    controller._record(b"ticket")
    controller._flush_redaction_carry()

    assert output.getvalue() == b"<queen-ticket>\n[host] interleave\n"
    assert secret not in output.getvalue()


def test_host_note_follows_complete_serial_prompt_without_reordering() -> None:
    """Ordinary serial evidence must be written before a later host action."""

    controller, output = redaction_controller(b"secret-ticket")
    controller._record(b"OK ATTACH\ncohesix>")
    controller.note("send reboot")
    controller._flush_redaction_carry()

    assert output.getvalue() == b"OK ATTACH\ncohesix>\n[host] send reboot\n"


def test_serial_timeout_tail_conceals_truncated_secret_prefix(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Timeout diagnostics must use the same streaming-safe finalizer as logs."""

    controller, _ = redaction_controller(b"secret-ticket")

    class TimeoutSerial:
        def __init__(self) -> None:
            self.reads = [b"tail:secret-tic", b""]

        def read(self, _size: int) -> bytes:
            return self.reads.pop(0) if self.reads else b""

    controller._serial = TimeoutSerial()
    times = iter((0.0, 0.0, 2.0))
    monkeypatch.setattr(pi4_serial_reboot.time, "monotonic", lambda: next(times))

    with pytest.raises(pi4_serial_reboot.SerialMarkerTimeout) as caught:
        controller.read_until((b"never",), 1.0, label="redacted marker")

    assert "secret-tic" not in str(caught.value)
    assert "<redacted-partial>" in str(caught.value)


def test_saved_wifi_uses_old_root_menu_option_one() -> None:
    """Saved Wi-Fi proof must use the saved-settings root path."""

    controller = FakeController()

    pi4_serial_reboot.select_lane(controller, "wifi", ROOT_MENU_SAVED)

    assert controller.sent == ["1"]
    assert controller.reinforced == [False]


def test_serial_commands_use_cr_line_ending() -> None:
    """Root serial diagnostics require CR; LF-only input can leave commands buffered."""

    assert pi4_serial_reboot.serial_line_bytes("netstats") == b"netstats\r"
    assert pi4_serial_reboot.serial_line_bytes("wifi diag") == b"wifi diag\r"


def test_root_prompt_marker_accepts_tail_fragment() -> None:
    """A read can consume most of the root prompt and leave only its suffix."""

    assert pi4_serial_reboot.serial_marker_seen(b"x>", pi4_serial_reboot.ROOT_PROMPT)
    assert pi4_serial_reboot.serial_marker_seen(
        b"OK NETSTATS\nx>", pi4_serial_reboot.ROOT_PROMPT
    )
    assert not pi4_serial_reboot.serial_marker_seen(
        b">", pi4_serial_reboot.ROOT_PROMPT
    )
    assert not pi4_serial_reboot.serial_marker_seen(
        b"OK NETSTATS\n", pi4_serial_reboot.ROOT_PROMPT
    )


def test_diagnostics_reinforce_root_command_terminators() -> None:
    """Root diagnostics use guarded terminators without injecting a blank command."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            b"OK NETSTATS\ncohesix>",
            b"OK NETTEST\ncohesix>",
            b"OK WIFI\ncohesix>",
            b"ERR WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "wifi", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "wifi diag",
        "wifi probe-ht",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.public_sent[0] == "netstats"
    assert controller.reinforced == [True, True, True, True, True, True, True]
    assert controller.drains == [(8.0, "post-root-prompt-settle-before-diagnostics")]


def test_diagnostics_accept_interleaved_result_marker() -> None:
    """Async local-seat logs can split diagnostic OK/ERR tokens on serial."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            (
                b"ERR NE[local-seat] usb keyboard command-ready "
                b"action=enable-command-input\nTSTATS reason=policy\ncohesix>"
            ),
            b"OK NETTEST\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]


def test_diagnostics_accept_prompt_tail_after_result() -> None:
    """Prompt suffix fragments are enough to preserve the command boundary."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            b"OK NETSTATS\nx>",
            b"OK NETTEST\nx>",
            b"OK USB\nx>",
            b"OK USB\nx>",
            b"OK SMP\nx>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.reads == []


def test_diagnostics_accept_command_ready_seen_during_settle_drain() -> None:
    """USB command-ready can arrive during the post-prompt settle drain."""

    controller = FakeController(
        [
            b"OK NETSTATS\ncohesix>",
            b"OK NETTEST\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )
    controller.drain_reads.append(
        b"[local-seat] usb keyboard command-ready action=enable-command-input "
        b"clean_polls=2 no_reply=0 recovery_pending=no\n"
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.reads == []
    assert not any(
        "serial_only_usb_unproven_after_command_ready_timeout" in note
        for note in controller.notes
    )


def test_diagnostics_do_not_wait_for_consumed_prompt_after_ok() -> None:
    """OK and prompt can arrive in the same read; the next command must proceed."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            b"OK NETSTATS\ncohesix>",
            b"OK NETTEST\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.reads == []
    assert controller.drains == [(8.0, "post-root-prompt-settle-before-diagnostics")]


def test_diagnostics_reject_gate_eight_keyboard_markers_as_command_ready() -> None:
    """Keyboard discovery and first-byte evidence are below command-ready."""

    controller = NoReadyThenController(
        [
            (
                b"USB console ready\n"
                b"usb: runtime_next_action action=enable-command-input\n"
                b"[local-seat] runtime keyboard first-byte read=1 ascii=0x68\n"
                b"usb: runtime_gate keyboard=yes first_report=yes first_byte=yes\n"
            ),
            b"OK NETSTATS\ncohesix>",
            b"OK NETTEST\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert any(
        "diagnostics serial_only_usb_unproven_after_command_ready_timeout" in note
        for note in controller.notes
    )
    assert any(
        "diagnostics serial_only_usb_unscored command='usb diag'" in note
        for note in controller.notes
    )


def test_diagnostics_wait_for_prompt_after_result_without_prompt() -> None:
    """A result line alone must not prove the next command boundary is clean."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            b"OK NETSTATS\n",
            b"cohesix>",
            b"OK NETTEST\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.reads == []


def test_diagnostics_continue_when_command_ready_never_arrives() -> None:
    """USB readiness is a settle signal; serial diagnostics must still run."""

    controller = TimeoutOnceController(
        [
            b"OK NETSTATS\ncohesix>",
            b"OK NETTEST\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(controller, "genet", prompt_ready=True)

    assert controller.sent == [
        "netstats",
        "nettest",
        "usb diag",
        "usb probe-kbd",
        "smp activity",
    ]
    assert controller.drains == [(8.0, "post-root-prompt-settle-before-diagnostics")]
    assert any(
        "diagnostics serial_only_usb_unproven_after_command_ready_timeout" in note
        for note in controller.notes
    )
    assert any(
        "diagnostics serial_only_usb_unscored command='usb probe-kbd'" in note
        for note in controller.notes
    )


def test_diagnostics_require_command_specific_result_marker() -> None:
    """A stale OK from another command must not satisfy the next diagnostic."""

    controller = FakeController(
        [
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
            b"OK REBOOT detail=scheduled\n",
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
            b"OK REBOOT detail=scheduled\n",
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


def test_reboot_from_root_rejects_uboot_without_scheduled_ack(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """U-Boot chatter alone cannot prove that Cohesix accepted reboot."""

    controller = FakeController(
        [
            b"cohesix>",
            b"OK PING\ncohesix>",
            b"OK ATTACH role=queen\ncohesix>",
            b"U-Boot 2026\n" + ROOT_MENU_SAVED,
        ]
    )
    monkeypatch.setattr(pi4_serial_reboot, "mint_ticket", lambda *_args: "secret-ticket")

    with pytest.raises(AssertionError):
        pi4_serial_reboot.reboot_from_root(
            controller,
            REPO_ROOT,
            pathlib.Path("cohsh"),
            pathlib.Path("config.toml"),
            30,
        )


def test_saved_wifi_refuses_default_settings_when_policy_missing() -> None:
    """Wi-Fi proof must not silently boot defaults after policy loss."""

    controller = FakeController()

    with pytest.raises(RuntimeError, match="saved Wi-Fi policy is not visible"):
        pi4_serial_reboot.select_lane(controller, "wifi", ROOT_MENU_DEFAULTS)

    assert controller.sent == []


def test_saved_wifi_refuses_saved_wired_policy() -> None:
    """Wi-Fi proof must not use option 1 when the saved policy is wired."""

    controller = FakeController()

    with pytest.raises(RuntimeError, match="saved network policy is not Wi-Fi"):
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


def test_genet_uses_guided_menu_without_saving_policy() -> None:
    """Ethernet proof uses DHCP and wired choices, then boots without saving."""

    controller = FakeController([DHCP_MENU, INTERFACE_MENU, REVIEW_WIRED_DHCP])

    pi4_serial_reboot.select_lane(controller, "genet", ROOT_MENU_SAVED)

    assert controller.sent == ["2", "1", "1", "1"]


def test_wifi_backs_out_of_dhcp_submenu_to_saved_root_menu() -> None:
    """If the menu is already in setup, Wi-Fi returns to saved policy."""

    controller = FakeController([ROOT_MENU_SAVED])

    pi4_serial_reboot.select_lane(controller, "wifi", DHCP_MENU)

    assert controller.sent == ["0", "1"]


def test_genet_edits_wifi_review_before_booting_wired() -> None:
    """A stale Wi-Fi review page is edited before selecting Ethernet DHCP."""

    controller = FakeController([DHCP_MENU, INTERFACE_MENU, REVIEW_WIRED_DHCP])

    pi4_serial_reboot.select_lane(controller, "genet", REVIEW_WIFI_DHCP)

    assert controller.sent == ["3", "1", "1", "1"]


def test_genet_reselects_dhcp_from_an_existing_interface_page() -> None:
    """An interface page may belong to manual mode, so DHCP is reselected."""

    controller = FakeController([DHCP_MENU, INTERFACE_MENU, REVIEW_WIRED_DHCP])

    pi4_serial_reboot.select_lane(controller, "genet", INTERFACE_MENU)

    assert controller.sent == ["0", "1", "1", "1"]


@pytest.mark.parametrize(
    ("snapshot", "expected"),
    (
        (WIFI_MENU, "wifi-setup"),
        (STATIC_MENU, "static-setup"),
        (RESET_MENU, "reset"),
    ),
)
def test_menu_state_classifier_covers_every_submenu(
    snapshot: bytes, expected: str
) -> None:
    """Serial automation recognizes every interactive menu page."""

    assert pi4_serial_reboot.classify_menu_state(snapshot) == expected


def test_wifi_backs_out_of_reset_confirmation_to_saved_root_menu() -> None:
    """Cancel uses the same zero-key convention before a saved Wi-Fi boot."""

    controller = FakeController([ROOT_MENU_SAVED])

    pi4_serial_reboot.select_lane(controller, "wifi", RESET_MENU)

    assert controller.sent == ["0", "1"]


@pytest.mark.parametrize("snapshot", (WIFI_MENU, STATIC_MENU))
def test_wifi_backs_out_of_nested_setup_pages_to_saved_root_menu(
    snapshot: bytes,
) -> None:
    """Nested pages follow zero-key Back until the saved root state is reached."""

    controller = FakeController([INTERFACE_MENU, DHCP_MENU, ROOT_MENU_SAVED])

    pi4_serial_reboot.select_lane(controller, "wifi", snapshot)

    assert controller.sent == ["0", "0", "0", "1"]
