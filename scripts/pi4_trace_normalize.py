#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Normalize Raspberry Pi 4 USB and WiFi serial traces for driver bring-up comparison.
# Copyright 2026 Lukas Bower

"""Normalize Pi 4 USB/WiFi bring-up logs into JSON events.

The normalizer keeps hardware traces comparable across U-Boot, Linux known-good
boots, and Cohesix root-task diagnostics. It intentionally uses only the Python
standard library so it can run from the same macOS host used for SD-card staging.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, TextIO


KEY_VALUE_RE = re.compile(
    r"(?P<key>[A-Za-z0-9_.:-]+)=(?P<value>\"[^\"]*\"|'[^']*'|[^ \t\r\n]+)"
)
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
USB_HINTS = ("usb", "xhci", "vl805", "keyboard", "local-seat")
WIFI_HINTS = ("wifi", "cyw", "brcmf", "sdio", "sdhci", "mmc")


@dataclass(frozen=True)
class TraceEvent:
    """One normalized serial trace event."""

    line: int
    domain: str
    source: str
    message: str
    raw: str
    fields: dict[str, str]
    stage: str | None = None

    def to_record(self) -> dict[str, object]:
        """Return a JSON-serializable event record."""

        record: dict[str, object] = {
            "line": self.line,
            "domain": self.domain,
            "source": self.source,
            "message": self.message,
            "raw": self.raw,
        }
        if self.stage is not None:
            record["stage"] = self.stage
        if self.fields:
            record["fields"] = self.fields
        return record


@dataclass(frozen=True)
class GateSummary:
    """Current USB/WiFi hardware bring-up gate state."""

    usb_gate: int
    usb_blocker: str
    wifi_gate: int
    wifi_blocker: str

    def to_record(self) -> dict[str, object]:
        """Return a JSON-serializable gate summary."""

        return {
            "USB_GATE": self.usb_gate,
            "USB_BLOCKER": self.usb_blocker,
            "WIFI_GATE": self.wifi_gate,
            "WIFI_BLOCKER": self.wifi_blocker,
        }

    def to_env_lines(self) -> list[str]:
        """Return stable KEY=VALUE lines for shell-friendly assertions."""

        record = self.to_record()
        return [f"{key}={value}" for key, value in record.items()]


def sanitize_line(line: str) -> str:
    """Strip terminal control noise while preserving trace payload text."""

    clean = ANSI_RE.sub("", line).replace("\r", "").strip()
    if clean.startswith("cohesix> "):
        clean = clean.removeprefix("cohesix> ").strip()
    return clean


def parse_fields(text: str) -> dict[str, str]:
    """Extract key=value fields from one trace line."""

    fields: dict[str, str] = {}
    for match in KEY_VALUE_RE.finditer(text):
        value = match.group("value")
        if (value.startswith('"') and value.endswith('"')) or (
            value.startswith("'") and value.endswith("'")
        ):
            value = value[1:-1]
        fields[match.group("key")] = value
    return fields


def classify_source(line: str, domain: str) -> str:
    """Classify which stack produced a trace line."""

    lower = line.lower()
    if "[cohesix:usb-trace]" in lower:
        return "uboot"
    if "brcmfmac" in lower or "brcmf_" in lower:
        return "linux"
    if domain == "usb" and line.startswith("[cohesix]"):
        return "uboot"
    return "cohesix"


def classify_domain(line: str) -> str | None:
    """Classify USB/WiFi trace domain, or return None for unrelated lines."""

    lower = line.lower()
    if "[cohesix:usb-trace]" in lower:
        return "usb"
    if line.startswith("usb:") or "[local-seat]" in lower:
        return "usb"
    if line.startswith("Kernel entry via Interrupt"):
        return "usb"
    if line.startswith("wifi:") or "[pi4-wifi]" in lower or "[cyw43]" in lower:
        return "wifi"
    if line.startswith("ERR NETTEST") and "cause=cyw43-" in lower:
        return "wifi"
    if "cyw43-" in lower and ("net-disabled" in lower or "net-console" in lower):
        return "wifi"
    if "brcmfmac" in lower or "brcmf_" in lower:
        return "wifi"
    if line.startswith("[cohesix]") and any(hint in lower for hint in USB_HINTS):
        return "usb"
    if line.startswith("[cohesix]") and any(hint in lower for hint in WIFI_HINTS):
        return "wifi"
    return None


def extract_message(line: str, domain: str, source: str) -> str:
    """Remove stable prefixes from a line and return the event message."""

    if "[cohesix:usb-trace]" in line:
        return line.split("[cohesix:usb-trace]", 1)[1].strip()
    if line.startswith("usb:"):
        return line.removeprefix("usb:").strip()
    if line.startswith("wifi:"):
        return line.removeprefix("wifi:").strip()
    for marker in ("[local-seat]", "[pi4-wifi]", "[cyw43]", "[cohesix]"):
        if marker in line:
            return line.split(marker, 1)[1].strip()
    if source == "linux" and domain == "wifi":
        return line
    return line


def choose_stage(fields: dict[str, str]) -> str | None:
    """Choose the most useful stage-like field for comparisons."""

    for key in ("stage", "current", "outcome", "verdict", "focus", "cause"):
        value = fields.get(key)
        if value:
            return value
    return None


def parse_line(line: str, line_number: int) -> TraceEvent | None:
    """Parse one serial line into a normalized trace event."""

    clean = sanitize_line(line)
    if not clean:
        return None
    domain = classify_domain(clean)
    if domain is None:
        return None
    source = classify_source(clean, domain)
    fields = parse_fields(clean)
    message = extract_message(clean, domain, source)
    return TraceEvent(
        line=line_number,
        domain=domain,
        source=source,
        message=message,
        raw=clean,
        fields=fields,
        stage=choose_stage(fields),
    )


def parse_events(lines: Iterable[str]) -> list[TraceEvent]:
    """Parse all relevant trace lines from an iterable of log lines."""

    events: list[TraceEvent] = []
    for line_number, line in enumerate(lines, start=1):
        event = parse_line(line, line_number)
        if event is not None:
            events.append(event)
    return events


def filter_events(events: Iterable[TraceEvent], domains: set[str]) -> list[TraceEvent]:
    """Filter events by domain when requested."""

    if not domains:
        return list(events)
    return [event for event in events if event.domain in domains]


def summarize_events(events: Iterable[TraceEvent]) -> dict[str, object]:
    """Build a compact summary for quick boot-to-boot comparison."""

    event_list = list(events)
    domain_counts = Counter(event.domain for event in event_list)
    stage_counts = Counter(
        f"{event.domain}:{event.stage}" for event in event_list if event.stage
    )
    latest: dict[str, dict[str, object]] = {}
    for event in event_list:
        latest[event.domain] = event.to_record()
    blockers = [
        event.to_record()
        for event in event_list
        if any(
            key in event.fields
            for key in ("blocker", "cause", "exact", "exact_error", "outcome", "verdict")
        )
    ]
    return {
        "events": len(event_list),
        "domains": dict(sorted(domain_counts.items())),
        "stages": dict(sorted(stage_counts.items())),
        "latest": latest,
        "blockers": blockers[-16:],
        "gates": summarize_gates(event_list).to_record(),
    }


def normalize_usb_blocker(value: str) -> str:
    """Normalize USB blocker strings into stable gate labels."""

    lower = value.lower()
    if "cmd-pre-doorbell-vtimer-interrupt" in lower:
        return "cmd-pre-doorbell-vtimer-interrupt"
    if "cmd-doorbell-vtimer-interrupt" in lower:
        return "cmd-doorbell-vtimer-interrupt"
    if "cmd-poll-only-timeout" in lower:
        return "cmd-poll-only-timeout"
    if "command-ring" in lower:
        return "command-ring"
    if "pcie-config-replay" in lower:
        return "pcie-config-replay"
    if "root-port-sample-deferred" in lower:
        return "root-port-sample-deferred"
    if "no-controller-edge-yet" in lower:
        return "no-controller-edge-yet"
    return value


def normalize_wifi_blocker(value: str) -> str:
    """Normalize WiFi blocker strings into stable gate labels."""

    lower = value.lower()
    if "armcr4-release-readback-unavailable" in lower:
        return "armcr4-release-readback-unavailable"
    if (
        "device-on" in lower
        or "sleepcsr-devon" in lower
        or "devon-timeout" in lower
        or (
            "devon" in lower
            and any(token in lower for token in ("timeout", "missing", "absent"))
        )
    ):
        return "devon-timeout"
    if "ht-clock" in lower or "ht-avail" in lower:
        return "ht-clock-timeout"
    if "firmware-verify-readback" in lower:
        return "firmware-verify-readback"
    if "function2-disabled" in lower:
        return "function2-disabled"
    if "firmware-verify-mismatch" in lower:
        return "firmware-verify-mismatch"
    if "firmware" in lower:
        return "firmware-load"
    return value


def summarize_usb_gate(events: Iterable[TraceEvent]) -> tuple[int, str]:
    """Summarize the USB xHCI proof gate from normalized events."""

    usb_events = [event for event in events if event.domain == "usb"]
    if not usb_events:
        return 0, "missing"

    gate = 1
    blocker = "unknown"
    saw_command_doorbell = False
    saw_command_event_ring_before = False
    for event in usb_events:
        raw = event.raw.lower()
        fields = event.fields
        tag = fields.get("tag", "")
        if "cfg_window=mapped" in raw or "selected cfg=hal-ext" in raw:
            gate = max(gate, 2)
        if "controller-ready" in raw or "controller-init-complete" in raw:
            gate = max(gate, 3)
        if tag == "cmd-submit":
            saw_command_doorbell = False
            saw_command_event_ring_before = False
            gate = max(gate, 3)
        if (
            "cmd-poll-only-timeout" in raw
            or tag == "cmd-poll-only-timeout"
            or fields.get("exact") == "cmd-poll-only-timeout"
            or fields.get("verdict") == "command-ring-edge"
        ):
            gate = max(gate, 3)
            blocker = "cmd-poll-only-timeout"
        elif tag.startswith("cmd-event-ring-before"):
            saw_command_event_ring_before = True
            gate = max(gate, 3)
        elif tag in {
            "cmd-doorbell-write",
            "cmd-doorbell-write-done",
            "cmd-doorbell-post-barrier",
        }:
            saw_command_doorbell = True
            gate = max(gate, 3)
        elif (
            saw_command_doorbell
            and "kernel entry via interrupt" in raw
            and "irq 27" in raw
        ):
            gate = max(gate, 3)
            blocker = "cmd-doorbell-vtimer-interrupt"
        elif (
            saw_command_event_ring_before
            and not saw_command_doorbell
            and "kernel entry via interrupt" in raw
            and "irq 27" in raw
        ):
            gate = max(gate, 3)
            blocker = "cmd-pre-doorbell-vtimer-interrupt"
        elif fields.get("command_probe", "").endswith("-ok") or fields.get(
            "verdict", ""
        ).startswith("command-ring-ready"):
            gate = max(gate, 4)
            blocker = "none"
        else:
            for key in ("blocker", "cause", "exact", "outcome", "verdict", "focus"):
                value = fields.get(key)
                if value and value not in {"none", "n/a"}:
                    blocker = normalize_usb_blocker(value)

    return gate, blocker


def summarize_wifi_gate(events: Iterable[TraceEvent]) -> tuple[int, str]:
    """Summarize the WiFi CYW43455 HT-clock proof gate from events."""

    wifi_events = [event for event in events if event.domain == "wifi"]
    if not wifi_events:
        return 0, "missing"

    gate = 1
    blocker = "unknown"
    for event in wifi_events:
        raw = event.raw.lower()
        fields = event.fields
        explicit_blocker = None
        for key in ("blocker", "cause", "exact", "exact_error", "outcome"):
            value = fields.get(key)
            if value and value not in {"none", "n/a"}:
                explicit_blocker = normalize_wifi_blocker(value)
        if "f1=enabled" in raw or "ioex=0x02" in raw or "iordy=0x02" in raw:
            gate = max(gate, 2)
        if (
            "firmware_release" in raw
            or "armcr4_release=1" in raw
            or "rstvec=" in raw
        ):
            gate = max(gate, 4)
        if explicit_blocker == "devon-timeout":
            gate = max(gate, 4)
            blocker = explicit_blocker
            continue
        if (
            "ht-clock" in raw
            or "ht-avail" in raw
            or fields.get("focus") == "wait-ht-clock"
            or fields.get("current") == "wait-ht-clock"
        ):
            gate = max(gate, 4)
            if blocker != "devon-timeout":
                blocker = "ht-clock-timeout"
        elif "firmware-verify-readback" in raw:
            gate = max(gate, 3)
            blocker = "firmware-verify-readback"
        elif "ht_avail=yes" in raw or "ht_avail=ready" in raw:
            gate = max(gate, 5)
            blocker = "none"
        elif explicit_blocker:
            blocker = explicit_blocker

    if blocker == "function2-disabled" and gate >= 4:
        blocker = "ht-clock-timeout"
    return gate, blocker


def summarize_gates(events: Iterable[TraceEvent]) -> GateSummary:
    """Build the current narrow USB/WiFi hardware proof gate summary."""

    event_list = list(events)
    usb_gate, usb_blocker = summarize_usb_gate(event_list)
    wifi_gate, wifi_blocker = summarize_wifi_gate(event_list)
    return GateSummary(
        usb_gate=usb_gate,
        usb_blocker=usb_blocker,
        wifi_gate=wifi_gate,
        wifi_blocker=wifi_blocker,
    )


def parse_expectations(expectations: Iterable[str]) -> dict[str, str]:
    """Parse KEY=VALUE gate expectations from CLI arguments."""

    parsed: dict[str, str] = {}
    for expectation in expectations:
        key, separator, value = expectation.partition("=")
        if not separator or not key or not value:
            raise SystemExit(f"invalid expectation, use KEY=VALUE: {expectation}")
        parsed[key] = value
    return parsed


def check_gate_expectations(
    summary: GateSummary, expectations: dict[str, str], stderr: TextIO
) -> bool:
    """Return true when all expected gate values match."""

    actual = {key: str(value) for key, value in summary.to_record().items()}
    ok = True
    for key, expected_value in expectations.items():
        actual_value = actual.get(key)
        if actual_value != expected_value:
            print(
                f"gate assertion failed: {key} expected {expected_value} got {actual_value}",
                file=stderr,
            )
            ok = False
    return ok


def read_input(path: str) -> list[str]:
    """Read input lines from a file path or stdin marker."""

    if path == "-":
        return sys.stdin.readlines()
    log_path = Path(path)
    if not log_path.is_file():
        raise SystemExit(f"trace log not found: {log_path}")
    return log_path.read_text(encoding="utf-8", errors="replace").splitlines()


def write_jsonl(events: Iterable[TraceEvent], output: TextIO) -> None:
    """Write normalized events as JSON Lines."""

    for event in events:
        output.write(json.dumps(event.to_record(), sort_keys=True) + "\n")


def build_parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""

    parser = argparse.ArgumentParser(
        description="Normalize Pi 4 USB/WiFi serial traces into JSON events."
    )
    parser.add_argument("log", help="serial log path, or '-' for stdin")
    parser.add_argument(
        "--domain",
        choices=("usb", "wifi"),
        action="append",
        default=[],
        help="limit output to a domain; may be repeated",
    )
    parser.add_argument(
        "--summary",
        action="store_true",
        help="emit a compact JSON summary instead of JSON Lines",
    )
    parser.add_argument(
        "--gate-summary",
        action="store_true",
        help="emit stable USB/WiFi gate KEY=VALUE lines instead of JSON Lines",
    )
    parser.add_argument(
        "--expect",
        action="append",
        default=[],
        help="assert a gate KEY=VALUE value; may be repeated with --gate-summary",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    """CLI entry point."""

    parser = build_parser()
    args = parser.parse_args(argv)
    events = filter_events(parse_events(read_input(args.log)), set(args.domain))
    if args.gate_summary:
        gate_summary = summarize_gates(events)
        print("\n".join(gate_summary.to_env_lines()))
        if not check_gate_expectations(
            gate_summary, parse_expectations(args.expect), sys.stderr
        ):
            return 2
    elif args.summary:
        print(json.dumps(summarize_events(events), indent=2, sort_keys=True))
    else:
        if args.expect:
            raise SystemExit("--expect requires --gate-summary")
        write_jsonl(events, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
