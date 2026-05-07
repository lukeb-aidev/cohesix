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
USB_PROGRESS_GATES = {
    "no-controller": 1,
    "controller-ready": 3,
    "command-ring-ready": 4,
    "root-port-detect": 4,
    "root-port-connected": 5,
    "device-addressed": 6,
    "address": 6,
    "device-desc": 7,
    "device-descriptor": 7,
    "config-desc": 7,
    "config-descriptor": 7,
    "config-parse": 7,
    "config-parsed": 7,
    "device-configure": 7,
    "device-configured": 7,
    "keyboard-ready": 8,
    "hid-keyboard-ready": 8,
    "first-report": 9,
    "hid-first-report": 9,
    "keyboard-report": 9,
    "first-byte": 10,
    "console-byte": 10,
}
WIFI_PROGRESS_GATES = {
    "function2-ready": 6,
    "firmware-channel-ready": 6,
    "setup-firmware-channel-ready": 6,
    "sdio-function-ready": 6,
    "firmware-ready": 7,
    "post-firmware-ready": 7,
    "control-plane-reply": 7,
    "control-plane-ready": 7,
    "init-control-plane": 7,
    "cyw43-init-control-plane-fail": 7,
    "associated": 8,
    "join-complete": 8,
    "dhcp-bound": 9,
    "dhcp-lease": 9,
    "nettest-passed": 10,
}
USB_OUTCOME_BLOCKERS = {
    "address-failed",
    "config-descriptor",
    "config-parse",
    "device-descriptor",
    "hid-first-report",
    "hid-init-failed",
    "hid-interrupt-in",
    "hid-queue-read-failed",
    "invalid-config-value",
    "keyboard-first-byte",
    "no-connected-ports",
    "no-keyboard-found",
    "root-port-sample-deferred",
    "safe-port-event-required",
    "safe-port-state",
    "set-config",
}


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
    if line.startswith("OK NETTEST") or line.startswith("ERR NETTEST"):
        return "wifi"
    if (
        line.startswith("[dhcp]")
        or line.startswith("[net-selftest]")
        or line.startswith("[net]")
        or line.startswith("netstatus")
        or line.startswith("netstats")
    ):
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
            for key in (
                "blocker",
                "cause",
                "exact",
                "exact_error",
                "focus",
                "outcome",
                "result",
                "tag",
                "verdict",
            )
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
    if "cmd-controller-not-running" in lower:
        return "cmd-controller-not-running"
    if "cmd-controller-halted" in lower:
        return "cmd-controller-halted"
    if "cmd-event-ring-timeout" in lower or "event-ring-missing" in lower:
        return "cmd-event-ring-timeout"
    if "cmd-fetch-timeout" in lower or "cmd-fetch-missing" in lower:
        return "cmd-fetch-timeout"
    if "cmd-live-timeout-snapshot-missing" in lower:
        return "cmd-live-timeout-snapshot-missing"
    if "cmd-pre-doorbell-vtimer-interrupt" in lower:
        return "cmd-pre-doorbell-timer-halt"
    if "cmd-doorbell-vtimer-interrupt" in lower:
        return "cmd-doorbell-timer-halt"
    if "cmd-pre-doorbell-timer-halt" in lower:
        return "cmd-pre-doorbell-timer-halt"
    if "cmd-doorbell-timer-halt" in lower:
        return "cmd-doorbell-timer-halt"
    if "cmd-poll-only-timeout" in lower:
        return "cmd-poll-only-timeout"
    if "command-ring" in lower:
        return "command-ring"
    if "pcie-config-replay" in lower:
        return "pcie-config-replay"
    if "root-port-sample-deferred" in lower:
        return "root-port-sample-deferred"
    if "no-connected-ports" in lower:
        return "no-connected-ports"
    if "address-failed" in lower:
        return "address-failed"
    if "invalid-config-value" in lower:
        return "invalid-config-value"
    if "hid-init-failed" in lower:
        return "hid-init-failed"
    if "queue-read" in lower and any(
        token in lower for token in ("fail", "error", "timeout")
    ):
        return "hid-queue-read-failed"
    if "first-report" in lower and any(
        token in lower for token in ("fail", "missing", "timeout")
    ):
        return "hid-first-report"
    if "interrupt-in" in lower and any(
        token in lower for token in ("fail", "missing", "timeout")
    ):
        return "hid-interrupt-in"
    if "first-byte" in lower and any(
        token in lower for token in ("fail", "missing", "timeout")
    ):
        return "keyboard-first-byte"
    if "no-keyboard" in lower or "keyboard-missing" in lower:
        return "no-keyboard-found"
    if ("device-descriptor" in lower or "device-desc" in lower) and any(
        token in lower for token in ("fail", "timeout", "missing", "error")
    ):
        return "device-descriptor"
    if ("config-descriptor" in lower or "config-desc" in lower) and any(
        token in lower for token in ("fail", "timeout", "missing", "error")
    ):
        return "config-descriptor"
    if "config-parse" in lower and any(
        token in lower for token in ("fail", "timeout", "missing", "error")
    ):
        return "config-parse"
    if "set-config" in lower and any(
        token in lower for token in ("fail", "timeout", "missing", "error")
    ):
        return "set-config"
    if "safe-port-event-required" in lower:
        return "safe-port-event-required"
    if "safe-port-state" in lower:
        return "safe-port-state"
    if "root-port-detect" in lower and any(
        token in lower for token in ("fail", "timeout", "missing", "deferred")
    ):
        return "root-port-detect"
    if "no-op-timeout" in lower:
        return "cmd-poll-only-timeout"
    if "enable-slot-timeout" in lower:
        return "address-failed"
    if "enable-slot-cmd-fail" in lower or "no-op-command-failed" in lower:
        return "command-ring"
    if "no-controller-edge-yet" in lower:
        return "no-controller-edge-yet"
    return value


