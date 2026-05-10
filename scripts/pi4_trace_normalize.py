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
UNSUPPORTED_OPERATION_FIELD_RE = re.compile(
    r"(?P<key>[A-Za-z0-9_.:-]+)=unsupported operation: "
    r"(?P<value>[A-Za-z0-9_.:-]+)"
)
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
WIFI_SECRET_REDACTIONS = (
    re.compile(r"(?i)(coh_wifi_psk=)([^ \t\r\n;]+)"),
    re.compile(r"(?i)(cohesix,wifi-psk=)([^ \t\r\n;]+)"),
    re.compile(r"(?i)(setenv\s+coh_wifi_psk\s+)([^;\r\n]+)"),
    re.compile(r"(?i)(Wi-Fi PSK \(blank for open network\):\s*)(.*)"),
)
TRACE_SEGMENT_RE = re.compile(
    r"(?=(?:"
    r"\[cohesix:usb-trace\]"
    r"|\[local-seat\]"
    r"|\[pi4-wifi\]"
    r"|\[cyw43\]"
    r"|\[dhcp\]"
    r"|\[net-selftest\]"
    r"|\[net\]"
    r"|(?<![A-Za-z0-9_.:-])(?:usb:|USB:|wifi:|WiFi:|WIFI:)"
    r"|(?<![A-Za-z0-9_.:-])(?:OK|ERR) NETTEST"
    r"|Kernel entry via Interrupt"
    r"))"
)
MALFORMED_WIFI_PREFIX_RE = re.compile(r"(?<![A-Za-z0-9_.:-])(?:wif|wi):")
USB_HINTS = ("usb", "xhci", "vl805", "keyboard", "local-seat", "usbhid")
WIFI_HINTS = ("wifi", "wi-fi", "wlan", "cyw", "brcmf", "sdio", "sdhci", "mmc")
BOOT_CHAIN_ROOT_MARKERS = (
    "u-boot ",
)
BOOT_CHAIN_CONTINUATION_MARKERS = (
    "starting kernel ...",
    "elf-loader started",
)
BOOT_START_MARKERS = (
    "bootstrapping kernel",
    "booting all finished, dropped to user space",
    "[kernel:entry] root-task entry reached",
    "[cohesix:root-task] cohesix boot: root-task online",
)
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
    "root-port-read-begin",
    "root-port-read-timer-preempted",
    "root-port-sample-deferred",
    "reset-pre-usbcmd-source",
    "reset-pre-usbcmd-source-timer-preempted",
    "port-register-access-disabled",
    "port-reset-timeout",
    "port-enable-timeout",
    "root-port-device-not-found",
    "address-device-timeout",
    "address-device-pending",
    "cmd-poll-pending",
    "cmd-doorbell-write-halt",
    "cmd-timeout",
    "safe-port-event-required",
    "safe-port-state",
    "set-config",
}
USB_BOOTLOADER_HANDOFF_FIELD_KEYS = {
    "policy",
    "origin",
    "seed",
    "handoff",
    "run",
    "route",
    "outcome",
    "publish_guard",
}
USB_BOOTLOADER_HANDOFF_VALUES = {
    "bootloader-authorized",
    "bootloader-owned",
    "cold-start-from-snapshot",
    "fw-handoff-cold-start-from-snapshot",
    "preserve-controller-state",
    "preserve-state",
    "reset-owned-stop-seed",
    "run-uboot",
    "seeded-cold-start",
    "stop-seed",
    "stop-state",
    "stop-state-preserve",
    "uboot-first",
}
USB_COLD_BOOT_MARKERS = (
    "xhci state discarded before cohesix cold boot",
    "xhci cold boot starts unseeded",
)
USB_STALE_UEFI_HINT_MARKERS = (
    "uefi vars: xhcipci=0 xhcireload=1 systemtablemode=1",
)


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
    wifi_exact: str = "none"
    wifi_phase: str = "none"
    wifi_blocker_line: int = 0
    serial_clean: bool = True
    boot_halted: bool = False
    timer_irq27_seen: bool = False
    boot_halt_reason: str = "none"
    usb_bootloader_handoff_seen: bool = False
    usb_cold_boot_seen: bool = False
    usb_stale_uefi_hint_seen: bool = False

    def to_record(self) -> dict[str, object]:
        """Return a JSON-serializable gate summary."""

        return {
            "USB_GATE": self.usb_gate,
            "USB_BLOCKER": self.usb_blocker,
            "WIFI_GATE": self.wifi_gate,
            "WIFI_BLOCKER": self.wifi_blocker,
            "WIFI_EXACT": self.wifi_exact,
            "WIFI_PHASE": self.wifi_phase,
            "WIFI_BLOCKER_LINE": self.wifi_blocker_line,
            "SERIAL_CLEAN": "yes" if self.serial_clean else "no",
            "BOOT_HALTED": "yes" if self.boot_halted else "no",
            "TIMER_IRQ27_SEEN": "yes" if self.timer_irq27_seen else "no",
            "BOOT_HALT_REASON": self.boot_halt_reason,
            "USB_BOOTLOADER_HANDOFF_SEEN": (
                "yes" if self.usb_bootloader_handoff_seen else "no"
            ),
            "USB_COLD_BOOT_SEEN": "yes" if self.usb_cold_boot_seen else "no",
            "USB_STALE_UEFI_HINT_SEEN": (
                "yes" if self.usb_stale_uefi_hint_seen else "no"
            ),
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


def redact_sensitive_line(line: str) -> str:
    """Redact Wi-Fi secrets before normalized trace records are emitted."""

    redacted = line
    for pattern in WIFI_SECRET_REDACTIONS:
        redacted = pattern.sub(r"\1<redacted>", redacted)
    return redacted


def split_trace_segments(line: str) -> list[str]:
    """Split physical UART lines that contain multiple trace producers."""

    clean = sanitize_line(line)
    if not clean:
        return []
    matches = [match.start() for match in TRACE_SEGMENT_RE.finditer(clean)]
    if not matches:
        return [clean]

    positions: list[int] = []
    if matches[0] > 0:
        positions.append(0)
    positions.extend(matches)
    positions.append(len(clean))

    segments: list[str] = []
    for start, end in zip(positions, positions[1:]):
        segment = clean[start:end].strip()
        if segment:
            segments.append(segment)
    return segments


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
    for match in UNSUPPORTED_OPERATION_FIELD_RE.finditer(text):
        fields[match.group("key")] = match.group("value")
    return fields


def usb_bootloader_handoff_evidence(event: TraceEvent) -> bool:
    """Return true when a USB event carries active bootloader handoff state."""

    if event.domain != "usb":
        return False
    lowered_raw = event.raw.lower()
    if (
        "xhci stop seed" in lowered_raw
        or "cohesix,xhci-usbcmd" in lowered_raw
        or "cohesix,xhci-usbsts" in lowered_raw
        or "cohesix,xhci-iman0" in lowered_raw
    ):
        return True
    if (event.stage or "").lower().startswith("handoff-"):
        return True
    lowered_fields = {key.lower(): value.lower() for key, value in event.fields.items()}
    for key in USB_BOOTLOADER_HANDOFF_FIELD_KEYS:
        value = lowered_fields.get(key)
        if value in USB_BOOTLOADER_HANDOFF_VALUES:
            return True
        if value and any(marker in value for marker in USB_BOOTLOADER_HANDOFF_VALUES):
            return True
    return False


def usb_cold_boot_evidence(event: TraceEvent) -> bool:
    """Return true when a USB event proves Cohesix-owned cold boot setup."""

    if event.domain != "usb":
        return False
    lowered_raw = event.raw.lower()
    if any(marker in lowered_raw for marker in USB_COLD_BOOT_MARKERS):
        return True
    fields = {key.lower(): value.lower() for key, value in event.fields.items()}
    return (
        fields.get("policy") in {"full-reset-start", "platform-reset-complete"}
        and fields.get("origin") in {"live-runtime-default", "mailbox-reset-complete"}
        and fields.get("handoff", "none") == "none"
        and fields.get("seed", "none") == "none"
        and fields.get("run", "run-default") in {"run-default", "run-cold"}
    )


def usb_stale_uefi_hint_evidence(event: TraceEvent) -> bool:
    """Return true when a USB event proves a stale UEFI-era image ran."""

    if event.domain != "usb":
        return False
    lowered_raw = event.raw.lower()
    return any(marker in lowered_raw for marker in USB_STALE_UEFI_HINT_MARKERS)


def serial_corruption_reason(line: str, fields: dict[str, str] | None = None) -> str | None:
    """Return a stable reason when a trace segment looks byte-interleaved."""

    lower = line.lower()
    if lower.startswith("wifi:") and len(line) > len("wifi:") and not line[len("wifi:")].isspace():
        return "wifi-prefix-glued"
    if lower.startswith("usb:") and len(line) > len("usb:") and not line[len("usb:")].isspace():
        return "usb-prefix-glued"
    if lower.count("wifi:") > 1:
        return "repeated-wifi-prefix"
    if lower.count("usb:") > 1:
        return "repeated-usb-prefix"
    if MALFORMED_WIFI_PREFIX_RE.search(lower):
        return "malformed-wifi-prefix"
    if lower.startswith("wifi:") and fields is not None and not fields:
        return "wifi-line-unparsed"
    return None


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
    if line.startswith("usb:") or line.startswith("USB:") or "[local-seat]" in lower:
        return "usb"
    if line == "halting...":
        return "kernel"
    if line.startswith("Kernel entry via Interrupt"):
        return "kernel"
    if "[pi4-wifi]" in lower and (
        "vl805-usb-hcd-power" in lower
        or "xhci-reset-notify" in lower
        or "owner=vl805-usb-hcd-power" in lower
        or "owner=xhci-reset-notify" in lower
        or (
            "mailbox power-on" in lower
            and "module=0x00000003" in lower
        )
    ):
        return "usb"
    if (
        line.startswith("wifi:")
        or line.startswith("WiFi:")
        or line.startswith("WIFI:")
        or "[pi4-wifi]" in lower
        or "[cyw43]" in lower
    ):
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

    if not line:
        return None
    line = redact_sensitive_line(line)
    early_reason = serial_corruption_reason(line)
    if early_reason is not None:
        domain = "usb" if "usb" in early_reason else "wifi"
        return TraceEvent(
            line=line_number,
            domain=domain,
            source="cohesix",
            message=f"serial-corruption reason={early_reason}",
            raw=line,
            fields={"serial_error": early_reason},
            stage="serial-corrupt",
        )
    domain = classify_domain(line)
    if domain is None:
        return None
    source = classify_source(line, domain)
    fields = parse_fields(line)
    stage = choose_stage(fields)
    message = extract_message(line, domain, source)
    if line == "halting...":
        fields = {**fields, "halt": "yes", "reason": "kernel-halt"}
        stage = "halt"
        message = "halt reason=kernel-halt"
    elif line.startswith("Kernel entry via Interrupt"):
        irq = line.rsplit("irq ", 1)[-1] if "irq " in line else "unknown"
        fields = {
            **fields,
            "interrupt": "kernel-entry",
            "irq": irq,
            "timer_irq": "yes" if irq == "27" else "no",
        }
        stage = "timer-irq" if irq == "27" else "interrupt"
        message = f"kernel-entry irq={irq}"
    late_reason = serial_corruption_reason(line, fields)
    if late_reason is not None:
        fields = {**fields, "serial_error": late_reason}
    return TraceEvent(
        line=line_number,
        domain=domain,
        source=source,
        message=message,
        raw=line,
        fields=fields,
        stage=stage,
    )


def parse_events(lines: Iterable[str], line_base: int = 0) -> list[TraceEvent]:
    """Parse all relevant trace lines from an iterable of log lines."""

    events: list[TraceEvent] = []
    for line_number, line in enumerate(lines, start=line_base + 1):
        for segment in split_trace_segments(line):
            event = parse_line(segment, line_number)
            if event is not None:
                events.append(event)
    return events


def filter_events(events: Iterable[TraceEvent], domains: set[str]) -> list[TraceEvent]:
    """Filter events by domain when requested."""

    if not domains:
        return list(events)
    return [event for event in events if event.domain in domains]


def serial_clean(events: Iterable[TraceEvent]) -> bool:
    """Return false when parsed proof evidence includes corrupted serial segments."""

    return all("serial_error" not in event.fields for event in events)


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
        "blockers": blockers[-32:],
        "serial_clean": serial_clean(event_list),
        "gates": summarize_gates(event_list).to_record(),
    }


