# Author: Lukas Bower
# Purpose: Regression tests for the Pi 4 pyserial reboot helper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_serial_reboot.py."""

from __future__ import annotations

import importlib.util
import io
import pathlib
from collections.abc import Iterable
from types import SimpleNamespace

import pytest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4_serial_reboot.py"
EXPECTED_GIT_COMMIT = "1" * 40
EXPECTED_IMAGE_ID = "2" * 64
EXPECTED_BUILD_ID = "3" * 64
EXPECTED_BUILD_MARKER_SHA256 = "4" * 64
EXPECTED_BUILD_MARKER = (
    f"[BUILD] {EXPECTED_GIT_COMMIT} 2026-08-28T00:00:00Z "
    f"image-id={EXPECTED_IMAGE_ID} "
    "features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 "
    "net-console:1 qemu-driver-task-smoke:0]"
)
SMP_ACTIVITY_RATE_LINE = (
    b"[smp] activity rates window_ms=17000 cpu_pct=unavailable "
    b"view=counter-delta task_allocation=multi\n"
)

SPEC = importlib.util.spec_from_file_location("pi4_serial_reboot", SCRIPT_PATH)
assert SPEC is not None
pi4_serial_reboot = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(pi4_serial_reboot)
REAL_START_NETTEST_TCP_PEER = pi4_serial_reboot.start_nettest_tcp_peer


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

NETTEST_RUN_GENERATION = 31
NETTEST_STARTED = (
    b"OK NETTEST detail=started run_generation=31\ncohesix>"
)
NETTEST_RESULT = (
    b"[net-selftest] result generation=14 run_generation=31 "
    b"tx_ok=true udp_echo_ok=true tcp_ok=true console_ok=true "
    b"peer_assisted_ok=false result=pass\n"
)
NETTEST_FAILURE_RESULT = (
    b"[net-selftest] result generation=15 run_generation=31 "
    b"tx_ok=true udp_echo_ok=false tcp_ok=false console_ok=true "
    b"peer_assisted_ok=false result=fail\n"
)
NETTEST_PEER_ASSISTED_RESULT = (
    b"[net-selftest] result generation=14 run_generation=31 "
    b"tx_ok=true udp_echo_ok=false tcp_ok=false console_ok=true "
    b"peer_assisted_ok=true result=peer-assisted-pass\n"
)
NETSTATS_OK = b"OK NETSTATS\ncohesix>"
NETSTATS_WIFI_PENDING = (
    b"netstats: generation=4 mode=dhcp policy=wifi active=wifi standby=none "
    b"addr_src=dhcp-pending ip=0.0.0.0 gateway=0.0.0.0 dhcp=selecting\n"
    + NETSTATS_OK
)
NETSTATS_WIFI_BOUND = (
    b"netstats: generation=4 mode=dhcp policy=wifi active=wifi standby=none "
    b"addr_src=dhcp-lease ip=192.168.86.154 gateway=192.168.86.1 dhcp=bound\n"
    + NETSTATS_OK
)
NETSTATS_GENET_BOUND = (
    b"netstats: generation=4 mode=dhcp policy=wired active=wired standby=none "
    b"addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound\n"
    + NETSTATS_OK
)
NETTEST_STATUS_PASS = (
    b"nettest: generation=14 run_generation=31 enabled=true running=false "
    b"verdict=pass tx_ok=true udp_echo_ok=true tcp_ok=true "
    b"console_ok=true peer_assisted_ok=false\n"
)
NETTEST_STATUS_FAILURE = (
    b"nettest: generation=15 run_generation=31 enabled=true running=false "
    b"verdict=fail tx_ok=true udp_echo_ok=false tcp_ok=false "
    b"console_ok=true peer_assisted_ok=false\n"
)
NETTEST_STATUS_PEER_ASSISTED = (
    b"nettest: generation=14 run_generation=31 enabled=true running=false "
    b"verdict=peer-assisted-pass tx_ok=true udp_echo_ok=false tcp_ok=false "
    b"console_ok=true peer_assisted_ok=true\n"
)
NETSTATS_TERMINAL_PASS = NETTEST_STATUS_PASS + NETSTATS_OK
NETSTATS_TERMINAL_FAILURE = NETTEST_STATUS_FAILURE + NETSTATS_OK
NETSTATS_TERMINAL_PEER_ASSISTED = NETTEST_STATUS_PEER_ASSISTED + NETSTATS_OK
TEST_NETTEST_PEER = pi4_serial_reboot.NettestPeerConfig(
    repo=pathlib.Path("/test/repo"),
    cohsh=pathlib.Path("/test/repo/cohsh"),
    script=pathlib.Path("/test/repo/scripts/cohsh/boot_v0.coh"),
    auth_token="test-console-secret",
)


class FakePeerProcess:
    """Bounded successful nettest peer child used by diagnostic tests."""

    def __init__(self, return_code: int = 0) -> None:
        self.return_code = return_code
        self.terminated = False
        self.killed = False

    def wait(self, *, timeout: float) -> int:
        del timeout
        return self.return_code

    def poll(self) -> int | None:
        return self.return_code

    def terminate(self) -> None:
        self.terminated = True

    def kill(self) -> None:
        self.killed = True


@pytest.fixture(autouse=True)
def stub_nettest_peer(
    monkeypatch: pytest.MonkeyPatch,
) -> list[tuple[pi4_serial_reboot.NettestPeerConfig, str]]:
    """Keep serial-state tests hermetic while recording each peer launch."""

    launches: list[tuple[pi4_serial_reboot.NettestPeerConfig, str]] = []

    def start(
        config: pi4_serial_reboot.NettestPeerConfig,
        target_ip: str,
        lane: str,
    ) -> FakePeerProcess:
        assert lane in {"wifi", "genet"}
        launches.append((config, target_ip))
        return FakePeerProcess()

    monkeypatch.setattr(pi4_serial_reboot, "start_nettest_tcp_peer", start)
    return launches


def wifi_supervisor_record(
    status: str,
    *,
    attempt: int = 1,
    console_seq: int = 5,
    next_attempt_ms: int | None = None,
) -> bytes:
    """Build one current production CYW43 supervisor wire record."""

    if next_attempt_ms is None:
        next_attempt_ms = (
            pi4_serial_reboot.U64_MAX if status == "failed" else 12345
        )
    return (
        "CYW43_BOOTSTRAP_SUPERVISOR "
        f"attempt={attempt} status={status} "
        f"backoff_ms=0 next_attempt_ms={next_attempt_ms} "
        "serial=ready local_seat=enabled recovery=full "
        f"console_seq={console_seq} "
        "telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes\n"
    ).encode()


class FakeController:
    """Small serial-controller test double for menu-selection logic."""

    def __init__(self, reads: Iterable[bytes] = ()) -> None:
        self.reads = list(reads)
        self.sent: list[str] = []
        self.public_sent: list[str] = []
        self.reinforced: list[bool] = []
        self.root_terminator_guards: list[bool] = []
        self.notes: list[str] = []
        self.drains: list[tuple[float, str]] = []
        self.drain_reads: list[bytes] = []
        self.redactions: list[tuple[str, str]] = []
        self.diagnostic_barriers: list[str] = []
        self.diagnostic_deadlines: list[float | None] = []

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
        guard_root_terminator: bool = False,
    ) -> None:
        self.sent.append(line)
        self.public_sent.append(public_line if public_line is not None else line)
        self.reinforced.append(reinforce_terminator)
        self.root_terminator_guards.append(guard_root_terminator)

    def read_until(
        self,
        markers: Iterable[bytes],
        timeout_s: float,
        *,
        label: str,
        stream_prefix: bytes = b"",
    ) -> bytes:
        del timeout_s
        assert self.reads, f"unexpected read for markers {tuple(markers)!r}"
        snapshot = stream_prefix + self.reads.pop(0)
        assert any(pi4_serial_reboot.serial_marker_seen(snapshot, marker) for marker in markers)
        if (
            label == "result for smp activity"
            and b"[smp] activity rates" not in snapshot
        ):
            snapshot = SMP_ACTIVITY_RATE_LINE + snapshot
        return snapshot

    def drain_for(self, duration_s: float, *, label: str) -> bytes:
        self.drains.append((duration_s, label))
        if self.drain_reads:
            return self.drain_reads.pop(0)
        if (
            label == "nettest terminal observation window"
            and self.reads
            and b"[net-selftest] result generation=" in self.reads[0]
        ):
            return self.reads.pop(0)
        return b""

    def synchronize_root_diagnostic_command(
        self,
        *,
        label: str,
        deadline: float | None = None,
    ) -> None:
        self.diagnostic_barriers.append(label)
        self.diagnostic_deadlines.append(deadline)