def usb_progress_gate(value: str | None) -> int | None:
    """Return the USB gate proved by a progress label, if any."""

    if value is None:
        return None
    label = value.lower().strip().replace("_", "-")
    return USB_PROGRESS_GATES.get(label)


def parse_hex_int(value: str | None) -> int | None:
    """Parse a decimal or hex integer field value, returning None on absence."""

    if value is None:
        return None
    try:
        return int(value, 0)
    except ValueError:
        return None


def normalize_wifi_blocker(value: str) -> str:
    """Normalize WiFi blocker strings into stable gate labels."""

    lower = value.lower()
    if "ht-backplane-cmd53-r5-rejected" in lower:
        return "ht-backplane-cmd53-r5-rejected"
    if "sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower:
        return "ht-backplane-cmd53-r5-rejected"
    if "ht-backplane-cmd53-data-wait" in lower:
        return "ht-backplane-cmd53-data-wait"
    if "ht-backplane-cmd52-r5-rejected" in lower:
        return "ht-backplane-cmd52-r5-rejected"
    if "ht-backplane-cmd52-unreadable" in lower:
        return "ht-backplane-cmd52-unreadable"
    if "diagnostic-ht-timeout-backplane-cmd52-rejected" in lower:
        return "ht-backplane-cmd52-r5-rejected"
    if "diagnostic-ht-timeout-backplane-unreadable" in lower:
        if "mode=cmd53" in lower or "cmd53-byte-after-cmd52" in lower:
            return "ht-backplane-cmd53-data-wait"
        if "mode=cmd52" in lower:
            return "ht-backplane-cmd52-unreadable"
        return "ht-backplane-cmd53-data-wait"
    if "armcr4-release-readback-unavailable" in lower:
        return "armcr4-release-readback-unavailable"
    if "firmware-channel" in lower or "channel-f2" in lower:
        return "firmware-channel-f2"
    if "firmware-ready" in lower and any(
        token in lower for token in ("timeout", "missing", "failed", "fail")
    ):
        return "firmware-ready-timeout"
    if "mailbox" in lower and any(
        token in lower for token in ("timeout", "missing", "failed", "fail")
    ):
        return "mailbox-ready-timeout"
    if "sdpcm-credit" in lower or "credit-timeout" in lower:
        return "sdpcm-credit-timeout"
    if "ioctl-timeout" in lower or "ioctl timeout" in lower:
        return "ioctl-timeout"
    if "control-plane" in lower:
        if "sideband" in lower and any(
            token in lower for token in ("unreadable", "timeout", "missing")
        ):
            return "control-plane-sideband-unreadable"
        if "interrupt" in lower and "deferred" in lower:
            return "control-plane-interrupts-deferred"
        if "rearm-timeout" in lower:
            return "control-plane-rearm-timeout"
        if "reply-timeout" in lower or "startup-link" in lower:
            return "control-plane-startup-link-timeout"
        if "no-reply" in lower:
            return "control-plane-no-reply"
        return "control-plane"
    if (
        "join-timeout" in lower
        or ("join" in lower and "timeout" in lower)
        or "association-timeout" in lower
    ):
        return "join-timeout"
    if "wifi-association-failed" in lower or (
        "association" in lower and "failed" in lower
    ):
        return "wifi-association-failed"
    if "dhcp-pending" in lower:
        return "dhcp-pending"
    if lower in {"discover-timeout", "request-timeout", "lease-expired"}:
        return "dhcp-failed"
    if "dhcp-failed" in lower or ("dhcp" in lower and "failed" in lower):
        return "dhcp-failed"
    if lower.startswith("not-ready:"):
        return f"net-{lower.replace(':', '-')}"
    if "policy-disabled" in lower:
        return "nettest-policy-disabled"
    if "selftest-disabled" in lower:
        return "nettest-selftest-disabled"
    if lower == "unsupported" or "detail=unsupported" in lower:
        return "nettest-unsupported"
    if "nettest" in lower and any(
        token in lower for token in ("failed", "error", "timeout")
    ):
        return "nettest-failed"
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


