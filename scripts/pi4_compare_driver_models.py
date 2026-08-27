#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Compare Pi driver logs and provenance-bound QEMU/Pi benchmarks.
# Copyright 2026 Lukas Bower

"""Compare Pi driver logs or provenance-bound QEMU/Pi throughput reports.

Serial mode reports only observable log breadcrumbs. Benchmark mode rejects
stale or mismatched target evidence before evaluating successful throughput.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import stat
import sys
import time
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable


BENCHMARK_REPORT_SCHEMA = "cohesix-benchmark-report/v1"
BENCHMARK_PROVENANCE_SCHEMA = "cohesix-benchmark-provenance/v2"
BENCHMARK_TARGET_EVIDENCE_SCHEMA = "cohesix-benchmark-target-evidence/v2"
DEFAULT_MAX_AGE_SECS = 6 * 60 * 60
MAX_BENCHMARK_AGE_SECS = 7 * 24 * 60 * 60
MAX_REFERENCE_CLOCK_SKEW_SECS = 300
MAX_REPORT_BYTES = 8 * 1024 * 1024
CANONICAL_EXECUTABLE_WORKERS = 256
CANONICAL_HIGH_WORKLOAD = {
    "mode": "simulate",
    "population_mode": "executable",
    "control_write_outcome": "admitted",
    "scenario": "mixed",
    "seed": 2608,
    "entropy": 5.0,
    "workers_min": CANONICAL_EXECUTABLE_WORKERS,
    "workers_max": CANONICAL_EXECUTABLE_WORKERS,
    "worker_cap": CANONICAL_EXECUTABLE_WORKERS,
    "multi_hive": False,
    "hives": 1,
    "workers_per_hive": CANONICAL_EXECUTABLE_WORKERS,
    "intensity_min": 8,
    "intensity_max": 8,
    "base_rps": 4.0,
    "target_rps_min": 8192.0,
    "target_rps_max": 8192.0,
    "duration_s": 120.0,
    "ramp_step_secs": 30,
    "max_inflight_configured": 32,
    "tail_bytes": 4096,
    "telemetry_reference_chunk_bytes": 16 * 1024 * 1024,
    "include_lifecycle": False,
    "auto_approve": True,
    "strict_control_errors": True,
    "transient_retries": False,
    "error_budget_rate": 0.01,
    "request_timeout_s": 10.0,
    "request_auth_enabled": True,
    "role": "queen",
}
EXECUTABLE_ROLE_SLOTS = {
    "worker-heartbeat": 1,
    "worker-gpu": 127,
    "worker-lora": 128,
}
EXECUTABLE_ROLE_CORES = {
    "worker-heartbeat": 3,
    "worker-gpu": 2,
    "worker-lora": 3,
}
WORKER_EXEMPLAR_FIELDS = {
    "role",
    "slot",
    "lease_epoch",
    "supervisor_generation",
    "cap_generation",
    "worker",
    "lifecycle",
    "artifact",
    "receipt",
    "execution_proof",
    "ready_sequence",
    "control_sequence",
    "receipt_sequence",
    "completion_sequence",
    "image_sha256",
    "core",
    "scheduling_context",
    "object_inventory",
}
OBJECT_INVENTORY_FIELDS = {
    "tcbs",
    "scheduling_contexts",
    "reply_objects",
    "vspaces",
    "cnodes",
    "page_tables",
    "asids",
    "frames",
    "endpoints",
    "notifications",
    "fault_caps",
    "timeout_fault_caps",
    "cspace_slots",
    "untyped_bytes",
}
EXECUTABLE_TARGET_SESSION_FIELDS = {
    "manifest_sha256",
    "root_image_sha256",
    "worker_archive_sha256",
    "worker_image_manifest_sha256",
    "worker_abi_sha256",
}
MAX_SAFE_JSON_INT = (1 << 53) - 1
LATENCY_FIELDS = {
    "avg_s",
    "min_s",
    "max_s",
    "p50_s",
    "p90_s",
    "p95_s",
    "p99_s",
}
PERFORMANCE_HASH_FIELDS = (
    "source_sha256",
    "manifest_sha256",
    "image_sha256",
    "root_image_sha256",
    "target_session_sha256",
    "runtime_evidence_sha256",
    "network_evidence_sha256",
)
PARITY_BACKPRESSURE_FIELDS = (
    "control_waiters",
    "telemetry_waiters",
    "control_waiters_high_water",
    "telemetry_waiters_high_water",
    "pool_exhausted",
    "checkout_retries",
    "timeout_rejections",
    "control_write_retryable_errors",
    "control_write_retry_exhaustions",
)


KEY_VALUE_RE = re.compile(
    r"(?P<key>[A-Za-z0-9_.:-]+)="
    r"(?P<value>\"[^\"]*\"|'[^']*'|[^ \t\r\n]+)"
)
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
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

POSITIVE_COMPARISON_KEYS = (
    "serial_prompt_seen",
    "serial_input_loop_seen",
    "hdmi_map_seen",
    "hdmi_visible_seen",
    "hdmi_mirror_seen",
    "pcie_hal_prep_seen",
    "pcie_engine_init_return_seen",
    "usb_keyboard_route_seen",
    "usb_first_byte_seen",
    "wifi_sdio_seen",
    "wifi_cyw43_seen",
    "wifi_dhcp_seen",
    "wifi_net_diag_seen",
)
NEGATIVE_COMPARISON_KEYS = (
    "serial_no_reply_seen",
    "halt_seen",
    "panic_seen",
    "hdmi_timeout_seen",
    "pcie_engine_init_timeout_seen",
    "usb_blocker_seen",
    "wifi_blocker_seen",
)
COUNT_COMPARISON_KEYS = (
    "ring_call_outstanding",
    "ring_call_timeouts",
)
FIELD_OUTPUT_ORDER = (
    "line_count",
    "boot_slice_start",
    *POSITIVE_COMPARISON_KEYS,
    *NEGATIVE_COMPARISON_KEYS,
    "halt_reason",
    "usb_blocker",
    "wifi_blocker",
    "ring_call_begin_count",
    "ring_call_return_count",
    *COUNT_COMPARISON_KEYS,
    "ring_call_timeout_contracts",
    "milestone_state",
    "score",
)


def parse_fields(text: str) -> dict[str, str]:
    """Extract KEY=VALUE fields from one log line."""

    fields: dict[str, str] = {}
    for match in KEY_VALUE_RE.finditer(text):
        value = match.group("value")
        if (value.startswith('"') and value.endswith('"')) or (
            value.startswith("'") and value.endswith("'")
        ):
            value = value[1:-1]
        fields[match.group("key")] = value
    return fields


def normalize_line(line: str) -> str:
    """Remove serial control characters that should not affect comparison."""

    return ANSI_RE.sub("", line.replace("\r", "")).strip()


def usb_linked_hid_source(fields: dict[str, str]) -> bool:
    """Return whether USB first-byte proof came from the linked HID runtime."""

    return (
        fields.get("source", "").lower() == "linked-runtime-hid"
        or fields.get("first_byte_source", "").lower() == "linked-runtime-hid"
    )


def latest_boot_slice(lines: list[str]) -> tuple[int, list[str]]:
    """Return the latest boot-like slice from an accumulated serial capture."""

    latest_start: int | None = None
    latest_start_is_chain = False
    for index, line in enumerate(lines):
        clean = normalize_line(line).lower()
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
        if any(marker in clean for marker in BOOT_START_MARKERS):
            if not latest_start_is_chain:
                latest_start = index
                latest_start_is_chain = False
    if latest_start is None:
        return 0, lines
    return latest_start, lines[latest_start:]


def truth(value: bool) -> str:
    """Render a boolean in Cohesix summary form."""

    return "yes" if value else "no"


def first_non_none(values: Iterable[str | None], default: str = "none") -> str:
    """Return the first non-empty value from a sequence."""

    for value in values:
        if value:
            return value
    return default


def normalize_blocker(value: str) -> str:
    """Normalize a raw diagnostic value into a stable comparison token."""

    clean = value.strip().strip('"').strip("'").lower()
    clean = clean.replace("_", "-")
    clean = re.sub(r"[^a-z0-9.+-]+", "-", clean).strip("-")
    if not clean:
        return "unknown"
    replacements = {
        "cyw43-ht-clock-timeout-before-function2": "ht-clock-timeout",
        "cyw43-control-plane-no-reply-linux-f2-armed": "cyw43-no-reply",
        "cyw43-control-plane-pure-f2-startup-link-no-reply": (
            "cyw43-no-reply"
        ),
        "driver-task-no-reply": "driver-task-no-reply",
        "pcie-vl805-config-contract-missing": (
            "pcie-vl805-config-contract-missing"
        ),
    }
    return replacements.get(clean, clean)


USB_DETAIL_BLOCKERS = {
    "0x0201": "command-event-ring-not-proven",
    "0201": "command-event-ring-not-proven",
    "513": "command-event-ring-not-proven",
    "0x0203": "enable-slot-completion-pending",
    "0203": "enable-slot-completion-pending",
    "515": "enable-slot-completion-pending",
    "0x0213": "address-device-failed",
    "0213": "address-device-failed",
    "531": "address-device-failed",
}

USB_BLOCKER_RANK = {
    "runtime-ring-submit-busy": 100,
    "address-device-failed": 90,
    "enable-slot-completion-pending": 85,
    "command-event-ring-not-proven": 80,
    "link-or-rc-not-ready": 10,
}

WIFI_DETAIL_BLOCKERS = {
    "0x5101": "sdio-command-unavailable",
    "5101": "sdio-command-unavailable",
    "20737": "sdio-command-unavailable",
    "0x530a": "cyw43-descriptor-invalid",
    "530a": "cyw43-descriptor-invalid",
    "21258": "cyw43-descriptor-invalid",
    "0x5329": "cyw43-firmware-retry-exhausted",
    "5329": "cyw43-firmware-retry-exhausted",
    "21289": "cyw43-firmware-retry-exhausted",
}

WIFI_LIFECYCLE_BLOCKERS = frozenset(
    {
        "pair-recovery-required",
    }
)


def normalized_detail_blocker(
    fields: dict[str, str], mapping: dict[str, str]
) -> str | None:
    """Return a stable blocker name for known numeric detail fields."""

    detail = fields.get("detail")
    if detail is None:
        return None
    return mapping.get(detail.strip().lower())


def usb_blocker_rank(blocker: str) -> int:
    """Return comparison priority for USB blockers."""

    return USB_BLOCKER_RANK.get(normalize_blocker(blocker), 50)


@dataclass
class RingTracker:
    """Track begin/return/timeout driver-task ring evidence."""

    begins: Counter[tuple[str, str]] = field(default_factory=Counter)
    begin_count: int = 0
    return_count: int = 0
    timeout_count: int = 0
    timeout_contracts: set[str] = field(default_factory=set)

    def ring_id(self, fields: dict[str, str]) -> tuple[str, str]:
        """Build a stable ring-call identity from common diagnostic fields."""

        contract = fields.get("contract", "unknown")
        request = first_non_none(
            (
                fields.get("request"),
                fields.get("req"),
                fields.get("sequence"),
                fields.get("seq"),
                fields.get("opcode"),
            ),
            default="unknown",
        )
        return contract, request

    def record_begin(self, fields: dict[str, str]) -> None:
        """Record a ring call begin line."""

        self.begin_count += 1
        self.begins[self.ring_id(fields)] += 1

    def record_return(self, fields: dict[str, str]) -> None:
        """Record a ring call return line."""

        self.return_count += 1
        ring_id = self.ring_id(fields)
        if self.begins[ring_id] > 0:
            self.begins[ring_id] -= 1
            if self.begins[ring_id] == 0:
                del self.begins[ring_id]

    def record_timeout(self, fields: dict[str, str]) -> None:
        """Record a ring call timeout line."""

        self.timeout_count += 1
        contract = fields.get("contract")
        if contract:
            self.timeout_contracts.add(contract)

    @property
    def outstanding_count(self) -> int:
        """Return the aggregate number of begins without matching returns."""

        return sum(self.begins.values())


@dataclass
class LogSummary:
    """Machine-comparable evidence extracted from one Pi 4 log."""

    label: str
    line_count: int = 0
    boot_slice_start: int = 0
    serial_prompt_seen: bool = False
    serial_input_loop_seen: bool = False
    serial_no_reply_seen: bool = False
    hdmi_map_seen: bool = False
    hdmi_visible_seen: bool = False
    hdmi_mirror_seen: bool = False
    hdmi_timeout_seen: bool = False
    pcie_hal_prep_seen: bool = False
    pcie_engine_init_return_seen: bool = False
    pcie_engine_init_timeout_seen: bool = False
    usb_keyboard_route_seen: bool = False
    usb_first_byte_seen: bool = False
    usb_blocker_seen: bool = False
    usb_blocker: str = "none"
    wifi_sdio_seen: bool = False
    wifi_cyw43_seen: bool = False
    wifi_dhcp_seen: bool = False
    wifi_net_diag_seen: bool = False
    wifi_blocker_seen: bool = False
    wifi_blocker: str = "none"
    halt_seen: bool = False
    halt_reason: str = "none"
    panic_seen: bool = False
    ring_call_begin_count: int = 0
    ring_call_return_count: int = 0
    ring_call_outstanding: int = 0
    ring_call_timeouts: int = 0
    ring_call_timeout_contracts: str = "none"

    def score(self) -> int:
        """Return a deterministic readiness score for old/new ordering."""

        positive = sum(
            1 for key in POSITIVE_COMPARISON_KEYS if bool(getattr(self, key))
        )
        negative = sum(
            1 for key in NEGATIVE_COMPARISON_KEYS if bool(getattr(self, key))
        )
        negative += self.ring_call_outstanding + self.ring_call_timeouts
        return positive - negative

    def milestone_state(self) -> str:
        """Return a concise state token for the visible milestone frontier."""

        if self.halt_seen:
            return "halted"
        if not self.serial_prompt_seen:
            return "no-prompt"
        missing = []
        if not self.hdmi_visible_seen:
            missing.append("hdmi")
        if not self.usb_first_byte_seen:
            missing.append("usb")
        if not self.wifi_dhcp_seen:
            missing.append("wifi-dhcp")
        if self.ring_call_outstanding or self.ring_call_timeouts:
            missing.append("ring")
        if missing:
            return "partial-" + "+".join(missing)
        return "interactive-local-seat-network"

    def to_record(self) -> dict[str, str]:
        """Return stable string fields for KEY=VALUE emission."""

        record: dict[str, str] = {}
        for key in FIELD_OUTPUT_ORDER:
            if key == "score":
                record[key] = str(self.score())
            elif key == "milestone_state":
                record[key] = self.milestone_state()
            else:
                value = getattr(self, key)
                if isinstance(value, bool):
                    record[key] = truth(value)
                else:
                    record[key] = str(value)
        return record


def mark_halt(summary: LogSummary, reason: str) -> None:
    """Record the first halt reason while preserving halt visibility."""

    summary.halt_seen = True
    if summary.halt_reason == "none":
        summary.halt_reason = reason


def update_serial(summary: LogSummary, line: str) -> None:
    """Extract serial prompt and input-loop evidence."""

    lowered = line.lower()
    if line.startswith("cohesix>"):
        summary.serial_prompt_seen = True
        command = line.removeprefix("cohesix>").strip()
        if command:
            summary.serial_input_loop_seen = True
    if "[cohesix] root console ready" in lowered:
        summary.serial_prompt_seen = True
    if "[mark] root-console.start.ok" in lowered:
        summary.serial_input_loop_seen = True
    if "serial_echo" in lowered or "serial-console:input-loop" in lowered:
        summary.serial_input_loop_seen = True
    if "serial-console:serial-runtime-init:no-reply" in lowered:
        summary.serial_no_reply_seen = True


def update_hdmi(summary: LogSummary, line: str, fields: dict[str, str]) -> None:
    """Extract HDMI map, visibility, mirror, and timeout evidence."""

    lowered = line.lower()
    hdmi_line = (
        "hdmi" in lowered
        or fields.get("contract") == "hdmi-text"
        or fields.get("hot_path") == "hdmi-text"
    )
    if not hdmi_line:
        return
    if any(token in lowered for token in ("map", "mapped", "framebuffer")):
        summary.hdmi_map_seen = True
    if "driver_task_owner_state" in lowered and fields.get("hot_path") == "hdmi-text":
        summary.hdmi_map_seen = True
    if any(
        token in lowered
        for token in (
            "hdmi_responsive",
            "visible=yes",
            "banner=yes",
            "frame visible",
            "console-ready",
        )
    ):
        summary.hdmi_visible_seen = True
    if "mirrored_bytes=" in lowered or "mirror" in lowered:
        summary.hdmi_mirror_seen = True
    if "driver_task_ring_call_timeout" in lowered or "timeout" in lowered:
        summary.hdmi_timeout_seen = True


def update_pcie(summary: LogSummary, line: str, fields: dict[str, str]) -> None:
    """Extract PCIe HAL prep and engine-init return evidence."""

    lowered = line.lower()
    pcie_line = (
        "pcie" in lowered
        or fields.get("contract") == "pcie-root"
        or fields.get("hot_path") == "pcie-root"
    )
    if not pcie_line:
        return
    if any(
        token in lowered
        for token in (
            "hal-prep",
            "hal prep",
            "driver_task_resource_init",
            "owner_state",
            "pcie-vl805",
        )
    ):
        summary.pcie_hal_prep_seen = True
    engine_init = "engine-init" in lowered or fields.get("opcode") == "engine-init"
    if engine_init and "driver_task_ring_call_return" in lowered:
        summary.pcie_engine_init_return_seen = True
    if engine_init and (
        "driver_task_ring_call_timeout" in lowered or "timeout" in lowered
    ):
        summary.pcie_engine_init_timeout_seen = True


def update_usb(summary: LogSummary, line: str, fields: dict[str, str]) -> None:
    """Extract USB keyboard route, first byte, and blocker evidence."""

    lowered = line.lower()
    usb_line = (
        "usb" in lowered
        or "xhci" in lowered
        or "local-seat" in lowered
        or "keyboard runtime proof" in lowered
        or fields.get("contract") == "usb-local-seat"
        or fields.get("hot_path") == "usb-keyboard"
    )
    if not usb_line:
        return
    if any(
        token in lowered
        for token in (
            "route=usb-keyboard",
            "hot_path=usb-keyboard",
            "keyboard route",
            "shared parser",
            "runtime keyboard poll",
            "usb_burst",
        )
    ):
        summary.usb_keyboard_route_seen = True
    if usb_linked_hid_source(fields) and (
        "runtime keyboard first-byte" in lowered
        or fields.get("first_byte") == "yes"
        or (
            fields.get("proof_gate") == "10"
            and fields.get("blocker") == "none"
            and fields.get("keyboard") == "yes"
        )
    ):
        summary.usb_first_byte_seen = True
    ring_submit_blocker = (
        "runtime-ring-submit-busy"
        if fields.get("stage") == "runtime-ring-submit"
        and fields.get("status") == "busy"
        else None
    )
    structured_blocker = first_non_none(
        (
            ring_submit_blocker,
            fields.get("blocker"),
            normalized_detail_blocker(fields, USB_DETAIL_BLOCKERS),
        )
    )
    blocker = first_non_none(
        (
            structured_blocker if structured_blocker != "none" else None,
            fields.get("blocker"),
            fields.get("detail"),
            fields.get("reason"),
            fields.get("exact"),
            fields.get("exact_error"),
        )
    )
    if blocker != "none" and (
        "missing" in blocker
        or "timeout" in blocker
        or "not-ready" in blocker
        or "no-reply" in blocker
        or "pcie-vl805" in blocker
        or "address-device" in blocker
        or "command-event-ring-not-proven" in blocker
        or "enable-slot-completion-pending" in blocker
        or "runtime-ring-submit" in blocker
        or "busy" in blocker
        or "failed" in blocker
    ):
        summary.usb_blocker_seen = True
        candidate = normalize_blocker(blocker)
        if summary.usb_blocker == "none" or usb_blocker_rank(
            candidate
        ) > usb_blocker_rank(summary.usb_blocker):
            summary.usb_blocker = candidate


def update_wifi(summary: LogSummary, line: str, fields: dict[str, str]) -> None:
    """Extract WiFi SDIO, CYW43, DHCP, and diagnostic evidence."""

    lowered = line.lower()
    wifi_line = any(
        token in lowered
        for token in (
            "wifi",
            "wi-fi",
            "sdio",
            "cyw43",
            "cyw43455",
            "dhcp",
            "nettest",
            "netstats",
        )
    )
    if not wifi_line:
        return
    if "sdio" in lowered or fields.get("contract") == "sdio-host":
        summary.wifi_sdio_seen = True
    if "cyw43" in lowered or "cyw43455" in lowered:
        summary.wifi_cyw43_seen = True
    if (
        fields.get("dhcp") == "bound"
        or fields.get("dhcp_phase") == "bound"
        or "lease bound" in lowered
    ):
        summary.wifi_dhcp_seen = True
    if any(
        token in lowered
        for token in (
            "wifi diag",
            "wifi dump-state",
            "nettest",
            "netstats",
            "net_console",
            "net-console",
        )
    ):
        summary.wifi_net_diag_seen = True
    structured_blocker = first_non_none(
        (
            fields.get("reason")
            if "cyw43" in fields.get("reason", "")
            or "firmware" in fields.get("reason", "")
            else None,
            fields.get("descriptor_status"),
            normalized_detail_blocker(fields, WIFI_DETAIL_BLOCKERS),
            fields.get("blocker"),
        )
    )
    blocker = first_non_none(
        (
            structured_blocker if structured_blocker != "none" else None,
            fields.get("exact_error"),
            fields.get("exact"),
            fields.get("cause"),
            fields.get("reason"),
            fields.get("detail"),
            fields.get("blocker"),
        )
    )
    if blocker != "none" and (
        "cyw43" in blocker
        or "sdio" in blocker
        or "dhcp" in blocker
        or "net-disabled" in blocker
        or "not-ready" in blocker
        or "timeout" in blocker
        or "firmware-retry-exhausted" in blocker
        or "failed" in blocker
        or blocker in WIFI_LIFECYCLE_BLOCKERS
    ):
        summary.wifi_blocker_seen = True
        if summary.wifi_blocker == "none" or structured_blocker != "none":
            summary.wifi_blocker = normalize_blocker(blocker)


def safe_int(value: str) -> int:
    """Parse an integer field without raising on malformed serial text."""

    try:
        return int(value, 0)
    except ValueError:
        return 0


def summarize_log(label: str, lines: list[str]) -> LogSummary:
    """Summarize one old or new driver-model log."""

    boot_slice_start, boot_lines = latest_boot_slice(lines)
    summary = LogSummary(
        label=label,
        line_count=len(boot_lines),
        boot_slice_start=boot_slice_start,
    )
    rings = RingTracker()
    for raw_line in boot_lines:
        line = normalize_line(raw_line)
        if not line:
            continue
        fields = parse_fields(line)
        lowered = line.lower()
        update_serial(summary, line)
        update_hdmi(summary, line, fields)
        update_pcie(summary, line, fields)
        update_usb(summary, line, fields)
        update_wifi(summary, line, fields)
        if "driver_task_ring_call_begin" in lowered:
            rings.record_begin(fields)
        if "driver_task_ring_call_return" in lowered:
            rings.record_return(fields)
        if "driver_task_ring_call_timeout" in lowered:
            rings.record_timeout(fields)
        if "driver_task_ring_call_abort" in lowered:
            rings.record_return(fields)
        if "[panic]" in lowered or "panicked at " in lowered:
            summary.panic_seen = True
            mark_halt(summary, "panic")
        if "halting..." in lowered:
            mark_halt(summary, "kernel-halt")
        if "kernel entry via interrupt" in lowered:
            if "irq 27" in lowered:
                mark_halt(summary, "kernel-interrupt-irq-27")
            else:
                mark_halt(summary, "kernel-interrupt")
    summary.ring_call_begin_count = rings.begin_count
    summary.ring_call_return_count = rings.return_count
    summary.ring_call_outstanding = rings.outstanding_count
    summary.ring_call_timeouts = rings.timeout_count
    if rings.timeout_contracts:
        summary.ring_call_timeout_contracts = ",".join(
            sorted(rings.timeout_contracts)
        )
    return summary


def read_log(path: Path) -> list[str]:
    """Read one serial log path."""

    if not path.is_file():
        raise SystemExit(f"log not found: {path}")
    return path.read_text(encoding="utf-8", errors="replace").splitlines()


def comparison_terms(old: LogSummary, new: LogSummary) -> tuple[list[str], list[str]]:
    """Return regression and advancement tokens."""

    regressions: list[str] = []
    advancements: list[str] = []
    for key in POSITIVE_COMPARISON_KEYS:
        old_value = bool(getattr(old, key))
        new_value = bool(getattr(new, key))
        if old_value and not new_value:
            regressions.append(key.removesuffix("_seen"))
        elif not old_value and new_value:
            advancements.append(key.removesuffix("_seen"))
    for key in NEGATIVE_COMPARISON_KEYS:
        old_value = bool(getattr(old, key))
        new_value = bool(getattr(new, key))
        if not old_value and new_value:
            regressions.append(key.removesuffix("_seen"))
        elif old_value and not new_value:
            advancements.append(key.removesuffix("_seen"))
    for key in COUNT_COMPARISON_KEYS:
        old_value = int(getattr(old, key))
        new_value = int(getattr(new, key))
        if new_value > old_value:
            regressions.append(key)
        elif new_value < old_value:
            advancements.append(key)
    return regressions, advancements


def comparison_verdict(
    regressions: list[str], advancements: list[str], score_delta: int
) -> str:
    """Classify the comparison without hiding mixed results."""

    if regressions and advancements:
        return "mixed-regression"
    if regressions:
        return "regression"
    if advancements or score_delta > 0:
        return "advancement"
    if score_delta < 0:
        return "regression"
    return "unchanged"


def join_terms(terms: list[str]) -> str:
    """Render comparison terms as a stable comma-separated value."""

    return ",".join(terms) if terms else "none"


def summary_line(
    old: LogSummary,
    new: LogSummary,
    verdict: str,
    regressions: list[str],
    advancements: list[str],
) -> str:
    """Build the concise human-readable milestone comparison value."""

    return (
        f"{verdict}: old={old.milestone_state()} new={new.milestone_state()} "
        f"regressions={join_terms(regressions)} "
        f"advancements={join_terms(advancements)}"
    )


def to_env_lines(old: LogSummary, new: LogSummary) -> list[str]:
    """Return stable KEY=VALUE comparison lines."""

    old_record = old.to_record()
    new_record = new.to_record()
    regressions, advancements = comparison_terms(old, new)
    score_delta = new.score() - old.score()
    verdict = comparison_verdict(regressions, advancements, score_delta)
    lines: list[str] = []
    for prefix, record in (("OLD", old_record), ("NEW", new_record)):
        for key in FIELD_OUTPUT_ORDER:
            lines.append(f"{prefix}_{key.upper()}={record[key]}")
    lines.extend(
        [
            f"COMPARISON_SCORE_DELTA={score_delta}",
            f"COMPARISON_REGRESSIONS={join_terms(regressions)}",
            f"COMPARISON_ADVANCEMENTS={join_terms(advancements)}",
            f"COMPARISON_VERDICT={verdict}",
            "MILESTONE_COMPARISON_SUMMARY="
            + summary_line(old, new, verdict, regressions, advancements),
        ]
    )
    return lines


class BenchmarkComparisonError(RuntimeError):
    """Benchmark reports are unsafe or ineligible for comparison."""


@dataclass(frozen=True)
class BenchmarkArtifact:
    """One frozen report plus the hash of the complete input file bytes."""

    report: dict[str, object]
    sha256: str


def canonical_json_sha256(value: object) -> str:
    """Hash one JSON value using the benchmark report's canonical encoding."""

    raw = json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=True,
    ).encode("utf-8")
    return hashlib.sha256(raw).hexdigest()