def normalize_usb_blocker(value: str) -> str:
    """Normalize USB blocker strings into stable gate labels."""

    lower = value.lower()
    if "cmd-controller-not-running" in lower:
        return "cmd-controller-not-running"
    if "cmd-controller-halted" in lower:
        return "cmd-controller-halted"
    if "usbcmd-run-preserved-reset-bit" in lower:
        return "usbcmd-run-preserved-reset-bit"
    if "usbcmd-run-posted-flush-halt" in lower:
        return "usbcmd-run-posted-flush-halt"
    if "cmd-event-ring-timeout" in lower or "event-ring-missing" in lower:
        return "cmd-event-ring-timeout"
    if "cmd-ring-timeout" in lower:
        return "cmd-timeout"
    if "cmd-fetch-timeout" in lower or "cmd-fetch-missing" in lower:
        return "cmd-fetch-timeout"
    if "cmd-live-timeout-snapshot-missing" in lower:
        return "cmd-live-timeout-snapshot-missing"
    if "cmd-poll-pending" in lower:
        return "cmd-poll-pending"
    if "cmd-doorbell-write-halt" in lower:
        return "cmd-doorbell-write-halt"
    if (
        "no-op-unproven" in lower
        or "enable-slot-unproven" in lower
        or "enable-slot-linux-event-unproven" in lower
        or "cmd-prompt-safe-return-to-shell" in lower
    ):
        return "cmd-poll-pending"
    if (
        "cmd-submit-proof-timer-preempted" in lower
        or "cmd-submit-vtimer-interrupt" in lower
        or "cmd-submit-timer-halt" in lower
    ):
        return "cmd-submit-proof-timer-preempted"
    if (
        "cmd-pre-doorbell-proof-timer-preempted" in lower
        or "cmd-pre-doorbell-vtimer-interrupt" in lower
        or "cmd-pre-doorbell-timer-halt" in lower
    ):
        return "cmd-pre-doorbell-proof-timer-preempted"
    if (
        "raw-phys-cmd-doorbell-proof-timer-preempted" in lower
        or "cmd-raw-phys-doorbell-proof-timer-preempted" in lower
    ):
        return "raw-phys-cmd-doorbell-proof-timer-preempted"
    if "pcie-window-cmd-doorbell-proof-timer-preempted" in lower:
        return "pcie-window-cmd-doorbell-proof-timer-preempted"
    if "raw-phys-cmd-poll-only-timeout" in lower:
        return "raw-phys-cmd-poll-only-timeout"
    if "pcie-window-no-op-timeout" in lower:
        return "pcie-window-no-op-timeout"
    if (
        "cmd-doorbell-proof-timer-preempted" in lower
        or "cmd-doorbell-vtimer-interrupt" in lower
        or "cmd-doorbell-timer-halt" in lower
    ):
        return "cmd-doorbell-proof-timer-preempted"
    if "cmd-poll-only-timeout" in lower:
        return "cmd-poll-only-timeout"
    if "command-ring" in lower:
        return "command-ring"
    if "pcie-irq-quiesce-failed" in lower:
        return "pcie-irq-quiesce-failed"
    if "pcie-irq-quiesce-missing" in lower:
        return "pcie-irq-quiesce-missing"
    if "controller-gate" in lower:
        return "controller-gate"
    if "policy-skip-before-run" in lower:
        return "policy-skip-before-run"
    if "pcie-config-replay" in lower:
        return "pcie-config-replay"
    if "brcm-axi-setup-read" in lower or "stage=0x0111" in lower or "axiwra" in lower:
        return "brcm-axi-setup-read"
    if "root-port-sample-deferred" in lower:
        return "root-port-sample-deferred"
    if (
        "port-register-access-disabled" in lower
        or "platform-reset-portsc-toxic" in lower
        or "deferred-platform-reset-portsc-toxic" in lower
        or "stage=0x03f5" in lower
        or "stage=0x03f6" in lower
        or "stage=0x03f7" in lower
    ):
        return "port-register-access-disabled"
    if "root-port-read-timer-preempted" in lower:
        return "root-port-read-begin"
    if "reset-pre-usbcmd-source-timer-preempted" in lower:
        return "reset-pre-usbcmd-source"
    if "reset-pre-usbcmd-source" in lower or "stage=0x0226" in lower:
        return "reset-pre-usbcmd-source"
    if "root-port read-begin" in lower or "root-port-read-begin" in lower:
        return "root-port-read-begin"
    if "port-reset-timeout" in lower or "portresettimeout" in lower:
        return "port-reset-timeout"
    if "port-enable-timeout" in lower or "portenabletimeout" in lower:
        return "port-enable-timeout"
    if "device-not-found" in lower or "devicenotfound" in lower:
        return "root-port-device-not-found"
    if "address-device-timeout" in lower or "addressdevicetimeout" in lower:
        return "address-device-timeout"
    if "address-device-pending" in lower or "root-port-deferred-capture" in lower:
        return "address-device-pending"
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