def wifi_progress_gate(value: str | None) -> int | None:
    """Return the WiFi gate proved by a progress label, if any."""

    if value is None:
        return None
    label = value.lower().strip().replace("_", "-")
    return WIFI_PROGRESS_GATES.get(label)


def wifi_ht_runtime_evidence(
    raw: str, fields: dict[str, str], explicit_blocker: str | None
) -> bool:
    """Return true when a WiFi line proves the HT request/readback path ran."""

    if explicit_blocker == "ht-clock-timeout":
        return True
    if fields.get("ht_req") == "yes":
        return True
    if fields.get("ht_avail") in {"yes", "ready", "no"} and "ht_state" in raw:
        return True
    if any(
        token in raw
        for token in (
            "active-ht-request",
            "active-ht-stable-timeout",
            "active-ht-terminal-timeout",
            "diagnostic-force-ht-readback",
            "diagnostic-force-ht-timeout",
            "status=active-ht-request-readback",
            "status=active-ht-terminal-timeout",
        )
    ):
        return True
    return False


def summarize_usb_gate(events: Iterable[TraceEvent]) -> tuple[int, str]:
    """Summarize the USB xHCI proof gate from normalized events."""

    usb_events = [event for event in events if event.domain == "usb"]
    if not usb_events:
        return 0, "missing"

    gate = 1
    blocker = "unknown"
    saw_command_doorbell = False
    saw_command_event_ring_before = False
    saw_command_timeout_plan = False
    command_timeout_detail: str | None = None
    precise_command_timeout_details = {
        "cmd-fetch-timeout",
        "cmd-event-ring-timeout",
        "cmd-controller-not-running",
        "cmd-controller-halted",
    }
    for event in usb_events:
        raw = event.raw.lower()
        fields = event.fields
        tag = fields.get("tag", "")
        for key in ("progress", "phase", "current", "outcome"):
            progress_gate = usb_progress_gate(fields.get(key))
            if progress_gate is not None:
                gate = max(gate, progress_gate)
                if progress_gate >= 8:
                    blocker = "none"
        expected_gate = usb_progress_gate(fields.get("expected"))
        if expected_gate is not None:
            gate = max(gate, min(expected_gate, 4))
        connected_mask = parse_hex_int(fields.get("connected_mask"))
        if connected_mask is not None and connected_mask != 0:
            gate = max(gate, 5)
        if "usb root-enum failed" in raw:
            stage = fields.get("stage", "").lower()
            if stage == "address":
                gate = max(gate, 5)
                blocker = "address-failed"
                continue
            if stage in {"device-desc", "device-descriptor"}:
                gate = max(gate, 6)
                blocker = "device-descriptor"
                continue
            if stage in {"config-desc", "config-descriptor"}:
                gate = max(gate, 7)
                blocker = "config-descriptor"
                continue
            if stage == "config-parse":
                gate = max(gate, 7)
                blocker = "config-parse"
                continue
            if stage.startswith("set-config"):
                gate = max(gate, 7)
                blocker = "set-config"
                continue
        if "usb hid keyboard ready" in raw:
            gate = max(gate, 8)
            blocker = "none"
        if "usb hid attach failed" in raw:
            gate = max(gate, 7)
            blocker = "hid-init-failed"
            continue
        if "usb hid queue-read failed" in raw:
            gate = max(gate, 7)
            blocker = "hid-queue-read-failed"
            continue
        if "usb hid first report pending" in raw:
            gate = max(gate, 8)
            blocker = "hid-first-report"
            continue
        if "usb hid first report" in raw:
            gate = max(gate, 9)
            blocker = "none"
        if "runtime keyboard first-byte" in raw:
            gate = max(gate, 10)
            blocker = "none"
        if "cfg_window=mapped" in raw or "selected cfg=hal-ext" in raw:
            gate = max(gate, 2)
        if "controller-ready" in raw or "controller-init-complete" in raw:
            gate = max(gate, 3)
        if tag == "cmd-submit":
            saw_command_doorbell = False
            saw_command_event_ring_before = False
            saw_command_timeout_plan = False
            gate = max(gate, 3)
        if (
            "cmd-poll-only-timeout" in raw
            or tag == "cmd-poll-only-timeout"
            or fields.get("exact") == "cmd-poll-only-timeout"
            or fields.get("verdict") == "command-ring-edge"
        ):
            gate = max(gate, 3)
            if command_timeout_detail in precise_command_timeout_details:
                blocker = command_timeout_detail
            else:
                command_timeout_detail = "cmd-poll-only-timeout"
                blocker = command_timeout_detail
        elif tag.startswith("cmd-gate-timeout-plan"):
            gate = max(gate, 3)
            saw_command_timeout_plan = True
            if command_timeout_detail is None:
                command_timeout_detail = "cmd-live-timeout-snapshot-missing"
            blocker = command_timeout_detail
        elif tag == "cmd-gate-timeout-live-snapshot-deferred":
            gate = max(gate, 3)
            command_timeout_detail = "cmd-poll-only-timeout"
            blocker = command_timeout_detail
        elif tag == "cmd-gate-timeout-live-crcr":
            gate = max(gate, 3)
            ptr_match = parse_hex_int(fields.get("ptr_match"))
            live_crcr = parse_hex_int(fields.get("live_crcr"))
            expected_ptr = parse_hex_int(fields.get("expected_ptr"))
            if ptr_match == 1:
                command_timeout_detail = "cmd-fetch-timeout"
            elif live_crcr is not None and expected_ptr is not None:
                command_timeout_detail = "cmd-event-ring-timeout"
            else:
                command_timeout_detail = "cmd-poll-only-timeout"
            blocker = command_timeout_detail
        elif tag == "cmd-gate-timeout-live-state":
            gate = max(gate, 3)
            usbcmd_usbsts = parse_hex_int(fields.get("usbcmd_usbsts"))
            if usbcmd_usbsts is not None:
                usbcmd = (usbcmd_usbsts >> 32) & 0xFFFF_FFFF
                usbsts = usbcmd_usbsts & 0xFFFF_FFFF
                if (usbcmd & 0x1) == 0:
                    command_timeout_detail = "cmd-controller-not-running"
                    blocker = command_timeout_detail
                elif (usbsts & 0x1) != 0:
                    command_timeout_detail = "cmd-controller-halted"
                    blocker = command_timeout_detail
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
            saw_command_timeout_plan
            and "kernel entry via interrupt" in raw
            and "irq 27" in raw
        ):
            gate = max(gate, 3)
            blocker = command_timeout_detail or "cmd-live-timeout-snapshot-missing"
        elif (
            saw_command_doorbell
            and "kernel entry via interrupt" in raw
            and "irq 27" in raw
        ):
            gate = max(gate, 3)
            blocker = "cmd-doorbell-timer-halt"
        elif (
            saw_command_event_ring_before
            and not saw_command_doorbell
            and "kernel entry via interrupt" in raw
            and "irq 27" in raw
        ):
            gate = max(gate, 3)
            blocker = "cmd-pre-doorbell-timer-halt"
        elif (
            fields.get("command_probe", "").endswith("-ok")
            or (
                fields.get("result", "").endswith("-ok")
                and "command-probe" in raw
            )
            or fields.get("verdict", "").startswith("command-ring-ready")
        ):
            gate = max(gate, 4)
            outcome_blocker = normalize_usb_blocker(fields.get("outcome", "none"))
            if outcome_blocker in USB_OUTCOME_BLOCKERS:
                blocker = outcome_blocker
            else:
                blocker = "none"
        else:
            for key in ("blocker", "cause", "exact", "outcome", "result", "verdict"):
                value = fields.get(key)
                if value and value not in {"none", "n/a"}:
                    blocker = normalize_usb_blocker(value)
            focus = fields.get("focus")
            if focus and focus not in {"none", "n/a"} and blocker in {
                "unknown",
                "none",
            }:
                blocker = normalize_usb_blocker(focus)

    return gate, blocker


