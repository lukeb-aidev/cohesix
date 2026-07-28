#!/usr/bin/env python3
"""Author: Lukas Bower
Purpose: Drive repeatable Raspberry Pi 4 serial reboots through pyserial.
Copyright 2026 Lukas Bower
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys
import time
from collections.abc import Iterable

try:
    import serial
except ImportError as exc:  # pragma: no cover - exercised by host setup.
    raise SystemExit(
        "pyserial is required; run `.venv/bin/pip install pyserial` from the repo root"
    ) from exc


DEFAULT_PORT = "/dev/cu.usbserial-0001"
DEFAULT_BAUD = 115_200
DEFAULT_REPO = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_COHSH = pathlib.Path("out/cohesix/host-tools/cohsh")
DEFAULT_TICKET_CONFIG = pathlib.Path("configs/root_task_pi4_uboot_aarch64.toml")
DEFAULT_CHAR_DELAY_S = 0.06
DEFAULT_LINE_TERMINATOR = "\r"
TAIL_LIMIT = 131_072
CHOICE_PROMPT = b"Select option ["
ROOT_PROMPT = b"cohesix>"
ROOT_PROMPT_FULL = b"cohesix> "
ROOT_PROMPT_MIN_SUFFIX = 2
ROOT_MENU_MARKERS = (
    b"[cohesix] Cohesix boot menu",
    b"Boot with saved settings",
    b"Boot with default settings",
)
DHCP_MENU_MARKER = b"Choose IPv4 configuration"
INTERFACE_MENU_MARKER = b"Choose network connection"
REVIEW_MENU_MARKER = b"Review network settings"
WIFI_SETUP_MARKER = b"Choose Wi-Fi network"
STATIC_SETUP_MARKER = b"Network setup: manual IPv4"
RESET_MENU_MARKER = b"Reset saved settings?"
MENU_MARKERS = (
    *ROOT_MENU_MARKERS,
    DHCP_MENU_MARKER,
    INTERFACE_MENU_MARKER,
    REVIEW_MENU_MARKER,
    WIFI_SETUP_MARKER,
    STATIC_SETUP_MARKER,
    RESET_MENU_MARKER,
    CHOICE_PROMPT,
)
DIAGNOSTIC_RESULT_MARKERS: dict[str, tuple[bytes, bytes]] = {
    "netstats": (b"OK NETSTATS", b"ERR NETSTATS"),
    "nettest": (b"OK NETTEST", b"ERR NETTEST"),
    "wifi diag": (b"OK WIFI", b"ERR WIFI"),
    "usb diag": (b"OK USB", b"ERR USB"),
    "usb probe-kbd": (b"OK USB", b"ERR USB"),
    "smp activity": (b"OK SMP", b"ERR SMP"),
}
DIAGNOSTIC_READY_MARKERS = (
    b"usb keyboard command-ready",
)
DIAGNOSTIC_SETTLE_TIMEOUT_S = 30.0
DIAGNOSTIC_COMMAND_DRAIN_S = 0.25
NETTEST_STARTED_MARKER = b"OK NETTEST detail=started"
NETTEST_OBSERVATION_S = 17.0
WIFI_SUPERVISOR_TERMINAL_TIMEOUT_S = 240.0
WIFI_DHCP_TERMINAL_TIMEOUT_S = 60.0
WIFI_DHCP_POLL_INTERVAL_S = 5.0
WIFI_SUPERVISOR_TERMINAL_MARKERS = (
    b"CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready",
    b"CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=failed",
    b"CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=permanent",
)
U32_MAX = (1 << 32) - 1
U64_MAX = (1 << 64) - 1
WIFI_SUPERVISOR_PREFIX = b"CYW43_BOOTSTRAP_SUPERVISOR "
WIFI_SUPERVISOR_HEADER_RE = re.compile(
    rb"^CYW43_BOOTSTRAP_SUPERVISOR "
    rb"attempt=(?P<attempt>[^ \r\n]+) "
    rb"status=(?P<status>[^ \r\n]+)"
)
WIFI_SUPERVISOR_TERMINAL_RE = re.compile(
    rb"^CYW43_BOOTSTRAP_SUPERVISOR "
    rb"attempt=(?P<attempt>0|[1-9][0-9]*) "
    rb"status=(?P<status>ready|failed|permanent) "
    rb"backoff_ms=(?P<backoff_ms>0|[1-9][0-9]*) "
    rb"next_attempt_ms=(?P<next_attempt_ms>0|[1-9][0-9]*) "
    rb"serial=(?P<serial>ready|blocked) "
    rb"local_seat=(?P<local_seat>enabled|disabled|ready) "
    rb"recovery=full "
    rb"console_seq=(?P<console_seq>0|[1-9][0-9]*) "
    rb"telemetry_sinks=serial\+qlog\+hdmi "
    rb"prompt_refresh=yes$"
)
NETSTATS_NETWORK_RE = re.compile(
    rb"netstats: generation=(?P<generation>0|[1-9][0-9]*) "
    rb"mode=(?P<mode>[^ \r\n]+) "
    rb"policy=(?P<policy>[^ \r\n]+) "
    rb"active=(?P<active>[^ \r\n]+) "
    rb"standby=[^ \r\n]+ "
    rb"addr_src=(?P<address_source>[^ \r\n]+) "
    rb"ip=(?P<ip>[^ \r\n]+) "
    rb"gateway=[^ \r\n]+ "
    rb"dhcp=(?P<dhcp_phase>[^ \r\n]+)"
)
NETTEST_STARTED_RE = re.compile(
    rb"OKNETTESTdetail=startedrun_generation="
    rb"(?P<run_generation>[1-9][0-9]*)"
)
NETTEST_RESULT_RE = re.compile(
    rb"\[net-selftest\] result "
    rb"generation=(?P<generation>0|[1-9][0-9]*) "
    rb"run_generation=(?P<run_generation>[1-9][0-9]*) "
    rb"tx_ok=(?P<tx_ok>true|false) "
    rb"udp_echo_ok=(?P<udp_echo_ok>true|false) "
    rb"tcp_ok=(?P<tcp_ok>true|false) "
    rb"console_ok=(?P<console_ok>true|false) "
    rb"peer_assisted_ok=(?P<peer_assisted_ok>true|false) "
    rb"result=(?P<result>pass|peer-assisted-pass|fail)(?:\r?\n|$)"
)
NETTEST_STATUS_RE = re.compile(
    rb"nettest: generation=(?P<generation>0|[1-9][0-9]*) "
    rb"run_generation=(?P<run_generation>0|[1-9][0-9]*) "
    rb"enabled=(?P<enabled>true|false) "
    rb"running=(?P<running>true|false) "
    rb"verdict=(?P<result>none|running|pass|peer-assisted-pass|fail) "
    rb"tx_ok=(?P<tx_ok>true|false|na) "
    rb"udp_echo_ok=(?P<udp_echo_ok>true|false|na) "
    rb"tcp_ok=(?P<tcp_ok>true|false|na) "
    rb"console_ok=(?P<console_ok>true|false|na) "
    rb"peer_assisted_ok=(?P<peer_assisted_ok>true|false|na)"
)
ASYNC_RESULT_FRAGMENT_RE = re.compile(
    rb"(?:"
    rb"\[[A-Za-z0-9_.:-]+\]"
    rb"|SERIAL_INPUT_TRACE"
    rb"|HDMI_FRAME_[A-Z_]*"
    rb"|DRIVER_TASK_[A-Z_]*"
    rb"|SCHED_CONTRACT"
    rb"|CYW43_[A-Z0-9_]*"
    rb"|NET_DRIVER_TASK_[A-Z_]*"
    rb"|SDIO_DRIVER_TASK_[A-Z_]*"
    rb")[^\r\n]*"
)

MENU_ROOT = "root"
MENU_DHCP = "dhcp"
MENU_INTERFACE = "interface"
MENU_REVIEW = "review"
MENU_WIFI_SETUP = "wifi-setup"
MENU_STATIC_SETUP = "static-setup"
MENU_RESET = "reset"
MENU_UNKNOWN = "unknown"


class SerialMarkerTimeout(RuntimeError):
    """Raised when the expected serial marker does not arrive in time."""


def remaining_before_deadline(deadline: float, *, label: str) -> float:
    """Return positive time remaining before one absolute host deadline."""

    remaining_s = deadline - time.monotonic()
    if remaining_s <= 0:
        raise SerialMarkerTimeout(f"timed out waiting for {label}")
    return remaining_s


class RedactingSerialController:
    """Own the serial port, log redacted bytes, and send paced command lines."""

    def __init__(
        self,
        port: str,
        baud: int,
        log_path: pathlib.Path,
        *,
        echo: bool,
        char_delay_s: float,
    ) -> None:
        self._serial = serial.Serial(port, baud, timeout=0.05, write_timeout=1)
        self._log = log_path.open("ab")
        self._redactions: list[tuple[bytes, bytes]] = []
        self._redaction_carry = b""
        self._pending_annotations: list[bytes] = []
        self._echo = echo
        self._char_delay_s = char_delay_s

    def close(self) -> None:
        self._flush_redaction_carry()
        self._log.close()
        self._serial.close()

    def add_redaction(self, secret: str, replacement: str) -> None:
        encoded = secret.encode()
        if not encoded:
            raise ValueError("serial redaction secret must not be empty")
        self._redactions.append((encoded, replacement.encode()))

    def _matching_redaction(self, data: bytes, offset: int) -> tuple[int, bytes] | None:
        """Return the longest configured secret beginning at ``offset``."""

        matches = [
            (len(secret), replacement)
            for secret, replacement in self._redactions
            if data.startswith(secret, offset)
        ]
        return max(matches, default=None, key=lambda match: match[0])

    def _redact_final(self, data: bytes) -> bytes:
        """Redact complete secrets and conceal any trailing secret prefix."""

        if not self._redactions:
            return data
        redacted = bytearray()
        offset = 0
        while offset < len(data):
            match = self._matching_redaction(data, offset)
            if match is not None:
                length, replacement = match
                redacted.extend(replacement)
                offset += length
                continue
            suffix = data[offset:]
            if any(secret.startswith(suffix) for secret, _ in self._redactions):
                redacted.extend(b"<redacted-partial>")
                break
            redacted.append(data[offset])
            offset += 1
        return bytes(redacted)

    def _write_safe(self, data: bytes) -> None:
        self._log.write(data)
        self._log.flush()
        if self._echo:
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()

    def _flush_redaction_carry(self) -> None:
        if self._redaction_carry:
            self._write_safe(self._redact_final(self._redaction_carry))
            self._redaction_carry = b""
        self._flush_pending_annotations()

    def _flush_pending_annotations(self) -> None:
        for annotation in self._pending_annotations:
            self._write_safe(annotation)
        self._pending_annotations.clear()

    def _record(self, data: bytes) -> None:
        if not self._redactions:
            self._write_safe(data)
            return
        pending = self._redaction_carry + data
        redacted = bytearray()
        offset = 0
        while offset < len(pending):
            suffix = pending[offset:]
            if any(
                len(secret) > len(suffix) and secret.startswith(suffix)
                for secret, _ in self._redactions
            ):
                break
            match = self._matching_redaction(pending, offset)
            if match is not None:
                length, replacement = match
                redacted.extend(replacement)
                offset += length
            else:
                redacted.append(pending[offset])
                offset += 1
        self._redaction_carry = pending[offset:]
        if redacted:
            self._write_safe(bytes(redacted))
        if not self._redaction_carry:
            self._flush_pending_annotations()

    def note(self, text: str) -> None:
        """Write a host annotation without disturbing serial-stream redaction."""

        annotation = self._redact_final(f"\n[host] {text}\n".encode())
        if self._redaction_carry:
            self._pending_annotations.append(annotation)
        else:
            self._write_safe(annotation)

    def send_line(
        self,
        line: str,
        *,
        public_line: str | None = None,
        reinforce_terminator: bool = False,
        guard_root_terminator: bool = False,
    ) -> None:
        self.note(f"send {public_line if public_line is not None else line}")
        for byte in serial_line_bytes(
            line,
            guard_root_terminator=guard_root_terminator,
        ):
            self._serial.write(bytes([byte]))
            self._serial.flush()
            time.sleep(self._char_delay_s)
        if reinforce_terminator and line:
            time.sleep(max(self._char_delay_s, 0.1))
            self._serial.write(DEFAULT_LINE_TERMINATOR.encode())
            self._serial.flush()

    def read_until(
        self,
        markers: Iterable[bytes],
        timeout_s: float,
        *,
        label: str,
    ) -> bytes:
        needles = tuple(markers)
        deadline = time.monotonic() + timeout_s
        seen = bytearray()
        while time.monotonic() < deadline:
            chunk = self._serial.read(4096)
            if not chunk:
                continue
            self._record(chunk)
            seen.extend(chunk)
            if len(seen) > TAIL_LIMIT:
                del seen[: len(seen) - TAIL_LIMIT]
            snapshot = bytes(seen)
            if any(serial_marker_seen(snapshot, marker) for marker in needles):
                return snapshot
        tail = self._redact_final(bytes(seen[-2048:])).decode(
            "utf-8", errors="replace"
        )
        raise SerialMarkerTimeout(f"timed out waiting for {label}; tail={tail!r}")

    def drain_for(self, duration_s: float, *, label: str) -> bytes:
        """Record post-marker serial chatter before sending the next command."""

        self.note(f"drain {label} duration_s={duration_s:.2f}")
        deadline = time.monotonic() + duration_s
        seen = bytearray()
        while time.monotonic() < deadline:
            chunk = self._serial.read(4096)
            if chunk:
                self._record(chunk)
                seen.extend(chunk)
        return bytes(seen)

    def synchronize_root_diagnostic_command(
        self,
        *,
        label: str,
        deadline: float | None = None,
    ) -> None:
        """Prove a fresh, complete root prompt before one diagnostic command."""

        drain_s = DIAGNOSTIC_COMMAND_DRAIN_S
        if deadline is not None:
            drain_s = min(
                drain_s,
                remaining_before_deadline(
                    deadline,
                    label=f"diagnostic barrier before {label}",
                ),
            )
        self.drain_for(
            drain_s,
            label=f"stale serial output before {label}",
        )
        self.send_line("", public_line="<clear-line>")
        self.send_line("ping", guard_root_terminator=True)
        ping_timeout_s = 10.0
        if deadline is not None:
            ping_timeout_s = min(
                ping_timeout_s,
                remaining_before_deadline(
                    deadline,
                    label=f"root ping before {label}",
                ),
            )
        ping_snapshot = self.read_until(
            (b"OK PING",),
            ping_timeout_s,
            label=f"root ping OK before {label}",
        )
        prompt_offset = ping_snapshot.rfind(ROOT_PROMPT_FULL)
        prompt_follows_ping = prompt_offset >= 0 and serial_marker_seen(
            ping_snapshot[:prompt_offset],
            b"OK PING",
        )
        if not prompt_follows_ping:
            prompt_timeout_s = 10.0
            if deadline is not None:
                prompt_timeout_s = min(
                    prompt_timeout_s,
                    remaining_before_deadline(
                        deadline,
                        label=f"root prompt before {label}",
                    ),
                )
            self.read_until(
                (ROOT_PROMPT_FULL,),
                prompt_timeout_s,
                label=f"fresh full root prompt before {label}",
            )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Drive a Cohesix Pi 4 reboot from serial with pyserial."
    )
    parser.add_argument("--lane", choices=("wifi", "genet"), required=True)
    parser.add_argument("--log", type=pathlib.Path, required=True)
    parser.add_argument("--repo", type=pathlib.Path, default=DEFAULT_REPO)
    parser.add_argument("--port", default=DEFAULT_PORT)
    parser.add_argument("--baud", type=int, default=DEFAULT_BAUD)
    parser.add_argument("--cohsh", type=pathlib.Path, default=DEFAULT_COHSH)
    parser.add_argument("--ticket-config", type=pathlib.Path, default=DEFAULT_TICKET_CONFIG)
    parser.add_argument("--char-delay-ms", type=float, default=DEFAULT_CHAR_DELAY_S * 1000)
    parser.add_argument("--initial-timeout-s", type=float, default=90)
    parser.add_argument("--uboot-timeout-s", type=float, default=150)
    parser.add_argument("--boot-timeout-s", type=float, default=240)
    parser.add_argument(
        "--initial-state",
        choices=("auto", "menu", "root"),
        default="auto",
        help=(
            "Use menu when the Cohesix U-Boot menu is already displayed, "
            "root when the root prompt is already ready, or auto to wait "
            "for fresh serial markers."
        ),
    )
    parser.add_argument("--no-echo", action="store_true")
    parser.add_argument(
        "--diagnostics",
        action="store_true",
        help="Run the required boot diagnostic commands after the root prompt returns.",
    )
    return parser.parse_args()


def resolve_under_repo(repo: pathlib.Path, path: pathlib.Path) -> pathlib.Path:
    return path if path.is_absolute() else repo / path


def serial_line_bytes(
    line: str,
    *,
    guard_root_terminator: bool = False,
) -> bytes:
    """Encode a serial command using the Pi 4 console's CR line discipline."""

    terminator_guard = " " if line and guard_root_terminator else ""
    return f"{line}{terminator_guard}{DEFAULT_LINE_TERMINATOR}".encode()