def expected_image_identity(*, clean: bool = True) -> SimpleNamespace:
    """Return the exact nonsecret identity fields consumed by the helper."""

    return SimpleNamespace(
        source_tree_clean=clean,
        git_commit=EXPECTED_GIT_COMMIT,
        build_id=EXPECTED_BUILD_ID,
        image_id=EXPECTED_IMAGE_ID,
        build_marker=EXPECTED_BUILD_MARKER,
        build_marker_sha256=EXPECTED_BUILD_MARKER_SHA256,
    )


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


def test_serial_read_matches_prompt_across_prior_stream_tail(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A marker can remain contiguous while crossing two bounded waits."""

    controller, _ = redaction_controller(b"secret-ticket")

    class PromptSerial:
        def read(self, _size: int) -> bytes:
            return b"ohesix> "

    controller._serial = PromptSerial()
    times = iter((0.0, 0.0))
    monkeypatch.setattr(pi4_serial_reboot.time, "monotonic", lambda: next(times))

    snapshot = controller.read_until(
        (pi4_serial_reboot.ROOT_PROMPT_FULL,),
        1.0,
        label="split prompt",
        stream_prefix=b"c",
    )

    assert snapshot == pi4_serial_reboot.ROOT_PROMPT_FULL


def test_serial_read_rejects_noncontiguous_prompt_tail(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Unrelated bytes between waits cannot complete the root prompt."""

    controller, _ = redaction_controller(b"secret-ticket")

    class InterruptedPromptSerial:
        def __init__(self) -> None:
            self.reads = [b"async\nohesix> ", b""]

        def read(self, _size: int) -> bytes:
            return self.reads.pop(0) if self.reads else b""

    controller._serial = InterruptedPromptSerial()
    times = iter((0.0, 0.0, 2.0))
    monkeypatch.setattr(pi4_serial_reboot.time, "monotonic", lambda: next(times))

    with pytest.raises(pi4_serial_reboot.SerialMarkerTimeout):
        controller.read_until(
            (pi4_serial_reboot.ROOT_PROMPT_FULL,),
            1.0,
            label="interrupted split prompt",
            stream_prefix=b"c",
        )


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
    assert (
        pi4_serial_reboot.serial_line_bytes(
            "netstats",
            guard_root_terminator=True,
        )
        == b"netstats \r"
    )
    assert (
        pi4_serial_reboot.serial_line_bytes(
            "",
            guard_root_terminator=True,
        )
        == b"\r"
    )


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


def test_wifi_supervisor_parser_accepts_only_terminal_records() -> None:
    """Progress records cannot admit commands before Wi-Fi bootstrap is stable."""

    progress = (
        b"CYW43_BOOTSTRAP_SUPERVISOR attempt=0 status=preflight\n"
        + wifi_supervisor_record("begin", console_seq=3)
        + wifi_supervisor_record("stabilizing", console_seq=4)
    )

    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(progress) is None
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        progress + wifi_supervisor_record("ready")
    ) == (1, "ready")
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("ready").removesuffix(b"\n") + b"\r"
    ) == (1, "ready")
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        b"CYW43_BOOTSTRAP_SUPERVISOR attempt=0 status=preflight\r\r\n"
        + wifi_supervisor_record("ready")
    ) == (1, "ready")
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("failed")
    ) == (1, "failed")
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("permanent")
    ) == (1, "permanent")
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("ready", attempt=2)
    ) is None
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        b"CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready\n"
    ) is None
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("ready").removesuffix(b"\n")
    ) is None
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("ready").removesuffix(b"\n") + b" garbage\n"
    ) is None
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("ready") + wifi_supervisor_record("ready")
    ) is None
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        wifi_supervisor_record("ready") + wifi_supervisor_record("permanent")
    ) is None

    _, later_attempt_error = pi4_serial_reboot.inspect_wifi_supervisor_evidence(
        wifi_supervisor_record("ready") + wifi_supervisor_record("ready", attempt=2)
    )
    assert later_attempt_error == "attempt-2-forbidden"
    retracted_terminal, contradiction_error = (
        pi4_serial_reboot.inspect_wifi_supervisor_evidence(
            wifi_supervisor_record("ready") + wifi_supervisor_record("permanent")
        )
    )
    assert retracted_terminal is None
    assert contradiction_error == "terminal-contradiction"
    trailing_retry = (
        wifi_supervisor_record("ready")
        + b"CYW43_BOOTSTRAP_SUPERVISOR attempt=2 status=ready"
    )
    assert pi4_serial_reboot.inspect_wifi_supervisor_evidence(trailing_retry) == (
        (1, "ready"),
        None,
    )
    _, trailing_retry_error = pi4_serial_reboot.inspect_wifi_supervisor_evidence(
        trailing_retry,
        reject_trailing_partial=True,
    )
    assert trailing_retry_error == "attempt-2-forbidden"


def test_wifi_supervisor_parser_treats_runtime_recovery_as_proof_failure() -> None:
    """Post-Ready recovery fails the sample and bootstrap Ready stays unique."""

    retracted = (
        wifi_supervisor_record("begin", console_seq=3)
        + wifi_supervisor_record("stabilizing", console_seq=4)
        + wifi_supervisor_record("ready", console_seq=5)
        + wifi_supervisor_record("recovery", console_seq=6)
        + wifi_supervisor_record("stabilizing", console_seq=7)
    )

    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(retracted) == (
        1,
        "recovery",
    )
    _, republished_ready_error = pi4_serial_reboot.inspect_wifi_supervisor_evidence(
        retracted + wifi_supervisor_record("ready", console_seq=8)
    )
    assert republished_ready_error == "bootstrap-ready-republication-forbidden"
    assert pi4_serial_reboot.parse_wifi_supervisor_terminal(
        retracted + wifi_supervisor_record("permanent", console_seq=8)
    ) == (1, "permanent")

    _, repeated_recovery_error = pi4_serial_reboot.inspect_wifi_supervisor_evidence(
        retracted
        + wifi_supervisor_record("recovery", console_seq=8)
        + wifi_supervisor_record("stabilizing", console_seq=9)
    )
    assert repeated_recovery_error == "recovery-limit-exceeded"