def usb_command_probe_success(value: str) -> bool:
    """Return true for command proofs that satisfy the cold-boot poll-only gate."""

    label = value.lower().strip()
    if "linux-event-ok" in label:
        return False
    return label.endswith("-ok") or label.endswith("-ok-cleanup-failed")


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
    stripped = lower.strip()
    if (
        "d11-prereset-fgc-cmd53-r5-rejected" in lower
        or (
            "stage=d11-disable" in lower
            and (
                "terminal-disable-fail" in lower
                or "pre-upload-f1-reset-write-rejected" in lower
            )
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or (
            "base=0x18101000" in lower
            and "off=0x408" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or "arg=0x95281004" in lower
        or "arg=0x91281004" in lower
        or "arg=0x95281001" in lower
        or "arg=0x91281001" in lower
    ):
        return "d11-prereset-fgc-cmd53-r5-rejected"
    if (
        "blocker_phase=pre-f2-core-control" in lower
        or "policy=pre-f2-core-control" in lower
        or "gate=core-control-blocked-before-f2" in lower
        or "pre-upload-f1-reset-write-rejected" in lower
    ):
        return "pre-f2-core-control"
    if (
        "firmware-core-control-edge" in lower
        or "current=firmware-core-control" in lower
        or "focus=firmware-core-control" in lower
        or "expected=f1-backplane-core-control" in lower
    ):
        return "firmware-core-control"
    if (
        "chipcommon-socram-remap-cmd53-r5-rejected" in lower
        or (
            "stage=chipcommon-config-write" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or "arg=0x95802004" in lower
        or "arg=0x91802004" in lower
    ):
        return "chipcommon-socram-remap-cmd53-r5-rejected"
    if (
        "armcr4-reset-assert-cmd52-r5-rejected" in lower
        or (
            "sdio cmd52 fail" in lower
            and "op=write-no-cmd53-fallback" in lower
            and ("addr=0x0b800" in lower or "addr=0x03800" in lower)
            and "val=0x01" in lower
        )
        or (
            "sdio-cmd52-write" in lower
            and "mode=cmd52-byte-transfer-window-reset-assert" in lower
            and "base=0x18103000" in lower
            and "off=0x800" in lower
        )
    ):
        return "armcr4-reset-assert-cmd52-r5-rejected"
    if (
        "armcr4-reset-assert-cmd53-r5-rejected" in lower
        or (
            "stage=assert-reset" in lower
            and "base=0x18103000" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or (
            "base=0x18103000" in lower
            and "off=0x800" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or "arg=0x91700004" in lower
        or "arg=0x95700004" in lower
    ):
        return "armcr4-reset-assert-cmd53-r5-rejected"
    if "ht-recover-cmd5-timeout" in lower or (
        "ht-retry-sdio-card-not-ready" in lower and "phase=card-ready" in lower
    ):
        return "ht-recover-cmd5-timeout"
    if "linux-probe-pmu-write-skip" in lower or "pmu-res-reload-write-skip" in lower:
        return "linux-probe-pmu-write-skip"
    if "linux-probe-pmu-cmd53-r5-rejected" in lower:
        return "linux-probe-pmu-cmd53-r5-rejected"
    if (
        "socram-clear-reset-cmd53-r5-rejected" in lower
        or (
            "stage=clear-reset-primary" in lower
            and "base=0x18104000" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
    ):
        return "socram-clear-reset-cmd53-r5-rejected"
    if (
        "socram-assert-reset-cmd53-r5-rejected" in lower
        or (
            "stage=assert-reset" in lower
            and "base=0x18104000" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or (
            "base=0x18104000" in lower
            and "off=0x800" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
    ):
        return "socram-assert-reset-cmd53-r5-rejected"
    if (
        "socram-postreset-clock-cmd53-r5-rejected" in lower
        or (
            "stage=postreset-clock-en-write" in lower
            and "base=0x18104000" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
    ):
        return "socram-postreset-clock-cmd53-r5-rejected"
    if (
        "socram-prereset-zero-cmd53-r5-rejected" in lower
        or (
            "prereset-zero-ioctrl" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or (
            "base=0x18104000" in lower
            and "off=0x408" in lower
            and "prereset-zero-ioctrl" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
    ):
        return "socram-prereset-zero-cmd53-r5-rejected"
    if (
        "socram-prereset-fgc-cmd53-r5-rejected" in lower
        or (
            "base=0x18104000" in lower
            and "off=0x408" in lower
            and "prereset-fgc-clock" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
    ):
        return "socram-prereset-fgc-cmd53-r5-rejected"
    if stripped in {
        "firmware-window-cmd52-write",
        "unsupported operation: firmware-window-cmd52-write",
    } or "blocker=firmware-window-cmd52-write" in lower:
        return "firmware-window-cmd52-write"
    if stripped in {
        "firmware-window-sdhci-int-timeout",
        "unsupported operation: firmware-window-sdhci-int-timeout",
    } or "blocker=firmware-window-sdhci-int-timeout" in lower:
        return "firmware-window-sdhci-int-timeout"
    if stripped in {
        "firmware-window-sdhci-io-path",
        "unsupported operation: firmware-window-sdhci-io-path",
    } or "blocker=firmware-window-sdhci-io-path" in lower:
        return "firmware-window-sdhci-io-path"
    if stripped in {
        "sdio-cmd52-write",
        "unsupported operation: sdio-cmd52-write",
    }:
        return "sdio-cmd52-write"
    if stripped in {
        "sdio-cmd52-read",
        "unsupported operation: sdio-cmd52-read",
    }:
        return "sdio-cmd52-read"
    if stripped in {
        "sdio-cmd53-r5-error",
        "unsupported operation: sdio-cmd53-r5-error",
    }:
        return "sdio-cmd53-r5-error"
    if (
        "armcr4-prereset-fgc-cmd53-r5-rejected" in lower
        or (
            "prereset-fgc-clock" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or "arg=0x90681001" in lower
        or "arg=0x95681001" in lower
    ):
        return "armcr4-prereset-fgc-cmd53-r5-rejected"
    if "ht-backplane-cmd53-r5-rejected" in lower:
        return "ht-backplane-cmd53-r5-rejected"
    if "sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower:
        return "ht-backplane-cmd53-r5-rejected"
    if "ht-backplane-cmd53-data-wait" in lower:
        return "ht-backplane-cmd53-data-wait"
    if "ht-backplane-cmd52-r5-rejected" in lower:
        return "ht-backplane-cmd52-r5-rejected"
    if (
        "chipclkcsr-cmd52-pre-f2" in lower
        or (
            "stage=debug-probe-ht" in lower
            and "arg=0x12001c00" in lower
        )
        or (
            "cmd=52" in lower
            and "arg=0x12001c00" in lower
            and (
                "sdhci cmd error" in lower
                or "sdhci xfer error" in lower
                or "sdio cmd52" in lower
            )
        )
        or (
            "stage=debug-probe-ht" in lower
            and "chipclkcsr" in lower
            and "cmd52" in lower
            and any(
                token in lower
                for token in (
                    "sdio-cmd52-read",
                    "sdio-cmd52-write",
                    "sdhci-command-error",
                    "sdhci-int-timeout",
                )
            )
        )
    ):
        return "chipclkcsr-cmd52-pre-f2"
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
    if "join-pending" in lower or "association-pending" in lower:
        return "join-pending"
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

    event_list = list(events)
    usb_events = [event for event in event_list if event.domain == "usb"]
    if not usb_events:
        return 0, "missing"

    gate = 1
    blocker = "unknown"
    saw_command_submit = False
    saw_command_doorbell = False
    saw_command_doorbell_write_pending = False
    saw_command_event_ring_before = False
    saw_command_timeout_plan = False
    root_port_read_pending = False
    brcm_axi_read_pending = False
    reset_pre_usbcmd_pending = False
    run_posted_flush_pending = False
    command_probe_bus: str | None = None
    command_timeout_detail: str | None = None
    run_usbcmd_preserved_reset_bit = False
    usbcmd_controller_command_bits = 0x0000_0382
    precise_command_timeout_details = {
        "cmd-poll-only-timeout",
        "pcie-window-no-op-timeout",
        "raw-phys-cmd-poll-only-timeout",
        "cmd-fetch-timeout",
        "cmd-event-ring-timeout",
        "cmd-controller-not-running",
        "cmd-controller-halted",
        "usbcmd-run-preserved-reset-bit",
        "usbcmd-run-posted-flush-halt",
        "cmd-timeout",
        "cmd-poll-pending",
        "cmd-doorbell-write-halt",
        "cmd-submit-proof-timer-preempted",
    }
    stale_command_timeout_details = precise_command_timeout_details - {"cmd-poll-pending"}
    for event in event_list:
        raw = event.raw.lower()
        fields = event.fields
        tag = fields.get("tag", "")
        if event.domain == "kernel":
            if (
                run_posted_flush_pending
                and command_timeout_detail is None
                and blocker in {"unknown", "none", "cmd-poll-pending"}
                and ("halting" in raw or "kernel entry via interrupt" in raw)
            ):
                gate = max(gate, 3)
                command_timeout_detail = "usbcmd-run-posted-flush-halt"
                blocker = command_timeout_detail
                continue
            if (
                saw_command_doorbell_write_pending
                and command_timeout_detail is None
                and blocker in {
                    "unknown",
                    "none",
                    "cmd-poll-pending",
                    "root-port-sample-deferred",
                    "port-register-access-disabled",
                }
                and ("halting" in raw or "kernel entry via interrupt" in raw)
            ):
                gate = max(gate, 3)
                command_timeout_detail = "cmd-doorbell-write-halt"
                blocker = command_timeout_detail
                continue
            if (
                brcm_axi_read_pending
                and "kernel entry via interrupt" in raw
                and "irq 27" in raw
            ):
                gate = max(gate, 3)
                continue
            if (
                root_port_read_pending
                and "kernel entry via interrupt" in raw
                and "irq 27" in raw
            ):
                gate = max(gate, 3)
                continue
            if (
                reset_pre_usbcmd_pending
                and "kernel entry via interrupt" in raw
                and "irq 27" in raw
            ):
                gate = max(gate, 3)
                continue
            if (
                saw_command_timeout_plan
                and "kernel entry via interrupt" in raw
                and "irq 27" in raw
            ):
                gate = max(gate, 3)
                blocker = command_timeout_detail or "cmd-live-timeout-snapshot-missing"
                continue
            if (
                saw_command_doorbell
                and "kernel entry via interrupt" in raw
                and "irq 27" in raw
            ):
                gate = max(gate, 3)
                if command_timeout_detail in precise_command_timeout_details:
                    blocker = command_timeout_detail
                else:
                    blocker = "cmd-poll-pending"
                continue
            if (
                saw_command_event_ring_before
                and not saw_command_doorbell
                and blocker in {"unknown", "none", "cmd-poll-pending"}
                and "kernel entry via interrupt" in raw
                and "irq 27" in raw
            ):
                gate = max(gate, 3)
                blocker = "cmd-pre-doorbell-proof-timer-preempted"
                continue
            if (
                saw_command_submit
                and not saw_command_event_ring_before
                and not saw_command_doorbell
                and command_timeout_detail is None
                and blocker in {"unknown", "none", "cmd-poll-pending"}
                and "kernel entry via interrupt" in raw
                and "irq 27" in raw
            ):
                gate = max(gate, 3)
                blocker = "cmd-submit-proof-timer-preempted"
                continue
            continue
        if event.domain != "usb":
            continue
        if "vl805 posted-write flush" in raw:
            run_posted_flush_pending = False
        if "xhci.diag stage=0x0111" in raw or tag == "brcm-axi-setup-read":
            brcm_axi_read_pending = True
            gate = max(gate, 3)
            blocker = "brcm-axi-setup-read"
            continue
        if "xhci.diag stage=0x0112" in raw or "xhci.diag stage=0x0113" in raw:
            brcm_axi_read_pending = False
        if tag == "reset-pre-usbcmd-source" or "xhci.diag stage=0x0226" in raw:
            reset_pre_usbcmd_pending = True
            gate = max(gate, 3)
            blocker = "reset-pre-usbcmd-source"
            continue
        if "xhci.diag" in raw:
            reset_pre_usbcmd_pending = False
            if blocker == "reset-pre-usbcmd-source":
                blocker = "unknown"
        if "root-port read-begin" in raw:
            root_port_read_pending = True
            gate = max(gate, 3)
            blocker = "root-port-read-begin"
            continue
        if "root-port read-done" in raw:
            root_port_read_pending = False
            continue
        if raw.startswith("usb: runtime_gate"):
            proof_gate = parse_hex_int(fields.get("proof_gate"))
            if proof_gate is not None and proof_gate > 0:
                gate = max(gate, proof_gate)
                runtime_blocker = normalize_usb_blocker(fields.get("blocker", "none"))
                blocker = "none" if runtime_blocker == "none" else runtime_blocker
            continue
        if "usb proof_summary" in raw:
            proof_gate = parse_hex_int(fields.get("gate"))
            if proof_gate is not None and proof_gate > 0:
                gate = max(gate, proof_gate)
            proof_blocker = normalize_usb_blocker(fields.get("blocker", "none"))
            if proof_blocker != "none":
                if (
                    command_timeout_detail == "usbcmd-run-preserved-reset-bit"
                    and proof_blocker == "cmd-event-ring-timeout"
                ):
                    blocker = command_timeout_detail
                else:
                    blocker = proof_blocker
            elif fields.get("command") in {
                "no-op-unproven",
                "enable-slot-unproven",
                "enable-slot-linux-event-unproven",
            }:
                blocker = "cmd-poll-pending"
            else:
                blocker = "none"
            continue
        if "root-port sample skipped" in raw:
            gate = max(gate, 3)
            if command_timeout_detail in stale_command_timeout_details:
                blocker = command_timeout_detail
                continue
            reason = normalize_usb_blocker(fields.get("reason", "root-port-sample-deferred"))
            if reason in USB_OUTCOME_BLOCKERS:
                blocker = reason
            else:
                blocker = "root-port-sample-deferred"
            continue
        if "root-port deferred-capture" in raw:
            gate = max(gate, 5)
            blocker = "address-device-pending"
            continue
        if "usb root-enum deferred-port" in raw:
            gate = max(gate, 5)
            blocker = "address-device-pending"
            continue
        if "command-probe begin" in raw and fields.get("bus") in {
            "pcie-window",
            "phys",
        }:
            command_probe_bus = fields["bus"]
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
        if "xhci root-port stage=" in raw:
            ccs = fields.get("ccs")
            stage = fields.get("stage", "").lower()
            if ccs == "1":
                gate = max(gate, 5)
                if blocker in {"unknown", "no-connected-ports"}:
                    blocker = "none"
            elif stage in {"detect-zero", "detect-slow-zero"}:
                gate = max(gate, 4)
                blocker = "no-connected-ports"
        if "usb root-enum classify" in raw and fields.get("stage") == "address":
            gate = max(gate, 5)
            kind = normalize_usb_blocker(fields.get("kind", "address-failed"))
            blocker = kind if kind in USB_OUTCOME_BLOCKERS else "address-failed"
            continue
        if "usb root-enum failed" in raw:
            stage = fields.get("stage", "").lower()
            if stage == "address":
                gate = max(gate, 5)
                detail = normalize_usb_blocker(fields.get("detail", "address-failed"))
                blocker = detail if detail in USB_OUTCOME_BLOCKERS else "address-failed"
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
        if "usb device-desc ready" in raw or "hub child device-desc ready" in raw:
            gate = max(gate, 7)
            if blocker == "unknown":
                blocker = "none"
        if "usb config-desc ready" in raw or "hub child config-desc ready" in raw:
            gate = max(gate, 7)
            if blocker == "unknown":
                blocker = "none"
        if "usb set-config ready" in raw or "hub child set-config ready" in raw:
            gate = max(gate, 7)
            if blocker == "unknown":
                blocker = "none"
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
        if tag == "usb-hid-report-event":
            gate = max(gate, 8)
            if blocker in {"unknown", "none"}:
                blocker = "hid-report-event"
            continue
        if tag == "usb-hid-report-decode-fail":
            gate = max(gate, 8)
            blocker = "hid-report-decode-fail"
            continue
        if tag == "usb-hid-report-empty":
            gate = max(gate, 8)
            blocker = "hid-first-report"
            continue
        if tag == "usb-hid-report-transfer-fail":
            gate = max(gate, 8)
            blocker = "hid-interrupt-in"
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
        if (
            "cfg_window=mapped" in raw
            or "cfg_window=hal-ext-cfg-proven" in raw
            or "selected cfg=hal-ext" in raw
        ):
            gate = max(gate, 2)
        if "controller-ready" in raw or "controller-init-complete" in raw:
            gate = max(gate, 3)
        if tag in {"usbcmd-run-write", "usbcmd-run-write-done"}:
            run_cmd = parse_hex_int(fields.get("reg"))
            if run_cmd is None:
                run_cmd = parse_hex_int(fields.get("b"))
            if run_cmd is not None and (run_cmd & usbcmd_controller_command_bits) != 0:
                run_usbcmd_preserved_reset_bit = True
                command_timeout_detail = "usbcmd-run-preserved-reset-bit"
                blocker = command_timeout_detail
                gate = max(gate, 3)
            if tag == "usbcmd-run-write-done":
                run_posted_flush_pending = True
        if tag == "cmd-submit":
            saw_command_submit = True
            saw_command_doorbell = False
            saw_command_doorbell_write_pending = False
            saw_command_event_ring_before = False
            saw_command_timeout_plan = False
            command_timeout_detail = (
                "usbcmd-run-preserved-reset-bit"
                if run_usbcmd_preserved_reset_bit
                else None
            )
            if blocker in stale_command_timeout_details:
                blocker = "unknown"
            if run_usbcmd_preserved_reset_bit:
                blocker = "usbcmd-run-preserved-reset-bit"
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
                if command_probe_bus == "pcie-window":
                    command_timeout_detail = "pcie-window-no-op-timeout"
                elif command_probe_bus == "phys":
                    command_timeout_detail = "raw-phys-cmd-poll-only-timeout"
                else:
                    command_timeout_detail = "cmd-poll-only-timeout"
                blocker = command_timeout_detail
        elif tag.startswith("cmd-ring-timeout"):
            gate = max(gate, 3)
            if command_timeout_detail != "cmd-event-ring-timeout":
                command_timeout_detail = "cmd-timeout"
                blocker = command_timeout_detail
        elif tag.startswith("cmd-event-ring-timeout"):
            gate = max(gate, 3)
            if command_timeout_detail != "usbcmd-run-preserved-reset-bit":
                command_timeout_detail = "cmd-event-ring-timeout"
            blocker = command_timeout_detail
        elif tag.startswith("cmd-gate-timeout-plan"):
            gate = max(gate, 3)
            saw_command_timeout_plan = True
            if command_timeout_detail is None:
                command_timeout_detail = "cmd-live-timeout-snapshot-missing"
            blocker = command_timeout_detail
        elif tag == "cmd-gate-timeout-live-snapshot-deferred":
            gate = max(gate, 3)
            if command_timeout_detail not in precise_command_timeout_details:
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
        elif tag == "cmd-timeout":
            gate = max(gate, 3)
            command_timeout_detail = "cmd-timeout"
            blocker = command_timeout_detail
        elif tag == "cmd-prompt-safe-return-to-shell":
            gate = max(gate, 3)
            if command_timeout_detail not in precise_command_timeout_details:
                command_timeout_detail = "cmd-poll-pending"
            blocker = command_timeout_detail
        elif tag.startswith("cmd-event-ring-before"):
            saw_command_event_ring_before = True
            gate = max(gate, 3)
        elif tag == "cmd-doorbell-write":
            saw_command_doorbell = True
            saw_command_doorbell_write_pending = True
            gate = max(gate, 3)
            if (
                command_timeout_detail is None
                and blocker in {"unknown", "none"}
            ):
                blocker = "cmd-poll-pending"
        elif tag in {"cmd-doorbell-write-done", "cmd-doorbell-post-barrier"}:
            saw_command_doorbell = True
            saw_command_doorbell_write_pending = False
            gate = max(gate, 3)
            if (
                command_timeout_detail is None
                and blocker in {"unknown", "none"}
            ):
                blocker = "cmd-poll-pending"
        elif (
            usb_command_probe_success(fields.get("command_probe", ""))
            or (
                usb_command_probe_success(fields.get("result", ""))
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
            for key in (
                "blocker",
                "cause",
                "controller_gate",
                "exact",
                "outcome",
                "result",
                "verdict",
            ):
                value = fields.get(key)
                if value and value not in {"none", "n/a"}:
                    normalized_value = normalize_usb_blocker(value)
                    if (
                        key == "result"
                        and value
                        in {
                            "enable-slot-unproven",
                            "enable-slot-linux-event-unproven",
                            "enable-slot-uboot-first-unproven",
                            "no-op-unproven",
                        }
                        and fields.get("detail") == "cmd-event-ring-timeout"
                    ):
                        gate = max(gate, 3)
                        blocker = "cmd-event-ring-timeout"
                    elif (
                        normalized_value in USB_OUTCOME_BLOCKERS
                        and command_timeout_detail in stale_command_timeout_details
                    ):
                        blocker = command_timeout_detail
                    elif (
                        key == "result"
                        and normalized_value == "cmd-poll-only-timeout"
                        and fields.get("bus") == "pcie-window"
                    ):
                        if command_timeout_detail in precise_command_timeout_details:
                            blocker = command_timeout_detail
                        else:
                            blocker = "pcie-window-no-op-timeout"
                    elif (
                        key == "result"
                        and normalized_value == "cmd-poll-only-timeout"
                        and fields.get("bus") == "phys"
                    ):
                        blocker = "raw-phys-cmd-poll-only-timeout"
                    elif (
                        key == "verdict"
                        and normalized_value == "policy-skip-before-run"
                        and blocker not in {"unknown", "none", "policy-skip-before-run"}
                    ):
                        continue
                    else:
                        blocker = normalized_value
                    if normalized_value in {
                        "pcie-irq-quiesce-failed",
                        "pcie-irq-quiesce-missing",
                    }:
                        gate = max(gate, 3)
            focus = fields.get("focus")
            if focus and focus not in {"none", "n/a"} and blocker in {
                "unknown",
                "none",
            }:
                blocker = normalize_usb_blocker(focus)

    if (
        command_timeout_detail in stale_command_timeout_details
        and blocker in USB_OUTCOME_BLOCKERS
    ):
        blocker = command_timeout_detail

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
        "ht-recover-cmd5-timeout",
        "linux-probe-pmu-write-skip",
        "linux-probe-pmu-cmd53-r5-rejected",
        "pre-f2-core-control",
        "firmware-core-control",
        "chipcommon-socram-remap-cmd53-r5-rejected",
        "armcr4-reset-assert-cmd52-r5-rejected",
        "armcr4-reset-assert-cmd53-r5-rejected",
        "socram-assert-reset-cmd53-r5-rejected",
        "socram-clear-reset-cmd53-r5-rejected",
        "socram-postreset-clock-cmd53-r5-rejected",
        "socram-prereset-zero-cmd53-r5-rejected",
        "socram-prereset-fgc-cmd53-r5-rejected",
        "armcr4-prereset-fgc-cmd53-r5-rejected",
        "d11-prereset-fgc-cmd53-r5-rejected",
        "firmware-window-cmd52-write",
        "firmware-window-sdhci-int-timeout",
        "firmware-window-sdhci-io-path",
        "sdio-cmd52-write",
        "sdio-cmd52-read",
        "sdio-cmd53-r5-error",
        "ht-backplane-cmd53-r5-rejected",
        "ht-backplane-cmd53-data-wait",
        "ht-backplane-cmd52-r5-rejected",
        "ht-backplane-cmd52-unreadable",
        "chipclkcsr-cmd52-pre-f2",
    }
    direct_sdio_blockers = {
        "sdio-cmd52-write",
        "sdio-cmd52-read",
        "sdio-cmd53-r5-error",
    }
    specific_sdio_blockers = precise_ht_blockers - direct_sdio_blockers
    exact_reset_blockers = specific_sdio_blockers - {
        "pre-f2-core-control",
        "firmware-core-control",
    }
    reset_phase_blockers = {
        "linux-probe-pmu-write-skip",
        "linux-probe-pmu-cmd53-r5-rejected",
        "pre-f2-core-control",
        "firmware-core-control",
        "chipcommon-socram-remap-cmd53-r5-rejected",
        "armcr4-reset-assert-cmd52-r5-rejected",
        "armcr4-reset-assert-cmd53-r5-rejected",
        "socram-assert-reset-cmd53-r5-rejected",
        "socram-clear-reset-cmd53-r5-rejected",
        "socram-postreset-clock-cmd53-r5-rejected",
        "socram-prereset-zero-cmd53-r5-rejected",
        "socram-prereset-fgc-cmd53-r5-rejected",
        "armcr4-prereset-fgc-cmd53-r5-rejected",
        "d11-prereset-fgc-cmd53-r5-rejected",
        "sdio-cmd53-r5-error",
        "function2-disabled",
        "unknown",
    }
    ht_available_seen = False
    post_f2_progress_seen = False
    firmware_release_seen = False
    linux_probe_attach_seen = False
    linux_probe_pmu_write_active = False
    armcr4_prereset_ioctrl_active = False
    chipcommon_config_write_active = False
    socram_core_ctrl_stage: str | None = None
    specific_reset_blocker: str | None = None
    for event in wifi_events:
        raw = event.raw.lower()
        fields = event.fields
        cached_only_evidence = fields.get("source", "").lower() == "cached"
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
        raw_contract_blocker = normalize_wifi_blocker(raw)
        if raw_contract_blocker in {
            "pre-f2-core-control",
            "firmware-core-control",
            "chipcommon-socram-remap-cmd53-r5-rejected",
            "armcr4-reset-assert-cmd52-r5-rejected",
            "armcr4-reset-assert-cmd53-r5-rejected",
            "d11-prereset-fgc-cmd53-r5-rejected",
        }:
            explicit_blocker = raw_contract_blocker
        if (
            fields.get("policy") == "pre-f2-core-control"
            or fields.get("blocker_phase") == "pre-f2-core-control"
            or fields.get("gate") == "core-control-blocked-before-f2"
        ):
            explicit_blocker = "pre-f2-core-control"
        if (
            "base=0x18104000" in raw
            and "sdio-cmd53-r5-error" in raw
            and (
                "firmware core-ctrl access" in raw
                or "firmware core-disable" in raw
                or "firmware core-reset" in raw
            )
        ):
            explicit_blocker = normalize_wifi_blocker(raw)
        if (
            "ht-clock-recover-retry-fail" in raw
            and "ht-retry-sdio-card-not-ready" in raw
            and "phase=card-ready" in raw
        ):
            explicit_blocker = normalize_wifi_blocker(raw)
        if "pmu-res-reload-write-skip" in raw:
            explicit_blocker = "linux-probe-pmu-write-skip"
        if cached_only_evidence:
            if gate >= 4 and explicit_blocker in precise_ht_blockers | {"ht-clock-timeout"}:
                blocker = explicit_blocker
            continue
        if (
            "stage=armcr4-passive" in raw
            and "action=advisory-reset-skip" in raw
        ):
            gate = max(gate, 4)
            if blocker in {
                "armcr4-reset-assert-cmd52-r5-rejected",
                "armcr4-reset-assert-cmd53-r5-rejected",
            }:
                blocker = "none"
                specific_reset_blocker = None
            continue
        if "stage=d11-disable" in raw and "action=advisory-skip" in raw:
            gate = max(gate, 4)
            if blocker == "d11-prereset-fgc-cmd53-r5-rejected":
                blocker = "none"
                specific_reset_blocker = None
            continue
        if explicit_blocker == "pre-f2-core-control":
            gate = max(gate, 4)
            if specific_reset_blocker is not None:
                blocker = specific_reset_blocker
            elif blocker == "firmware-core-control" or blocker not in specific_sdio_blockers:
                blocker = explicit_blocker
            continue
        if "linux-probe-attach-state" in raw:
            linux_probe_attach_seen = True
        if linux_probe_attach_seen and (
            fields.get("addr") == "0x00603" or "addr=0x00603" in raw
        ):
            linux_probe_pmu_write_active = True
        if (
            "pmu-res-reload-write-skip" in raw
            or "action=pmu-res-reload" in raw
            or "stage=pre-core-reset-sdio-clock" in raw
            or "firmware core-ctrl access" in raw
        ):
            linux_probe_pmu_write_active = False
        if (
            "prereset-fgc-clock" in raw
            or (
                "firmware core-ctrl access" in raw
                and "base=0x18103000" in raw
                and "off=0x408" in raw
            )
        ):
            armcr4_prereset_ioctrl_active = True
            socram_core_ctrl_stage = None
            chipcommon_config_write_active = False
        if (
            "stage=armcr4-passive action=advisory-reset-skip" in raw
            or "stage=d11-disable" in raw
            or "base=0x18101000" in raw
            or "base=0x18104000" in raw
        ):
            armcr4_prereset_ioctrl_active = False
        if "stage=chipcommon-config-write" in raw:
            chipcommon_config_write_active = True
            socram_core_ctrl_stage = None
        if "stage=chipcommon-config action=skip-socram-remap" in raw:
            chipcommon_config_write_active = False
        if fields.get("base") == "0x18104000" or "base=0x18104000" in raw:
            stage = fields.get("stage")
            if stage == "prereset-zero-ioctrl":
                socram_core_ctrl_stage = "prereset-zero-ioctrl"
            elif stage == "prereset-fgc-clock":
                socram_core_ctrl_stage = "prereset-fgc-clock"
            elif stage == "assert-reset":
                socram_core_ctrl_stage = "assert-reset"
            elif stage == "clear-reset-primary":
                socram_core_ctrl_stage = "clear-reset-primary"
            elif stage == "postreset-clock-en-write":
                socram_core_ctrl_stage = "postreset-clock-en-write"
        for key in ("stage", "current", "outcome", "focus", "result", "source"):
            progress_gate = wifi_progress_gate(fields.get(key))
            if progress_gate is not None:
                gate = max(gate, progress_gate)
                if progress_gate >= 6:
                    post_f2_progress_seen = True
        if "sdio function-ready" in raw and fields.get("fn") == "2":
            gate = max(gate, 6)
            post_f2_progress_seen = True
        if "function2 ready-snapshot" in raw:
            gate = max(gate, 6)
            post_f2_progress_seen = True
        if fields.get("f2_enabled") == "yes" and fields.get("f2_ready") == "yes":
            gate = max(gate, 6)
            post_f2_progress_seen = True
        if "setup-firmware-channel-ready" in raw:
            gate = max(gate, 6)
            post_f2_progress_seen = True
        if "firmware-ready" in raw and "timeout" not in raw and "fail" not in raw:
            gate = max(gate, 7)
            post_f2_progress_seen = True
        if "control-plane reply" in raw:
            gate = max(gate, 7)
            post_f2_progress_seen = True
        if "control-plane step=init-complete action=ready" in raw or "[cyw43] ready:" in raw:
            gate = max(gate, 7)
            post_f2_progress_seen = True
            blocker = "none"
        if (
            "control-plane step=" in raw or "control-plane preinit step=" in raw
        ) and " action=fail" in raw:
            gate = max(gate, 7)
            if blocker not in precise_ht_blockers:
                blocker = explicit_blocker or "control-plane"
            continue
        if "join complete" in raw:
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "none"
        if "join pending" in raw or "join armed" in raw:
            gate = max(gate, 7)
            post_f2_progress_seen = True
            blocker = "join-pending"
            continue
        if "join failed" in raw:
            gate = max(gate, 7)
            post_f2_progress_seen = True
            blocker = normalize_wifi_blocker(raw)
            continue
        if fields.get("address_source") == "wifi-associating" or fields.get(
            "src"
        ) == "wifi-associating":
            gate = max(gate, 7)
            post_f2_progress_seen = True
            blocker = "join-pending"
            continue
        if (
            "dhcp] lease bound" in raw
            or fields.get("addr_src") == "dhcp-lease"
            or fields.get("src") == "dhcp-lease"
        ):
            gate = max(gate, 9)
            post_f2_progress_seen = True
            blocker = "none"
        if "[dhcp] tx queued" in raw:
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-pending"
            continue
        if "[dhcp] rx transition" in raw:
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-pending"
            continue
        if "[dhcp] rx ignored" in raw:
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-invalid-packet"
            continue
        if "[dhcp] rx failed" in raw:
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-failed"
            continue
        if "[dhcp] failed" in raw or "[dhcp] send failed" in raw:
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-failed"
            continue
        if explicit_blocker in {"dhcp-pending", "dhcp-failed"}:
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = explicit_blocker
            continue
        if "[net-selftest] starting run" in raw:
            gate = max(gate, 9)
            post_f2_progress_seen = True
        if "[net-selftest] result" in raw:
            tx_ok = fields.get("tx_ok")
            udp_ok = fields.get("udp_echo_ok")
            tcp_ok = fields.get("tcp_ok")
            console_ok = fields.get("console_ok")
            if {tx_ok, udp_ok, tcp_ok, console_ok} <= {"true", "1"}:
                gate = max(gate, 10)
                post_f2_progress_seen = True
                blocker = "none"
            else:
                gate = max(gate, 9)
                post_f2_progress_seen = True
                blocker = "nettest-failed"
            continue
        if raw.startswith("ok nettest"):
            gate = max(gate, 10)
            post_f2_progress_seen = True
            blocker = "none"
            continue
        if raw.startswith("err nettest"):
            if explicit_blocker in direct_sdio_blockers | {
                "chipclkcsr-cmd52-pre-f2",
            }:
                if blocker not in specific_sdio_blockers:
                    blocker = explicit_blocker
            elif (
                blocker == "ht-clock-timeout"
                and explicit_blocker == "sdio-cmd53-r5-error"
            ):
                blocker = blocker
            elif blocker in precise_ht_blockers:
                blocker = blocker
            elif explicit_blocker:
                blocker = explicit_blocker
            else:
                blocker = (
                    "ht-backplane-cmd53-data-wait"
                    if blocker == "ht-backplane-cmd53-data-wait"
                    else "nettest-failed"
                )
            if blocker in {"dhcp-pending", "dhcp-failed"}:
                gate = max(gate, 8)
            elif blocker in direct_sdio_blockers | {"chipclkcsr-cmd52-pre-f2"}:
                gate = max(gate, 4)
            elif blocker.startswith("net-not-ready") or blocker.startswith(
                "nettest-"
            ):
                gate = max(gate, 9)
            continue
        if "sdio-cmd52-write" in raw and "firmware core-ctrl access" in raw:
            gate = max(gate, 4)
            blocker = "sdio-cmd52-write"
            continue
        if "sdio-cmd52-read" in raw and "firmware core-ctrl access" in raw:
            gate = max(gate, 4)
            blocker = "sdio-cmd52-read"
            continue
        if "diagnostic-ht-timeout-backplane-unreadable" in raw:
            gate = max(gate, 4)
            if blocker not in {
                "ht-backplane-cmd53-data-wait",
                "ht-recover-cmd5-timeout",
            }:
                blocker = normalize_wifi_blocker(raw)
            continue
        if "diagnostic-ht-timeout-backplane-cmd52-rejected" in raw:
            gate = max(gate, 4)
            blocker = normalize_wifi_blocker(raw)
            continue
        if (
            ("stage=debug-probe-ht" in raw and "arg=0x12001c00" in raw)
            or (
                "cmd=52" in raw
                and "arg=0x12001c00" in raw
                and any(
                    token in raw
                    for token in (
                        "sdhci cmd error",
                        "sdhci xfer error",
                        "sdio cmd52",
                    )
                )
            )
        ):
            gate = max(gate, 4)
            blocker = "chipclkcsr-cmd52-pre-f2"
            continue
        if (
            "stage=debug-probe-ht" in raw
            and "chipclkcsr" in raw
            and "cmd52" in raw
            and any(
                token in raw
                for token in (
                    "sdio-cmd52-read",
                    "sdio-cmd52-write",
                    "sdhci-command-error",
                    "sdhci-int-timeout",
                )
            )
        ):
            gate = max(gate, 4)
            blocker = "chipclkcsr-cmd52-pre-f2"
            continue
        if "sdio cmd53 r5 fail" in raw:
            gate = max(gate, 4)
            if "base=0x18103000" in raw and (
                "stage=assert-reset" in raw or "off=0x800" in raw
            ):
                blocker = "armcr4-reset-assert-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "prereset-zero-ioctrl":
                blocker = "socram-prereset-zero-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "prereset-fgc-clock":
                blocker = "socram-prereset-fgc-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "assert-reset":
                blocker = "socram-assert-reset-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "clear-reset-primary":
                blocker = "socram-clear-reset-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "postreset-clock-en-write":
                blocker = "socram-postreset-clock-cmd53-r5-rejected"
            elif armcr4_prereset_ioctrl_active or fields.get("arg") in {
                "0x90681001",
                "0x95681001",
                "0x95681004",
            }:
                blocker = "armcr4-prereset-fgc-cmd53-r5-rejected"
            elif fields.get("arg") in {
                "0x95281004",
                "0x91281004",
                "0x95281001",
                "0x91281001",
            }:
                blocker = "d11-prereset-fgc-cmd53-r5-rejected"
            elif linux_probe_pmu_write_active or fields.get("arg") == "0x900c0601":
                blocker = "linux-probe-pmu-cmd53-r5-rejected"
            elif chipcommon_config_write_active or fields.get("arg") in {
                "0x95802004",
                "0x91802004",
            }:
                blocker = "chipcommon-socram-remap-cmd53-r5-rejected"
            elif blocker not in {
                "ht-backplane-cmd53-data-wait",
                "ht-recover-cmd5-timeout",
                "linux-probe-pmu-write-skip",
                "linux-probe-pmu-cmd53-r5-rejected",
                "pre-f2-core-control",
                "firmware-core-control",
                "chipcommon-socram-remap-cmd53-r5-rejected",
                "armcr4-reset-assert-cmd52-r5-rejected",
                "armcr4-reset-assert-cmd53-r5-rejected",
                "socram-assert-reset-cmd53-r5-rejected",
                "socram-clear-reset-cmd53-r5-rejected",
                "socram-postreset-clock-cmd53-r5-rejected",
                "socram-prereset-fgc-cmd53-r5-rejected",
                "armcr4-prereset-fgc-cmd53-r5-rejected",
                "d11-prereset-fgc-cmd53-r5-rejected",
            }:
                blocker = normalize_wifi_blocker(raw)
            if blocker in exact_reset_blockers:
                specific_reset_blocker = blocker
            continue
        if (
            (
                "sdhci xfer error cmd=53 arg=0x15000018" in raw
                or "sdhci xfer error cmd=53 arg=0x15bd8818" in raw
            )
            and "phase=data-wait" in raw
        ):
            gate = max(gate, 4)
            blocker = "ht-backplane-cmd53-data-wait"
            continue
        if "f1=enabled" in raw or "ioex=0x02" in raw or "iordy=0x02" in raw:
            gate = max(gate, 2)
        if fields.get("ht_avail") in {"yes", "ready"} or "ht_avail=ready" in raw:
            ht_available_seen = True
            post_f2_progress_seen = True
            gate = max(gate, 5)
            if blocker in {"unknown", "ht-clock-timeout"} or blocker in precise_ht_blockers:
                blocker = "none"
            continue
        if (
            "firmware_release" in raw
            or "armcr4_release=1" in raw
            or "rstvec=" in raw
        ):
            gate = max(gate, 3)
            if "armcr4_release=1" in raw:
                firmware_release_seen = True
        if explicit_blocker == "devon-timeout":
            gate = max(gate, 4)
            blocker = explicit_blocker
            continue
        if explicit_blocker == "ht-recover-cmd5-timeout":
            gate = max(gate, 4)
            blocker = explicit_blocker
            continue
        if explicit_blocker in {
            "linux-probe-pmu-write-skip",
            "linux-probe-pmu-cmd53-r5-rejected",
            "pre-f2-core-control",
            "firmware-core-control",
            "chipcommon-socram-remap-cmd53-r5-rejected",
            "armcr4-reset-assert-cmd52-r5-rejected",
            "armcr4-reset-assert-cmd53-r5-rejected",
            "socram-assert-reset-cmd53-r5-rejected",
            "socram-clear-reset-cmd53-r5-rejected",
            "socram-postreset-clock-cmd53-r5-rejected",
            "socram-prereset-zero-cmd53-r5-rejected",
            "socram-prereset-fgc-cmd53-r5-rejected",
            "armcr4-prereset-fgc-cmd53-r5-rejected",
            "d11-prereset-fgc-cmd53-r5-rejected",
            "firmware-window-cmd52-write",
            "firmware-window-sdhci-int-timeout",
            "firmware-window-sdhci-io-path",
            "sdio-cmd52-write",
            "sdio-cmd52-read",
            "sdio-cmd53-r5-error",
        }:
            gate = max(gate, 4)
            if explicit_blocker == "pre-f2-core-control":
                if specific_reset_blocker is not None:
                    blocker = specific_reset_blocker
                elif blocker == "firmware-core-control" or blocker not in specific_sdio_blockers:
                    blocker = explicit_blocker
            elif explicit_blocker == "firmware-core-control":
                if specific_reset_blocker is not None:
                    blocker = specific_reset_blocker
                elif blocker not in specific_sdio_blockers:
                    blocker = explicit_blocker
            elif explicit_blocker in direct_sdio_blockers and specific_reset_blocker is not None:
                blocker = specific_reset_blocker
            elif explicit_blocker in direct_sdio_blockers and blocker in {
                "pre-f2-core-control",
                "firmware-core-control",
            }:
                pass
            elif (
                explicit_blocker not in direct_sdio_blockers
                or blocker not in specific_sdio_blockers
            ):
                blocker = explicit_blocker
            if blocker in exact_reset_blockers:
                specific_reset_blocker = blocker
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
            if blocker in precise_ht_blockers and not ht_available_seen:
                gate = max(gate, 4)
            else:
                gate = max(gate, 7)
            if blocker not in precise_ht_blockers:
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
            if (
                explicit_blocker == "ht-clock-timeout"
                and firmware_release_seen
                and blocker in reset_phase_blockers
                and blocker not in direct_sdio_blockers
            ) or (
                explicit_blocker == "ht-clock-timeout"
                and blocker in reset_phase_blockers
                and blocker not in direct_sdio_blockers
                and blocker not in specific_sdio_blockers
            ) or blocker not in precise_ht_blockers:
                blocker = "ht-clock-timeout"
        elif "firmware-verify-readback" in raw:
            gate = max(gate, 3)
            blocker = "firmware-verify-readback"
        elif explicit_blocker:
            if blocker not in precise_ht_blockers and blocker != "ht-clock-timeout":
                blocker = explicit_blocker

    if blocker == "function2-disabled" and gate >= 4 and not ht_available_seen:
        blocker = "ht-clock-timeout"
    if blocker in precise_ht_blockers and not ht_available_seen and not post_f2_progress_seen:
        gate = min(gate, 4)
    return gate, blocker


def wifi_failure_detail_from_fields(event: TraceEvent) -> tuple[str, str]:
    """Return the exact failure and phase carried by a Wi-Fi event."""

    exact = "none"
    for key in ("exact", "exact_error", "cause", "reason", "err", "detail"):
        value = event.fields.get(key)
        if value and value not in {"none", "n/a"}:
            exact = normalize_wifi_blocker(value)
            break
    phase = (
        event.fields.get("stage")
        or event.fields.get("current")
        or event.fields.get("focus")
        or event.stage
        or "none"
    )
    return exact, phase


def summarize_wifi_failure_detail(
    events: Iterable[TraceEvent], wifi_blocker: str
) -> tuple[str, str, int]:
    """Find the best source line for the current Wi-Fi gate blocker."""

    socram_core_ctrl_stage: str | None = None
    armcr4_prereset_ioctrl_active = False
    exact = "none"
    phase = "none"
    line = 0
    blocker_matched = False
    for event in (event for event in events if event.domain == "wifi"):
        raw = event.raw.lower()
        fields = event.fields
        if (
            "prereset-fgc-clock" in raw
            or (
                "firmware core-ctrl access" in raw
                and "base=0x18103000" in raw
                and "off=0x408" in raw
            )
        ):
            armcr4_prereset_ioctrl_active = True
            socram_core_ctrl_stage = None
        if (
            "stage=armcr4-passive action=advisory-reset-skip" in raw
            or "stage=d11-disable" in raw
            or "base=0x18101000" in raw
            or "base=0x18104000" in raw
        ):
            armcr4_prereset_ioctrl_active = False
        if fields.get("base") == "0x18104000" or "base=0x18104000" in raw:
            event_stage = fields.get("stage")
            if event_stage in {
                "prereset-zero-ioctrl",
                "prereset-fgc-clock",
                "assert-reset",
                "clear-reset-primary",
                "postreset-clock-en-write",
            }:
                socram_core_ctrl_stage = event_stage

        candidate = normalize_wifi_blocker(raw)
        if "sdio cmd53 r5 fail" in raw:
            if socram_core_ctrl_stage == "prereset-zero-ioctrl":
                candidate = "socram-prereset-zero-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "prereset-fgc-clock":
                candidate = "socram-prereset-fgc-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "assert-reset":
                candidate = "socram-assert-reset-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "clear-reset-primary":
                candidate = "socram-clear-reset-cmd53-r5-rejected"
            elif socram_core_ctrl_stage == "postreset-clock-en-write":
                candidate = "socram-postreset-clock-cmd53-r5-rejected"
            elif armcr4_prereset_ioctrl_active:
                candidate = "armcr4-prereset-fgc-cmd53-r5-rejected"
        event_exact, event_phase = wifi_failure_detail_from_fields(event)
        if event_exact != "none" and not blocker_matched:
            exact = event_exact
            phase = event_phase
            line = event.line
        if candidate == wifi_blocker:
            blocker_matched = True
            exact = event_exact
            if exact == "none" and "sdio cmd53 r5 fail" in raw:
                exact = "sdio-cmd53-r5-error"
            if exact == "none":
                exact = candidate
            phase = (
                socram_core_ctrl_stage
                or fields.get("stage")
                or event.stage
                or event_phase
                or "none"
            )
            line = event.line
    return exact, phase, line


def summarize_gates(events: Iterable[TraceEvent]) -> GateSummary:
    """Build the current USB/WiFi hardware proof gate summary."""

    event_list = list(events)
    usb_gate, usb_blocker = summarize_usb_gate(event_list)
    wifi_gate, wifi_blocker = summarize_wifi_gate(event_list)
    wifi_exact, wifi_phase, wifi_blocker_line = summarize_wifi_failure_detail(
        event_list, wifi_blocker
    )
    boot_halted = any(
        event.domain == "kernel" and event.fields.get("halt") == "yes"
        for event in event_list
    )
    timer_irq27_seen = any(
        event.domain == "kernel"
        and event.fields.get("irq") == "27"
        and event.fields.get("timer_irq") == "yes"
        for event in event_list
    )
    if boot_halted:
        boot_halt_reason = "kernel-halt"
    elif timer_irq27_seen:
        boot_halt_reason = "timer-irq27-observed"
    else:
        boot_halt_reason = "none"
    usb_bootloader_handoff_seen = any(
        usb_bootloader_handoff_evidence(event) for event in event_list
    )
    usb_cold_boot_seen = any(usb_cold_boot_evidence(event) for event in event_list)
    usb_stale_uefi_hint_seen = any(
        usb_stale_uefi_hint_evidence(event) for event in event_list
    )
    return GateSummary(
        usb_gate=usb_gate,
        usb_blocker=usb_blocker,
        wifi_gate=wifi_gate,
        wifi_blocker=wifi_blocker,
        wifi_exact=wifi_exact,
        wifi_phase=wifi_phase,
        wifi_blocker_line=wifi_blocker_line,
        serial_clean=serial_clean(event_list),
        boot_halted=boot_halted,
        timer_irq27_seen=timer_irq27_seen,
        boot_halt_reason=boot_halt_reason,
        usb_bootloader_handoff_seen=usb_bootloader_handoff_seen,
        usb_cold_boot_seen=usb_cold_boot_seen,
        usb_stale_uefi_hint_seen=usb_stale_uefi_hint_seen,
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


def parse_expectation_pairs(expectations: Iterable[str]) -> list[tuple[str, str]]:
    """Parse KEY=VALUE gate expectations while preserving duplicate keys."""

    parsed: list[tuple[str, str]] = []
    for expectation in expectations:
        key, separator, value = expectation.partition("=")
        if not separator or not key or not value:
            raise SystemExit(f"invalid expectation, use KEY=VALUE: {expectation}")
        parsed.append((key, value))
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
    summary: GateSummary,
    expectations: dict[str, str] | Iterable[tuple[str, str]],
    stderr: TextIO,
) -> bool:
    """Return true when all gate values differ from rejected values."""

    actual = {key: str(value) for key, value in summary.to_record().items()}
    ok = True
    items = expectations.items() if isinstance(expectations, dict) else expectations
    for key, rejected_value in items:
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


def latest_boot_lines(lines: list[str]) -> list[str]:
    """Return the latest boot slice from an accumulated serial capture."""

    _, latest_lines = latest_boot_slice(lines)
    return latest_lines


def latest_boot_slice(lines: list[str]) -> tuple[int, list[str]]:
    """Return the latest boot slice plus its original zero-based line offset."""

    latest_start = None
    latest_start_is_chain = False
    for index, line in enumerate(lines):
        clean = ANSI_RE.sub("", line).lower()
        if any(marker in clean for marker in BOOT_CHAIN_ROOT_MARKERS):
            latest_start = index
            latest_start_is_chain = True
            continue
        if any(marker in clean for marker in BOOT_CHAIN_CONTINUATION_MARKERS):
            if latest_start_is_chain and latest_start is not None:
                continue
            latest_start = index
            latest_start_is_chain = True
            continue
        if any(marker in clean for marker in BOOT_START_MARKERS) and not latest_start_is_chain:
            latest_start = index
            latest_start_is_chain = False
    if latest_start is None:
        return 0, lines
    return latest_start, lines[latest_start:]


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
        help="emit a compact JSON summary for the latest boot instead of JSON Lines",
    )
    parser.add_argument(
        "--gate-summary",
        action="store_true",
        help=(
            "emit stable USB/WiFi gate KEY=VALUE lines for the latest boot "
            "instead of JSON Lines"
        ),
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
    lines = read_input(args.log)
    line_base = 0
    if args.gate_summary or args.summary:
        line_base, lines = latest_boot_slice(lines)
    events = filter_events(parse_events(lines, line_base=line_base), set(args.domain))
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
            gate_summary, parse_expectation_pairs(args.expect_not), sys.stderr
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