def serial_marker_seen(snapshot: bytes, marker: bytes) -> bool:
    """Return whether a marker is present, tolerating async logs in result tokens."""

    if marker in snapshot:
        return True
    if marker == ROOT_PROMPT:
        tail = snapshot.rstrip()
        return any(
            tail.endswith(marker[-suffix_len:])
            for suffix_len in range(ROOT_PROMPT_MIN_SUFFIX, len(marker))
        )
    if not marker.startswith((b"OK ", b"ERR ")):
        return False
    cleaned = ASYNC_RESULT_FRAGMENT_RE.sub(b"", snapshot)
    compact_snapshot = b"".join(cleaned.split())
    compact_marker = b"".join(marker.split())
    return compact_marker in compact_snapshot


def inspect_wifi_supervisor_evidence(
    snapshot: bytes,
    *,
    reject_trailing_partial: bool = False,
) -> tuple[tuple[int, str] | None, str | None]:
    """Validate complete supervisor lines and return one production terminal."""

    terminal: tuple[int, str] | None = None
    for raw_line in snapshot.splitlines(keepends=True):
        line_complete = raw_line.endswith((b"\r", b"\n"))
        line = (
            raw_line.removesuffix(b"\n").removesuffix(b"\r")
            if line_complete
            else raw_line
        )
        if not line.startswith(WIFI_SUPERVISOR_PREFIX):
            continue
        header = WIFI_SUPERVISOR_HEADER_RE.match(line)
        if not line_complete:
            if not reject_trailing_partial:
                continue
            if header is not None:
                attempt_raw = header.group("attempt")
                if (
                    attempt_raw.isdigit()
                    and not (
                        len(attempt_raw) > 1 and attempt_raw.startswith(b"0")
                    )
                    and len(attempt_raw) <= 10
                ):
                    attempt = int(attempt_raw)
                    if attempt <= U32_MAX and attempt > 1:
                        return None, f"attempt-{attempt}-forbidden"
            return None, "trailing-supervisor-fragment"
        if header is None:
            if any(
                token in line
                for token in (
                    b"status=ready",
                    b"status=failed",
                    b"status=permanent",
                )
            ):
                return None, "terminal-schema-invalid"
            continue
        attempt_raw = header.group("attempt")
        if (
            not attempt_raw.isdigit()
            or (len(attempt_raw) > 1 and attempt_raw.startswith(b"0"))
            or len(attempt_raw) > 10
        ):
            return None, "attempt-invalid"
        attempt = int(attempt_raw)
        if attempt > U32_MAX:
            return None, "attempt-invalid"
        if attempt > 1:
            return None, f"attempt-{attempt}-forbidden"
        status = header.group("status").decode("ascii", errors="replace")
        if status not in {"ready", "failed", "permanent"}:
            continue
        match = WIFI_SUPERVISOR_TERMINAL_RE.fullmatch(line)
        if match is None or attempt != 1:
            return None, "terminal-schema-invalid"
        numeric_fields = (
            match.group("backoff_ms"),
            match.group("next_attempt_ms"),
            match.group("console_seq"),
        )
        if any(len(value) > 20 or int(value) > U64_MAX for value in numeric_fields):
            return None, "terminal-numeric-field-invalid"
        if int(match.group("backoff_ms")) != 0:
            return None, "terminal-backoff-invalid"
        candidate = (attempt, status)
        if terminal is not None:
            if terminal != candidate:
                return None, "terminal-contradiction"
            return None, "terminal-duplicate"
        terminal = candidate
    return terminal, None


