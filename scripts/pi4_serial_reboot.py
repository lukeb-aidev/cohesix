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
    "wifi probe-ht": (b"OK WIFI", b"ERR WIFI"),
    "usb diag": (b"OK USB", b"ERR USB"),
    "usb probe-kbd": (b"OK USB", b"ERR USB"),
    "smp activity": (b"OK SMP", b"ERR SMP"),
}
DIAGNOSTIC_READY_MARKERS = (
    b"usb keyboard command-ready",
)
DIAGNOSTIC_SETTLE_TIMEOUT_S = 30.0
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
        self._echo = echo
        self._char_delay_s = char_delay_s

    def close(self) -> None:
        if self._redaction_carry:
            self._write_safe(self._redact(self._redaction_carry))
            self._redaction_carry = b""
        self._log.close()
        self._serial.close()

    def add_redaction(self, secret: str, replacement: str) -> None:
        self._redactions.append((secret.encode(), replacement.encode()))

    def _redact(self, data: bytes) -> bytes:
        for secret, replacement in self._redactions:
            data = data.replace(secret, replacement)
        return data

    def _write_safe(self, data: bytes) -> None:
        self._log.write(data)
        self._log.flush()
        if self._echo:
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()

    def _record(self, data: bytes) -> None:
        if not self._redactions:
            self._write_safe(data)
            return
        pending = self._redaction_carry + data
        keep = max((len(secret) - 1 for secret, _ in self._redactions), default=0)
        if len(pending) <= keep:
            self._redaction_carry = pending
            return
        safe_len = len(pending) - keep
        safe = self._redact(pending[:safe_len])
        self._redaction_carry = pending[safe_len:]
        self._write_safe(safe)

    def note(self, text: str) -> None:
        self._record(f"\n[host] {text}\n".encode())

    def send_line(
        self,
        line: str,
        *,
        public_line: str | None = None,
        reinforce_terminator: bool = False,
    ) -> None:
        self.note(f"send {public_line if public_line is not None else line}")
        for byte in serial_line_bytes(line):
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
        tail = self._redact(bytes(seen[-2048:])).decode("utf-8", errors="replace")
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


def serial_line_bytes(line: str) -> bytes:
    """Encode a serial command using the Pi 4 console's CR line discipline."""

    return f"{line}{DEFAULT_LINE_TERMINATOR}".encode()


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
    controller.send_line("ping")
    controller.read_until((b"OK PING",), 10, label="root ping OK")
    ticket = mint_ticket(repo, cohsh, ticket_config)
    controller.add_redaction(ticket, "<queen-ticket>")
    controller.send_line(f"attach queen {ticket}", public_line="attach queen <ticket>")
    controller.read_until(
        (b"OK ATTACH", b"OK ATTAC", b"role=queen"),
        10,
        label="Queen attach OK",
    )
    controller.send_line("reboot")
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


def run_diagnostics(
    controller: RedactingSerialController,
    lane: str,
    *,
    prompt_ready: bool,
) -> None:
    commands = ["netstats", "nettest"]
    if lane == "wifi":
        commands.extend(["wifi diag", "wifi probe-ht"])
    commands.extend(["usb diag", "usb probe-kbd", "smp activity"])
    usb_scored = True
    if prompt_ready:
        settle = controller.drain_for(
            8.0,
            label="post-root-prompt-settle-before-diagnostics",
        )
        if not any(marker in settle for marker in DIAGNOSTIC_READY_MARKERS):
            try:
                controller.read_until(
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
    for command in commands:
        if command.startswith("usb ") and not usb_scored:
            controller.note(
                f"diagnostics serial_only_usb_unscored command={command!r}"
            )
        if prompt_ready:
            prompt_ready = False
        else:
            controller.read_until((b"cohesix>",), 30, label=f"prompt before {command}")
        controller.send_line(command, reinforce_terminator=True)
        try:
            result_snapshot = controller.read_until(
                DIAGNOSTIC_RESULT_MARKERS[command],
                90,
                label=f"result for {command}",
            )
            prompt_ready = serial_marker_seen(result_snapshot, ROOT_PROMPT)
        except SerialMarkerTimeout as exc:
            controller.note(f"diagnostic timeout command={command!r} error={exc}")
            raise


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
        controller.read_until((b"[BUILD]",), args.boot_timeout_s, label="fresh build marker")
        controller.read_until((ROOT_PROMPT,), args.boot_timeout_s, label="root prompt")
        if args.diagnostics:
            run_diagnostics(controller, args.lane, prompt_ready=True)
        controller.note("complete")
        return 0
    finally:
        controller.close()


if __name__ == "__main__":
    raise SystemExit(run())
