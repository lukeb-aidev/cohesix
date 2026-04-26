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
    if line.startswith("wifi:") or "[pi4-wifi]" in lower or "[cyw43]" in lower:
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
    }


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
    return parser


def main(argv: list[str] | None = None) -> int:
    """CLI entry point."""

    parser = build_parser()
    args = parser.parse_args(argv)
    events = filter_events(parse_events(read_input(args.log)), set(args.domain))
    if args.summary:
        print(json.dumps(summarize_events(events), indent=2, sort_keys=True))
    else:
        write_jsonl(events, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