def test_wifi_diagnostics_wait_for_supervisor_and_dhcp_before_nettest(
    stub_nettest_peer: list[
        tuple[pi4_serial_reboot.NettestPeerConfig, str]
    ],
) -> None:
    """Wi-Fi commands start after bootstrap and nettest starts after a lease."""

    class OrderedController(FakeController):
        def __init__(self, reads: Iterable[bytes]) -> None:
            super().__init__(reads)
            self.events: list[str] = []

        def read_until(
            self,
            markers: Iterable[bytes],
            timeout_s: float,
            *,
            label: str,
        ) -> bytes:
            self.events.append(f"read:{label}")
            return super().read_until(markers, timeout_s, label=label)

        def synchronize_root_diagnostic_command(
            self,
            *,
            label: str,
            deadline: float | None = None,
        ) -> None:
            self.events.append(f"barrier:{label}")
            super().synchronize_root_diagnostic_command(
                label=label,
                deadline=deadline,
            )

    controller = OrderedController(
        [
            b"\nCYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready",
            (
                b" backoff_ms=0 next_attempt_ms=12345 serial=ready "
                b"local_seat=enabled recovery=full console_seq=5 "
                b"telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes\n"
            ),
            (
                b"[local-seat] usb keyboard command-ready "
                b"action=enable-command-input\n"
            ),
            NETSTATS_WIFI_PENDING,
            NETSTATS_WIFI_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK WIFI\ncohesix>",
            b"OK WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "wifi",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
        boot_snapshot=b"[BUILD]\ncohesix> ",
    )

    assert diagnostics_ok
    assert controller.sent[:4] == [
        "netstats",
        "netstats",
        "smp activity",
        "nettest",
    ]
    assert controller.diagnostic_barriers[:4] == [
        "netstats",
        "netstats-dhcp-poll-2",
        "smp activity-prefix",
        "nettest",
    ]
    assert controller.events.index(
        "read:terminal CYW43 bootstrap supervisor status"
    ) < controller.events.index("barrier:netstats")
    assert controller.events.index(
        "read:complete CYW43 bootstrap supervisor line"
    ) < controller.events.index("barrier:netstats")
    assert (
        "wifi DHCP poll poll=1 generation=4 mode=dhcp policy=wifi "
        "active=wifi address_source=dhcp-pending ip=0.0.0.0 "
        "dhcp=selecting terminal=no"
    ) in controller.notes
    assert (
        "wifi DHCP poll poll=2 generation=4 mode=dhcp policy=wifi "
        "active=wifi address_source=dhcp-lease ip=192.168.86.154 "
        "dhcp=bound terminal=bound"
    ) in controller.notes
    assert (
        pi4_serial_reboot.WIFI_DHCP_POLL_INTERVAL_S,
        "wifi DHCP progress before poll 2",
    ) in controller.drains
    assert (
        pi4_serial_reboot.WIFI_READY_STABILITY_WINDOW_S,
        "post-ready stable-lifetime observation",
    ) in controller.drains
    assert any(
        "wifi supervisor stable lifetime" in note
        and "action=diagnostics-admitted" in note
        for note in controller.notes
    )
    assert stub_nettest_peer == [(TEST_NETTEST_PEER, "192.168.86.154")]


def test_wifi_failed_supervisor_fails_closed_without_nettest() -> None:
    """A terminal bootstrap failure cannot inherit an older nettest verdict."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            b"OK SMP\ncohesix>",
            NETSTATS_OK,
            NETSTATS_TERMINAL_PASS,
            b"OK WIFI\ncohesix>",
            b"OK WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "wifi",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
        boot_snapshot=wifi_supervisor_record("failed") + b"cohesix> ",
    )

    assert not diagnostics_ok
    assert "nettest" not in controller.sent
    assert controller.sent[:3] == ["smp activity", "netstats", "netstats"]
    assert "wifi-supervisor:failed" in controller.notes[-1]
    assert any(
        "action=skip-unavailable-nettest" in note for note in controller.notes
    )


def test_wifi_ready_observation_rejects_later_attempt_before_any_command() -> None:
    """A forbidden retry observed after Ready cannot collide with diagnostics."""

    controller = FakeController(
        [
            NETSTATS_OK,
            NETSTATS_OK,
            b"OK WIFI\ncohesix>",
            b"OK WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )
    controller.drain_reads.append(
        b"\n"
        + b"CYW43_BOOTSTRAP_SUPERVISOR attempt=2 status=ready"
    )

    with pytest.raises(RuntimeError, match="attempt-2-forbidden"):
        pi4_serial_reboot.run_diagnostics(
            controller,
            "wifi",
            nettest_peer=TEST_NETTEST_PEER,
            prompt_ready=True,
            boot_snapshot=(
                wifi_supervisor_record("ready")
                + b"[local-seat] usb keyboard command-ready "
                b"action=enable-command-input\ncohesix> "
            ),
        )

    assert controller.sent == []
    assert controller.diagnostic_barriers == []
    assert controller.drains == [
        (
            pi4_serial_reboot.WIFI_READY_STABILITY_WINDOW_S,
            "post-ready stable-lifetime observation",
        )
    ]


def test_wifi_ready_retraction_waits_for_permanent_before_diagnostics() -> None:
    """R01-style Ready/recovery/permanent stays passive until quarantine."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            b"OK SMP\ncohesix>",
            NETSTATS_OK,
            NETSTATS_OK,
            b"OK WIFI\ncohesix>",
            b"OK WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )
    controller.drain_reads.append(
        wifi_supervisor_record("recovery", console_seq=6)
        + wifi_supervisor_record("stabilizing", console_seq=7)
        + wifi_supervisor_record("permanent", console_seq=8)
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "wifi",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
        boot_snapshot=wifi_supervisor_record("ready", console_seq=5),
    )

    assert not diagnostics_ok
    assert "nettest" not in controller.sent
    assert controller.sent == [
        "smp activity",
        "netstats",
        "netstats",
        "wifi dump-state",
        "wifi diag",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.drains[0] == (
        pi4_serial_reboot.WIFI_READY_STABILITY_WINDOW_S,
        "post-ready stable-lifetime observation",
    )
    assert any(
        "status=permanent" in note and "action=diagnostics-admitted" in note
        for note in controller.notes
    )
    assert "wifi-supervisor:permanent" in controller.notes[-1]


def test_wifi_post_ready_recovery_stops_passive_wait_immediately() -> None:
    """Runtime recovery fails repeatability without waiting for a later terminal."""

    controller = FakeController(
        [wifi_supervisor_record("permanent", console_seq=8)]
    )
    controller.drain_reads.append(
        wifi_supervisor_record("recovery", console_seq=6)
        + wifi_supervisor_record("stabilizing", console_seq=7)
    )

    status, observed = pi4_serial_reboot.wait_for_wifi_supervisor_terminal(
        controller,
        wifi_supervisor_record("ready", console_seq=5),
        240.0,
    )

    assert status == "recovery"
    assert wifi_supervisor_record("recovery", console_seq=6) in observed
    assert wifi_supervisor_record("permanent", console_seq=8) not in observed
    assert controller.sent == []
    assert controller.diagnostic_barriers == []
    assert any(
        "wifi supervisor terminal" in note
        and "status=recovery" in note
        and "action=diagnostics-admitted" in note
        for note in controller.notes
    )


def test_wifi_republished_bootstrap_ready_is_rejected() -> None:
    """Runtime restoration cannot publish a second bootstrap Ready record."""

    controller = FakeController(
        [
            NETSTATS_WIFI_BOUND,
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )
    controller.drain_reads.extend(
        [
            (
                wifi_supervisor_record("recovery", console_seq=6)
                + wifi_supervisor_record("stabilizing", console_seq=7)
                + wifi_supervisor_record("ready", console_seq=8)
            ),
            b"",
        ]
    )

    with pytest.raises(
        RuntimeError,
        match="bootstrap-ready-republication-forbidden",
    ):
        pi4_serial_reboot.run_diagnostics(
            controller,
            "wifi",
            nettest_peer=TEST_NETTEST_PEER,
            prompt_ready=False,
            boot_snapshot=wifi_supervisor_record("ready", console_seq=5),
        )

    assert controller.drains == [
        (
            pi4_serial_reboot.WIFI_READY_STABILITY_WINDOW_S,
            "post-ready stable-lifetime observation",
        )
    ]
    assert controller.sent == []


def test_wifi_dhcp_timeout_preserves_later_diagnostics() -> None:
    """The absolute DHCP deadline fails closed without losing evidence."""

    class DhcpResultTimeoutController(FakeController):
        def __init__(self, reads: Iterable[bytes]) -> None:
            super().__init__(reads)
            self.dhcp_result_timeout_s: float | None = None

        def read_until(
            self,
            markers: Iterable[bytes],
            timeout_s: float,
            *,
            label: str,
        ) -> bytes:
            if label == "result for netstats" and self.dhcp_result_timeout_s is None:
                self.dhcp_result_timeout_s = timeout_s
                raise pi4_serial_reboot.SerialMarkerTimeout(
                    "bounded DHCP result timeout"
                )
            return super().read_until(markers, timeout_s, label=label)

    controller = DhcpResultTimeoutController(
        [
            b"OK SMP\ncohesix>",
            NETSTATS_OK,
            b"OK WIFI\ncohesix>",
            b"OK WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "wifi",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=False,
        boot_snapshot=wifi_supervisor_record("ready"),
        wifi_dhcp_timeout_s=0.5,
    )

    assert not diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "netstats",
        "wifi dump-state",
        "wifi diag",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.dhcp_result_timeout_s is not None
    assert 0 < controller.dhcp_result_timeout_s <= 0.5
    assert controller.diagnostic_deadlines[0] is not None
    assert controller.diagnostic_deadlines[1:] == [
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ]
    assert any(
        "wifi DHCP terminal result=timeout" in note
        and "action=skip-premature-nettest" in note
        for note in controller.notes
    )
    assert "wifi-dhcp:terminal-timeout" in controller.notes[-1]


def test_diagnostic_barrier_rejects_prompt_tail_after_ping() -> None:
    """A split prompt after ping cannot authorize the diagnostic command."""

    controller = FakeController(
        [
            b"cohesix> stale\nOK PING reply=pong\nx>",
            b"cohesix> ",
        ]
    )

    pi4_serial_reboot.RedactingSerialController.synchronize_root_diagnostic_command(
        controller,
        label="netstats",
    )

    assert controller.sent == ["", "ping"]
    assert controller.public_sent == ["<clear-line>", "ping"]
    assert controller.root_terminator_guards == [False, True]
    assert controller.reads == []
    assert controller.drains == [
        (
            pi4_serial_reboot.DIAGNOSTIC_COMMAND_DRAIN_S,
            "stale serial output before netstats",
        )
    ]


def test_diagnostic_barrier_accepts_full_prompt_following_ping() -> None:
    """A complete prompt in the ping response proves the next command boundary."""

    controller = FakeController([b"OK PING reply=pong\ncohesix> "])

    pi4_serial_reboot.RedactingSerialController.synchronize_root_diagnostic_command(
        controller,
        label="wifi diag",
    )

    assert controller.sent == ["", "ping"]
    assert controller.root_terminator_guards == [False, True]
    assert controller.reads == []


def test_diagnostic_barrier_accepts_prompt_split_after_first_byte() -> None:
    """The ping read tail and next read form one contiguous fresh prompt."""

    controller = FakeController(
        [
            b"OK PING reply=pong\nc",
            b"ohesix> ",
        ]
    )

    pi4_serial_reboot.RedactingSerialController.synchronize_root_diagnostic_command(
        controller,
        label="wifi diag",
    )

    assert controller.sent == ["", "ping"]
    assert controller.root_terminator_guards == [False, True]
    assert controller.reads == []


def test_diagnostics_reinforce_root_command_terminators() -> None:
    """Root diagnostics use guarded terminators without injecting a blank command."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            NETSTATS_WIFI_BOUND,
            b"OK SMP\ncohesix>",
            (
                NETTEST_RESULT.replace(b"generation=14", b"generation=13")
                + NETTEST_STARTED
            ),
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK WIFI\ncohesix>",
            b"OK WIFI\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "wifi",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
        boot_snapshot=wifi_supervisor_record("ready") + b"cohesix> ",
    )

    assert diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "wifi dump-state",
        "wifi diag",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.public_sent[0] == "netstats"
    assert controller.reinforced == [
        True,
        True,
        True,
        True,
        True,
        True,
        True,
        True,
        True,
    ]
    assert controller.diagnostic_barriers == [
        "netstats",
        "smp activity-prefix",
        "nettest",
        "netstats-final",
        "wifi dump-state",
        "wifi diag",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert (
        "nettest async-terminal observed generation=14 "
        "run_generation=31 result=pass "
        "action=confirm-with-netstats"
    ) in controller.notes
    assert (
        "nettest terminal generation=14 run_generation=31 "
        "running=false result=pass source=netstats"
    ) in controller.notes
    assert "wifi probe-ht" not in controller.sent
    assert "diagnostics complete result=pass" in controller.notes
    assert controller.drains == [
        (
            pi4_serial_reboot.WIFI_READY_STABILITY_WINDOW_S,
            "post-ready stable-lifetime observation",
        ),
        (8.0, "post-root-prompt-settle-before-diagnostics"),
        (
            pi4_serial_reboot.NETTEST_OBSERVATION_S,
            "nettest terminal observation window",
        ),
    ]


def test_diagnostics_accept_interleaved_result_marker() -> None:
    """Async local-seat logs can split diagnostic OK/ERR tokens on serial."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            (
                b"ERR NE[local-seat] usb keyboard command-ready "
                b"action=enable-command-input\nTSTATS reason=policy\ncohesix>"
            ),
            b"OK SMP\ncohesix>",
            NETSTATS_OK,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert not diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert (
        "diagnostics complete result=fail "
        "failures=netstats:err,nettest:peer-target-unavailable"
        in controller.notes
    )


def test_diagnostics_accept_prompt_tail_after_result() -> None:
    """Prompt suffix fragments are enough to preserve the command boundary."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            NETSTATS_GENET_BOUND.replace(b"cohesix>", b"x>"),
            b"OK SMP\nx>",
            b"OK NETTEST detail=started run_generation=31\nx>",
            NETTEST_RESULT,
            NETTEST_STATUS_PASS + b"OK NETSTATS\nx>",
            b"OK USB\nx>",
            b"OK USB\nx>",
            b"OK SMP\nx>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.diagnostic_deadlines == [
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ]
    assert controller.reads == []


def test_diagnostics_run_active_usb_probe_only_when_requested() -> None:
    """The potentially mutating keyboard probe is an explicit opt-in."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_PEER_ASSISTED_RESULT,
            NETSTATS_TERMINAL_PEER_ASSISTED,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
        active_usb_probe=True,
    )

    assert diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "usb probe-kbd",
        "smp activity",
    ]


def test_diagnostics_accept_command_ready_seen_during_settle_drain() -> None:
    """USB command-ready can arrive during the post-prompt settle drain."""

    controller = FakeController(
        [
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )
    controller.drain_reads.append(
        b"[local-seat] usb keyboard command-ready action=enable-command-input "
        b"clean_polls=2 no_reply=0 recovery_pending=no\n"
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
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
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.reads == []
    assert controller.drains == [
        (8.0, "post-root-prompt-settle-before-diagnostics"),
        (
            pi4_serial_reboot.NETTEST_OBSERVATION_S,
            "nettest terminal observation window",
        ),
    ]


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
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
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


def test_diagnostics_barrier_replaces_prompt_wait_after_result() -> None:
    """Every next command gets a fresh barrier even when a result has no prompt."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no\n",
            NETSTATS_GENET_BOUND.replace(b"cohesix>", b""),
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.diagnostic_barriers == [
        "netstats",
        "smp activity-prefix",
        "nettest",
        "netstats-final",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.reads == []


def test_diagnostics_continue_when_command_ready_never_arrives() -> None:
    """USB readiness is a settle signal; serial diagnostics must still run."""

    controller = TimeoutOnceController(
        [
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.drains == [
        (8.0, "post-root-prompt-settle-before-diagnostics"),
        (
            pi4_serial_reboot.NETTEST_OBSERVATION_S,
            "nettest terminal observation window",
        ),
    ]
    assert any(
        "diagnostics serial_only_usb_unproven_after_command_ready_timeout" in note
        for note in controller.notes
    )
    assert any(
        "diagnostics serial_only_usb_unscored command='usb status'" in note
        for note in controller.notes
    )


def test_genet_nettest_peer_launches_after_exact_admission_before_observation(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The authenticated peer runs only inside the admitted self-test window."""

    class OrderedController(FakeController):
        def __init__(self, reads: Iterable[bytes]) -> None:
            super().__init__(reads)
            self.events: list[str] = []

        def read_until(
            self,
            markers: Iterable[bytes],
            timeout_s: float,
            *,
            label: str,
            stream_prefix: bytes = b"",
        ) -> bytes:
            self.events.append(f"read:{label}")
            return super().read_until(
                markers,
                timeout_s,
                label=label,
                stream_prefix=stream_prefix,
            )

        def drain_for(self, duration_s: float, *, label: str) -> bytes:
            self.events.append(f"drain:{label}")
            return super().drain_for(duration_s, label=label)

    controller = OrderedController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    def start_peer(
        config: pi4_serial_reboot.NettestPeerConfig,
        target_ip: str,
        lane: str,
    ) -> FakePeerProcess:
        assert config is TEST_NETTEST_PEER
        assert target_ip == "192.168.10.50"
        assert lane == "genet"
        controller.events.append("peer:start:run-generation-31")
        return FakePeerProcess()

    monkeypatch.setattr(pi4_serial_reboot, "start_nettest_tcp_peer", start_peer)

    assert pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )
    assert controller.events.index("read:result for nettest") < controller.events.index(
        "peer:start:run-generation-31"
    )
    assert controller.events.index(
        "peer:start:run-generation-31"
    ) < controller.events.index("drain:nettest terminal observation window")


def test_genet_invalid_peer_target_skips_nettest_and_fails_closed(
    stub_nettest_peer: list[
        tuple[pi4_serial_reboot.NettestPeerConfig, str]
    ],
) -> None:
    """An unusable preflight address cannot launch cohsh or reuse old status."""

    invalid = NETSTATS_GENET_BOUND.replace(
        b"addr_src=dhcp-lease ip=192.168.10.50",
        b"addr_src=dhcp-lease ip=0.0.0.0",
    )
    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            invalid,
            b"OK SMP\ncohesix>",
            NETSTATS_OK,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    assert not pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )
    assert "nettest" not in controller.sent
    assert stub_nettest_peer == []
    assert (
        "diagnostics complete result=fail failures="
        "genet-dhcp:invalid-or-unbound,nettest:peer-target-unavailable"
    ) in controller.notes


def test_missing_nettest_admission_generation_never_launches_peer(
    stub_nettest_peer: list[
        tuple[pi4_serial_reboot.NettestPeerConfig, str]
    ],
) -> None:
    """A marker without its immutable run generation cannot authorize a peer."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            b"OK NETTEST detail=started\ncohesix>",
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    assert not pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )
    assert stub_nettest_peer == []
    assert "nettest:started-run-generation-missing" in controller.notes[-1]


def test_nettest_peer_nonzero_exit_cannot_override_target_pass(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Both the host peer and exact target terminal must independently pass."""

    monkeypatch.setattr(
        pi4_serial_reboot,
        "start_nettest_tcp_peer",
        lambda *_args: FakePeerProcess(return_code=7),
    )
    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    assert not pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )
    assert "nettest:peer-exit-7" in controller.notes[-1]
    assert any(
        note == "nettest terminal generation=14 run_generation=31 "
        "running=false result=pass source=netstats"
        for note in controller.notes
    )


