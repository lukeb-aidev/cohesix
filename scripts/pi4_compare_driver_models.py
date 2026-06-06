#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Compare old and new Raspberry Pi 4 driver-model serial logs.
# Copyright 2026 Lukas Bower

"""Deterministically compare old and new Pi 4 driver-model logs.

The comparator is intentionally conservative: it reports evidence present in
the two supplied logs and does not infer hardware acceptance beyond the
observable serial breadcrumbs.
"""

from __future__ import annotations

import argparse
import re
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable


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
    "0x0213": "address-device-failed",
    "0213": "address-device-failed",
    "531": "address-device-failed",
}

WIFI_DETAIL_BLOCKERS = {
    "0x5329": "cyw43-firmware-retry-exhausted",
    "5329": "cyw43-firmware-retry-exhausted",
    "21289": "cyw43-firmware-retry-exhausted",
}


def normalized_detail_blocker(
    fields: dict[str, str], mapping: dict[str, str]
) -> str | None:
    """Return a stable blocker name for known numeric detail fields."""

    detail = fields.get("detail")
    if detail is None:
        return None
    return mapping.get(detail.strip().lower())


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
    if (
        "runtime keyboard first-byte" in lowered
        or "source=first-byte" in lowered
        or fields.get("first_byte") == "yes"
        or (
            fields.get("proof_gate") == "10"
            and fields.get("blocker") == "none"
            and fields.get("keyboard") == "yes"
        )
    ):
        summary.usb_first_byte_seen = True
    structured_blocker = first_non_none(
        (
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
        or "failed" in blocker
    ):
        summary.usb_blocker_seen = True
        if summary.usb_blocker == "none" or structured_blocker != "none":
            summary.usb_blocker = normalize_blocker(blocker)


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


def build_parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""

    parser = argparse.ArgumentParser(
        description="Compare old and new Pi 4 driver-model serial logs."
    )
    parser.add_argument("--old", required=True, type=Path, help="old log path")
    parser.add_argument("--new", required=True, type=Path, help="new log path")
    return parser


def main(argv: list[str] | None = None) -> int:
    """CLI entry point."""

    args = build_parser().parse_args(argv)
    old = summarize_log("old", read_log(args.old))
    new = summarize_log("new", read_log(args.new))
    print("\n".join(to_env_lines(old, new)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