def parse_wifi_supervisor_terminal(snapshot: bytes) -> tuple[int, str] | None:
    """Return one line-complete, exact production Wi-Fi terminal record."""

    terminal, error = inspect_wifi_supervisor_evidence(snapshot)
    return terminal if error is None else None


def wifi_terminal_marker_offset(snapshot: bytes) -> int:
    """Return the newest terminal-prefix offset, or ``-1`` when absent."""

    return max(snapshot.rfind(marker) for marker in WIFI_SUPERVISOR_TERMINAL_MARKERS)


def parse_netstats_network_status(
    snapshot: bytes,
) -> tuple[int, str, str, str, str, str, str] | None:
    """Return one complete network-state line from a ``netstats`` response."""

    matches = [
        NETSTATS_NETWORK_RE.fullmatch(line.removesuffix(b"\r"))
        for line in snapshot.splitlines()
    ]
    matches = [match for match in matches if match is not None]
    if len(matches) != 1:
        return None
    match = matches[0]
    generation = int(match.group("generation"))
    if generation > U32_MAX:
        return None
    return (
        generation,
        match.group("mode").decode("ascii"),
        match.group("policy").decode("ascii"),
        match.group("active").decode("ascii"),
        match.group("address_source").decode("ascii"),
        match.group("ip").decode("ascii"),
        match.group("dhcp_phase").decode("ascii"),
    )