def summarize_wifi_gate(events: Iterable[TraceEvent]) -> tuple[int, str]:
    """Summarize the WiFi CYW43455 proof gate from HT through nettest."""

    wifi_events = [event for event in events if event.domain == "wifi"]
    if not wifi_events:
        return 0, "missing"

    gate = 1
    blocker = "unknown"
    precise_ht_blockers = {
        "devon-timeout",
        "ht-backplane-cmd53-r5-rejected",
        "ht-backplane-cmd53-data-wait",
        "ht-backplane-cmd52-r5-rejected",
        "ht-backplane-cmd52-unreadable",
    }
    ht_available_seen = False
    for event in wifi_events:
        raw = event.raw.lower()
        fields = event.fields
        explicit_blocker = None
        for key in (
            "reason",
            "detail",
            "err",
            "outcome",
            "blocker",
            "cause",
            "exact",
            "exact_error",
        ):
            value = fields.get(key)
            if value and value not in {"none", "n/a"}:
                explicit_blocker = normalize_wifi_blocker(value)
        for key in ("stage", "current", "outcome", "focus", "result", "source"):
            progress_gate = wifi_progress_gate(fields.get(key))
            if progress_gate is not None:
                gate = max(gate, progress_gate)
        if "sdio function-ready" in raw and fields.get("fn") == "2":
            gate = max(gate, 6)
        if "function2 ready-snapshot" in raw:
            gate = max(gate, 6)
        if fields.get("f2_enabled") == "yes" and fields.get("f2_ready") == "yes":
            gate = max(gate, 6)
        if "setup-firmware-channel-ready" in raw:
            gate = max(gate, 6)
        if "firmware-ready" in raw and "timeout" not in raw and "fail" not in raw:
            gate = max(gate, 7)
        if "control-plane step=init-complete action=ready" in raw or "[cyw43] ready:" in raw:
            gate = max(gate, 7)
            blocker = "none"
        if "control-plane step=" in raw and " action=fail" in raw:
            gate = max(gate, 7)
            blocker = explicit_blocker or "control-plane"
            continue
        if "join complete" in raw:
            gate = max(gate, 8)
            blocker = "none"
        if "join failed" in raw:
            gate = max(gate, 7)
            blocker = normalize_wifi_blocker(raw)
            continue
        if (
            "dhcp] lease bound" in raw
            or fields.get("addr_src") == "dhcp-lease"
            or fields.get("src") == "dhcp-lease"
        ):
            gate = max(gate, 9)
            blocker = "none"
        if "[dhcp] failed" in raw or "[dhcp] send failed" in raw:
            gate = max(gate, 8)
            blocker = "dhcp-failed"
            continue
        if explicit_blocker in {"dhcp-pending", "dhcp-failed"}:
            gate = max(gate, 8)
            blocker = explicit_blocker
            continue
        if "[net-selftest] starting run" in raw:
            gate = max(gate, 9)
        if "[net-selftest] result" in raw:
            tx_ok = fields.get("tx_ok")
            udp_ok = fields.get("udp_echo_ok")
            tcp_ok = fields.get("tcp_ok")
            console_ok = fields.get("console_ok")
            if {tx_ok, udp_ok, tcp_ok, console_ok} <= {"true", "1"}:
                gate = max(gate, 10)
                blocker = "none"
            else:
                gate = max(gate, 9)
                blocker = "nettest-failed"
            continue
        if raw.startswith("ok nettest"):
            gate = max(gate, 10)
            blocker = "none"
            continue
        if raw.startswith("err nettest"):
            if explicit_blocker:
                blocker = explicit_blocker
            else:
                blocker = "nettest-failed"
            if blocker in {"dhcp-pending", "dhcp-failed"}:
                gate = max(gate, 8)
            elif blocker.startswith("net-not-ready") or blocker.startswith(
                "nettest-"
            ):
                gate = max(gate, 9)
            continue
        if "diagnostic-ht-timeout-backplane-unreadable" in raw:
            gate = max(gate, 4)
            blocker = normalize_wifi_blocker(raw)
            continue
        if "diagnostic-ht-timeout-backplane-cmd52-rejected" in raw:
            gate = max(gate, 4)
            blocker = normalize_wifi_blocker(raw)
            continue
        if "sdio cmd53 r5 fail" in raw:
            gate = max(gate, 4)
            blocker = normalize_wifi_blocker(raw)
            continue
        if (
            "sdhci xfer error cmd=53 arg=0x15000018" in raw
            and "phase=data-wait" in raw
        ):
            gate = max(gate, 4)
            blocker = "ht-backplane-cmd53-data-wait"
            continue
        if "f1=enabled" in raw or "ioex=0x02" in raw or "iordy=0x02" in raw:
            gate = max(gate, 2)
        if fields.get("ht_avail") in {"yes", "ready"} or "ht_avail=ready" in raw:
            ht_available_seen = True
            gate = max(gate, 5)
            if blocker in {"unknown", "ht-clock-timeout"}:
                blocker = "none"
            continue
        if (
            "firmware_release" in raw
            or "armcr4_release=1" in raw
            or "rstvec=" in raw
        ):
            gate = max(gate, 3)
        if explicit_blocker == "devon-timeout":
            gate = max(gate, 4)
            blocker = explicit_blocker
            continue
        if explicit_blocker == "armcr4-release-readback-unavailable":
            gate = max(gate, 4)
            blocker = explicit_blocker
            continue
        if explicit_blocker in {
            "firmware-channel-f2",
            "firmware-ready-timeout",
            "mailbox-ready-timeout",
            "sdpcm-credit-timeout",
        }:
            gate = max(gate, 6)
            blocker = explicit_blocker
            continue
        if explicit_blocker in {
            "control-plane",
            "control-plane-interrupts-deferred",
            "control-plane-no-reply",
            "control-plane-rearm-timeout",
            "control-plane-sideband-unreadable",
            "control-plane-startup-link-timeout",
            "ioctl-timeout",
        }:
            gate = max(gate, 7)
            blocker = explicit_blocker
            continue
        if explicit_blocker in {"join-timeout", "wifi-association-failed"}:
            gate = max(gate, 7)
            blocker = explicit_blocker
            continue
        ht_evidence = wifi_ht_runtime_evidence(raw, fields, explicit_blocker)
        if (
            explicit_blocker == "ht-clock-timeout"
            or ht_evidence
        ):
            gate = max(gate, 4)
            if blocker not in precise_ht_blockers:
                blocker = "ht-clock-timeout"
        elif "firmware-verify-readback" in raw:
            gate = max(gate, 3)
            blocker = "firmware-verify-readback"
        elif explicit_blocker:
            if blocker not in precise_ht_blockers:
                blocker = explicit_blocker

    if blocker == "function2-disabled" and gate >= 4 and not ht_available_seen:
        blocker = "ht-clock-timeout"
    return gate, blocker