def valid_sha256(value: object) -> bool:
    """Return whether a value is an exact lowercase SHA-256 string."""

    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def json_number(value: object) -> bool:
    """Return whether a value is a finite JSON number but not a boolean."""

    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and value >= 0
        and value not in (float("inf"), float("-inf"))
        and value == value
    )


def _open_directory_chain(path: Path) -> int:
    """Open an existing directory path without following any symlink component."""

    no_follow = getattr(os, "O_NOFOLLOW", None)
    directory_flag = getattr(os, "O_DIRECTORY", None)
    if no_follow is None or directory_flag is None:
        raise OSError("host lacks no-follow directory traversal support")
    absolute = Path(os.path.abspath(os.fspath(path)))
    parts = absolute.parts
    if not parts or parts[0] != os.sep:
        raise OSError("qualified path is not absolute after normalization")
    flags = os.O_RDONLY | no_follow | directory_flag | getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open(os.sep, flags)
    try:
        for component in parts[1:]:
            next_descriptor = os.open(component, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor
    except Exception:
        os.close(descriptor)
        raise


def _open_no_symlink_file(path: Path, flags: int, mode: int = 0o600) -> int:
    """Open one leaf through pinned no-follow ancestor directory descriptors."""

    absolute = Path(os.path.abspath(os.fspath(path)))
    if not absolute.name:
        raise OSError("qualified file path has no leaf name")
    parent_descriptor = _open_directory_chain(absolute.parent)
    try:
        return os.open(
            absolute.name,
            flags
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0),
            mode,
            dir_fd=parent_descriptor,
        )
    finally:
        os.close(parent_descriptor)