def parse_nettest_started_run_generation(snapshot: bytes) -> int | None:
    """Return the immutable run generation from one admission ACK."""

    cleaned = ASYNC_RESULT_FRAGMENT_RE.sub(b"", snapshot)
    compact = b"".join(cleaned.split())
    matches = list(NETTEST_STARTED_RE.finditer(compact))
    if len(matches) != 1:
        return None
    run_generation = int(matches[0].group("run_generation"))
    return run_generation if run_generation <= U64_MAX else None


def nettest_result_verdict(
    *,
    tx_ok: bool,
    udp_echo_ok: bool,
    tcp_ok: bool,
    console_ok: bool,
    peer_assisted_ok: bool,
) -> str:
    """Return the target's deterministic verdict for one result tuple."""

    if tx_ok and udp_echo_ok and tcp_ok and console_ok:
        return "pass"
    if peer_assisted_ok:
        return "peer-assisted-pass"
    return "fail"


def parse_nettest_result(snapshot: bytes) -> tuple[int, int, str] | None:
    """Return both generations and verdict from one complete async result."""

    matches = list(NETTEST_RESULT_RE.finditer(snapshot))
    if len(matches) != 1:
        return None
    match = matches[0]
    generation = int(match.group("generation"))
    run_generation = int(match.group("run_generation"))
    result = match.group("result").decode("ascii")
    expected_result = nettest_result_verdict(
        tx_ok=match.group("tx_ok") == b"true",
        udp_echo_ok=match.group("udp_echo_ok") == b"true",
        tcp_ok=match.group("tcp_ok") == b"true",
        console_ok=match.group("console_ok") == b"true",
        peer_assisted_ok=match.group("peer_assisted_ok") == b"true",
    )
    if (
        generation > U32_MAX
        or run_generation > U64_MAX
        or result != expected_result
    ):
        return None
    return generation, run_generation, result


def parse_nettest_status(snapshot: bytes) -> tuple[int, int, bool, str] | None:
    """Return a complete, internally consistent compact ``netstats`` status."""

    candidates = [
        line.removesuffix(b"\r")
        for line in snapshot.splitlines()
        if line.startswith(b"nettest:")
    ]
    if len(candidates) != 1 or b"[truncated]" in candidates[0]:
        return None
    match = NETTEST_STATUS_RE.fullmatch(candidates[0])
    if match is None:
        return None
    generation = int(match.group("generation"))
    run_generation = int(match.group("run_generation"))
    enabled = match.group("enabled") == b"true"
    if generation > U32_MAX or run_generation > U64_MAX:
        return None
    running = match.group("running") == b"true"
    result = match.group("result").decode("ascii")
    result_fields = tuple(
        match.group(field).decode("ascii")
        for field in (
            "tx_ok",
            "udp_echo_ok",
            "tcp_ok",
            "console_ok",
            "peer_assisted_ok",
        )
    )
    if run_generation == 0 and (
        running or result != "none" or result_fields != ("na",) * 5
    ):
        return None
    if not enabled:
        if running or result != "none" or result_fields != ("na",) * 5:
            return None
    elif running:
        if result != "running" or result_fields != ("na",) * 5:
            return None
    elif result == "none":
        if result_fields != ("na",) * 5:
            return None
    elif result == "running" or any(value == "na" for value in result_fields):
        return None
    else:
        expected_result = nettest_result_verdict(
            tx_ok=result_fields[0] == "true",
            udp_echo_ok=result_fields[1] == "true",
            tcp_ok=result_fields[2] == "true",
            console_ok=result_fields[3] == "true",
            peer_assisted_ok=result_fields[4] == "true",
        )
        if result != expected_result:
            return None
    return generation, run_generation, running, result