def summarize_gates(events: Iterable[TraceEvent]) -> GateSummary:
    """Build the current USB/WiFi hardware proof gate summary."""

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


def check_gate_min_expectations(
    summary: GateSummary, expectations: dict[str, str], stderr: TextIO
) -> bool:
    """Return true when numeric gate values meet lower-bound expectations."""

    actual = {key: str(value) for key, value in summary.to_record().items()}
    ok = True
    for key, expected_value in expectations.items():
        actual_value = actual.get(key)
        try:
            actual_number = int(actual_value or "", 10)
            expected_number = int(expected_value, 10)
        except ValueError:
            print(
                f"gate assertion failed: {key} min {expected_value} got {actual_value}",
                file=stderr,
            )
            ok = False
            continue
        if actual_number < expected_number:
            print(
                f"gate assertion failed: {key} min {expected_number} got {actual_number}",
                file=stderr,
            )
            ok = False
    return ok


def check_gate_not_expectations(
    summary: GateSummary, expectations: dict[str, str], stderr: TextIO
) -> bool:
    """Return true when all gate values differ from rejected values."""

    actual = {key: str(value) for key, value in summary.to_record().items()}
    ok = True
    for key, rejected_value in expectations.items():
        if key not in actual:
            print(
                f"gate assertion failed: unknown key {key}",
                file=stderr,
            )
            ok = False
            continue
        actual_value = actual.get(key)
        if actual_value == rejected_value:
            print(
                f"gate assertion failed: {key} rejected {rejected_value}",
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
    parser.add_argument(
        "--expect-min",
        action="append",
        default=[],
        help="assert a numeric gate KEY is at least VALUE; may be repeated with --gate-summary",
    )
    parser.add_argument(
        "--expect-not",
        action="append",
        default=[],
        help="assert a gate KEY is not VALUE; may be repeated with --gate-summary",
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
        exact_ok = check_gate_expectations(
            gate_summary, parse_expectations(args.expect), sys.stderr
        )
        min_ok = check_gate_min_expectations(
            gate_summary, parse_expectations(args.expect_min), sys.stderr
        )
        not_ok = check_gate_not_expectations(
            gate_summary, parse_expectations(args.expect_not), sys.stderr
        )
        if not (exact_ok and min_ok and not_ok):
            return 2
    elif args.summary:
        print(json.dumps(summarize_events(events), indent=2, sort_keys=True))
    else:
        if args.expect or args.expect_min or args.expect_not:
            raise SystemExit("--expect, --expect-min, and --expect-not require --gate-summary")
        write_jsonl(events, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