def read_benchmark_report(path: Path) -> BenchmarkArtifact:
    """Read one bounded regular summary and select its canonical report."""

    try:
        descriptor = _open_no_symlink_file(path, os.O_RDONLY)
    except OSError as exc:
        raise BenchmarkComparisonError(f"cannot open benchmark report: {path}") from exc
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_size <= 0
            or metadata.st_size > MAX_REPORT_BYTES
        ):
            raise BenchmarkComparisonError("benchmark report has an invalid bounded size")
        chunks: list[bytes] = []
        remaining = metadata.st_size
        while remaining:
            chunk = os.read(descriptor, min(remaining, 1024 * 1024))
            if not chunk:
                raise BenchmarkComparisonError(
                    "benchmark report changed during bounded read"
                )
            chunks.append(chunk)
            remaining -= len(chunk)
        raw = b"".join(chunks)
        final_metadata = os.fstat(descriptor)
        if (
            os.read(descriptor, 1)
            or final_metadata.st_dev != metadata.st_dev
            or final_metadata.st_ino != metadata.st_ino
            or final_metadata.st_size != metadata.st_size
            or final_metadata.st_mtime_ns != metadata.st_mtime_ns
        ):
            raise BenchmarkComparisonError("benchmark report changed during bounded read")
    finally:
        os.close(descriptor)
    try:
        def object_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
            result: dict[str, object] = {}
            for key, value in pairs:
                if key in result:
                    raise ValueError(f"duplicate JSON key: {key}")
                result[key] = value
            return result

        payload = json.loads(
            raw,
            object_pairs_hook=object_pairs,
            parse_constant=lambda token: (_ for _ in ()).throw(
                ValueError(f"non-finite JSON value: {token}")
            ),
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as exc:
        raise BenchmarkComparisonError("benchmark report is not valid JSON") from exc
    if not isinstance(payload, dict):
        raise BenchmarkComparisonError("benchmark report must be a JSON object")
    report = payload.get("report", payload)
    if not isinstance(report, dict):
        raise BenchmarkComparisonError("canonical benchmark report is absent")
    return BenchmarkArtifact(report=report, sha256=hashlib.sha256(raw).hexdigest())


def write_exclusive_output(path: Path, rendered: str) -> None:
    """Create one comparison result through a no-follow path without overwrite."""

    descriptor = _open_no_symlink_file(
        path,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL,
    )
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise OSError("comparison output is not a regular file")
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            descriptor = -1
            handle.write(rendered)
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def performance_qualification_identity(
    provenance: dict[str, object],
) -> dict[str, object]:
    """Project immutable performance fields without implying Pi acceptance."""

    return {
        "target": provenance["target"],
        "transport": provenance["transport"],
        "proof_class": provenance["proof_class"],
        **{field: provenance[field] for field in PERFORMANCE_HASH_FIELDS},
        "component_acceptance_sha256": provenance[
            "component_acceptance_sha256"
        ],
        "captured_unix_s": provenance["captured_unix_s"],
    }


def _safe_nonnegative_int(value: object) -> bool:
    """Return whether value is an exactly representable bounded JSON integer."""

    return (
        isinstance(value, int)
        and not isinstance(value, bool)
        and 0 <= value <= MAX_SAFE_JSON_INT
    )


def _validate_worker_exemplars(
    value: object,
    *,
    proof_class: str,
    phase: str,
) -> list[dict[str, object]]:
    """Validate the exact bounded three-role exemplar projection."""

    expected_roles = tuple(EXECUTABLE_ROLE_SLOTS)
    if not isinstance(value, list) or len(value) != len(expected_roles):
        raise BenchmarkComparisonError(
            f"benchmark {phase} requires exactly three Worker role exemplars"
        )
    rows: list[dict[str, object]] = []
    for index, row in enumerate(value):
        if not isinstance(row, dict) or set(row) != WORKER_EXEMPLAR_FIELDS:
            raise BenchmarkComparisonError(
                f"benchmark {phase} Worker exemplar schema is invalid"
            )
        role = row["role"]
        expected_role = expected_roles[index]
        expected_receipt = "none" if role == "worker-heartbeat" else "confirmed"
        if (
            role != expected_role
            or row["lifecycle"] != "ready"
            or row["artifact"] != "verified"
            or row["receipt"] != expected_receipt
            or row["execution_proof"] != proof_class
            or not isinstance(row["worker"], str)
            or re.fullmatch(r"worker-[1-9][0-9]*", row["worker"]) is None
            or not valid_sha256(row["image_sha256"])
        ):
            raise BenchmarkComparisonError(
                f"benchmark {phase} Worker exemplar state is invalid"
            )
        slot = row["slot"]
        core = row["core"]
        if (
            not _safe_nonnegative_int(slot)
            or int(slot) >= EXECUTABLE_ROLE_SLOTS[expected_role]
            or not _safe_nonnegative_int(core)
            or core != EXECUTABLE_ROLE_CORES[expected_role]
        ):
            raise BenchmarkComparisonError(
                f"benchmark {phase} Worker exemplar placement is invalid"
            )
        for field in (
            "lease_epoch",
            "supervisor_generation",
            "cap_generation",
            "ready_sequence",
        ):
            if not _safe_nonnegative_int(row[field]) or row[field] == 0:
                raise BenchmarkComparisonError(
                    f"benchmark {phase} Worker exemplar identity is invalid"
                )
        for field in (
            "control_sequence",
            "receipt_sequence",
            "completion_sequence",
        ):
            if not _safe_nonnegative_int(row[field]):
                raise BenchmarkComparisonError(
                    f"benchmark {phase} Worker exemplar sequence is invalid"
                )
        scheduling_context = row["scheduling_context"]
        if scheduling_context != {"budget_us": 0, "period_us": 0}:
            raise BenchmarkComparisonError(
                f"benchmark {phase} Worker exemplar is not passive"
            )
        inventory = row["object_inventory"]
        if (
            not isinstance(inventory, dict)
            or set(inventory) != OBJECT_INVENTORY_FIELDS
            or any(not _safe_nonnegative_int(item) for item in inventory.values())
        ):
            raise BenchmarkComparisonError(
                f"benchmark {phase} Worker object inventory is invalid"
            )
        rows.append(row)
    return rows


def validate_benchmark_report(
    report: dict[str, object],
    *,
    target: str,
    transport: str,
    proof_class: str,
    reference_unix_s: int,
    max_age_secs: int,
) -> dict[str, object]:
    """Validate exact report provenance, population, and metric contracts."""

    if report.get("schema") != BENCHMARK_REPORT_SCHEMA:
        raise BenchmarkComparisonError("benchmark report schema is unsupported")
    workload = report.get("workload")
    provenance = report.get("provenance")
    population = report.get("population")
    throughput = report.get("throughput")
    reliability = report.get("reliability")
    backpressure = report.get("backpressure")
    latency = report.get("latency")
    if not all(
        isinstance(value, dict)
        for value in (
            workload,
            provenance,
            population,
            throughput,
            reliability,
            backpressure,
            latency,
        )
    ):
        raise BenchmarkComparisonError("benchmark report sections are incomplete")
    assert isinstance(workload, dict)
    assert isinstance(provenance, dict)
    assert isinstance(population, dict)
    assert isinstance(throughput, dict)
    assert isinstance(reliability, dict)
    assert isinstance(backpressure, dict)
    assert isinstance(latency, dict)
    provenance_fields = {
        "schema",
        "qualification",
        "target",
        "transport",
        "proof_class",
        *PERFORMANCE_HASH_FIELDS,
        "component_acceptance_sha256",
        "performance_qualification_sha256",
        "captured_unix_s",
        "workload_sha256",
    }
    if set(provenance) != provenance_fields:
        if "component_acceptance_sha256" not in provenance:
            if target == "qemu":
                raise BenchmarkComparisonError(
                    "QEMU benchmark requires an exact component-acceptance hash"
                )
            if target == "pi4":
                raise BenchmarkComparisonError(
                    "Pi performance qualification requires exact null component acceptance"
                )
        raise BenchmarkComparisonError("benchmark provenance target lane is invalid")
    if (
        provenance.get("schema") != BENCHMARK_PROVENANCE_SCHEMA
        or provenance.get("qualification") != "target-qualified"
        or provenance.get("target") != target
        or provenance.get("transport") != transport
        or provenance.get("proof_class") != proof_class
    ):
        raise BenchmarkComparisonError("benchmark provenance target lane is invalid")
    if any(
        not valid_sha256(provenance.get(field))
        for field in PERFORMANCE_HASH_FIELDS
    ):
        raise BenchmarkComparisonError("benchmark provenance contains an invalid hash")
    component_acceptance = provenance.get("component_acceptance_sha256")
    if target == "qemu" and not valid_sha256(component_acceptance):
        raise BenchmarkComparisonError(
            "QEMU benchmark requires an exact component-acceptance hash"
        )
    if target == "pi4" and component_acceptance is not None:
        raise BenchmarkComparisonError(
            "Pi performance qualification cannot claim component acceptance"
        )
    if not valid_sha256(provenance.get("performance_qualification_sha256")):
        raise BenchmarkComparisonError(
            "benchmark performance-qualification seal is invalid"
        )
    if provenance["performance_qualification_sha256"] != canonical_json_sha256(
        performance_qualification_identity(provenance)
    ):
        raise BenchmarkComparisonError(
            "benchmark performance-qualification seal does not match fields"
        )
    workload_sha256 = canonical_json_sha256(workload)
    if provenance.get("workload_sha256") != workload_sha256:
        raise BenchmarkComparisonError("benchmark workload hash does not match report")
    captured = provenance.get("captured_unix_s")
    if not isinstance(captured, int) or isinstance(captured, bool):
        raise BenchmarkComparisonError("benchmark capture timestamp is invalid")
    age = reference_unix_s - captured
    if age < -300 or age > max_age_secs:
        raise BenchmarkComparisonError("benchmark report is stale or future-dated")

    maximum = population.get("maximum_live_tasks")
    counts = tuple(population.get(field) for field in ("requested", "discovered", "ready"))
    population_observations = population.get("observations")
    if (
        set(population)
        != {
            "mode",
            "maximum_live_tasks",
            "requested",
            "discovered",
            "ready",
            "backend_class",
            "proof_class",
            "observations",
        }
        or population.get("mode") != "executable"
        or population.get("backend_class") != "console-projection"
        or population.get("proof_class") != proof_class
        or not isinstance(maximum, int)
        or isinstance(maximum, bool)
        or maximum != CANONICAL_EXECUTABLE_WORKERS
        or counts != (maximum, maximum, maximum)
        or not isinstance(population_observations, list)
        or not 1 <= len(population_observations) <= 128
        or any(
            not isinstance(observation, dict)
            or set(observation)
            != {
                "requested",
                "discovered",
                "ready",
                "backend_class",
                "proof_class",
            }
            or observation.get("requested") != maximum
            or observation.get("discovered") != maximum
            or observation.get("ready") != maximum
            or observation.get("backend_class") != "console-projection"
            or observation.get("proof_class") != proof_class
            for observation in population_observations
        )
    ):
        raise BenchmarkComparisonError("benchmark lacks the exact aggregate READY population")
    if (
        workload.get("population_mode") != "executable"
        or workload.get("worker_cap") != maximum
        or workload.get("workers_min") != maximum
        or workload.get("workers_max") != maximum
    ):
        raise BenchmarkComparisonError("benchmark workload differs from population seal")
    if set(workload) != set(CANONICAL_HIGH_WORKLOAD) or any(
        workload.get(field) != expected
        for field, expected in CANONICAL_HIGH_WORKLOAD.items()
    ):
        raise BenchmarkComparisonError(
            "benchmark workload is not the canonical M26e HIGH profile"
        )

    executable_state = report.get("executable_state")
    if not isinstance(executable_state, dict):
        raise BenchmarkComparisonError("benchmark executable state is absent")
    session = executable_state.get("target_session")
    if (
        not isinstance(session, dict)
        or set(session) != EXECUTABLE_TARGET_SESSION_FIELDS
        or any(not valid_sha256(value) for value in session.values())
        or session.get("manifest_sha256") != provenance["manifest_sha256"]
        or session.get("root_image_sha256") != provenance["root_image_sha256"]
        or not valid_sha256(executable_state.get("topology_sha256"))
    ):
        raise BenchmarkComparisonError("benchmark manifest/image session binding mismatches")
    exemplar_rows: dict[str, list[dict[str, object]]] = {}
    for phase in ("pre", "post"):
        snapshot = executable_state.get(phase)
        rows = snapshot.get("workers") if isinstance(snapshot, dict) else None
        census = snapshot.get("ready_census") if isinstance(snapshot, dict) else None
        if (
            not isinstance(census, dict)
            or set(census)
            != {
                "maximum_live_tasks",
                "discovered",
                "ready",
                "topology_sha256",
            }
            or census.get("maximum_live_tasks") != maximum
            or census.get("discovered") != maximum
            or census.get("ready") != maximum
            or census.get("topology_sha256")
            != executable_state.get("topology_sha256")
        ):
            raise BenchmarkComparisonError("benchmark role exemplars or READY census drifted")
        exemplar_rows[phase] = _validate_worker_exemplars(
            rows,
            proof_class=proof_class,
            phase=phase,
        )
    static_exemplar_fields = (
        "role",
        "slot",
        "image_sha256",
        "core",
        "scheduling_context",
        "object_inventory",
    )
    if any(
        any(before[field] != after[field] for field in static_exemplar_fields)
        for before, after in zip(exemplar_rows["pre"], exemplar_rows["post"])
    ):
        raise BenchmarkComparisonError(
            "benchmark Worker exemplar static placement changed during pressure"
        )
    heartbeat_pre = exemplar_rows["pre"][0]
    heartbeat_post = exemplar_rows["post"][0]
    if (
        heartbeat_post["supervisor_generation"]
        <= heartbeat_pre["supervisor_generation"]
        or heartbeat_post["worker"] == heartbeat_pre["worker"]
    ):
        raise BenchmarkComparisonError(
            "benchmark Heartbeat exemplar did not retain fresh-generation recreation"
        )
    for index, role in enumerate(("worker-gpu", "worker-lora"), start=1):
        before = exemplar_rows["pre"][index]
        after = exemplar_rows["post"][index]
        if any(
            before[field] != after[field]
            for field in (
                "role",
                "slot",
                "lease_epoch",
                "supervisor_generation",
                "cap_generation",
            )
        ) or any(
            after[field] <= before[field]
            for field in ("receipt_sequence", "completion_sequence")
        ):
            raise BenchmarkComparisonError(
                f"benchmark {role} exemplar lifecycle did not advance"
            )
    if target == "pi4":
        embedded = executable_state.get("target_evidence")
        expected = {
            "schema": BENCHMARK_TARGET_EVIDENCE_SCHEMA,
            **{
                key: provenance[key]
                for key in (
                    "target",
                    "transport",
                    "proof_class",
                    *PERFORMANCE_HASH_FIELDS,
                    "component_acceptance_sha256",
                    "performance_qualification_sha256",
                    "captured_unix_s",
                )
            },
        }
        if embedded != expected:
            raise BenchmarkComparisonError("Pi report differs from embedded target evidence")

    if any(
        not json_number(throughput.get(field))
        for field in ("ops_per_s", "ok_ops_per_s", "err_ops_per_s")
    ):
        raise BenchmarkComparisonError("benchmark throughput metrics are invalid")
    error_rate = reliability.get("error_rate")
    error_budget_rate = reliability.get("error_budget_rate")
    error_budget_pass = reliability.get("error_budget_pass")
    count = reliability.get("count")
    ok = reliability.get("ok")
    err = reliability.get("err")
    if (
        not json_number(error_rate)
        or float(error_rate) > 1.0
        or not json_number(error_budget_rate)
        or float(error_budget_rate) > 1.0
        or error_budget_rate != workload.get("error_budget_rate")
        or any(
            not isinstance(value, int)
            or isinstance(value, bool)
            or value < 0
            for value in (count, ok, err)
        )
        or not isinstance(error_budget_pass, bool)
        or error_budget_pass != (float(error_rate) <= float(error_budget_rate))
    ):
        raise BenchmarkComparisonError("benchmark reliability metrics are invalid")
    assert isinstance(count, int)
    assert isinstance(ok, int)
    assert isinstance(err, int)
    expected_error_rate = 0.0 if count == 0 else err / count
    duration_s = float(workload["duration_s"])
    expected_rates = {
        "ops_per_s": count / duration_s,
        "ok_ops_per_s": ok / duration_s,
        "err_ops_per_s": err / duration_s,
    }
    if (
        count != ok + err
        or not math.isclose(
            float(error_rate),
            expected_error_rate,
            rel_tol=1e-12,
            abs_tol=1e-12,
        )
        or any(
            not math.isclose(
                float(throughput[field]),
                expected,
                rel_tol=1e-12,
                abs_tol=1e-12,
            )
            for field, expected in expected_rates.items()
        )
    ):
        raise BenchmarkComparisonError("benchmark counts and rates are inconsistent")
    if any(
        not isinstance(backpressure.get(field), int)
        or isinstance(backpressure.get(field), bool)
        or backpressure[field] < 0
        for field in PARITY_BACKPRESSURE_FIELDS
    ):
        raise BenchmarkComparisonError("benchmark backpressure metrics are invalid")
    if set(latency) != LATENCY_FIELDS or any(
        not json_number(value) for value in latency.values()
    ):
        raise BenchmarkComparisonError("benchmark latency metrics are invalid")
    if not (
        float(latency["min_s"])
        <= float(latency["p50_s"])
        <= float(latency["p90_s"])
        <= float(latency["p95_s"])
        <= float(latency["p99_s"])
        <= float(latency["max_s"])
        and float(latency["min_s"])
        <= float(latency["avg_s"])
        <= float(latency["max_s"])
    ):
        raise BenchmarkComparisonError("benchmark latency ordering is inconsistent")
    return report


def compare_benchmark_reports(
    qemu_report: dict[str, object],
    pi_report: dict[str, object],
    *,
    reference_unix_s: int,
    max_age_secs: int,
    min_throughput_ratio: float,
    genet_max_p95_ms: float,
    qemu_input_sha256: str,
    pi_input_sha256: str,
    wifi_report: dict[str, object] | None = None,
    wifi_min_ok_ops_per_s: float | None = None,
    wifi_max_p95_ms: float | None = None,
    wifi_input_sha256: str | None = None,
) -> dict[str, object]:
    """Compare wired Pi throughput without using QEMU latency in the verdict."""

    if (
        not math.isfinite(min_throughput_ratio)
        or min_throughput_ratio < 1.0
        or not isinstance(reference_unix_s, int)
        or isinstance(reference_unix_s, bool)
        or not isinstance(max_age_secs, int)
        or isinstance(max_age_secs, bool)
        or max_age_secs <= 0
        or max_age_secs > MAX_BENCHMARK_AGE_SECS
        or not valid_sha256(qemu_input_sha256)
        or not valid_sha256(pi_input_sha256)
        or not math.isfinite(genet_max_p95_ms)
        or genet_max_p95_ms <= 0
        or genet_max_p95_ms > 60_000
    ):
        raise BenchmarkComparisonError("comparison bounds or input seal are invalid")
    if wifi_report is None and wifi_input_sha256 is not None:
        raise BenchmarkComparisonError("WiFi input seal was supplied without a report")
    if wifi_min_ok_ops_per_s is not None and (
        not math.isfinite(wifi_min_ok_ops_per_s)
        or wifi_min_ok_ops_per_s < 0
    ):
        raise BenchmarkComparisonError("WiFi industry-norm threshold is invalid")
    if wifi_max_p95_ms is not None and (
        not math.isfinite(wifi_max_p95_ms)
        or wifi_max_p95_ms <= 0
        or wifi_max_p95_ms > 60_000
    ):
        raise BenchmarkComparisonError("WiFi latency-norm threshold is invalid")
    qemu = validate_benchmark_report(
        qemu_report,
        target="qemu",
        transport="qemu",
        proof_class="qemu",
        reference_unix_s=reference_unix_s,
        max_age_secs=max_age_secs,
    )
    pi = validate_benchmark_report(
        pi_report,
        target="pi4",
        transport="genet",
        proof_class="fresh-pi",
        reference_unix_s=reference_unix_s,
        max_age_secs=max_age_secs,
    )
    qemu_provenance = qemu["provenance"]
    pi_provenance = pi["provenance"]
    if qemu["workload"] != pi["workload"]:
        raise BenchmarkComparisonError("QEMU and Pi workloads do not match")
    if qemu_provenance["source_sha256"] != pi_provenance["source_sha256"]:
        raise BenchmarkComparisonError("QEMU and Pi source identities do not match")
    if qemu["population"]["maximum_live_tasks"] != pi["population"][
        "maximum_live_tasks"
    ]:
        raise BenchmarkComparisonError("QEMU and Pi population seals do not match")
    for field in (
        "worker_archive_sha256",
        "worker_image_manifest_sha256",
        "worker_abi_sha256",
    ):
        if qemu["executable_state"]["target_session"][field] != pi[
            "executable_state"
        ]["target_session"][field]:
            raise BenchmarkComparisonError(
                "QEMU and Pi target-neutral Worker artifacts do not match"
            )
    qemu_worker_images = {
        row["role"]: row["image_sha256"]
        for row in qemu["executable_state"]["pre"]["workers"]
    }
    pi_worker_images = {
        row["role"]: row["image_sha256"]
        for row in pi["executable_state"]["pre"]["workers"]
    }
    if qemu_worker_images != pi_worker_images:
        raise BenchmarkComparisonError(
            "QEMU and Pi Worker role image identities do not match"
        )

    qemu_ok = float(qemu["throughput"]["ok_ops_per_s"])
    pi_ok = float(pi["throughput"]["ok_ops_per_s"])
    if qemu_ok <= 0:
        raise BenchmarkComparisonError("QEMU successful throughput must be positive")
    throughput_ratio = pi_ok / qemu_ok
    throughput_pass = throughput_ratio >= min_throughput_ratio
    error_budget_pass = (
        qemu["reliability"]["error_budget_pass"] is True
        and pi["reliability"]["error_budget_pass"] is True
    )
    pressure_deltas = {
        field: int(pi["backpressure"][field]) - int(qemu["backpressure"][field])
        for field in PARITY_BACKPRESSURE_FIELDS
    }
    wired_pass = throughput_pass and error_budget_pass
    pi_p95_ms = float(pi["latency"]["p95_s"]) * 1000.0
    result: dict[str, object] = {
        "schema": "cohesix-qemu-pi-throughput-comparison/v1",
        "verdict": "PASS" if wired_pass else "FAIL",
        "lane": "pi4-genet-vs-qemu",
        "source_sha256": qemu_provenance["source_sha256"],
        "workload_sha256": qemu_provenance["workload_sha256"],
        "population": qemu["population"]["maximum_live_tasks"],
        "comparison_bounds": {
            "reference_unix_s": reference_unix_s,
            "max_age_secs": max_age_secs,
            "minimum_throughput_ratio": min_throughput_ratio,
        },
        "inputs": {
            "qemu_sha256": qemu_input_sha256,
            "pi_sha256": pi_input_sha256,
            "wifi_sha256": wifi_input_sha256,
        },
        "freshness": {
            "qemu": {
                "captured_unix_s": qemu_provenance["captured_unix_s"],
                "age_secs": reference_unix_s - qemu_provenance["captured_unix_s"],
            },
            "pi": {
                "captured_unix_s": pi_provenance["captured_unix_s"],
                "age_secs": reference_unix_s - pi_provenance["captured_unix_s"],
            },
            "wifi": None,
        },
        "throughput": {
            "qemu_ok_ops_per_s": qemu_ok,
            "pi_ok_ops_per_s": pi_ok,
            "pi_to_qemu_ratio": throughput_ratio,
            "minimum_ratio": min_throughput_ratio,
            "pass": throughput_pass,
        },
        "errors": {
            "qemu_error_rate": qemu["reliability"]["error_rate"],
            "pi_error_rate": pi["reliability"]["error_rate"],
            "qemu_errors": qemu["reliability"]["err"],
            "pi_errors": pi["reliability"]["err"],
            "qemu_error_budget_pass": qemu["reliability"]["error_budget_pass"],
            "pi_error_budget_pass": pi["reliability"]["error_budget_pass"],
            "qemu_error_budget_rate": qemu["reliability"]["error_budget_rate"],
            "pi_error_budget_rate": pi["reliability"]["error_budget_rate"],
            "error_budget_pass": error_budget_pass,
            "comparative_counts_included_in_verdict": False,
            "error_budget_included_in_verdict": True,
        },
        "backpressure": {
            "pi_minus_qemu": pressure_deltas,
            "qemu": {
                field: qemu["backpressure"][field]
                for field in PARITY_BACKPRESSURE_FIELDS
            },
            "pi": {
                field: pi["backpressure"][field]
                for field in PARITY_BACKPRESSURE_FIELDS
            },
            "included_in_verdict": False,
        },
        "latency": {
            "qemu": qemu["latency"],
            "pi": pi["latency"],
            "pi_p95_ms": pi_p95_ms,
            "pi_p95_max_ms": genet_max_p95_ms,
            "physical_norm_status": (
                "PASS" if pi_p95_ms <= genet_max_p95_ms else "FLAG"
            ),
            "included_in_verdict": False,
        },
        "wifi": {"status": "not-supplied", "included_in_wired_verdict": False},
    }
    if wifi_report is not None:
        if not valid_sha256(wifi_input_sha256):
            raise BenchmarkComparisonError("WiFi report input seal is invalid")
        wifi = validate_benchmark_report(
            wifi_report,
            target="pi4",
            transport="wifi",
            proof_class="fresh-pi",
            reference_unix_s=reference_unix_s,
            max_age_secs=max_age_secs,
        )
        wifi_provenance = wifi["provenance"]
        if wifi["workload"] != pi["workload"] or any(
            wifi_provenance[field] != pi_provenance[field]
            for field in (
                "source_sha256",
                "manifest_sha256",
                "image_sha256",
                "root_image_sha256",
                "target_session_sha256",
            )
        ):
            raise BenchmarkComparisonError("WiFi diagnostic provenance does not match")
        if wifi["executable_state"]["target_session"] != pi["executable_state"][
            "target_session"
        ]:
            raise BenchmarkComparisonError(
                "WiFi diagnostic target-neutral Worker artifacts do not match"
            )
        wifi_worker_images = {
            row["role"]: row["image_sha256"]
            for row in wifi["executable_state"]["pre"]["workers"]
        }
        if wifi_worker_images != pi_worker_images:
            raise BenchmarkComparisonError(
                "WiFi diagnostic Worker role image identities do not match"
            )
        wifi_ok = float(wifi["throughput"]["ok_ops_per_s"])
        industry_pass = (
            None
            if wifi_min_ok_ops_per_s is None
            else wifi_ok >= wifi_min_ok_ops_per_s
            and int(wifi["reliability"]["err"]) == 0
        )
        wifi_p95_ms = float(wifi["latency"]["p95_s"]) * 1000.0
        result["wifi"] = {
            "status": "not-evaluated" if industry_pass is None else (
                "PASS" if industry_pass else "FAIL"
            ),
            "ok_ops_per_s": wifi_ok,
            "industry_min_ok_ops_per_s": wifi_min_ok_ops_per_s,
            "latency": wifi["latency"],
            "p95_ms": wifi_p95_ms,
            "p95_max_ms": wifi_max_p95_ms,
            "latency_norm_status": (
                "not-evaluated"
                if wifi_max_p95_ms is None
                else ("PASS" if wifi_p95_ms <= wifi_max_p95_ms else "FLAG")
            ),
            "included_in_wired_verdict": False,
        }
        result["freshness"]["wifi"] = {
            "captured_unix_s": wifi["provenance"]["captured_unix_s"],
            "age_secs": (
                reference_unix_s - wifi["provenance"]["captured_unix_s"]
            ),
        }
    return result


def build_parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""

    parser = argparse.ArgumentParser(
        description="Compare Pi driver logs or qualified QEMU/Pi benchmarks."
    )
    parser.add_argument("--old", type=Path, help="old serial log path")
    parser.add_argument("--new", type=Path, help="new serial log path")
    parser.add_argument("--qemu-report", type=Path, help="qualified QEMU summary JSON")
    parser.add_argument("--pi-report", type=Path, help="qualified Pi GENET summary JSON")
    parser.add_argument(
        "--wifi-report",
        type=Path,
        help="optional separate qualified Pi WiFi summary JSON",
    )
    parser.add_argument(
        "--wifi-min-ok-ops-per-s",
        type=float,
        default=None,
        help="explicit external WiFi industry-norm throughput floor",
    )
    parser.add_argument(
        "--reference-unix-s",
        type=int,
        default=None,
        help="comparison clock for deterministic freshness checks",
    )
    parser.add_argument(
        "--max-age-secs",
        type=int,
        default=DEFAULT_MAX_AGE_SECS,
        help="maximum accepted target-evidence age (default: %(default)s)",
    )
    parser.add_argument(
        "--min-throughput-ratio",
        type=float,
        default=1.0,
        help="required Pi GENET/QEMU successful-throughput ratio (default: %(default)s)",
    )
    parser.add_argument(
        "--genet-max-p95-ms",
        type=float,
        default=None,
        help="documented physical-LAN/control-plane p95 ceiling",
    )
    parser.add_argument(
        "--wifi-max-p95-ms",
        type=float,
        default=None,
        help="documented WiFi/control-plane p95 ceiling",
    )
    parser.add_argument("--output", type=Path, help="optional comparison JSON path")
    return parser


def main(argv: list[str] | None = None) -> int:
    """CLI entry point."""

    args = build_parser().parse_args(argv)
    benchmark_mode = args.qemu_report is not None or args.pi_report is not None
    if benchmark_mode:
        if args.old is not None or args.new is not None:
            raise SystemExit("benchmark reports cannot be mixed with --old/--new logs")
        if args.qemu_report is None or args.pi_report is None:
            raise SystemExit("benchmark comparison requires --qemu-report and --pi-report")
        if (
            args.max_age_secs <= 0
            or args.max_age_secs > MAX_BENCHMARK_AGE_SECS
        ):
            raise SystemExit("--max-age-secs must be in 1..604800")
        if (
            not math.isfinite(args.min_throughput_ratio)
            or args.min_throughput_ratio < 1.0
        ):
            raise SystemExit("--min-throughput-ratio must be finite and >= 1.0")
        if (
            args.genet_max_p95_ms is None
            or not math.isfinite(args.genet_max_p95_ms)
            or args.genet_max_p95_ms <= 0
            or args.genet_max_p95_ms > 60_000
        ):
            raise SystemExit(
                "benchmark comparison requires finite --genet-max-p95-ms in (0,60000]"
            )
        if args.wifi_min_ok_ops_per_s is not None and (
            args.wifi_report is None
            or not math.isfinite(args.wifi_min_ok_ops_per_s)
            or args.wifi_min_ok_ops_per_s < 0
        ):
            raise SystemExit(
                "--wifi-min-ok-ops-per-s requires --wifi-report and a nonnegative value"
            )
        if args.wifi_report is not None and (
            args.wifi_max_p95_ms is None
            or not math.isfinite(args.wifi_max_p95_ms)
            or args.wifi_max_p95_ms <= 0
            or args.wifi_max_p95_ms > 60_000
        ):
            raise SystemExit(
                "--wifi-report requires finite --wifi-max-p95-ms in (0,60000]"
            )
        if args.wifi_report is None and args.wifi_max_p95_ms is not None:
            raise SystemExit("--wifi-max-p95-ms requires --wifi-report")
        current_unix_s = int(time.time())
        reference = (
            current_unix_s
            if args.reference_unix_s is None
            else args.reference_unix_s
        )
        if abs(reference - current_unix_s) > MAX_REFERENCE_CLOCK_SKEW_SECS:
            raise SystemExit(
                "--reference-unix-s must be within 300 seconds of the host clock"
            )
        try:
            qemu_artifact = read_benchmark_report(args.qemu_report)
            pi_artifact = read_benchmark_report(args.pi_report)
            wifi_artifact = (
                None
                if args.wifi_report is None
                else read_benchmark_report(args.wifi_report)
            )
            result = compare_benchmark_reports(
                qemu_artifact.report,
                pi_artifact.report,
                reference_unix_s=reference,
                max_age_secs=args.max_age_secs,
                min_throughput_ratio=args.min_throughput_ratio,
                genet_max_p95_ms=args.genet_max_p95_ms,
                qemu_input_sha256=qemu_artifact.sha256,
                pi_input_sha256=pi_artifact.sha256,
                wifi_report=(None if wifi_artifact is None else wifi_artifact.report),
                wifi_min_ok_ops_per_s=args.wifi_min_ok_ops_per_s,
                wifi_max_p95_ms=args.wifi_max_p95_ms,
                wifi_input_sha256=(
                    None if wifi_artifact is None else wifi_artifact.sha256
                ),
            )
        except BenchmarkComparisonError as exc:
            print(f"comparison rejected: {exc}", file=sys.stderr)
            return 2
        rendered = json.dumps(result, indent=2, sort_keys=True) + "\n"
        if args.output is not None:
            try:
                write_exclusive_output(args.output, rendered)
            except OSError as exc:
                print(f"comparison output refused: {exc}", file=sys.stderr)
                return 2
        print(rendered, end="")
        return 0 if result["verdict"] == "PASS" else 1
    if args.old is None or args.new is None:
        raise SystemExit("serial comparison requires --old and --new")
    old = summarize_log("old", read_log(args.old))
    new = summarize_log("new", read_log(args.new))
    print("\n".join(to_env_lines(old, new)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