def mint_ticket(repo: pathlib.Path, cohsh: pathlib.Path, ticket_config: pathlib.Path) -> str:
    ticket = subprocess.check_output(
        [
            str(resolve_under_repo(repo, cohsh)),
            "--mint-ticket",
            "--role",
            "queen",
            "--ticket-config",
            str(ticket_config),
        ],
        cwd=repo,
        text=True,
    ).strip()
    if not ticket:
        raise RuntimeError("cohsh returned an empty Queen ticket")
    return ticket


def classify_menu_state(snapshot: bytes) -> str:
    """Classify the currently visible Cohesix U-Boot menu page."""

    candidates: list[tuple[int, str]] = []
    for marker in ROOT_MENU_MARKERS:
        candidates.append((snapshot.rfind(marker), MENU_ROOT))
    for marker, state in (
        (DHCP_MENU_MARKER, MENU_DHCP),
        (INTERFACE_MENU_MARKER, MENU_INTERFACE),
        (REVIEW_MENU_MARKER, MENU_REVIEW),
        (WIFI_SETUP_MARKER, MENU_WIFI_SETUP),
        (STATIC_SETUP_MARKER, MENU_STATIC_SETUP),
        (RESET_MENU_MARKER, MENU_RESET),
    ):
        candidates.append((snapshot.rfind(marker), state))
    offset, state = max(candidates, key=lambda item: item[0])
    if offset >= 0:
        return state
    return MENU_UNKNOWN


def read_menu_snapshot(
    controller: RedactingSerialController,
    timeout_s: float,
    *,
    label: str,
    initial_snapshot: bytes = b"",
) -> bytes:
    """Read enough U-Boot menu output to include the choice prompt."""

    snapshot = initial_snapshot
    if not snapshot:
        snapshot = controller.read_until(MENU_MARKERS, timeout_s, label=label)
    if CHOICE_PROMPT not in snapshot:
        snapshot += controller.read_until(
            (CHOICE_PROMPT,),
            min(timeout_s, 20),
            label=f"{label} choice prompt",
        )
    return snapshot


def wait_for_menu_state(
    controller: RedactingSerialController,
    timeout_s: float,
    *,
    label: str,
) -> tuple[str, bytes]:
    """Wait for a U-Boot menu page and return its classified state."""

    snapshot = read_menu_snapshot(controller, timeout_s, label=label)
    return classify_menu_state(snapshot), snapshot


def return_to_root_menu(
    controller: RedactingSerialController,
    state: str,
    snapshot: bytes,
) -> bytes:
    """Back out of submenus to the root boot menu."""

    if state == MENU_ROOT:
        return snapshot
    if state == MENU_DHCP:
        controller.send_line("0")
        return wait_for_menu_state(controller, 20, label="root U-Boot menu")[1]
    if state == MENU_INTERFACE:
        controller.send_line("0")
        wait_for_menu_state(controller, 20, label="U-Boot DHCP mode prompt")
        controller.send_line("0")
        return wait_for_menu_state(controller, 20, label="root U-Boot menu")[1]
    if state == MENU_REVIEW:
        controller.send_line("0")
        return wait_for_menu_state(controller, 20, label="root U-Boot menu")[1]
    if state == MENU_WIFI_SETUP:
        controller.send_line("0")
        next_state, next_snapshot = wait_for_menu_state(
            controller, 20, label="U-Boot interface prompt"
        )
        return return_to_root_menu(controller, next_state, next_snapshot)
    if state == MENU_STATIC_SETUP:
        controller.send_line("0")
        next_state, next_snapshot = wait_for_menu_state(
            controller, 20, label="U-Boot interface prompt"
        )
        return return_to_root_menu(controller, next_state, next_snapshot)
    if state == MENU_RESET:
        controller.send_line("0")
        return wait_for_menu_state(controller, 20, label="root U-Boot menu")[1]
    raise RuntimeError("cannot recover to root U-Boot menu from unknown menu state")


def boot_saved_wifi(
    controller: RedactingSerialController,
    state: str,
    snapshot: bytes,
) -> None:
    """Select the saved-settings root-menu path for Wi-Fi proof boots."""

    root_snapshot = return_to_root_menu(controller, state, snapshot)
    if b"Default network settings active" in root_snapshot or (
        b"Boot with default settings" in root_snapshot
    ):
        raise RuntimeError(
            "saved Wi-Fi policy is not visible in the U-Boot root menu; "
            "refusing to boot default settings for a Wi-Fi proof lane"
        )
    if (
        b"Network: Wi-Fi" not in root_snapshot
        or b"Network: Ethernet" in root_snapshot
    ):
        raise RuntimeError(
            "saved network policy is not Wi-Fi; refusing to boot a non-Wi-Fi "
            "saved policy for a Wi-Fi proof lane"
        )
    controller.send_line("1")


def boot_genet_dhcp(
    controller: RedactingSerialController,
    state: str,
    snapshot: bytes,
) -> None:
    """Use guided setup for a one-time Ethernet DHCP boot."""

    if state == MENU_UNKNOWN:
        raise RuntimeError(
            "cannot select Ethernet lane from unknown U-Boot menu state"
        )
    if state in (MENU_WIFI_SETUP, MENU_STATIC_SETUP, MENU_RESET):
        snapshot = return_to_root_menu(controller, state, snapshot)
        state = MENU_ROOT
    if state == MENU_REVIEW:
        if (
            b"Network: Ethernet" in snapshot
            and b"IPv4: Automatic (DHCP)" in snapshot
        ):
            controller.send_line("1")
            return
        controller.send_line("3")
        state, snapshot = wait_for_menu_state(
            controller,
            20,
            label="U-Boot DHCP mode prompt",
        )
    if state == MENU_ROOT:
        controller.send_line("2")
        state, snapshot = wait_for_menu_state(
            controller,
            20,
            label="U-Boot DHCP mode prompt",
        )
    if state == MENU_INTERFACE:
        controller.send_line("0")
        state, snapshot = wait_for_menu_state(
            controller,
            20,
            label="U-Boot DHCP mode prompt",
        )
    if state == MENU_DHCP:
        controller.send_line("1")
        state, snapshot = wait_for_menu_state(
            controller,
            20,
            label="U-Boot interface prompt",
        )
    if state == MENU_INTERFACE:
        controller.send_line("1")
        state, snapshot = wait_for_menu_state(
            controller,
            20,
            label="U-Boot review prompt",
        )
    if state != MENU_REVIEW:
        raise RuntimeError(
            f"expected U-Boot review prompt before Ethernet boot, got {state}"
        )
    controller.send_line("1")