def test_nettest_peer_auth_secret_is_environment_only(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The console credential never enters argv, child output, or config repr."""

    secret = "unique-peer-console-secret"
    config = pi4_serial_reboot.NettestPeerConfig(
        repo=REPO_ROOT,
        cohsh=REPO_ROOT / "out/cohesix/host-tools/cohsh",
        script=REPO_ROOT / "scripts/cohsh/boot_v0.coh",
        auth_token=secret,
    )
    captured: dict[str, object] = {}

    def popen(argv: list[str], **kwargs: object) -> FakePeerProcess:
        captured["argv"] = argv
        captured.update(kwargs)
        return FakePeerProcess()

    monkeypatch.setenv("COH_AUTH_TOKEN", "stale-lower-priority-secret")
    monkeypatch.setattr(pi4_serial_reboot.subprocess, "Popen", popen)
    monkeypatch.setattr(
        pi4_serial_reboot,
        "validate_nettest_peer_route",
        lambda lane, target: target if lane == "genet" else None,
    )

    process = REAL_START_NETTEST_TCP_PEER(config, "192.168.10.50", "genet")

    assert isinstance(process, FakePeerProcess)
    argv = captured["argv"]
    assert isinstance(argv, list)
    assert secret not in " ".join(argv)
    assert "--auth-token" not in argv
    assert "--role" not in argv
    environment = captured["env"]
    assert isinstance(environment, dict)
    assert environment["COHSH_AUTH_TOKEN"] == secret
    assert "COH_AUTH_TOKEN" not in environment
    assert captured["stdout"] is pi4_serial_reboot.subprocess.DEVNULL
    assert captured["stderr"] is pi4_serial_reboot.subprocess.DEVNULL
    assert captured["stdin"] is pi4_serial_reboot.subprocess.DEVNULL
    assert captured["close_fds"] is True
    assert captured["cwd"] == REPO_ROOT
    assert argv[argv.index("--tcp-port") + 1] == "31337"
    assert argv[argv.index("--script") + 1] == str(config.script)
    assert secret not in repr(config)


@pytest.mark.parametrize(
    ("snapshot", "required_lane", "expected"),
    (
        (NETSTATS_WIFI_BOUND, "wifi", ("wifi", "192.168.86.154", 4)),
        (NETSTATS_GENET_BOUND, "genet", ("genet", "192.168.10.50", 4)),
        (NETSTATS_GENET_BOUND, None, ("genet", "192.168.10.50", 4)),
    ),
)
def test_nettest_peer_selects_exact_dhcp_bound_physical_lane(
    snapshot: bytes,
    required_lane: str | None,
    expected: tuple[str, str, int],
) -> None:
    """The reusable peer selector binds one current physical lane and address."""

    assert (
        pi4_serial_reboot.select_nettest_peer_target(snapshot, required_lane)
        == expected
    )


@pytest.mark.parametrize(
    ("snapshot", "required_lane", "message"),
    (
        (NETSTATS_WIFI_PENDING, "wifi", "not exactly DHCP-bound"),
        (
            NETSTATS_WIFI_BOUND.replace(b"generation=4", b"generation=0"),
            "wifi",
            "not exactly DHCP-bound",
        ),
        (NETSTATS_WIFI_BOUND, "genet", "does not match required lane"),
        (
            NETSTATS_WIFI_BOUND + b"\n" + NETSTATS_WIFI_BOUND,
            "wifi",
            "one exact netstats network row",
        ),
    ),
)
def test_nettest_peer_selector_fails_closed_on_noncanonical_status(
    snapshot: bytes,
    required_lane: str,
    message: str,
) -> None:
    """Pending, zero-generation, cross-lane, and ambiguous state never launch."""

    with pytest.raises(RuntimeError, match=message):
        pi4_serial_reboot.select_nettest_peer_target(snapshot, required_lane)


def test_nettest_peer_observation_uses_canonical_window_and_always_reaps(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The gate wrapper's reusable peer lifecycle has one bound and one cleanup."""

    process = FakePeerProcess()
    events: list[tuple[str, object]] = []
    monkeypatch.setattr(
        pi4_serial_reboot,
        "start_nettest_tcp_peer",
        lambda config, target, lane: events.append(
            ("start", (config, target, lane))
        )
        or process,
    )
    monkeypatch.setattr(
        pi4_serial_reboot.time,
        "sleep",
        lambda seconds: events.append(("sleep", seconds)),
    )
    monkeypatch.setattr(
        pi4_serial_reboot,
        "finish_nettest_tcp_peer",
        lambda child: events.append(("finish", child)) or "exit-7",
    )
    monkeypatch.setattr(
        pi4_serial_reboot,
        "stop_nettest_tcp_peer",
        lambda child: events.append(("stop", child)),
    )

    failure = pi4_serial_reboot.observe_nettest_tcp_peer(
        TEST_NETTEST_PEER,
        "192.168.10.50",
        "genet",
    )

    assert failure == "exit-7"
    assert events == [
        ("start", (TEST_NETTEST_PEER, "192.168.10.50", "genet")),
        ("sleep", pi4_serial_reboot.NETTEST_OBSERVATION_S),
        ("finish", process),
        ("stop", process),
    ]
    with pytest.raises(RuntimeError, match="canonical bound"):
        pi4_serial_reboot.observe_nettest_tcp_peer(
            TEST_NETTEST_PEER,
            "192.168.10.50",
            "genet",
            pi4_serial_reboot.NETTEST_OBSERVATION_S - 1,
        )


def test_nettest_peer_route_is_exactly_bound_to_requested_interface(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Wired proof cannot escape en8 or use a network/broadcast/self address."""

    route_interface = "en8"

    def run(argv: list[str], **kwargs: object) -> SimpleNamespace:
        assert kwargs["check"] is False
        assert kwargs["capture_output"] is True
        assert kwargs["text"] is True
        if argv[0] == "/sbin/route":
            return SimpleNamespace(
                returncode=0,
                stdout=f"   interface: {route_interface}\n",
            )
        assert argv == ["/sbin/ifconfig", "en8"]
        return SimpleNamespace(
            returncode=0,
            stdout=(
                "en8: flags=8863<UP,BROADCAST,RUNNING>\n"
                "\tinet 192.168.10.1 netmask 0xffffff00 "
                "broadcast 192.168.10.255\n"
            ),
        )

    monkeypatch.setattr(pi4_serial_reboot.subprocess, "run", run)
    assert (
        pi4_serial_reboot.validate_nettest_peer_route("genet", "192.168.10.50")
        == "192.168.10.50"
    )

    route_interface = "en0"
    with pytest.raises(RuntimeError, match="requested lane"):
        pi4_serial_reboot.validate_nettest_peer_route("genet", "192.168.10.50")

    route_interface = "en8"
    for target in ("192.168.10.1", "192.168.10.255", "192.168.10.0"):
        with pytest.raises(RuntimeError, match="exact host subnet"):
            pi4_serial_reboot.validate_nettest_peer_route("genet", target)


@pytest.mark.parametrize(
    "target",
    (
        "8.8.8.8",
        "169.254.1.2",
        "127.0.0.1",
        "224.0.0.1",
        "0.0.0.0",
    ),
)
def test_nettest_peer_rejects_nonprivate_or_nondirect_targets(target: str) -> None:
    """The Queen credential cannot be sent to public or special-use targets."""

    assert pi4_serial_reboot.validate_nettest_peer_ip(target) is None


def test_nettest_peer_revalidates_preflight_files_before_spawn(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A replaced cohsh or script cannot inherit the already-loaded credential."""

    cohsh = tmp_path / "cohsh"
    script = tmp_path / "boot_v0.coh"
    cohsh.write_bytes(b"cohsh-v1")
    cohsh.chmod(0o755)
    script.write_text("ping\n", encoding="utf-8")
    config = pi4_serial_reboot.NettestPeerConfig(
        repo=tmp_path,
        cohsh=cohsh,
        script=script,
        auth_token="exact-secret",
        cohsh_sha256=pi4_serial_reboot.regular_file_sha256(cohsh),
        script_sha256=pi4_serial_reboot.regular_file_sha256(script),
    )
    monkeypatch.setattr(
        pi4_serial_reboot,
        "validate_nettest_peer_route",
        lambda *_args: "192.168.10.50",
    )
    monkeypatch.setattr(
        pi4_serial_reboot.subprocess,
        "Popen",
        lambda *_args, **_kwargs: pytest.fail("changed input reached Popen"),
    )

    script.write_text("help\n", encoding="utf-8")
    with pytest.raises(RuntimeError, match="changed after preflight"):
        REAL_START_NETTEST_TCP_PEER(config, "192.168.10.50", "genet")


def test_nettest_peer_timeout_cleanup_contains_nested_wait_failure() -> None:
    """Terminate, kill, and second-wait failures never leak into UART cleanup."""

    class StubbornProcess:
        def __init__(self) -> None:
            self.terminated = False
            self.killed = False
            self.waits = 0

        def poll(self) -> None:
            return None

        def terminate(self) -> None:
            self.terminated = True

        def kill(self) -> None:
            self.killed = True

        def wait(self, *, timeout: float) -> int:
            del timeout
            self.waits += 1
            if self.waits == 1:
                raise pi4_serial_reboot.subprocess.TimeoutExpired("cohsh", 1.0)
            raise OSError("already gone")

    process = StubbornProcess()
    pi4_serial_reboot.stop_nettest_tcp_peer(process)
    assert process.terminated
    assert process.killed
    assert process.waits == 2


def test_post_launch_note_failure_still_reaps_authenticated_peer(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Every exception after Popen preserves the original error and cleans up."""

    class RunningProcess:
        def __init__(self) -> None:
            self.terminated = False

        def poll(self) -> None:
            return None

        def terminate(self) -> None:
            self.terminated = True

        def kill(self) -> None:
            raise AssertionError("normal terminate should reap this child")

        def wait(self, *, timeout: float) -> int:
            del timeout
            assert self.terminated
            return 0

    class NoteFailureController(FakeController):
        def note(self, text: str) -> None:
            if text.startswith("nettest peer launch result=started"):
                raise RuntimeError("host log failed")
            super().note(text)

    process = RunningProcess()
    monkeypatch.setattr(
        pi4_serial_reboot,
        "start_nettest_tcp_peer",
        lambda *_args: process,
    )
    controller = NoteFailureController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
        ]
    )
    with pytest.raises(RuntimeError, match="host log failed"):
        pi4_serial_reboot.run_diagnostics(
            controller,
            "genet",
            nettest_peer=TEST_NETTEST_PEER,
            prompt_ready=True,
        )
    assert process.terminated


def test_prepare_nettest_peer_requires_one_usable_queen_secret(
    tmp_path: pathlib.Path,
) -> None:
    """Peer preparation binds a validated executable to one exact secret."""

    cohsh = tmp_path / "cohsh"
    cohsh.write_bytes(b"test executable\n")
    cohsh.chmod(0o700)
    manifest = tmp_path / "pi.toml"
    manifest.write_text(
        '[[tickets]]\nrole = "queen"\nsecret = "private-console-token"\n',
        encoding="utf-8",
    )

    config = pi4_serial_reboot.prepare_nettest_peer(
        REPO_ROOT,
        cohsh,
        manifest,
    )

    assert config.cohsh == cohsh
    assert config.script == REPO_ROOT / "scripts/cohsh/boot_v0.coh"
    assert config.auth_token == "private-console-token"
    assert "private-console-token" not in repr(config)

    manifest.write_text(
        '[[tickets]]\nrole = "queen"\nsecret = "one"\n'
        '[[tickets]]\nrole = "queen"\nsecret = "two"\n',
        encoding="utf-8",
    )
    with pytest.raises(RuntimeError, match="exactly one Queen secret"):
        pi4_serial_reboot.prepare_nettest_peer(
            REPO_ROOT,
            cohsh,
            manifest,
        )


def test_prepare_nettest_peer_rejects_symlink_manifest(
    tmp_path: pathlib.Path,
) -> None:
    """Credential inputs cannot redirect preparation through a symlink."""

    cohsh = tmp_path / "cohsh"
    cohsh.write_bytes(b"test executable\n")
    cohsh.chmod(0o700)
    manifest = tmp_path / "pi.toml"
    manifest.write_text(
        '[[tickets]]\nrole = "queen"\nsecret = "private-console-token"\n',
        encoding="utf-8",
    )
    link = tmp_path / "pi-link.toml"
    link.symlink_to(manifest)

    with pytest.raises(RuntimeError, match="canonical regular non-symlink"):
        pi4_serial_reboot.prepare_nettest_peer(REPO_ROOT, cohsh, link)


def test_nettest_result_parser_requires_complete_generation_tagged_terminal() -> None:
    """Admission ACKs and incomplete or inconsistent results are not terminal."""

    assert pi4_serial_reboot.parse_nettest_result(NETTEST_STARTED) is None
    assert (
        pi4_serial_reboot.parse_nettest_result(
            b"[net-selftest] result generation=14 tx_ok=true "
            b"udp_echo_ok=true tcp_ok=true console_ok=true "
            b"peer_assisted_ok=false result=pass\n"
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_result(
            b"[net-selftest] result generation=14 run_generation=31 "
            b"tx_ok=true udp_echo_ok=true tcp_ok=true console_ok=true "
            b"peer_assisted_ok=false\n"
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_result(
            NETTEST_RESULT.replace(b"result=pass", b"result=fail")
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_result(
            NETTEST_RESULT.replace(b"run_generation=31", b"run_generation=0")
        )
        is None
    )
    assert pi4_serial_reboot.parse_nettest_result(NETTEST_RESULT) == (
        14,
        NETTEST_RUN_GENERATION,
        "pass",
    )
    assert pi4_serial_reboot.parse_nettest_result(
        NETTEST_PEER_ASSISTED_RESULT
    ) == (14, NETTEST_RUN_GENERATION, "peer-assisted-pass")


def test_nettest_started_parser_requires_one_immutable_run_generation() -> None:
    """Admission proof carries one canonical run generation despite async logs."""

    assert (
        pi4_serial_reboot.parse_nettest_started_run_generation(NETTEST_STARTED)
        == NETTEST_RUN_GENERATION
    )
    assert (
        pi4_serial_reboot.parse_nettest_started_run_generation(
            b"OK NETTEST detail=started\ncohesix>"
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_started_run_generation(
            b"OK NETTEST detail=started run_generation=0\ncohesix>"
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_started_run_generation(
            b"OK NETTEST detail=started run_[local-seat] redraw\n"
            b"generation=31\ncohesix>"
        )
        == NETTEST_RUN_GENERATION
    )


def test_nettest_status_parser_requires_complete_compact_terminal() -> None:
    """Only the complete generation-tagged ``netstats`` line is authoritative."""

    assert pi4_serial_reboot.parse_nettest_status(NETTEST_STATUS_PASS) == (
        14,
        NETTEST_RUN_GENERATION,
        False,
        "pass",
    )
    assert pi4_serial_reboot.parse_nettest_status(
        NETTEST_STATUS_PEER_ASSISTED
    ) == (14, NETTEST_RUN_GENERATION, False, "peer-assisted-pass")
    assert (
        pi4_serial_reboot.parse_nettest_status(
            b"nettest: generation=14 run_generation=31 enabled=true "
            b"running=false verdict=pass tx_ok=true [truncated]\n"
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_status(
            b"nettest: generation=14 profile_backend=cyw43 "
            b"active_driver=cyw43 enabled=true running=false last=Some(...)\n"
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_status(
            NETTEST_STATUS_PASS.replace(b"udp_echo_ok=true", b"udp_echo_ok=false")
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_status(
            NETTEST_STATUS_PASS.replace(b"enabled=true", b"enabled=false")
        )
        is None
    )
    assert (
        pi4_serial_reboot.parse_nettest_status(
            NETTEST_STATUS_PASS.replace(b"run_generation=31", b"run_generation=0")
        )
        is None
    )
    assert pi4_serial_reboot.parse_nettest_status(
        b"nettest: generation=0 run_generation=0 enabled=false running=false "
        b"verdict=none tx_ok=na udp_echo_ok=na tcp_ok=na "
        b"console_ok=na peer_assisted_ok=na\n"
    ) == (0, 0, False, "none")
    assert pi4_serial_reboot.parse_nettest_status(
        b"nettest: generation=0 run_generation=0 enabled=true running=false "
        b"verdict=none tx_ok=na udp_echo_ok=na tcp_ok=na "
        b"console_ok=na peer_assisted_ok=na\n"
    ) == (0, 0, False, "none")


def test_smp_activity_rate_parser_requires_one_positive_delta_window() -> None:
    """A first, stale, duplicate, or overflowing sample is not rate evidence."""

    assert (
        pi4_serial_reboot.parse_smp_activity_rate_window(SMP_ACTIVITY_RATE_LINE)
        == 17_000
    )
    assert (
        pi4_serial_reboot.parse_smp_activity_rate_window(
            b"[smp] activity rates window_"
            b"[local-seat] redraw=deferred\n"
            b"ms=17000 cpu_pct=unavailable view=counter-delta "
            b"task_allocation=multi\n"
        )
        == 17_000
    )
    for invalid in (
        b"[smp] activity rates sample=first run_again=yes cpu_pct=unavailable\n",
        b"[smp] activity rates window_ms=0 status=stale "
        b"run_again=yes cpu_pct=unavailable\n",
        SMP_ACTIVITY_RATE_LINE + SMP_ACTIVITY_RATE_LINE,
        SMP_ACTIVITY_RATE_LINE.replace(b"17000", str(1 << 64).encode("ascii")),
    ):
        assert pi4_serial_reboot.parse_smp_activity_rate_window(invalid) is None


def test_diagnostics_fail_when_second_smp_sample_has_no_rate_window() -> None:
    """A second first/stale sample cannot silently become performance telemetry."""

    class MissingRateController(FakeController):
        def read_until(
            self,
            markers: Iterable[bytes],
            timeout_s: float,
            *,
            label: str,
            stream_prefix: bytes = b"",
        ) -> bytes:
            snapshot = super().read_until(
                markers,
                timeout_s,
                label=label,
                stream_prefix=stream_prefix,
            )
            if label == "result for smp activity":
                return snapshot.replace(SMP_ACTIVITY_RATE_LINE, b"")
            return snapshot

    controller = MissingRateController(
        [
            b"[local-seat] usb keyboard command-ready "
            b"action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    assert not pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )
    assert "smp-activity:rate-window-invalid" in controller.notes[-1]


def test_diagnostics_capture_final_netstats_after_nettest_error() -> None:
    """A refused self-test is terminal but still requires final counter capture."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            b"ERR NETTEST reason=policy detail=dhcp-pending\ncohesix>",
            NETSTATS_GENET_BOUND,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert not diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert controller.diagnostic_barriers[:3] == [
        "netstats",
        "smp activity-prefix",
        "nettest",
    ]
    assert not any(note.startswith("nettest terminal") for note in controller.notes)
    assert (
        "diagnostics complete result=fail failures=nettest:err"
        in controller.notes
    )


def test_diagnostics_accept_netstats_terminal_without_async_log() -> None:
    """The target's compact status is authoritative when async logs stay internal."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETSTATS_TERMINAL_PASS,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert diagnostics_ok
    assert (
        "nettest async-terminal absent action=query-generation-tagged-netstats"
        in controller.notes
    )
    assert (
        "nettest terminal generation=14 run_generation=31 "
        "running=false result=pass source=netstats"
    ) in controller.notes


def test_diagnostics_reject_netstats_run_generation_mismatch() -> None:
    """The final status must belong to the run admitted by this helper."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETSTATS_TERMINAL_PASS.replace(
                b"run_generation=31",
                b"run_generation=32",
            ),
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert not diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert (
        "nettest terminal mismatch started_run_generation=31 "
        "netstats_run_generation=32 action=fail-closed"
    ) in controller.notes
    assert (
        "diagnostics complete result=fail "
        "failures=nettest:run-generation-mismatch"
    ) in controller.notes


def test_diagnostics_reject_async_and_netstats_generation_mismatch() -> None:
    """Optional async evidence must exactly match the authoritative status."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            NETSTATS_TERMINAL_PASS.replace(
                b"generation=14",
                b"generation=15",
            ),
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert not diagnostics_ok
    assert (
        "nettest terminal mismatch async_generation=14 "
        "async_run_generation=31 async_result=pass "
        "netstats_generation=15 netstats_run_generation=31 "
        "netstats_result=pass action=fail-closed"
    ) in controller.notes
    assert (
        "diagnostics complete result=fail "
        "failures=nettest:terminal-contract-mismatch"
    ) in controller.notes


def test_diagnostics_reject_async_and_netstats_result_mismatch() -> None:
    """The same generations cannot identify two different terminal results."""

    terminal_failure = (
        b"nettest: generation=14 run_generation=31 enabled=true running=false "
        b"verdict=fail tx_ok=true udp_echo_ok=false tcp_ok=false "
        b"console_ok=true peer_assisted_ok=false\n"
        + NETSTATS_OK
    )
    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_RESULT,
            terminal_failure,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert not diagnostics_ok
    assert (
        "nettest terminal mismatch async_generation=14 "
        "async_run_generation=31 async_result=pass "
        "netstats_generation=14 netstats_run_generation=31 "
        "netstats_result=fail action=fail-closed"
    ) in controller.notes
    assert (
        "diagnostics complete result=fail "
        "failures=nettest:terminal-contract-mismatch"
    ) in controller.notes


def test_diagnostics_continue_after_generation_tagged_nettest_failure() -> None:
    """A failed async self-test is scored only after every diagnostic runs."""

    controller = FakeController(
        [
            b"[local-seat] usb keyboard command-ready action=enable-command-input\n",
            NETSTATS_GENET_BOUND,
            b"OK SMP\ncohesix>",
            NETTEST_STARTED,
            NETTEST_FAILURE_RESULT,
            NETSTATS_TERMINAL_FAILURE,
            b"OK USB\ncohesix>",
            b"OK USB\ncohesix>",
            b"OK SMP\ncohesix>",
        ]
    )

    diagnostics_ok = pi4_serial_reboot.run_diagnostics(
        controller,
        "genet",
        nettest_peer=TEST_NETTEST_PEER,
        prompt_ready=True,
    )

    assert not diagnostics_ok
    assert controller.sent == [
        "netstats",
        "smp activity",
        "nettest",
        "netstats",
        "usb diag",
        "usb status",
        "smp activity",
    ]
    assert (
        "nettest terminal generation=15 run_generation=31 "
        "running=false result=fail source=netstats"
    ) in controller.notes
    assert (
        "diagnostics complete result=fail "
        "failures=nettest:generation-15-run-31-fail"
    ) in controller.notes


def test_exact_build_marker_binds_boot_and_reuses_same_snapshot_prompt() -> None:
    """The exact sidecar marker, not its generic prefix, admits diagnostics."""

    controller = FakeController(
        [(EXPECTED_BUILD_MARKER + "\ncohesix> ").encode("ascii")]
    )

    snapshot, prompt_ready = pi4_serial_reboot.require_exact_build_marker(
        controller,
        EXPECTED_BUILD_MARKER,
        1.0,
    )

    assert EXPECTED_BUILD_MARKER.encode("ascii") in snapshot
    assert prompt_ready


def test_exact_build_marker_matches_across_serial_chunks(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The exact marker remains matchable when one UART read splits it."""

    controller, output = redaction_controller(b"secret-ticket")
    encoded = EXPECTED_BUILD_MARKER.encode("ascii")

    class MarkerSerial:
        def __init__(self) -> None:
            split = len(encoded) // 2
            self.reads = [encoded[:split], encoded[split:] + b"\ncohesix> "]

        def read(self, _size: int) -> bytes:
            return self.reads.pop(0) if self.reads else b""

    controller._serial = MarkerSerial()
    times = iter((0.0, 0.0, 0.0))
    monkeypatch.setattr(pi4_serial_reboot.time, "monotonic", lambda: next(times))

    snapshot, prompt_ready = pi4_serial_reboot.require_exact_build_marker(
        controller,
        EXPECTED_BUILD_MARKER,
        1.0,
    )

    assert snapshot == encoded + b"\ncohesix> "
    assert output.getvalue() == snapshot
    assert prompt_ready


def test_exact_build_marker_rejects_correct_marker_after_root_prompt() -> None:
    """A late exact marker cannot retroactively bind an already-ready boot."""

    controller = FakeController(
        [("cohesix> \n" + EXPECTED_BUILD_MARKER).encode("ascii")]
    )

    with pytest.raises(RuntimeError, match="before the expected sealed build marker"):
        pi4_serial_reboot.require_exact_build_marker(
            controller,
            EXPECTED_BUILD_MARKER,
            1.0,
        )


@pytest.mark.parametrize(
    "observed_marker",
    (
        "[BUILD] stale-image",
        EXPECTED_BUILD_MARKER.replace(EXPECTED_IMAGE_ID, "5" * 64),
    ),
)
def test_exact_build_marker_rejects_wrong_image_before_root(
    observed_marker: str,
) -> None:
    """A generic or same-source marker with another image ID fails closed."""

    controller = FakeController([(observed_marker + "\ncohesix> ").encode("ascii")])

    with pytest.raises(RuntimeError, match="before the expected sealed build marker"):
        pi4_serial_reboot.require_exact_build_marker(
            controller,
            EXPECTED_BUILD_MARKER,
            1.0,
        )


def test_image_identity_loader_rejects_dirty_metadata(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: pathlib.Path,
) -> None:
    """A dirty staged identity cannot authorize a physical diagnostic run."""

    monkeypatch.setattr(
        pi4_serial_reboot.pi4_image_identity,
        "read_metadata",
        lambda _path: expected_image_identity(clean=False),
    )

    with pytest.raises(RuntimeError, match="does not name a clean source tree"):
        pi4_serial_reboot.load_expected_image_identity(
            REPO_ROOT,
            tmp_path / "pi4-image-identity.json",
        )


def test_image_identity_loader_rejects_malformed_metadata(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: pathlib.Path,
) -> None:
    """Canonical sidecar validation errors remain fatal before serial opens."""

    def reject_metadata(_path: pathlib.Path) -> None:
        raise pi4_serial_reboot.pi4_image_identity.ImageIdentityError(
            "metadata canonical build ID is invalid"
        )

    monkeypatch.setattr(
        pi4_serial_reboot.pi4_image_identity,
        "read_metadata",
        reject_metadata,
    )

    with pytest.raises(RuntimeError, match="invalid Pi image identity metadata"):
        pi4_serial_reboot.load_expected_image_identity(
            REPO_ROOT,
            tmp_path / "pi4-image-identity.json",
        )


def test_run_validates_identity_before_opening_serial(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: pathlib.Path,
) -> None:
    """Malformed identity metadata cannot acquire or disturb the UART."""

    class Args:
        repo = REPO_ROOT
        image_identity_metadata = tmp_path / "pi4-image-identity.json"
        log = tmp_path / "serial.log"

    serial_constructed = False

    def construct_serial(*_args: object, **_kwargs: object) -> None:
        nonlocal serial_constructed
        serial_constructed = True
        raise AssertionError("serial must not open before identity validation")

    def reject_identity(*_args: object) -> None:
        raise RuntimeError("invalid identity")

    monkeypatch.setattr(pi4_serial_reboot, "parse_args", Args)
    monkeypatch.setattr(
        pi4_serial_reboot,
        "load_expected_image_identity",
        reject_identity,
    )
    monkeypatch.setattr(
        pi4_serial_reboot,
        "RedactingSerialController",
        construct_serial,
    )

    with pytest.raises(RuntimeError, match="invalid identity"):
        pi4_serial_reboot.run()
    assert not serial_constructed


def test_run_validates_nettest_peer_before_opening_serial(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: pathlib.Path,
) -> None:
    """Missing diagnostic peer inputs cannot acquire or disturb the UART."""

    class Args:
        repo = REPO_ROOT
        image_identity_metadata = tmp_path / "pi4-image-identity.json"
        diagnostics = True
        cohsh = tmp_path / "missing-cohsh"
        ticket_config = REPO_ROOT / "configs/root_task_pi4_uboot_aarch64.toml"
        log = tmp_path / "serial.log"

    serial_constructed = False

    def construct_serial(*_args: object, **_kwargs: object) -> None:
        nonlocal serial_constructed
        serial_constructed = True
        raise AssertionError("serial must not open before peer preflight")

    monkeypatch.setattr(pi4_serial_reboot, "parse_args", Args)
    monkeypatch.setattr(
        pi4_serial_reboot,
        "load_expected_image_identity",
        lambda *_args: expected_image_identity(),
    )
    monkeypatch.setattr(
        pi4_serial_reboot,
        "RedactingSerialController",
        construct_serial,
    )

    with pytest.raises(RuntimeError, match="nettest peer cohsh is unavailable"):
        pi4_serial_reboot.run()
    assert not serial_constructed


def test_run_returns_nonzero_after_diagnostic_failure(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: pathlib.Path,
) -> None:
    """The CLI reports diagnostic failure only after the evidence run completes."""

    class Args:
        lane = "genet"
        log = tmp_path / "serial.log"
        image_identity_metadata = tmp_path / "pi4-image-identity.json"
        repo = REPO_ROOT
        cohsh = pathlib.Path("cohsh")
        ticket_config = pathlib.Path("config.toml")
        port = "serial-test"
        baud = 115_200
        no_echo = True
        char_delay_ms = 0.0
        initial_state = "menu"
        boot_timeout_s = 1.0
        diagnostics = True

    class RunController(FakeController):
        def __init__(self, *_args: object, **_kwargs: object) -> None:
            super().__init__(
                [EXPECTED_BUILD_MARKER.encode("ascii") + b"\ncohesix", b"> "]
            )
            self.closed = False

        def close(self) -> None:
            self.closed = True

    controller = RunController()
    monkeypatch.setattr(pi4_serial_reboot, "parse_args", Args)
    monkeypatch.setattr(
        pi4_serial_reboot,
        "load_expected_image_identity",
        lambda *_args: expected_image_identity(),
    )
    monkeypatch.setattr(
        pi4_serial_reboot,
        "prepare_nettest_peer",
        lambda *_args: TEST_NETTEST_PEER,
    )
    monkeypatch.setattr(
        pi4_serial_reboot,
        "RedactingSerialController",
        lambda *_args, **_kwargs: controller,
    )
    monkeypatch.setattr(pi4_serial_reboot, "select_lane", lambda *_args: None)
    monkeypatch.setattr(
        pi4_serial_reboot,
        "run_diagnostics",
        lambda *_args, **_kwargs: False,
    )

    assert pi4_serial_reboot.run() == 1
    assert controller.closed
    assert any(
        note == (
            "image identity matched "
            f"git_commit={EXPECTED_GIT_COMMIT} "
            f"build_id={EXPECTED_BUILD_ID} "
            f"image_id={EXPECTED_IMAGE_ID} "
            f"build_marker_sha256={EXPECTED_BUILD_MARKER_SHA256}"
        )
        for note in controller.notes
    )
    assert "complete result=diagnostic-failure exit=1" in controller.notes
    assert controller.redactions == [
        (
            "AUTH test-console-secret",
            "AUTH <queen-console-token>",
        )
    ]


def test_diagnostics_require_command_specific_result_marker() -> None:
    """A stale OK from another command must not satisfy the next diagnostic."""

    controller = FakeController(
        [
            b"OK WIFI\ncohesix>",
        ]
    )

    with pytest.raises(AssertionError):
        pi4_serial_reboot.run_diagnostics(
            controller,
            "genet",
            nettest_peer=TEST_NETTEST_PEER,
            prompt_ready=True,
        )


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
    assert controller.root_terminator_guards == [False, True, True, True]
    assert controller.redactions == [("secret-ticket", "<queen-ticket>")]
    assert controller.diagnostic_barriers == ["authenticated reboot"]
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
    assert controller.diagnostic_barriers == ["authenticated reboot"]
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