def select_lane(
    controller: RedactingSerialController,
    lane: str,
    menu_snapshot: bytes | None = None,
) -> None:
    """Select a boot lane through the Cohesix U-Boot menu."""

    if menu_snapshot is None:
        controller.note("reading U-Boot menu before selecting boot lane")
        snapshot = read_menu_snapshot(controller, 60, label="Cohesix U-Boot menu")
        state = classify_menu_state(snapshot)
    else:
        snapshot = read_menu_snapshot(
            controller,
            20,
            label="Cohesix U-Boot menu",
            initial_snapshot=menu_snapshot,
        )
        state = classify_menu_state(snapshot)
    if lane == "wifi":
        boot_saved_wifi(controller, state, snapshot)
        return
    boot_genet_dhcp(controller, state, snapshot)


def reboot_from_root(
    controller: RedactingSerialController,
    repo: pathlib.Path,
    cohsh: pathlib.Path,
    ticket_config: pathlib.Path,
    uboot_timeout_s: float,
) -> bytes:
    controller.send_line("", public_line="<clear-line>")
    try:
        controller.read_until((ROOT_PROMPT,), 5, label="root prompt after clear-line")
    except SerialMarkerTimeout as exc:
        controller.note(f"root clear-line prompt not observed; continuing error={exc}")
    controller.send_line("ping", guard_root_terminator=True)
    controller.read_until((b"OK PING",), 10, label="root ping OK")
    ticket = mint_ticket(repo, cohsh, ticket_config)
    controller.add_redaction(ticket, "<queen-ticket>")
    controller.send_line(
        f"attach queen {ticket}",
        public_line="attach queen <ticket>",
        guard_root_terminator=True,
    )
    controller.read_until(
        (b"OK ATTACH", b"OK ATTAC", b"role=queen"),
        10,
        label="Queen attach OK",
    )
    controller.send_line("reboot", guard_root_terminator=True)
    reboot_ack = controller.read_until(
        (b"OK REBOOT detail=scheduled",),
        10,
        label="scheduled reboot ACK",
    )
    if b"U-Boot " in reboot_ack or any(marker in reboot_ack for marker in ROOT_MENU_MARKERS):
        snapshot = reboot_ack
    else:
        snapshot = controller.read_until(
            (b"U-Boot ", *ROOT_MENU_MARKERS),
            uboot_timeout_s,
            label="U-Boot after reboot",
        )
    return read_menu_snapshot(
        controller,
        60,
        label="Cohesix U-Boot menu",
        initial_snapshot=snapshot,
    )


def wait_for_wifi_supervisor_terminal(
    controller: RedactingSerialController,
    initial_snapshot: bytes,
    timeout_s: float,
) -> tuple[str, bytes]:
    """Wait passively until the single Wi-Fi bootstrap attempt is terminal."""

    deadline = time.monotonic() + timeout_s
    evidence = initial_snapshot
    observed = b""
    terminal, error = inspect_wifi_supervisor_evidence(evidence)
    if error is not None:
        raise RuntimeError(f"invalid CYW43 bootstrap supervisor evidence: {error}")
    source = "boot-snapshot"
    if terminal is None:
        source = "serial-wait"
        marker_offset = wifi_terminal_marker_offset(evidence)
        if marker_offset < 0:
            chunk = controller.read_until(
                WIFI_SUPERVISOR_TERMINAL_MARKERS,
                remaining_before_deadline(
                    deadline,
                    label="terminal CYW43 bootstrap supervisor status",
                ),
                label="terminal CYW43 bootstrap supervisor status",
            )
            observed += chunk
            evidence += chunk
            marker_offset = wifi_terminal_marker_offset(evidence)
        if marker_offset >= 0 and not any(
            terminator in evidence[marker_offset:]
            for terminator in (b"\r", b"\n")
        ):
            chunk = controller.read_until(
                (b"\r", b"\n"),
                remaining_before_deadline(
                    deadline,
                    label="complete CYW43 bootstrap supervisor line",
                ),
                label="complete CYW43 bootstrap supervisor line",
            )
            observed += chunk
            evidence += chunk
        terminal, error = inspect_wifi_supervisor_evidence(evidence)
    if error is not None:
        raise RuntimeError(f"invalid CYW43 bootstrap supervisor evidence: {error}")
    if terminal is None:
        raise RuntimeError(
            "terminal CYW43 bootstrap marker did not contain one line-complete "
            "current production supervisor record"
        )
    attempt, status = terminal
    controller.note(
        "wifi supervisor terminal "
        f"attempt={attempt} status={status} source={source} "
        "action=diagnostics-admitted"
    )
    return status, observed


def issue_diagnostic_command(
    controller: RedactingSerialController,
    command: str,
    label: str,
    result_markers: tuple[bytes, ...],
    *,
    deadline: float | None = None,
) -> bytes:
    """Issue one diagnostic only after a fresh root-command boundary."""

    controller.synchronize_root_diagnostic_command(label=label, deadline=deadline)
    controller.send_line(
        command,
        reinforce_terminator=True,
        guard_root_terminator=True,
    )
    result_timeout_s = 90.0
    if deadline is not None:
        result_timeout_s = remaining_before_deadline(
            deadline,
            label=f"result for {label}",
        )
    return controller.read_until(
        result_markers,
        result_timeout_s,
        label=f"result for {label}",
    )


def wait_for_wifi_dhcp_bound(
    controller: RedactingSerialController,
    *,
    timeout_s: float,
) -> tuple[bool, list[str]]:
    """Poll guarded ``netstats`` until the selected Wi-Fi lease is current."""

    deadline = time.monotonic() + timeout_s
    max_polls = max(1, int(timeout_s / WIFI_DHCP_POLL_INTERVAL_S) + 1)
    for poll in range(1, max_polls + 1):
        label = "netstats" if poll == 1 else f"netstats-dhcp-poll-{poll}"
        try:
            snapshot = issue_diagnostic_command(
                controller,
                "netstats",
                label,
                DIAGNOSTIC_RESULT_MARKERS["netstats"],
                deadline=deadline,
            )
        except SerialMarkerTimeout as exc:
            controller.note(
                "wifi DHCP terminal result=timeout "
                f"polls={poll} phase=guarded-netstats error={exc} "
                "action=skip-premature-nettest"
            )
            return False, ["wifi-dhcp:terminal-timeout"]
        if time.monotonic() >= deadline:
            controller.note(
                "wifi DHCP terminal result=timeout "
                f"polls={poll} phase=guarded-netstats-complete "
                "action=skip-premature-nettest"
            )
            return False, ["wifi-dhcp:terminal-timeout"]
        if serial_marker_seen(
            snapshot,
            DIAGNOSTIC_RESULT_MARKERS["netstats"][1],
        ):
            controller.note(
                "diagnostic terminal command='netstats' "
                f"label={label!r} result=err action=skip-premature-nettest"
            )
            return False, [f"{label}:err"]

        status = parse_netstats_network_status(snapshot)
        if status is None:
            controller.note(
                "wifi DHCP poll "
                f"poll={poll} status=invalid-or-missing "
                "action=wait-before-nettest"
            )
        else:
            (
                generation,
                mode,
                policy,
                active,
                address_source,
                ip,
                dhcp_phase,
            ) = status
            bound = (
                mode == "dhcp"
                and policy == "wifi"
                and active == "wifi"
                and address_source == "dhcp-lease"
                and ip != "0.0.0.0"
                and dhcp_phase == "bound"
            )
            controller.note(
                "wifi DHCP poll "
                f"poll={poll} generation={generation} mode={mode} "
                f"policy={policy} active={active} "
                f"address_source={address_source} ip={ip} "
                f"dhcp={dhcp_phase} "
                f"terminal={'bound' if bound else 'no'}"
            )
            if bound:
                return True, []

        remaining_s = deadline - time.monotonic()
        if remaining_s <= 0 or poll == max_polls:
            controller.note(
                "wifi DHCP terminal result=timeout "
                f"polls={poll} action=skip-premature-nettest"
            )
            return False, ["wifi-dhcp:terminal-timeout"]
        controller.drain_for(
            min(WIFI_DHCP_POLL_INTERVAL_S, remaining_s),
            label=f"wifi DHCP progress before poll {poll + 1}",
        )
    raise AssertionError("bounded Wi-Fi DHCP poll loop fell through")


def run_diagnostics(
    controller: RedactingSerialController,
    lane: str,
    *,
    prompt_ready: bool,
    boot_snapshot: bytes = b"",
    wifi_supervisor_timeout_s: float = WIFI_SUPERVISOR_TERMINAL_TIMEOUT_S,
    wifi_dhcp_timeout_s: float = WIFI_DHCP_TERMINAL_TIMEOUT_S,
) -> bool:
    """Run every diagnostic and return whether all terminal results passed."""

    usb_scored = True
    failures: list[str] = []
    nettest_started = False
    nettest_started_run_generation: int | None = None
    nettest_final_observed = False
    nettest_async_result: tuple[int, int, str] | None = None
    supervisor_status: str | None = None
    readiness_snapshot = boot_snapshot
    if lane == "wifi":
        supervisor_status, supervisor_snapshot = wait_for_wifi_supervisor_terminal(
            controller,
            boot_snapshot,
            wifi_supervisor_timeout_s,
        )
        readiness_snapshot += supervisor_snapshot
    if prompt_ready:
        settle = controller.drain_for(
            8.0,
            label="post-root-prompt-settle-before-diagnostics",
        )
        readiness_snapshot += settle
        if not any(
            marker in readiness_snapshot for marker in DIAGNOSTIC_READY_MARKERS
        ):
            try:
                readiness_snapshot += controller.read_until(
                    DIAGNOSTIC_READY_MARKERS,
                    DIAGNOSTIC_SETTLE_TIMEOUT_S,
                    label="root command readiness before diagnostics",
                )
            except SerialMarkerTimeout as exc:
                usb_scored = False
                controller.note(
                    "diagnostics serial_only_usb_unproven_after_command_ready_timeout "
                    f"error={exc}"
                )
    if lane == "wifi":
        settled_terminal, settled_error = inspect_wifi_supervisor_evidence(
            readiness_snapshot,
            reject_trailing_partial=True,
        )
        if settled_error is not None:
            supervisor_status = f"invalid-{settled_error}"
            controller.note(
                "wifi supervisor settled evidence "
                f"result=fail reason={settled_error} "
                "action=skip-unavailable-nettest"
            )
        elif settled_terminal is None or settled_terminal[1] != supervisor_status:
            supervisor_status = "invalid-terminal-mismatch"
            controller.note(
                "wifi supervisor settled evidence "
                "result=fail reason=terminal-mismatch "
                "action=skip-unavailable-nettest"
            )
    if lane == "wifi":
        if supervisor_status == "ready":
            dhcp_bound, dhcp_failures = wait_for_wifi_dhcp_bound(
                controller,
                timeout_s=wifi_dhcp_timeout_s,
            )
            failures.extend(dhcp_failures)
            commands = (
                [
                    ("nettest", "nettest"),
                    ("netstats", "netstats-final"),
                ]
                if dhcp_bound
                else [
                    ("netstats", "netstats-final"),
                ]
            )
        else:
            failures.append(f"wifi-supervisor:{supervisor_status}")
            controller.note(
                "wifi supervisor terminal "
                f"status={supervisor_status} "
                "result=fail action=skip-unavailable-nettest"
            )
            commands = [
                ("netstats", "netstats"),
                ("netstats", "netstats-final"),
            ]
    else:
        commands = [
            ("netstats", "netstats"),
            ("nettest", "nettest"),
            ("netstats", "netstats-final"),
        ]
    if lane == "wifi":
        commands.append(("wifi diag", "wifi diag"))
    commands.extend(
        [
            ("usb diag", "usb diag"),
            ("usb probe-kbd", "usb probe-kbd"),
            ("smp activity", "smp activity"),
        ]
    )
    for command, label in commands:
        if command.startswith("usb ") and not usb_scored:
            controller.note(
                f"diagnostics serial_only_usb_unscored command={command!r}"
            )
        try:
            result_markers = (
                (NETTEST_STARTED_MARKER, b"ERR NETTEST")
                if command == "nettest"
                else DIAGNOSTIC_RESULT_MARKERS[command]
            )
            command_snapshot = issue_diagnostic_command(
                controller,
                command,
                label,
                result_markers,
            )
            error_marker = DIAGNOSTIC_RESULT_MARKERS[command][1]
            if serial_marker_seen(command_snapshot, error_marker):
                failures.append(f"{label}:err")
                controller.note(
                    f"diagnostic terminal command={command!r} label={label!r} "
                    "result=err action=continue"
                )
            elif command == "nettest" and serial_marker_seen(
                command_snapshot,
                NETTEST_STARTED_MARKER,
            ):
                # Ignore any older asynchronous result that arrived before the
                # fresh admission ACK in this snapshot, but preserve a result
                # that followed an unsplit ACK in the same serial read.
                started_offset = command_snapshot.rfind(NETTEST_STARTED_MARKER)
                post_ack_snapshot = (
                    command_snapshot[
                        started_offset + len(NETTEST_STARTED_MARKER) :
                    ]
                    if started_offset >= 0
                    else b""
                )
                nettest_started = True
                nettest_started_run_generation = (
                    parse_nettest_started_run_generation(command_snapshot)
                )
                if nettest_started_run_generation is None:
                    failures.append("nettest:started-run-generation-missing")
                    controller.note(
                        "nettest admission run_generation=missing "
                        "action=observe-and-fail-closed"
                    )
                observation = controller.drain_for(
                    NETTEST_OBSERVATION_S,
                    label="nettest terminal observation window",
                )
                nettest_async_result = parse_nettest_result(
                    post_ack_snapshot + observation
                )
                if nettest_async_result is None:
                    controller.note(
                        "nettest async-terminal absent "
                        "action=query-generation-tagged-netstats"
                    )
                else:
                    generation, run_generation, result = nettest_async_result
                    controller.note(
                        "nettest async-terminal observed "
                        f"generation={generation} "
                        f"run_generation={run_generation} result={result} "
                        "action=confirm-with-netstats"
                    )
            elif label == "netstats-final" and nettest_started:
                nettest_final_observed = True
                terminal = parse_nettest_status(command_snapshot)
                if terminal is None:
                    failures.append("nettest:terminal-netstats-missing")
                    controller.note(
                        "nettest terminal result=missing "
                        "source=netstats action=fail-closed"
                    )
                else:
                    generation, run_generation, running, result = terminal
                    controller.note(
                        "nettest terminal "
                        f"generation={generation} "
                        f"run_generation={run_generation} "
                        f"running={str(running).lower()} "
                        f"result={result} source=netstats"
                    )
                    if (
                        nettest_started_run_generation is not None
                        and run_generation != nettest_started_run_generation
                    ):
                        failures.append("nettest:run-generation-mismatch")
                        controller.note(
                            "nettest terminal mismatch "
                            f"started_run_generation="
                            f"{nettest_started_run_generation} "
                            f"netstats_run_generation={run_generation} "
                            "action=fail-closed"
                        )
                    elif (
                        nettest_async_result is not None
                        and nettest_async_result
                        != (generation, run_generation, result)
                    ):
                        (
                            async_generation,
                            async_run_generation,
                            async_result,
                        ) = nettest_async_result
                        failures.append("nettest:terminal-contract-mismatch")
                        controller.note(
                            "nettest terminal mismatch "
                            f"async_generation={async_generation} "
                            f"async_run_generation={async_run_generation} "
                            f"async_result={async_result} "
                            f"netstats_generation={generation} "
                            f"netstats_run_generation={run_generation} "
                            f"netstats_result={result} action=fail-closed"
                        )
                    elif running or result in {"none", "running"}:
                        failures.append(
                            "nettest:"
                            f"generation-{generation}-"
                            f"run-{run_generation}-not-terminal"
                        )
                    elif result == "fail":
                        failures.append(
                            "nettest:"
                            f"generation-{generation}-"
                            f"run-{run_generation}-fail"
                        )
        except SerialMarkerTimeout as exc:
            controller.note(
                f"diagnostic timeout command={command!r} label={label!r} error={exc}"
            )
            raise
    if nettest_started and not nettest_final_observed:
        failures.append("nettest:terminal-netstats-unavailable")
        controller.note(
            "nettest terminal result=unavailable "
            "source=netstats action=fail-closed"
        )
    if failures:
        controller.note(
            "diagnostics complete result=fail failures=" + ",".join(failures)
        )
        return False
    controller.note("diagnostics complete result=pass")
    return True


def run() -> int:
    args = parse_args()
    repo = args.repo.resolve()
    args.log.parent.mkdir(parents=True, exist_ok=True)

    controller = RedactingSerialController(
        args.port,
        args.baud,
        args.log,
        echo=not args.no_echo,
        char_delay_s=args.char_delay_ms / 1000,
    )
    try:
        menu_snapshot: bytes | None = None
        if args.initial_state == "root":
            controller.note("assuming root prompt is already ready")
            menu_snapshot = reboot_from_root(
                controller,
                repo,
                args.cohsh,
                args.ticket_config,
                args.uboot_timeout_s,
            )
        elif args.initial_state == "menu":
            controller.note("assuming Cohesix U-Boot menu is already displayed")
        else:
            first = controller.read_until(
                (ROOT_PROMPT, *MENU_MARKERS),
                args.initial_timeout_s,
                label="root prompt or Cohesix U-Boot menu",
            )
            if serial_marker_seen(first, ROOT_PROMPT):
                menu_snapshot = reboot_from_root(
                    controller,
                    repo,
                    args.cohsh,
                    args.ticket_config,
                    args.uboot_timeout_s,
                )
            else:
                menu_snapshot = first

        select_lane(controller, args.lane, menu_snapshot)
        build_snapshot = controller.read_until(
            (b"[BUILD]",),
            args.boot_timeout_s,
            label="fresh build marker",
        )
        root_snapshot = controller.read_until(
            (ROOT_PROMPT,),
            args.boot_timeout_s,
            label="root prompt",
        )
        if args.diagnostics and not run_diagnostics(
            controller,
            args.lane,
            prompt_ready=True,
            boot_snapshot=build_snapshot + root_snapshot,
            wifi_supervisor_timeout_s=args.boot_timeout_s,
        ):
            controller.note("complete result=diagnostic-failure exit=1")
            return 1
        controller.note("complete")
        return 0
    finally:
        controller.close()


if __name__ == "__main__":
    raise SystemExit(run())
