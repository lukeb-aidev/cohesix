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
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Callable, Iterable, Mapping, TextIO


KEY_VALUE_RE = re.compile(
    r"(?P<key>[A-Za-z0-9_.:-]+)=(?P<value>\"[^\"]*\"|'[^']*'|[^ \t\r\n]+)"
)
UNSUPPORTED_OPERATION_FIELD_RE = re.compile(
    r"(?P<key>[A-Za-z0-9_.:-]+)=unsupported operation: "
    r"(?P<value>[A-Za-z0-9_.:-]+)"
)
CYW43_CONTROL_PLANE_EXACT_RE = re.compile(r"cyw43-control-plane-[a-z0-9-]+")
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
PANIC_REASON_TOKEN_RE = re.compile(r"[^A-Za-z0-9]+")
WIFI_SECRET_REDACTIONS = (
    re.compile(r"(?i)(coh_wifi_psk=)([^ \t\r\n;]+)"),
    re.compile(r"(?i)(cohesix,wifi-psk=)([^ \t\r\n;]+)"),
    re.compile(r"(?i)(setenv\s+coh_wifi_psk\s+)([^;\r\n]+)"),
    re.compile(
        r"(?i)(Wi-Fi password \(leave blank for an open network\):\s*)(.*)"
    ),
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
    r"|\[cohsh-net\]"
    r"|\[net-console\]"
    r"|\[smp\]"
    r"|\[timers\]"
    r"|SERIAL_INPUT_TRACE"
    r"|HDMI_FRAME_"
    r"|(?<![A-Za-z0-9_.:-])DRIVER_TASK_"
    r"|(?<![A-Za-z0-9_.:-])SCHED_CONTRACT"
    r"|CYW43_"
    r"|(?<![A-Za-z0-9_.:-])(?:usb:|USB:|wifi:|WiFi:|WIFI:)"
    r"|(?<![A-Za-z0-9_.:-])(?:OK|ERR) NETTEST"
    r"|Kernel entry via Interrupt"
    r"))"
)
MALFORMED_WIFI_PREFIX_RE = re.compile(r"(?<![A-Za-z0-9_.:-])(?:wif|wi):")
STARTUP_DIAG_GATE_RE = re.compile(
    r"^(?P<domain>usb|wifi): gate (?P<gate>[0-9]+)\b", re.IGNORECASE
)
CYW43_SDIO_DPC_RE = re.compile(
    r"^CYW43_SDIO_DPC "
    r"generation=(?P<generation>[0-9]+) "
    r"captures=(?P<captures>[0-9]+) "
    r"published=(?P<published>[0-9]+) "
    r"consumed=(?P<consumed>[0-9]+) "
    r"rearms=(?P<rearms>[0-9]+) "
    r"overruns=(?P<overruns>[0-9]+) "
    r"epoch_errors=(?P<epoch_errors>[0-9]+) "
    r"sequence_errors=(?P<sequence_errors>[0-9]+) "
    r"ack_failures=(?P<ack_failures>[0-9]+) "
    r"poisoned=(?P<poisoned>yes|no)"
    r"(?: masked=(?P<masked>yes|no))?$"
)
CYW43_BOOTSTRAP_SUPERVISOR_BASE_RE = re.compile(
    r"^CYW43_BOOTSTRAP_SUPERVISOR "
    r"attempt=(?P<attempt>[0-9]+) "
    r"status=(?P<status>[a-z0-9-]+) "
    r"backoff_ms=(?P<backoff_ms>[0-9]+) "
    r"next_attempt_ms=(?P<next_attempt_ms>[0-9]+)"
    r"(?P<production_suffix>.*)$"
)
CYW43_BOOTSTRAP_SUPERVISOR_PRODUCTION_SUFFIX_RE = re.compile(
    r"^ serial=(?P<serial>ready|blocked) local_seat=(?:ready|disabled) "
    r"recovery=full "
    r"console_seq=(?P<console_seq>[0-9]+) "
    r"telemetry_sinks=serial\+qlog\+hdmi "
    r"prompt_refresh=yes$"
)
CYW43_BOOTSTRAP_RETRY_BACKOFF_MS = (1_000, 2_000, 4_000, 8_000)
CYW43_BOOTSTRAP_MAX_ATTEMPTS = 5
U64_MAX = (1 << 64) - 1
CYW43_BOOTSTRAP_NO_ATTEMPT_MS = U64_MAX
CYW43_BOOTSTRAP_SERIAL_RETRY_MS = 250
USB_HINTS = ("usb", "xhci", "vl805", "keyboard", "local-seat", "usbhid")
WIFI_HINTS = ("wifi", "wi-fi", "wlan", "cyw", "brcmf", "sdio", "sdhci", "mmc")
UBOOT_WIFI_POLICY_MISSING_MARKERS = (
    "u-boot policy missing",
    "has no saved wi-fi credentials",
    "serial wi-fi password entry is disabled because u-boot echoes input",
    "file-based policy recovery",
    "no wi-fi network is configured and local usb input is unavailable",
)
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
    "controller-init": 4,
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
    "command-input-ready": 10,
    "first-byte": 9,
    "console-byte": 10,
}
USB_RUNTIME_DETAIL_GATES = {
    0x0201: (3, "command-event-ring-not-proven"),
    0x0202: (8, "none"),
    0x0203: (4, "enable-slot-completion-pending"),
    0x0204: (4, "command-ring-ready"),
    0x0205: (5, "root-port-connected"),
    0x0206: (6, "device-addressed"),
    0x0207: (7, "device-descriptor"),
    0x0208: (7, "config-descriptor"),
    0x0210: (7, "hub-topology-no-keyboard"),
    0x0211: (7, "hid-endpoint-not-ready"),
    0x0212: (5, "enable-slot-failed"),
    0x0213: (6, "address-device-failed"),
    0x0214: (6, "device-descriptor-failed"),
    0x0215: (7, "config-descriptor-failed"),
    0x0216: (7, "hid-attach-failed"),
    0x0217: (7, "hub-attach-failed"),
    0x0218: (7, "hub-set-configuration-failed"),
    0x0219: (7, "hub-descriptor-failed"),
    0x021A: (7, "hub-context-failed"),
    0x0500: (9, "hid-first-report"),
    0x0501: (9, "command-input-ready"),
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
JOIN_SECURITY_EXACT_BY_BLOCKER = {
    "join-security-wpaie-loop": "cyw43-join-security-wpaie-loop",
    "join-security-wpa-auth-initial-loop": "cyw43-join-security-wpa-auth-initial-loop",
    "join-security-wpa-auth-final-loop": "cyw43-join-security-wpa-auth-final-loop",
    "join-security-auth-loop": "cyw43-join-security-auth-loop",
    "join-security-wsec-first-loop": "cyw43-join-security-wsec-first-loop",
    "join-security-sup-wpa-loop": "cyw43-join-security-sup-wpa-loop",
    "join-security-bsscfg-sup-wpa-loop": "cyw43-join-security-bsscfg-sup-wpa-loop",
}
USB_OUTCOME_BLOCKERS = {
    "address-device-failed",
    "address-failed",
    "awaiting-physical-key",
    "config-descriptor",
    "config-descriptor-failed",
    "config-parse",
    "command-event-ring-not-proven",
    "device-descriptor",
    "device-descriptor-failed",
    "driver-task-runtime-deferred",
    "enumeration-disabled-bootloader-owned",
    "enable-slot-failed",
    "enable-slot-completion-pending",
    "hid-attach-failed",
    "hid-first-report",
    "hid-endpoint-not-ready",
    "hid-endpoint-parse-no-reply",
    "hid-endpoint-not-found",
    "hid-interface-not-found",
    "hid-interrupt-in-not-found",
    "hid-config-descriptor-malformed",
    "hub-child-scan-no-reply",
    "hub-child-probe-no-reply",
    "hub-topology-no-keyboard",
    "hid-configure-endpoint-no-reply",
    "hid-configure-endpoint-failed",
    "hid-set-configuration-no-reply",
    "hid-set-configuration-failed",
    "hid-control-no-reply",
    "hid-control-failed",
    "hid-interrupt-queue-no-reply",
    "hid-interrupt-queue-failed",
    "usb-hid-interrupt-no-completion",
    "root-port-reset-no-reply",
    "root-port-connect-no-reply",
    "root-port-connect-timeout",
    "root-port-reset-completion-no-reply",
    "root-port-enable-no-reply",
    "root-port-enable-timeout",
    "root-port-reset-timeout",
    "root-port-reset-retry",
    "root-port-reset-failed",
    "root-port-stale-cleanup-no-reply",
    "root-port-stale-cleanup-failed",
    "address-enable-slot-no-reply",
    "address-device-context-publish-no-reply",
    "address-device-command-submit-no-reply",
    "address-device-command-completion-no-reply",
    "address-device-publish-no-reply",
    "device-descriptor-no-reply",
    "device-descriptor-submit-no-reply",
    "device-descriptor-transfer-no-reply",
    "device-descriptor-status-no-reply",
    "device-descriptor-transfer-failed",
    "device-descriptor-transfer-timeout",
    "device-descriptor-status-timeout",
    "device-descriptor-transfer-event-slot-empty",
    "device-descriptor-transfer-event-cycle-mismatch",
    "device-descriptor-transfer-event-ignored",
    "device-descriptor-status-event-slot-empty",
    "device-descriptor-status-event-cycle-mismatch",
    "device-descriptor-status-event-ignored",
    "device-descriptor-prime-submit-no-reply",
    "device-descriptor-prime-transfer-no-reply",
    "device-descriptor-prime-status-no-reply",
    "device-descriptor-full-read-no-reply",
    "device-descriptor-prime-transfer-failed",
    "device-descriptor-prime-transfer-timeout",
    "device-descriptor-prime-status-timeout",
    "device-descriptor-prime-transfer-event-slot-empty",
    "device-descriptor-prime-transfer-event-cycle-mismatch",
    "device-descriptor-prime-transfer-event-ignored",
    "device-descriptor-prime-status-event-slot-empty",
    "device-descriptor-prime-status-event-cycle-mismatch",
    "device-descriptor-prime-status-event-ignored",
    "config-descriptor-no-reply",
    "config-descriptor-header-submit-no-reply",
    "config-descriptor-header-transfer-no-reply",
    "config-descriptor-header-status-no-reply",
    "config-descriptor-full-read-no-reply",
    "config-descriptor-header-transfer-failed",
    "config-descriptor-header-transfer-timeout",
    "config-descriptor-header-status-timeout",
    "config-descriptor-header-transfer-event-slot-empty",
    "config-descriptor-header-transfer-event-cycle-mismatch",
    "config-descriptor-header-transfer-event-ignored",
    "config-descriptor-header-status-event-slot-empty",
    "config-descriptor-header-status-event-cycle-mismatch",
    "config-descriptor-header-status-event-ignored",
    "config-descriptor-full-submit-no-reply",
    "config-descriptor-full-transfer-no-reply",
    "config-descriptor-full-status-no-reply",
    "config-descriptor-full-transfer-failed",
    "config-descriptor-full-transfer-timeout",
    "config-descriptor-full-status-timeout",
    "config-descriptor-full-transfer-event-slot-empty",
    "config-descriptor-full-transfer-event-cycle-mismatch",
    "config-descriptor-full-transfer-event-ignored",
    "config-descriptor-full-status-event-slot-empty",
    "config-descriptor-full-status-event-cycle-mismatch",
    "config-descriptor-full-status-event-ignored",
    "hub-attach-failed",
    "hub-context-failed",
    "hub-descriptor-failed",
    "hub-set-configuration-failed",
    "hub-set-configuration-no-reply",
    "hub-set-configuration-status-no-reply",
    "hub-set-configuration-complete-no-reply",
    "hub-set-configuration-status-timeout",
    "hub-set-configuration-status-event-slot-empty",
    "hub-set-configuration-status-event-cycle-mismatch",
    "hub-set-configuration-status-event-ignored",
    "hub-set-configuration-settle-no-reply",
    "hub-descriptor-no-reply",
    "hub-descriptor-transfer-no-reply",
    "hub-descriptor-status-no-reply",
    "hub-descriptor-transfer-failed",
    "hub-descriptor-transfer-timeout",
    "hub-descriptor-status-timeout",
    "hub-descriptor-transfer-event-slot-empty",
    "hub-descriptor-transfer-event-cycle-mismatch",
    "hub-descriptor-transfer-event-ignored",
    "hub-descriptor-status-event-slot-empty",
    "hub-descriptor-status-event-cycle-mismatch",
    "hub-descriptor-status-event-ignored",
    "hub-context-no-reply",
    "hub-port-power-no-reply",
    "hub-port-status-no-reply",
    "hub-port-status-transfer-no-reply",
    "hub-port-status-status-no-reply",
    "hub-port-status-transfer-timeout",
    "hub-port-status-timeout",
    "hub-port-status-transfer-event-slot-empty",
    "hub-port-status-transfer-event-cycle-mismatch",
    "hub-port-status-transfer-event-ignored",
    "hub-port-status-status-event-slot-empty",
    "hub-port-status-status-event-cycle-mismatch",
    "hub-port-status-status-event-ignored",
    "hub-port-status-payload-no-reply",
    "hub-port-disconnected",
    "hub-port-reset-still-active",
    "hub-port-enable-missing",
    "hub-port-clear-changes-no-reply",
    "hub-port-clear-changes-failed",
    "hub-port-status-failed",
    "hub-port-reset-no-reply",
    "hub-port-reset-set-no-reply",
    "hub-port-reset-set-failed",
    "hub-port-reset-completion-no-reply",
    "hub-child-scan-no-reply",
    "hub-child-probe-no-reply",
    "hub-child-speed-fallback-no-reply",
    "hub-topology-no-keyboard",
    "hid-init-failed",
    "hid-interrupt-in",
    "hid-queue-read-failed",
    "hid-first-byte",
    "invalid-config-value",
    "keyboard-first-byte",
    "no-connected-ports",
    "no-keyboard-found",
    "usb-xhci-ready-keyboard-not-enumerated",
    "pcie-xhci-device-coverage-missing",
    "pcie-owner-ring-unavailable",
    "pcie-vl805",
    "pcie-vl805-config-contract-missing",
    "root-port-read-begin",
    "root-port-read-timer-preempted",
    "root-port-sample-deferred",
    "reset-hcrst-timeout",
    "reset-pre-usbcmd-source",
    "reset-pre-usbcmd-source-timer-preempted",
    "reset-controller-not-halted",
    "reset-pre-hcrst-controller-not-ready",
    "reset-controller-not-ready",
    "port-register-access-disabled",
    "port-reset-timeout",
    "port-enable-timeout",
    "root-port-device-not-found",
    "address-device-timeout",
    "address-device-pending",
    "captured-root-port-enum",
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
    "xhci trust tokens cleared before cohesix cold boot",
    "xhci cold boot starts unseeded",
)
USB_STALE_UEFI_HINT_MARKERS = (
    "uefi vars: xhcipci=0 xhcireload=1 systemtablemode=1",
)
DRIVER_TASK_EXPECTED_AFFINITY_CORES = {
    "serial": 1,
    "usb-local-seat": 1,
    "hdmi-text": 2,
    "bcmgenet-v5": 3,
    "cyw43455": 3,
    "sdio-host": 3,
    "pcie-root": 2,
    "rtl8139": 2,
    "virtio-net": 3,
}
PI4_COMMON_DRIVER_TASK_AFFINITY_CONTRACTS = frozenset(
    ("serial", "usb-local-seat", "hdmi-text", "pcie-root")
)
PI4_WIFI_DRIVER_TASK_AFFINITY_CONTRACTS = (
    PI4_COMMON_DRIVER_TASK_AFFINITY_CONTRACTS | {"cyw43455", "sdio-host"}
)
PI4_WIRED_DRIVER_TASK_AFFINITY_CONTRACTS = (
    PI4_COMMON_DRIVER_TASK_AFFINITY_CONTRACTS | {"bcmgenet-v5"}
)
REQUIRED_PI4_DRIVER_TASK_AFFINITY_CONTRACTS = (
    PI4_WIFI_DRIVER_TASK_AFFINITY_CONTRACTS | PI4_WIRED_DRIVER_TASK_AFFINITY_CONTRACTS
)


def pi4_selected_driver_task_affinity_contracts(
    selection: str, selected_only: bool
) -> frozenset[str]:
    """Return the affinity contract set required by a selected Pi 4 profile."""

    normalized = selection.strip().lower()
    if normalized in {"wifi", "cyw43", "cyw43455"}:
        return PI4_WIFI_DRIVER_TASK_AFFINITY_CONTRACTS
    if normalized in {"wired", "nic", "genet", "bcmgenet", "bcmgenet-v5"}:
        return PI4_WIRED_DRIVER_TASK_AFFINITY_CONTRACTS
    if selected_only:
        return REQUIRED_PI4_DRIVER_TASK_AFFINITY_CONTRACTS
    return REQUIRED_PI4_DRIVER_TASK_AFFINITY_CONTRACTS


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
class DriverTaskFrontiers:
    """Fail-closed driver-task frontier details for Pi 4 bring-up logs."""

    serial_driver_accepted: bool = False
    serial_fallback_active: bool = False
    serial_runtime_frontier: str = "none"
    hdmi_descriptor_ready: bool = False
    hdmi_engine_ready: bool = False
    hdmi_owner_state_ready: bool = False
    hdmi_runtime_frontier: str = "none"
    usb_driver_task_frontier: str = "none"
    wifi_replay_frontier: str = "none"


@dataclass(frozen=True)
class SequenceStep:
    """One ordered old-good replay step for reopened Pi 4 acceptance."""

    name: str
    matcher: Callable[["TraceEvent"], bool]


@dataclass(frozen=True)
class SequenceResult:
    """Machine-checkable result for one ordered replay profile."""

    replay: bool
    last: str
    missing: str


@dataclass(frozen=True)
class WifiGate7Subgate:
    """Gate 7 sub-gate frontier with source detail for WiFi bring-up."""

    subgate: str
    name: str
    source: str = "none"
    status: str = "none"
    reason: str = "none"
    line: int = 0


@dataclass(frozen=True)
class WifiGate7Proof:
    """Ordered host-EAPOL Gate 7a through 7e completion proof."""

    complete: bool = False
    seen: str = "none"
    last: str = "none"
    missing: str = "7a"


@dataclass(frozen=True)
class WifiDpcProof:
    """Latest exact CYW43/SDIO DPC proof plus fail-closed verdict."""

    proof: bool = False
    reason: str = "missing"
    generation: int = 0
    captures: int = 0
    published: int = 0
    consumed: int = 0
    rearms: int = 0
    overruns: int = 0
    epoch_errors: int = 0
    sequence_errors: int = 0
    ack_failures: int = 0
    poisoned: str = "unknown"
    masked: str = "unknown"
    line: int = 0


@dataclass(frozen=True)
class Cyw43BootstrapSupervisorProof:
    """Validated terminal state for persistent CYW43 bootstrap retries."""

    seen: bool = False
    max_attempt: int = 0
    transient_retries: int = 0
    last_status: str = "none"
    ready: bool = False
    blocker: str = "none"


@dataclass(frozen=True)
class GateSummary:
    """Current USB/WiFi hardware bring-up gate state."""

    usb_gate: int
    usb_blocker: str
    wifi_gate: int
    wifi_blocker: str
    wifi_subgate: str = "none"
    wifi_subgate_name: str = "none"
    wifi_subgate_source: str = "none"
    wifi_subgate_status: str = "none"
    wifi_subgate_reason: str = "none"
    wifi_subgate_line: int = 0
    wifi_gate7_complete: bool = False
    wifi_gate7_seen: str = "none"
    wifi_gate7_last: str = "none"
    wifi_gate7_missing: str = "7a"
    usb_oldgood_replay: bool = False
    usb_oldgood_last: str = "none"
    usb_oldgood_missing: str = "not-run"
    wifi_oldgood_replay: bool = False
    wifi_oldgood_last: str = "none"
    wifi_oldgood_missing: str = "not-run"
    wifi_exact: str = "none"
    wifi_phase: str = "none"
    wifi_blocker_line: int = 0
    serial_clean: bool = True
    boot_halted: bool = False
    timer_irq27_seen: bool = False
    timer_backend: str = "unknown"
    timer_clock_hz: int = 0
    timer_el0_counter: str = "none"
    dummy_timer_seen: bool = False
    sdio_irq158_seen: bool = False
    sdio_irq158_bound: bool = False
    sdio_irq158_inband_proof: bool = False
    sdio_irq158_line: int = 0
    boot_halt_reason: str = "none"
    panic_seen: bool = False
    panic_reason: str = "none"
    usb_bootloader_handoff_seen: bool = False
    usb_cold_boot_seen: bool = False
    usb_stale_uefi_hint_seen: bool = False
    usb_event_ring_alive: bool = False
    usb_psc_drain_count: int = 0
    usb_psc_drain_mask: int = 0
    root_console_ready: bool = False
    root_prompt_seen: bool = False
    net_active: str = "unknown"
    net_addr_src: str = "unknown"
    net_dhcp: str = "unknown"
    net_tcp_ready: bool = False
    nettest_proof: bool = False
    cohsh_tcp_auth_proof: bool = False
    tcp_accepts: int = 0
    tcp_auth_sessions: int = 0
    tcp_rx_bytes: int = 0
    wifi_data_path_tx: int = 0
    wifi_data_path_rx_preserved: int = 0
    wifi_data_path_rx_delivered: int = 0
    wifi_data_path_rx_dropped: int = 0
    wifi_data_path_last: str = "none"
    wifi_service_op: int = 0
    wifi_service_reason: int = 0
    wifi_service_progress: int = 0
    wifi_service_seq: int = 0
    wifi_service_credit: int = 0
    wifi_service_channel: int = 0
    wifi_service_rframe: int = 0
    wifi_service_eapol_m1: int = 0
    wifi_service_eapol_m2: int = 0
    wifi_service_eapol_m3: int = 0
    wifi_service_eapol_m4: int = 0
    wifi_rx_irq_preserve_count: int = 0
    wifi_rx_irq_preserve_reason: str = "none"
    wifi_rx_irq_preserve_int: int = 0
    wifi_rx_irq_preserve_ack: int = 0
    wifi_rx_irq_episode_preserves: int = 0
    wifi_rx_irq_episode_rearms: int = 0
    wifi_rxtrace_seq: int = 0
    wifi_rxtrace_start_ticks: int = 0
    wifi_rxtrace_pre_sample_delta_ticks: int = 0
    wifi_rxtrace_transfer_delta_ticks: int = 0
    wifi_rxtrace_post_sample_delta_ticks: int = 0
    wifi_dpc_proof: bool = False
    wifi_dpc_reason: str = "missing"
    wifi_dpc_generation: int = 0
    wifi_dpc_captures: int = 0
    wifi_dpc_published: int = 0
    wifi_dpc_consumed: int = 0
    wifi_dpc_rearms: int = 0
    wifi_dpc_overruns: int = 0
    wifi_dpc_epoch_errors: int = 0
    wifi_dpc_sequence_errors: int = 0
    wifi_dpc_ack_failures: int = 0
    wifi_dpc_poisoned: str = "unknown"
    wifi_dpc_masked: str = "unknown"
    wifi_dpc_line: int = 0
    cyw43_bootstrap_supervisor_seen: bool = False
    cyw43_bootstrap_supervisor_max_attempt: int = 0
    cyw43_bootstrap_supervisor_transient_retries: int = 0
    cyw43_bootstrap_supervisor_last_status: str = "none"
    cyw43_bootstrap_supervisor_ready: bool = False
    cyw43_bootstrap_supervisor_blocker: str = "none"
    wifi_firmware_identity_proof: bool = False
    wifi_firmware_identity_blocker: str = "not-seen"
    wifi_clm_ready_proof: bool = False
    wifi_firmware_version_proof: bool = False
    wifi_clm_version_proof: bool = False
    driver_task_default_requested: bool = False
    driver_task_live_hot_paths: bool = False
    driver_task_contracts: int = 0
    driver_task_dedicated: int = 0
    driver_task_compatibility: int = 0
    driver_task_dedicated_ready: bool = False
    driver_task_serial_dedicated: bool = False
    driver_task_usb_dedicated: bool = False
    driver_task_display_dedicated: bool = False
    driver_task_net_dedicated: bool = False
    driver_task_sdio_dedicated: bool = False
    driver_task_pcie_dedicated: bool = False
    driver_task_substrate_ready: bool = False
    driver_task_failed_count: int = 0
    driver_task_capset_proof: bool = False
    driver_task_fault_proof: bool = False
    driver_task_revoke_proof: bool = False
    driver_task_sched_proof: bool = False
    driver_task_affinity_proof: bool = False
    driver_task_affinity_configured: int = 0
    driver_task_affinity_applied: int = 0
    driver_task_affinity_manifest_proof: bool = False
    driver_task_affinity_manifest_matches: int = 0
    driver_task_affinity_manifest_missing: int = len(
        REQUIRED_PI4_DRIVER_TASK_AFFINITY_CONTRACTS
    )
    driver_task_affinity_manifest_mismatches: int = 0
    driver_task_notification_bind_deferred: bool = False
    driver_task_vspace_proof: bool = False
    driver_task_pointer_free_ipc_proof: bool = False
    driver_task_owner_state_proof: bool = False
    driver_task_runtime_descriptor_seal_proof: bool = False
    driver_task_runtime_descriptor_seal_proofs: int = 0
    driver_task_runtime_descriptor_seal_blocker: str = "none"
    driver_task_active_net: str = "unknown"
    driver_task_budget_overruns: int = 0
    driver_task_latency_proofs: int = 0
    driver_task_ring_call_begin: int = 0
    driver_task_ring_call_return: int = 0
    driver_task_ring_call_outstanding: int = 0
    driver_task_ring_call_timeout: int = 0
    driver_task_ring_call_unresolved_timeout: int = 0
    driver_task_ring_call_keep_active: int = 0
    driver_task_ring_call_abort: int = 0
    driver_task_bootstrap_deferred: int = 0
    driver_task_resource_init: int = 0
    driver_task_resource_blocker: str = "none"
    driver_task_resource_current_blocker: str = "none"
    driver_task_counter_snapshots: int = 0
    driver_task_counter_invalid: int = 0
    driver_task_counter_busy: int = 0
    driver_task_counter_same_request: int = 0
    driver_task_counter_timeouts: int = 0
    driver_task_counter_keep_active: int = 0
    driver_task_counter_aborts: int = 0
    driver_task_counter_overruns: int = 0
    driver_task_counter_drops: int = 0
    driver_task_counter_staged_bytes: int = 0
    driver_task_counter_cache_ops: int = 0
    driver_task_counter_cache_bytes: int = 0
    driver_task_counter_rx_frames: int = 0
    driver_task_counter_tx_frames: int = 0
    driver_task_counter_rx_bytes: int = 0
    driver_task_counter_tx_bytes: int = 0
    driver_task_dma_proofs: int = 0
    driver_task_dma_blocker: str = "none"
    pi4_runtime_dma_proof: str = "absent"
    pi4_runtime_dma_proof_reason: str = "no-driver-task-runtime-proof"
    pi4_runtime_dma_counter_proof: str = "absent"
    serial_output_tx_pending: str = "unknown"
    serial_output_interactive: str = "unknown"
    serial_output_deferred: int = 0
    serial_output_flushed: int = 0
    serial_output_backpressure: int = 0
    hdmi_display_pending_bytes: int = 0
    hdmi_display_pending_redraw: str = "unknown"
    hdmi_display_submitted: int = 0
    hdmi_display_deferred: int = 0
    hdmi_display_busy: int = 0
    hdmi_display_no_reply: int = 0
    hdmi_display_coalesced: int = 0
    hdmi_display_backpressure_bytes: int = 0
    hdmi_display_superseded_bytes: int = 0
    usb_keyboard_no_replies: int = 0
    usb_keyboard_poll_cooldown: int = 0
    usb_keyboard_cooldown_skips: int = 0
    usb_runtime_queued_reports: int = 0
    usb_runtime_transfer_events: int = 0
    usb_runtime_report_status: str = "unknown"
    usb_runtime_queue_valid: str = "unknown"
    usb_runtime_doorbell_pending: str = "unknown"
    usb_runtime_recovery_diag_valid: str = "unknown"
    usb_runtime_endpoint_recoveries: int = 0
    usb_runtime_endpoint_recovery_failures: int = 0
    usb_runtime_queue_collapse_recoveries: int = 0
    usb_runtime_recovery_stage: str = "unknown"
    usb_runtime_recovery_reason: str = "unknown"
    usb_runtime_command_completion_blocked: int = 0
    usb_event_loop_runtime_skipped: int = 0
    usb_post_first_byte_blocker: str = "none"
    usb_startup_blocker_seen: bool = False
    usb_active_blocker_seen: bool = False
    usb_recovered_from_blocker: bool = False
    usb_recovery_state: str = "unknown"
    usb_local_seat_state: str = "unknown"
    usb_local_seat_reason: str = "unknown"
    usb_command_ready: bool = False
    usb_first_report_ready: bool = False
    usb_first_byte_ready: bool = False
    usb_busy_after_ready: bool = False
    serial_driver_accepted: bool = False
    serial_fallback_active: bool = False
    serial_runtime_frontier: str = "none"
    hdmi_descriptor_ready: bool = False
    hdmi_engine_ready: bool = False
    hdmi_owner_state_ready: bool = False
    hdmi_runtime_frontier: str = "none"
    usb_driver_task_frontier: str = "none"
    wifi_replay_frontier: str = "none"
    net_driver_task_replay_events: int = 0
    net_driver_task_replay_blocker: str = "none"
    sdio_driver_task_replay_events: int = 0
    sdio_driver_task_replay_blocker: str = "none"
    serial_responsive_proof: bool = False
    usb_burst_proof: bool = False
    usb_burst_drops: int = -1
    hdmi_responsive_proof: bool = False

    def to_record(self) -> dict[str, object]:
        """Return a JSON-serializable gate summary."""

        return {
            "USB_GATE": self.usb_gate,
            "USB_BLOCKER": self.usb_blocker,
            "WIFI_GATE": self.wifi_gate,
            "WIFI_BLOCKER": self.wifi_blocker,
            "WIFI_SUBGATE": self.wifi_subgate,
            "WIFI_SUBGATE_NAME": self.wifi_subgate_name,
            "WIFI_SUBGATE_SOURCE": self.wifi_subgate_source,
            "WIFI_SUBGATE_STATUS": self.wifi_subgate_status,
            "WIFI_SUBGATE_REASON": self.wifi_subgate_reason,
            "WIFI_SUBGATE_LINE": self.wifi_subgate_line,
            "WIFI_GATE7_COMPLETE": "yes" if self.wifi_gate7_complete else "no",
            "WIFI_GATE7_SEEN": self.wifi_gate7_seen,
            "WIFI_GATE7_LAST": self.wifi_gate7_last,
            "WIFI_GATE7_MISSING": self.wifi_gate7_missing,
            "USB_OLDGOOD_REPLAY": "yes" if self.usb_oldgood_replay else "no",
            "USB_OLDGOOD_LAST": self.usb_oldgood_last,
            "USB_OLDGOOD_MISSING": self.usb_oldgood_missing,
            "WIFI_OLDGOOD_REPLAY": "yes" if self.wifi_oldgood_replay else "no",
            "WIFI_OLDGOOD_LAST": self.wifi_oldgood_last,
            "WIFI_OLDGOOD_MISSING": self.wifi_oldgood_missing,
            "WIFI_EXACT": self.wifi_exact,
            "WIFI_PHASE": self.wifi_phase,
            "WIFI_BLOCKER_LINE": self.wifi_blocker_line,
            "SERIAL_CLEAN": "yes" if self.serial_clean else "no",
            "BOOT_HALTED": "yes" if self.boot_halted else "no",
            "TIMER_IRQ27_SEEN": "yes" if self.timer_irq27_seen else "no",
            "TIMER_BACKEND": self.timer_backend,
            "TIMER_CLOCK_HZ": self.timer_clock_hz,
            "TIMER_EL0_COUNTER": self.timer_el0_counter,
            "DUMMY_TIMER_SEEN": "yes" if self.dummy_timer_seen else "no",
            "SDIO_IRQ158_SEEN": "yes" if self.sdio_irq158_seen else "no",
            "SDIO_IRQ158_BOUND": "yes" if self.sdio_irq158_bound else "no",
            "SDIO_IRQ158_INBAND_PROOF": (
                "yes" if self.sdio_irq158_inband_proof else "no"
            ),
            "SDIO_IRQ158_LINE": self.sdio_irq158_line,
            "BOOT_HALT_REASON": self.boot_halt_reason,
            "PANIC_SEEN": "yes" if self.panic_seen else "no",
            "PANIC_REASON": self.panic_reason,
            "USB_BOOTLOADER_HANDOFF_SEEN": (
                "yes" if self.usb_bootloader_handoff_seen else "no"
            ),
            "USB_COLD_BOOT_SEEN": "yes" if self.usb_cold_boot_seen else "no",
            "USB_STALE_UEFI_HINT_SEEN": (
                "yes" if self.usb_stale_uefi_hint_seen else "no"
            ),
            "USB_EVENT_RING_ALIVE": "yes" if self.usb_event_ring_alive else "no",
            "USB_PSC_DRAIN_COUNT": self.usb_psc_drain_count,
            "USB_PSC_DRAIN_MASK": f"0x{self.usb_psc_drain_mask:08x}",
            "ROOT_CONSOLE_READY": "yes" if self.root_console_ready else "no",
            "ROOT_PROMPT_SEEN": "yes" if self.root_prompt_seen else "no",
            "NET_ACTIVE": self.net_active,
            "NET_ADDR_SRC": self.net_addr_src,
            "NET_DHCP": self.net_dhcp,
            "NET_TCP_READY": "yes" if self.net_tcp_ready else "no",
            "NETTEST_PROOF": "yes" if self.nettest_proof else "no",
            "COHSH_TCP_AUTH_PROOF": "yes" if self.cohsh_tcp_auth_proof else "no",
            "TCP_ACCEPTS": self.tcp_accepts,
            "TCP_AUTH_SESSIONS": self.tcp_auth_sessions,
            "TCP_RX_BYTES": self.tcp_rx_bytes,
            "WIFI_DATA_PATH_TX": self.wifi_data_path_tx,
            "WIFI_DATA_PATH_RX_PRESERVED": self.wifi_data_path_rx_preserved,
            "WIFI_DATA_PATH_RX_DELIVERED": self.wifi_data_path_rx_delivered,
            "WIFI_DATA_PATH_RX_DROPPED": self.wifi_data_path_rx_dropped,
            "WIFI_DATA_PATH_LAST": self.wifi_data_path_last,
            "WIFI_SERVICE_OP": self.wifi_service_op,
            "WIFI_SERVICE_REASON": self.wifi_service_reason,
            "WIFI_SERVICE_PROGRESS": f"0x{self.wifi_service_progress:08x}",
            "WIFI_SERVICE_SEQ": self.wifi_service_seq,
            "WIFI_SERVICE_CREDIT": self.wifi_service_credit,
            "WIFI_SERVICE_CHANNEL": self.wifi_service_channel,
            "WIFI_SERVICE_RFRAME": self.wifi_service_rframe,
            "WIFI_SERVICE_EAPOL_M1": self.wifi_service_eapol_m1,
            "WIFI_SERVICE_EAPOL_M2": self.wifi_service_eapol_m2,
            "WIFI_SERVICE_EAPOL_M3": self.wifi_service_eapol_m3,
            "WIFI_SERVICE_EAPOL_M4": self.wifi_service_eapol_m4,
            "WIFI_RX_IRQ_PRESERVE_COUNT": self.wifi_rx_irq_preserve_count,
            "WIFI_RX_IRQ_PRESERVE_REASON": self.wifi_rx_irq_preserve_reason,
            "WIFI_RX_IRQ_PRESERVE_INT": f"0x{self.wifi_rx_irq_preserve_int:08x}",
            "WIFI_RX_IRQ_PRESERVE_ACK": f"0x{self.wifi_rx_irq_preserve_ack:08x}",
            "WIFI_RX_IRQ_EPISODE_PRESERVES": self.wifi_rx_irq_episode_preserves,
            "WIFI_RX_IRQ_EPISODE_REARMS": self.wifi_rx_irq_episode_rearms,
            "WIFI_RXTRACE_SEQ": self.wifi_rxtrace_seq,
            "WIFI_RXTRACE_START_TICKS": f"0x{self.wifi_rxtrace_start_ticks:08x}",
            "WIFI_RXTRACE_PRE_SAMPLE_DELTA_TICKS": self.wifi_rxtrace_pre_sample_delta_ticks,
            "WIFI_RXTRACE_TRANSFER_DELTA_TICKS": self.wifi_rxtrace_transfer_delta_ticks,
            "WIFI_RXTRACE_POST_SAMPLE_DELTA_TICKS": self.wifi_rxtrace_post_sample_delta_ticks,
            "WIFI_DPC_PROOF": "yes" if self.wifi_dpc_proof else "no",
            "WIFI_DPC_REASON": self.wifi_dpc_reason,
            "WIFI_DPC_GENERATION": self.wifi_dpc_generation,
            "WIFI_DPC_CAPTURES": self.wifi_dpc_captures,
            "WIFI_DPC_PUBLISHED": self.wifi_dpc_published,
            "WIFI_DPC_CONSUMED": self.wifi_dpc_consumed,
            "WIFI_DPC_REARMS": self.wifi_dpc_rearms,
            "WIFI_DPC_OVERRUNS": self.wifi_dpc_overruns,
            "WIFI_DPC_EPOCH_ERRORS": self.wifi_dpc_epoch_errors,
            "WIFI_DPC_SEQUENCE_ERRORS": self.wifi_dpc_sequence_errors,
            "WIFI_DPC_ACK_FAILURES": self.wifi_dpc_ack_failures,
            "WIFI_DPC_POISONED": self.wifi_dpc_poisoned,
            "WIFI_DPC_MASKED": self.wifi_dpc_masked,
            "WIFI_DPC_LINE": self.wifi_dpc_line,
            "CYW43_BOOTSTRAP_SUPERVISOR_SEEN": (
                "yes" if self.cyw43_bootstrap_supervisor_seen else "no"
            ),
            "CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT": (
                self.cyw43_bootstrap_supervisor_max_attempt
            ),
            "CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES": (
                self.cyw43_bootstrap_supervisor_transient_retries
            ),
            "CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS": (
                self.cyw43_bootstrap_supervisor_last_status
            ),
            "CYW43_BOOTSTRAP_SUPERVISOR_READY": (
                "yes" if self.cyw43_bootstrap_supervisor_ready else "no"
            ),
            "CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER": (
                self.cyw43_bootstrap_supervisor_blocker
            ),
            "WIFI_FIRMWARE_IDENTITY_PROOF": (
                "yes" if self.wifi_firmware_identity_proof else "no"
            ),
            "WIFI_FIRMWARE_IDENTITY_BLOCKER": self.wifi_firmware_identity_blocker,
            "WIFI_CLM_READY_PROOF": "yes" if self.wifi_clm_ready_proof else "no",
            "WIFI_FIRMWARE_VERSION_PROOF": (
                "yes" if self.wifi_firmware_version_proof else "no"
            ),
            "WIFI_CLM_VERSION_PROOF": (
                "yes" if self.wifi_clm_version_proof else "no"
            ),
            "DRIVER_TASK_DEFAULT_REQUESTED": (
                "yes" if self.driver_task_default_requested else "no"
            ),
            "DRIVER_TASK_LIVE_HOT_PATHS": (
                "yes" if self.driver_task_live_hot_paths else "no"
            ),
            "DRIVER_TASK_CONTRACTS": self.driver_task_contracts,
            "DRIVER_TASK_DEDICATED": self.driver_task_dedicated,
            "DRIVER_TASK_COMPATIBILITY": self.driver_task_compatibility,
            "DRIVER_TASK_DEDICATED_READY": (
                "yes" if self.driver_task_dedicated_ready else "no"
            ),
            "DRIVER_TASK_SERIAL_DEDICATED": (
                "yes" if self.driver_task_serial_dedicated else "no"
            ),
            "DRIVER_TASK_USB_DEDICATED": (
                "yes" if self.driver_task_usb_dedicated else "no"
            ),
            "DRIVER_TASK_DISPLAY_DEDICATED": (
                "yes" if self.driver_task_display_dedicated else "no"
            ),
            "DRIVER_TASK_NET_DEDICATED": (
                "yes" if self.driver_task_net_dedicated else "no"
            ),
            "DRIVER_TASK_SDIO_DEDICATED": (
                "yes" if self.driver_task_sdio_dedicated else "no"
            ),
            "DRIVER_TASK_PCIE_DEDICATED": (
                "yes" if self.driver_task_pcie_dedicated else "no"
            ),
            "DRIVER_TASK_SUBSTRATE_READY": (
                "yes" if self.driver_task_substrate_ready else "no"
            ),
            "DRIVER_TASK_FAILED_COUNT": self.driver_task_failed_count,
            "DRIVER_TASK_CAPSET_PROOF": (
                "yes" if self.driver_task_capset_proof else "no"
            ),
            "DRIVER_TASK_FAULT_PROOF": (
                "yes" if self.driver_task_fault_proof else "no"
            ),
            "DRIVER_TASK_REVOKE_PROOF": (
                "yes" if self.driver_task_revoke_proof else "no"
            ),
            "DRIVER_TASK_SCHED_PROOF": (
                "yes" if self.driver_task_sched_proof else "no"
            ),
            "DRIVER_TASK_AFFINITY_PROOF": (
                "yes" if self.driver_task_affinity_proof else "no"
            ),
            "DRIVER_TASK_AFFINITY_CONFIGURED": self.driver_task_affinity_configured,
            "DRIVER_TASK_AFFINITY_APPLIED": self.driver_task_affinity_applied,
            "DRIVER_TASK_AFFINITY_MANIFEST_PROOF": (
                "yes" if self.driver_task_affinity_manifest_proof else "no"
            ),
            "DRIVER_TASK_AFFINITY_MANIFEST_MATCHES": (
                self.driver_task_affinity_manifest_matches
            ),
            "DRIVER_TASK_AFFINITY_MANIFEST_MISSING": (
                self.driver_task_affinity_manifest_missing
            ),
            "DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES": (
                self.driver_task_affinity_manifest_mismatches
            ),
            "DRIVER_TASK_NOTIFICATION_BIND_DEFERRED": (
                "yes" if self.driver_task_notification_bind_deferred else "no"
            ),
            "DRIVER_TASK_VSPACE_PROOF": (
                "yes" if self.driver_task_vspace_proof else "no"
            ),
            "DRIVER_TASK_POINTER_FREE_IPC_PROOF": (
                "yes" if self.driver_task_pointer_free_ipc_proof else "no"
            ),
            "DRIVER_TASK_OWNER_STATE_PROOF": (
                "yes" if self.driver_task_owner_state_proof else "no"
            ),
            "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF": (
                "yes" if self.driver_task_runtime_descriptor_seal_proof else "no"
            ),
            "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOFS": (
                self.driver_task_runtime_descriptor_seal_proofs
            ),
            "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER": (
                self.driver_task_runtime_descriptor_seal_blocker
            ),
            "DRIVER_TASK_ACTIVE_NET": self.driver_task_active_net,
            "DRIVER_TASK_BUDGET_OVERRUNS": self.driver_task_budget_overruns,
            "DRIVER_TASK_LATENCY_PROOFS": self.driver_task_latency_proofs,
            "DRIVER_TASK_RING_CALL_BEGIN": self.driver_task_ring_call_begin,
            "DRIVER_TASK_RING_CALL_RETURN": self.driver_task_ring_call_return,
            "DRIVER_TASK_RING_CALL_OUTSTANDING": self.driver_task_ring_call_outstanding,
            "DRIVER_TASK_RING_CALL_TIMEOUT": self.driver_task_ring_call_timeout,
            "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT": (
                self.driver_task_ring_call_unresolved_timeout
            ),
            "DRIVER_TASK_RING_CALL_KEEP_ACTIVE": self.driver_task_ring_call_keep_active,
            "DRIVER_TASK_RING_CALL_ABORT": self.driver_task_ring_call_abort,
            "DRIVER_TASK_BOOTSTRAP_DEFERRED": self.driver_task_bootstrap_deferred,
            "DRIVER_TASK_RESOURCE_INIT": self.driver_task_resource_init,
            "DRIVER_TASK_RESOURCE_BLOCKER": self.driver_task_resource_blocker,
            "DRIVER_TASK_RESOURCE_CURRENT_BLOCKER": (
                self.driver_task_resource_current_blocker
            ),
            "DRIVER_TASK_COUNTER_SNAPSHOTS": self.driver_task_counter_snapshots,
            "DRIVER_TASK_COUNTER_INVALID": self.driver_task_counter_invalid,
            "DRIVER_TASK_COUNTER_BUSY": self.driver_task_counter_busy,
            "DRIVER_TASK_COUNTER_SAME_REQUEST": self.driver_task_counter_same_request,
            "DRIVER_TASK_COUNTER_TIMEOUTS": self.driver_task_counter_timeouts,
            "DRIVER_TASK_COUNTER_KEEP_ACTIVE": self.driver_task_counter_keep_active,
            "DRIVER_TASK_COUNTER_ABORTS": self.driver_task_counter_aborts,
            "DRIVER_TASK_COUNTER_OVERRUNS": self.driver_task_counter_overruns,
            "DRIVER_TASK_COUNTER_DROPS": self.driver_task_counter_drops,
            "DRIVER_TASK_COUNTER_STAGED_BYTES": self.driver_task_counter_staged_bytes,
            "DRIVER_TASK_COUNTER_CACHE_OPS": self.driver_task_counter_cache_ops,
            "DRIVER_TASK_COUNTER_CACHE_BYTES": self.driver_task_counter_cache_bytes,
            "DRIVER_TASK_COUNTER_RX_FRAMES": self.driver_task_counter_rx_frames,
            "DRIVER_TASK_COUNTER_TX_FRAMES": self.driver_task_counter_tx_frames,
            "DRIVER_TASK_COUNTER_RX_BYTES": self.driver_task_counter_rx_bytes,
            "DRIVER_TASK_COUNTER_TX_BYTES": self.driver_task_counter_tx_bytes,
            "DRIVER_TASK_DMA_PROOFS": self.driver_task_dma_proofs,
            "DRIVER_TASK_DMA_BLOCKER": self.driver_task_dma_blocker,
            "PI4_RUNTIME_DMA_PROOF": self.pi4_runtime_dma_proof,
            "PI4_RUNTIME_DMA_PROOF_REASON": self.pi4_runtime_dma_proof_reason,
            "PI4_RUNTIME_DMA_COUNTER_PROOF": self.pi4_runtime_dma_counter_proof,
            "SERIAL_OUTPUT_TX_PENDING": self.serial_output_tx_pending,
            "SERIAL_OUTPUT_INTERACTIVE": self.serial_output_interactive,
            "SERIAL_OUTPUT_DEFERRED": self.serial_output_deferred,
            "SERIAL_OUTPUT_FLUSHED": self.serial_output_flushed,
            "SERIAL_OUTPUT_BACKPRESSURE": self.serial_output_backpressure,
            "HDMI_DISPLAY_PENDING_BYTES": self.hdmi_display_pending_bytes,
            "HDMI_DISPLAY_PENDING_REDRAW": self.hdmi_display_pending_redraw,
            "HDMI_DISPLAY_SUBMITTED": self.hdmi_display_submitted,
            "HDMI_DISPLAY_DEFERRED": self.hdmi_display_deferred,
            "HDMI_DISPLAY_BUSY": self.hdmi_display_busy,
            "HDMI_DISPLAY_NO_REPLY": self.hdmi_display_no_reply,
            "HDMI_DISPLAY_COALESCED": self.hdmi_display_coalesced,
            "HDMI_DISPLAY_BACKPRESSURE_BYTES": (
                self.hdmi_display_backpressure_bytes
            ),
            "HDMI_DISPLAY_SUPERSEDED_BYTES": self.hdmi_display_superseded_bytes,
            "USB_KEYBOARD_NO_REPLIES": self.usb_keyboard_no_replies,
            "USB_KEYBOARD_POLL_COOLDOWN": self.usb_keyboard_poll_cooldown,
            "USB_KEYBOARD_COOLDOWN_SKIPS": self.usb_keyboard_cooldown_skips,
            "USB_RUNTIME_QUEUED_REPORTS": self.usb_runtime_queued_reports,
            "USB_RUNTIME_TRANSFER_EVENTS": self.usb_runtime_transfer_events,
            "USB_RUNTIME_REPORT_STATUS": self.usb_runtime_report_status,
            "USB_RUNTIME_QUEUE_VALID": self.usb_runtime_queue_valid,
            "USB_RUNTIME_DOORBELL_PENDING": self.usb_runtime_doorbell_pending,
            "USB_RUNTIME_RECOVERY_DIAG_VALID": self.usb_runtime_recovery_diag_valid,
            "USB_RUNTIME_ENDPOINT_RECOVERIES": self.usb_runtime_endpoint_recoveries,
            "USB_RUNTIME_ENDPOINT_RECOVERY_FAILURES": (
                self.usb_runtime_endpoint_recovery_failures
            ),
            "USB_RUNTIME_QUEUE_COLLAPSE_RECOVERIES": (
                self.usb_runtime_queue_collapse_recoveries
            ),
            "USB_RUNTIME_RECOVERY_STAGE": self.usb_runtime_recovery_stage,
            "USB_RUNTIME_RECOVERY_REASON": self.usb_runtime_recovery_reason,
            "USB_RUNTIME_COMMAND_COMPLETION_BLOCKED": (
                self.usb_runtime_command_completion_blocked
            ),
            "USB_EVENT_LOOP_RUNTIME_SKIPPED": self.usb_event_loop_runtime_skipped,
            "USB_POST_FIRST_BYTE_BLOCKER": self.usb_post_first_byte_blocker,
            "USB_STARTUP_BLOCKER_SEEN": (
                "yes" if self.usb_startup_blocker_seen else "no"
            ),
            "USB_ACTIVE_BLOCKER_SEEN": (
                "yes" if self.usb_active_blocker_seen else "no"
            ),
            "USB_RECOVERED_FROM_BLOCKER": (
                "yes" if self.usb_recovered_from_blocker else "no"
            ),
            "USB_RECOVERY_STATE": self.usb_recovery_state,
            "USB_LOCAL_SEAT_STATE": self.usb_local_seat_state,
            "USB_LOCAL_SEAT_REASON": self.usb_local_seat_reason,
            "USB_COMMAND_READY": "yes" if self.usb_command_ready else "no",
            "USB_FIRST_REPORT_READY": (
                "yes" if self.usb_first_report_ready else "no"
            ),
            "USB_FIRST_BYTE_READY": "yes" if self.usb_first_byte_ready else "no",
            "USB_BUSY_AFTER_READY": "yes" if self.usb_busy_after_ready else "no",
            "SERIAL_DRIVER_ACCEPTED": (
                "yes" if self.serial_driver_accepted else "no"
            ),
            "SERIAL_FALLBACK_ACTIVE": (
                "yes" if self.serial_fallback_active else "no"
            ),
            "SERIAL_RUNTIME_FRONTIER": self.serial_runtime_frontier,
            "HDMI_DESCRIPTOR_READY": "yes" if self.hdmi_descriptor_ready else "no",
            "HDMI_ENGINE_READY": "yes" if self.hdmi_engine_ready else "no",
            "HDMI_OWNER_STATE_READY": (
                "yes" if self.hdmi_owner_state_ready else "no"
            ),
            "HDMI_RUNTIME_FRONTIER": self.hdmi_runtime_frontier,
            "USB_DRIVER_TASK_FRONTIER": self.usb_driver_task_frontier,
            "WIFI_REPLAY_FRONTIER": self.wifi_replay_frontier,
            "NET_DRIVER_TASK_REPLAY_EVENTS": self.net_driver_task_replay_events,
            "NET_DRIVER_TASK_REPLAY_BLOCKER": self.net_driver_task_replay_blocker,
            "SDIO_DRIVER_TASK_REPLAY_EVENTS": self.sdio_driver_task_replay_events,
            "SDIO_DRIVER_TASK_REPLAY_BLOCKER": self.sdio_driver_task_replay_blocker,
            "SERIAL_RESPONSIVE_PROOF": (
                "yes" if self.serial_responsive_proof else "no"
            ),
            "USB_BURST_PROOF": "yes" if self.usb_burst_proof else "no",
            "USB_BURST_DROPS": self.usb_burst_drops,
            "HDMI_RESPONSIVE_PROOF": "yes" if self.hdmi_responsive_proof else "no",
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


def normalize_panic_reason(reason: str) -> str:
    """Return a stable, compact panic reason token."""

    normalized = PANIC_REASON_TOKEN_RE.sub("-", reason.strip().lower()).strip("-")
    return normalized or "root-task-panic"


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


def prompt_trace_interleaving_reason(line: str) -> str | None:
    """Return a serial cleanliness reason for prompt/log glued evidence."""

    clean = ANSI_RE.sub("", line).replace("\r", "").strip()
    if not clean.startswith("cohesix> "):
        return None
    payload = clean.removeprefix("cohesix> ").strip()
    if not payload:
        return None
    match = TRACE_SEGMENT_RE.match(payload)
    if match is None or match.start() != 0:
        return None
    return "prompt-prefixed-trace"


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


def startup_diag_gate(raw: str, domain: str) -> int | None:
    """Return the gate number from a startup black-box diagnostic line."""

    match = STARTUP_DIAG_GATE_RE.match(raw)
    if match is None or match.group("domain").lower() != domain:
        return None
    try:
        return int(match.group("gate"))
    except ValueError:
        return None


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


def root_console_ready_evidence(event: TraceEvent) -> bool:
    """Return true when serial root-console readiness reached userland."""

    if event.domain != "console":
        return False
    lowered_raw = event.raw.lower()
    return (
        "cohesix console ready" in lowered_raw
        or "root console ready" in lowered_raw
        or "root console banner emitted" in lowered_raw
    )


def root_prompt_evidence(event: TraceEvent) -> bool:
    """Return true when the interactive root prompt reached serial."""

    return event.domain == "console" and (
        event.fields.get("prompt") == "yes" or event.raw.startswith("cohesix>")
    )


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
    if line.startswith("[cohesix]") and uboot_wifi_policy_missing_line(line):
        return "uboot"
    if "brcmfmac" in lower or "brcmf_" in lower:
        return "linux"
    if domain == "usb" and line.startswith("[cohesix]"):
        return "uboot"
    return "cohesix"


def uboot_wifi_policy_missing_line(line: str) -> bool:
    """Return true for U-Boot menu recovery text for missing Wi-Fi policy."""

    lower = line.lower()
    return any(marker in lower for marker in UBOOT_WIFI_POLICY_MISSING_MARKERS)


def classify_domain(line: str) -> str | None:
    """Classify USB/WiFi trace domain, or return None for unrelated lines."""

    prompt = line.lstrip()
    if prompt.startswith("cohesix>"):
        prompt_payload = prompt.removeprefix("cohesix>").strip()
        if prompt_payload:
            prompt_domain = classify_domain(prompt_payload)
            if prompt_domain not in {None, "console"}:
                return prompt_domain
        return "console"

    lower = line.lower()
    if line.startswith("[cohesix]") and uboot_wifi_policy_missing_line(line):
        return "wifi"
    if (
        line.startswith("BOOTINFO_SNAPSHOT_CORRUPTED")
        or line.startswith("[HARD_GUARD]")
        or "[panic]" in lower
        or "[timers]" in lower
        or lower.startswith("[bootstrap:fatal]")
        or lower.startswith("[cohesix:root-task] panic")
    ):
        return "kernel"
    if (
        line.startswith("cohesix>")
        or "cohesix console ready" in lower
        or "root console ready" in lower
        or "root console banner emitted" in lower
        or "[dbg] console: root console task entry" in lower
    ):
        return "console"
    if "[cohesix:usb-trace]" in lower:
        return "usb"
    if (
        line.startswith("usb:")
        or line.startswith("USB:")
        or line.startswith("USB_RUNTIME_")
        or "[local-seat]" in lower
        or (
            "[pi4-platform]" in lower
            and (
                "mmio preseed" in lower
                or "vl805-usb-hcd-power" in lower
                or "xhci-reset-notify" in lower
                or "owner=vl805-usb-hcd-power" in lower
                or "owner=xhci-reset-notify" in lower
                or (
                    "mailbox power-on" in lower
                    and "module=0x00000003" in lower
                )
            )
        )
    ):
        return "usb"
    if line == "halting...":
        return "kernel"
    if line.startswith("Kernel entry via Interrupt"):
        return "kernel"
    if (
        line.startswith("wifi:")
        or line.startswith("WiFi:")
        or line.startswith("WIFI:")
        or line.startswith("CYW43_")
        or line.startswith("OK NETTEST")
        or line.startswith("ERR NETTEST")
        or (
            "[pi4-wifi]" in lower
            and "vl805-usb-hcd-power" not in lower
            and "xhci-reset-notify" not in lower
            and "owner=vl805-usb-hcd-power" not in lower
            and "owner=xhci-reset-notify" not in lower
            and not (
                "mailbox power-on" in lower
                and "module=0x00000003" in lower
            )
        )
        or "[cyw43]" in lower
        or (
            "[net-console]" in lower
            and (
                "deferred reason=local-seat-usb-first" in lower
                or "action=serial-local-seat-first" in lower
                or "action=serial-root-console-first" in lower
                or "action=root-console-wait-for-wifi" in lower
                or "action=wait-for-wifi" in lower
                or "action=start-wifi" in lower
                or "bringup_status=wifi-" in lower
                or ("device initialized" in lower and "interface=wifi" in lower)
                or "root console wait" in lower
                or "wifi-net-console-deferred-until-root-console" in lower
                or "wifi-net-console-pending-before-root-console" in lower
                or "wifi-not-ready" in lower
                or "deferred failed detail=" in lower
                or "deferred ready backend=cyw43" in lower
            )
        )
    ):
        return "wifi"
    if (
        "DRIVER_TASK" in line
        or "SCHED_CONTRACT" in line
        or "BUDGET_OVERRUN" in line
        or "[driver-task]" in lower
        or "driver-task" in lower
        or "driver task" in lower
    ):
        return "driver"
    if (
        line.startswith("SERIAL_ECHO")
        or line.startswith("SERIAL_INPUT_TRACE")
        or line.startswith("USB_BURST")
        or line.startswith("HDMI_RESPONSIVE")
        or line.startswith("HDMI_FRAME_")
        or line.startswith("[smp] activity local-seat")
        or line.startswith("[smp] activity local-seat-display")
        or "serial echo" in lower
        or "keyboard burst" in lower
        or "hdmi stats" in lower
        or "display stats" in lower
    ):
        return "driver"
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
    if "[pi4-platform]" in lower:
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
        or "[cohsh-net][auth] auth ok" in lower
        or "[net-console] auth ok" in lower
        or (
            "[net-console]" in lower
            and "conn " in lower
            and " authenticated" in lower
        )
        or line.startswith("[smp] activity net")
        or line.startswith("netstatus")
        or line.startswith("netstats")
    ):
        return "wifi"
    if "[net-console]" in lower and (
        "deferred reason=local-seat-usb-first" in lower
        or "action=serial-local-seat-first" in lower
        or "action=serial-root-console-first" in lower
        or "action=root-console-wait-for-wifi" in lower
        or "action=wait-for-wifi" in lower
        or "action=start-wifi" in lower
        or "bringup_status=wifi-" in lower
        or ("device initialized" in lower and "interface=wifi" in lower)
        or "root console wait" in lower
        or "wifi-net-console-deferred-until-root-console" in lower
        or "wifi-net-console-pending-before-root-console" in lower
        or "wifi-not-ready" in lower
        or "deferred failed detail=" in lower
        or "deferred ready backend=cyw43" in lower
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
    for marker in ("[local-seat]", "[pi4-wifi]", "[cyw43]", "[net-console]", "[cohesix]"):
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
    elif line.startswith("[HARD_GUARD]"):
        tag = normalize_panic_reason(fields.get("tag", "unknown"))
        violation = normalize_panic_reason(
            fields.get("v", "unknown").split("{", 1)[0]
        )
        reason = f"hard-guard-{tag}-{violation}"
        fields = {**fields, "halt": "yes", "reason": reason}
        stage = "hard-guard"
        message = f"halt reason={reason}"
    elif line.startswith("BOOTINFO_SNAPSHOT_CORRUPTED"):
        fields = {
            **fields,
            "halt": "yes",
            "panic": "yes",
            "bootinfo_corrupted": "yes",
            "reason": "bootinfo-snapshot-corrupted",
        }
        stage = "bootinfo-snapshot-corrupted"
        message = "panic reason=bootinfo-snapshot-corrupted"
    elif line.startswith("[bootstrap:fatal]"):
        reason = normalize_panic_reason(line.split("]", 1)[-1])
        fields = {
            **fields,
            "halt": "yes",
            "panic": "yes",
            "reason": reason,
        }
        stage = "panic"
        message = f"panic reason={reason}"
    elif "[PANIC]" in line or "panic: panicked at" in line:
        fields = {
            **fields,
            "halt": "yes",
            "panic": "yes",
            "reason": "root-task-panic",
        }
        stage = "panic"
        message = "panic reason=root-task-panic"
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
        raw_clean = ANSI_RE.sub("", line).replace("\r", "").strip()
        if raw_clean.startswith("cohesix>"):
            events.append(
                TraceEvent(
                    line=line_number,
                    domain="console",
                    source="cohesix",
                    message="prompt",
                    raw="cohesix>",
                    fields={"prompt": "yes"},
                    stage="prompt",
                )
            )
            prompt_interleave_reason = prompt_trace_interleaving_reason(line)
            if prompt_interleave_reason is not None:
                events.append(
                    TraceEvent(
                        line=line_number,
                        domain="console",
                        source="cohesix",
                        message=(
                            "serial-interleaving "
                            f"reason={prompt_interleave_reason}"
                        ),
                        raw="cohesix>",
                        fields={"serial_error": prompt_interleave_reason},
                        stage="serial-interleave",
                    )
                )
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
    """Return false when parsed proof evidence includes corruption or a panic."""

    return all(
        "serial_error" not in event.fields
        and event.fields.get("panic") != "yes"
        and event.fields.get("bootinfo_corrupted") != "yes"
        for event in events
    )


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
    stripped = lower.strip()
    if stripped in {"", "none", "ok", "online", "ready", "success"}:
        return "none"
    if stripped == "awaiting-physical-key":
        return "awaiting-physical-key"
    if (
        "boot_blocker" in lower
        and "post-prompt-local-seat-attach" in lower
        and "status=retry-exhausted" in lower
    ):
        return "post-prompt-attach-retry-exhausted"
    if "no-device-coverage" in lower and (
        "xhci" in lower
        or "vl805" in lower
        or "pcie-root-cfg" in lower
        or "pi4-pcie-root-cfg" in lower
    ):
        return "pcie-xhci-device-coverage-missing"
    if "pcie-vl805-config-contract-missing" in lower:
        return "pcie-vl805-config-contract-missing"
    if "pcie-owner-ring-unavailable" in lower:
        return "pcie-owner-ring-unavailable"
    if stripped == "pcie-vl805":
        return "pcie-vl805"
    if (
        "deferred-until-root-console" in lower
        or "driver-task-runtime-unproved" in lower
        or "root-shell-first" in lower
        or "root-prompt-first" in lower
        or "root-prompt-delayed" in lower
        or "serial-shell-first" in lower
    ):
        return "driver-task-runtime-deferred"
    if "enumeration-disabled-bootloader-owned" in lower:
        return "enumeration-disabled-bootloader-owned"
    if "usb-engine-init-mark-no-reply" in lower:
        return "usb-engine-init-mark-no-reply"
    if "usb-runtime-init-entry-no-reply" in lower:
        return "usb-runtime-init-entry-no-reply"
    if "usb-runtime-state-access-no-reply" in lower:
        return "usb-runtime-state-access-no-reply"
    if "usb-engine-init-state-reset-no-reply" in lower:
        return "usb-engine-init-state-reset-no-reply"
    if "usb-engine-init-hardware-entry-no-reply" in lower:
        return "usb-engine-init-hardware-entry-no-reply"
    if "usb-xhci-capability-read-no-reply" in lower:
        return "usb-xhci-capability-read-no-reply"
    if "usb-xhci-capability-invalid" in lower:
        return "usb-xhci-capability-invalid"
    if "usb-pcie-posted-write-flush-no-reply" in lower:
        return "usb-pcie-posted-write-flush-no-reply"
    if "usb-pcie-posted-write-flush-failed" in lower:
        return "usb-pcie-posted-write-flush-failed"
    if "usb-pcie-posted-write-flush-next-edge-no-reply" in lower:
        return "usb-pcie-posted-write-flush-next-edge-no-reply"
    if "usb-xhci-mmio-entry-no-reply" in lower:
        return "usb-xhci-mmio-entry-no-reply"
    if "usb-engine-init-hardware-no-reply" in lower:
        return "usb-engine-init-hardware-no-reply"
    if "cmd-controller-not-running" in lower:
        return "cmd-controller-not-running"
    if "cmd-controller-not-ready" in lower:
        return "cmd-controller-not-ready"
    if "reset-hcrst-timeout" in lower or "cmd-recovery-hcrst-timeout" in lower:
        return "reset-hcrst-timeout"
    if (
        "reset-controller-not-halted" in lower
        or "halt-revalidation-timeout" in lower
        or "cmd-recovery-stop-timeout" in lower
        or "cmd-recovery-controller-not-halted" in lower
    ):
        return "reset-controller-not-halted"
    if "stop-revalidation-timeout" in lower:
        return "reset-controller-not-halted"
    if (
        "reset-pre-hcrst-controller-not-ready" in lower
        or "pre-hcrst-controller-not-ready" in lower
        or "reset-pre-hcrst-cnr-timeout" in lower
    ):
        return "reset-pre-hcrst-controller-not-ready"
    if "reset-controller-not-ready" in lower or "reset-cnr-timeout" in lower:
        return "reset-controller-not-ready"
    if "cmd-recovery-cnr-timeout" in lower:
        return "reset-controller-not-ready"
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
        return "cmd-event-ring-timeout"
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
    if "pcie-window-enable-slot-timeout" in lower:
        return "pcie-window-enable-slot-timeout"
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
        return "root-port-read-timer-preempted"
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
    if "address-device-failed" in lower:
        return "address-device-failed"
    if "enable-slot-failed" in lower:
        return "enable-slot-failed"
    if "root-port-deferred-capture" in lower or "captured-root-port-enum" in lower:
        return "captured-root-port-enum"
    if "address-device-pending" in lower:
        return "address-device-pending"
    if "no-connected-ports" in lower:
        return "no-connected-ports"
    if "address-failed" in lower:
        return "address-failed"
    if "device-descriptor-submit-no-reply" in lower:
        return "device-descriptor-submit-no-reply"
    if "device-descriptor-transfer-no-reply" in lower:
        return "device-descriptor-transfer-no-reply"
    if "device-descriptor-status-no-reply" in lower:
        return "device-descriptor-status-no-reply"
    if "device-descriptor-transfer-failed" in lower:
        return "device-descriptor-transfer-failed"
    if "device-descriptor-transfer-timeout" in lower:
        return "device-descriptor-transfer-timeout"
    if "device-descriptor-status-timeout" in lower:
        return "device-descriptor-status-timeout"
    for token in (
        "device-descriptor-transfer-event-slot-empty",
        "device-descriptor-transfer-event-cycle-mismatch",
        "device-descriptor-transfer-event-ignored",
        "device-descriptor-status-event-slot-empty",
        "device-descriptor-status-event-cycle-mismatch",
        "device-descriptor-status-event-ignored",
    ):
        if token in lower:
            return token
    if "device-descriptor-prime-submit-no-reply" in lower:
        return "device-descriptor-prime-submit-no-reply"
    if "device-descriptor-prime-transfer-no-reply" in lower:
        return "device-descriptor-prime-transfer-no-reply"
    if "device-descriptor-prime-status-no-reply" in lower:
        return "device-descriptor-prime-status-no-reply"
    if "device-descriptor-full-read-no-reply" in lower:
        return "device-descriptor-full-read-no-reply"
    if "device-descriptor-prime-transfer-failed" in lower:
        return "device-descriptor-prime-transfer-failed"
    if "device-descriptor-prime-transfer-timeout" in lower:
        return "device-descriptor-prime-transfer-timeout"
    if "device-descriptor-prime-status-timeout" in lower:
        return "device-descriptor-prime-status-timeout"
    for token in (
        "device-descriptor-prime-transfer-event-slot-empty",
        "device-descriptor-prime-transfer-event-cycle-mismatch",
        "device-descriptor-prime-transfer-event-ignored",
        "device-descriptor-prime-status-event-slot-empty",
        "device-descriptor-prime-status-event-cycle-mismatch",
        "device-descriptor-prime-status-event-ignored",
    ):
        if token in lower:
            return token
    if "config-descriptor-no-reply" in lower:
        return "config-descriptor-no-reply"
    if "config-descriptor-header-submit-no-reply" in lower:
        return "config-descriptor-header-submit-no-reply"
    if "config-descriptor-header-transfer-no-reply" in lower:
        return "config-descriptor-header-transfer-no-reply"
    if "config-descriptor-header-status-no-reply" in lower:
        return "config-descriptor-header-status-no-reply"
    if "config-descriptor-full-read-no-reply" in lower:
        return "config-descriptor-full-read-no-reply"
    if "config-descriptor-header-transfer-failed" in lower:
        return "config-descriptor-header-transfer-failed"
    if "config-descriptor-header-transfer-timeout" in lower:
        return "config-descriptor-header-transfer-timeout"
    if "config-descriptor-header-status-timeout" in lower:
        return "config-descriptor-header-status-timeout"
    for token in (
        "config-descriptor-header-transfer-event-slot-empty",
        "config-descriptor-header-transfer-event-cycle-mismatch",
        "config-descriptor-header-transfer-event-ignored",
        "config-descriptor-header-status-event-slot-empty",
        "config-descriptor-header-status-event-cycle-mismatch",
        "config-descriptor-header-status-event-ignored",
    ):
        if token in lower:
            return token
    if "config-descriptor-full-submit-no-reply" in lower:
        return "config-descriptor-full-submit-no-reply"
    if "config-descriptor-full-transfer-no-reply" in lower:
        return "config-descriptor-full-transfer-no-reply"
    if "config-descriptor-full-status-no-reply" in lower:
        return "config-descriptor-full-status-no-reply"
    if "config-descriptor-full-transfer-failed" in lower:
        return "config-descriptor-full-transfer-failed"
    if "config-descriptor-full-transfer-timeout" in lower:
        return "config-descriptor-full-transfer-timeout"
    if "config-descriptor-full-status-timeout" in lower:
        return "config-descriptor-full-status-timeout"
    for token in (
        "config-descriptor-full-transfer-event-slot-empty",
        "config-descriptor-full-transfer-event-cycle-mismatch",
        "config-descriptor-full-transfer-event-ignored",
        "config-descriptor-full-status-event-slot-empty",
        "config-descriptor-full-status-event-cycle-mismatch",
        "config-descriptor-full-status-event-ignored",
    ):
        if token in lower:
            return token
    if "invalid-config-value" in lower:
        return "invalid-config-value"
    if "hid-init-failed" in lower:
        return "hid-init-failed"
    if "hid-endpoint-not-ready" in lower:
        return "hid-endpoint-not-ready"
    if "hid-endpoint-parse-no-reply" in lower:
        return "hid-endpoint-parse-no-reply"
    if "hid-endpoint-not-found" in lower:
        return "hid-endpoint-not-found"
    if "hid-interface-not-found" in lower:
        return "hid-interface-not-found"
    if "hid-interrupt-in-not-found" in lower:
        return "hid-interrupt-in-not-found"
    if "hid-config-descriptor-malformed" in lower:
        return "hid-config-descriptor-malformed"
    if "hub-child-scan-no-reply" in lower:
        return "hub-child-scan-no-reply"
    if "hub-child-probe-no-reply" in lower:
        return "hub-child-probe-no-reply"
    for token in (
        "hub-set-configuration-no-reply",
        "hub-set-configuration-status-no-reply",
        "hub-set-configuration-complete-no-reply",
        "hub-set-configuration-status-timeout",
        "hub-set-configuration-status-event-slot-empty",
        "hub-set-configuration-status-event-cycle-mismatch",
        "hub-set-configuration-status-event-ignored",
        "hub-set-configuration-settle-no-reply",
        "hub-descriptor-no-reply",
        "hub-descriptor-transfer-no-reply",
        "hub-descriptor-status-no-reply",
        "hub-descriptor-transfer-failed",
        "hub-descriptor-transfer-timeout",
        "hub-descriptor-status-timeout",
        "hub-descriptor-transfer-event-slot-empty",
        "hub-descriptor-transfer-event-cycle-mismatch",
        "hub-descriptor-transfer-event-ignored",
        "hub-descriptor-status-event-slot-empty",
        "hub-descriptor-status-event-cycle-mismatch",
        "hub-descriptor-status-event-ignored",
        "hub-context-no-reply",
        "hub-port-power-no-reply",
        "hub-port-status-no-reply",
        "hub-port-status-transfer-no-reply",
        "hub-port-status-status-no-reply",
        "hub-port-status-transfer-timeout",
        "hub-port-status-timeout",
        "hub-port-status-transfer-event-slot-empty",
        "hub-port-status-transfer-event-cycle-mismatch",
        "hub-port-status-transfer-event-ignored",
        "hub-port-status-status-event-slot-empty",
        "hub-port-status-status-event-cycle-mismatch",
        "hub-port-status-status-event-ignored",
        "hub-port-status-payload-no-reply",
        "hub-port-disconnected",
        "hub-port-reset-still-active",
        "hub-port-enable-missing",
        "hub-port-clear-changes-no-reply",
        "hub-port-clear-changes-failed",
        "hub-port-status-failed",
        "hub-port-reset-no-reply",
        "hub-port-reset-set-no-reply",
        "hub-port-reset-set-failed",
        "hub-port-reset-completion-no-reply",
        "hub-child-speed-fallback-no-reply",
    ):
        if token in lower:
            return token
    if "hub-topology-no-keyboard" in lower:
        return "hub-topology-no-keyboard"
    if "hid-configure-endpoint-no-reply" in lower:
        return "hid-configure-endpoint-no-reply"
    if "hid-configure-endpoint-failed" in lower:
        return "hid-configure-endpoint-failed"
    if "hid-set-configuration-no-reply" in lower:
        return "hid-set-configuration-no-reply"
    if "hid-set-configuration-failed" in lower:
        return "hid-set-configuration-failed"
    if "hid-control-no-reply" in lower:
        return "hid-control-no-reply"
    if "hid-control-failed" in lower:
        return "hid-control-failed"
    if "hid-interrupt-queue-no-reply" in lower:
        return "hid-interrupt-queue-no-reply"
    if "hid-interrupt-queue-failed" in lower:
        return "hid-interrupt-queue-failed"
    if stripped == "first-hid-report":
        return "hid-first-report"
    if "hid-attach-failed" in lower:
        return "hid-attach-failed"
    if "hub-attach-failed" in lower:
        return "hub-attach-failed"
    if "hub-topology-no-keyboard" in lower:
        return "hub-topology-no-keyboard"
    if "queue-read" in lower and any(
        token in lower for token in ("fail", "error", "timeout")
    ):
        return "hid-queue-read-failed"
    if "first-report" in lower and any(
        token in lower for token in ("fail", "missing", "timeout")
    ):
        return "hid-first-report"
    if "command-input-ready" in lower or "command-ready" in lower:
        return "command-input-ready"
    if "interrupt-in" in lower and any(
        token in lower for token in ("fail", "missing", "timeout")
    ):
        return "hid-interrupt-in"
    if "first-byte" in lower and any(
        token in lower for token in ("fail", "missing", "timeout")
    ):
        return "keyboard-first-byte"
    if "hid-first-byte" in lower:
        return "keyboard-first-byte"
    if "no-keyboard" in lower or "keyboard-missing" in lower:
        return "no-keyboard-found"
    if ("device-descriptor" in lower or "device-desc" in lower) and any(
        token in lower for token in ("fail", "timeout", "missing", "error")
    ):
        return "device-descriptor-failed"
    if ("config-descriptor" in lower or "config-desc" in lower) and any(
        token in lower for token in ("fail", "timeout", "missing", "error")
    ):
        return "config-descriptor-failed"
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


def usb_runtime_detail_gate_blocker(detail: int | None) -> tuple[int, str] | None:
    """Return the gate/blocker represented by a USB runtime detail code."""

    if detail is None:
        return None
    return USB_RUNTIME_DETAIL_GATES.get(detail)


def usb_command_probe_success(
    value: str,
    event_generation: str | None = None,
    cleanup_generation: str | None = None,
    recovery_source: str | None = None,
) -> bool:
    """Return true for command proofs that satisfy the Pi 4 command gate."""

    label = value.lower().strip()
    event = (event_generation or "").lower().strip()
    cleanup = (cleanup_generation or "").lower().strip()
    recovery = (recovery_source or "").lower().strip()
    if label.startswith("no-op-"):
        return False
    if label in {
        "enable-slot-linux-captured-ok",
        "enable-slot-linux-captured-ok-cleanup-failed",
    }:
        return (
            event == "linux-captured-command-event-generation-after-uboot-timeout"
            and cleanup == "linux-captured-command-event-generation"
            and recovery == "enable-slot-recovery-timeout"
        )
    if "linux-event-ok" in label:
        return False
    if event and "linux-shaped" in event:
        return False
    if cleanup and "linux" in cleanup:
        return False
    if label.startswith("enable-slot-recovery-ok"):
        return cleanup == "uboot-poll-only" and recovery == "enable-slot-timeout"
    return label.endswith("-ok") or label.endswith("-ok-cleanup-failed")


def usb_command_probe_result_success(
    raw: str,
    fields: dict[str, str],
    command_recovery_source: str | None = None,
) -> bool:
    """Return true when a result= command probe can credit linked-runtime proof."""

    lower = raw.lower()
    if "command-probe" not in lower:
        return False
    if "[local-seat] xhci" in lower:
        result = fields.get("result", "").lower().strip()
        event = fields.get("event_generation", "").lower().strip()
        if not (result.endswith("-ok-cleanup-failed") and "uboot" in event):
            return False
    return usb_command_probe_success(
        fields.get("result", ""),
        fields.get("event_generation"),
        fields.get("cleanup_generation"),
        fields.get("recovery_source") or command_recovery_source,
    )


def parse_hex_int(value: str | None) -> int | None:
    """Parse a decimal or hex integer field value, returning None on absence."""

    if value is None:
        return None
    try:
        return int(value, 0)
    except ValueError:
        return None


def driver_task_service_budget_overrun(fields: Mapping[str, str]) -> bool:
    """Return true when observed service time exceeds a declared max budget."""

    observed_us = parse_hex_int(fields.get("observed_service_us"))
    if observed_us is None:
        observed_us = parse_hex_int(fields.get("service_us"))
    if observed_us is None:
        observed_us = parse_hex_int(fields.get("latency_us"))
    max_service_us = parse_hex_int(fields.get("max_service_us"))
    if max_service_us is None:
        max_service_us = parse_hex_int(fields.get("service_max_us"))
    if max_service_us is None:
        max_service_us = parse_hex_int(fields.get("max_latency_us"))
    if observed_us is None or max_service_us is None or max_service_us <= 0:
        return False
    return observed_us > max_service_us


CYW43_CONTROL_EXCHANGE_FAULT_DETAIL = 0x530B
CYW43_CONTROL_EXCHANGE_OP = 11
CYW43_CONTROL_EXCHANGE_BCME_UNSUPPORTED = 0xFFFF_FFE9
CYW43_CONTROL_EXCHANGE_BCME_BADARG = 0xFFFF_FFFE
CYW43_CONTROL_EXCHANGE_TIMEOUT_RESULT_MAGIC = 0x4300_0000
CYW43_CONTROL_EXCHANGE_TIMEOUT_RESULT_MASK = 0xFF00_0000
CYW43_CONTROL_EXCHANGE_TIMEOUT_REASON_SHIFT = 16
CYW43_CONTROL_EXCHANGE_TIMEOUT_REASON_MASK = 0xFF
CYW43_CONTROL_EXCHANGE_TIMEOUT_REASONS = {
    1: "cyw43-control-rx-not-ready",
    2: "cyw43-control-rframe-count-read-failed",
    3: "cyw43-control-rx-no-rframe",
    4: "cyw43-control-rx-invalid-rframe-len",
    5: "cyw43-control-rx-request-too-large",
    6: "cyw43-control-rx-f2-read-failed",
    7: "cyw43-control-rx-sdpcm-decode-miss",
    8: "cyw43-control-reply-nonmatching",
    9: "cyw43-control-rx-firstread-failed",
    10: "cyw43-control-rx-firstread-empty",
    11: "cyw43-control-rx-firstread-invalid-sdpcm",
    12: "cyw43-control-rx-firstread-remainder-failed",
    13: "cyw43-control-rx-firstread-remainder-too-large",
    14: "cyw43-control-rx-firstread-source-asserted-empty",
}
CYW43_CONTROL_POLL_IDLE_DETAIL_REASONS = {
    0x5701: "cyw43-control-rx-not-ready",
    0x5702: "cyw43-control-rframe-count-read-failed",
    0x5703: "cyw43-control-rx-no-rframe",
    0x5704: "cyw43-control-rx-invalid-rframe-len",
    0x5705: "cyw43-control-rx-request-too-large",
    0x5706: "cyw43-control-rx-f2-read-failed",
    0x5707: "cyw43-control-rx-sdpcm-decode-miss",
    0x5709: "cyw43-control-rx-firstread-failed",
    0x570A: "cyw43-control-rx-firstread-empty",
    0x570B: "cyw43-control-rx-firstread-invalid-sdpcm",
    0x570C: "cyw43-control-rx-firstread-remainder-failed",
    0x570D: "cyw43-control-rx-firstread-remainder-too-large",
    0x570E: "cyw43-control-rx-firstread-source-asserted-empty",
}

CYW43_HOST_EAPOL_FIRSTREAD_BLOCKERS = {
    0x5706: "cyw43-data-rx-f2-read-failed",
    0x5707: "cyw43-data-rx-sdpcm-decode-miss",
    0x5709: "cyw43-data-rx-firstread-failed",
    0x570A: "cyw43-data-rx-firstread-empty",
    0x570B: "cyw43-data-rx-firstread-invalid-sdpcm",
    0x570C: "cyw43-data-rx-firstread-remainder-failed",
    0x570D: "cyw43-data-rx-firstread-remainder-too-large",
    0x570E: "cyw43-data-rx-firstread-source-asserted-empty",
}
CYW43_HOST_EAPOL_FIRSTREAD_BLOCKER_NAMES = frozenset(
    set(CYW43_HOST_EAPOL_FIRSTREAD_BLOCKERS.values())
    | {
        "cyw43-control-rx-retransmit-ack-timeout",
        "cyw43-control-rx-retransmit-live-source-rframe-unavailable",
        "cyw43-control-rx-retransmit-sample-block",
        "cyw43-control-rx-cmd53-fifo-window-mismatch",
        "cyw43-control-rx-cmd53-function-mismatch",
        "cyw43-control-rx-cmd53-write",
        "cyw43-control-rx-firstread-cmd53-block-mode",
        "cyw43-control-rx-firstread-cmd53-count-mismatch",
        "cyw43-control-rx-firstread-short-read",
        "cyw43-control-rx-firstread-transfer-no-result",
        "cyw43-control-rx-queue-push-failed",
        "cyw43-control-rx-queue-full",
        "cyw43-control-rx-queue-invalid-len",
        "cyw43-control-rx-queue-invalid-flags",
        "cyw43-control-rx-ring-copy-failed",
        "cyw43-data-rx-retransmit-ack-timeout",
        "cyw43-data-rx-retransmit-live-source-rframe-unavailable",
        "cyw43-data-rx-retransmit-sample-block",
        "cyw43-data-rx-cmd53-fifo-window-mismatch",
        "cyw43-data-rx-cmd53-function-mismatch",
        "cyw43-data-rx-cmd53-write",
        "cyw43-data-rx-firstread-cmd53-block-mode",
        "cyw43-data-rx-firstread-cmd53-count-mismatch",
        "cyw43-data-rx-firstread-short-read",
        "cyw43-data-rx-firstread-transfer-no-result",
        "cyw43-data-rx-queue-push-failed",
        "cyw43-data-rx-queue-full",
        "cyw43-data-rx-queue-invalid-len",
        "cyw43-data-rx-queue-invalid-flags",
        "cyw43-data-rx-ring-copy-failed",
    }
)
CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY = (
    "cyw43-data-rx-firstread-source-asserted-empty"
)
CYW43_HOST_EAPOL_KEY_INSTALL_BLOCKERS = {
    "host-eapol-ptk-install",
    "host-eapol-gtk-install",
    "cyw43-host-eapol-ptk-tx-no-reply",
    "cyw43-host-eapol-ptk-tx-retry-no-reply",
    "cyw43-host-eapol-ptk-poll-timeout",
    "cyw43-host-eapol-ptk-reply-nonmatching",
    "cyw43-host-eapol-gtk-tx-no-reply",
    "cyw43-host-eapol-gtk-tx-retry-no-reply",
    "cyw43-host-eapol-gtk-poll-timeout",
    "cyw43-host-eapol-gtk-reply-nonmatching",
    "cyw43-host-eapol-post-secure-ptk-tx-no-reply",
    "cyw43-host-eapol-post-secure-ptk-tx-retry-no-reply",
    "cyw43-host-eapol-post-secure-ptk-poll-timeout",
    "cyw43-host-eapol-post-secure-ptk-reply-nonmatching",
    "cyw43-host-eapol-post-secure-gtk-tx-no-reply",
    "cyw43-host-eapol-post-secure-gtk-tx-retry-no-reply",
    "cyw43-host-eapol-post-secure-gtk-poll-timeout",
    "cyw43-host-eapol-post-secure-gtk-reply-nonmatching",
}
CYW43_HOST_EAPOL_KEY_INSTALL_STATUS_BLOCKERS = {
    "host-eapol-ptk-install",
    "host-eapol-gtk-install",
}
CYW43_HOST_EAPOL_KEY_STAGES = {
    "cyw43-host-eapol-ptk",
    "cyw43-host-eapol-gtk",
    "cyw43-host-eapol-post-secure-ptk",
    "cyw43-host-eapol-post-secure-gtk",
}
CYW43_HOST_EAPOL_KEY_TX_NO_REPLY_STATUSES = {
    "tx-no-reply",
    "tx-retry-no-reply",
}
CYW43_HOST_EAPOL_KEY_POLL_TIMEOUT_STATUSES = {
    "poll-timeout",
}
CYW43_RXTRACE_RETRANSMIT_ACK_TIMEOUT = 0x0010
CYW43_RXTRACE_RETRANSMIT_STALE_CLEARED = 0x0020
CYW43_RXTRACE_QUEUE_PUSH_FAILED = 0x0040
CYW43_RXTRACE_QUEUE_FULL = 0x0080
CYW43_RXTRACE_QUEUE_INVALID_LEN = 0x0100
CYW43_RXTRACE_QUEUE_INVALID_FLAGS = 0x0200
CYW43_RXTRACE_RING_COPY_FAILED = 0x0400
CYW43_RXTRACE_RETRANSMIT_SOURCE_UNAVAILABLE = 0x0800
CYW43_RXTRACE_RETRANSMIT_RFRAME_ZERO = 0x1000
CYW43_RXTRACE_RETRANSMIT_RFRAME_READY = 0x2000
CYW43_RXTRACE_RETRANSMIT_SOURCE_ASSERTED = 0x4000
CYW43_RXTRACE_RETRANSMIT_ASSERTED_ZERO_READ = 0x8000
CYW43_RXTRACE_RETX_ACTION_MASK = 0x000F
CYW43_RXTRACE_RETX_ACTION_BLOCK = 1
CYW43_RXTRACE_RETX_ACTION_CLEAR_STALE = 2
CYW43_RXTRACE_RETX_ACTION_READ_ASSERTED_ZERO = 3
CYW43_RXTRACE_RETX_ACTION_READ_RFRAME_READY = 4
CYW43_RXTRACE_RETX_ACTION_READ_SOURCE_ASSERTED = 5
CYW43_RXTRACE_RETX_SOURCE_SHIFT = 4
CYW43_RXTRACE_RETX_SOURCE_MASK = 0x000F
CYW43_RXTRACE_RETX_SOURCE_ASSERTED = 2
CYW43_RXTRACE_RETX_RFRAME_SHIFT = 8
CYW43_RXTRACE_RETX_RFRAME_MASK = 0x000F
CYW43_RXTRACE_RETX_RFRAME_UNAVAILABLE = 1
CYW43_RXTRACE_SOURCE_PRE_ASSERTED = 0x0002
CYW43_RXTRACE_SOURCE_POST_ASSERTED = 0x0008
CYW43_CMD53_BYTE_MODE_MAX = 512
CYW43_CMD53_FUNCTION2 = 2
CYW43_FUNCTION2_FIFO_WINDOW = 0x8000
CYW43_HOST_EAPOL_RX_OWNER_BLOCKER_PREFIXES = (
    "cyw43-control-rx-sdio-owner-",
    "cyw43-data-rx-sdio-owner-",
)
CYW43_HOST_EAPOL_FIRSTREAD_DETAILS = frozenset(
    {
        0x5709,
        0x570A,
        0x570B,
        0x570E,
    }
)
CYW43_HOST_EAPOL_RX_CMD53_DETAILS = CYW43_HOST_EAPOL_FIRSTREAD_DETAILS | {
    0x5706,
    0x570C,
    0x570D,
}
CYW43_ASSOCIATION_EVENT_MISSING = "cyw43-association-event-missing"
CYW43_HOST_EAPOL_BSSID_TX_SUBMIT_FAIL = (
    "cyw43-host-eapol-bssid-probe-tx-submit-fail"
)
WIFI_GATE7_SUBGATE_NAMES = {
    "7a": "join-submit",
    "7b": "association",
    "7c": "eapol-rx",
    "7d": "eapol-handshake",
    "7e": "secure-release",
}


def cyw43_host_eapol_rx_blocker_name(value: str) -> bool:
    """Return true for host-EAPOL RX blockers, including owner-fault variants."""

    return value in CYW43_HOST_EAPOL_FIRSTREAD_BLOCKER_NAMES or value.startswith(
        CYW43_HOST_EAPOL_RX_OWNER_BLOCKER_PREFIXES
    )


def cyw43_host_eapol_key_tx_no_reply_detail(
    event: TraceEvent,
) -> tuple[str, str] | None:
    """Return exact PTK/GTK key-install no-reply blocker and runtime phase."""

    stage = event.fields.get("stage", "").lower()
    status = event.fields.get("status", "").lower()
    command_no_reply = (
        "cyw43_driver_task_command_no_reply" in event.raw.lower()
        and event.fields.get("reason", "").lower() == "cyw43-runtime-command-no-reply"
    )
    poll_timeout_command = (
        command_no_reply
        and parse_hex_int(event.fields.get("op")) == CYW43_CONTROL_EXCHANGE_OP
        and event.fields.get("iovar", "").lower() == "wsec_key"
    )
    if stage not in CYW43_HOST_EAPOL_KEY_STAGES or (
        status not in CYW43_HOST_EAPOL_KEY_TX_NO_REPLY_STATUSES
        and status not in CYW43_HOST_EAPOL_KEY_POLL_TIMEOUT_STATUSES
        and not command_no_reply
    ):
        return None
    blocker = event.fields.get("blocker", "").lower()
    if blocker.startswith(f"{stage}-"):
        exact = blocker
    elif poll_timeout_command:
        exact = f"{stage}-poll-timeout"
    else:
        exact = f"{stage}-{status or 'tx-no-reply'}"
    phase = event.fields.get("progress_phase_name") or stage
    return exact, phase


def cyw43_host_eapol_key_install_success_stage(event: TraceEvent) -> str | None:
    """Return the key stage when later runtime evidence marks it successful."""

    raw = event.raw.lower()
    stage = event.fields.get("stage", "").lower()
    if stage not in CYW43_HOST_EAPOL_KEY_STAGES:
        return None
    if (
        "cyw43_driver_task_host_eapol_key" in raw
        and event.fields.get("status", "").lower() == "ready"
    ):
        return stage
    if "cyw43_driver_task_control_reply" not in raw:
        return None
    if event.fields.get("event", "").lower() != "matched-reply":
        return None
    if parse_hex_int(event.fields.get("status")) != 0:
        return None
    return stage


def normalize_wifi_gate7_subgate(value: str | None) -> str:
    """Return a stable Gate 7 sub-gate label."""

    if value is None:
        return "none"
    subgate = value.lower().strip()
    return subgate if subgate in WIFI_GATE7_SUBGATE_NAMES else "none"


def summarize_wifi_gate7_status_subgate(event: TraceEvent) -> WifiGate7Subgate | None:
    """Infer a Gate 7 sub-gate from a host-EAPOL status record."""

    if "cyw43_driver_task_host_eapol_status" not in event.raw.lower():
        return None

    fields = event.fields
    status = fields.get("status", "none").lower()
    reason = normalize_wifi_blocker(fields.get("reason", ""))
    next_action = fields.get("next_action", "none").lower()
    associated = cyw43_field_yes(fields, "associated")
    link_up = cyw43_field_yes(fields, "link_up")
    event_rx = parse_hex_int(fields.get("event_rx")) or 0
    eapol_rx = parse_hex_int(fields.get("eapol_rx")) or 0
    data_rx = parse_hex_int(fields.get("data_rx")) or 0
    polls = parse_hex_int(fields.get("polls")) or 0
    detail = next_action if next_action != "none" else reason
    if detail in {"none", "unknown"}:
        detail = status if status else "none"

    if status == "secure" or next_action == "release-dhcp-data":
        return WifiGate7Subgate(
            "7e", "secure-release", "host-eapol-status", status, detail, event.line
        )
    if associated and link_up and eapol_rx > 0:
        return WifiGate7Subgate(
            "7d", "eapol-handshake", "host-eapol-status", status, detail, event.line
        )
    if associated and link_up:
        return WifiGate7Subgate(
            "7c", "eapol-rx", "host-eapol-status", status, detail, event.line
        )
    if status == "pending" and polls == 0 and event_rx == 0 and data_rx == 0:
        return WifiGate7Subgate(
            "7a", "join-submit", "host-eapol-status", status, "join-accepted", event.line
        )
    if status in {"pending", "required", "event-rx", "rx-observed", "eapol-rx"} or (
        polls != 0 or event_rx != 0 or data_rx != 0
    ):
        return WifiGate7Subgate(
            "7b", "association", "host-eapol-status", status, detail, event.line
        )
    return None


def summarize_wifi_gate7_subgate_detail(
    events: Iterable[TraceEvent], wifi_gate: int, wifi_blocker: str
) -> WifiGate7Subgate:
    """Return the latest WiFi Gate 7 sub-gate frontier with source detail."""

    latest: WifiGate7Subgate | None = None
    for event in events:
        raw = event.raw.lower()
        if (
            "cyw43_driver_task_wifi_gate7" in raw
            or "cyw43_driver_task_join_submit_window" in raw
        ):
            subgate = normalize_wifi_gate7_subgate(event.fields.get("subgate"))
            if subgate != "none":
                source = event.fields.get("source") or "join-submit-window"
                reason = (
                    event.fields.get("reason")
                    or event.fields.get("focus")
                    or event.fields.get("name")
                    or "none"
                )
                latest = WifiGate7Subgate(
                    subgate,
                    WIFI_GATE7_SUBGATE_NAMES[subgate],
                    source,
                    event.fields.get("status", "none"),
                    reason,
                    event.line,
                )
                continue
        status_subgate = summarize_wifi_gate7_status_subgate(event)
        if status_subgate is not None:
            latest = status_subgate
    if latest is not None:
        return latest
    if wifi_gate != 7:
        return WifiGate7Subgate("none", "none")
    if wifi_blocker in {
        "cyw43-post-release-ht-clock",
        "cyw43-post-release-function2-ready",
        "cyw43-post-release-corecontrol",
        "cyw43-post-release-mailbox-ready",
        "cyw43-post-release-protocol-version",
        "cyw43-post-release-rframe-sample",
    }:
        return WifiGate7Subgate("none", "none", reason=wifi_blocker)
    if wifi_blocker in {
        "cyw43-sdio-descriptor-transfer-failed",
        "cyw43-control-tx-no-reply",
        "cyw43-control-tx-retry-no-reply",
        "cyw43-control-tx-not-submitted",
    }:
        return WifiGate7Subgate("none", "none", reason=wifi_blocker)
    if wifi_blocker in {
        "cyw43-association-event-missing",
        "cyw43-association-not-associated",
    }:
        return WifiGate7Subgate("7b", "association", reason=wifi_blocker)
    if wifi_blocker in {"host-eapol-required", "wifi-host-eapol-pending"}:
        return WifiGate7Subgate("7c", "eapol-rx", reason=wifi_blocker)
    if wifi_blocker in {
        "join-pending",
        "join-completion-unproven",
        "firmware-supplicant-unsupported",
        "wsec-pmk-bad-argument",
    } or wifi_blocker.startswith("join-security-"):
        return WifiGate7Subgate("7a", "join-submit", reason=wifi_blocker)
    if cyw43_host_eapol_rx_blocker_name(wifi_blocker):
        return WifiGate7Subgate("7c", "eapol-rx", reason=wifi_blocker)
    return WifiGate7Subgate("7a", "join-submit", reason=wifi_blocker)


def summarize_wifi_gate7_subgate(
    events: Iterable[TraceEvent], wifi_gate: int, wifi_blocker: str
) -> tuple[str, str]:
    """Return the latest WiFi Gate 7 sub-gate frontier."""

    detail = summarize_wifi_gate7_subgate_detail(events, wifi_gate, wifi_blocker)
    return detail.subgate, detail.name


def cyw43_field_yes(fields: dict[str, str], key: str) -> bool:
    """Return true when a decoded trace field carries a yes/true value."""

    return fields.get(key, "").lower() in {"yes", "true", "1"}


def cyw43_trace_field(fields: dict[str, str], prefix: str, suffix: str) -> str | None:
    """Return a prefixed trace field, or an unprefixed standalone field."""

    key = f"{prefix}_{suffix}" if prefix else suffix
    return fields.get(key)


def cyw43_host_eapol_rx_source_asserted(fields: dict[str, str], prefix: str) -> bool:
    """Return true when a host-EAPOL source lane reports pending RX source bits."""

    mode = fields.get(f"{prefix}_mode", "").lower()
    if mode.endswith("cached"):
        return False
    return (
        cyw43_field_yes(fields, f"{prefix}_f2_ready")
        and (
            cyw43_field_yes(fields, f"{prefix}_frame_ind")
            or cyw43_field_yes(fields, f"{prefix}_host_int")
            or cyw43_field_yes(fields, f"{prefix}_card_int")
        )
    )


def cyw43_host_eapol_rxtrace_source_asserted(
    fields: dict[str, str], prefix: str
) -> bool:
    """Return true when v3 RX trace source flags show asserted source bits."""

    def field_name(suffix: str) -> str:
        return f"{prefix}_{suffix}" if prefix else suffix

    source_flags = parse_hex_int(fields.get(field_name("source_flags"))) or 0
    if source_flags & (
        CYW43_RXTRACE_SOURCE_PRE_ASSERTED | CYW43_RXTRACE_SOURCE_POST_ASSERTED
    ):
        return True
    if fields.get(field_name("source_asserted_ever"), "").lower() == "yes":
        return True
    if fields.get(field_name("pre_asserted"), "").lower() == "yes":
        return True
    if fields.get(field_name("post_asserted"), "").lower() == "yes":
        return True
    flags = parse_hex_int(fields.get(field_name("flags"))) or 0
    return bool(
        flags
        & (
            CYW43_RXTRACE_RETRANSMIT_SOURCE_ASSERTED
            | CYW43_RXTRACE_RETRANSMIT_ASSERTED_ZERO_READ
        )
    )


def cyw43_host_eapol_any_rx_source_asserted(fields: dict[str, str]) -> bool:
    """Return true when status fields prove live RX source assertion."""

    firstread_class = fields.get("firstread_class", "").lower()
    return (
        firstread_class == "source-asserted-empty"
        or cyw43_host_eapol_rx_source_asserted(fields, "rxsrc")
        or cyw43_host_eapol_rx_source_asserted(fields, "control_rxsrc")
        or cyw43_host_eapol_rxtrace_source_asserted(fields, "rxtrace")
        or cyw43_host_eapol_rxtrace_source_asserted(fields, "control_rxtrace")
    )


def cyw43_host_eapol_quiet_preassoc_firstread(fields: dict[str, str]) -> bool:
    """Return true when an empty firstread is only pre-association cadence evidence."""

    firstread_class = fields.get("firstread_class", "").lower()
    if firstread_class == "preassoc-cadence-empty":
        return True
    if firstread_class == "source-asserted-empty":
        return False
    reason = normalize_wifi_blocker(fields.get("reason", ""))
    detail = parse_hex_int(fields.get("last_rx_idle_detail")) or 0
    if detail == 0x570E:
        return False
    if (
        reason == "cyw43-association-not-associated"
        or cyw43_host_eapol_post_rescue_association_gap(fields)
    ):
        return True
    return (
        reason in {CYW43_ASSOCIATION_EVENT_MISSING, "cyw43-association-not-associated"}
        and fields.get("associated", "").lower() in {"", "no"}
        and not cyw43_host_eapol_any_rx_source_asserted(fields)
    )


def cyw43_host_eapol_retx_sample(fields: dict[str, str], prefix: str) -> int:
    """Return the decoded host-EAPOL retransmit sample word."""

    return parse_hex_int(cyw43_trace_field(fields, prefix, "retx_sample")) or 0


def cyw43_host_eapol_retx_action(fields: dict[str, str], prefix: str) -> int:
    """Return the retransmit sample action for a host-EAPOL trace lane."""

    action = cyw43_trace_field(fields, prefix, "retx_action")
    if action == "block":
        return CYW43_RXTRACE_RETX_ACTION_BLOCK
    if action == "clear-stale":
        return CYW43_RXTRACE_RETX_ACTION_CLEAR_STALE
    if action == "read-asserted-zero":
        return CYW43_RXTRACE_RETX_ACTION_READ_ASSERTED_ZERO
    if action == "read-rframe-ready":
        return CYW43_RXTRACE_RETX_ACTION_READ_RFRAME_READY
    if action == "read-source-asserted":
        return CYW43_RXTRACE_RETX_ACTION_READ_SOURCE_ASSERTED
    return cyw43_host_eapol_retx_sample(fields, prefix) & CYW43_RXTRACE_RETX_ACTION_MASK


def cyw43_host_eapol_retx_blocker(fields: dict[str, str], prefix: str, lane: str) -> str | None:
    """Return the precise retransmit sample blocker for a host-EAPOL lane."""

    sample = cyw43_host_eapol_retx_sample(fields, prefix)
    if cyw43_host_eapol_retx_action(fields, prefix) != CYW43_RXTRACE_RETX_ACTION_BLOCK:
        return None
    source = (sample >> CYW43_RXTRACE_RETX_SOURCE_SHIFT) & CYW43_RXTRACE_RETX_SOURCE_MASK
    rframe = (sample >> CYW43_RXTRACE_RETX_RFRAME_SHIFT) & CYW43_RXTRACE_RETX_RFRAME_MASK
    if (
        source == CYW43_RXTRACE_RETX_SOURCE_ASSERTED
        and rframe == CYW43_RXTRACE_RETX_RFRAME_UNAVAILABLE
    ):
        return f"cyw43-{lane}-rx-retransmit-live-source-rframe-unavailable"
    return f"cyw43-{lane}-rx-retransmit-sample-block"


def cyw43_host_eapol_cmd53_arg(fields: dict[str, str], prefix: str) -> int:
    """Return the raw CMD53 argument recorded by a host-EAPOL trace lane."""

    return parse_hex_int(cyw43_trace_field(fields, prefix, "cmd53_arg")) or 0


def cyw43_host_eapol_cmd53_bool(
    fields: dict[str, str], prefix: str, suffix: str, arg: int
) -> bool:
    """Return a decoded CMD53 boolean field, falling back to the raw argument."""

    value = (cyw43_trace_field(fields, prefix, suffix) or "").lower()
    if value in {"yes", "true", "1"}:
        return True
    if value in {"no", "false", "0"}:
        return False
    if suffix == "cmd53_write":
        return arg & (1 << 31) != 0
    if suffix == "cmd53_inc":
        return arg & (1 << 26) != 0
    return False


def cyw43_host_eapol_cmd53_function(
    fields: dict[str, str], prefix: str, arg: int
) -> int:
    """Return the CMD53 function for a host-EAPOL trace lane."""

    return parse_hex_int(cyw43_trace_field(fields, prefix, "cmd53_fn")) or (
        (arg >> 28) & 0x7
    )


def cyw43_host_eapol_cmd53_addr(fields: dict[str, str], prefix: str, arg: int) -> int:
    """Return the CMD53 address/window for a host-EAPOL trace lane."""

    return parse_hex_int(cyw43_trace_field(fields, prefix, "cmd53_addr")) or (
        (arg >> 9) & 0x1FFFF
    )


def cyw43_host_eapol_cmd53_mode(fields: dict[str, str], prefix: str, arg: int) -> str:
    """Return the CMD53 transfer mode for a host-EAPOL trace lane."""

    mode = (cyw43_trace_field(fields, prefix, "cmd53_mode") or "").lower()
    if mode:
        return mode
    if arg == 0:
        return "none"
    if arg & (1 << 27):
        return "block"
    if cyw43_host_eapol_cmd53_count(fields, prefix, arg) == CYW43_CMD53_BYTE_MODE_MAX:
        return "byte512"
    return "byte"


def cyw43_host_eapol_cmd53_count(fields: dict[str, str], prefix: str, arg: int) -> int:
    """Return the decoded CMD53 byte or block count for a host-EAPOL trace lane."""

    decoded = parse_hex_int(cyw43_trace_field(fields, prefix, "cmd53_count"))
    if decoded is not None:
        return decoded
    if arg == 0:
        return 0
    raw_count = arg & 0x1FF
    if arg & (1 << 27) == 0 and raw_count == 0:
        return CYW43_CMD53_BYTE_MODE_MAX
    return raw_count


def cyw43_host_eapol_rxtrace_shape_blocker(
    fields: dict[str, str], prefix: str, lane: str
) -> str | None:
    """Return a CMD53 shape or transfer-result blocker for a host-EAPOL lane."""

    detail = parse_hex_int(cyw43_trace_field(fields, prefix, "detail")) or 0
    if detail not in CYW43_HOST_EAPOL_RX_CMD53_DETAILS:
        return None
    arg = cyw43_host_eapol_cmd53_arg(fields, prefix)
    if arg == 0:
        return None
    function = cyw43_host_eapol_cmd53_function(fields, prefix, arg)
    if function != CYW43_CMD53_FUNCTION2:
        return f"cyw43-{lane}-rx-cmd53-function-mismatch"
    if cyw43_host_eapol_cmd53_bool(fields, prefix, "cmd53_write", arg):
        return f"cyw43-{lane}-rx-cmd53-write"
    addr = cyw43_host_eapol_cmd53_addr(fields, prefix, arg)
    if addr & CYW43_FUNCTION2_FIFO_WINDOW == 0:
        return f"cyw43-{lane}-rx-cmd53-fifo-window-mismatch"

    if detail in CYW43_HOST_EAPOL_FIRSTREAD_DETAILS:
        mode = cyw43_host_eapol_cmd53_mode(fields, prefix, arg)
        request_len = parse_hex_int(cyw43_trace_field(fields, prefix, "request_len")) or 0
        count = cyw43_host_eapol_cmd53_count(fields, prefix, arg)
        if mode == "block":
            return f"cyw43-{lane}-rx-firstread-cmd53-block-mode"
        if request_len != 0 and count != 0 and count != request_len:
            return f"cyw43-{lane}-rx-firstread-cmd53-count-mismatch"
        transfer_result = (
            parse_hex_int(cyw43_trace_field(fields, prefix, "transfer_result")) or 0
        )
        if request_len != 0 and 0 < transfer_result < request_len:
            return f"cyw43-{lane}-rx-firstread-short-read"
        payload_after = (
            parse_hex_int(cyw43_trace_field(fields, prefix, "payload_after")) or 0
        )
        if detail == 0x5709 and transfer_result == 0 and payload_after == 0:
            return f"cyw43-{lane}-rx-firstread-transfer-no-result"
    return None


def cyw43_host_eapol_rxtrace_lane_blocker(
    fields: dict[str, str], prefix: str, lane: str
) -> str | None:
    """Return the precise blocker for one host-EAPOL RX trace lane."""

    flags = parse_hex_int(cyw43_trace_field(fields, prefix, "flags")) or 0
    if flags & CYW43_RXTRACE_RING_COPY_FAILED:
        return f"cyw43-{lane}-rx-ring-copy-failed"
    if flags & CYW43_RXTRACE_QUEUE_FULL:
        return f"cyw43-{lane}-rx-queue-full"
    if flags & CYW43_RXTRACE_QUEUE_INVALID_LEN:
        return f"cyw43-{lane}-rx-queue-invalid-len"
    if flags & CYW43_RXTRACE_QUEUE_INVALID_FLAGS:
        return f"cyw43-{lane}-rx-queue-invalid-flags"
    if flags & CYW43_RXTRACE_QUEUE_PUSH_FAILED:
        return f"cyw43-{lane}-rx-queue-push-failed"
    retx_blocker = cyw43_host_eapol_retx_blocker(fields, prefix, lane)
    if retx_blocker is not None:
        return retx_blocker
    shape_blocker = cyw43_host_eapol_rxtrace_shape_blocker(fields, prefix, lane)
    if shape_blocker is not None:
        return shape_blocker
    retx_action = cyw43_host_eapol_retx_action(fields, prefix)
    if retx_action in {
        CYW43_RXTRACE_RETX_ACTION_CLEAR_STALE,
        CYW43_RXTRACE_RETX_ACTION_READ_ASSERTED_ZERO,
        CYW43_RXTRACE_RETX_ACTION_READ_RFRAME_READY,
        CYW43_RXTRACE_RETX_ACTION_READ_SOURCE_ASSERTED,
    }:
        return None
    if (
        flags & CYW43_RXTRACE_RETRANSMIT_ACK_TIMEOUT
        and flags & CYW43_RXTRACE_RETRANSMIT_STALE_CLEARED == 0
        and flags & CYW43_RXTRACE_RETRANSMIT_ASSERTED_ZERO_READ == 0
    ):
        return f"cyw43-{lane}-rx-retransmit-ack-timeout"
    return None


def cyw43_host_eapol_rxtrace_blocker(fields: dict[str, str]) -> str | None:
    """Return the precise host-EAPOL RX trace blocker, if present."""

    for prefix, lane in (("rxtrace", "data"), ("control_rxtrace", "control")):
        blocker = cyw43_host_eapol_rxtrace_lane_blocker(fields, prefix, lane)
        if blocker is not None:
            return blocker
    return None


def cyw43_host_eapol_rxtrace_line_blocker(fields: dict[str, str]) -> str | None:
    """Return the precise blocker from one standalone v3 RXTRACE line."""

    lane = fields.get("lane", "data").lower()
    if lane not in {"data", "control"}:
        lane = "data"
    return cyw43_host_eapol_rxtrace_lane_blocker(fields, "", lane)


def cyw43_sdio_owner_fault_reason_slug(value: str) -> str:
    """Return a stable suffix for a CYW43 SDIO owner fault reason."""

    slug = re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")
    return slug or "fault"


def cyw43_host_eapol_sdio_owner_fault_blocker(fields: dict[str, str]) -> str | None:
    """Return the host-EAPOL RX owner-fault blocker, if a line carries one."""

    owner_window = fields.get("owner_window", "").lower()
    function = parse_hex_int(fields.get("fn")) or 0
    if owner_window != "function2-fifo" and function != CYW43_CMD53_FUNCTION2:
        return None
    if fields.get("write", "").lower() == "yes":
        return None
    op = parse_hex_int(fields.get("op")) or 0
    stage = fields.get("stage", "").lower()
    if op not in {8, 10} and "eapol" not in stage and "rx" not in stage:
        return None
    lane = "control" if op == 10 or "control" in stage else "data"
    reason = fields.get("xfer_reason") or fields.get("reason") or "fault"
    return f"cyw43-{lane}-rx-sdio-owner-{cyw43_sdio_owner_fault_reason_slug(reason)}"


def cyw43_host_eapol_firstread_blocker(fields: dict[str, str]) -> str | None:
    """Return the precise host-EAPOL RX first-read blocker, if present."""

    rxtrace_blocker = cyw43_host_eapol_rxtrace_blocker(fields)
    if rxtrace_blocker is not None:
        return rxtrace_blocker
    firstread_class = fields.get("firstread_class", "").lower()
    if firstread_class == "source-asserted-empty":
        return CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY
    if cyw43_host_eapol_quiet_preassoc_firstread(fields):
        return None
    detail = parse_hex_int(fields.get("last_rx_idle_detail"))
    if detail in CYW43_HOST_EAPOL_FIRSTREAD_BLOCKERS:
        return CYW43_HOST_EAPOL_FIRSTREAD_BLOCKERS[detail]
    if (parse_hex_int(fields.get("rx_firstread_invalid")) or 0) > 0:
        return "cyw43-data-rx-firstread-invalid-sdpcm"
    if (parse_hex_int(fields.get("rx_firstread_remainder_failed")) or 0) > 0:
        return "cyw43-data-rx-firstread-remainder-failed"
    if (parse_hex_int(fields.get("rx_firstread_failed")) or 0) > 0:
        return "cyw43-data-rx-firstread-failed"
    if (parse_hex_int(fields.get("rx_firstread_decode_miss")) or 0) > 0:
        return "cyw43-data-rx-sdpcm-decode-miss"
    if (parse_hex_int(fields.get("rx_firstread_empty")) or 0) > 0:
        return "cyw43-data-rx-firstread-empty"
    return None


def cyw43_host_eapol_post_rescue_association_gap(fields: dict[str, str]) -> bool:
    """Return true when a spent SET_SSID rescue still lacks association proof."""

    return (
        fields.get("assoc_set_ssid_rescue", "").lower() == "yes"
        and fields.get("associated", "").lower() == "no"
        and fields.get("link_up", "").lower() == "no"
        and fields.get("assoc_event", "").lower() in {"", "none"}
        and (parse_hex_int(fields.get("data_rx")) or 0) == 0
        and (parse_hex_int(fields.get("event_rx")) or 0) == 0
        and (parse_hex_int(fields.get("eapol_rx")) or 0) == 0
    )


def summarize_host_eapol_firstread_status(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return the latest direct host-EAPOL first-read blocker proof."""

    latest: tuple[str, str, int] | None = None
    for event in events:
        raw = event.raw.lower()
        if "cyw43_sdio_owner_fault" in raw:
            owner_blocker = cyw43_host_eapol_sdio_owner_fault_blocker(event.fields)
            if owner_blocker is not None:
                latest = (owner_blocker, "runtime-rx", event.line)
            continue
        if "cyw43_driver_task_host_eapol_rxtrace" in raw:
            rxtrace_blocker = cyw43_host_eapol_rxtrace_line_blocker(event.fields)
            if rxtrace_blocker is not None:
                latest = (rxtrace_blocker, "runtime-rx", event.line)
                continue
            if cyw43_host_eapol_rxtrace_source_asserted(event.fields, ""):
                latest = (CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY, "runtime-rx", event.line)
            continue
        if "cyw43_driver_task_host_eapol_status" not in raw:
            continue
        fields = event.fields
        blocker = cyw43_host_eapol_firstread_blocker(fields)
        if blocker is None:
            continue
        status = fields.get("status", "").lower()
        reason = normalize_wifi_blocker(fields.get("reason", ""))
        if reason in CYW43_HOST_EAPOL_KEY_INSTALL_BLOCKERS:
            continue
        rxtrace_blocker = cyw43_host_eapol_rxtrace_blocker(fields)
        source_asserted_empty = blocker == CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY
        if (
            reason == CYW43_ASSOCIATION_EVENT_MISSING
            and rxtrace_blocker is None
            and not source_asserted_empty
        ):
            continue
        if (
            reason == "cyw43-association-not-associated"
            and rxtrace_blocker is None
            and not source_asserted_empty
        ):
            continue
        if (
            status == "required"
            and cyw43_host_eapol_post_rescue_association_gap(fields)
            and rxtrace_blocker is None
            and not source_asserted_empty
        ):
            continue
        if status == "required" or reason == "host-eapol-required" or source_asserted_empty:
            latest = (blocker, "runtime-rx", event.line)
    return latest


def cyw43_host_eapol_bssid_tx_submit_fail(event: TraceEvent) -> bool:
    """Return true when the post-join BSSID probe failed before reply polling."""

    raw = event.raw.lower()
    fields = event.fields
    if CYW43_HOST_EAPOL_BSSID_TX_SUBMIT_FAIL in raw:
        return True
    return (
        "driver_task_resource_init" in raw
        and fields.get("contract", "").lower() == "cyw43455"
        and fields.get("stage", "").lower() == "cyw43-host-eapol-bssid-probe"
        and fields.get("status", "").lower() == "tx-submit-fail"
    )


def summarize_host_eapol_bssid_tx_submit_fail(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return the latest BSSID-probe TX-submit blocker, if present."""

    latest: tuple[str, str, int] | None = None
    for event in events:
        if cyw43_host_eapol_bssid_tx_submit_fail(event):
            latest = (
                CYW43_HOST_EAPOL_BSSID_TX_SUBMIT_FAIL,
                "control-tx",
                event.line,
            )
    return latest


def cyw43_control_exchange_timeout_exact(result: int | None) -> str | None:
    """Return the precise CYW43 control timeout reason encoded by the runtime."""

    if result is None:
        return None
    if (
        result & CYW43_CONTROL_EXCHANGE_TIMEOUT_RESULT_MASK
    ) != CYW43_CONTROL_EXCHANGE_TIMEOUT_RESULT_MAGIC:
        return None
    reason = (
        result >> CYW43_CONTROL_EXCHANGE_TIMEOUT_REASON_SHIFT
    ) & CYW43_CONTROL_EXCHANGE_TIMEOUT_REASON_MASK
    return CYW43_CONTROL_EXCHANGE_TIMEOUT_REASONS.get(reason)


def cyw43_control_exchange_timeout_event_exact(event: TraceEvent) -> str | None:
    """Return the exact linked-runtime CYW43 control-exchange timeout reason."""

    fields = event.fields
    if "cyw43_driver_task_command_fault" not in event.raw.lower():
        return None
    if fields.get("contract", "").lower() != "cyw43455":
        return None
    if parse_hex_int(fields.get("op")) != CYW43_CONTROL_EXCHANGE_OP:
        return None
    if (
        parse_hex_int(fields.get("detail"))
        != CYW43_CONTROL_EXCHANGE_FAULT_DETAIL
    ):
        return None
    return cyw43_control_exchange_timeout_exact(parse_hex_int(fields.get("result")))


def cyw43_control_exchange_status_event_exact(event: TraceEvent) -> str | None:
    """Return exact firmware status rejections from linked-runtime control exchange."""

    fields = event.fields
    if "cyw43_driver_task_command_fault" not in event.raw.lower():
        return None
    if fields.get("contract", "").lower() != "cyw43455":
        return None
    if parse_hex_int(fields.get("op")) != CYW43_CONTROL_EXCHANGE_OP:
        return None
    if parse_hex_int(fields.get("detail")) != CYW43_CONTROL_EXCHANGE_FAULT_DETAIL:
        return None
    stage = fields.get("stage", "").lower()
    result = parse_hex_int(fields.get("result"))
    if stage == "cyw43-control-txglomalign":
        if result == CYW43_CONTROL_EXCHANGE_BCME_BADARG:
            return "cyw43-control-txglomalign-badarg"
        if result == CYW43_CONTROL_EXCHANGE_BCME_UNSUPPORTED:
            return "cyw43-control-txglomalign-unsupported"
    return None


def cyw43_control_split_event_exact(event: TraceEvent) -> str | None:
    """Return the exact parent-side split-control exchange failure reason."""

    fields = event.fields
    if "cyw43_driver_task_control_split" not in event.raw.lower():
        return None
    if fields.get("contract", "").lower() != "cyw43455":
        return None
    event_name = fields.get("event", "").lower()
    detail_exact = CYW43_CONTROL_POLL_IDLE_DETAIL_REASONS.get(
        parse_hex_int(fields.get("detail")) or -1
    )
    if event_name in {"tx-complete", "tx-retry-complete"} and parse_hex_int(
        fields.get("detail")
    ) == 0x5103:
        return "cyw43-sdio-descriptor-transfer-failed"
    if event_name == "cyw43-control-reply-nonmatching":
        stage = fields.get("stage", "").lower()
        if stage in CYW43_HOST_EAPOL_KEY_STAGES:
            return f"{stage}-reply-nonmatching"
        return "cyw43-control-reply-nonmatching"
    if event_name == "cyw43-control-split-no-reply":
        return detail_exact or "cyw43-control-split-timeout"
    if event_name in {
        "cyw43-control-tx-no-reply",
        "cyw43-control-tx-retry-no-reply",
        "cyw43-control-tx-not-submitted",
        "cyw43-control-poll-unexpected-completion",
        "cyw43-control-frame-unavailable",
    }:
        return event_name
    return None


def cyw43_control_reply_event_exact(event: TraceEvent) -> str | None:
    """Return the exact CDC reply issue carried by split-control telemetry."""

    fields = event.fields
    if "cyw43_driver_task_control_reply" not in event.raw.lower():
        return None
    if fields.get("contract", "").lower() != "cyw43455":
        return None
    event_name = fields.get("event", "").lower()
    terminal = fields.get("terminal", "").lower() in {"yes", "true", "1"}
    if event_name == "malformed-reply":
        return "cyw43-control-reply-malformed" if terminal else None
    if event_name == "nonmatching-reply":
        return "cyw43-control-reply-nonmatching" if terminal else None
    status = parse_hex_int(fields.get("status"))
    stage = fields.get("stage", "").lower()
    if event_name == "matched-reply" and status not in {None, 0}:
        if stage == "cyw43-control-revinfo" and status == CYW43_CONTROL_EXCHANGE_BCME_BADARG:
            return "cyw43-control-revinfo-badarg"
        return "cyw43-control-reply-status"
    return None


def cyw43_command_no_reply_event_exact(event: TraceEvent) -> str | None:
    """Return the exact linked-runtime CYW43 command no-reply reason."""

    fields = event.fields
    if "cyw43_driver_task_command_no_reply" not in event.raw.lower():
        return None
    if fields.get("contract", "").lower() != "cyw43455":
        return None
    exact = normalize_wifi_exact(fields.get("reason", ""))
    if exact == "cyw43-runtime-command-no-reply":
        progress_exact = normalize_wifi_exact(fields.get("progress_phase_name", ""))
        progress_sequence = parse_hex_int(fields.get("progress_sequence"))
        request = parse_hex_int(fields.get("request"))
        progress_aux0 = parse_hex_int(fields.get("progress_aux0"))
        if (
            fields.get("progress_marker_valid", "").lower() in {"yes", "true", "1"}
            and request is not None
            and progress_sequence == request
            and progress_aux0 == 0x43595734
            and progress_exact != "none"
        ):
            return progress_exact
    if exact != "none":
        return exact
    if parse_hex_int(fields.get("op")) == CYW43_CONTROL_EXCHANGE_OP:
        return "cyw43-runtime-command-no-reply"
    return None


def wifi_join_complete_proven(fields: dict[str, str]) -> bool:
    """Return true when a join-complete log carries the required proof fields."""

    secure = fields.get("secure")
    if secure == "yes":
        if fields.get("completion_rule") == "host-eapol-required":
            return (
                fields.get("m1") == "yes"
                and fields.get("m2") == "yes"
                and fields.get("m3") == "yes"
                and fields.get("m4") == "yes"
                and fields.get("wsec_key") == "ptk+gtk"
                and fields.get("carrier") == "yes"
            )
        return (
            fields.get("completion_rule") == "firmware-supplicant-psk-sup"
            and fields.get("set_ssid") == "yes"
            and fields.get("fwsup") == "yes"
            and fields.get("psk_sup") == "yes"
            and parse_hex_int(fields.get("psk_status")) == 6
        )
    if secure == "no":
        return fields.get("completion_rule") == "set-ssid" and fields.get(
            "set_ssid"
        ) == "yes"
    return False


ARMCR4_WRAP_BASES = ("0x18102000", "0x18103000")


def has_armcr4_wrap_base(value: str) -> bool:
    """Return true when a trace line names the current or legacy ARMCR4 wrapper."""

    lower = value.lower()
    return any(f"base={base}" in lower for base in ARMCR4_WRAP_BASES)


def normalize_wifi_blocker(value: str) -> str:
    """Normalize WiFi blocker strings into stable gate labels."""

    lower = value.lower()
    stripped = lower.strip()
    if stripped in {
        "0",
        "0x0",
        "0x00",
        "0x0000",
        "0x00000000",
        "none",
        "ok",
        "online",
        "ready",
        "success",
    }:
        return "none"
    if (
        "stale-admission-exhausted" in lower
        and ("engine-init" in lower or "net-engine-init" in lower)
    ) or stripped == "stale-admission-exhausted":
        return "cyw43-engine-init-stale-admission-exhausted"
    if uboot_wifi_policy_missing_line(value):
        return "uboot-policy-missing"
    if "wifi-data-rx-admission-blocked" in lower or (
        "cyw43_driver_task_host_eapol_rx_admission" in lower
        and "status=error" in lower
    ):
        return "wifi-data-rx-admission-blocked"
    if "wifi-rx-overflow" in lower or stripped == "rx-overflow":
        return "wifi-rx-overflow"
    if "wifi-rx-starvation" in lower or stripped == "rx-starvation":
        return "wifi-rx-starvation"
    if "host-eapol-m3-missing" in lower:
        return "host-eapol-m3-missing"
    if "wifi-tx-credit-anomaly" in lower or stripped == "tx-credit-anomaly":
        return "wifi-tx-credit-anomaly"
    if "cyw43-transport-command-admission" in lower:
        return "cyw43-runtime-command-rejected"
    if stripped in {"21259", "0x530b"}:
        return "control-plane-reply-idle-loop"
    if cyw43_host_eapol_rx_blocker_name(stripped):
        return stripped
    if "linked_runtime_progress" in lower:
        marker_blocker = parse_fields(value).get("blocker", "").lower()
        if marker_blocker.startswith(
            (
                "cyw43-engine-init-",
                "cyw43-state-reset-",
                "cyw43-bus-link-",
                "cyw43-backplane-",
                "cyw43-release-",
                "cyw43-shared-control-",
                "cyw43-sdio-owner-",
                "cyw43-resource-",
            )
        ) or marker_blocker == "cyw43-forbidden-sdio-mmio":
            return marker_blocker
    if (
        "net_driver_task_replay_status" in lower
        and "role=cyw43-wifi" in lower
        and (
            "stage=engine-init " in lower
            or "stage=engine-init-replay" in lower
        )
        and "blocker=no-reply" in lower
    ) or (
        "driver_task_resource_init" in lower
        and "contract=cyw43455" in lower
        and (
            "stage=net-engine-init " in lower
            or "stage=net-engine-init-replay" in lower
        )
        and "status=no-reply" in lower
    ):
        return "cyw43-engine-init-no-reply"
    if (
        "net_driver_task_replay_status" in lower
        and "role=cyw43-wifi" in lower
        and (
            "stage=engine-init " in lower
            or "stage=engine-init-replay" in lower
        )
        and "blocker=stale-admission-exhausted" in lower
    ) or (
        "driver_task_resource_init" in lower
        and "contract=cyw43455" in lower
        and (
            "stage=net-engine-init " in lower
            or "stage=net-engine-init-replay" in lower
        )
        and "status=stale-admission-exhausted" in lower
    ):
        return "cyw43-engine-init-stale-admission-exhausted"
    if (
        "cyw43-association-not-associated" in lower
        or "association-not-associated" in lower
    ):
        return "cyw43-association-not-associated"
    if CYW43_ASSOCIATION_EVENT_MISSING in lower:
        return CYW43_ASSOCIATION_EVENT_MISSING
    if CYW43_HOST_EAPOL_BSSID_TX_SUBMIT_FAIL in lower:
        return CYW43_HOST_EAPOL_BSSID_TX_SUBMIT_FAIL
    for key_install_blocker in CYW43_HOST_EAPOL_KEY_INSTALL_BLOCKERS:
        if key_install_blocker in lower:
            return key_install_blocker
    for key_stage in CYW43_HOST_EAPOL_KEY_STAGES:
        for key_status in CYW43_HOST_EAPOL_KEY_TX_NO_REPLY_STATUSES:
            if f"stage={key_stage}" in lower and f"status={key_status}" in lower:
                return f"{key_stage}-{key_status}"
        for key_status in CYW43_HOST_EAPOL_KEY_POLL_TIMEOUT_STATUSES:
            if f"stage={key_stage}" in lower and f"status={key_status}" in lower:
                return f"{key_stage}-{key_status}"
        if (
            "cyw43_driver_task_command_no_reply" in lower
            and f"stage={key_stage}" in lower
            and "reason=cyw43-runtime-command-no-reply" in lower
        ):
            if "op=11" in lower and "iovar=wsec_key" in lower:
                return f"{key_stage}-poll-timeout"
            return f"{key_stage}-tx-no-reply"
    if (
        "host-eapol-required" in lower
        or "wifi-host-eapol-required" in lower
        or "completion_rule=host-eapol-required" in lower
        or ("cyw43-host-eapol" in lower and "status=required" in lower)
    ):
        return "host-eapol-required"
    if "cyw43-host-eapol" in lower and (
        "status=pending" in lower or " pending" in lower
    ):
        return "wifi-host-eapol-pending"
    if (
        stripped == "cyw43-control-tx-no-reply"
        or stripped == "cyw43-control-tx-retry-no-reply"
        or "cyw43-control-txglomalign-tx-no-reply" in lower
        or "cyw43-control-txglomalign-tx-retry-no-reply" in lower
        or (
            "stage=cyw43-control-txglomalign" in lower
            and ("status=tx-no-reply" in lower or "status=tx-retry-no-reply" in lower)
        )
    ):
        if "retry" in lower or stripped == "cyw43-control-tx-retry-no-reply":
            return "cyw43-control-tx-retry-no-reply"
        return "cyw43-control-tx-no-reply"
    if stripped == "cyw43-control-tx-not-submitted":
        return "cyw43-control-tx-not-submitted"
    if (
        stripped == "cyw43-wifi"
        or "pi4-wifi-driver-task-runtime-required" in lower
        or "driver-task-net-runtime-unproved" in lower
        or "driver-task runtime is pending hardware service" in lower
        or "driver_task_bootstrap_deferred contract=cyw43455" in lower
        or "driver_task_bootstrap_deferred contract=sdio-host" in lower
        or "driver_task_resource_init contract=cyw43455" in lower
        or "driver_task_resource_init contract=sdio-host" in lower
    ):
        return "wifi-driver-task-runtime-unproved"
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
            and (
                "addr=0x0a800" in lower
                or "addr=0x0b800" in lower
                or "addr=0x02800" in lower
                or "addr=0x03800" in lower
            )
            and "val=0x01" in lower
        )
        or (
            "sdio-cmd52-write" in lower
            and "mode=cmd52-byte-transfer-window-reset-assert" in lower
            and has_armcr4_wrap_base(lower)
            and "off=0x800" in lower
        )
    ):
        return "armcr4-reset-assert-cmd52-r5-rejected"
    if (
        "armcr4-reset-assert-cmd53-r5-rejected" in lower
        or (
            "stage=assert-reset" in lower
            and has_armcr4_wrap_base(lower)
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or (
            has_armcr4_wrap_base(lower)
            and "off=0x800" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or "arg=0x91500004" in lower
        or "arg=0x91700004" in lower
        or "arg=0x95500004" in lower
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
    if "cyw43-kso-timeout-before-alp" in lower:
        return "cyw43-kso-timeout-before-alp"
    if "cyw43-rxglom-unsupported" in lower or "rx glom frame unsupported" in lower:
        return "cyw43-rxglom-unsupported"
    if (
        "armcr4-prereset-fgc-cmd53-r5-rejected" in lower
        or (
            "prereset-fgc-clock" in lower
            and ("sdio-cmd53-r5-error" in lower or "sdio cmd53 r5 fail" in lower)
        )
        or "arg=0x90481001" in lower
        or "arg=0x90681001" in lower
        or "arg=0x95481001" in lower
        or "arg=0x95481004" in lower
        or "arg=0x95681001" in lower
        or "arg=0x95681004" in lower
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
    if "function2-interrupt-unbound" in lower or "sel4-irq-unbound" in lower:
        return "function2-interrupt-unbound"
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
    if (
        "wsec-pmk" in lower
        or "set_wsec_pmk" in lower
        or "setwsecpmk" in lower
        or "ioctl 0x0000010c" in lower
        or "ioctl 0x10c" in lower
    ) and ("status=0xfffffffe" in lower or "badarg" in lower or "bad-argument" in lower):
        return "wsec-pmk-bad-argument"
    if "wifi-host-eapol-pending" in lower or "host-eapol-pending" in lower:
        return "wifi-host-eapol-pending"
    if (
        "host-eapol-required" in lower
        or "wifi-host-eapol-required" in lower
        or "completion_rule=host-eapol-required" in lower
        or ("cyw43-host-eapol" in lower and "status=required" in lower)
    ):
        return "host-eapol-required"
    if "cyw43-host-eapol" in lower and (
        "status=pending" in lower or " pending" in lower
    ):
        return "wifi-host-eapol-pending"
    if (
        "stage=runtime-rx" in lower
        and (
            "action=no-frame-source-after-firstread" in lower
            or "action=irq-latched-firstread-invalid" in lower
            or "action=irq-latched-firstread-empty" in lower
        )
        and "int_status=0x00000000" in lower
        and ("card_int=y" in lower or "sdhci=0x00000100" in lower)
    ):
        return "runtime-rx-host-latch-spam"
    if stripped == "eapol-start" or (
        "host-eapol" in lower and "action=eapol-start" in lower
    ):
        return "none"
    if (
        "sup_wpa" in lower
        or "firmware-supplicant" in lower
        or (
            ("ioctl 0x00000107" in lower or "ioctl 0x107" in lower)
            and "step=join" in lower
        )
    ) and (
        "status=0xffffffe9" in lower
        or "bcme_unsupported" in lower
        or "unsupported" in lower
    ):
        return "firmware-supplicant-unsupported"
    if "control-plane step=gmode" in lower and "action=fail" in lower:
        return "control-plane-legacy-gmode-stall"
    if "cyw43-control-revinfo-badarg" in lower or (
        "revinfo" in lower and ("badarg" in lower or "bad-argument" in lower)
    ):
        return "control-plane-revinfo-badarg"
    if "ioctl-timeout" in lower or "ioctl timeout" in lower:
        return "ioctl-timeout"
    if "cur-etheraddr-len" in lower:
        return "control-plane-cur-etheraddr-len"
    if "bdc-event" in lower:
        return "control-plane-bdc-event"
    if stripped == "interrupt-programming-drift":
        return "control-plane-interrupt-programming-drift"
    if stripped == "partial-hint-visibility":
        return "control-plane-partial-hint-visibility"
    if stripped == "interrupts-deferred":
        return "control-plane-interrupts-deferred"
    if stripped == "sideband-unreadable":
        return "control-plane-sideband-unreadable"
    if stripped == "rearm-timeout":
        return "control-plane-rearm-timeout"
    if stripped == "reply-idle-loop":
        return "control-plane-reply-idle-loop"
    if "cyw43-command-completion" in lower:
        return "control-plane-reply-idle-loop"
    if (
        stripped == "cyw43-control-tx-no-reply"
        or stripped == "cyw43-control-tx-retry-no-reply"
        or "cyw43-control-txglomalign-tx-no-reply" in lower
        or "cyw43-control-txglomalign-tx-retry-no-reply" in lower
        or (
            "stage=cyw43-control-txglomalign" in lower
            and ("status=tx-no-reply" in lower or "status=tx-retry-no-reply" in lower)
        )
    ):
        if "retry" in lower or stripped == "cyw43-control-tx-retry-no-reply":
            return "cyw43-control-tx-retry-no-reply"
        return "cyw43-control-tx-no-reply"
    if stripped == "cyw43-control-tx-not-submitted":
        return "cyw43-control-tx-not-submitted"
    if "host-card-int-no-dongle-source" in lower:
        return "control-plane-host-card-int-no-dongle-source"
    if "host-card-int-source-unreadable" in lower:
        return "control-plane-host-card-int-source-unreadable"
    if "no-frame-indication-after-write" in lower or "no-frame-source" in lower:
        return "control-plane-no-frame-indication-after-write"
    if "hintless-firstread-no-irq" in lower or "post-write-no-irq" in lower:
        return "control-plane-hintless-firstread-no-irq"
    if "control-plane-reply-idle-loop" in lower:
        return "control-plane-reply-idle-loop"
    if any(
        token in lower
        for token in (
            "cyw43-control-frame-unavailable",
            "cyw43-control-poll-unexpected-completion",
            "cyw43-control-reply-",
            "cyw43-control-rx-",
            "cyw43-control-split-",
            "cyw43-control-tx-",
        )
    ):
        return "control-plane-reply-idle-loop"
    if "control-plane" in lower:
        if "hintless-firstread-no-irq" in lower or "post-write-no-irq" in lower:
            return "control-plane-hintless-firstread-no-irq"
        if "host-card-int-no-dongle-source" in lower:
            return "control-plane-host-card-int-no-dongle-source"
        if "host-card-int-source-unreadable" in lower:
            return "control-plane-host-card-int-source-unreadable"
        if "no-frame-indication-after-write" in lower or "no-frame-source" in lower:
            return "control-plane-no-frame-indication-after-write"
        if "sideband" in lower and any(
            token in lower for token in ("unreadable", "timeout", "missing")
        ):
            return "control-plane-sideband-unreadable"
        if "interrupt-programming-drift" in lower:
            return "control-plane-interrupt-programming-drift"
        if "partial-hint-visibility" in lower:
            return "control-plane-partial-hint-visibility"
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
        "root-console-wait-for-wifi" in lower
        or ("root console wait" in lower and "wifi-not-ready" in lower)
        or "wifi-net-console-pending-before-root-console" in lower
        or ("wifi-not-ready" in lower and "wait-for-wifi" in lower)
    ):
        return "boot-waiting-for-wifi"
    if "wifi-net-console-deferred-until-root-console" in lower or (
        "action=serial-root-console-first" in lower
        and "pi4-local-seat-" in lower
        and "wifi" in lower
    ):
        return "boot-deferred-root-console"
    if lower.startswith("pi4-local-seat-") and lower.endswith("-wifi"):
        return "none"
    if (
        "local-seat-usb-first" in lower
        or "serial-local-seat-first" in lower
    ):
        return "boot-deferred-local-seat-usb"
    if (
        "join-timeout" in lower
        or ("join" in lower and "timeout" in lower)
        or "association-timeout" in lower
    ):
        return "join-timeout"
    if (
        "join-security-bsscfg-sup-wpa-loop" in lower
        or "iovar set failed name=bsscfg:sup_wpa" in lower
    ):
        return "join-security-bsscfg-sup-wpa-loop"
    if (
        "primary-bsscfg-wrapper-join-security-loop" in lower
        or ("iovar set failed name=bsscfg:" in lower and "step=join" in lower)
        or "iovar set failed name=bsscfg:wsec" in lower
    ):
        return "primary-bsscfg-wrapper-join-security-loop"
    if "join-security-wpaie-loop" in lower or "iovar set failed name=wpaie" in lower:
        return "join-security-wpaie-loop"
    if (
        "join-security-wpa-auth-initial-loop" in lower
        or "join-security-wpa-auth-first-loop" in lower
        or "join-security-wpa-auth-initial-no-reply" in lower
    ):
        return "join-security-wpa-auth-initial-loop"
    if (
        "join-security-wpa-auth-final-loop" in lower
        or "join-security-wpa-auth-final-no-reply" in lower
    ):
        return "join-security-wpa-auth-final-loop"
    if "join-security-auth-loop" in lower or "iovar set failed name=auth" in lower:
        return "join-security-auth-loop"
    if (
        "join-security-wsec-first-loop" in lower
        or "iovar set failed name=wsec" in lower
        or "iovar no-progress-after-frame name=wsec" in lower
    ):
        return "join-security-wsec-first-loop"
    if "join-security-sup-wpa-loop" in lower or "iovar set failed name=sup_wpa" in lower:
        return "join-security-sup-wpa-loop"
    if "join-programming-host-latch-loop" in lower:
        return "join-programming-host-latch-loop"
    if "join-pending" in lower or "association-pending" in lower:
        return "join-pending"
    if "wifi-association-failed" in lower or (
        "association" in lower and "failed" in lower
    ):
        return "wifi-association-failed"
    if "wifi-link-down" in lower:
        return "wifi-link-down"
    if "dhcp-pending" in lower:
        return "dhcp-pending"
    if "dhcp-not-started" in lower:
        return "dhcp-not-started"
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
    if lower in {"20739", "0x5103"} or "sdio-descriptor-transfer-failed" in lower:
        return "cyw43-sdio-descriptor-transfer-failed"
    if lower in {"21289", "0x5329"} or "firmware-retry-exhausted" in lower:
        return "cyw43-firmware-retry-exhausted"
    if lower in {"21290", "0x532a"} or "cyw43-post-release-ht-clock" in lower:
        return "cyw43-post-release-ht-clock"
    if lower in {"21291", "0x532b"} or "cyw43-post-release-function2-ready" in lower:
        return "cyw43-post-release-function2-ready"
    if lower in {"21292", "0x532c"} or "cyw43-post-release-corecontrol" in lower:
        return "cyw43-post-release-corecontrol"
    if lower in {"21293", "0x532d"} or "cyw43-post-release-mailbox-ready" in lower:
        return "cyw43-post-release-mailbox-ready"
    if lower in {"21294", "0x532e"} or "cyw43-post-release-protocol-version" in lower:
        return "cyw43-post-release-protocol-version"
    if lower in {"21295", "0x532f"} or "cyw43-post-release-rframe-sample" in lower:
        return "cyw43-post-release-rframe-sample"
    if lower in {"21296", "0x5330"} or "cyw43-post-release-dpc-activate" in lower:
        return "cyw43-post-release-dpc-activate"
    probe_attach_faults = {
        "21297": "cyw43-probe-cardctrl-read",
        "0x5331": "cyw43-probe-cardctrl-read",
        "21298": "cyw43-probe-cardctrl-write",
        "0x5332": "cyw43-probe-cardctrl-write",
        "21299": "cyw43-probe-pmucontrol-read",
        "0x5333": "cyw43-probe-pmucontrol-read",
        "21300": "cyw43-probe-pmucontrol-write",
        "0x5334": "cyw43-probe-pmucontrol-write",
        "21301": "cyw43-probe-function2-disable-read",
        "0x5335": "cyw43-probe-function2-disable-read",
        "21302": "cyw43-probe-function2-disable-write",
        "0x5336": "cyw43-probe-function2-disable-write",
        "21303": "cyw43-probe-sdonly-clock",
        "0x5337": "cyw43-probe-sdonly-clock",
        "0x5338": "cyw43-release-intstatus-clear",
    }
    if lower in probe_attach_faults:
        return probe_attach_faults[lower]
    for fault in set(probe_attach_faults.values()):
        if fault in lower:
            return fault
    card_command_faults = {
        "21284": "cyw43-transport-card-cmd0",
        "0x5324": "cyw43-transport-card-cmd0",
        "21285": "cyw43-transport-card-cmd5-ocr",
        "0x5325": "cyw43-transport-card-cmd5-ocr",
        "21286": "cyw43-transport-card-cmd5-ready",
        "0x5326": "cyw43-transport-card-cmd5-ready",
        "21287": "cyw43-transport-card-cmd3-rca",
        "0x5327": "cyw43-transport-card-cmd3-rca",
        "21288": "cyw43-transport-card-cmd7-select",
        "0x5328": "cyw43-transport-card-cmd7-select",
    }
    if lower in card_command_faults:
        return card_command_faults[lower]
    for fault in set(card_command_faults.values()):
        if fault in lower:
            return fault
    if lower in {"20737", "0x5101"} or "sdio-command-unavailable" in lower:
        return "sdio-command-unavailable"
    if "function2-disabled" in lower:
        return "function2-disabled"
    if "firmware-verify-mismatch" in lower:
        return "firmware-verify-mismatch"
    if "firmware" in lower:
        return "firmware-load"
    return value


def cyw43_raw_engine_init_progress_blocker(event: TraceEvent) -> str | None:
    """Return CYW43 engine-init subgate carried by raw ring-progress telemetry."""

    raw = event.raw.lower()
    fields = event.fields
    if "driver_task_ring_progress" not in raw:
        return None
    if fields.get("contract", "").lower() != "cyw43455":
        return None
    if fields.get("marker_valid", "").lower() not in {"yes", "true", "1"}:
        return None
    aux0 = (
        fields.get("marker_aux0")
        or fields.get("expected_aux0")
        or fields.get("aux0")
        or ""
    ).lower()
    if aux0 != "0x494e4954":
        return None
    phase_name = fields.get("marker_phase_name", "").lower()
    return {
        "engine-init-runtime-entry": "cyw43-engine-init-runtime-entry-no-reply",
        "cyw43-engine-init-branch": "cyw43-engine-init-state-slot-no-reply",
        "cyw43-state-reset-begin": "cyw43-state-reset-no-reply",
        "cyw43-state-reset-done": "cyw43-forbidden-sdio-mmio-check-no-reply",
        "cyw43-forbidden-sdio-mmio": "cyw43-resource-forbidden-sdio-mmio",
        "cyw43-bus-link-check-begin": (
            "cyw43-resource-sdio-owner-bus-link-check-no-reply"
        ),
        "cyw43-shared-control-check-begin": (
            "cyw43-resource-shared-control-check-no-reply"
        ),
        "cyw43-shared-control-missing": "cyw43-resource-shared-control-missing",
        "cyw43-shared-control-ready": (
            "cyw43-engine-init-completion-publish-no-reply"
        ),
    }.get(phase_name)


def cyw43_raw_command_progress_blocker(event: TraceEvent) -> str | None:
    """Return post-firmware CYW43 command subgate from raw progress telemetry."""

    raw = event.raw.lower()
    fields = event.fields
    if "driver_task_ring_progress" not in raw:
        return None
    if fields.get("contract", "").lower() != "cyw43455":
        return None
    if fields.get("marker_valid", "").lower() not in {"yes", "true", "1"}:
        return None
    aux0 = (
        fields.get("marker_aux0")
        or fields.get("expected_aux0")
        or fields.get("aux0")
        or ""
    ).lower()
    if aux0 != "0x43595734":
        return None
    phase_name = fields.get("marker_phase_name", "").lower()
    return {
        "cyw43-backplane-alp-request": "cyw43-backplane-alp-request-no-reply",
        "cyw43-backplane-alp-poll": "cyw43-backplane-alp-poll-no-reply",
        "cyw43-backplane-force-alp": "cyw43-backplane-force-alp-no-reply",
        "cyw43-backplane-force-alp-settle": (
            "cyw43-backplane-force-alp-settle-no-reply"
        ),
        "cyw43-backplane-pullup-clear": (
            "cyw43-backplane-pullup-clear-no-reply"
        ),
        "cyw43-backplane-pullup-skipped": (
            "cyw43-backplane-pullup-skipped-no-reply"
        ),
        "cyw43-backplane-pullup-fault-contained": (
            "cyw43-backplane-pullup-fault-contained"
        ),
        "cyw43-backplane-window-low": "cyw43-backplane-window-low-no-reply",
        "cyw43-backplane-window-mid": "cyw43-backplane-window-mid-no-reply",
        "cyw43-backplane-window-high": "cyw43-backplane-window-high-no-reply",
        "cyw43-backplane-chipcommon-read": (
            "cyw43-backplane-chipcommon-read-no-reply"
        ),
        "cyw43-release-begin": "cyw43-release-begin-no-reply",
        "cyw43-release-reset-vector-begin": (
            "cyw43-release-reset-vector-no-reply"
        ),
        "cyw43-release-armcr4-reset-begin": (
            "cyw43-release-armcr4-reset-no-reply"
        ),
        "cyw43-release-upload-clock-begin": (
            "cyw43-release-upload-clock-no-reply"
        ),
        "cyw43-release-post-config-begin": (
            "cyw43-release-post-config-no-reply"
        ),
        "cyw43-release-ht-clock-begin": "cyw43-release-ht-clock-no-reply",
        "cyw43-release-f2-enable-begin": "cyw43-release-f2-enable-no-reply",
        "cyw43-release-int-mask-begin": "cyw43-release-int-mask-no-reply",
        "cyw43-release-corecontrol-begin": (
            "cyw43-release-corecontrol-no-reply"
        ),
        "cyw43-release-mailbox-version-begin": (
            "cyw43-release-mailbox-version-no-reply"
        ),
        "cyw43-release-firmware-ready-begin": (
            "cyw43-release-firmware-ready-no-reply"
        ),
        "cyw43-release-firmware-ready-done": (
            "cyw43-release-firmware-ready-done-no-reply"
        ),
    }.get(phase_name)


def normalize_wifi_exact(value: str) -> str:
    """Preserve exact CYW43 terminal reasons while keeping stable blockers."""

    lower = value.lower()
    if lower.strip() in {
        "",
        "0",
        "0x0",
        "0x00",
        "0x0000",
        "0x00000000",
        "none",
        "n/a",
        "net-ready",
    }:
        return "none"
    if (
        "stale-admission-exhausted" in lower
        and ("engine-init" in lower or "net-engine-init" in lower)
    ) or lower.strip() == "stale-admission-exhausted":
        return "cyw43-engine-init-stale-admission-exhausted"
    if "host-eapol-m3-missing" in lower:
        return "host-eapol-m3-missing"
    cyw43_transport_details = {
        "1": "cyw43-runtime-command-rejected",
        "0x1": "cyw43-runtime-command-rejected",
        "0x0001": "cyw43-runtime-command-rejected",
        "21249": "cyw43-transport-init",
        "0x5301": "cyw43-transport-init",
        "21264": "cyw43-transport-bus-link-missing",
        "0x5310": "cyw43-transport-bus-link-missing",
        "21266": "cyw43-transport-card-init",
        "0x5312": "cyw43-transport-card-init",
        "21267": "cyw43-transport-f1-block-size",
        "0x5313": "cyw43-transport-f1-block-size",
        "21268": "cyw43-transport-f2-block-size",
        "0x5314": "cyw43-transport-f2-block-size",
        "21269": "cyw43-transport-f1-enable",
        "0x5315": "cyw43-transport-f1-enable",
        "21270": "cyw43-transport-card-bus-width",
        "0x5316": "cyw43-transport-card-bus-width",
        "21271": "cyw43-transport-host-bus-width",
        "0x5317": "cyw43-transport-host-bus-width",
        "21272": "cyw43-transport-backplane",
        "0x5318": "cyw43-transport-backplane",
        "21273": "cyw43-transport-high-speed",
        "0x5319": "cyw43-transport-high-speed",
        "21274": "cyw43-backplane-alp",
        "0x531a": "cyw43-backplane-alp",
        "21275": "cyw43-backplane-wake",
        "0x531b": "cyw43-backplane-wake",
        "21276": "cyw43-backplane-kso",
        "0x531c": "cyw43-backplane-kso",
        "21277": "cyw43-backplane-watermark",
        "0x531d": "cyw43-backplane-watermark",
        "21278": "cyw43-backplane-device-control",
        "0x531e": "cyw43-backplane-device-control",
        "21279": "cyw43-backplane-armcr4-reset",
        "0x531f": "cyw43-backplane-armcr4-reset",
        "21280": "cyw43-firmware-range",
        "0x5320": "cyw43-firmware-range",
        "21256": "cyw43-firmware-prep",
        "0x5308": "cyw43-firmware-prep",
        "21257": "cyw43-descriptor-unavailable",
        "0x5309": "cyw43-descriptor-unavailable",
        "21258": "cyw43-descriptor-invalid",
        "0x530a": "cyw43-descriptor-invalid",
        "21281": "cyw43-backplane-window",
        "0x5321": "cyw43-backplane-window",
        "21282": "cyw43-post-release-cardcap",
        "0x5322": "cyw43-post-release-cardcap",
        "21283": "cyw43-backplane-chipcommon-read",
        "0x5323": "cyw43-backplane-chipcommon-read",
        "21284": "cyw43-transport-card-cmd0",
        "0x5324": "cyw43-transport-card-cmd0",
        "21285": "cyw43-transport-card-cmd5-ocr",
        "0x5325": "cyw43-transport-card-cmd5-ocr",
        "21286": "cyw43-transport-card-cmd5-ready",
        "0x5326": "cyw43-transport-card-cmd5-ready",
        "21287": "cyw43-transport-card-cmd3-rca",
        "0x5327": "cyw43-transport-card-cmd3-rca",
        "21288": "cyw43-transport-card-cmd7-select",
        "0x5328": "cyw43-transport-card-cmd7-select",
        "20737": "sdio-command-unavailable",
        "0x5101": "sdio-command-unavailable",
        "20738": "cyw43-sdio-descriptor-unavailable",
        "0x5102": "cyw43-sdio-descriptor-unavailable",
        "20739": "cyw43-sdio-descriptor-transfer-failed",
        "0x5103": "cyw43-sdio-descriptor-transfer-failed",
        "21289": "cyw43-firmware-retry-exhausted",
        "0x5329": "cyw43-firmware-retry-exhausted",
        "21290": "cyw43-post-release-ht-clock",
        "0x532a": "cyw43-post-release-ht-clock",
        "21291": "cyw43-post-release-function2-ready",
        "0x532b": "cyw43-post-release-function2-ready",
        "21292": "cyw43-post-release-corecontrol",
        "0x532c": "cyw43-post-release-corecontrol",
        "cyw43-transport-command-admission": (
            "cyw43-transport-command-admission"
        ),
        "21293": "cyw43-post-release-mailbox-ready",
        "0x532d": "cyw43-post-release-mailbox-ready",
        "21294": "cyw43-post-release-protocol-version",
        "0x532e": "cyw43-post-release-protocol-version",
        "21295": "cyw43-post-release-rframe-sample",
        "0x532f": "cyw43-post-release-rframe-sample",
        "21296": "cyw43-post-release-dpc-activate",
        "0x5330": "cyw43-post-release-dpc-activate",
        "21297": "cyw43-probe-cardctrl-read",
        "0x5331": "cyw43-probe-cardctrl-read",
        "21298": "cyw43-probe-cardctrl-write",
        "0x5332": "cyw43-probe-cardctrl-write",
        "21299": "cyw43-probe-pmucontrol-read",
        "0x5333": "cyw43-probe-pmucontrol-read",
        "21300": "cyw43-probe-pmucontrol-write",
        "0x5334": "cyw43-probe-pmucontrol-write",
        "21301": "cyw43-probe-function2-disable-read",
        "0x5335": "cyw43-probe-function2-disable-read",
        "21302": "cyw43-probe-function2-disable-write",
        "0x5336": "cyw43-probe-function2-disable-write",
        "21303": "cyw43-probe-sdonly-clock",
        "0x5337": "cyw43-probe-sdonly-clock",
        "0x5338": "cyw43-release-intstatus-clear",
    }
    if lower in cyw43_transport_details:
        return cyw43_transport_details[lower]
    control_plane_exact = CYW43_CONTROL_PLANE_EXACT_RE.search(lower)
    if control_plane_exact is not None:
        return control_plane_exact.group(0)
    for reason in (
        "cyw43-engine-init-runtime-entry-no-reply",
        "cyw43-engine-init-state-slot-no-reply",
        "cyw43-ht-clock-timeout-before-function2",
        "cyw43-device-on-timeout-before-ht",
        "cyw43-device-on-timeout-before-function2",
        "cyw43-control-plane-bdc-event",
        "cyw43-control-plane-hintless-firstread-no-irq",
        "cyw43-control-plane-host-card-int-no-dongle-source",
        "cyw43-control-plane-host-card-int-source-unreadable",
        "cyw43-control-plane-interrupt-programming-drift",
        "cyw43-join-programming-host-latch-loop",
        "cyw43-primary-bsscfg-wrapper-join-security-loop",
        "cyw43-join-security-wpaie-loop",
        "cyw43-join-security-wpa-auth-initial-loop",
        "cyw43-join-security-wpa-auth-final-loop",
        "cyw43-join-security-auth-loop",
        "cyw43-join-security-wsec-first-loop",
        "cyw43-join-security-sup-wpa-loop",
        "cyw43-join-security-bsscfg-sup-wpa-loop",
        "cyw43-control-plane-legacy-gmode-stall",
        "cyw43-control-plane-no-frame-indication-after-write",
        "cyw43-control-plane-partial-hint-visibility",
        "cyw43-control-revinfo-badarg",
        "cyw43-control-frame-unavailable",
        "cyw43-control-poll-unexpected-completion",
        "cyw43-control-reply-malformed",
        "cyw43-control-rframe-count-read-failed",
        "cyw43-control-reply-nonmatching",
        "cyw43-control-reply-status",
        "cyw43-control-rx-f2-read-failed",
        "cyw43-control-rx-firstread-empty",
        "cyw43-control-rx-firstread-failed",
        "cyw43-control-rx-firstread-invalid-sdpcm",
        "cyw43-control-rx-firstread-remainder-failed",
        "cyw43-control-rx-firstread-remainder-too-large",
        "cyw43-control-rx-retransmit-ack-timeout",
        "cyw43-control-rx-invalid-rframe-len",
        "cyw43-control-rx-no-rframe",
        "cyw43-control-rx-not-ready",
        "cyw43-control-rx-request-too-large",
        "cyw43-control-rx-sdpcm-decode-miss",
        "cyw43-control-split-timeout",
        "cyw43-control-txglomalign-badarg",
        "cyw43-control-txglomalign-unsupported",
        "cyw43-control-tx-no-reply",
        "cyw43-control-tx-retry-no-reply",
        "cyw43-control-tx-not-submitted",
        "host-eapol-ptk-install",
        "host-eapol-gtk-install",
        "cyw43-runtime-command-no-reply",
        "cyw43-data-rx-f2-read-failed",
        "cyw43-data-rx-firstread-empty",
        "cyw43-data-rx-firstread-failed",
        "cyw43-data-rx-firstread-invalid-sdpcm",
        "cyw43-data-rx-firstread-remainder-failed",
        "cyw43-data-rx-firstread-remainder-too-large",
        "cyw43-data-rx-retransmit-ack-timeout",
        "cyw43-data-rx-sdpcm-decode-miss",
        "cyw43-host-eapol-bssid-probe-tx-submit-fail",
        "cyw43-post-release-ht-clock",
        "cyw43-post-release-function2-ready",
        "cyw43-post-release-corecontrol",
        "cyw43-post-release-mailbox-ready",
        "cyw43-post-release-protocol-version",
        "cyw43-post-release-rframe-sample",
        "cyw43-post-release-dpc-activate",
        "cyw43-probe-cardctrl-read",
        "cyw43-probe-cardctrl-write",
        "cyw43-probe-pmucontrol-read",
        "cyw43-probe-pmucontrol-write",
        "cyw43-probe-function2-disable-read",
        "cyw43-probe-function2-disable-write",
        "cyw43-probe-sdonly-clock",
        "cyw43-protocol-error-cur-etheraddr-len",
        "wsec-pmk-bad-argument",
        "firmware-supplicant-unsupported",
        "wifi-host-eapol-pending",
        "host-eapol-required",
        "cyw43-function2-interrupt-unbound",
        "wifi-driver-task-runtime-unproved",
    ):
        if reason in lower:
            return reason
    if "bdc-event" in lower:
        return "cyw43-control-plane-bdc-event"
    if "cur-etheraddr-len" in lower:
        return "cyw43-protocol-error-cur-etheraddr-len"
    return normalize_wifi_blocker(value)


def wifi_progress_gate(value: str | None) -> int | None:
    """Return the WiFi gate proved by a progress label, if any."""

    if value is None:
        return None
    label = value.lower().strip().replace("_", "-")
    return WIFI_PROGRESS_GATES.get(label)


def wifi_firmware_stream_fault_blocker(event: TraceEvent) -> str | None:
    """Return the firmware-upload blocker carried by a CYW43 stream fault."""

    fields = event.fields
    stage = (
        fields.get("stage")
        or fields.get("name")
        or event.stage
        or ""
    ).lower()
    op = fields.get("op", "").lower()
    if stage not in {
        "firmware-upload",
        "cyw43-firmware-prep",
        "cyw43-firmware-chunk",
        "cyw43-nvram-chunk",
    } and op not in {"1", "2", "3"}:
        return None
    status = fields.get("status", "").lower()
    reason_exact = normalize_wifi_exact(fields.get("reason", ""))
    if reason_exact in {"none", "unknown", "cyw43-runtime-command-rejected"}:
        detail = fields.get("detail") or fields.get("fault_detail") or fields.get("reason") or ""
    else:
        detail = fields.get("reason", "")
    if status in {"blocked", "inferred", "pass", "ready"}:
        return None
    if status not in {"fail", "failed", "fault"} and not detail:
        return None
    exact = normalize_wifi_exact(detail) if detail else "none"
    return exact if exact != "none" else None


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
    linked_first_report_seen = any(usb_first_report_step(event) for event in event_list)
    linked_first_byte_seen = any(usb_first_byte_step(event) for event in event_list)
    linked_command_ready_seen = any(usb_command_ready_step(event) for event in event_list)
    usb_driver_progress_seen = any(
        "driver_task_ring_progress" in event.raw.lower()
        and usb_raw_driver_task_progress_blocker(event.fields) is not None
        for event in event_list
    )
    usb_resource_progress_seen = (
        linked_first_report_seen or linked_first_byte_seen or linked_command_ready_seen
    )
    if not usb_events and not usb_driver_progress_seen and not usb_resource_progress_seen:
        return 0, "missing"

    gate = 1
    blocker = "unknown"
    saw_command_submit = False
    saw_command_doorbell = False
    saw_command_doorbell_hal_flush = False
    saw_command_doorbell_write_pending = False
    saw_command_event_ring_before = False
    saw_command_timeout_plan = False
    saw_keyboard_preseed = False
    saw_keyboard_runtime_init = False
    saw_net_init_before_keyboard = False
    root_port_read_pending = False
    brcm_axi_read_pending = False
    reset_pre_usbcmd_pending = False
    run_posted_flush_pending = False
    command_probe_bus: str | None = None
    command_probe_verb: str | None = None
    command_timeout_detail: str | None = None
    command_recovery_source: str | None = None
    command_timeout_expected_ptr: int | None = None
    command_timeout_crcr_plan: int | None = None
    reset_init_blocker: str | None = None
    pcie_owner_blocker: str | None = None
    startup_fail_gate: int | None = None
    startup_fail_blocker: str | None = None
    startup_diag_blocker: str | None = None
    direct_usb_progress_blocker: str | None = None
    run_usbcmd_preserved_reset_bit = False
    usbcmd_controller_command_bits = 0x0000_0382
    linked_runtime_gate10_seen = False
    usb_idle_no_key_byte_seen = False
    precise_command_timeout_details = {
        "cmd-poll-only-timeout",
        "pcie-window-enable-slot-timeout",
        "pcie-window-no-op-timeout",
        "raw-phys-cmd-poll-only-timeout",
        "cmd-fetch-timeout",
        "cmd-event-ring-timeout",
        "cmd-controller-not-running",
        "cmd-controller-not-ready",
        "reset-controller-not-halted",
        "reset-pre-hcrst-controller-not-ready",
        "reset-controller-not-ready",
        "cmd-controller-halted",
        "usbcmd-run-preserved-reset-bit",
        "usbcmd-run-posted-flush-halt",
        "cmd-timeout",
        "cmd-poll-pending",
        "cmd-doorbell-write-halt",
        "cmd-submit-proof-timer-preempted",
        "cmd-stale-crcr-dequeue",
    }
    stale_command_timeout_details = precise_command_timeout_details - {"cmd-poll-pending"}
    for event in event_list:
        raw = event.raw.lower()
        if event.message.startswith("xhci_recent"):
            continue
        fields = event.fields
        raw_usb_blocker = normalize_usb_blocker(raw)
        if raw_usb_blocker == "post-prompt-attach-retry-exhausted":
            gate = max(gate, usb_driver_task_blocker_gate(raw_usb_blocker))
            blocker = raw_usb_blocker
            continue
        tag = fields.get("tag", "")
        if "pi4 keyboard preseed end" in raw:
            saw_keyboard_preseed = True
        if "pi4 keyboard runtime init begin" in raw:
            saw_keyboard_runtime_init = True
        if (
            not saw_keyboard_runtime_init
            and (event.domain == "wifi" or "[net-console] init: bringing up" in raw)
        ):
            saw_net_init_before_keyboard = True
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
                blocker = "root-port-read-timer-preempted"
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
        if raw.startswith("driver_task_ring_progress"):
            progress_blocker = usb_raw_driver_task_progress_blocker(fields)
            if progress_blocker is not None:
                blocker_gate = usb_driver_task_blocker_gate(progress_blocker)
                if blocker_gate > 1:
                    gate = max(gate, blocker_gate)
                    if blocker_gate >= gate or blocker in {"unknown", "none"}:
                        blocker = progress_blocker
                    direct_usb_progress_blocker = progress_blocker
                    continue
        if usb_command_ready_step(event):
            gate = max(gate, 10)
            linked_runtime_gate10_seen = True
            blocker = "none"
            continue
        if usb_first_byte_step(event):
            gate = max(gate, 9)
            if blocker in {
                "unknown",
                "none",
                "keyboard-first-byte",
                "first-console-byte",
                "hid-first-report",
            }:
                blocker = "command-input-ready"
            continue
        if usb_first_report_step(event):
            gate = max(gate, 9)
            if blocker in {"unknown", "none", "hid-report-event", "hid-first-report"}:
                blocker = "none"
            continue
        if event.domain != "usb":
            continue
        if raw.startswith(
            ("usb: runtime_queue", "usb: stall_telemetry", "usb: sustained_input")
        ) and usb_hid_interrupt_no_completion_seen(fields):
            gate = max(gate, 8)
            if blocker in {"unknown", "none", "hid-first-report"}:
                blocker = "usb-hid-interrupt-no-completion"
            continue
        if "map exact miss" in raw and fields.get("reason") == "no-device-coverage":
            gate = max(gate, 3)
            if (
                "xhci" in raw
                or "vl805" in raw
                or "pcie-root-cfg" in raw
                or "pi4-pcie-root-cfg" in raw
            ):
                blocker = "pcie-xhci-device-coverage-missing"
            elif blocker in {"unknown", "none", "unavailable"}:
                blocker = "device-coverage-missing"
            continue
        if "vl805 posted-write flush" in raw:
            run_posted_flush_pending = False
            if fields.get("reason", "").lower() == "pcie-owner-ring-unavailable":
                gate = max(gate, 3)
                pcie_owner_blocker = "pcie-owner-ring-unavailable"
                blocker = pcie_owner_blocker
                continue
            if (
                parse_hex_int(fields.get("stage")) == 0x031F
                and fields.get("role", "").lower() == "command-doorbell"
                and fields.get("source", "").lower() == "hal-ext-cfg"
            ):
                saw_command_doorbell_hal_flush = True
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
        if "root-port sample begin" in raw:
            root_port_read_pending = True
            gate = max(gate, 3)
            blocker = "root-port-read-begin"
            continue
        if "root-port read-begin" in raw:
            root_port_read_pending = True
            gate = max(gate, 3)
            blocker = "root-port-read-begin"
            continue
        if "root-port read-done" in raw or "root-port sample-done" in raw:
            root_port_read_pending = False
            continue
        explicit_proof_gate = parse_hex_int(fields.get("proof_gate"))
        if explicit_proof_gate is not None and explicit_proof_gate > 0:
            if (
                raw.startswith("usb: runtime_gate")
                and explicit_proof_gate >= 10
                and not linked_command_ready_seen
            ):
                explicit_proof_gate = 9
            gate = max(gate, explicit_proof_gate)
        diag_gate = startup_diag_gate(raw, "usb")
        if diag_gate is not None:
            status = fields.get("status", "").lower()
            name = fields.get("name", "usb-startup-gate")
            if status == "pass":
                gate = max(gate, diag_gate)
                if diag_gate >= 10:
                    blocker = "none"
                if startup_fail_gate is not None and diag_gate >= startup_fail_gate:
                    startup_fail_gate = None
                    startup_fail_blocker = None
            elif status == "fail":
                failed_gate = max(0, diag_gate - 1)
                direct_gate = usb_driver_task_blocker_gate(
                    direct_usb_progress_blocker or ""
                )
                if direct_usb_progress_blocker is not None and direct_gate >= failed_gate:
                    gate = max(gate, direct_gate)
                    blocker = direct_usb_progress_blocker
                else:
                    gate = max(gate, failed_gate)
                    blocker = normalize_usb_blocker(name)
                    if blocker == "none":
                        blocker = name
                if startup_fail_gate is None or diag_gate < startup_fail_gate:
                    startup_fail_gate = diag_gate
                    startup_fail_blocker = blocker
            continue
        if raw.startswith("usb: runtime_queue"):
            queued_reports = parse_hex_int(fields.get("queued_reports")) or 0
            if (
                field_lower(event, "queue_valid") == "yes"
                and queued_reports > 0
                and field_lower(event, "doorbell_pending") in {"no", "false"}
                and field_lower(event, "report_status") == "idle-report"
            ):
                usb_idle_no_key_byte_seen = True
            continue
        if raw.startswith("usb: acceptance") and field_lower(
            event, "input_observation"
        ) == "idle-report-no-key-byte":
            usb_idle_no_key_byte_seen = True
            continue
        if raw.startswith("usb: runtime_gate"):
            proof_gate = parse_hex_int(fields.get("proof_gate"))
            if proof_gate is not None and proof_gate > 0:
                waiting_for_command_ready = proof_gate >= 10 and not linked_command_ready_seen
                if waiting_for_command_ready:
                    proof_gate = 9
                gate = max(gate, proof_gate)
                runtime_blocker = normalize_usb_blocker(fields.get("blocker", "none"))
                if runtime_blocker == "none" and waiting_for_command_ready:
                    blocker = "command-input-ready"
                elif runtime_blocker == "none" and proof_gate >= 10:
                    linked_runtime_gate10_seen = True
                    blocker = "none"
                elif runtime_blocker == "none":
                    blocker = "none"
                elif runtime_blocker in {
                    "keyboard-first-byte",
                    "first-console-byte",
                    "awaiting-physical-key",
                } and field_lower(event, "input_observation") == "idle-report-no-key-byte":
                    usb_idle_no_key_byte_seen = True
                    blocker = "command-input-ready"
                else:
                    blocker = runtime_blocker
            continue
        if "linked_runtime_progress" in raw:
            progress_gate = parse_hex_int(fields.get("gate"))
            if progress_gate is not None and progress_gate > 0:
                gate = max(gate, progress_gate)
            progress_blocker = normalize_usb_blocker(fields.get("blocker", "none"))
            blocker_gate = usb_driver_task_blocker_gate(progress_blocker)
            if blocker_gate > 1:
                gate = max(gate, blocker_gate)
                if blocker_gate >= gate or blocker in {"unknown", "none"}:
                    blocker = progress_blocker
                direct_usb_progress_blocker = progress_blocker
            continue
        if raw.startswith("driver_task_ring_progress"):
            progress_blocker = usb_raw_driver_task_progress_blocker(fields)
            if progress_blocker is not None:
                blocker_gate = usb_driver_task_blocker_gate(progress_blocker)
                if blocker_gate > 1:
                    gate = max(gate, blocker_gate)
                    if blocker_gate >= gate or blocker in {"unknown", "none"}:
                        blocker = progress_blocker
                    direct_usb_progress_blocker = progress_blocker
                continue
        if raw.startswith("usb: next_action"):
            next_blocker = normalize_usb_blocker(fields.get("blocker", "none"))
            next_gate = usb_driver_task_blocker_gate(next_blocker)
            if next_gate > 1:
                startup_diag_blocker = next_blocker
                if (
                    startup_fail_gate is not None
                    and startup_fail_blocker == next_blocker
                ):
                    gate = max(gate, max(0, startup_fail_gate - 1))
                elif usb_driver_task_blocker_caps_gate(next_blocker):
                    gate = next_gate if gate <= 0 else min(gate, next_gate)
                else:
                    gate = max(gate, next_gate)
                blocker = next_blocker
            continue
        if raw.startswith("usb_runtime_enum_snapshot"):
            detail_gate = usb_runtime_detail_gate_blocker(
                parse_hex_int(fields.get("detail"))
            )
            if detail_gate is not None:
                detail_proof_gate, detail_blocker = detail_gate
                gate = max(gate, detail_proof_gate)
                if detail_blocker != "none":
                    blocker = detail_blocker
                elif detail_proof_gate >= 8:
                    blocker = "none"
            continue
        if "usb proof_summary" in raw:
            proof_gate = parse_hex_int(fields.get("gate"))
            proof_command_valid = usb_command_probe_success(
                fields.get("command", ""),
                fields.get("event_generation"),
                fields.get("cleanup_generation"),
                fields.get("recovery_source") or command_recovery_source,
            )
            if proof_gate == 4 and not proof_command_valid:
                gate = max(gate, 3)
                blocker = "cmd-event-ring-timeout"
                continue
            if proof_gate is not None and proof_gate > 0:
                gate = max(gate, proof_gate)
            proof_blocker = normalize_usb_blocker(fields.get("blocker", "none"))
            if proof_blocker != "none":
                if (
                    proof_blocker == "cmd-poll-pending"
                    and fields.get("command")
                    in {
                        "no-op-unproven",
                        "enable-slot-timeout",
                        "enable-slot-unproven",
                        "enable-slot-linux-event-unproven",
                    }
                ):
                    proof_blocker = "cmd-event-ring-timeout"
                elif proof_blocker == "cmd-poll-pending":
                    gate = max(gate, 4)
                if (
                    proof_blocker == "cmd-event-ring-timeout"
                    and command_timeout_detail in stale_command_timeout_details
                ):
                    blocker = command_timeout_detail
                else:
                    blocker = proof_blocker
            elif fields.get("command") in {
                "no-op-unproven",
                "enable-slot-timeout",
                "enable-slot-unproven",
                "enable-slot-linux-event-unproven",
            }:
                gate = max(gate, 3)
                blocker = "cmd-event-ring-timeout"
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
            gate = max(gate, 3)
            blocker = "captured-root-port-enum"
            continue
        if "usb root-enum deferred-port" in raw:
            gate = max(gate, 3)
            blocker = "captured-root-port-enum"
            continue
        if "command-probe begin" in raw and fields.get("bus") in {
            "pcie-window",
            "phys",
        }:
            command_probe_bus = fields["bus"]
            command_probe_verb = fields.get("verb")
        if (
            "command-probe" in raw
            and fields.get("action") == "recover-polling-event-generation"
            and fields.get("recovery_event_generation")
            == "uboot-timeout-polling-fresh-recovery"
        ):
            command_recovery_source = fields.get("result")
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
        if usb_first_report_step(event):
            gate = max(gate, 9)
            if blocker in {"unknown", "none", "hid-report-event", "hid-first-report"}:
                blocker = "none"
            continue
        if tag == "usb-hid-report-event":
            if usb_linked_hid_source(event):
                gate = max(gate, 9)
                if blocker in {"unknown", "none", "hid-report-event"}:
                    blocker = "none"
            else:
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
        if "usb hid first report" in raw and usb_linked_hid_source(event):
            gate = max(gate, 9)
            blocker = "none"
        elif "usb hid first report" in raw and blocker in {"unknown", "none"}:
            blocker = "hid-first-report"
        if "runtime keyboard first-byte" in raw and usb_linked_hid_source(event):
            gate = max(gate, 9)
            if blocker in {"unknown", "none", "keyboard-first-byte", "first-console-byte"}:
                blocker = "command-input-ready"
        elif "runtime keyboard first-byte" in raw and blocker in {"unknown", "none"}:
            blocker = "keyboard-first-byte"
        if "pi4 keyboard runtime proof" in raw:
            proof_gate = parse_hex_int(fields.get("gate"))
            linked_hid = usb_linked_hid_source(event)
            if proof_gate is not None and proof_gate >= 10 and not linked_hid:
                proof_gate = 9
            if proof_gate is not None and proof_gate > 0:
                gate = max(gate, proof_gate)
            if proof_gate is not None and proof_gate >= 10 and linked_hid:
                linked_runtime_gate10_seen = True
            proof_result = normalize_usb_blocker(fields.get("result", "none"))
            if proof_result == "none" and (proof_gate is None or proof_gate < 10 or linked_hid):
                blocker = "none"
            elif proof_result == "none":
                blocker = "keyboard-first-byte"
            elif proof_result == "unavailable" and blocker not in {"unknown", "none"}:
                pass
            elif (
                proof_result == "keyboard-unavailable"
                and usb_driver_task_blocker_gate(blocker) >= 4
            ):
                pass
            else:
                blocker = proof_result
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
            command_timeout_expected_ptr = None
            command_timeout_crcr_plan = None
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
                    if command_probe_verb and "enable-slot" in command_probe_verb:
                        command_timeout_detail = "pcie-window-enable-slot-timeout"
                    else:
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
            if command_timeout_detail not in {
                "usbcmd-run-preserved-reset-bit",
                "cmd-stale-crcr-dequeue",
            }:
                command_timeout_detail = "cmd-event-ring-timeout"
            blocker = command_timeout_detail
        elif tag.startswith("cmd-gate-timeout-plan"):
            gate = max(gate, 3)
            saw_command_timeout_plan = True
            if tag == "cmd-gate-timeout-plan-0":
                command_timeout_expected_ptr = parse_hex_int(fields.get("expected_ptr"))
                command_timeout_crcr_plan = None
            elif tag == "cmd-gate-timeout-plan-1":
                command_timeout_crcr_plan = parse_hex_int(fields.get("crcr_plan"))
                if (
                    command_timeout_expected_ptr is not None
                    and command_timeout_crcr_plan is not None
                    and command_timeout_expected_ptr != (command_timeout_crcr_plan & ~0xF)
                ):
                    command_timeout_detail = "cmd-stale-crcr-dequeue"
                    blocker = command_timeout_detail
                    continue
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
                elif (usbsts & 0x800) != 0:
                    command_timeout_detail = "cmd-controller-not-ready"
                    blocker = command_timeout_detail
                elif (usbsts & 0x1) != 0:
                    command_timeout_detail = "cmd-controller-halted"
                    blocker = command_timeout_detail
        elif tag in {"reset-hcrst-timeout", "cmd-recovery-hcrst-timeout"}:
            gate = max(gate, 2)
            reset_init_blocker = "reset-hcrst-timeout"
            blocker = reset_init_blocker
        elif tag in {
            "halt-revalidation-timeout",
            "stop-revalidation-timeout",
            "cmd-recovery-stop-timeout",
        }:
            gate = max(gate, 2)
            reset_init_blocker = "reset-controller-not-halted"
            blocker = reset_init_blocker
        elif tag == "reset-pre-hcrst-cnr-timeout":
            gate = max(gate, 2)
            reset_init_blocker = "reset-pre-hcrst-controller-not-ready"
            blocker = reset_init_blocker
        elif tag == "reset-cnr-timeout":
            gate = max(gate, 2)
            reset_init_blocker = "reset-controller-not-ready"
            blocker = reset_init_blocker
        elif tag == "cmd-recovery-cnr-timeout":
            gate = max(gate, 3)
            command_timeout_detail = "reset-controller-not-ready"
            blocker = command_timeout_detail
        elif tag == "cmd-timeout":
            gate = max(gate, 3)
            command_timeout_detail = "cmd-timeout"
            blocker = command_timeout_detail
        elif tag == "cmd-prompt-safe-return-to-shell":
            gate = max(gate, 3)
            if command_timeout_detail not in precise_command_timeout_details:
                command_timeout_detail = "cmd-event-ring-timeout"
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
            saw_command_doorbell_write_pending = not saw_command_doorbell_hal_flush
            gate = max(gate, 3)
            if (
                command_timeout_detail is None
                and blocker in {"unknown", "none", "cmd-poll-pending"}
            ):
                blocker = (
                    "cmd-poll-pending"
                    if saw_command_doorbell_hal_flush
                    else "cmd-doorbell-flush-unproven"
                )
        elif (
            usb_command_probe_success(
                fields.get("command_probe", ""),
                fields.get("event_generation"),
                fields.get("cleanup_generation"),
                fields.get("recovery_source") or command_recovery_source,
            )
            or (
                usb_command_probe_result_success(raw, fields, command_recovery_source)
            )
            or fields.get("verdict", "").startswith("command-ring-ready")
        ):
            gate = max(gate, 4)
            outcome_blocker = normalize_usb_blocker(fields.get("outcome", "none"))
            if outcome_blocker in USB_OUTCOME_BLOCKERS:
                blocker = outcome_blocker
            else:
                blocker = "none"
        elif "[local-seat] xhci" in raw.lower() and "command-probe" in raw.lower():
            gate = max(gate, 3)
            if blocker in {"unknown", "none", "cmd-poll-pending"}:
                blocker = "cmd-event-ring-timeout"
        elif (
            "command-probe" in raw
            and fields.get("result", "").startswith("enable-slot-recovery-ok")
        ):
            gate = max(gate, 3)
            blocker = "cmd-event-ring-timeout"
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
                    value_label = value.lower().strip()
                    normalized_value = normalize_usb_blocker(value)
                    if key == "result" and (
                        value_label.startswith("0x") or value_label.isdecimal()
                    ):
                        continue
                    if (
                        key == "result"
                        and value
                        in {
                            "enable-slot-unproven",
                            "enable-slot-timeout",
                            "enable-slot-linux-event-unproven",
                            "enable-slot-uboot-first-unproven",
                            "no-op-unproven",
                        }
                        and fields.get("detail") == "cmd-event-ring-timeout"
                    ):
                        gate = max(gate, 3)
                        blocker = "cmd-event-ring-timeout"
                    elif key == "result" and value_label in {
                        "enable-slot-linux-captured-ok",
                        "enable-slot-linux-captured-ok-cleanup-failed",
                    }:
                        gate = max(gate, 3)
                        blocker = "cmd-event-ring-timeout"
                    elif (
                        key == "result"
                        and value_label == "enable-slot-linux-captured-timeout"
                        and command_timeout_detail in stale_command_timeout_details
                    ):
                        gate = max(gate, 3)
                        blocker = command_timeout_detail
                    elif (
                        normalized_value in USB_OUTCOME_BLOCKERS
                        and command_timeout_detail in stale_command_timeout_details
                    ):
                        blocker = command_timeout_detail
                    elif (
                        normalized_value in stale_command_timeout_details
                        and command_timeout_detail in precise_command_timeout_details
                    ):
                        blocker = command_timeout_detail
                    elif normalized_value == "unavailable" and blocker not in {
                        "unknown",
                        "none",
                    }:
                        continue
                    elif (
                        key == "result"
                        and normalized_value == "cmd-poll-only-timeout"
                        and fields.get("bus") == "pcie-window"
                    ):
                        if command_timeout_detail in precise_command_timeout_details:
                            blocker = command_timeout_detail
                        elif command_probe_verb and "enable-slot" in command_probe_verb:
                            blocker = "pcie-window-enable-slot-timeout"
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
        and blocker in USB_OUTCOME_BLOCKERS.union({"unavailable"})
    ):
        blocker = command_timeout_detail
    if (
        command_timeout_detail in precise_command_timeout_details
        and blocker in stale_command_timeout_details
    ):
        blocker = command_timeout_detail
    if reset_init_blocker and blocker in {
        "unknown",
        "none",
        "controller-init",
        "controller-init-edge",
        "controller-init-failed",
        "keyboard-not-ready",
        "no-controller",
        "unavailable",
    }:
        blocker = reset_init_blocker
    if reset_init_blocker:
        gate = min(gate, 2)
    if pcie_owner_blocker and blocker in {
        "unknown",
        "none",
        "controller-init",
        "controller-init-edge",
        "controller-init-failed",
        "keyboard-not-ready",
        "no-controller",
        "unavailable",
    }:
        blocker = pcie_owner_blocker
        gate = max(gate, 3)
    if (
        startup_fail_gate is not None
        and startup_fail_blocker is not None
    ):
        if startup_diag_blocker is not None:
            startup_gate = usb_driver_task_blocker_gate(startup_diag_blocker)
            if startup_diag_blocker == startup_fail_blocker:
                gate = max(gate, max(0, startup_fail_gate - 1))
            elif usb_driver_task_blocker_caps_gate(startup_diag_blocker):
                gate = startup_gate if gate <= 0 else min(gate, startup_gate)
            else:
                gate = max(gate, startup_gate)
            blocker = startup_diag_blocker
        else:
            gate = min(gate, max(0, startup_fail_gate - 1))
            blocker = startup_fail_blocker
    if (
        gate == 1
        and blocker == "unknown"
        and saw_keyboard_preseed
        and not saw_keyboard_runtime_init
    ):
        blocker = (
            "keyboard-runtime-init-blocked-by-net-init"
            if saw_net_init_before_keyboard
            else "keyboard-runtime-init-not-reached"
        )
    if direct_usb_progress_blocker is not None:
        direct_gate = usb_driver_task_blocker_gate(direct_usb_progress_blocker)
        if direct_gate > 1 and (
            gate <= direct_gate
            or blocker
            in {
                "unknown",
                "none",
                "keyboard-unavailable",
                "enable-slot-completion-pending",
                "enable-slot-completion-poll-no-reply",
            }
        ):
            gate = max(gate, direct_gate)
            blocker = direct_usb_progress_blocker
    if linked_runtime_gate10_seen and (
        blocker
        in USB_OUTCOME_BLOCKERS.union(
            {
                "unknown",
                "none",
                "command-input-ready",
                "keyboard-first-byte",
                "first-console-byte",
            }
        )
        or blocker.startswith("usb-keyboard-enumeration-")
    ):
        gate = max(gate, 10)
        blocker = "none"
    if linked_first_report_seen and gate >= 9 and (
        blocker in USB_OUTCOME_BLOCKERS.union({"unknown", "none", "attached"})
        or blocker.startswith("usb-keyboard-enumeration-")
    ):
        blocker = "none"
    if gate == 9 and blocker in {
        "keyboard-first-byte",
        "first-console-byte",
    } and usb_idle_no_key_byte_seen:
        blocker = "awaiting-physical-key"

    return gate, blocker


def summarize_usb_event_ring_state(events: Iterable[TraceEvent]) -> tuple[bool, int, int]:
    """Summarize whether xHCI command waits observed live event-ring traffic."""

    alive = False
    psc_count = 0
    psc_mask = 0
    for event in events:
        if event.domain != "usb":
            continue
        fields = event.fields
        tag = fields.get("tag", "").lower()
        if tag == "cmd-prompt-safe-psc-preserved":
            alive = True
            psc_count = max(psc_count, parse_hex_int(fields.get("psc_count")) or 0)
            psc_mask |= parse_hex_int(fields.get("psc_mask")) or 0
        elif tag == "cmd-wait-other-event":
            trb_type = parse_hex_int(fields.get("trb_type"))
            if trb_type == 0x22:
                alive = True
        elif "usb_runtime_enum_snapshot" in event.raw.lower():
            cmd_events_seen = parse_hex_int(fields.get("cmd_events_seen")) or 0
            cmd_event_type = parse_hex_int(fields.get("cmd_event_type")) or 0
            if fields.get("cmd_proof", "").lower() == "yes" and cmd_events_seen > 0:
                alive = True
                if cmd_event_type == 2:
                    psc_count = max(psc_count, cmd_events_seen)
    return alive, psc_count, psc_mask


def join_security_blocker_for_iovar(
    name: str | None, wpa_auth_ready_count: int
) -> str | None:
    """Return the precise join-security blocker for the active iovar."""

    if not name:
        return None
    normalized = name.lower()
    if normalized == "wpaie":
        return "join-security-wpaie-loop"
    if normalized == "wpa_auth":
        if wpa_auth_ready_count == 0:
            return "join-security-wpa-auth-initial-loop"
        return "join-security-wpa-auth-final-loop"
    if normalized == "auth":
        return "join-security-auth-loop"
    if normalized == "wsec":
        return "join-security-wsec-first-loop"
    if normalized == "sup_wpa":
        return "join-security-sup-wpa-loop"
    if normalized == "bsscfg:sup_wpa":
        return "join-security-bsscfg-sup-wpa-loop"
    return None


def summarize_wifi_gate(events: Iterable[TraceEvent]) -> tuple[int, str]:
    """Summarize the WiFi CYW43455 proof gate from HT through nettest."""

    wifi_events = [
        event
        for event in events
        if event.domain == "wifi"
        or (
            event.domain == "driver"
            and normalize_wifi_blocker(event.raw) == "wifi-driver-task-runtime-unproved"
        )
        or (
            event.domain == "driver"
            and (
                "hot_path=cyw43-wifi" in event.raw.lower()
                and "cyw43-host-eapol" in event.raw.lower()
            )
        )
        or "cyw43_driver_task_host_eapol_status" in event.raw.lower()
    ]
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
        "cyw43-kso-timeout-before-alp",
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
    precise_control_plane_blockers = {
        "control-plane-bdc-event",
        "control-plane-cur-etheraddr-len",
        "control-plane-hintless-firstread-no-irq",
        "control-plane-host-card-int-no-dongle-source",
        "control-plane-host-card-int-source-unreadable",
        "control-plane-interrupt-programming-drift",
        "control-plane-interrupts-deferred",
        "cyw43-control-tx-no-reply",
        "cyw43-control-tx-retry-no-reply",
        "cyw43-control-tx-not-submitted",
        "cyw43-sdio-descriptor-transfer-failed",
        "join-programming-host-latch-loop",
        "primary-bsscfg-wrapper-join-security-loop",
        "runtime-rx-host-latch-spam",
        "control-plane-legacy-gmode-stall",
        "control-plane-no-frame-indication-after-write",
        "control-plane-partial-hint-visibility",
        "control-plane-reply-idle-loop",
        *CYW43_HOST_EAPOL_FIRSTREAD_BLOCKER_NAMES,
        "firmware-supplicant-unsupported",
        "wifi-host-eapol-pending",
        "host-eapol-required",
        "host-eapol-ptk-install",
        "host-eapol-gtk-install",
        "wsec-pmk-bad-argument",
    }
    firmware_upload_blockers = {"cyw43-firmware-retry-exhausted"}
    sticky_wifi_exact_blockers = {"cyw43-sdio-descriptor-transfer-failed"}
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
    terminal_ht_timeout_seen = False
    control_plane_write_seen = False
    control_plane_reply_seen_after_write = False
    control_plane_idle_poll_count = 0
    dhcp_started_seen = False
    nettest_success_seen = False
    remote_cohsh_auth_seen = False
    netstats_status_wifi_bound_seen = False
    netstats_wifi_secure_seen = False
    netstats_txrx_seen = False
    dhcp_bound_seen = False
    root_console_net_ready_seen = False
    linux_probe_attach_seen = False
    linux_probe_pmu_write_active = False
    armcr4_prereset_ioctrl_active = False
    chipcommon_config_write_active = False
    socram_core_ctrl_stage: str | None = None
    specific_reset_blocker: str | None = None
    join_programming_blocker: str | None = None
    join_begin_seen = False
    join_completion_seen = False
    join_security_pending_iovar: str | None = None
    join_security_wpa_auth_ready_count = 0
    join_programming_host_latch_only_count = 0
    join_programming_f1_status_count = 0
    runtime_rx_host_latch_spam_count = 0
    legacy_gmode_stall_seen = False
    startup_blackbox_blocker: str | None = None
    startup_blackbox_gate = 0
    early_startup_blackbox_blocker: str | None = None
    early_startup_blackbox_gate = 0
    host_eapol_firstread_blocker_seen: str | None = None
    host_eapol_terminal_blocker: str | None = None
    generation_reset_ready_seen = False
    for event in wifi_events:
        raw = event.raw.lower()
        fields = event.fields
        if (
            "driver_task_resource_init" in raw
            and fields.get("stage") == "cyw43-firmware-recover"
            and fields.get("status") == "generation-reset-ready"
        ):
            generation_reset_ready_seen = True
        cached_only_evidence = fields.get("source", "").lower() == "cached"
        explicit_blocker = None
        key_success_stage = cyw43_host_eapol_key_install_success_stage(event)
        if (
            key_success_stage is not None
            and host_eapol_terminal_blocker is not None
            and host_eapol_terminal_blocker.startswith(f"{key_success_stage}-")
        ):
            recovered_blocker = host_eapol_terminal_blocker
            host_eapol_terminal_blocker = None
            if blocker == recovered_blocker:
                blocker = "none"
        if (
            "[cohsh-net][auth] auth ok" in raw
            or "[net-console] auth ok" in raw
            or "conn " in raw
            and " authenticated" in raw
            and "[net-console]" in raw
        ):
            remote_cohsh_auth_seen = True
        for key in (
            "reason",
            "detail",
            "err",
            "failure_domain",
            "outcome",
            "blocker",
            "cause",
            "descriptor_status",
            "exact",
            "exact_error",
        ):
            value = fields.get(key)
            if key == "reason" and value and value.lower() == "net-ready":
                continue
            if value and value not in {"none", "n/a"}:
                normalized_value = normalize_wifi_blocker(value)
                if normalized_value not in {"none", "unknown"}:
                    if (
                        explicit_blocker in sticky_wifi_exact_blockers
                        and normalized_value not in sticky_wifi_exact_blockers
                    ):
                        continue
                    explicit_blocker = normalized_value
        raw_contract_blocker = normalize_wifi_blocker(raw)
        split_control_exact = cyw43_control_split_event_exact(event)
        if split_control_exact is not None:
            raw_contract_blocker = split_control_exact
            explicit_blocker = split_control_exact
        if (
            "cyw43_driver_task_event_rx" in raw
            and fields.get("label", "").lower()
            in {"deauth", "deauth-ind", "disassoc", "disassoc-ind"}
        ):
            raw_contract_blocker = "wifi-link-down"
            explicit_blocker = "wifi-link-down"
        host_eapol_key_tx_no_reply = cyw43_host_eapol_key_tx_no_reply_detail(event)
        if host_eapol_key_tx_no_reply is not None:
            exact, _phase = host_eapol_key_tx_no_reply
            gate = max(gate, 7)
            post_f2_progress_seen = True
            blocker = exact
            host_eapol_terminal_blocker = exact
            explicit_blocker = exact
            continue
        if "cyw43_driver_task_command_fault" in raw:
            fault_blocker = normalize_wifi_blocker(
                fields.get("reason") or fields.get("detail") or raw
            )
            if fault_blocker.startswith("cyw43-post-release-"):
                gate = max(gate, 7)
                post_f2_progress_seen = True
                blocker = fault_blocker
                continue
            if fault_blocker.startswith("cyw43-transport-card-cmd"):
                # A post-generation-reset card-discovery fault is a new attach
                # attempt, not subordinate evidence for the earlier release
                # failure. Report the terminal retry frontier rather than
                # leaving the recovered 0x532a detail sticky.
                gate = 2 if generation_reset_ready_seen else max(gate, 2)
                blocker = fault_blocker
                explicit_blocker = fault_blocker
                continue
        if raw_contract_blocker == "uboot-policy-missing":
            gate = max(gate, 1)
            blocker = "uboot-policy-missing"
            continue
        if raw_contract_blocker == "wifi-data-rx-admission-blocked":
            gate = max(gate, 9)
            post_f2_progress_seen = True
            blocker = raw_contract_blocker
            explicit_blocker = raw_contract_blocker
        if raw_contract_blocker == "control-plane-legacy-gmode-stall":
            legacy_gmode_stall_seen = True
        if "cyw43_driver_task_host_eapol_status" in raw:
            status = fields.get("status", "").lower()
            reason = normalize_wifi_blocker(fields.get("reason", ""))
            firstread_blocker = cyw43_host_eapol_firstread_blocker(fields)
            firstread_blocker_seen = firstread_blocker is not None
            source_asserted_empty = (
                firstread_blocker == CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY
            )
            if reason in CYW43_HOST_EAPOL_KEY_INSTALL_BLOCKERS:
                if (
                    reason in CYW43_HOST_EAPOL_KEY_INSTALL_STATUS_BLOCKERS
                    and host_eapol_terminal_blocker
                    in CYW43_HOST_EAPOL_KEY_INSTALL_BLOCKERS
                    and host_eapol_terminal_blocker
                    not in CYW43_HOST_EAPOL_KEY_INSTALL_STATUS_BLOCKERS
                ):
                    reason = host_eapol_terminal_blocker
                gate = max(gate, 7)
                post_f2_progress_seen = True
                blocker = reason
                host_eapol_terminal_blocker = reason
                explicit_blocker = reason
            elif reason == CYW43_ASSOCIATION_EVENT_MISSING and not firstread_blocker_seen:
                gate = max(gate, 7)
                post_f2_progress_seen = True
                blocker = reason
                host_eapol_terminal_blocker = reason
                explicit_blocker = reason
            elif reason == "cyw43-association-not-associated" and not firstread_blocker_seen:
                gate = max(gate, 7)
                post_f2_progress_seen = True
                blocker = reason
                host_eapol_terminal_blocker = reason
                explicit_blocker = reason
            elif (
                status == "required"
                and cyw43_host_eapol_post_rescue_association_gap(fields)
                and not firstread_blocker_seen
            ):
                gate = max(gate, 7)
                post_f2_progress_seen = True
                blocker = "cyw43-association-not-associated"
                host_eapol_terminal_blocker = blocker
                explicit_blocker = blocker
            elif (
                status == "required"
                or reason == "host-eapol-required"
                or source_asserted_empty
            ):
                gate = max(gate, 7)
                post_f2_progress_seen = True
                if firstread_blocker is not None:
                    host_eapol_firstread_blocker_seen = firstread_blocker
                explicit_blocker = firstread_blocker or "host-eapol-required"
            elif status == "pending":
                gate = max(gate, 7)
                post_f2_progress_seen = True
                explicit_blocker = "wifi-host-eapol-pending"
            elif status in {
                "event-rx",
                "rx-observed",
                "eapol-rx",
                "rx-admission-refresh",
                "rx-admission-rescue",
            }:
                gate = max(gate, 7)
                post_f2_progress_seen = True
                explicit_blocker = "wifi-host-eapol-pending"
            elif status == "secure":
                gate = max(gate, 8)
                post_f2_progress_seen = True
        if "[cyw43] control-plane step=join action=begin" in raw:
            join_begin_seen = True
            join_completion_seen = False
            join_security_pending_iovar = None
            join_security_wpa_auth_ready_count = 0
            join_programming_host_latch_only_count = 0
            join_programming_f1_status_count = 0
            gate = max(gate, 7)
            post_f2_progress_seen = True
        if raw_contract_blocker in {
            "boot-deferred-local-seat-usb",
            "boot-deferred-root-console",
            "boot-waiting-for-wifi",
            "wifi-driver-task-runtime-unproved",
        }:
            explicit_blocker = raw_contract_blocker
        if (
            raw_contract_blocker in precise_control_plane_blockers
            and not cyw43_host_eapol_rx_blocker_name(explicit_blocker or "")
        ):
            explicit_blocker = raw_contract_blocker
        if raw_contract_blocker == "runtime-rx-host-latch-spam":
            runtime_rx_host_latch_spam_count += 1
        diag_gate = startup_diag_gate(raw, "wifi")
        if diag_gate is not None:
            status = fields.get("status", "").lower()
            name = fields.get("name", "wifi-startup-gate")
            firmware_stream_blocker_active = startup_blackbox_gate >= 5 and (
                startup_blackbox_blocker is not None
                and (
                    startup_blackbox_blocker.startswith("cyw43-")
                    or startup_blackbox_blocker.startswith("sdio-")
                )
            )
            if (
                diag_gate == 6
                and status == "pass"
                and (
                    fields.get("uploaded", "").lower() == "no"
                    or (parse_hex_int(fields.get("fault_detail")) or 0) != 0
                )
            ):
                status = "fail"
            elif firmware_stream_blocker_active and diag_gate > 6:
                status = "blocked"
            elif (
                diag_gate == 7
                and status == "pass"
                and (
                    fields.get("f2_enabled", "").lower() == "no"
                    or fields.get("f2_ready", "").lower() == "no"
                )
            ):
                status = "blocked"
            if status in {"pass", "inferred"}:
                gate = max(gate, diag_gate)
                if diag_gate >= 7:
                    post_f2_progress_seen = True
                if diag_gate >= 10:
                    blocker = "none"
            elif status == "fail":
                gate = max(gate, max(0, diag_gate - 1))
                detail_blocker = normalize_wifi_exact(fields.get("fault_detail", ""))
                blocker = (
                    detail_blocker
                    if detail_blocker != "none"
                    else normalize_wifi_blocker(name)
                )
                if blocker == "none":
                    blocker = name
                if diag_gate < 5:
                    early_startup_blackbox_blocker = blocker
                    early_startup_blackbox_gate = max(0, diag_gate - 1)
            if diag_gate >= 5 and blocker not in {"none", "unknown"}:
                startup_blackbox_blocker = blocker
                startup_blackbox_gate = max(0, diag_gate - 1)
            continue
        firmware_stream_blocker = wifi_firmware_stream_fault_blocker(event)
        if firmware_stream_blocker is not None:
            gate = max(gate, 5)
            blocker = firmware_stream_blocker
            startup_blackbox_blocker = firmware_stream_blocker
            startup_blackbox_gate = max(startup_blackbox_gate, 5)
            continue
        if raw.startswith("netstats:"):
            if (
                fields.get("active") == "wifi"
                and fields.get("addr_src") == "dhcp-lease"
                and fields.get("dhcp") == "bound"
            ):
                netstats_status_wifi_bound_seen = True
                dhcp_bound_seen = True
                gate = max(gate, 9)
                post_f2_progress_seen = True
                if nettest_success_seen:
                    if blocker in {"none", "tcp-proof-missing"}:
                        blocker = "netstats-missing"
                elif blocker == "none":
                    blocker = "tcp-proof-missing"
            cyw43_status_blocker = wifi_cyw43_status_blocker(fields)
            if fields.get("active") == "wifi" and cyw43_status_blocker is not None:
                dhcp_started_seen = cyw43_status_blocker[0] != "wifi-data-rx-admission-blocked"
                gate = max(gate, 9)
                post_f2_progress_seen = True
                blocker = cyw43_status_blocker[0]
            if fields.get("active") == "wifi" and (
                wifi_address_source(fields) == "dhcp-pending"
                or wifi_dhcp_phase(fields) == "selecting"
            ):
                dhcp_started_seen = wifi_dhcp_phase(fields) != "disabled"
                gate = max(gate, 9)
                post_f2_progress_seen = True
                blocker = (
                    "dhcp-not-started"
                    if wifi_dhcp_phase(fields) == "disabled"
                    else "dhcp-pending"
                )
            if fields.get("active") == "wifi" and (
                wifi_address_source(fields) == "dhcp-failed"
                or wifi_dhcp_phase(fields) == "failed"
            ):
                dhcp_started_seen = True
                gate = max(gate, 9)
                post_f2_progress_seen = True
                blocker = "dhcp-failed"
            if (
                fields.get("wifi_assoc") == "1"
                and fields.get("wifi_link") == "1"
                and fields.get("eapol_secure") == "1"
                and (parse_hex_int(fields.get("eapol_rx")) or 0) > 0
            ):
                netstats_wifi_secure_seen = True
                gate = max(gate, 9)
                post_f2_progress_seen = True
            if (
                (parse_hex_int(fields.get("rx_pkts")) or 0) > 0
                and (parse_hex_int(fields.get("tx_pkts")) or 0) > 0
            ):
                netstats_txrx_seen = True
                gate = max(gate, 9)
                post_f2_progress_seen = True
            if (
                nettest_success_seen
                and netstats_status_wifi_bound_seen
                and netstats_wifi_secure_seen
                and netstats_txrx_seen
            ):
                gate = max(gate, 10)
                blocker = "none"
            continue
        cyw43_status_blocker = wifi_cyw43_status_blocker(fields)
        if wifi_fields_active(fields) and cyw43_status_blocker is not None:
            dhcp_started_seen = cyw43_status_blocker[0] != "wifi-data-rx-admission-blocked"
            gate = max(gate, 9)
            post_f2_progress_seen = True
            blocker = cyw43_status_blocker[0]
            continue
        if wifi_fields_active(fields) and (
            wifi_address_source(fields) == "dhcp-pending"
            or wifi_dhcp_phase(fields) == "selecting"
        ):
            dhcp_started_seen = wifi_dhcp_phase(fields) != "disabled"
            gate = max(gate, 9)
            post_f2_progress_seen = True
            blocker = (
                "dhcp-not-started"
                if wifi_dhcp_phase(fields) == "disabled"
                else "dhcp-pending"
            )
            continue
        if wifi_fields_active(fields) and (
            wifi_address_source(fields) == "dhcp-failed"
            or wifi_dhcp_phase(fields) == "failed"
        ):
            dhcp_started_seen = True
            gate = max(gate, 9)
            post_f2_progress_seen = True
            blocker = "dhcp-failed"
            continue
        if "[dhcp] start ready" in raw:
            dhcp_started_seen = True
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-pending"
            continue
        if raw_contract_blocker == "wifi-link-down" or explicit_blocker == "wifi-link-down":
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "wifi-link-down"
            continue
        if (
            raw_contract_blocker == "cyw43-rxglom-unsupported"
            or explicit_blocker == "cyw43-rxglom-unsupported"
        ):
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "cyw43-rxglom-unsupported"
            continue
        if (
            raw_contract_blocker == "cyw43-kso-timeout-before-alp"
            or explicit_blocker == "cyw43-kso-timeout-before-alp"
        ):
            gate = max(gate, 4)
            blocker = "cyw43-kso-timeout-before-alp"
            continue
        if (
            blocker
            in {
                "control-plane-host-card-int-no-dongle-source",
                "control-plane-host-card-int-source-unreadable",
                "firmware-supplicant-unsupported",
                "wifi-host-eapol-pending",
                "host-eapol-required",
                "wsec-pmk-bad-argument",
            }
            and explicit_blocker == "control-plane-partial-hint-visibility"
            and raw.startswith("wifi:")
        ):
            explicit_blocker = None
            raw_contract_blocker = "none"
        if raw_contract_blocker in {
            "control-plane",
            "control-plane-bdc-event",
            "control-plane-cur-etheraddr-len",
            "control-plane-hintless-firstread-no-irq",
            "control-plane-host-card-int-no-dongle-source",
            "control-plane-host-card-int-source-unreadable",
            "control-plane-interrupt-programming-drift",
            "control-plane-interrupts-deferred",
            "control-plane-legacy-gmode-stall",
            "control-plane-no-reply",
            "control-plane-partial-hint-visibility",
            "control-plane-rearm-timeout",
            "control-plane-reply-idle-loop",
            "control-plane-sideband-unreadable",
            "control-plane-startup-link-timeout",
            "cyw43-control-tx-no-reply",
            "cyw43-control-tx-retry-no-reply",
            "cyw43-control-tx-not-submitted",
            "cyw43-sdio-descriptor-transfer-failed",
            *CYW43_HOST_EAPOL_FIRSTREAD_BLOCKER_NAMES,
            "firmware-supplicant-unsupported",
            "wifi-host-eapol-pending",
            "host-eapol-required",
            "ioctl-timeout",
            "runtime-rx-host-latch-spam",
            "wsec-pmk-bad-argument",
        } and explicit_blocker in {None, "cyw43", "nettest-policy-disabled"}:
            explicit_blocker = raw_contract_blocker
        if join_begin_seen and "[cyw43] iovar set begin" in raw:
            iovar_name = fields.get("name")
            if iovar_name in {
                "wpaie",
                "wpa_auth",
                "auth",
                "wsec",
                "sup_wpa",
                "bsscfg:sup_wpa",
            }:
                join_security_pending_iovar = iovar_name
        if join_begin_seen and "[cyw43] iovar set ready" in raw:
            iovar_name = fields.get("name")
            if iovar_name == "wpa_auth":
                join_security_wpa_auth_ready_count += 1
            if iovar_name == join_security_pending_iovar:
                join_security_pending_iovar = None
        if explicit_blocker in {
            "firmware-supplicant-unsupported",
            "wifi-host-eapol-pending",
            "host-eapol-required",
            "join-security-wpaie-loop",
            "join-security-wpa-auth-initial-loop",
            "join-security-wpa-auth-final-loop",
            "join-security-auth-loop",
            "join-security-wsec-first-loop",
            "join-security-sup-wpa-loop",
            "join-security-bsscfg-sup-wpa-loop",
            "primary-bsscfg-wrapper-join-security-loop",
            "wsec-pmk-bad-argument",
        }:
            join_programming_blocker = explicit_blocker
        if join_begin_seen and "iovar set failed name=bsscfg:" in raw:
            gate = max(gate, 7)
            post_f2_progress_seen = True
            join_programming_blocker = "primary-bsscfg-wrapper-join-security-loop"
        if join_begin_seen and (
            "iovar set failed name=wsec" in raw
        ):
            gate = max(gate, 7)
            post_f2_progress_seen = True
            join_programming_blocker = "join-security-wsec-first-loop"
        if join_begin_seen and (
            "[cyw43] iovar set failed" in raw
            or "ioctl no-progress-after-frame" in raw
        ):
            iovar_name = fields.get("name") or join_security_pending_iovar
            precise_join_blocker = join_security_blocker_for_iovar(
                iovar_name, join_security_wpa_auth_ready_count
            )
            if precise_join_blocker is not None:
                gate = max(gate, 7)
                post_f2_progress_seen = True
                join_programming_blocker = precise_join_blocker
                blocker = precise_join_blocker
        if "firmware stage=control-plane-write" in raw and "linux-f2-write-shape" in raw:
            control_plane_write_seen = True
            control_plane_reply_seen_after_write = False
            control_plane_idle_poll_count = 0
        if control_plane_write_seen and (
            "control-plane reply" in raw
            or "control-plane-reply action=" in raw and "ready" in raw
            or "[cyw43] control-plane reply" in raw
        ):
            control_plane_reply_seen_after_write = True
        if (
            control_plane_write_seen
            and not control_plane_reply_seen_after_write
            and "sdio xfer chunk" in raw
            and "fn=1" in raw
            and "op=read" in raw
            and ("base=0x0c020" in raw or "chunk=0x0c020" in raw)
        ):
            control_plane_idle_poll_count += 1
        if join_begin_seen and not join_completion_seen:
            if (
                "sdio xfer chunk" in raw
                and "fn=1" in raw
                and "op=read" in raw
                and ("base=0x0c020" in raw or "chunk=0x0c020" in raw)
            ):
                join_programming_f1_status_count += 1
            if (
                "stage=control-plane-reply" in raw
                and "source_state=host-card-int-latch-only" in raw
            ):
                join_programming_host_latch_only_count += 1
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
                and has_armcr4_wrap_base(raw)
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
        if (
            "sdio function-ready" in raw
            and fields.get("fn") == "2"
            and "assumed=yes" not in raw
            and "experimental-continue-without-ready" not in raw
            and "block=" in raw
        ):
            ready = parse_hex_int(fields.get("ready"))
            if ready is not None and (ready & 0x04) != 0:
                gate = max(gate, 6)
                post_f2_progress_seen = True
        if "function2 ready-snapshot" in raw and fields.get("diagnosis") == "f2-ready":
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
            if blocker not in {
                "firmware-supplicant-unsupported",
                "wifi-host-eapol-pending",
                "host-eapol-required",
                "wsec-pmk-bad-argument",
            }:
                blocker = "none"
        if (
            "control-plane step=" in raw or "control-plane preinit step=" in raw
        ) and " action=fail" in raw:
            gate = max(gate, 7)
            if explicit_blocker == "wsec-pmk-bad-argument":
                blocker = explicit_blocker
            elif (
                blocker
                in {
                    "control-plane-bdc-event",
                    "control-plane-cur-etheraddr-len",
                    "firmware-supplicant-unsupported",
                    "wifi-host-eapol-pending",
                    "host-eapol-required",
                    "wsec-pmk-bad-argument",
                }
                and explicit_blocker in precise_control_plane_blockers
            ):
                blocker = blocker
            elif explicit_blocker in precise_control_plane_blockers:
                blocker = explicit_blocker
            elif blocker in precise_control_plane_blockers and explicit_blocker in {
                "control-plane",
                "ioctl-timeout",
                "cyw43",
            }:
                blocker = blocker
            elif blocker in JOIN_SECURITY_EXACT_BY_BLOCKER and explicit_blocker in {
                "cyw43",
                "ioctl-timeout",
            }:
                blocker = blocker
            elif blocker not in precise_ht_blockers:
                blocker = explicit_blocker or "control-plane"
            continue
        if "join complete" in raw:
            join_completion_seen = True
            post_f2_progress_seen = True
            if wifi_join_complete_proven(fields):
                gate = max(gate, 8)
                blocker = "none"
            else:
                gate = max(gate, 7)
                blocker = "join-completion-unproven"
            continue
        if "join pending" in raw or "join armed" in raw:
            join_completion_seen = True
            gate = max(gate, 7)
            post_f2_progress_seen = True
            if explicit_blocker == "host-eapol-required":
                blocker = explicit_blocker
            else:
                blocker = "join-pending"
            continue
        if "join failed" in raw:
            join_completion_seen = True
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
            or wifi_address_source(fields) == "dhcp-lease"
        ):
            dhcp_bound_seen = True
            gate = max(gate, 9)
            post_f2_progress_seen = True
            if blocker in {"none", "dhcp-pending", "wifi-host-eapol-pending"}:
                blocker = "tcp-proof-missing"
            continue
        if wifi_root_console_net_ready_step(event):
            root_console_net_ready_seen = True
            if dhcp_bound_seen and blocker == "none":
                blocker = "tcp-proof-missing"
            continue
        if "[dhcp] tx queued" in raw:
            dhcp_started_seen = True
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-pending"
            continue
        if "[dhcp] rx transition" in raw:
            dhcp_started_seen = True
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-pending"
            continue
        if "[dhcp] rx ignored" in raw:
            dhcp_started_seen = True
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-invalid-packet"
            continue
        if "[dhcp] rx failed" in raw:
            dhcp_started_seen = True
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-failed"
            continue
        if "[dhcp] failed" in raw or "[dhcp] send failed" in raw:
            dhcp_started_seen = True
            gate = max(gate, 8)
            post_f2_progress_seen = True
            blocker = "dhcp-failed"
            continue
        if explicit_blocker in {"dhcp-pending", "dhcp-failed"}:
            if explicit_blocker == "dhcp-pending" and not dhcp_started_seen:
                gate = max(gate, 8)
                post_f2_progress_seen = True
                blocker = "dhcp-not-started"
                continue
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
            peer_assisted_ok = fields.get("peer_assisted_ok")
            nettest_result = field_lower(event, "result")
            if {tx_ok, udp_ok, tcp_ok, console_ok} <= {"true", "1"}:
                nettest_success_seen = True
                gate = max(gate, 9)
                post_f2_progress_seen = True
                blocker = "netstats-missing"
            elif nettest_result in {"pass", "peer-assisted-pass"} or (
                peer_assisted_ok in {"true", "1"}
            ) or (
                tx_ok in {"true", "1"} and remote_cohsh_auth_seen
            ):
                nettest_success_seen = True
                gate = max(gate, 9)
                post_f2_progress_seen = True
                if blocker.startswith("nettest-"):
                    blocker = "netstats-missing"
            else:
                if not (
                    blocker in precise_control_plane_blockers
                    or blocker.startswith("cyw43-post-release-")
                ):
                    gate = max(gate, 9)
                    post_f2_progress_seen = True
                    blocker = "nettest-failed"
            continue
        if raw.startswith("ok nettest"):
            nettest_success_seen = True
            gate = max(gate, 9)
            post_f2_progress_seen = True
            blocker = "netstats-missing"
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
            elif (
                blocker == "control-plane-partial-hint-visibility"
                and explicit_blocker
                in {
                    "control-plane-host-card-int-no-dongle-source",
                    "control-plane-host-card-int-source-unreadable",
                }
            ):
                blocker = explicit_blocker
            elif (
                blocker in precise_control_plane_blockers
                and explicit_blocker in precise_control_plane_blockers
            ):
                blocker = blocker
            elif blocker in precise_control_plane_blockers and explicit_blocker in {
                "ioctl-timeout",
                "cyw43",
            }:
                blocker = blocker
            elif (
                blocker in precise_control_plane_blockers
                and explicit_blocker is not None
                and explicit_blocker.startswith("nettest-")
            ):
                blocker = blocker
            elif (
                blocker.startswith("cyw43-post-release-")
                and explicit_blocker is not None
                and explicit_blocker.startswith("nettest-")
            ):
                blocker = blocker
            elif blocker in firmware_upload_blockers and explicit_blocker in {
                "cyw43",
                "cyw43-command",
                "ioctl-timeout",
                "nettest-failed",
            }:
                blocker = blocker
            elif blocker.startswith("cyw43-post-release-") and explicit_blocker in {
                "cyw43",
                "cyw43-command",
                "ioctl-timeout",
            }:
                blocker = blocker
            elif blocker in JOIN_SECURITY_EXACT_BY_BLOCKER and explicit_blocker in {
                "ioctl-timeout",
                "cyw43",
            }:
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
            elif not (
                blocker in precise_control_plane_blockers
                or blocker.startswith("cyw43-post-release-")
                or blocker in firmware_upload_blockers
                or blocker in precise_ht_blockers
                or blocker in JOIN_SECURITY_EXACT_BY_BLOCKER
                or blocker
                in {
                    "armcr4-release-readback-unavailable",
                    "boot-deferred-local-seat-usb",
                    "boot-deferred-root-console",
                    "boot-waiting-for-wifi",
                    "wifi-driver-task-runtime-unproved",
                }
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
            if has_armcr4_wrap_base(raw) and (
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
                "0x90481001",
                "0x90681001",
                "0x95481001",
                "0x95481004",
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
        if (
            "armcr4-release-proof=cpuhalt-clear-core-up" in raw
            or "cpuhalt_state=cpuhalt-clear-core-up" in raw
            or "cpuhalt=cpuhalt-clear-core-up" in raw
            or "stage=armcr4-core-up" in raw
        ):
            firmware_release_seen = True
            gate = max(gate, 4)
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
            "cyw43-post-release-mailbox-ready",
            "cyw43-post-release-protocol-version",
            "cyw43-post-release-rframe-sample",
        }:
            gate = max(gate, 7)
            post_f2_progress_seen = True
            blocker = explicit_blocker
            continue
        if explicit_blocker in {
            "function2-interrupt-unbound",
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
            "control-plane-bdc-event",
            "control-plane-cur-etheraddr-len",
            "control-plane-hintless-firstread-no-irq",
            "control-plane-host-card-int-no-dongle-source",
            "control-plane-host-card-int-source-unreadable",
            "control-plane-interrupt-programming-drift",
            "control-plane-interrupts-deferred",
            "control-plane-no-reply",
            "control-plane-partial-hint-visibility",
            "control-plane-rearm-timeout",
            "control-plane-reply-idle-loop",
            "control-plane-sideband-unreadable",
            "control-plane-startup-link-timeout",
            "cyw43-sdio-descriptor-transfer-failed",
            *CYW43_HOST_EAPOL_FIRSTREAD_BLOCKER_NAMES,
            "firmware-supplicant-unsupported",
            "wifi-host-eapol-pending",
            "host-eapol-required",
            "ioctl-timeout",
            "runtime-rx-host-latch-spam",
            "wsec-pmk-bad-argument",
        }:
            if blocker in precise_ht_blockers and not ht_available_seen:
                gate = max(gate, 4)
            else:
                gate = max(gate, 7)
            preserve_precise_control = (
                blocker
                in {
                    "control-plane-bdc-event",
                    "control-plane-cur-etheraddr-len",
                    "control-plane-host-card-int-no-dongle-source",
                    "control-plane-host-card-int-source-unreadable",
                    "firmware-supplicant-unsupported",
                    "wifi-host-eapol-pending",
                    "host-eapol-required",
                    "runtime-rx-host-latch-spam",
                    "wsec-pmk-bad-argument",
                }
                and explicit_blocker in precise_control_plane_blockers
            )
            if (
                blocker == "control-plane-partial-hint-visibility"
                and explicit_blocker
                in {
                    "control-plane-host-card-int-no-dongle-source",
                    "control-plane-host-card-int-source-unreadable",
                }
            ):
                blocker = explicit_blocker
            elif explicit_blocker == "cyw43-sdio-descriptor-transfer-failed":
                blocker = explicit_blocker
            elif explicit_blocker == "wsec-pmk-bad-argument":
                blocker = explicit_blocker
            elif preserve_precise_control:
                blocker = blocker
            elif (
                blocker in precise_control_plane_blockers
                and explicit_blocker in precise_control_plane_blockers
            ):
                blocker = blocker
            elif (
                blocker in precise_control_plane_blockers
                and explicit_blocker in {"control-plane", "ioctl-timeout", "cyw43"}
            ):
                blocker = blocker
            elif explicit_blocker in precise_control_plane_blockers:
                blocker = explicit_blocker
            elif blocker not in precise_ht_blockers and not preserve_precise_control:
                blocker = explicit_blocker
            continue
        if explicit_blocker in {"join-timeout", "wifi-association-failed"}:
            gate = max(gate, 7)
            blocker = explicit_blocker
            continue
        if explicit_blocker in {
            "boot-deferred-local-seat-usb",
            "boot-deferred-root-console",
            "boot-waiting-for-wifi",
            "wifi-driver-task-runtime-unproved",
        }:
            gate = max(gate, 1)
            if gate >= 4 and blocker not in {"unknown", "none"}:
                continue
            if blocker not in {
                "control-plane",
                "control-plane-bdc-event",
                "control-plane-cur-etheraddr-len",
                "control-plane-hintless-firstread-no-irq",
                "control-plane-host-card-int-no-dongle-source",
                "control-plane-host-card-int-source-unreadable",
                "control-plane-interrupt-programming-drift",
                "control-plane-interrupts-deferred",
                "control-plane-no-reply",
                "control-plane-partial-hint-visibility",
                "control-plane-rearm-timeout",
                "control-plane-reply-idle-loop",
                "control-plane-sideband-unreadable",
                "control-plane-startup-link-timeout",
                "cyw43-control-tx-no-reply",
                "cyw43-control-tx-retry-no-reply",
                "cyw43-control-tx-not-submitted",
                "function2-interrupt-unbound",
                "firmware-channel-f2",
                "firmware-ready-timeout",
                "firmware-supplicant-unsupported",
                "wifi-host-eapol-pending",
                "host-eapol-required",
                "ioctl-timeout",
                "mailbox-ready-timeout",
                "sdpcm-credit-timeout",
                "wsec-pmk-bad-argument",
            }:
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
                and "stage=wait-ht-clock" in raw
                and (
                    "timeout-terminal" in raw
                    or "action=ht-clock-terminal" in raw
                    or "active-ht-stable-timeout" in raw
                    or "status=active-ht-terminal-timeout" in raw
                )
            ):
                terminal_ht_timeout_seen = True
            if (
                explicit_blocker == "ht-clock-timeout"
                and firmware_release_seen
                and blocker in reset_phase_blockers
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
            if (
                blocker == "control-plane-partial-hint-visibility"
                and explicit_blocker
                in {
                    "control-plane-host-card-int-no-dongle-source",
                    "control-plane-host-card-int-source-unreadable",
                }
            ):
                blocker = explicit_blocker
            elif (
                blocker in precise_control_plane_blockers
                and explicit_blocker in precise_control_plane_blockers | {"control-plane"}
            ):
                pass
            elif (
                blocker in sticky_wifi_exact_blockers
                and explicit_blocker not in sticky_wifi_exact_blockers
            ):
                pass
            elif blocker in JOIN_SECURITY_EXACT_BY_BLOCKER and explicit_blocker in {
                "cyw43",
                "ioctl-timeout",
            }:
                pass
            elif blocker not in precise_ht_blockers and blocker != "ht-clock-timeout":
                blocker = explicit_blocker

    if nettest_success_seen:
        if netstats_status_wifi_bound_seen and netstats_wifi_secure_seen and netstats_txrx_seen:
            gate = max(gate, 10)
            blocker = "none"
        else:
            gate = max(gate, 9)
            if blocker == "none":
                blocker = "netstats-missing"
    if dhcp_bound_seen and root_console_net_ready_seen and blocker == "none":
        blocker = "tcp-proof-missing"
    if blocker == "function2-disabled" and gate >= 4 and not ht_available_seen:
        blocker = "ht-clock-timeout"
    if terminal_ht_timeout_seen and not ht_available_seen and not post_f2_progress_seen:
        gate = min(gate, 4)
        blocker = "ht-clock-timeout"
    if (
        control_plane_idle_poll_count >= 64
        and not control_plane_reply_seen_after_write
        and blocker != "control-plane-hintless-firstread-no-irq"
    ):
        gate = max(gate, 7)
        post_f2_progress_seen = True
        blocker = "control-plane-reply-idle-loop"
    if join_programming_blocker is not None and blocker in {
        "control-plane",
        "control-plane-hintless-firstread-no-irq",
        "control-plane-partial-hint-visibility",
        "control-plane-reply-idle-loop",
        "ioctl-timeout",
    }:
        gate = max(gate, 7)
        blocker = join_programming_blocker
    if (
        join_begin_seen
        and not join_completion_seen
        and join_programming_host_latch_only_count >= 8
        and join_programming_f1_status_count >= 16
    ):
        gate = max(gate, 7)
        post_f2_progress_seen = True
        blocker = join_programming_blocker or "join-programming-host-latch-loop"
    if runtime_rx_host_latch_spam_count >= 8 and blocker in {
        "control-plane",
        "control-plane-no-frame-indication-after-write",
        "wifi-host-eapol-pending",
        "host-eapol-required",
        "ioctl-timeout",
        "join-pending",
        "none",
    }:
        gate = max(gate, 7)
        post_f2_progress_seen = True
        blocker = "runtime-rx-host-latch-spam"
    if legacy_gmode_stall_seen and blocker in {
        "control-plane",
        "control-plane-partial-hint-visibility",
        "control-plane-reply-idle-loop",
        "control-plane-sideband-unreadable",
        "ioctl-timeout",
    }:
        gate = max(gate, 7)
        blocker = "control-plane-legacy-gmode-stall"
    if blocker in precise_ht_blockers and not ht_available_seen and not post_f2_progress_seen:
        gate = min(gate, 4)
    if (
        blocker in firmware_upload_blockers
        and not firmware_release_seen
        and not post_f2_progress_seen
    ):
        gate = min(gate, 5)
    if (
        startup_blackbox_blocker is not None
        and startup_blackbox_gate >= 4
        and gate <= startup_blackbox_gate
    ):
        blocker = startup_blackbox_blocker
    if early_startup_blackbox_blocker is not None and not post_f2_progress_seen:
        gate = early_startup_blackbox_gate
        blocker = early_startup_blackbox_blocker
    if (
        host_eapol_firstread_blocker_seen is not None
        and blocker
        in {
            "cyw43-driver-task-replay",
            "firmware-channel-f2",
            "host-eapol-required",
            "none",
            "sdio-linked-runtime-progress-no-reply",
            "unknown",
            "wifi-host-eapol-pending",
        }
        and not dhcp_bound_seen
    ):
        gate = max(gate, 7)
        blocker = host_eapol_firstread_blocker_seen
    if host_eapol_terminal_blocker is not None and not dhcp_bound_seen:
        gate = max(gate, 7)
        blocker = host_eapol_terminal_blocker
    if dhcp_bound_seen and root_console_net_ready_seen and blocker == "none":
        blocker = "tcp-proof-missing"
    return gate, blocker


def summarize_terminal_wifi_diag_failure(
    events: Iterable[TraceEvent],
) -> tuple[int, str, str, int] | None:
    """Return an explicit WiFi failure preserved across secondary recovery.

    Passive recovery telemetry can describe a secondary failure after the
    linked runtime has already preserved its causal first fault. The serial
    `wifi diag` gate table is authoritative when recovery explicitly labels its
    later fault `causal_preserved=yes`. Unrelated precise runtime evidence keeps
    the normal priority rules.
    """

    terminal: tuple[int, str, str, int] | None = None
    causal_preserved = False
    boundary_proves_terminal = False
    for event in events:
        raw = event.raw.lower()
        diag_gate = startup_diag_gate(raw, "wifi")
        if diag_gate is None:
            if terminal is not None and raw.startswith("wifi: evidence boundary"):
                proof_gate = parse_hex_int(event.fields.get("proof_gate"))
                frontier_gate = parse_hex_int(event.fields.get("frontier_gate"))
                failing_gate = parse_hex_int(event.fields.get("failing_gate"))
                boundary_exact = normalize_wifi_exact(
                    event.fields.get("failure_domain", "")
                )
                boundary_proves_terminal = (
                    proof_gate == terminal[0]
                    and frontier_gate == proof_gate
                    and failing_gate == terminal[0] + 1
                    and boundary_exact == terminal[1]
                )
                continue
            if terminal is not None and raw.startswith("wifi: cyw43 fault"):
                exact = normalize_wifi_exact(event.fields.get("detail", ""))
                if exact == terminal[1]:
                    terminal = (
                        terminal[0],
                        terminal[1],
                        event.fields.get("stage") or terminal[2],
                        event.line,
                    )
            if (
                terminal is not None
                and raw.startswith("wifi: recovery fault")
                and event.fields.get("causal_preserved", "").lower() == "yes"
            ):
                causal_preserved = True
            continue
        status = event.fields.get("status", "").lower()
        if status == "fail":
            exact = normalize_wifi_exact(event.fields.get("fault_detail", ""))
            if exact == "none":
                exact = normalize_wifi_blocker(
                    event.fields.get("blocker")
                    or event.fields.get("name")
                    or "wifi-startup-gate"
                )
            phase = event.fields.get("name") or f"wifi-gate-{diag_gate}"
            terminal = (max(0, diag_gate - 1), exact, phase, event.line)
            causal_preserved = False
            boundary_proves_terminal = False
            continue
        if (
            terminal is not None
            and status == "pass"
            and diag_gate >= terminal[0] + 1
        ):
            terminal = None
            causal_preserved = False
            boundary_proves_terminal = False
    return terminal if causal_preserved or boundary_proves_terminal else None


def wifi_failure_detail_from_fields(event: TraceEvent) -> tuple[str, str]:
    """Return the exact failure and phase carried by a Wi-Fi event."""

    exact = "none"
    raw = event.raw.lower()
    if raw.startswith(("ok wifi", "err wifi")) and event.fields.get(
        "detail", ""
    ).lower().startswith("subcommand="):
        return "none", "none"
    failure_domain = normalize_wifi_exact(event.fields.get("failure_domain", ""))
    if failure_domain in {
        "cyw43-control-tx-no-reply",
        "cyw43-control-tx-retry-no-reply",
        "cyw43-control-tx-not-submitted",
    }:
        phase = (
            event.fields.get("stage")
            or event.fields.get("phase")
            or (
                event.stage
                if event.stage not in {"boundary", "evidence", "next_action"}
                else ""
            )
            or "none"
        )
        return failure_domain, phase
    if (
        event.fields.get("stage", "").lower() == "cyw43-control-txglomalign"
        and event.fields.get("status", "").lower() in {"tx-no-reply", "tx-retry-no-reply"}
    ):
        if event.fields.get("status", "").lower() == "tx-retry-no-reply":
            return "cyw43-control-tx-retry-no-reply", "cyw43-control-txglomalign"
        return "cyw43-control-tx-no-reply", "cyw43-control-txglomalign"
    if "linked_runtime_progress" in raw:
        marker_exact = normalize_wifi_exact(event.fields.get("blocker", ""))
        if marker_exact.startswith(
            (
                "cyw43-engine-init-",
                "cyw43-state-reset-",
                "cyw43-bus-link-",
                "cyw43-backplane-",
                "cyw43-release-",
                "cyw43-shared-control-",
                "cyw43-sdio-owner-",
                "cyw43-resource-",
            )
        ) or marker_exact == "cyw43-forbidden-sdio-mmio":
            return marker_exact, marker_exact
    raw_cyw43_progress = cyw43_raw_engine_init_progress_blocker(event)
    if raw_cyw43_progress is not None:
        return raw_cyw43_progress, raw_cyw43_progress
    raw_cyw43_progress = cyw43_raw_command_progress_blocker(event)
    if raw_cyw43_progress is not None:
        return raw_cyw43_progress, raw_cyw43_progress
    control_split_exact = cyw43_control_split_event_exact(event)
    if control_split_exact is not None:
        phase = event.fields.get("stage") or event.stage or "cyw43-control-split"
        return control_split_exact, phase
    key_tx_no_reply = cyw43_host_eapol_key_tx_no_reply_detail(event)
    if key_tx_no_reply is not None:
        return key_tx_no_reply
    control_reply_exact = cyw43_control_reply_event_exact(event)
    if control_reply_exact is not None:
        phase = event.fields.get("stage") or event.stage or "cyw43-control-reply"
        return control_reply_exact, phase
    command_no_reply_exact = cyw43_command_no_reply_event_exact(event)
    if command_no_reply_exact is not None:
        progress_phase = normalize_wifi_exact(
            event.fields.get("progress_phase_name", "")
        )
        if command_no_reply_exact == progress_phase:
            phase = event.fields.get("progress_phase_name") or "cyw43-command"
        else:
            phase = event.fields.get("stage") or event.stage or "cyw43-command"
        return command_no_reply_exact, phase
    if (
        "cyw43_driver_task_control_split" in raw
        and event.fields.get("event", "").lower() == "poll-complete"
    ):
        return "none", event.fields.get("stage") or event.stage or "cyw43-control-split"
    control_status_exact = cyw43_control_exchange_status_event_exact(event)
    if control_status_exact is not None:
        phase = event.fields.get("stage") or event.stage or "cyw43-control-exchange"
        return control_status_exact, phase
    control_timeout_exact = cyw43_control_exchange_timeout_event_exact(event)
    if control_timeout_exact is not None:
        phase = event.fields.get("stage") or event.stage or "cyw43-control-exchange"
        return control_timeout_exact, phase
    firmware_stream_blocker = wifi_firmware_stream_fault_blocker(event)
    if firmware_stream_blocker is not None:
        phase = (
            event.fields.get("stage")
            or event.fields.get("name")
            or event.stage
            or "firmware-upload"
        )
        return firmware_stream_blocker, phase
    if normalize_wifi_blocker(event.raw) == "control-plane-legacy-gmode-stall":
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "gmode"
        )
        return "cyw43-control-plane-legacy-gmode-stall", phase
    if normalize_wifi_blocker(event.raw) == "join-programming-host-latch-loop":
        return "cyw43-join-programming-host-latch-loop", "join"
    if normalize_wifi_blocker(event.raw) == "runtime-rx-host-latch-spam":
        return "cyw43-runtime-rx-host-latch-spam", "runtime-rx"
    join_security_exact = JOIN_SECURITY_EXACT_BY_BLOCKER.get(
        normalize_wifi_blocker(event.raw)
    )
    if join_security_exact is not None:
        return join_security_exact, "join"
    if normalize_wifi_blocker(event.raw) == "primary-bsscfg-wrapper-join-security-loop":
        return "cyw43-primary-bsscfg-wrapper-join-security-loop", "join"
    if (
        "post-write-no-frame-source-terminal" in raw
        or "hintless-firstread-no-frame-source-terminal" in raw
    ):
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "control-plane-reply"
        )
        exact = event.fields.get("exact_error") or event.fields.get("exact")
        if exact and exact not in {"none", "n/a"}:
            return normalize_wifi_exact(exact), phase
        return "cyw43-control-plane-no-frame-indication-after-write", phase
    if "post-write-no-irq-terminal" in raw or "hintless-firstread-no-irq" in raw:
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "control-plane-reply"
        )
        return "cyw43-control-plane-hintless-firstread-no-irq", phase
    if "bdc-event" in raw:
        exact = "cyw43-control-plane-bdc-event"
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "none"
        )
        return exact, phase
    if "cur-etheraddr-len" in event.raw.lower():
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "none"
        )
        return "cyw43-protocol-error-cur-etheraddr-len", phase
    if normalize_wifi_blocker(event.raw) == "wsec-pmk-bad-argument":
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "none"
        )
        return "wsec-pmk-bad-argument", phase
    if normalize_wifi_blocker(event.raw) == "firmware-supplicant-unsupported":
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "join-security"
        )
        return "firmware-supplicant-unsupported", phase
    if normalize_wifi_blocker(event.raw) == "wifi-host-eapol-pending":
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "join-security"
        )
        return "wifi-host-eapol-pending", phase
    key_install_blocker = normalize_wifi_blocker(event.raw)
    if key_install_blocker in CYW43_HOST_EAPOL_KEY_INSTALL_BLOCKERS:
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "join-security"
        )
        return key_install_blocker, phase
    if (
        "cyw43_driver_task_host_eapol_status" in event.raw.lower()
        and cyw43_host_eapol_post_rescue_association_gap(event.fields)
    ):
        firstread_blocker = cyw43_host_eapol_firstread_blocker(event.fields)
        if firstread_blocker == CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY:
            return firstread_blocker, "runtime-rx"
        return "cyw43-association-not-associated", "association"
    if normalize_wifi_blocker(event.raw) == "host-eapol-required":
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or event.stage
            or "join-security"
        )
        return "host-eapol-required", phase
    if normalize_wifi_blocker(event.raw) == "cyw43-association-not-associated":
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or "association"
        )
        return "cyw43-association-not-associated", phase
    if normalize_wifi_blocker(event.raw) == CYW43_ASSOCIATION_EVENT_MISSING:
        firstread_blocker = cyw43_host_eapol_firstread_blocker(event.fields)
        if firstread_blocker == CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY:
            return firstread_blocker, "runtime-rx"
        phase = (
            event.fields.get("stage")
            or event.fields.get("step")
            or event.fields.get("current")
            or event.fields.get("focus")
            or "association"
        )
        return CYW43_ASSOCIATION_EVENT_MISSING, phase
    for key in ("exact", "exact_error", "err", "cause", "detail", "reason"):
        value = event.fields.get(key)
        if value and value not in {"none", "n/a"}:
            exact = normalize_wifi_exact(value)
            break
    phase = (
        event.fields.get("stage")
        or event.fields.get("step")
        or event.fields.get("current")
        or event.fields.get("focus")
        or event.stage
        or "none"
    )
    return exact, phase


def wifi_failure_detail_priority(event: TraceEvent, wifi_blocker: str, candidate: str) -> int:
    """Rank matching Wi-Fi blocker lines so boot failures beat later commands."""

    raw = event.raw.lower()
    if candidate != wifi_blocker:
        return 100
    if (
        candidate == "cyw43-runtime-command-rejected"
        and wifi_firmware_stream_fault_blocker(event)
        == "cyw43-transport-command-admission"
    ):
        return 0
    if "post-write-no-frame-source-terminal" in raw:
        return 0
    if "hintless-firstread-no-frame-source-terminal" in raw:
        return 0
    if "post-write-no-irq-terminal" in raw:
        return 0
    if cyw43_control_exchange_timeout_event_exact(event) is not None:
        return 0
    if candidate in {"cyw43-control-tx-no-reply", "cyw43-control-tx-retry-no-reply"} and (
        f"failure_domain={candidate}" in raw
        or (
            "stage=cyw43-control-txglomalign" in raw
            and (
                (candidate == "cyw43-control-tx-no-reply" and "status=tx-no-reply" in raw)
                or (
                    candidate == "cyw43-control-tx-retry-no-reply"
                    and "status=tx-retry-no-reply" in raw
                )
            )
        )
    ):
        return 0
    if (
        cyw43_control_split_event_exact(event) is not None
        or cyw43_control_reply_event_exact(event) is not None
    ):
        return 0
    if wifi_firmware_stream_fault_blocker(event) == candidate:
        if raw.startswith("wifi: gate"):
            return 10
        return 0
    if candidate == "runtime-rx-host-latch-spam":
        return 1
    if "linked_runtime_progress" in raw and any(
        marker in raw
        for marker in (
            "blocker=cyw43-engine-init-",
            "blocker=cyw43-state-reset-",
            "blocker=cyw43-bus-link-",
            "blocker=cyw43-shared-control-",
            "blocker=cyw43-sdio-owner-",
            "blocker=cyw43-forbidden-sdio-mmio",
        )
    ):
        return 1
    if "[cyw43] control-plane" in raw and "action=fail" in raw:
        return 1
    if (
        event.domain == "driver"
        and event.fields.get("contract", "").lower() == "cyw43455"
        and event.fields.get("stage", "").lower() == "cyw43-transport-init"
    ):
        return 1
    if event.domain == "driver" and event.fields.get("contract", "").lower() == "cyw43455":
        return 2
    if "[cyw43] iovar" in raw and "failed" in raw:
        return 2
    if "[cyw43] init failure" in raw or "boot_failure source=live" in raw:
        return 3
    if "[net-console] deferred failed" in raw:
        return 4
    if "control-plane snapshot" in raw:
        return 5
    if raw.startswith("err nettest") or raw.startswith("ok nettest"):
        return 90
    return 10


def summarize_cyw43_control_revinfo_badarg(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return exact proof when firmware rejects the linked-runtime revinfo GET."""

    revinfo_active = False
    for event in events:
        raw = event.raw.lower()
        fields = event.fields
        if (
            event.domain == "driver"
            and fields.get("contract", "").lower() == "cyw43455"
            and "driver_task_resource_init" in raw
        ):
            stage = fields.get("stage", "").lower()
            status = fields.get("status", "").lower()
            if stage == "cyw43-control-revinfo" and status == "begin":
                revinfo_active = True
                continue
            if stage == "cyw43-control-revinfo" and status in {"ready", "unsupported"}:
                revinfo_active = False
                continue
            if stage.startswith("cyw43-control-") and status == "begin":
                revinfo_active = stage == "cyw43-control-revinfo"
        if not revinfo_active or "cyw43_driver_task_command_fault" not in raw:
            continue
        if fields.get("contract", "").lower() != "cyw43455":
            continue
        if parse_hex_int(fields.get("op")) != CYW43_CONTROL_EXCHANGE_OP:
            continue
        if parse_hex_int(fields.get("detail")) != CYW43_CONTROL_EXCHANGE_FAULT_DETAIL:
            continue
        if parse_hex_int(fields.get("result")) != CYW43_CONTROL_EXCHANGE_BCME_BADARG:
            continue
        return (
            "cyw43-control-revinfo-badarg",
            "cyw43-control-revinfo",
            event.line,
        )
    return None


def summarize_cyw43_control_txglomalign_reject(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return exact proof when firmware rejects the first txglomalign exchange."""

    latest: tuple[str, str, int] | None = None
    resolved_line = 0
    for event in events:
        raw_lower = event.raw.lower()
        if "driver_task_resource_init" in raw_lower:
            stage = event.fields.get("stage", "").lower()
            status = event.fields.get("status", "").lower()
            if (
                stage == "cyw43-control-txglomalign"
                and status
                in {
                    "ready",
                    "optional-badarg",
                    "optional-unsupported",
                    "optional-skip",
                }
            ) or (
                stage == "cyw43-control-txglomalign-fallback4"
                and status
                in {
                    "ready",
                    "optional-badarg",
                    "optional-unsupported",
                }
            ):
                resolved_line = max(resolved_line, event.line)
        exact = cyw43_control_exchange_status_event_exact(event)
        if exact not in {
            "cyw43-control-txglomalign-badarg",
            "cyw43-control-txglomalign-unsupported",
        }:
            continue
        latest = (exact, "cyw43-control-txglomalign", event.line)
    if latest is not None and latest[2] <= resolved_line:
        return None
    return latest


def summarize_cyw43_split_descriptor_fault(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return the raw split-control descriptor fault, if one occurred."""

    latest: tuple[str, str, int] | None = None
    for event in events:
        if (
            cyw43_control_split_event_exact(event)
            != "cyw43-sdio-descriptor-transfer-failed"
        ):
            continue
        phase = event.fields.get("stage") or event.stage or "cyw43-control-split"
        latest = ("cyw43-sdio-descriptor-transfer-failed", phase, event.line)
    return latest


def summarize_cyw43_control_tx_no_reply(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return terminal CYW43 control-TX no-reply proof, if unrecovered."""

    latest: tuple[str, str, int] | None = None
    for event in events:
        raw = event.raw.lower()
        if (
            "control-plane step=init-complete action=ready" in raw
            or (
                event.fields.get("stage", "").lower() == "cyw43-control-plane"
                and event.fields.get("status", "").lower() == "ready"
            )
        ):
            latest = None
            continue
        exact, phase = wifi_failure_detail_from_fields(event)
        if exact in {
            "cyw43-control-tx-no-reply",
            "cyw43-control-tx-retry-no-reply",
            "cyw43-control-tx-not-submitted",
        }:
            if latest is None or (
                latest[1] in {"none", "cyw43-control-tx"}
                and phase not in {"none", "cyw43-control-tx"}
            ):
                latest = (exact, phase, event.line)
    return latest


def summarize_wifi_failure_detail(
    events: Iterable[TraceEvent], wifi_blocker: str
) -> tuple[str, str, int]:
    """Find the best source line for the current Wi-Fi gate blocker."""

    if wifi_blocker == "none":
        return "none", "none", 0

    socram_core_ctrl_stage: str | None = None
    armcr4_prereset_ioctrl_active = False
    exact = "none"
    phase = "none"
    line = 0
    blocker_matched = False
    blocker_priority = 100
    join_begin_seen = False
    join_security_pending_iovar: str | None = None
    join_security_wpa_auth_ready_count = 0
    for event in (
        event
        for event in events
        if event.domain == "wifi"
        or (
            event.domain == "driver"
            and event.fields.get("contract", "").lower()
            in {"cyw43455", "sdio-host"}
        )
        or (
            event.domain == "driver"
            and event.fields.get("role", "").lower() in {"cyw43-wifi", "sdio-host"}
        )
        or (
            event.domain == "driver"
            and normalize_wifi_blocker(event.raw) == "wifi-driver-task-runtime-unproved"
        )
        or "cyw43_driver_task_host_eapol_status" in event.raw.lower()
    ):
        raw = event.raw.lower()
        fields = event.fields
        if "[cyw43] control-plane step=join action=begin" in raw:
            join_begin_seen = True
            join_security_pending_iovar = None
            join_security_wpa_auth_ready_count = 0
        if (
            "prereset-fgc-clock" in raw
            or (
                "firmware core-ctrl access" in raw
                and has_armcr4_wrap_base(raw)
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
        firstread_blocker = cyw43_host_eapol_firstread_blocker(fields)
        if (
            wifi_blocker == "cyw43-association-not-associated"
            and "cyw43_driver_task_host_eapol_status" in raw
            and cyw43_host_eapol_post_rescue_association_gap(fields)
            and firstread_blocker != CYW43_HOST_EAPOL_SOURCE_ASSERTED_EMPTY
        ):
            candidate = wifi_blocker
        if (
            wifi_blocker == CYW43_ASSOCIATION_EVENT_MISSING
            and "cyw43_driver_task_host_eapol_status" in raw
        ):
            candidate = wifi_blocker
        if wifi_blocker == firstread_blocker:
            candidate = wifi_blocker
        firmware_stream_blocker = wifi_firmware_stream_fault_blocker(event)
        if firmware_stream_blocker is not None and wifi_blocker == firmware_stream_blocker:
            candidate = firmware_stream_blocker
        if (
            wifi_blocker == "cyw43-runtime-command-rejected"
            and firmware_stream_blocker == "cyw43-transport-command-admission"
        ):
            candidate = wifi_blocker
        if (
            wifi_blocker == "cyw43-driver-task-replay"
            and event.domain == "driver"
            and fields.get("contract", "").lower() == "cyw43455"
            and "driver_task_resource_init" in raw
            and fields.get("status", "").lower()
            not in {"ready", "deferred", "begin", "progress"}
        ):
            candidate = "cyw43-driver-task-replay"
        if (
            wifi_blocker == "sdio-card-select"
            and event.domain == "driver"
            and fields.get("contract", "").lower() == "sdio-host"
            and (
                fields.get("stage", "").lower().startswith("sdio-cmd")
                or fields.get("stage", "").lower().startswith("sdio-first")
            )
            and "driver_task_resource_init" in raw
            and fields.get("status", "").lower()
            not in {"ready", "deferred", "begin", "progress"}
        ):
            candidate = "sdio-card-select"
        replay_status = fields.get("blocker", "").lower()
        if (
            wifi_blocker == "sdio-card-select"
            and "sdio_driver_task_replay_status" in raw
            and fields.get("stage", "").lower() in {"engine-init", "sdio-first-command"}
            and replay_status not in {"", "none", "begin", "ready", "success"}
        ):
            candidate = "sdio-card-select"
        if (
            wifi_blocker.startswith("sdio-engine-init-")
            and "sdio_driver_task_replay_status" in raw
            and fields.get("stage", "").lower() == "engine-init"
            and replay_status not in {"", "none", "begin", "ready", "success"}
        ):
            candidate = wifi_blocker
        if (
            wifi_blocker == "sdio-driver-task-replay"
            and "sdio_driver_task_replay_status" in raw
            and replay_status not in {"", "none", "begin", "ready", "success"}
        ):
            candidate = "sdio-driver-task-replay"
        if (
            wifi_blocker == "cyw43-driver-task-replay"
            and "net_driver_task_replay_status" in raw
            and replay_status not in {"", "none", "begin", "ready", "success"}
        ):
            candidate = "cyw43-driver-task-replay"
        if (
            wifi_blocker.startswith("cyw43-engine-init-")
            and "net_driver_task_replay_status" in raw
            and fields.get("stage", "").lower() in {"engine-init", "engine-init-replay"}
            and replay_status not in {"", "none", "begin", "ready", "success", "no-reply"}
        ):
            candidate = wifi_blocker
        if (
            wifi_blocker == "control-plane-reply-idle-loop"
            and "sdio xfer chunk" in raw
            and "fn=1" in raw
            and "op=read" in raw
            and ("base=0x0c020" in raw or "chunk=0x0c020" in raw)
        ):
            candidate = "control-plane-reply-idle-loop"
        if (
            wifi_blocker == "control-plane-reply-idle-loop"
            and cyw43_control_exchange_timeout_event_exact(event) is not None
        ):
            candidate = "control-plane-reply-idle-loop"
        if (
            wifi_blocker == "control-plane-reply-idle-loop"
            and (
                cyw43_control_split_event_exact(event) is not None
                or cyw43_control_reply_event_exact(event) is not None
            )
        ):
            candidate = "control-plane-reply-idle-loop"
        if (
            wifi_blocker == "control-plane-reply-idle-loop"
            and cyw43_command_no_reply_event_exact(event) is not None
        ):
            candidate = "control-plane-reply-idle-loop"
        if (
            wifi_blocker == "join-programming-host-latch-loop"
            and join_begin_seen
            and "stage=control-plane-reply" in raw
            and "source_state=host-card-int-latch-only" in raw
        ):
            candidate = "join-programming-host-latch-loop"
        if (
            wifi_blocker == "primary-bsscfg-wrapper-join-security-loop"
            and join_begin_seen
            and "iovar set failed name=bsscfg:" in raw
        ):
            candidate = "primary-bsscfg-wrapper-join-security-loop"
        if wifi_blocker == "runtime-rx-host-latch-spam" and (
            "action=no-frame-source-after-firstread" in raw
            or "action=irq-latched-firstread-invalid" in raw
            or "action=irq-latched-firstread-empty" in raw
        ):
            candidate = "runtime-rx-host-latch-spam"
        if join_begin_seen and "[cyw43] iovar set begin" in raw:
            iovar_name = fields.get("name")
            if iovar_name in {
                "wpaie",
                "wpa_auth",
                "auth",
                "wsec",
                "sup_wpa",
                "bsscfg:sup_wpa",
            }:
                join_security_pending_iovar = iovar_name
        if join_begin_seen and "[cyw43] iovar set ready" in raw:
            iovar_name = fields.get("name")
            if iovar_name == "wpa_auth":
                join_security_wpa_auth_ready_count += 1
            if iovar_name == join_security_pending_iovar:
                join_security_pending_iovar = None
        if wifi_blocker in JOIN_SECURITY_EXACT_BY_BLOCKER and join_begin_seen and (
            "[cyw43] iovar set failed" in raw
            or "ioctl no-progress-after-frame" in raw
        ):
            iovar_name = fields.get("name") or join_security_pending_iovar
            candidate = (
                join_security_blocker_for_iovar(
                    iovar_name, join_security_wpa_auth_ready_count
                )
                or candidate
            )
        if (
            wifi_blocker == "join-security-wsec-first-loop"
            and join_begin_seen
            and (
                "iovar set failed name=wsec" in raw
                or "ioctl no-progress-after-frame" in raw
            )
        ):
            candidate = "join-security-wsec-first-loop"
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
        if event_exact == wifi_blocker:
            candidate = wifi_blocker
        if (
            blocker_matched
            and wifi_blocker == "cyw43-engine-init-no-reply"
            and exact.startswith("cyw43-engine-init-")
            and exact != wifi_blocker
            and event_exact == wifi_blocker
        ):
            continue
        if (
            wifi_blocker == "cyw43-firmware-runtime-replay"
            and event_exact.startswith("cyw43-release-")
            and event_exact.endswith("-no-reply")
        ):
            candidate = wifi_blocker
        if (
            wifi_blocker == "cyw43-firmware-runtime-replay"
            and event_exact
            in {
                "cyw43-runtime-command-rejected",
                "cyw43-transport-command-admission",
                "cyw43-post-release-ht-clock",
                "cyw43-post-release-function2-ready",
                "cyw43-post-release-corecontrol",
                "cyw43-post-release-mailbox-ready",
                "cyw43-post-release-protocol-version",
                "cyw43-post-release-rframe-sample",
                "cyw43-post-release-dpc-activate",
                "cyw43-probe-cardctrl-read",
                "cyw43-probe-cardctrl-write",
                "cyw43-probe-pmucontrol-read",
                "cyw43-probe-pmucontrol-write",
                "cyw43-probe-function2-disable-read",
                "cyw43-probe-function2-disable-write",
                "cyw43-probe-sdonly-clock",
            }
        ):
            candidate = wifi_blocker
        if (
            wifi_blocker == "cyw43-engine-init-no-reply"
            and event_exact.startswith("cyw43-engine-init-")
            and event_exact.endswith("-no-reply")
        ):
            candidate = wifi_blocker
        if event_exact != "none" and not blocker_matched:
            exact = event_exact
            phase = event_phase
            line = event.line
        if candidate == wifi_blocker:
            candidate_priority = wifi_failure_detail_priority(event, wifi_blocker, candidate)
            if (
                blocker_matched
                and event_exact == "none"
                and exact != "none"
                and candidate_priority >= blocker_priority
            ):
                continue
            if blocker_matched and candidate_priority > blocker_priority:
                continue
            if blocker_matched and candidate_priority == blocker_priority and wifi_blocker in {
                "control-plane-host-card-int-no-dongle-source",
                "control-plane-host-card-int-source-unreadable",
            }:
                continue
            if blocker_matched and wifi_blocker in {
                "control-plane-cur-etheraddr-len",
                "wsec-pmk-bad-argument",
            }:
                continue
            blocker_matched = True
            blocker_priority = candidate_priority
            exact = event_exact
            if candidate in {
                "sdio-driver-task-replay",
                "cyw43-driver-task-replay",
                "sdio-card-select",
            } and (
                "sdio_driver_task_replay_status" in raw
                or "net_driver_task_replay_status" in raw
            ):
                replay_stage = fields.get("stage") or event.stage or "driver-task-replay"
                exact = f"{replay_stage}-{replay_status or 'unknown'}"
            if candidate.startswith("sdio-engine-init-"):
                exact = candidate
            if candidate.startswith("cyw43-engine-init-") and not (
                candidate == "cyw43-engine-init-no-reply"
                and event_exact.startswith("cyw43-engine-init-")
                and event_exact != candidate
            ):
                exact = candidate
            if candidate == "join-programming-host-latch-loop":
                exact = "cyw43-join-programming-host-latch-loop"
            if candidate == "runtime-rx-host-latch-spam":
                exact = "cyw43-runtime-rx-host-latch-spam"
            if cyw43_host_eapol_rx_blocker_name(candidate):
                exact = candidate
            if candidate in JOIN_SECURITY_EXACT_BY_BLOCKER:
                exact = JOIN_SECURITY_EXACT_BY_BLOCKER[candidate]
            if candidate == "primary-bsscfg-wrapper-join-security-loop":
                exact = "cyw43-primary-bsscfg-wrapper-join-security-loop"
            if exact == "none" and "sdio cmd53 r5 fail" in raw:
                exact = "sdio-cmd53-r5-error"
            if exact == "none":
                exact = candidate
            phase = (
                "join"
                if candidate
                in {
                    *JOIN_SECURITY_EXACT_BY_BLOCKER.keys(),
                    "join-programming-host-latch-loop",
                    "primary-bsscfg-wrapper-join-security-loop",
                }
                else
                "runtime-rx"
                if candidate == "runtime-rx-host-latch-spam"
                or cyw43_host_eapol_rx_blocker_name(candidate)
                else
                socram_core_ctrl_stage
                or (
                    event_phase
                    if event_exact != "none" and event_phase != "none"
                    else None
                )
                or fields.get("stage")
                or event.stage
                or "none"
            )
            line = event.line
    return exact, phase, line


CYW43_RELEASE_PHASE_EXACT = {
    206: "cyw43-release-precondition",
    207: "cyw43-release-reset-vector",
    208: "cyw43-release-armcr4-reset",
    209: "cyw43-release-upload-clock",
    210: "cyw43-release-function2-sideband",
    211: "cyw43-release-ht-clock",
    212: "cyw43-release-function2-enable",
    213: "cyw43-release-interrupt-mask",
    214: "cyw43-release-corecontrol",
    215: "cyw43-release-mailbox-version",
    216: "cyw43-release-firmware-ready",
    217: "cyw43-release-post-ready",
}

CYW43_ARMCR4_RESET_EDGE_EXACT = {
    1: "cyw43-armcr4-prereset-write",
    2: "cyw43-armcr4-prereset-flush",
    3: "cyw43-armcr4-assert-write",
    4: "cyw43-armcr4-in-reset-write",
    5: "cyw43-armcr4-in-reset-flush",
    6: "cyw43-armcr4-clear-write",
    7: "cyw43-armcr4-postreset-write",
    8: "cyw43-armcr4-postreset-flush",
}


def summarize_terminal_cyw43_release_fault(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return the causal CYW43 RELEASE fault until a later release succeeds."""

    terminal: tuple[str, str, int] | None = None
    for event in events:
        raw = event.raw.lower()
        stage = event.fields.get("stage", "").lower()
        status = event.fields.get("status", "").lower()
        if raw.startswith("cyw43_driver_task_command_fault") and stage in {
            "cyw43-firmware-prep",
            "cyw43-firmware-release",
        }:
            blocker = normalize_wifi_exact(
                event.fields.get("reason")
                or event.fields.get("detail")
                or event.raw
            )
            if blocker == "cyw43-release" and stage == "cyw43-firmware-release":
                phase = parse_hex_int(event.fields.get("result")) or 0
                blocker = CYW43_RELEASE_PHASE_EXACT.get(phase, blocker)
            elif blocker == "cyw43-backplane-armcr4-reset":
                result = parse_hex_int(event.fields.get("result")) or 0
                blocker = CYW43_ARMCR4_RESET_EDGE_EXACT.get(
                    result & 0xFF, blocker
                )
            if blocker.startswith(
                ("cyw43-release", "cyw43-post-release-", "cyw43-armcr4-")
            ) or blocker == "cyw43-backplane-armcr4-reset":
                terminal = (blocker, stage, event.line)
            continue
        if terminal is None:
            continue
        if (
            raw.startswith("driver_task_resource_init")
            and event.fields.get("contract", "").lower() == "cyw43455"
            and stage in {"cyw43-firmware-release", "cyw43-function2"}
            and status == "ready"
        ):
            terminal = None
    return terminal


def wifi_address_source(fields: Mapping[str, str]) -> str | None:
    """Return the Wi-Fi address-source field across log schema variants."""

    return fields.get("addr_src") or fields.get("address_source") or fields.get("src")


def wifi_dhcp_phase(fields: Mapping[str, str]) -> str | None:
    """Return the DHCP phase field across log schema variants."""

    return fields.get("dhcp") or fields.get("dhcp_phase")


def wifi_cyw43_status_blocker(fields: Mapping[str, str]) -> tuple[str, str] | None:
    """Return CYW43 data-path status blockers reported by netstats/netstatus."""

    addr_src = wifi_address_source(fields)
    dhcp = wifi_dhcp_phase(fields)
    if addr_src == "wifi-data-rx-admission-blocked" or dhcp == "rx-admission-blocked":
        return "wifi-data-rx-admission-blocked", "dhcp"
    if addr_src == "wifi-rx-overflow" or dhcp == "rx-overflow":
        return "wifi-rx-overflow", "runtime-rx"
    if addr_src == "wifi-rx-starvation" or dhcp == "rx-starvation":
        return "wifi-rx-starvation", "runtime-rx"
    if addr_src == "host-eapol-m3-missing" or dhcp == "host-eapol-m3-missing":
        return "host-eapol-m3-missing", "join-security"
    if addr_src == "wifi-tx-credit-anomaly" or dhcp == "tx-credit-anomaly":
        return "wifi-tx-credit-anomaly", "data-tx"
    return None


def wifi_root_console_net_ready_step(event: TraceEvent) -> bool:
    """Return true when the root-console wait ended because WiFi networking is up."""

    return (
        event.domain == "wifi"
        and "[net-console]" in event.raw.lower()
        and "root console wait complete" in event.raw.lower()
        and field_lower(event, "reason") == "net-ready"
    )


def wifi_fields_active(fields: Mapping[str, str]) -> bool:
    """Return whether a parsed event describes the active Wi-Fi interface."""

    return fields.get("active") == "wifi" or fields.get("evidence") == "active=wifi"


def summarize_wifi_dhcp_frontier(
    events: Iterable[TraceEvent],
) -> tuple[str, str, int] | None:
    """Return the latest explicit Wi-Fi DHCP blocker after secure release."""

    frontier: tuple[str, str, int] | None = None
    for event in events:
        fields = event.fields
        if not wifi_fields_active(fields):
            continue
        addr_src = wifi_address_source(fields)
        dhcp = wifi_dhcp_phase(fields)
        cyw43_status_blocker = wifi_cyw43_status_blocker(fields)
        if cyw43_status_blocker is not None:
            frontier = (cyw43_status_blocker[0], cyw43_status_blocker[1], event.line)
        elif addr_src == "dhcp-pending" and dhcp == "disabled":
            frontier = ("dhcp-not-started", "dhcp", event.line)
        elif addr_src == "dhcp-pending" or dhcp == "selecting":
            frontier = ("dhcp-pending", "dhcp", event.line)
        elif addr_src == "dhcp-failed" or dhcp == "failed":
            frontier = ("dhcp-failed", "dhcp", event.line)
    return frontier


def summarize_wifi_data_path(
    events: Iterable[TraceEvent],
) -> tuple[int, int, int, int, str]:
    """Return CYW43 DHCP/ARP data-path trace counts."""

    tx = 0
    rx_preserved = 0
    rx_delivered = 0
    rx_dropped = 0
    last = "none"
    for event in events:
        raw = event.raw.lower()
        if "cyw43_driver_task_data_path" not in raw:
            continue
        data_event = field_lower(event, "event")
        action = field_lower(event, "action")
        dhcp = field_lower(event, "dhcp")
        arp = field_lower(event, "arp")
        label = "none"
        if dhcp and dhcp != "none":
            label = f"dhcp-{dhcp}"
        elif arp and arp != "none":
            label = f"arp-{arp}"
        if label != "none":
            last = f"{data_event}:{action}:{label}"
        if data_event.startswith("tx"):
            tx += 1
        elif data_event == "rx-preserve":
            rx_preserved += 1
        elif data_event == "rx-deliver":
            rx_delivered += 1
        elif "drop" in data_event:
            rx_dropped += 1
    return tx, rx_preserved, rx_delivered, rx_dropped, last


def summarize_wifi_service_turn(
    events: Iterable[TraceEvent],
) -> tuple[int, int, int, int, int, int, int, int, int, int, int]:
    """Return the latest compact CYW43 linked-runtime service-turn ledger."""

    latest = (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    for event in events:
        if not event.raw.lower().startswith("netstats: wifi_service_turn"):
            continue
        fields = event.fields
        latest = (
            parse_hex_int(fields.get("op")) or 0,
            parse_hex_int(fields.get("reason")) or 0,
            parse_hex_int(fields.get("progress")) or 0,
            parse_hex_int(fields.get("seq")) or 0,
            parse_hex_int(fields.get("credit")) or 0,
            parse_hex_int(fields.get("channel")) or 0,
            parse_hex_int(fields.get("rframe")) or 0,
            parse_hex_int(fields.get("eapol_m1")) or 0,
            parse_hex_int(fields.get("eapol_m2")) or 0,
            parse_hex_int(fields.get("eapol_m3")) or 0,
            parse_hex_int(fields.get("eapol_m4")) or 0,
        )
    return latest


WIFI_RX_IRQ_PRESERVE_REASONS = {
    0: "none",
    1: "sdpcm-next-frame",
    2: "rframe-pending",
    3: "rframe-unavailable",
    4: "rframe-invalid",
    5: "deprecated-source-asserted",
}


def summarize_wifi_rx_irq_preserve(
    events: Iterable[TraceEvent],
) -> tuple[int, str, int, int, int, int]:
    """Return CYW43 frame-interrupt preservation proof from RX trace lines."""

    best_count = 0
    best_reason = "none"
    best_int = 0
    best_ack = 0
    best_episode_preserves = 0
    best_episode_rearms = 0
    best_line = 0
    for event in events:
        raw = event.raw.lower()
        if "cyw43_driver_task_host_eapol_rxtrace" not in raw:
            continue
        count = parse_hex_int(event.fields.get("irq_preserve_count")) or 0
        if count <= 0:
            continue
        if count < best_count or (count == best_count and event.line < best_line):
            continue
        reason_code = parse_hex_int(event.fields.get("irq_preserve_reason")) or 0
        best_count = count
        best_reason = WIFI_RX_IRQ_PRESERVE_REASONS.get(
            reason_code, f"unknown-{reason_code}"
        )
        best_int = parse_hex_int(event.fields.get("irq_preserve_int")) or 0
        best_ack = parse_hex_int(event.fields.get("irq_preserve_ack")) or 0
        best_episode_preserves = (
            parse_hex_int(event.fields.get("irq_episode_preserves")) or 0
        )
        best_episode_rearms = parse_hex_int(event.fields.get("irq_episode_rearms")) or 0
        best_line = event.line
    return (
        best_count,
        best_reason,
        best_int,
        best_ack,
        best_episode_preserves,
        best_episode_rearms,
    )


def summarize_wifi_rxtrace_timing(
    events: Iterable[TraceEvent],
) -> tuple[int, int, int, int, int]:
    """Return the latest CYW43 RX trace sequence and compact timing deltas."""

    best_seq = 0
    best_start = 0
    best_pre_delta = 0
    best_transfer_delta = 0
    best_post_delta = 0
    best_line = 0
    for event in events:
        raw = event.raw.lower()
        if "cyw43_driver_task_host_eapol_rxtrace" not in raw:
            continue
        sequence = parse_hex_int(event.fields.get("trace_seq")) or 0
        if sequence == 0:
            continue
        if sequence < best_seq or (sequence == best_seq and event.line < best_line):
            continue
        best_seq = sequence
        best_start = parse_hex_int(event.fields.get("start_ticks_lo")) or 0
        best_pre_delta = parse_hex_int(event.fields.get("pre_sample_delta_ticks")) or 0
        best_transfer_delta = parse_hex_int(event.fields.get("transfer_delta_ticks")) or 0
        best_post_delta = parse_hex_int(event.fields.get("post_sample_delta_ticks")) or 0
        best_line = event.line
    return best_seq, best_start, best_pre_delta, best_transfer_delta, best_post_delta


def _truthy_field(value: str | None) -> bool:
    """Return whether a normalized field value represents true/non-zero."""

    if value is None:
        return False
    return value.lower() not in {"", "0", "false", "no", "none", "n/a"}


DRIVER_TASK_COUNTER_REQUIRED_FIELDS = (
    "contract",
    "hot_path",
    "source",
    "sequence",
    "submitted",
    "completed",
    "idle",
    "fault",
    "budget",
    "frame",
    "desc",
    "staged_bytes",
    "clean_ops",
    "clean_bytes",
    "inv_ops",
    "inv_bytes",
    "sends",
    "yields",
    "busy",
    "same_request",
    "timeouts",
    "keep_active",
    "aborts",
    "rx_frames",
    "rx_bytes",
    "tx_frames",
    "tx_bytes",
)
DRIVER_TASK_COUNTER_ACTIVITY_FIELDS = DRIVER_TASK_COUNTER_REQUIRED_FIELDS[3:]
DRIVER_TASK_COUNTER_OPTIONAL_ACTIVITY_FIELDS = ("overruns", "drops")


@dataclass(frozen=True)
class DriverTaskCounterSummary:
    """Aggregate non-authority driver-task counter evidence."""

    snapshots: int = 0
    invalid: int = 0
    busy: int = 0
    same_request: int = 0
    timeouts: int = 0
    keep_active: int = 0
    aborts: int = 0
    overruns: int = 0
    drops: int = 0
    staged_bytes: int = 0
    cache_ops: int = 0
    cache_bytes: int = 0
    rx_frames: int = 0
    tx_frames: int = 0
    rx_bytes: int = 0
    tx_bytes: int = 0


@dataclass(frozen=True)
class OutputPressureSummary:
    """Latest serial/HDMI output pressure counters from console diagnostics."""

    serial_tx_pending: str = "unknown"
    serial_interactive: str = "unknown"
    serial_deferred: int = 0
    serial_flushed: int = 0
    serial_backpressure: int = 0
    hdmi_pending_bytes: int = 0
    hdmi_pending_redraw: str = "unknown"
    hdmi_submitted: int = 0
    hdmi_deferred: int = 0
    hdmi_busy: int = 0
    hdmi_no_reply: int = 0
    hdmi_coalesced: int = 0
    hdmi_backpressure_bytes: int = 0
    hdmi_superseded_bytes: int = 0


@dataclass(frozen=True)
class UsbKeyboardPressureSummary:
    """Latest USB local-seat no-reply pressure counters."""

    no_replies: int = 0
    poll_cooldown: int = 0
    cooldown_skips: int = 0


@dataclass(frozen=True)
class UsbRuntimeQueueSummary:
    """Latest USB runtime queue and sustained-input counters."""

    queued_reports: int = 0
    transfer_events: int = 0
    report_status: str = "unknown"
    queue_valid: str = "unknown"
    doorbell_pending: str = "unknown"
    recovery_diag_valid: str = "unknown"
    endpoint_recoveries: int = 0
    endpoint_recovery_failures: int = 0
    queue_collapse_recoveries: int = 0
    recovery_stage: str = "unknown"
    recovery_reason: str = "unknown"
    command_completion_blocked: int = 0
    runtime_skipped: int = 0
    startup_blocker_seen: bool = False
    active_blocker_seen: bool = False
    recovered_from_blocker: bool = False
    recovery_state: str = "unknown"
    command_ready: bool = False
    first_report_ready: bool = False
    first_byte_ready: bool = False
    busy_after_ready: bool = False
    local_seat_reason: str = "unknown"


def update_usb_keyboard_pressure_field(
    values: dict[str, int],
    fields: Mapping[str, str],
    out_key: str,
    in_key: str,
) -> None:
    """Update one USB keyboard-pressure field when present."""

    parsed = parse_hex_int(fields.get(in_key))
    if parsed is not None:
        values[out_key] = parsed


def usb_hid_interrupt_no_completion_seen(fields: Mapping[str, str]) -> bool:
    """Return true for a full HID interrupt queue that has produced no events."""

    queued_reports = parse_hex_int(fields.get("queued_reports"))
    transfer_events = parse_hex_int(fields.get("transfer_events"))
    doorbell_pending = (
        fields.get("doorbell_pending") or fields.get("doorbell") or ""
    ).lower()
    report_status = fields.get("report_status", "none").lower().replace("_", "-")
    full_idle_queue = fields.get("full_idle_queue", "").lower() in {"1", "true", "yes"}
    pre_first_report_no_completion = fields.get(
        "pre_first_report_no_completion", ""
    ).lower() in {"1", "true", "yes"}
    if (
        queued_reports is not None
        and queued_reports >= 32
        and pre_first_report_no_completion
        and doorbell_pending in {"", "0", "false", "no"}
    ):
        return True
    return (
        queued_reports is not None
        and queued_reports >= 32
        and (transfer_events == 0 or (transfer_events is None and full_idle_queue))
        and doorbell_pending in {"", "0", "false", "no"}
        and report_status in {"none", "idle", "idle-report", "decoded-empty"}
    )


def usb_runtime_hid_interrupt_no_completion(
    runtime: UsbRuntimeQueueSummary,
) -> bool:
    """Return true when latest runtime queue evidence proves a stuck HID IN path."""

    return (
        not runtime.first_byte_ready
        and runtime.queued_reports >= 32
        and runtime.transfer_events == 0
        and runtime.doorbell_pending in {"unknown", "", "0", "false", "no"}
        and runtime.report_status in {"unknown", "none", "idle", "idle-report", "decoded-empty"}
    )


def refine_usb_gate_for_runtime_truth(
    usb_gate: int,
    usb_blocker: str,
    runtime: UsbRuntimeQueueSummary,
) -> tuple[int, str, UsbRuntimeQueueSummary]:
    """Downgrade impossible USB-ready combinations to the current blocker."""

    if usb_runtime_hid_interrupt_no_completion(runtime):
        active_after_ready = runtime.command_ready or runtime.active_blocker_seen
        refined_runtime = replace(
            runtime,
            command_ready=False,
            active_blocker_seen=active_after_ready,
            recovery_state="degraded-active" if active_after_ready else runtime.recovery_state,
            local_seat_reason="usb-hid-interrupt-no-completion",
            busy_after_ready=active_after_ready or runtime.busy_after_ready,
        )
        return 8, "usb-hid-interrupt-no-completion", refined_runtime
    if (
        runtime.command_ready
        and not runtime.first_report_ready
        and not runtime.first_byte_ready
    ):
        refined_runtime = replace(
            runtime,
            command_ready=False,
            active_blocker_seen=True,
            recovery_state="degraded-active",
            local_seat_reason="usb-first-report-missing",
            busy_after_ready=True,
        )
        return min(usb_gate, 8), "usb-hid-interrupt-no-completion", refined_runtime
    return usb_gate, usb_blocker, runtime


def usb_runtime_active_blocker_seen(raw: str, fields: Mapping[str, str]) -> bool:
    """Return whether a USB runtime line is active degraded evidence."""

    queue_valid = fields.get("queue_valid", "").lower()
    if queue_valid == "no":
        return True
    if usb_hid_interrupt_no_completion_seen(fields):
        return True
    doorbell_pending = fields.get("doorbell_pending", "").lower()
    if doorbell_pending in {"1", "true", "yes"}:
        return True
    if fields.get("recovery_aux_pending", "").lower() == "yes":
        return True
    report_status = fields.get("report_status", "").lower().replace("_", "-")
    if report_status in {
        "queue-collapse",
        "recovery-failed",
        "unmatched-transfer",
    }:
        return True

    action = fields.get("action", "").lower().replace("_", "-")
    if raw.startswith("usb: recovery_request") and action in {"no-reply", "recover"}:
        return True

    blocker = normalize_usb_blocker(fields.get("blocker", "none"))
    if blocker in {
        "",
        "none",
        "hid-first-report",
        "command-input-ready",
        "keyboard-first-byte",
        "first-console-byte",
        "awaiting-physical-key",
    }:
        blocker = "none"
    if blocker != "none":
        return True

    if raw.startswith("usb: runtime_gate"):
        keyboard = fields.get("keyboard", "").lower()
        if keyboard == "no":
            return True
    return False


def usb_runtime_active_blocker_event(event: TraceEvent) -> bool:
    """Return whether an event is USB-scoped active degraded evidence."""

    raw = event.raw.lower()
    contract = event.fields.get("contract", "").lower()
    if event.domain not in {"usb", "driver"}:
        return False
    if event.domain == "driver" and contract != "usb-local-seat":
        return False
    if event.domain == "usb" and (
        raw.startswith("wifi:")
        or raw.startswith("cyw43_")
        or "[cyw43]" in raw
        or "[pi4-wifi]" in raw
    ):
        return False
    return usb_runtime_active_blocker_seen(raw, event.fields)


def summarize_usb_keyboard_pressure(
    events: Iterable[TraceEvent],
) -> UsbKeyboardPressureSummary:
    """Return latest USB local-seat no-reply/cooldown counters."""

    summary = UsbKeyboardPressureSummary()
    values = summary.__dict__.copy()
    for event in events:
        raw = event.raw.lower()
        fields = event.fields
        if (
            raw.startswith("usb: local-seat drops")
            or raw.startswith("usb: stall_telemetry")
            or raw.startswith("usb: keyboard_trace")
            or raw.startswith("usb: sustained_input")
            or raw.startswith("usb: recovery_request")
            or raw.startswith("[smp] activity local-seat ")
        ):
            update_usb_keyboard_pressure_field(
                values, fields, "no_replies", "driver_task_no_replies"
            )
            update_usb_keyboard_pressure_field(
                values, fields, "no_replies", "no_reply"
            )
            update_usb_keyboard_pressure_field(
                values, fields, "poll_cooldown", "poll_cooldown"
            )
            update_usb_keyboard_pressure_field(
                values, fields, "poll_cooldown", "cooldown"
            )
            update_usb_keyboard_pressure_field(
                values, fields, "cooldown_skips", "cooldown_skips"
            )
    return UsbKeyboardPressureSummary(**values)


def summarize_usb_runtime_queue(events: Iterable[TraceEvent]) -> UsbRuntimeQueueSummary:
    """Return latest USB runtime queue and event-loop counters."""

    summary = UsbRuntimeQueueSummary()
    values = summary.__dict__.copy()
    command_ready_seen = False
    for event in events:
        raw = event.raw.lower()
        fields = event.fields
        active_blocker_seen = usb_runtime_active_blocker_event(event)
        if active_blocker_seen:
            if command_ready_seen:
                values["active_blocker_seen"] = True
                values["recovery_state"] = "degraded-active"
            else:
                values["startup_blocker_seen"] = True
        if usb_command_ready_step(event):
            command_ready_seen = True
            values["command_ready"] = True
            if "[local-seat] usb keyboard command-ready" in raw:
                values["first_report_ready"] = True
            if (
                "pi4 keyboard runtime proof" in raw
                and field_lower(event, "result") in {"online", "ready"}
                and (parse_hex_int(event.fields.get("gate")) or 0) >= 10
            ):
                values["first_report_ready"] = True
            if values["active_blocker_seen"]:
                values["recovered_from_blocker"] = True
                values["recovery_state"] = "ready-recovered"
                values["local_seat_reason"] = "recovered-from-blocker"
            else:
                values["recovery_state"] = "ready-clean"
                values["local_seat_reason"] = "command-ready"
        if usb_first_report_step(event):
            values["first_report_ready"] = True
        if usb_first_byte_step(event):
            values["first_byte_ready"] = True
            if command_ready_seen and not values["active_blocker_seen"]:
                values["busy_after_ready"] = False
        if raw.startswith("usb: runtime_gate") or raw.startswith("usb: acceptance"):
            first_report = field_lower(event, "first_report")
            if first_report == "yes":
                values["first_report_ready"] = True
            elif first_report == "no":
                values["first_report_ready"] = False
            if field_lower(event, "first_byte") == "yes":
                values["first_byte_ready"] = True
                if command_ready_seen and not values["active_blocker_seen"]:
                    values["busy_after_ready"] = False
            if field_lower(event, "command_ready") == "yes":
                command_ready_seen = True
                values["command_ready"] = True
        if sustained_input_progress_proof(raw, event.fields):
            if command_ready_seen and not values["active_blocker_seen"]:
                values["busy_after_ready"] = False
        if command_ready_seen and (
            "usb console busy" in raw
            or "usb keyboard command-deferred" in raw
            or active_blocker_seen
        ):
            values["busy_after_ready"] = True
        if (
            raw.startswith("usb: runtime_queue")
            or raw.startswith("usb: stall_telemetry")
            or raw.startswith("usb: sustained_input")
            or raw.startswith("usb: recovery_request")
        ):
            queued_reports = parse_hex_int(fields.get("queued_reports"))
            if queued_reports is not None:
                values["queued_reports"] = queued_reports
            transfer_events = parse_hex_int(fields.get("transfer_events"))
            if transfer_events is not None:
                values["transfer_events"] = transfer_events
            report_status = field_lower(event, "report_status")
            if report_status:
                values["report_status"] = report_status.replace("_", "-")
            queue_valid = field_lower(event, "queue_valid")
            if queue_valid:
                values["queue_valid"] = queue_valid
            doorbell_pending = field_lower(event, "doorbell_pending")
            if doorbell_pending:
                values["doorbell_pending"] = doorbell_pending
        if raw.startswith("usb: runtime_recovery"):
            diag_valid = field_lower(event, "diag_valid")
            if diag_valid == "unknown" and values["recovery_diag_valid"] == "yes":
                continue
            if diag_valid:
                values["recovery_diag_valid"] = diag_valid
            recoveries = parse_hex_int(fields.get("recoveries"))
            if recoveries is not None:
                values["endpoint_recoveries"] = recoveries
            failures = parse_hex_int(fields.get("failures"))
            if failures is not None:
                values["endpoint_recovery_failures"] = failures
            queue_collapse = parse_hex_int(fields.get("queue_collapse"))
            if queue_collapse is not None:
                values["queue_collapse_recoveries"] = queue_collapse
            stage = field_lower(event, "stage")
            if stage:
                values["recovery_stage"] = stage.replace("_", "-")
            reason = field_lower(event, "reason")
            if reason:
                values["recovery_reason"] = reason.replace("_", "-")
            blocked = parse_hex_int(fields.get("command_completion_blocked"))
            if blocked is not None:
                values["command_completion_blocked"] = blocked
        if raw.startswith("usb: event_loop") or raw.startswith("usb: sustained_input"):
            runtime_skipped = parse_hex_int(fields.get("runtime_skipped"))
            if runtime_skipped is not None:
                values["runtime_skipped"] = runtime_skipped
    if values["recovery_state"] == "unknown" and values["active_blocker_seen"]:
        values["recovery_state"] = "degraded-active"
        values["local_seat_reason"] = values["report_status"]
    if values["recovery_state"] == "unknown":
        if values["command_ready"]:
            values["recovery_state"] = "ready-clean"
            values["local_seat_reason"] = "command-ready"
        elif values["active_blocker_seen"]:
            values["recovery_state"] = "degraded-active"
        else:
            values["recovery_state"] = "unknown"
    return UsbRuntimeQueueSummary(**values)


def usb_local_seat_state_from_runtime(
    usb_gate: int,
    usb_blocker: str,
    runtime: UsbRuntimeQueueSummary,
    post_first_byte_blocker: str,
) -> tuple[str, str, bool]:
    """Return user-facing local-seat readiness state, reason, and busy-after-ready."""

    busy_after_ready = runtime.busy_after_ready
    if usb_gate < 10:
        return "blocked", usb_blocker or "usb-gate-incomplete", busy_after_ready
    if usb_runtime_recovery_diag_invalid_degrades(runtime):
        return "degraded", "usb-runtime-diag-invalid", runtime.command_ready or busy_after_ready
    if not runtime.first_report_ready:
        return "degraded", "usb-first-report-missing", runtime.command_ready or busy_after_ready
    if not runtime.command_ready:
        return "degraded", "usb-command-ready-missing", busy_after_ready
    if post_first_byte_blocker != "none":
        return "degraded", post_first_byte_blocker, runtime.command_ready or busy_after_ready
    if runtime.busy_after_ready:
        return "degraded", "usb-post-ready-busy", True
    if runtime.recovery_state == "degraded-active":
        reason = runtime.local_seat_reason
        if reason in {"", "unknown", "none"}:
            reason = "usb-runtime-degraded"
        return "degraded", reason, busy_after_ready
    if runtime.recovered_from_blocker or runtime.recovery_state == "ready-recovered":
        reason = runtime.local_seat_reason
        if reason in {"", "unknown", "none"}:
            reason = "recovered-from-blocker"
        return "recovered", reason, busy_after_ready
    return "ready", "command-ready", busy_after_ready


def usb_runtime_recovery_diag_invalid_degrades(runtime: UsbRuntimeQueueSummary) -> bool:
    """Return whether a missing recovery diagnostic invalidates local-seat proof."""

    if runtime.recovery_diag_valid != "no":
        return False
    healthy_report_status = {
        "idle-report",
        "no-idle-report",
        "decoded-empty",
        "produced-byte",
        "filtered-key",
        "flexible-fallback",
    }
    if (
        runtime.command_ready
        and runtime.first_report_ready
        and runtime.report_status in healthy_report_status
        and (runtime.transfer_events != 0 or runtime.report_status == "no-idle-report")
        and runtime.endpoint_recovery_failures == 0
        and runtime.queue_collapse_recoveries == 0
        and runtime.command_completion_blocked == 0
        and runtime.recovery_stage in {"unknown", "none", "ready"}
        and runtime.recovery_reason in {"unknown", "none"}
    ):
        return False
    return True


def summarize_output_pressure(events: Iterable[TraceEvent]) -> OutputPressureSummary:
    """Return latest output-pressure counters split by serial and HDMI."""

    summary = OutputPressureSummary()
    values = summary.__dict__.copy()
    for event in events:
        raw = event.raw.lower()
        fields = event.fields
        if raw.startswith("usb: output_pressure"):
            values["serial_tx_pending"] = fields.get(
                "serial_tx_pending", values["serial_tx_pending"]
            )
            values["serial_interactive"] = fields.get(
                "serial_interactive", values["serial_interactive"]
            )
            for out_key, in_key in (
                ("serial_deferred", "deferred"),
                ("serial_flushed", "flushed"),
                ("serial_backpressure", "backpressure"),
                ("hdmi_pending_bytes", "hdmi_pending_bytes"),
                ("hdmi_submitted", "hdmi_submitted"),
                ("hdmi_deferred", "hdmi_deferred"),
                ("hdmi_busy", "hdmi_busy"),
                ("hdmi_no_reply", "hdmi_no_reply"),
                ("hdmi_coalesced", "hdmi_coalesced"),
                ("hdmi_backpressure_bytes", "hdmi_backpressure_bytes"),
                ("hdmi_superseded_bytes", "hdmi_superseded_bytes"),
            ):
                parsed = parse_hex_int(fields.get(in_key))
                if parsed is not None:
                    values[out_key] = parsed
            values["hdmi_pending_redraw"] = fields.get(
                "hdmi_pending_redraw", values["hdmi_pending_redraw"]
            )
        elif raw.startswith("[smp] activity local-seat-display"):
            for out_key, in_key in (
                ("hdmi_pending_bytes", "pending_bytes"),
                ("hdmi_submitted", "submitted"),
                ("hdmi_deferred", "deferred"),
                ("hdmi_busy", "busy"),
                ("hdmi_no_reply", "no_reply"),
                ("hdmi_coalesced", "coalesced"),
                ("hdmi_backpressure_bytes", "backpressure_bytes"),
                ("hdmi_superseded_bytes", "superseded_bytes"),
            ):
                parsed = parse_hex_int(fields.get(in_key))
                if parsed is not None:
                    values[out_key] = parsed
            values["hdmi_pending_redraw"] = fields.get(
                "pending_redraw", values["hdmi_pending_redraw"]
            )
        elif raw.startswith("hdmi_frame_queue"):
            for out_key, in_key in (
                ("hdmi_pending_bytes", "pending_bytes"),
                ("hdmi_submitted", "submitted"),
                ("hdmi_deferred", "deferred"),
                ("hdmi_busy", "busy"),
                ("hdmi_no_reply", "no_reply"),
            ):
                parsed = parse_hex_int(fields.get(in_key))
                if parsed is not None:
                    values[out_key] = parsed
            values["hdmi_pending_redraw"] = fields.get(
                "pending_redraw", values["hdmi_pending_redraw"]
            )
        elif raw.startswith("hdmi_frame_counters"):
            for out_key, in_key in (
                ("hdmi_coalesced", "coalesced"),
                ("hdmi_backpressure_bytes", "backpressure_bytes"),
                ("hdmi_superseded_bytes", "superseded_bytes"),
            ):
                parsed = parse_hex_int(fields.get(in_key))
                if parsed is not None:
                    values[out_key] = parsed
    return OutputPressureSummary(**values)


def summarize_driver_task_counters(
    events: Iterable[TraceEvent],
) -> DriverTaskCounterSummary:
    """Return bounded diagnostic counter totals from DRIVER_TASK_COUNTER lines."""

    snapshots = 0
    invalid = 0
    busy = 0
    same_request = 0
    timeouts = 0
    keep_active = 0
    aborts = 0
    overruns = 0
    drops = 0
    staged_bytes = 0
    cache_ops = 0
    cache_bytes = 0
    rx_frames = 0
    tx_frames = 0
    rx_bytes = 0
    tx_bytes = 0
    for event in events:
        if not event.raw.lower().startswith("driver_task_counter "):
            continue
        snapshots += 1
        fields = event.fields
        missing_required = any(
            field not in fields for field in DRIVER_TASK_COUNTER_REQUIRED_FIELDS
        )
        parsed = {
            field: parse_hex_int(fields.get(field))
            for field in DRIVER_TASK_COUNTER_ACTIVITY_FIELDS
        }
        optional = {
            field: parse_hex_int(fields.get(field)) or 0
            for field in DRIVER_TASK_COUNTER_OPTIONAL_ACTIVITY_FIELDS
        }
        bad_numeric = any(value is None for value in parsed.values())
        source = fields.get("source", "").lower()
        no_activity = not any((value or 0) != 0 for value in parsed.values()) and not any(
            value != 0 for value in optional.values()
        )
        if missing_required or bad_numeric or source != "root-ring" or no_activity:
            invalid += 1
            continue
        busy += parsed["busy"] or 0
        same_request += parsed["same_request"] or 0
        timeouts += parsed["timeouts"] or 0
        keep_active += parsed["keep_active"] or 0
        aborts += parsed["aborts"] or 0
        overruns += optional["overruns"]
        drops += optional["drops"]
        staged_bytes += parsed["staged_bytes"] or 0
        cache_ops += (parsed["clean_ops"] or 0) + (parsed["inv_ops"] or 0)
        cache_bytes += (parsed["clean_bytes"] or 0) + (parsed["inv_bytes"] or 0)
        rx_frames += parsed["rx_frames"] or 0
        tx_frames += parsed["tx_frames"] or 0
        rx_bytes += parsed["rx_bytes"] or 0
        tx_bytes += parsed["tx_bytes"] or 0
    return DriverTaskCounterSummary(
        snapshots=snapshots,
        invalid=invalid,
        busy=busy,
        same_request=same_request,
        timeouts=timeouts,
        keep_active=keep_active,
        aborts=aborts,
        overruns=overruns,
        drops=drops,
        staged_bytes=staged_bytes,
        cache_ops=cache_ops,
        cache_bytes=cache_bytes,
        rx_frames=rx_frames,
        tx_frames=tx_frames,
        rx_bytes=rx_bytes,
        tx_bytes=tx_bytes,
    )


def summarize_net_state(events: Iterable[TraceEvent]) -> tuple[str, str, str, bool, bool]:
    """Return the latest compact netstats/netstatus state."""

    active = "unknown"
    addr_src = "unknown"
    dhcp = "unknown"
    dhcp_interface = "unknown"
    tcp_ready = False
    host_tcp_proof = False
    nettest_proof = False
    for event in events:
        raw = event.raw.lower()
        detail = field_lower(event, "detail")
        if "[cohsh-net][auth] auth ok" in raw or "[net-console] auth ok" in raw:
            host_tcp_proof = True
            tcp_ready = True
        if raw.startswith("ok nettest") and (detail == "pass" or "success" in raw):
            nettest_proof = True
        if raw.startswith("err nettest") or "detail=net-disabled" in raw:
            host_tcp_proof = False
            tcp_ready = False
            nettest_proof = False
        if raw_has(event, "[net-selftest] result", "tx_ok=true", "console_ok=true") or (
            "[net-selftest] result" in raw
            and field_lower(event, "result") in {"pass", "peer-assisted-pass"}
        ):
            nettest_proof = True
        if raw.startswith("[dhcp]") or "[dhcp]" in raw:
            interface = event.fields.get("interface")
            if interface:
                dhcp_interface = interface
            if "[dhcp] lease bound" in raw:
                active = (
                    dhcp_interface
                    if dhcp_interface != "unknown"
                    else ("wifi" if event.domain == "wifi" else active)
                )
                addr_src = "dhcp-lease"
                dhcp = "bound"
                continue
        if not event.raw.lower().startswith(
            ("netstats:", "netstatus:", "[smp] activity net")
        ):
            continue
        active = event.fields.get("active", active)
        addr_src = event.fields.get("addr_src", event.fields.get("src", addr_src))
        dhcp = event.fields.get("dhcp", dhcp)
        tcp_ready_field = field_lower(event, "tcp_ready")
        if tcp_ready_field == "yes":
            tcp_ready = True
        elif tcp_ready_field == "no":
            tcp_ready = host_tcp_proof
    return active, addr_src, dhcp, tcp_ready, nettest_proof


def summarize_cohsh_tcp_auth(events: Iterable[TraceEvent]) -> tuple[bool, int, int, int]:
    """Return same-boot authenticated TCP counters, separate from nettest."""

    auth_line_seen = False
    accepts = 0
    auth_sessions = 0
    rx_bytes = 0
    for event in events:
        raw = event.raw.lower()
        if "[cohsh-net][auth] auth ok" in raw or "[net-console] auth ok" in raw:
            auth_line_seen = True
        if not (
            raw.startswith("netstats:")
            or "wifi: gate 10" in raw
            or "wifi: acceptance" in raw
        ):
            continue
        accepts = max(accepts, parse_hex_int(event.fields.get("tcp_accepts")) or 0)
        auth_sessions = max(
            auth_sessions, parse_hex_int(event.fields.get("tcp_auth")) or 0
        )
        rx_bytes = max(rx_bytes, parse_hex_int(event.fields.get("tcp_rx_bytes")) or 0)
    proof = accepts > 0 and auth_sessions > 0 and rx_bytes > 0
    return proof or (auth_line_seen and rx_bytes > 0), accepts, auth_sessions, rx_bytes


def classify_driver_task_role(label: str) -> str | None:
    """Classify a driver-task label into a reopened 26a/26b proof role."""

    normalized = label.lower().replace("_", "-")
    if "serial" in normalized or "uart" in normalized:
        return "serial"
    if (
        "usb" in normalized
        or "xhci" in normalized
        or "hid" in normalized
        or "local-seat" in normalized
        or "keyboard" in normalized
    ):
        return "usb"
    if (
        "hdmi" in normalized
        or "display" in normalized
        or "framebuffer" in normalized
        or normalized in {"fb", "video"}
    ):
        return "display"
    if any(
        token in normalized
        for token in (
            "bcmgenet",
            "genet",
            "cyw",
            "43455",
            "wifi",
            "wireless",
            "wired",
            "ethernet",
            "rtl8139",
            "virtio-net",
            "nic",
        )
    ):
        return "net"
    if "sdio" in normalized or "sdhci" in normalized or "mmc" in normalized:
        return "sdio"
    if "pcie" in normalized or "pci-root" in normalized or "vl805" in normalized:
        return "pcie"
    return None


REQUIRED_DRIVER_TASK_OWNER_HOT_PATHS = {
    "serial-console",
    "usb-keyboard",
    "hdmi-text",
    "genet-nic",
    "cyw43-wifi",
    "sdio-host",
    "pcie-root",
}

BASE_DRIVER_TASK_OWNER_HOT_PATHS = {
    "serial-console",
    "usb-keyboard",
    "hdmi-text",
    "pcie-root",
}

BASE_DRIVER_TASK_ROLES = {"serial", "usb", "display", "pcie"}


def normalize_active_net_selection(selected_net: str, active_net: str) -> str:
    """Return the selected concrete Pi 4 network owner label."""

    normalized_active = active_net.lower().replace("_", "-")
    if normalized_active in {"cyw43", "genet", "none"}:
        return normalized_active
    normalized_selected = selected_net.lower().replace("_", "-")
    if normalized_selected in {"wifi", "wireless", "cyw43", "cyw43-wifi"}:
        return "cyw43"
    if normalized_selected in {"wired", "ethernet", "genet", "genet-nic", "bcmgenet-v5"}:
        return "genet"
    if normalized_selected in {"disabled", "none", "off"}:
        return "none"
    return "unknown"


def required_driver_task_owner_hot_paths(
    selected_net: str,
    active_net: str,
) -> set[str]:
    """Return owner-state hot paths required for the selected Pi 4 network."""

    normalized = normalize_active_net_selection(selected_net, active_net)
    required = set(BASE_DRIVER_TASK_OWNER_HOT_PATHS)
    if normalized == "cyw43":
        required.update({"cyw43-wifi", "sdio-host"})
    elif normalized == "genet":
        required.add("genet-nic")
    elif normalized == "unknown":
        required = set(REQUIRED_DRIVER_TASK_OWNER_HOT_PATHS)
    return required


def required_driver_task_roles(selected_net: str, active_net: str) -> set[str]:
    """Return dedicated driver roles required for the selected Pi 4 network."""

    normalized = normalize_active_net_selection(selected_net, active_net)
    required = set(BASE_DRIVER_TASK_ROLES)
    if normalized == "cyw43":
        required.update({"net", "sdio"})
    elif normalized == "genet":
        required.add("net")
    elif normalized == "unknown":
        required.update({"net", "sdio"})
    return required


def classify_owner_state_hot_path(fields: dict[str, str]) -> str | None:
    """Classify a DRIVER_TASK_OWNER_STATE line into a concrete Pi 4 hot path."""

    value = fields.get("hot_path")
    if value:
        normalized = value.lower().replace("_", "-")
        if normalized in REQUIRED_DRIVER_TASK_OWNER_HOT_PATHS:
            return normalized
    return None


def _pointer_free_ipc_proven(fields: dict[str, str]) -> bool:
    explicit = fields.get("pointer_free_ipc")
    if explicit is not None:
        return explicit.lower() in {"yes", "pass", "true", "1"}
    return fields.get("ipc_abi", "").lower() in {
        "pointer-free",
        "ring-command",
        "shared-ring-command",
    }


def _owner_state_proven(fields: dict[str, str]) -> bool:
    explicit = fields.get("owner_state")
    if explicit is None:
        return False
    return explicit.lower() == "driver-owned"


RUNTIME_DESCRIPTOR_SEAL_VERSION = 4
SPLIT_RUNTIME_DESCRIPTOR_SEAL_HOT_PATHS = frozenset(
    {"usb-keyboard", "cyw43-wifi", "sdio-host"}
)


def driver_runtime_descriptor_seal_status(fields: dict[str, str]) -> str:
    """Return a stable status for sealed runtime descriptor evidence fields."""

    hot_path = classify_owner_state_hot_path(fields)
    if hot_path is None:
        return "unknown-hot-path"
    version = parse_hex_int(fields.get("descriptor_version"))
    if version != RUNTIME_DESCRIPTOR_SEAL_VERSION:
        return "descriptor-version-missing"
    if fields.get("descriptor_seal", "").lower() != "valid":
        return "descriptor-seal-missing"
    if fields.get("artifact_hash", "").lower() not in {"nonzero", "present", "valid"}:
        return "artifact-hash-missing"
    bus_link_seal = fields.get("bus_link_seal", "").lower()
    if hot_path in SPLIT_RUNTIME_DESCRIPTOR_SEAL_HOT_PATHS:
        if bus_link_seal != "valid":
            return "bus-link-seal-missing"
    elif bus_link_seal not in {"none", "valid"}:
        return "bus-link-seal-missing"
    return "ready"


def driver_runtime_descriptor_seal_proven(fields: dict[str, str]) -> bool:
    """Return whether one proof line carries the sealed descriptor contract."""

    return driver_runtime_descriptor_seal_status(fields) == "ready"


def summarize_driver_task_proofs(
    events: Iterable[TraceEvent],
) -> tuple[
    bool,  # default_requested
    bool,  # live_hot_paths
    int,  # contracts
    int,  # dedicated_contracts
    int,  # compatibility_contracts
    bool,  # dedicated_ready
    bool,  # serial dedicated
    bool,  # usb dedicated
    bool,  # display dedicated
    bool,  # net dedicated
    bool,  # sdio dedicated
    bool,  # pcie dedicated
    bool,  # substrate_ready
    int,  # failed_count
    bool,  # capset_proof
    bool,  # fault_proof
    bool,  # revoke_proof
    bool,  # sched_proof
    bool,  # affinity_proof
    int,  # affinity_configured
    int,  # affinity_applied
    bool,  # affinity manifest proof
    int,  # affinity manifest matches
    int,  # affinity manifest missing
    int,  # affinity manifest mismatches
    bool,  # vspace_proof
    bool,  # pointer_free_ipc_proof
    bool,  # owner_state_proof
    str,  # active_net
    int,  # budget_overruns
    int,  # latency_proofs
    bool,  # serial_responsive
    bool,  # usb_burst_proof
    int,  # usb_burst_drops
    bool,  # hdmi_responsive
]:
    """Summarize Pi 4 driver-task and responsiveness proof breadcrumbs."""

    default_requested = False
    live_hot_paths = False
    contracts: set[str] = set()
    dedicated_contracts: set[str] = set()
    compatibility_contracts: set[str] = set()
    dedicated_hot_roles: set[str] = set()
    dedicated_ready_claimed = False
    acceptance_compatibility_count: int | None = None
    substrate_ready = False
    failed_count = 0
    capset_proof = False
    fault_proof = False
    revoke_proof = False
    sched_proof = False
    affinity_proof = False
    affinity_configured = 0
    affinity_applied = 0
    affinity_manifest_matches: set[str] = set()
    affinity_manifest_mismatches = 0
    vspace_proof = False
    pointer_free_ipc_proof = False
    owner_state_hot_paths: set[str] = set()
    active_net = "unknown"
    selected_net = "unknown"
    selected_only = False
    budget_overruns = 0
    latency_proofs = 0
    serial_responsive = False
    usb_burst_proof = False
    usb_burst_drops = -1
    hdmi_responsive = False
    for event in events:
        raw = event.raw.lower()
        fields = event.fields
        driver_task_line = (
            "driver_task" in raw
            or "driver-task" in raw
            or "sched_contract" in raw
            or "budget_overrun" in raw
            or "budget overrun" in raw
        )
        if "active_contracts" in fields:
            selected_only |= fields.get("active_contracts", "").lower() == "selected-only"
            selected_net = fields.get("net", selected_net).lower()
        if driver_task_line:
            owner_state_line = "driver_task_owner_state" in raw
            if raw.startswith("driver_task_boot ") or raw.startswith(
                "driver_task_boot_smoke "
            ):
                contract = fields.get("contract")
                if contract in DRIVER_TASK_EXPECTED_AFFINITY_CORES:
                    expected_core = DRIVER_TASK_EXPECTED_AFFINITY_CORES[contract]
                    affinity_core = parse_hex_int(fields.get("affinity_core"))
                    if affinity_core == expected_core:
                        affinity_manifest_matches.add(contract)
                    else:
                        affinity_manifest_mismatches += 1
            if "driver_task_selected" in raw:
                selected_net = fields.get("selection", selected_net).lower()
                active_net = fields.get("active_net", active_net).lower()
            if (
                "net_driver_task_replay_status" in raw
                and fields.get("selected", "").lower() == "yes"
            ):
                role = field_lower(event, "role")
                if role in {"cyw43-wifi", "cyw43455"}:
                    selected_net = "wifi"
                    active_net = "cyw43"
                elif role in {"genet-nic", "bcmgenet-v5", "genet"}:
                    selected_net = "wired"
                    active_net = "genet"
            if "driver_task_default" in raw:
                default_requested |= fields.get("requested", "").lower() in {
                    "dedicated",
                    "dedicated-sel4-task",
                    "yes",
                }
                live_hot_paths |= _truthy_field(fields.get("live_hot_paths"))
            contract_declaration_line = (
                (
                    "sched_contract" in raw
                    or raw.startswith("driver_task ")
                    or raw.startswith("driver-task ")
                )
                and "driver_task_acceptance" not in raw
                and "driver_task_summary" not in raw
            )
            contract_names: set[str] = set()
            if not owner_state_line:
                for key in ("contract", "name", "task", "driver"):
                    value = fields.get(key)
                    if value:
                        contracts.add(value)
                        contract_names.add(value)
                if not contract_names:
                    value = fields.get("role")
                    if value:
                        contracts.add(value)
                        contract_names.add(value)
            if contract_declaration_line and not contract_names:
                contracts.add("unnamed")
                contract_names.add("unnamed")
            isolation = fields.get("isolation", "").lower()
            dedicated = isolation in {"dedicated-sel4-task", "dedicated", "sel4-task"}
            compatibility = isolation in {
                "root-task-compatibility",
                "root-task",
                "compatibility",
            }
            live_tcb = fields.get("live_tcb", "").lower() in {"yes", "true", "1", "pass"}
            hot_path = fields.get("hot_path", "").lower() in {
                "dedicated",
                "dedicated-sel4-task",
                "yes",
                "true",
                "1",
                "pass",
            }
            for name in contract_names:
                if dedicated:
                    dedicated_contracts.add(name)
                    if live_tcb and hot_path:
                        role = classify_driver_task_role(name)
                        if role is not None:
                            dedicated_hot_roles.add(role)
                elif compatibility:
                    compatibility_contracts.add(name)
            if "driver_task_summary" in raw:
                dedicated_summary = parse_hex_int(fields.get("dedicated"))
                if dedicated_summary is not None:
                    for index in range(dedicated_summary):
                        dedicated_contracts.add(f"summary-dedicated-{index}")
                compatibility_summary = parse_hex_int(fields.get("compatibility"))
                if compatibility_summary is not None:
                    for index in range(compatibility_summary):
                        compatibility_contracts.add(f"summary-compatibility-{index}")
            if "driver_task_acceptance" in raw:
                dedicated_ready_claimed |= (
                    fields.get("dedicated_ready", "").lower() == "yes"
                )
                parsed_compatibility = parse_hex_int(fields.get("compatibility"))
                if parsed_compatibility is not None:
                    acceptance_compatibility_count = max(
                        acceptance_compatibility_count or 0,
                        parsed_compatibility,
                    )
                substrate_ready |= fields.get("substrate", "").lower() in {"active", "yes", "pass"}
                capset_proof |= fields.get("capset", "").lower() == "pass"
                fault_proof |= fields.get("fault", "").lower() == "pass"
                revoke_proof |= fields.get("revoke", "").lower() == "pass"
                sched_proof |= fields.get("sched", "").lower() == "pass"
                affinity_proof |= fields.get("affinity", "").lower() in {
                    "pass",
                    "per-driver",
                    "yes",
                }
                vspace_proof |= fields.get("vspace", "").lower() in {"isolated", "yes", "pass"}
                pointer_free_ipc_proof |= _pointer_free_ipc_proven(fields)
                live_hot_paths |= _truthy_field(fields.get("live_hot_paths"))
                active_net = fields.get("active_net", active_net).lower()
            if "driver_task_substrate" in raw:
                substrate_ready |= fields.get("active", "").lower() == "yes"
                parsed_failed = parse_hex_int(fields.get("failed_count"))
                if parsed_failed is not None:
                    failed_count = max(failed_count, parsed_failed)
                legacy_root_authority = (
                    fields.get("root_authority_retained", "").lower() == "yes"
                )
                linked_runtime_authority = (
                    fields.get("root_authority", "").lower()
                    == "admission-descriptor-diagnostics-only"
                    and fields.get("hardware_owner", "").lower() == "linked-runtime"
                )
                capset_proof |= (
                    legacy_root_authority or linked_runtime_authority
                ) and parse_hex_int(fields.get("broad_caps_leaked")) == 0
                fault_proof |= fields.get("fault_endpoint_ready", "").lower() == "yes"
                revoke_proof |= fields.get("revoke_ready", "").lower() == "yes"
                sched_proof |= "mcs" in fields or _truthy_field(fields.get("sched"))
                configured = parse_hex_int(fields.get("affinity_configured"))
                applied = parse_hex_int(fields.get("affinity_applied"))
                if configured is not None:
                    affinity_configured = max(affinity_configured, configured)
                if applied is not None:
                    affinity_applied = max(affinity_applied, applied)
                if configured is not None and applied is not None:
                    affinity_proof |= configured == applied and configured > 0
                affinity_proof |= fields.get("affinity", "").lower() in {
                    "pass",
                    "per-driver",
                    "yes",
                }
                vspace_proof |= fields.get("vspace", "").lower() in {"isolated", "yes", "pass"}
                pointer_free_ipc_proof |= _pointer_free_ipc_proven(fields)
                live_hot_paths |= _truthy_field(fields.get("live_hot_paths"))
            if "driver_task_owner_state" in raw:
                hot_path = classify_owner_state_hot_path(fields)
                root_pointer = fields.get("root_pointer", "").lower()
                descriptor = fields.get("descriptor", "").lower()
                if (
                    hot_path is not None
                    and _owner_state_proven(fields)
                    and descriptor == "present"
                    and root_pointer == "no"
                ):
                    owner_state_hot_paths.add(hot_path)
            unexpected_caps = parse_hex_int(fields.get("unexpected_caps"))
            if unexpected_caps == 0:
                capset_proof |= fields.get("capset", "").lower() in {
                    "device-only",
                    "network-frame-transport",
                    "console-transport",
                    "display-sink",
                    "pass",
                }
            fault_proof |= fields.get("fault_probe", "").lower() == "pass"
            revoke_proof |= fields.get("revoke_ready", "").lower() == "yes"
            sched_proof |= _truthy_field(fields.get("sched")) or _truthy_field(
                fields.get("priority")
            )
            active_net = fields.get("active_net", active_net).lower()
            budget_overrun_count = parse_hex_int(fields.get("driver_task_budget_overruns"))
            if (
                raw.startswith("budget_overrun")
                or "budget overrun" in raw
                or _truthy_field(fields.get("budget_overrun"))
                or (budget_overrun_count or 0) > 0
                or driver_task_service_budget_overrun(fields)
            ):
                budget_overruns += 1
            if any(
                key in fields
                for key in (
                    "observed_service_us",
                    "latency_us",
                    "max_latency_us",
                    "service_us",
                    "p95_us",
                    "p99_us",
                )
            ):
                latency_proofs += 1
        if line_has_serial_responsiveness(raw, fields):
            serial_responsive = True
        if line_has_usb_burst_proof(raw, fields):
            usb_burst_proof = True
            drops = parse_hex_int(fields.get("drops"))
            if drops is None:
                drops = parse_hex_int(fields.get("dropped"))
            usb_burst_drops = 0 if drops is None else drops
        if line_has_hdmi_responsiveness(raw, fields):
            hdmi_responsive = True
    active_net = normalize_active_net_selection(selected_net, active_net)
    required_owner_hot_paths = required_driver_task_owner_hot_paths(selected_net, active_net)
    required_roles = required_driver_task_roles(selected_net, active_net)
    owner_state_proof = required_owner_hot_paths.issubset(owner_state_hot_paths)
    expected_affinity_contracts = pi4_selected_driver_task_affinity_contracts(
        selected_net, selected_only
    )
    affinity_manifest_missing = len(
        expected_affinity_contracts - affinity_manifest_matches
    )
    affinity_manifest_proof = (
        affinity_manifest_missing == 0 and affinity_manifest_mismatches == 0
    )
    compatibility_free = not compatibility_contracts and (
        acceptance_compatibility_count in {None, 0}
    )
    dedicated_ready = (
        dedicated_ready_claimed
        and substrate_ready
        and failed_count == 0
        and capset_proof
        and fault_proof
        and revoke_proof
        and sched_proof
        and affinity_proof
        and vspace_proof
        and pointer_free_ipc_proof
        and owner_state_proof
        and live_hot_paths
        and compatibility_free
        and required_roles.issubset(dedicated_hot_roles)
    )
    return (
        default_requested,
        live_hot_paths,
        len(contracts),
        len(dedicated_contracts),
        len(compatibility_contracts),
        dedicated_ready,
        "serial" in dedicated_hot_roles,
        "usb" in dedicated_hot_roles,
        "display" in dedicated_hot_roles,
        "net" in dedicated_hot_roles,
        "sdio" in dedicated_hot_roles,
        "pcie" in dedicated_hot_roles,
        substrate_ready,
        failed_count,
        capset_proof,
        fault_proof,
        revoke_proof,
        sched_proof,
        affinity_proof,
        affinity_configured,
        affinity_applied,
        affinity_manifest_proof,
        len(affinity_manifest_matches),
        affinity_manifest_missing,
        affinity_manifest_mismatches,
        vspace_proof,
        pointer_free_ipc_proof,
        owner_state_proof,
        active_net,
        budget_overruns,
        latency_proofs,
        serial_responsive,
        usb_burst_proof,
        usb_burst_drops,
        hdmi_responsive,
    )


def line_has_serial_responsiveness(raw: str, fields: dict[str, str]) -> bool:
    """Return whether a line proves serial echo/display responsiveness."""

    return (
        raw.startswith("serial_echo")
        or "serial echo" in raw
        or (
            "serial_input_trace" in raw
            and fields.get("stage") in {"consume-line", "line-ready"}
        )
        or fields.get("serial_responsive") in {"1", "true", "yes"}
    )


def line_has_usb_burst_proof(raw: str, fields: dict[str, str]) -> bool:
    """Return whether a line proves sustained USB keyboard burst handling."""

    return (
        raw.startswith("usb_burst")
        or "keyboard burst" in raw
        or fields.get("usb_burst") in {"1", "true", "yes"}
        or sustained_input_progress_proof(raw, fields)
    )


def sustained_input_progress_proof(raw: str, fields: dict[str, str]) -> bool:
    """Return whether a sustained-input line proves post-first-byte progress."""

    if not raw.startswith("usb: sustained_input"):
        return False
    if fields.get("queue_valid", "").lower() == "no":
        return False
    if fields.get("recovery_aux_pending", "").lower() == "yes":
        return False
    blocker = fields.get("blocker", "none").lower().replace("_", "-")
    if blocker and blocker != "none":
        return False
    report_status = fields.get("report_status", "").lower().replace("_", "-")
    if report_status not in {"", "none", "idle-report", "no-idle-report", "produced-byte"}:
        return False
    accepted = parse_hex_int(fields.get("accepted"))
    drained = parse_hex_int(fields.get("drained"))
    echoed = parse_hex_int(fields.get("echoed"))
    if None in {accepted, drained, echoed}:
        return False
    return (accepted or 0) > 0 and (drained or 0) >= (accepted or 0) and (
        echoed or 0
    ) >= (accepted or 0)


def line_has_hdmi_responsiveness(raw: str, fields: dict[str, str]) -> bool:
    """Return whether a line proves HDMI/display local-seat responsiveness."""

    if raw.startswith("hdmi_frame_submit"):
        reason = fields.get("reason", "").lower()
        payload_bytes = parse_hex_int(fields.get("bytes"))
        if payload_bytes is None:
            payload_bytes = parse_hex_int(fields.get("result"))
        return (
            reason in {"queued-output", "keyboard-scrollback"}
            and fields.get("status", "").lower() == "ready"
            and fields.get("root_console_ready", "").lower() == "yes"
            and fields.get("attached", "").lower() == "yes"
            and (payload_bytes or 0) > 0
        )
    if raw.startswith("hdmi_frame_queue"):
        reason = fields.get("reason", "").lower()
        chunk_bytes = parse_hex_int(fields.get("chunk_bytes")) or 0
        busy = parse_hex_int(fields.get("busy")) or 0
        no_reply = parse_hex_int(fields.get("no_reply")) or 0
        return (
            reason in {"queued-output", "keyboard-scrollback"}
            and chunk_bytes > 0
            and fields.get("pending_redraw", "").lower() == "no"
            and busy == 0
            and no_reply == 0
        )
    return (
        raw.startswith("hdmi_responsive")
        or "hdmi stats" in raw
        or "display stats" in raw
        or fields.get("hdmi_responsive") in {"1", "true", "yes"}
    )


WIFI_REPLAY_REFINABLE_BLOCKERS = frozenset(
    (
        "",
        "unknown",
        "missing",
        "none",
        "wifi-driver-task-runtime-unproved",
        "wifi-started-no-replay",
        "sdio-host-linked-runtime",
        "sdio-linked-runtime-progress-no-reply",
        "runtime-power-reset",
        "hal-power-reset",
        "root-prompt-printed",
        "root-prompt-delayed",
        "boot-deferred-local-seat-usb",
        "boot-deferred-root-console",
        "boot-waiting-for-wifi",
        "chipclkcsr-cmd52-pre-f2",
    )
)


def wifi_replay_should_refine(blocker: str) -> bool:
    """Return true when replay telemetry should replace a generic WiFi blocker."""

    return blocker in WIFI_REPLAY_REFINABLE_BLOCKERS


def usb_driver_task_blocker_gate(blocker: str) -> int:
    """Return the last USB gate proven by direct linked-runtime stall evidence."""

    if blocker in {
        "enable-slot-completion-pending",
        "enable-slot-completion-poll-no-reply",
        "enable-slot-event-dma-load-done-no-reply",
        "enable-slot-event-invalidate-done-no-reply",
        "enable-slot-event-peek-no-reply",
        "enable-slot-event-read-begin-no-reply",
        "enable-slot-event-read-done-no-reply",
        "enable-slot-event-slot-empty",
        "enable-slot-event-cycle-mismatch",
        "enable-slot-failed",
        "enable-slot-poll-leading-port-status",
        "enable-slot-command-event-seen",
        "enable-slot-poll-non-command-event",
        "enable-slot-event-ack-pending",
        "enable-slot-event-ack-complete",
        "cmd-event-ring-timeout",
        "cmd-timeout",
        "cmd-poll-pending",
    }:
        return 4
    if blocker in {
        "root-port-reset-no-reply",
        "root-port-connect-no-reply",
        "root-port-connect-timeout",
        "root-port-reset-completion-no-reply",
        "root-port-enable-no-reply",
        "root-port-enable-timeout",
        "root-port-reset-timeout",
        "root-port-reset-retry",
        "root-port-reset-failed",
        "root-port-stale-cleanup-no-reply",
        "root-port-stale-cleanup-failed",
        "address-enable-slot-no-reply",
        "address-device-context-publish-no-reply",
        "address-device-command-submit-no-reply",
        "address-device-command-completion-no-reply",
    }:
        return 5
    if blocker in {
        "address-device-publish-no-reply",
        "device-descriptor-no-reply",
        "device-descriptor-submit-no-reply",
        "device-descriptor-transfer-no-reply",
        "device-descriptor-status-no-reply",
        "device-descriptor-transfer-failed",
        "device-descriptor-transfer-timeout",
        "device-descriptor-status-timeout",
        "device-descriptor-transfer-event-slot-empty",
        "device-descriptor-transfer-event-cycle-mismatch",
        "device-descriptor-transfer-event-ignored",
        "device-descriptor-status-event-slot-empty",
        "device-descriptor-status-event-cycle-mismatch",
        "device-descriptor-status-event-ignored",
        "device-descriptor-prime-submit-no-reply",
        "device-descriptor-prime-transfer-no-reply",
        "device-descriptor-prime-status-no-reply",
        "device-descriptor-full-read-no-reply",
        "device-descriptor-prime-transfer-failed",
        "device-descriptor-prime-transfer-timeout",
        "device-descriptor-prime-status-timeout",
        "device-descriptor-prime-transfer-event-slot-empty",
        "device-descriptor-prime-transfer-event-cycle-mismatch",
        "device-descriptor-prime-transfer-event-ignored",
        "device-descriptor-prime-status-event-slot-empty",
        "device-descriptor-prime-status-event-cycle-mismatch",
        "device-descriptor-prime-status-event-ignored",
        "address-device-failed",
    }:
        return 6
    if blocker in {
        "config-descriptor-no-reply",
        "config-descriptor-header-submit-no-reply",
        "config-descriptor-header-transfer-no-reply",
        "config-descriptor-header-status-no-reply",
        "config-descriptor-full-read-no-reply",
        "config-descriptor-header-transfer-failed",
        "config-descriptor-header-transfer-timeout",
        "config-descriptor-header-status-timeout",
        "config-descriptor-header-transfer-event-slot-empty",
        "config-descriptor-header-transfer-event-cycle-mismatch",
        "config-descriptor-header-transfer-event-ignored",
        "config-descriptor-header-status-event-slot-empty",
        "config-descriptor-header-status-event-cycle-mismatch",
        "config-descriptor-header-status-event-ignored",
        "config-descriptor-full-submit-no-reply",
        "config-descriptor-full-transfer-no-reply",
        "config-descriptor-full-status-no-reply",
        "config-descriptor-full-transfer-failed",
        "config-descriptor-full-transfer-timeout",
        "config-descriptor-full-status-timeout",
        "config-descriptor-full-transfer-event-slot-empty",
        "config-descriptor-full-transfer-event-cycle-mismatch",
        "config-descriptor-full-transfer-event-ignored",
        "config-descriptor-full-status-event-slot-empty",
        "config-descriptor-full-status-event-cycle-mismatch",
        "config-descriptor-full-status-event-ignored",
        "hid-endpoint-not-ready",
        "hid-endpoint-parse-no-reply",
        "hid-endpoint-not-found",
        "hid-interface-not-found",
        "hid-interrupt-in-not-found",
        "hid-config-descriptor-malformed",
        "hub-child-scan-no-reply",
        "hub-set-configuration-failed",
        "hub-set-configuration-no-reply",
        "hub-set-configuration-status-no-reply",
        "hub-set-configuration-complete-no-reply",
        "hub-set-configuration-status-timeout",
        "hub-set-configuration-status-event-slot-empty",
        "hub-set-configuration-status-event-cycle-mismatch",
        "hub-set-configuration-status-event-ignored",
        "hub-set-configuration-settle-no-reply",
        "hub-descriptor-no-reply",
        "hub-descriptor-failed",
        "hub-descriptor-transfer-no-reply",
        "hub-descriptor-status-no-reply",
        "hub-descriptor-transfer-failed",
        "hub-descriptor-transfer-timeout",
        "hub-descriptor-status-timeout",
        "hub-descriptor-transfer-event-slot-empty",
        "hub-descriptor-transfer-event-cycle-mismatch",
        "hub-descriptor-transfer-event-ignored",
        "hub-descriptor-status-event-slot-empty",
        "hub-descriptor-status-event-cycle-mismatch",
        "hub-descriptor-status-event-ignored",
        "hub-context-no-reply",
        "hub-context-failed",
        "hub-port-power-no-reply",
        "hub-port-status-no-reply",
        "hub-port-status-transfer-no-reply",
        "hub-port-status-status-no-reply",
        "hub-port-status-transfer-timeout",
        "hub-port-status-timeout",
        "hub-port-status-transfer-event-slot-empty",
        "hub-port-status-transfer-event-cycle-mismatch",
        "hub-port-status-transfer-event-ignored",
        "hub-port-status-status-event-slot-empty",
        "hub-port-status-status-event-cycle-mismatch",
        "hub-port-status-status-event-ignored",
        "hub-port-status-payload-no-reply",
        "hub-port-disconnected",
        "hub-port-reset-still-active",
        "hub-port-enable-missing",
        "hub-port-clear-changes-no-reply",
        "hub-port-clear-changes-failed",
        "hub-port-status-failed",
        "hub-port-reset-no-reply",
        "hub-port-reset-set-no-reply",
        "hub-port-reset-set-failed",
        "hub-port-reset-completion-no-reply",
        "hub-child-probe-no-reply",
        "hub-child-speed-fallback-no-reply",
        "hub-topology-no-keyboard",
        "hid-configure-endpoint-no-reply",
        "hid-configure-endpoint-failed",
        "hid-set-configuration-no-reply",
        "hid-set-configuration-failed",
        "hid-control-no-reply",
        "hid-control-failed",
    }:
        return 7
    if blocker in {
        "hid-interrupt-queue-no-reply",
        "hid-interrupt-queue-failed",
    }:
        return 8
    if blocker in {
        "hid-first-report",
        "post-prompt-attach-retry-exhausted",
    }:
        return 9
    if blocker in {
        "usb-engine-init-hardware-no-reply",
        "usb-runtime-init-entry-no-reply",
        "usb-runtime-state-access-no-reply",
        "usb-engine-init-state-reset-no-reply",
        "usb-engine-init-hardware-entry-no-reply",
        "usb-xhci-mmio-entry-no-reply",
        "usb-xhci-capability-read-no-reply",
        "usb-xhci-capability-invalid",
    } or blocker.startswith("usb-xhci-"):
        if blocker in {
            "usb-engine-init-hardware-no-reply",
            "usb-runtime-init-entry-no-reply",
            "usb-runtime-state-access-no-reply",
            "usb-engine-init-state-reset-no-reply",
            "usb-engine-init-hardware-entry-no-reply",
            "usb-xhci-mmio-entry-no-reply",
            "usb-xhci-capability-read-no-reply",
            "usb-xhci-capability-invalid",
        }:
            return 2
        return 3
    if blocker.startswith("usb-pcie-posted-write-flush-"):
        return 3
    if blocker.startswith(("usb-engine-init-", "usb-resource-")):
        return 2
    if blocker == "command-event-ring-not-proven":
        return 3
    if blocker.startswith("usb-keyboard-enumeration-"):
        return 4
    return 1


def usb_raw_driver_task_progress_blocker(fields: dict[str, str]) -> str | None:
    """Return a USB blocker from raw driver-task progress telemetry."""

    if fields.get("contract", "").lower() != "usb-local-seat":
        return None
    if fields.get("marker_valid", "").lower() not in {"yes", "true", "1"}:
        return None
    aux0 = (
        fields.get("marker_aux0")
        or fields.get("expected_aux0")
        or fields.get("aux0")
        or ""
    ).lower()
    if aux0 != "0x55534245":
        return None
    return {
        "usb-command-proof-poll-begin": "enable-slot-completion-poll-no-reply",
        "usb-command-proof-event-peek-begin": "enable-slot-event-peek-no-reply",
        "usb-command-proof-event-read-begin": "enable-slot-event-read-begin-no-reply",
        "usb-command-proof-event-dma-load-done": (
            "enable-slot-event-dma-load-done-no-reply"
        ),
        "usb-command-proof-event-invalidate-done": (
            "enable-slot-event-invalidate-done-no-reply"
        ),
        "usb-command-proof-event-read-done": "enable-slot-event-read-done-no-reply",
        "usb-command-proof-event-slot-empty": "enable-slot-event-slot-empty",
        "usb-command-proof-event-cycle-mismatch": "enable-slot-event-cycle-mismatch",
        "usb-command-proof-event-port-status": "enable-slot-poll-leading-port-status",
        "usb-command-proof-event-command": "enable-slot-command-event-seen",
        "usb-command-proof-event-other": "enable-slot-poll-non-command-event",
        "usb-command-proof-erdp-ack-begin": "enable-slot-event-ack-pending",
        "usb-command-proof-erdp-ack-done": "enable-slot-event-ack-complete",
        "usb-root-port-reset-begin": "root-port-reset-no-reply",
        "usb-root-port-reset-done": "address-enable-slot-no-reply",
        "usb-root-port-reset-power-write-done": "root-port-connect-no-reply",
        "usb-root-port-connect-wait-begin": "root-port-connect-no-reply",
        "usb-root-port-connect-timeout": "root-port-connect-timeout",
        "usb-root-port-reset-pr-set": "root-port-reset-completion-no-reply",
        "usb-root-port-reset-poll-begin": "root-port-reset-completion-no-reply",
        "usb-root-port-reset-prc-seen": "root-port-enable-no-reply",
        "usb-root-port-enable-timeout": "root-port-enable-timeout",
        "usb-root-port-reset-timeout": "root-port-reset-timeout",
        "usb-root-port-reset-retry": "root-port-reset-retry",
        "usb-root-port-reset-failed": "root-port-reset-failed",
        "usb-root-port-stale-cleanup-begin": "root-port-stale-cleanup-no-reply",
        "usb-root-port-stale-cleanup-done": "address-enable-slot-no-reply",
        "usb-root-port-stale-cleanup-failed": "root-port-stale-cleanup-failed",
        "usb-address-enable-slot-begin": "address-enable-slot-no-reply",
        "usb-address-enable-slot-done": "address-device-context-publish-no-reply",
        "usb-address-contexts-published": "address-device-command-submit-no-reply",
        "usb-address-command-begin": "address-device-command-completion-no-reply",
        "usb-address-command-done": "address-device-publish-no-reply",
        "usb-address-command-failed": "address-device-failed",
        "usb-device-addressed": "device-descriptor-no-reply",
        "usb-device-descriptor-begin": "device-descriptor-submit-no-reply",
        "usb-device-descriptor-doorbell-done": "device-descriptor-transfer-no-reply",
        "usb-device-descriptor-wait-begin": "device-descriptor-transfer-no-reply",
        "usb-device-descriptor-data-event": "device-descriptor-status-no-reply",
        "usb-device-descriptor-status-event": "config-descriptor-no-reply",
        "usb-device-descriptor-failed": "device-descriptor-transfer-failed",
        "usb-device-descriptor-transfer-timeout": "device-descriptor-transfer-timeout",
        "usb-device-descriptor-status-timeout": "device-descriptor-status-timeout",
        "usb-device-descriptor-transfer-event-slot-empty": "device-descriptor-transfer-event-slot-empty",
        "usb-device-descriptor-transfer-event-cycle-mismatch": "device-descriptor-transfer-event-cycle-mismatch",
        "usb-device-descriptor-transfer-event-ignored": "device-descriptor-transfer-event-ignored",
        "usb-device-descriptor-status-event-slot-empty": "device-descriptor-status-event-slot-empty",
        "usb-device-descriptor-status-event-cycle-mismatch": "device-descriptor-status-event-cycle-mismatch",
        "usb-device-descriptor-status-event-ignored": "device-descriptor-status-event-ignored",
        "usb-device-descriptor-prime-begin": "device-descriptor-prime-submit-no-reply",
        "usb-device-descriptor-prime-doorbell-done": "device-descriptor-prime-transfer-no-reply",
        "usb-device-descriptor-prime-wait-begin": "device-descriptor-prime-transfer-no-reply",
        "usb-device-descriptor-prime-data-event": "device-descriptor-prime-status-no-reply",
        "usb-device-descriptor-prime-status-event": "device-descriptor-full-read-no-reply",
        "usb-device-descriptor-prime-failed": "device-descriptor-prime-transfer-failed",
        "usb-device-descriptor-prime-transfer-timeout": "device-descriptor-prime-transfer-timeout",
        "usb-device-descriptor-prime-status-timeout": "device-descriptor-prime-status-timeout",
        "usb-device-descriptor-prime-transfer-event-slot-empty": "device-descriptor-prime-transfer-event-slot-empty",
        "usb-device-descriptor-prime-transfer-event-cycle-mismatch": "device-descriptor-prime-transfer-event-cycle-mismatch",
        "usb-device-descriptor-prime-transfer-event-ignored": "device-descriptor-prime-transfer-event-ignored",
        "usb-device-descriptor-prime-status-event-slot-empty": "device-descriptor-prime-status-event-slot-empty",
        "usb-device-descriptor-prime-status-event-cycle-mismatch": "device-descriptor-prime-status-event-cycle-mismatch",
        "usb-device-descriptor-prime-status-event-ignored": "device-descriptor-prime-status-event-ignored",
        "usb-config-descriptor-header-begin": "config-descriptor-header-submit-no-reply",
        "usb-config-descriptor-header-doorbell-done": "config-descriptor-header-transfer-no-reply",
        "usb-config-descriptor-header-wait-begin": "config-descriptor-header-transfer-no-reply",
        "usb-config-descriptor-header-data-event": "config-descriptor-header-status-no-reply",
        "usb-config-descriptor-header-status-event": "config-descriptor-full-read-no-reply",
        "usb-config-descriptor-header-failed": "config-descriptor-header-transfer-failed",
        "usb-config-descriptor-header-transfer-timeout": "config-descriptor-header-transfer-timeout",
        "usb-config-descriptor-header-status-timeout": "config-descriptor-header-status-timeout",
        "usb-config-descriptor-header-transfer-event-slot-empty": "config-descriptor-header-transfer-event-slot-empty",
        "usb-config-descriptor-header-transfer-event-cycle-mismatch": "config-descriptor-header-transfer-event-cycle-mismatch",
        "usb-config-descriptor-header-transfer-event-ignored": "config-descriptor-header-transfer-event-ignored",
        "usb-config-descriptor-header-status-event-slot-empty": "config-descriptor-header-status-event-slot-empty",
        "usb-config-descriptor-header-status-event-cycle-mismatch": "config-descriptor-header-status-event-cycle-mismatch",
        "usb-config-descriptor-header-status-event-ignored": "config-descriptor-header-status-event-ignored",
        "usb-config-descriptor-full-begin": "config-descriptor-full-submit-no-reply",
        "usb-config-descriptor-full-doorbell-done": "config-descriptor-full-transfer-no-reply",
        "usb-config-descriptor-full-wait-begin": "config-descriptor-full-transfer-no-reply",
        "usb-config-descriptor-full-data-event": "config-descriptor-full-status-no-reply",
        "usb-config-descriptor-full-status-event": "hid-endpoint-not-ready",
        "usb-config-descriptor-full-failed": "config-descriptor-full-transfer-failed",
        "usb-config-descriptor-full-transfer-timeout": "config-descriptor-full-transfer-timeout",
        "usb-config-descriptor-full-status-timeout": "config-descriptor-full-status-timeout",
        "usb-config-descriptor-full-transfer-event-slot-empty": "config-descriptor-full-transfer-event-slot-empty",
        "usb-config-descriptor-full-transfer-event-cycle-mismatch": "config-descriptor-full-transfer-event-cycle-mismatch",
        "usb-config-descriptor-full-transfer-event-ignored": "config-descriptor-full-transfer-event-ignored",
        "usb-config-descriptor-full-status-event-slot-empty": "config-descriptor-full-status-event-slot-empty",
        "usb-config-descriptor-full-status-event-cycle-mismatch": "config-descriptor-full-status-event-cycle-mismatch",
        "usb-config-descriptor-full-status-event-ignored": "config-descriptor-full-status-event-ignored",
        "usb-hid-endpoint-parse-begin": "hid-endpoint-parse-no-reply",
        "usb-hid-endpoint-parse-found": "hid-configure-endpoint-no-reply",
        "usb-hid-endpoint-parse-missing": "hid-endpoint-not-found",
        "usb-hid-endpoint-parse-no-interface": "hid-interface-not-found",
        "usb-hid-endpoint-parse-no-interrupt-in": "hid-interrupt-in-not-found",
        "usb-hid-endpoint-parse-malformed": "hid-config-descriptor-malformed",
        "usb-hub-scan-begin": "hub-child-scan-no-reply",
        "usb-hub-set-configuration-begin": "hub-set-configuration-no-reply",
        "usb-hub-set-configuration-doorbell-done": "hub-set-configuration-status-no-reply",
        "usb-hub-set-configuration-wait-begin": "hub-set-configuration-status-no-reply",
        "usb-hub-set-configuration-status-event": "hub-set-configuration-complete-no-reply",
        "usb-hub-set-configuration-status-event-slot-empty": "hub-set-configuration-status-event-slot-empty",
        "usb-hub-set-configuration-status-event-cycle-mismatch": "hub-set-configuration-status-event-cycle-mismatch",
        "usb-hub-set-configuration-status-event-ignored": "hub-set-configuration-status-event-ignored",
        "usb-hub-set-configuration-status-timeout": "hub-set-configuration-status-timeout",
        "usb-hub-set-configuration-failed": "hub-set-configuration-failed",
        "usb-hub-set-configuration-done": "hub-set-configuration-settle-no-reply",
        "usb-hub-descriptor-begin": "hub-descriptor-no-reply",
        "usb-hub-descriptor-doorbell-done": "hub-descriptor-transfer-no-reply",
        "usb-hub-descriptor-wait-begin": "hub-descriptor-transfer-no-reply",
        "usb-hub-descriptor-data-event": "hub-descriptor-status-no-reply",
        "usb-hub-descriptor-status-event": "hub-context-no-reply",
        "usb-hub-descriptor-failed": "hub-descriptor-transfer-failed",
        "usb-hub-descriptor-transfer-timeout": "hub-descriptor-transfer-timeout",
        "usb-hub-descriptor-status-timeout": "hub-descriptor-status-timeout",
        "usb-hub-descriptor-transfer-event-slot-empty": "hub-descriptor-transfer-event-slot-empty",
        "usb-hub-descriptor-transfer-event-cycle-mismatch": "hub-descriptor-transfer-event-cycle-mismatch",
        "usb-hub-descriptor-transfer-event-ignored": "hub-descriptor-transfer-event-ignored",
        "usb-hub-descriptor-status-event-slot-empty": "hub-descriptor-status-event-slot-empty",
        "usb-hub-descriptor-status-event-cycle-mismatch": "hub-descriptor-status-event-cycle-mismatch",
        "usb-hub-descriptor-status-event-ignored": "hub-descriptor-status-event-ignored",
        "usb-hub-descriptor-done": "hub-context-no-reply",
        "usb-hub-context-begin": "hub-context-no-reply",
        "usb-hub-context-done": "hub-port-power-no-reply",
        "usb-hub-port-power-begin": "hub-port-power-no-reply",
        "usb-hub-port-power-done": "hub-port-status-no-reply",
        "usb-hub-port-status-begin": "hub-port-status-no-reply",
        "usb-hub-port-status-doorbell-done": "hub-port-status-transfer-no-reply",
        "usb-hub-port-status-wait-begin": "hub-port-status-transfer-no-reply",
        "usb-hub-port-status-data-event": "hub-port-status-status-no-reply",
        "usb-hub-port-status-status-event": "hub-port-reset-no-reply",
        "usb-hub-port-status-ack-done": "hub-port-status-payload-no-reply",
        "usb-hub-port-status-payload-read": "hub-port-reset-no-reply",
        "usb-hub-port-status-disconnected": "hub-port-disconnected",
        "usb-hub-port-status-reset-active": "hub-port-reset-still-active",
        "usb-hub-port-status-enable-missing": "hub-port-enable-missing",
        "usb-hub-port-clear-changes-begin": "hub-port-clear-changes-no-reply",
        "usb-hub-port-clear-changes-done": "hub-port-reset-no-reply",
        "usb-hub-port-clear-changes-failed": "hub-port-clear-changes-failed",
        "usb-hub-port-status-transfer-timeout": "hub-port-status-transfer-timeout",
        "usb-hub-port-status-status-timeout": "hub-port-status-timeout",
        "usb-hub-port-status-transfer-event-slot-empty": "hub-port-status-transfer-event-slot-empty",
        "usb-hub-port-status-transfer-event-cycle-mismatch": "hub-port-status-transfer-event-cycle-mismatch",
        "usb-hub-port-status-transfer-event-ignored": "hub-port-status-transfer-event-ignored",
        "usb-hub-port-status-status-event-slot-empty": "hub-port-status-status-event-slot-empty",
        "usb-hub-port-status-status-event-cycle-mismatch": "hub-port-status-status-event-cycle-mismatch",
        "usb-hub-port-status-status-event-ignored": "hub-port-status-status-event-ignored",
        "usb-hub-port-status-done": "hub-port-reset-no-reply",
        "usb-hub-port-status-failed": "hub-port-status-failed",
        "usb-hub-port-reset-begin": "hub-port-reset-no-reply",
        "usb-hub-port-reset-set-begin": "hub-port-reset-set-no-reply",
        "usb-hub-port-reset-set-done": "hub-port-reset-completion-no-reply",
        "usb-hub-port-reset-set-failed": "hub-port-reset-set-failed",
        "usb-hub-port-ready": "hub-child-probe-no-reply",
        "usb-hub-child-probe-begin": "hub-child-probe-no-reply",
        "usb-hub-child-speed-fallback-begin": "hub-child-speed-fallback-no-reply",
        "usb-hub-scan-no-keyboard": "hub-topology-no-keyboard",
        "usb-hid-configure-endpoint-begin": "hid-configure-endpoint-no-reply",
        "usb-hid-configure-endpoint-done": "hid-set-configuration-no-reply",
        "usb-hid-configure-endpoint-failed": "hid-configure-endpoint-failed",
        "usb-hid-set-configuration-begin": "hid-set-configuration-no-reply",
        "usb-hid-set-configuration-done": "hid-control-no-reply",
        "usb-hid-set-configuration-failed": "hid-set-configuration-failed",
        "usb-hid-control-begin": "hid-control-no-reply",
        "usb-hid-control-done": "hid-interrupt-queue-no-reply",
        "usb-hid-control-failed": "hid-control-failed",
        "usb-hid-interrupt-queue-begin": "hid-interrupt-queue-no-reply",
        "usb-hid-interrupt-queue-ready": "hid-first-report",
        "usb-hid-interrupt-queue-failed": "hid-interrupt-queue-failed",
    }.get(fields.get("marker_phase_name", "").lower())


def usb_driver_task_blocker_caps_gate(blocker: str) -> bool:
    """Return true when precise stall evidence must cap stale projected gates."""

    return blocker in {
        "usb-engine-init-hardware-no-reply",
        "usb-runtime-init-entry-no-reply",
        "usb-runtime-state-access-no-reply",
        "usb-engine-init-state-reset-no-reply",
        "usb-engine-init-hardware-entry-no-reply",
        "usb-xhci-mmio-entry-no-reply",
        "usb-xhci-capability-read-no-reply",
        "usb-xhci-capability-invalid",
    }


def usb_frontier_advances_pcie(frontier: str) -> bool:
    """Return true when direct USB runtime proof supersedes a stale PCIe blocker."""

    return frontier.startswith(
        (
            "usb-engine-init-",
            "usb-owner-state-",
            "usb-keyboard-",
        )
    )


def wifi_blocker_is_exact_sdio_progress(blocker: str) -> bool:
    """Return true when the WiFi blocker already names a precise SDIO phase."""

    return blocker.startswith(
        (
            "sdio-reset-",
            "sdio-engine-init-",
            "sdio-shadow-reset-",
            "sdio-state-reset-",
            "sdio-hardware-entry-",
            "sdio-sdhci-",
            "sdio-power-",
            "sdio-clock-",
            "sdio-wl-on-",
        )
    )


def wifi_blocker_is_exact_cyw43_progress(blocker: str) -> bool:
    """Return true when the WiFi blocker already names a precise CYW43 phase."""

    return blocker.startswith(
        (
            "cyw43-engine-init-",
            "cyw43-state-reset-",
            "cyw43-bus-link-",
            "cyw43-backplane-",
            "cyw43-release-",
            "cyw43-shared-control-",
            "cyw43-sdio-owner-",
            "cyw43-resource-",
        )
    ) or blocker == "cyw43-forbidden-sdio-mmio"


def field_lower(event: TraceEvent, key: str) -> str:
    """Return a lower-cased parsed field value."""

    return event.fields.get(key, "").lower()


def raw_has(event: TraceEvent, *tokens: str) -> bool:
    """Return true when all tokens appear in the raw event text."""

    raw = event.raw.lower()
    return all(token.lower() in raw for token in tokens)


def raw_has_any(event: TraceEvent, tokens: Iterable[str]) -> bool:
    """Return true when any token appears in the raw event text."""

    raw = event.raw.lower()
    return any(token.lower() in raw for token in tokens)


def field_is(event: TraceEvent, key: str, values: set[str] | tuple[str, ...]) -> bool:
    """Return true when a parsed field matches one normalized value."""

    return field_lower(event, key).replace("_", "-") in values


def usb_linked_hid_source(event: TraceEvent) -> bool:
    """Return true when USB input proof came from the linked HID runtime."""

    return field_lower(event, "source") == "linked-runtime-hid"


def resource_init_step(
    event: TraceEvent,
    *,
    contract: str | None = None,
    hot_path: str | None = None,
    stages: set[str] | tuple[str, ...],
    statuses: set[str] | tuple[str, ...],
) -> bool:
    """Return true for a matching DRIVER_TASK_RESOURCE_INIT breadcrumb."""

    if "driver_task_resource_init" not in event.raw.lower():
        return False
    if contract is not None and field_lower(event, "contract") != contract:
        return False
    if hot_path is not None and field_lower(event, "hot_path") != hot_path:
        return False
    return field_is(event, "stage", stages) and field_is(event, "status", statuses)


def control_reply_matched_step(event: TraceEvent, stage: str) -> bool:
    """Return true for a matched CYW43 control reply at one stage."""

    result = parse_hex_int(event.fields.get("status"))
    return (
        "cyw43_driver_task_control_reply" in event.raw.lower()
        and field_lower(event, "stage") == stage
        and field_lower(event, "reply_match") == "yes"
        and (result is None or result == 0)
    )


def owner_state_step(
    event: TraceEvent,
    hot_path: str,
    contract: str | None = None,
) -> bool:
    """Return true for a pointer-free linked driver owner-state line."""

    if "driver_task_owner_state" not in event.raw.lower():
        return False
    if field_lower(event, "hot_path") != hot_path:
        return False
    if contract is not None and field_lower(event, "contract") != contract:
        return False
    return (
        field_lower(event, "owner_state") == "driver-owned"
        and field_lower(event, "descriptor") == "present"
        and field_lower(event, "root_pointer") == "no"
    )


def ordered_sequence_result(
    events: Iterable[TraceEvent],
    steps: list[SequenceStep],
    required_any: list[SequenceStep] | None = None,
    forbidden: list[SequenceStep] | None = None,
) -> SequenceResult:
    """Match ordered replay steps while checking unordered prerequisites."""

    event_list = list(events)
    for forbidden_step in forbidden or []:
        if any(forbidden_step.matcher(event) for event in event_list):
            return SequenceResult(False, "none", f"forbidden-{forbidden_step.name}")

    for required_step in required_any or []:
        if not any(required_step.matcher(event) for event in event_list):
            return SequenceResult(False, "none", required_step.name)

    index = 0
    last = "none"
    for event in event_list:
        if index < len(steps) and steps[index].matcher(event):
            last = steps[index].name
            index += 1
    if index == len(steps):
        return SequenceResult(True, last, "none")
    return SequenceResult(False, last, steps[index].name)


def usb_xhci_ready_step(event: TraceEvent) -> bool:
    """Return true once linked runtime xHCI readiness is proven."""

    if resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-engine-init", "usb-xhci-init", "net-engine-init"},
        statuses={"ready"},
    ):
        return True
    if event.domain != "usb":
        return False
    diag_gate = startup_diag_gate(event.raw.lower(), "usb")
    if (
        diag_gate is not None
        and diag_gate >= 3
        and field_lower(event, "status") == "pass"
    ):
        return True
    return raw_has_any(
        event,
        (
            "controller-ready",
            "controller-init-complete",
            "usb-controller-ready",
            "usb-xhci-ready",
        ),
    )


def usb_command_event_step(event: TraceEvent) -> bool:
    """Return true for linked Enable Slot command-event proof."""

    if resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"command-ring-ready", "usb-command-ring-ready"},
    ):
        return True
    if event.domain != "usb":
        return False
    raw = event.raw.lower()
    if (
        "xhci root-port command-probe" in raw
        or "no-op" in raw
        or "linux-event" in raw
        or "linux-shaped" in raw
    ):
        return False
    diag_gate = startup_diag_gate(raw, "usb")
    if (
        diag_gate is not None
        and diag_gate >= 4
        and field_lower(event, "status") == "pass"
    ):
        return True
    if raw.startswith("usb_runtime_enum_snapshot"):
        detail = parse_hex_int(event.fields.get("detail"))
        return detail == 0x0204 and field_lower(event, "cmd_path") == "yes"
    return (
        raw_has(event, "linked_runtime", "command-probe", "enable-slot-ok")
        or raw_has_any(
            event,
            (
                "usb-command-ring-ready",
                "usb-command-event-proof",
                "usb-enable-slot-completion",
            ),
        )
        or field_lower(event, "status") == "command-ring-ready"
    )


def usb_root_port_reset_step(event: TraceEvent) -> bool:
    """Return true after live root-port reset completes."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"root-port-connected"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        (
            "usb-root-port-reset-done",
            "root-port-reset-done",
            "usb-root-port-connected",
        ),
    ))


def usb_hub_addressed_step(event: TraceEvent) -> bool:
    """Return true once the root hub device has been addressed."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"device-addressed"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        (
            "usb-device-addressed",
            "hub-device-addressed",
            "usb root hub addressed",
        ),
    ))


def usb_hub_configured_step(event: TraceEvent) -> bool:
    """Return true once hub set-configuration completes."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"hub-set-configuration-done", "usb-hub-set-configuration-done"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        (
            "usb-hub-set-configuration-done",
            "usb hub configured",
            "hub set-config ready",
        ),
    ))


def usb_hub_descriptor_step(event: TraceEvent) -> bool:
    """Return true after hub descriptor and context proof."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"hub-context-done", "hub-descriptor-done", "usb-hub-context-done"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        (
            "usb-hub-context-done",
            "usb-hub-descriptor-done",
            "hub descriptor ready",
        ),
    ))


def usb_hub_port_power_step(event: TraceEvent) -> bool:
    """Return true once hub port power is applied."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"hub-port-power-done", "usb-hub-port-power-done"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        ("usb-hub-port-power-done", "hub port power done"),
    ))


def usb_hub_port_status_step(event: TraceEvent) -> bool:
    """Return true after same-slot hub-port GET_STATUS completes."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"hub-port-status-done", "usb-hub-port-status-done"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        ("usb-hub-port-status-done", "hub-port-status-done"),
    ))


def usb_hub_port_ready_step(event: TraceEvent) -> bool:
    """Return true after hub port reset leaves the child port enabled."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"hub-port-ready", "hub-port-reset-set-done", "usb-hub-port-ready"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        (
            "usb-hub-port-reset-set-done",
            "usb-hub-port-ready",
            "hub port terminal",
        ),
    ))


def usb_hub_child_probe_step(event: TraceEvent) -> bool:
    """Return true when the keyboard child probe starts from the hub slot."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"hub-child-probe-begin", "hub-child-probe-done"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        (
            "usb-hub-child-probe-begin",
            "usb-hub-child-probe-done",
            "hub child device-desc ready",
        ),
    ))


def usb_hid_endpoint_step(event: TraceEvent) -> bool:
    """Return true once the boot keyboard interrupt-IN endpoint is found."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-enumeration", "usb-keyboard-enumeration-retry"},
        statuses={"hid-endpoint-ready", "ready"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        (
            "usb-hid-endpoint-parse-found",
            "usb hid keyboard ready",
        ),
    ))


def usb_interrupt_in_step(event: TraceEvent) -> bool:
    """Return true once an interrupt-IN TRB is queued for the HID endpoint."""

    return resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-interrupt-in"},
        statuses={"interrupt-in-ready", "ready"},
    ) or (event.domain == "usb" and raw_has_any(
        event,
        ("usb-hid-interrupt-queue-ready", "hid interrupt queue ready"),
    ))


def usb_first_report_step(event: TraceEvent) -> bool:
    """Return true for a non-pending HID first-report proof."""

    if resource_init_step(
        event,
        contract="usb-local-seat",
        hot_path="usb-keyboard",
        stages={"usb-keyboard-first-report"},
        statuses={"ready"},
    ):
        return True
    if event.domain != "usb":
        return False
    raw = event.raw.lower()
    if "pending" in raw or "empty" in raw or "failed" in raw:
        return False
    return (
        ("usb hid first report" in raw or field_lower(event, "tag") == "usb-hid-report-event")
        and usb_linked_hid_source(event)
    )


def usb_first_byte_step(event: TraceEvent) -> bool:
    """Return true when the runtime decodes a keyboard byte."""

    if event.domain != "usb":
        return False
    return "runtime keyboard first-byte" in event.raw.lower() and usb_linked_hid_source(event)


def usb_command_ready_step(event: TraceEvent) -> bool:
    """Return true when the linked USB keyboard is ready for command input."""

    raw = event.raw.lower()
    if "[local-seat] usb keyboard command-ready" in raw:
        clean_polls = parse_hex_int(event.fields.get("clean_polls")) or 0
        return (
            clean_polls >= 2
            and field_lower(event, "recovery_pending") != "yes"
            and (parse_hex_int(event.fields.get("no_reply")) or 0) == 0
        )
    if (
        "[local-seat] hdmi prompt enabled" in raw
        and field_lower(event, "reason") == "usb-console-command-ready"
    ):
        return False
    if raw.startswith("usb: acceptance"):
        return (
            field_lower(event, "command_ready") == "yes"
            and field_lower(event, "usable") == "yes"
            and field_lower(event, "first_report") == "yes"
        )
    if (
        "pi4 keyboard runtime proof" in raw
        and field_lower(event, "result") in {"online", "ready"}
        and usb_linked_hid_source(event)
        and (parse_hex_int(event.fields.get("gate")) or 0) >= 10
    ):
        return True
    return False


def usb_runtime_gate10_step(event: TraceEvent) -> bool:
    """Return true for the final USB gate-10 command-input summary."""

    if event.domain != "usb":
        return False
    proof_gate = parse_hex_int(event.fields.get("proof_gate"))
    return (
        event.raw.lower().startswith("usb: runtime_gate")
        and proof_gate is not None
        and proof_gate >= 10
        and field_lower(event, "keyboard") == "yes"
        and field_lower(event, "first_report") == "yes"
        and normalize_usb_blocker(event.fields.get("blocker", "none")) == "none"
    )


def summarize_usb_post_first_byte_blocker(events: Iterable[TraceEvent]) -> str:
    """Return sustained-input blocker after linked first-byte proof."""

    first_byte_seen = False
    last_local_seat: tuple[int, int, int, int, int, int] | None = None
    saw_keyboard_no_reply = False
    sustained_progress_seen = False
    blocker = "none"
    for event in events:
        raw = event.raw.lower()
        if usb_first_byte_step(event) or usb_runtime_gate10_step(event):
            first_byte_seen = True
        if not first_byte_seen:
            continue

        report_status = event.fields.get("report_status", "").lower().replace("_", "-")
        if raw.startswith("usb: recovery_request"):
            if field_lower(event, "action") == "no-reply":
                blocker = "usb-post-first-byte-recovery-request-no-reply"
            continue
        if (
            "driver_task_ring_call_abort" in raw
            or "driver_task_ring_call_timeout" in raw
        ) and field_lower(event, "contract") == "usb-local-seat":
            aux0 = parse_hex_int(event.fields.get("aux0"))
            marker_aux0 = parse_hex_int(event.fields.get("marker_aux0"))
            if (
                0x55534252 in {aux0, marker_aux0}
                and field_lower(event, "reason") == "timeout-resume-limit"
            ):
                phase = field_lower(event, "marker_phase_name")
                if phase == "usb-hid-interrupt-queue-begin":
                    blocker = "usb-post-first-byte-recovery-request-timeout"
                else:
                    blocker = "usb-post-first-byte-recovery-request-no-reply"
                continue
        if raw.startswith("usb: sustained_input"):
            if sustained_input_progress_proof(raw, event.fields):
                sustained_progress_seen = True
                blocker = "none"
                no_reply_streak = parse_hex_int(event.fields.get("no_reply_streak")) or 0
                if no_reply_streak == 0:
                    saw_keyboard_no_reply = False
                continue
            sustained_blocker = field_lower(event, "blocker").replace("_", "-")
            if sustained_blocker and sustained_blocker != "none":
                blocker = sustained_blocker
                continue
            if (
                field_lower(event, "recovery_aux_pending") == "yes"
                and field_lower(event, "queue_valid") == "no"
            ):
                blocker = "usb-post-first-byte-recovery-pending-no-diag"
                continue
        if report_status == "queue-collapse" or "queue-collapse" in raw:
            blocker = "usb-post-first-byte-queue-collapse"
            continue
        if report_status == "recovery-failed" or "recovery-failed" in raw:
            blocker = "usb-post-first-byte-recovery-failed"
            continue
        if report_status == "recovery-success" or "recovery-success" in raw:
            blocker = "none"
            saw_keyboard_no_reply = False
            continue
        if report_status == "unmatched-transfer" or "unmatched-transfer" in raw:
            blocker = "usb-post-first-byte-unmatched-transfer"
            continue
        if raw.startswith("usb: stall_telemetry") or raw.startswith("usb: runtime_queue"):
            queued_reports = parse_hex_int(event.fields.get("queued_reports"))
            transfer_events = parse_hex_int(event.fields.get("transfer_events"))
            if (
                queued_reports is not None
                and transfer_events is not None
                and queued_reports <= 4
                and transfer_events >= 32
            ):
                blocker = "usb-post-first-byte-queue-collapse-risk"
                continue
        if raw.startswith("usb: event_loop"):
            runtime_skipped = parse_hex_int(event.fields.get("runtime_skipped")) or 0
            if runtime_skipped > 0 and not sustained_progress_seen:
                saw_keyboard_no_reply = True

        if raw.startswith("[smp] activity local-seat "):
            fields = event.fields
            backend_polls = parse_hex_int(fields.get("backend_polls"))
            backend_bytes = parse_hex_int(fields.get("backend_bytes"))
            accepted = parse_hex_int(fields.get("accepted"))
            drained = parse_hex_int(fields.get("drained"))
            echoed = parse_hex_int(fields.get("echoed"))
            no_reply = parse_hex_int(fields.get("no_reply"))
            previous_local_seat = last_local_seat
            if None not in {backend_polls, backend_bytes, accepted, drained, echoed}:
                snapshot = (
                    backend_polls or 0,
                    backend_bytes or 0,
                    accepted or 0,
                    drained or 0,
                    echoed or 0,
                    no_reply or 0,
                )
                if (
                    previous_local_seat is not None
                    and snapshot[0] > previous_local_seat[0]
                    and snapshot[1:5] == previous_local_seat[1:5]
                    and snapshot[5] > previous_local_seat[5]
                ):
                    blocker = "usb-post-first-byte-no-progress"
                last_local_seat = snapshot
                if (
                    blocker != "usb-post-first-byte-no-progress"
                    and snapshot[2] > 0
                    and snapshot[3] >= snapshot[2]
                    and snapshot[4] >= snapshot[2]
                ):
                    blocker = "none"
                    saw_keyboard_no_reply = False
                    continue
            if (
                no_reply is not None
                and previous_local_seat is not None
                and no_reply > previous_local_seat[5]
            ):
                saw_keyboard_no_reply = True

    if blocker != "none":
        return blocker
    if saw_keyboard_no_reply:
        return "usb-post-first-byte-no-reply"
    return "none"


def summarize_usb_oldgood_replay(events: Iterable[TraceEvent]) -> SequenceResult:
    """Validate the reopened 26b USB hub-keyboard old-good replay profile."""

    return ordered_sequence_result(
        events,
        required_any=[
            SequenceStep("cold-boot-unseeded", usb_cold_boot_evidence),
            SequenceStep(
                "usb-owner-state",
                lambda event: owner_state_step(
                    event,
                    hot_path="usb-keyboard",
                    contract="usb-local-seat",
                ),
            ),
            SequenceStep(
                "pcie-owner-state",
                lambda event: owner_state_step(
                    event,
                    hot_path="pcie-root",
                    contract="pcie-root",
                ),
            ),
        ],
        forbidden=[SequenceStep("bootloader-handoff", usb_bootloader_handoff_evidence)],
        steps=[
            SequenceStep("xhci-controller-ready", usb_xhci_ready_step),
            SequenceStep("command-event-proof", usb_command_event_step),
            SequenceStep("root-port-live-reset", usb_root_port_reset_step),
            SequenceStep("hub-device-addressed", usb_hub_addressed_step),
            SequenceStep("hub-configured", usb_hub_configured_step),
            SequenceStep("hub-descriptor-context", usb_hub_descriptor_step),
            SequenceStep("hub-port-power-done", usb_hub_port_power_step),
            SequenceStep("hub-port-status-done", usb_hub_port_status_step),
            SequenceStep("hub-port-reset-ready", usb_hub_port_ready_step),
            SequenceStep("hub-child-probe", usb_hub_child_probe_step),
            SequenceStep("hid-endpoint", usb_hid_endpoint_step),
            SequenceStep("interrupt-in-armed", usb_interrupt_in_step),
            SequenceStep("first-report", usb_first_report_step),
            SequenceStep("command-ready", usb_command_ready_step),
            SequenceStep("first-byte", usb_first_byte_step),
            SequenceStep("runtime-gate10", usb_runtime_gate10_step),
        ],
    )


def wifi_sdio_engine_ready_step(event: TraceEvent) -> bool:
    """Return true when linked SDIO engine-init is ready."""

    raw = event.raw.lower()
    if (
        "sdio_driver_task_replay_status" in raw
        and field_lower(event, "stage") == "engine-init"
    ):
        return field_lower(event, "blocker") in {"ready", "none"}
    return (
        "driver_task_resource_init" in raw
        and field_lower(event, "contract") == "sdio-host"
        and "engine-init" in field_lower(event, "stage")
        and field_lower(event, "status") == "ready"
    ) or (
        event.domain in {"wifi", "driver"}
        and field_lower(event, "status") == "ready"
        and raw_has_any(
            event,
            (
                "sdio-engine-init",
                "sdio engine-init",
            ),
        )
    )


def wifi_cyw43_transport_step(event: TraceEvent) -> bool:
    """Return true when the linked CYW43 transport is admitted and live."""

    if resource_init_step(
        event,
        contract="cyw43455",
        hot_path="cyw43-wifi",
        stages={"net-engine-init", "cyw43-transport-init"},
        statuses={"ready"},
    ):
        return True
    if (
        "net_driver_task_replay_status" in event.raw.lower()
        and field_lower(event, "role") == "cyw43-wifi"
        and field_lower(event, "stage") == "engine-init"
        and field_lower(event, "blocker") in {"ready", "none"}
    ):
        return True
    if event.domain not in {"wifi", "driver"}:
        return False
    return raw_has_any(
        event,
        (
            "cyw43-transport-ready",
            "cyw43-engine-init status=ready",
            "cyw43 linked transport ready",
        ),
    )


def wifi_firmware_ready_step(event: TraceEvent) -> bool:
    """Return true after firmware upload/release reaches ready."""

    if resource_init_step(
        event,
        contract="cyw43455",
        hot_path="cyw43-wifi",
        stages={"cyw43-firmware", "cyw43-firmware-release"},
        statuses={"ready"},
    ):
        return True
    if event.domain not in {"wifi", "driver"}:
        return False
    return (
        field_lower(event, "status") == "ready"
        and raw_has_any(
            event,
            (
                "cyw43-release-firmware-ready-done",
                "firmware release ready",
                "cyw43 firmware-ready",
                "firmware-ready",
            ),
        )
    )


def wifi_function2_ready_step(event: TraceEvent) -> bool:
    """Return true when Function 2 is enabled and usable."""

    return resource_init_step(
        event,
        contract="cyw43455",
        hot_path="cyw43-wifi",
        stages={"cyw43-function2"},
        statuses={"ready"},
    )


def wifi_control_txglomalign_step(event: TraceEvent) -> bool:
    """Return true once Linux-shaped txglomalign negotiation is resolved."""

    return (
        resource_init_step(
            event,
            contract="cyw43455",
            hot_path="cyw43-wifi",
            stages={"cyw43-control-txglomalign"},
            statuses={
                "ready",
                "optional-badarg",
                "optional-unsupported",
                "optional-skip",
            },
        )
        or resource_init_step(
            event,
            contract="cyw43455",
            hot_path="cyw43-wifi",
            stages={"cyw43-control-txglomalign-fallback4"},
            statuses={"ready", "optional-badarg", "optional-unsupported"},
        )
        or control_reply_matched_step(event, "cyw43-control-txglomalign")
        or (
            event.domain in {"wifi", "driver"}
            and raw_has_any(
                event,
                (
                    "cyw43-control-txglomalign ready",
                    "control_exchange step=cyw43-control-txglomalign status=matched",
                    "bus:txglomalign=8",
                ),
            )
        )
    )


def wifi_control_txglom_pre_tx_drain_ready_step(event: TraceEvent) -> bool:
    """Return true when split txglom proves the FIFO empty before TX."""

    raw = event.raw.lower()
    return (
        "cyw43_driver_task_control_split" in raw
        and field_lower(event, "stage") == "cyw43-control-txglomalign"
        and field_lower(event, "event") == "pre-tx-drain-ready"
    )


def wifi_control_txglom_tx_complete_step(event: TraceEvent) -> bool:
    """Return true when txglom TX follows the completed pre-TX drain."""

    raw = event.raw.lower()
    return (
        "cyw43_driver_task_control_split" in raw
        and field_lower(event, "stage") == "cyw43-control-txglomalign"
        and field_lower(event, "event") == "tx-complete"
    )


def wifi_control_ulp_sdioctrl_step(event: TraceEvent) -> bool:
    """Return true once the optional ULP SDIO control step is resolved."""

    return (
        resource_init_step(
            event,
            contract="cyw43455",
            hot_path="cyw43-wifi",
            stages={"cyw43-control-ulp-sdioctrl"},
            statuses={"ready", "unsupported"},
        )
        or control_reply_matched_step(event, "cyw43-control-ulp-sdioctrl")
        or (
            event.domain in {"wifi", "driver"}
            and raw_has_any(
                event,
                (
                    "cyw43-control-ulp-sdioctrl ready",
                    "control_exchange step=cyw43-control-ulp-sdioctrl status=matched",
                    "control_exchange step=cyw43-control-ulp-sdioctrl status=unsupported",
                    "cyw43-control-ulp-sdioctrl unsupported",
                    "ulp_sdioctrl unsupported",
                ),
            )
        )
    )


def wifi_control_rxglom_step(event: TraceEvent) -> bool:
    """Return true once the Linux-shaped rxglom control step is matched."""

    return (
        resource_init_step(
            event,
            contract="cyw43455",
            hot_path="cyw43-wifi",
            stages={"cyw43-control-rxglom"},
            statuses={"ready"},
        )
        or control_reply_matched_step(event, "cyw43-control-rxglom")
        or (
            event.domain in {"wifi", "driver"}
            and raw_has_any(
                event,
                (
                    "cyw43-control-rxglom ready",
                    "control_exchange step=cyw43-control-rxglom status=matched",
                    "bus:rxglom=1",
                ),
            )
        )
    )


def wifi_control_cur_etheraddr_step(event: TraceEvent) -> bool:
    """Return true once the current MAC address control read is matched."""

    return (
        resource_init_step(
            event,
            contract="cyw43455",
            hot_path="cyw43-wifi",
            stages={"cyw43-control-cur-etheraddr", "cyw43-control-mac"},
            statuses={"ready"},
        )
        or control_reply_matched_step(event, "cyw43-control-cur-etheraddr")
        or control_reply_matched_step(event, "cyw43-control-mac")
        or (
            event.domain in {"wifi", "driver"}
            and raw_has_any(
                event,
                (
                    "cyw43-control-cur-etheraddr ready",
                    "cyw43-control-mac ready",
                    "control_exchange step=cyw43-control-cur-etheraddr status=matched",
                    "control_exchange step=cyw43-control-mac status=matched",
                    "cur_etheraddr matched",
                ),
            )
        )
    )


def wifi_control_revinfo_step(event: TraceEvent) -> bool:
    """Return true once revinfo has a matched control reply."""

    return (
        resource_init_step(
            event,
            contract="cyw43455",
            hot_path="cyw43-wifi",
            stages={"cyw43-control-revinfo"},
            statuses={"ready"},
        )
        or control_reply_matched_step(event, "cyw43-control-revinfo")
        or (event.domain in {"wifi", "driver"} and raw_has_any(
        event,
        (
            "cyw43-control-revinfo ready",
            "control_exchange step=cyw43-control-revinfo status=matched",
            "revinfo matched",
        ),
        ))
    )


def wifi_control_up_step(event: TraceEvent) -> bool:
    """Return true once WLC_UP and related control setup are complete."""

    return (
        resource_init_step(
            event,
            contract="cyw43455",
            hot_path="cyw43-wifi",
            stages={"cyw43-control-up"},
            statuses={"ready"},
        )
        or control_reply_matched_step(event, "cyw43-control-up")
        or (event.domain in {"wifi", "driver"} and raw_has_any(
        event,
        (
            "cyw43-control-up ready",
            "control_exchange step=cyw43-control-up status=matched",
            "wlc_up ready",
            "control-plane ready",
        ),
        ))
    )


def wifi_join_request_step(event: TraceEvent) -> bool:
    """Return true for the primary BSS join request, not rescue shortcuts."""

    raw = event.raw.lower()
    if "assoc_rescue" in raw or "action=set-ssid" in raw:
        return False
    result = parse_hex_int(event.fields.get("result"))
    return (
        "cyw43_driver_task_join_request" in raw
        and field_lower(event, "contract") == "cyw43455"
        and field_lower(event, "path") == "primary-bsscfg:join"
        and field_lower(event, "action") == "ready"
        and (result is None or result == 0)
    ) or (
        raw_has(event, "primary-bsscfg:join", "action=ready")
        and (result is None or result == 0)
    )


def wifi_association_link_step(event: TraceEvent) -> bool:
    """Return true when association and link-up are both proven."""

    if event.domain not in {"wifi", "driver"}:
        return False
    return (
        "cyw43_driver_task_host_eapol_status" in event.raw.lower()
        and field_lower(event, "associated") == "yes"
        and field_lower(event, "link_up") == "yes"
    ) or (
        event.raw.lower().startswith("netstats:")
        and event.fields.get("wifi_assoc") == "1"
        and event.fields.get("wifi_link") == "1"
    )


def wifi_eapol_message_step(event: TraceEvent, message: str) -> bool:
    """Return true for one explicit host-EAPOL message step."""

    if event.domain not in {"wifi", "driver"}:
        return False
    raw = event.raw.lower()
    msg = message.lower()
    explicit = "cyw43_driver_task_host_eapol_message" in raw
    legacy_host_eapol = "host-eapol" in raw
    message_matches = (
        field_lower(event, "msg") == msg
        or field_lower(event, "message") == msg
        or f"stage=cyw43-host-eapol-{msg}" in raw
        or f"action=send-{msg}" in raw
        or f"action=recv-{msg}" in raw
        or f"host-eapol-{msg}" in raw
    )
    return (explicit or legacy_host_eapol) and message_matches


def wifi_key_install_step(event: TraceEvent, key_kind: str) -> bool:
    """Return true when the host EAPOL path installs one key class."""

    if event.domain not in {"wifi", "driver"}:
        return False
    raw = event.raw.lower()
    kind = key_kind.lower()
    return (
        "cyw43_driver_task_host_eapol_key" in raw
        and field_lower(event, "kind") == kind
        and field_lower(event, "status") in {"ready", "ok"}
    ) or raw_has(event, "install-wsec-key", f"kind={kind}") or (
        field_lower(event, "kind") == kind
        and raw_has_any(
            event,
            (
                "cyw43-host-eapol-ptk",
                "cyw43-host-eapol-gtk",
            ),
        )
    )


def wifi_secure_release_step(event: TraceEvent) -> bool:
    """Return true once secure host-EAPOL state releases DHCP/data."""

    if event.domain not in {"wifi", "driver"}:
        return False
    raw = event.raw.lower()
    eapol_rx = parse_hex_int(event.fields.get("eapol_rx")) or 0
    if (
        "cyw43_driver_task_host_eapol_status" in raw
        and field_lower(event, "status") == "secure"
        and field_lower(event, "associated") == "yes"
        and field_lower(event, "link_up") == "yes"
        and eapol_rx >= 2
    ):
        return True
    if resource_init_step(
        event,
        contract="cyw43455",
        hot_path="cyw43-wifi",
        stages={"cyw43-host-eapol"},
        statuses={"secure"},
    ):
        return True
    return raw_has(event, "host-eapol-complete", "action=allow-dhcp")


def wifi_dhcp_start_step(event: TraceEvent) -> bool:
    """Return true when DHCP starts after secure release."""

    return event.domain == "wifi" and "[dhcp] start ready" in event.raw.lower()


def wifi_dhcp_bound_step(event: TraceEvent) -> bool:
    """Return true when WiFi DHCP binds a non-zero address."""

    raw = event.raw.lower()
    if event.domain != "wifi" or "[dhcp] lease bound" not in raw:
        return False
    ip = event.fields.get("ip", "")
    gateway = event.fields.get("gateway", "")
    return ip and not ip.startswith("0.0.0.0") and gateway and gateway != "0.0.0.0"


def wifi_nettest_step(event: TraceEvent) -> bool:
    """Return true for successful WiFi nettest evidence."""

    raw = event.raw.lower()
    return event.domain == "wifi" and (
        (
            raw.startswith("ok nettest")
            and (field_lower(event, "detail") == "pass" or "success" in raw)
        )
        or raw_has(event, "[net-selftest] result", "tx_ok=true", "console_ok=true")
    )


def wifi_netstats_bound_step(event: TraceEvent) -> bool:
    """Return true for bound WiFi netstats mode/state."""

    return (
        event.domain == "wifi"
        and event.raw.lower().startswith("netstats:")
        and event.fields.get("active") == "wifi"
        and event.fields.get("addr_src") == "dhcp-lease"
        and event.fields.get("dhcp") == "bound"
    )


def wifi_netstats_counters_step(event: TraceEvent) -> bool:
    """Return true for non-zero WiFi RX/TX counters."""

    return (
        event.domain == "wifi"
        and event.raw.lower().startswith("netstats:")
        and (parse_hex_int(event.fields.get("rx_pkts")) or 0) > 0
        and (parse_hex_int(event.fields.get("tx_pkts")) or 0) > 0
    )


def wifi_netstats_secure_step(event: TraceEvent) -> bool:
    """Return true for final secure WiFi association/link counters."""

    return (
        event.domain == "wifi"
        and event.raw.lower().startswith("netstats:")
        and event.fields.get("wifi_assoc") == "1"
        and event.fields.get("wifi_link") == "1"
        and event.fields.get("eapol_secure") == "1"
        and (parse_hex_int(event.fields.get("eapol_rx")) or 0) >= 2
    )


def wifi_tcp_authenticated_step(event: TraceEvent) -> bool:
    """Return true for non-empty authenticated WiFi TCP console traffic."""

    raw = event.raw.lower()
    if "[cohsh-net][auth] auth ok" in raw or "[net-console] auth ok" in raw:
        return True
    return (
        event.domain == "wifi"
        and raw.startswith("netstats:")
        and (parse_hex_int(event.fields.get("tcp_accepts")) or 0) > 0
        and (parse_hex_int(event.fields.get("tcp_auth")) or 0) > 0
        and (parse_hex_int(event.fields.get("tcp_rx_bytes")) or 0) > 0
    )


def wifi_tcp_ready_step(event: TraceEvent) -> bool:
    """Return true for explicit same-trace WiFi TCP readiness."""

    return (
        event.domain == "wifi"
        and event.raw.lower().startswith(("netstatus:", "netstats:"))
        and field_lower(event, "tcp_ready") == "yes"
    )


def wifi_dpc_healthy_step(event: TraceEvent) -> bool:
    """Return true for a complete healthy DPC sample after TCP proof."""

    return summarize_wifi_dpc_proof([event]).proof


def wifi_forbidden_shortcut(event: TraceEvent) -> bool:
    """Return true for root/shortcut evidence that cannot satisfy acceptance."""

    raw = event.raw.lower()
    return (
        field_lower(event, "root_pointer") == "yes"
        or "root-context" in raw
        or "compatibility service" in raw
        or "firmware supplicant" in raw
        or "psk_sup" in raw
    )


def summarize_wifi_gate7_proof(events: Iterable[TraceEvent]) -> WifiGate7Proof:
    """Require one attempt's ordered, explicit host-EAPOL Gate 7 proof."""

    event_list = list(events)
    if any(wifi_forbidden_shortcut(event) for event in event_list):
        return WifiGate7Proof(missing="forbidden-shortcut")

    steps: tuple[tuple[str, bool, Callable[[TraceEvent], bool]], ...] = (
        ("7a", True, wifi_join_request_step),
        ("7b", True, wifi_association_link_step),
        ("7c", True, lambda event: wifi_eapol_message_step(event, "m1")),
        ("7d", False, lambda event: wifi_eapol_message_step(event, "m2")),
        ("7d", False, lambda event: wifi_eapol_message_step(event, "m3")),
        ("7d", False, lambda event: wifi_eapol_message_step(event, "m4")),
        ("7d", False, lambda event: wifi_key_install_step(event, "ptk")),
        ("7d", True, lambda event: wifi_key_install_step(event, "gtk")),
        ("7e", True, wifi_secure_release_step),
    )
    index = 0
    completed: list[str] = []
    for event in event_list:
        # A later accepted primary join owns a new association/EAPOL attempt.
        # Discard all earlier attempt proof so a retry cannot splice its late
        # handshake into an older attempt's 7a/7b/7c prefix.
        if wifi_join_request_step(event):
            index = 1
            completed = ["7a"]
            continue
        if index >= len(steps):
            continue
        subgate, closes_subgate, matcher = steps[index]
        if not matcher(event):
            continue
        if closes_subgate:
            completed.append(subgate)
        index += 1

    seen = ">".join(completed) if completed else "none"
    last = completed[-1] if completed else "none"
    if index == len(steps):
        return WifiGate7Proof(True, seen, last, "none")
    return WifiGate7Proof(False, seen, last, steps[index][0])


CYW43_CAPTURE_FIRMWARE_LEN = 609_309
CYW43_CAPTURE_RAW_NVRAM_LEN = 2_074
CYW43_CAPTURE_NVRAM_UPLOAD_LEN = 1_744
CYW43_CAPTURE_CLM_LEN = 2_676
CYW43_CAPTURE_FIRMWARE_SHA256 = (
    "d608f866582519c0a28d86db43040f4f1b98dd1d153e72e9752586546b4a36c3"
)
CYW43_CAPTURE_NVRAM_SHA256 = (
    "ca709be81a78bdb6932936374f39943acbd7af07fae6151011127599a3ce9e3d"
)
CYW43_CAPTURE_CLM_SHA256 = (
    "9823842cae9fb9a5dd1e5fb31f595516ec7deee341354bef30bb3026eee29cc1"
)


def wifi_firmware_identity_step(event: TraceEvent) -> bool:
    """Return true for captured Pi 4B CYW43455 firmware identity evidence."""

    raw = event.raw.lower()
    if not raw.startswith(("wifi: firmware_contract", "wifi: firmware_release")):
        return False
    firmware_len = parse_hex_int(event.fields.get("fw"))
    nvram_len = parse_hex_int(event.fields.get("nvram"))
    clm_len = parse_hex_int(event.fields.get("clm"))
    return (
        firmware_len == CYW43_CAPTURE_FIRMWARE_LEN
        and nvram_len == CYW43_CAPTURE_NVRAM_UPLOAD_LEN
        and clm_len == CYW43_CAPTURE_CLM_LEN
        and field_lower(event, "fw_hash") == CYW43_CAPTURE_FIRMWARE_SHA256
        and field_lower(event, "nvram_hash") == CYW43_CAPTURE_NVRAM_SHA256
        and field_lower(event, "clm_hash") == CYW43_CAPTURE_CLM_SHA256
    )


def wifi_firmware_identity_blocker(event: TraceEvent) -> str:
    """Return the first mismatched captured Raspberry Pi OS Pi 4B identity field."""

    if parse_hex_int(event.fields.get("fw")) != CYW43_CAPTURE_FIRMWARE_LEN:
        return "firmware-len"
    nvram_len = parse_hex_int(event.fields.get("nvram"))
    if nvram_len != CYW43_CAPTURE_NVRAM_UPLOAD_LEN:
        if nvram_len == CYW43_CAPTURE_RAW_NVRAM_LEN:
            return "nvram-upload-len"
        return "nvram-len"
    if parse_hex_int(event.fields.get("clm")) != CYW43_CAPTURE_CLM_LEN:
        return "clm-len"
    if field_lower(event, "fw_hash") != CYW43_CAPTURE_FIRMWARE_SHA256:
        return "firmware-hash"
    if field_lower(event, "nvram_hash") != CYW43_CAPTURE_NVRAM_SHA256:
        return "nvram-hash"
    if field_lower(event, "clm_hash") != CYW43_CAPTURE_CLM_SHA256:
        return "clm-hash"
    return "none"


def summarize_wifi_firmware_identity(events: Iterable[TraceEvent]) -> tuple[bool, str]:
    """Return whether proof used the exact captured Raspberry Pi OS Pi 4B bundle."""

    blocker = "not-seen"
    for event in events:
        raw = event.raw.lower()
        if "cyw43_driver_task_stream_progress" in raw:
            if field_lower(event, "contract") != "cyw43455":
                continue
            if field_lower(event, "stage") != "cyw43-firmware-chunk":
                continue
            total_len = parse_hex_int(event.fields.get("total_len"))
            uploaded = parse_hex_int(event.fields.get("uploaded"))
            if total_len != CYW43_CAPTURE_FIRMWARE_LEN:
                blocker = "firmware-upload-len"
                continue
            if uploaded != total_len:
                blocker = "firmware-upload-incomplete"
                continue
            blocker = "firmware-identity-line-missing"
            continue
        if not raw.startswith(("wifi: firmware_contract", "wifi: firmware_release")):
            continue
        blocker = wifi_firmware_identity_blocker(event)
        if blocker == "none":
            return True, "none"
    return False, blocker


def wifi_clm_ready_step(event: TraceEvent) -> bool:
    """Return true when CLM download completed with the captured CLM size."""

    return (
        "cyw43_driver_task_clm" in event.raw.lower()
        and field_lower(event, "contract") == "cyw43455"
        and field_lower(event, "stage") == "cyw43-control-clmload"
        and field_lower(event, "action") == "ready"
        and parse_hex_int(event.fields.get("len")) == CYW43_CAPTURE_CLM_LEN
    )


def wifi_text_iovar_step(event: TraceEvent, *, stage: str, name: str) -> bool:
    """Return true when a nonempty CYW43 text iovar proof was captured."""

    return (
        "cyw43_driver_task_text_iovar" in event.raw.lower()
        and field_lower(event, "contract") == "cyw43455"
        and field_lower(event, "stage") == stage
        and field_lower(event, "name") == name
        and (parse_hex_int(event.fields.get("printable_len")) or 0) > 0
    )


def wifi_firmware_version_step(event: TraceEvent) -> bool:
    """Return true when firmware version text was read after CLM load."""

    return wifi_text_iovar_step(event, stage="cyw43-control-firmware-version", name="ver")


def wifi_clm_version_step(event: TraceEvent) -> bool:
    """Return true when CLM version text was read after CLM load."""

    return wifi_text_iovar_step(event, stage="cyw43-control-clm-version", name="clmver")


def summarize_wifi_oldgood_replay(events: Iterable[TraceEvent]) -> SequenceResult:
    """Validate the reopened 26b CYW43 host-EAPOL old-good replay profile."""

    return ordered_sequence_result(
        events,
        required_any=[
            SequenceStep(
                "cyw43-owner-state",
                lambda event: owner_state_step(
                    event,
                    hot_path="cyw43-wifi",
                    contract="cyw43455",
                ),
            ),
            SequenceStep(
                "sdio-owner-state",
                lambda event: owner_state_step(
                    event,
                    hot_path="sdio-host",
                    contract="sdio-host",
                ),
            ),
            SequenceStep("firmware-identity", wifi_firmware_identity_step),
        ],
        forbidden=[SequenceStep("wifi-shortcut", wifi_forbidden_shortcut)],
        steps=[
            SequenceStep("sdio-engine-ready", wifi_sdio_engine_ready_step),
            SequenceStep("cyw43-transport-ready", wifi_cyw43_transport_step),
            SequenceStep("firmware-ready", wifi_firmware_ready_step),
            SequenceStep("function2-ready", wifi_function2_ready_step),
            SequenceStep(
                "control-txglom-pre-tx-drain-ready",
                wifi_control_txglom_pre_tx_drain_ready_step,
            ),
            SequenceStep(
                "control-txglom-tx-complete", wifi_control_txglom_tx_complete_step
            ),
            SequenceStep("control-txglomalign", wifi_control_txglomalign_step),
            SequenceStep("control-ulp-sdioctrl", wifi_control_ulp_sdioctrl_step),
            SequenceStep("control-rxglom", wifi_control_rxglom_step),
            SequenceStep("control-cur-etheraddr", wifi_control_cur_etheraddr_step),
            SequenceStep("control-revinfo", wifi_control_revinfo_step),
            SequenceStep("clm-ready", wifi_clm_ready_step),
            SequenceStep("firmware-version", wifi_firmware_version_step),
            SequenceStep("clm-version", wifi_clm_version_step),
            SequenceStep("control-up", wifi_control_up_step),
            SequenceStep("join-request", wifi_join_request_step),
            SequenceStep("association-link", wifi_association_link_step),
            SequenceStep(
                "host-eapol-m1",
                lambda event: wifi_eapol_message_step(event, "m1"),
            ),
            SequenceStep(
                "host-eapol-m2",
                lambda event: wifi_eapol_message_step(event, "m2"),
            ),
            SequenceStep(
                "host-eapol-m3",
                lambda event: wifi_eapol_message_step(event, "m3"),
            ),
            SequenceStep(
                "host-eapol-m4",
                lambda event: wifi_eapol_message_step(event, "m4"),
            ),
            SequenceStep(
                "ptk-install",
                lambda event: wifi_key_install_step(event, "ptk"),
            ),
            SequenceStep(
                "gtk-install",
                lambda event: wifi_key_install_step(event, "gtk"),
            ),
            SequenceStep("secure-release", wifi_secure_release_step),
            SequenceStep("dhcp-start", wifi_dhcp_start_step),
            SequenceStep("dhcp-bound", wifi_dhcp_bound_step),
            SequenceStep("nettest", wifi_nettest_step),
            SequenceStep("netstats-counters", wifi_netstats_counters_step),
            SequenceStep("tcp-authenticated", wifi_tcp_authenticated_step),
            SequenceStep("netstats-bound", wifi_netstats_bound_step),
            SequenceStep("netstats-secure", wifi_netstats_secure_step),
            SequenceStep("tcp-ready", wifi_tcp_ready_step),
            SequenceStep("dpc-healthy-after-tcp", wifi_dpc_healthy_step),
        ],
    )


def summarize_timer_backend(events: Iterable[TraceEvent]) -> tuple[str, int, str, bool]:
    """Return timer backend, clock, counter kind, and dummy-timer evidence."""

    backend = "unknown"
    timer_clock_hz = 0
    counter = "none"
    dummy_seen = False

    for event in events:
        raw_lower = event.raw.lower()
        if "[timers]" not in raw_lower:
            continue

        if "dummysofttimer" in raw_lower or "dummy software counter" in raw_lower:
            dummy_seen = True
            if backend == "unknown":
                backend = "dummy"

        if "architected" in raw_lower and "counter" in raw_lower:
            backend = "arch-counter"
            if counter == "none":
                counter = "vct"

        if event.fields.get("backend") == "arch-counter":
            backend = "arch-counter"
        elif event.fields.get("backend") == "dummy":
            backend = "dummy"
            dummy_seen = True

        if event.fields.get("counter") in {"virtual", "vct"}:
            counter = "vct"
        elif event.fields.get("counter") == "none":
            counter = "none"

        raw_freq = event.fields.get("timer_freq_hz")
        if raw_freq is None and "timer_freq_hz=" in event.raw:
            raw_freq = event.raw.split("timer_freq_hz=", 1)[1].split(None, 1)[0]
        if raw_freq is not None:
            try:
                timer_clock_hz = int(str(raw_freq).rstrip("Hz"))
            except ValueError:
                pass

    if backend == "dummy":
        counter = "none"

    return backend, timer_clock_hz, counter, dummy_seen


def sdio_irq158_contract_inband_step(event: TraceEvent) -> bool:
    """Return true for the real IRQ-158 SDIO device-clear contract."""

    return (
        "sdio irq contract" in event.raw.lower()
        and event.fields.get("irq") == "158"
        and event.fields.get("bound") == "1"
        and "sdio-intstatus+sdhci-cardint" in field_lower(event, "device_clear")
    )


def sdio_ienx_or_card_int_step(event: TraceEvent) -> bool:
    """Return true when logs show IENx arming or CARD_INT source evidence."""

    raw = event.raw.lower()
    if "cccr-ienx+sdhci-card-int" in raw:
        ien = parse_hex_int(event.fields.get("ien"))
        if ien is None or ien == 0x07:
            return True
    for key in ("rxsrc_ien", "control_rxsrc_ien", "ienx"):
        value = parse_hex_int(event.fields.get(key))
        if value == 0x07:
            return True
    return (
        "card_int=y" in raw
        or "card_int=true" in raw
        or field_lower(event, "card_int") in {"y", "yes", "true", "1"}
        or field_lower(event, "rxsrc_card_int") in {"y", "yes", "true", "1"}
        or field_lower(event, "control_rxsrc_card_int") in {"y", "yes", "true", "1"}
    )


def summarize_sdio_irq158_inband_proof(events: Iterable[TraceEvent]) -> bool:
    """Return true only for IRQ-158 plus IENx/CARD_INT in-band evidence."""

    event_list = list(events)
    return any(sdio_irq158_contract_inband_step(event) for event in event_list) and any(
        sdio_ienx_or_card_int_step(event) for event in event_list
    )


def wifi_dpc_failure_reason(proof: WifiDpcProof) -> str | None:
    """Return the strongest fail-closed reason for one exact DPC proof line."""

    if proof.poisoned == "yes":
        return "poisoned"
    if proof.overruns != 0:
        return "overrun"
    if proof.epoch_errors != 0:
        return "epoch-error"
    if proof.sequence_errors != 0:
        return "sequence-error"
    if proof.ack_failures != 0:
        return "ack-failure"
    if proof.captures == 0 or proof.published == 0:
        return "no-activity"
    if proof.captures != proof.published:
        return "capture-publish-mismatch"
    if proof.consumed != proof.published:
        return "consume-publish-mismatch"
    if proof.masked == "yes":
        return "masked"
    if proof.masked != "unknown" and proof.rearms < proof.captures:
        return "unrearmed"
    return None


def summarize_wifi_dpc_proof(events: Iterable[TraceEvent]) -> WifiDpcProof:
    """Summarize exact CYW43/SDIO DPC lines without crediting partial text."""

    latest: WifiDpcProof | None = None
    first_failure: str | None = None
    for event in events:
        match = CYW43_SDIO_DPC_RE.fullmatch(event.raw)
        if match is None:
            continue
        values = {
            key: int(match.group(key))
            for key in (
                "generation",
                "captures",
                "published",
                "consumed",
                "rearms",
                "overruns",
                "epoch_errors",
                "sequence_errors",
                "ack_failures",
            )
        }
        latest = WifiDpcProof(
            generation=values["generation"],
            captures=values["captures"],
            published=values["published"],
            consumed=values["consumed"],
            rearms=values["rearms"],
            overruns=values["overruns"],
            epoch_errors=values["epoch_errors"],
            sequence_errors=values["sequence_errors"],
            ack_failures=values["ack_failures"],
            poisoned=match.group("poisoned"),
            masked=match.group("masked") or "unknown",
            line=event.line,
        )
        failure = wifi_dpc_failure_reason(latest)
        if failure is not None and first_failure is None:
            first_failure = failure
    if latest is None:
        return WifiDpcProof()
    if first_failure is not None:
        return replace(latest, proof=False, reason=first_failure)
    return replace(latest, proof=True, reason="none")


def summarize_cyw43_bootstrap_supervisor(
    events: Iterable[TraceEvent],
) -> Cyw43BootstrapSupervisorProof:
    """Validate the persistent bootstrap supervisor's terminal retry state."""

    supervisor_events = [
        event
        for event in events
        if event.raw.startswith("CYW43_BOOTSTRAP_SUPERVISOR")
    ]
    if not supervisor_events:
        return Cyw43BootstrapSupervisorProof()

    blocker: str | None = None
    state = "none"
    current_attempt = 0
    max_attempt = 0
    transient_retries = 0
    last_status = "none"
    begin_ms = 0
    scheduled_ms = 0
    episode_start_status = "none"
    preflight_seen = False
    preflight_serial_ready = False
    last_preflight_next_attempt_ms: int | None = None
    last_console_sequence: int | None = None

    def mark_blocker(reason: str) -> None:
        nonlocal blocker
        if blocker is None:
            blocker = reason

    def parse_canonical_u64(value: str) -> int | None:
        if not value or (len(value) > 1 and value.startswith("0")):
            return None
        if len(value) > 20:
            return None
        parsed = int(value)
        return parsed if parsed <= U64_MAX else None

    for event in supervisor_events:
        match = CYW43_BOOTSTRAP_SUPERVISOR_BASE_RE.fullmatch(event.raw)
        if match is None:
            last_status = event.fields.get("status", "malformed")
            mark_blocker("malformed-line")
            continue

        suffix_match = CYW43_BOOTSTRAP_SUPERVISOR_PRODUCTION_SUFFIX_RE.fullmatch(
            match.group("production_suffix")
        )
        if suffix_match is None:
            # Older records remain useful for attempt/backoff diagnosis, but
            # only the current full-fidelity UART record proves that serial,
            # local-seat, recovery, console ordering, telemetry routing, and
            # prompt refresh state were emitted atomically.
            mark_blocker("production-suffix-incomplete")
        else:
            console_sequence = parse_canonical_u64(suffix_match.group("console_seq"))
            if console_sequence is None:
                mark_blocker("numeric-field-invalid")
            else:
                if (
                    last_console_sequence is not None
                    and console_sequence <= last_console_sequence
                ):
                    mark_blocker("console-sequence-not-monotonic")
                last_console_sequence = console_sequence

        status = match.group("status")
        last_status = status
        attempt = parse_canonical_u64(match.group("attempt"))
        backoff_ms = parse_canonical_u64(match.group("backoff_ms"))
        next_attempt_ms = parse_canonical_u64(match.group("next_attempt_ms"))
        if attempt is None or backoff_ms is None or next_attempt_ms is None:
            mark_blocker("numeric-field-invalid")
            continue

        if status == "preflight":
            preflight_seen = True
            if state != "none":
                mark_blocker("invalid-preflight-order")
            if attempt != 0:
                mark_blocker("preflight-attempt-nonzero")
            if suffix_match is not None:
                preflight_serial_ready = suffix_match.group("serial") == "ready"
                if preflight_serial_ready:
                    if backoff_ms != 0:
                        mark_blocker("malformed-preflight-timing")
                elif (
                    backoff_ms != CYW43_BOOTSTRAP_SERIAL_RETRY_MS
                    or next_attempt_ms < CYW43_BOOTSTRAP_SERIAL_RETRY_MS
                ):
                    mark_blocker("malformed-preflight-timing")
                if (
                    last_preflight_next_attempt_ms is not None
                    and next_attempt_ms < last_preflight_next_attempt_ms
                ):
                    mark_blocker("malformed-preflight-timing")
                last_preflight_next_attempt_ms = next_attempt_ms
            continue

        if attempt == 0:
            mark_blocker("attempt-zero")
            continue
        if attempt > CYW43_BOOTSTRAP_MAX_ATTEMPTS:
            mark_blocker("attempt-overflow")
            continue
        max_attempt = max(max_attempt, attempt)
        if suffix_match is not None and suffix_match.group("serial") != "ready":
            mark_blocker("serial-blocked")

        if status in ("begin", "recovery"):
            sequence_valid = True
            if backoff_ms != 0:
                mark_blocker("malformed-backoff-progression")
                sequence_valid = False
            if state == "none":
                if attempt != 1 or status != "begin":
                    mark_blocker("attempt-sequence-gap")
                    sequence_valid = False
            elif state == "backoff":
                if attempt <= current_attempt:
                    mark_blocker("attempt-regression")
                    sequence_valid = False
                elif attempt != current_attempt + 1:
                    mark_blocker("attempt-sequence-gap")
                    sequence_valid = False
                if status != episode_start_status:
                    mark_blocker("episode-kind-changed")
                    sequence_valid = False
                if next_attempt_ms < scheduled_ms:
                    mark_blocker("malformed-backoff-progression")
                    sequence_valid = False
            elif state == "ready":
                if attempt != 1 or status != "recovery":
                    mark_blocker("invalid-status-sequence")
                    sequence_valid = False
            else:
                mark_blocker("invalid-status-sequence")
                sequence_valid = False
            if sequence_valid:
                state = "active"
                current_attempt = attempt
                begin_ms = next_attempt_ms
                episode_start_status = status
            continue

        if status == "backoff":
            transient_retries += 1
            sequence_valid = (
                state == "active"
                and attempt == current_attempt
                and attempt < CYW43_BOOTSTRAP_MAX_ATTEMPTS
            )
            if not sequence_valid:
                mark_blocker("invalid-status-sequence")
            expected_backoff_ms = CYW43_BOOTSTRAP_RETRY_BACKOFF_MS[
                min(attempt - 1, len(CYW43_BOOTSTRAP_RETRY_BACKOFF_MS) - 1)
            ]
            if (
                backoff_ms != expected_backoff_ms
                or next_attempt_ms < begin_ms + backoff_ms
            ):
                mark_blocker("malformed-backoff-progression")
                sequence_valid = False
            if sequence_valid:
                state = status
                scheduled_ms = next_attempt_ms
            continue

        if status == "ready":
            sequence_valid = state == "active" and attempt == current_attempt
            if not sequence_valid:
                if state == "active" and attempt < current_attempt:
                    mark_blocker("attempt-regression")
                else:
                    mark_blocker("invalid-status-sequence")
            if backoff_ms != 0 or next_attempt_ms < begin_ms:
                mark_blocker("malformed-backoff-progression")
                sequence_valid = False
            if sequence_valid:
                state = "ready"
            continue

        if status == "exhausted":
            sequence_valid = (
                state == "active"
                and attempt == current_attempt
                and attempt == CYW43_BOOTSTRAP_MAX_ATTEMPTS
            )
            if not sequence_valid:
                mark_blocker("invalid-status-sequence")
            if backoff_ms != 0 or next_attempt_ms != CYW43_BOOTSTRAP_NO_ATTEMPT_MS:
                mark_blocker("malformed-exhausted-sentinel")
                sequence_valid = False
            if sequence_valid:
                state = "exhausted"
                mark_blocker("retry-exhausted")
            continue

        if status == "permanent" or status.startswith("permanent-"):
            sequence_valid = backoff_ms == 0 and (
                (state == "none" and attempt == 1)
                or (state == "active" and attempt == current_attempt)
            )
            if not sequence_valid:
                mark_blocker("invalid-status-sequence")
            mark_blocker("permanent-status")
            state = "permanent"
            current_attempt = attempt
            continue

        mark_blocker("invalid-status")
        state = status
        current_attempt = attempt

    if blocker is None:
        if state == "active":
            blocker = "begin-not-terminal"
        elif state == "backoff":
            blocker = "scheduled-not-terminal"
        elif state == "none" and preflight_seen and not preflight_serial_ready:
            blocker = "serial-blocked"
        elif state != "ready":
            blocker = "ready-missing"

    ready = state == "ready" and blocker is None
    return Cyw43BootstrapSupervisorProof(
        seen=True,
        max_attempt=max_attempt,
        transient_retries=transient_retries,
        last_status=last_status,
        ready=ready,
        blocker=blocker or "none",
    )


def summarize_gates(events: Iterable[TraceEvent]) -> GateSummary:
    """Build the current USB/WiFi hardware proof gate summary."""

    event_list = list(events)
    wifi_dpc = summarize_wifi_dpc_proof(event_list)
    cyw43_bootstrap_supervisor = summarize_cyw43_bootstrap_supervisor(event_list)
    usb_gate, usb_blocker = summarize_usb_gate(event_list)
    usb_oldgood = summarize_usb_oldgood_replay(event_list)
    usb_event_ring_alive, usb_psc_drain_count, usb_psc_drain_mask = (
        summarize_usb_event_ring_state(event_list)
    )
    wifi_gate, wifi_blocker = summarize_wifi_gate(event_list)
    wifi_oldgood = summarize_wifi_oldgood_replay(event_list)
    wifi_exact, wifi_phase, wifi_blocker_line = summarize_wifi_failure_detail(
        event_list, wifi_blocker
    )
    if wifi_blocker == "backplane-window" and wifi_exact.startswith(
        "cyw43-backplane-"
    ):
        # The dependency gate row is intentionally broad. Preserve the linked
        # runtime's exact retained attach frontier when it immediately precedes
        # that generic Gate-5 failure.
        wifi_blocker = wifi_exact
    initial_wifi_subgate = summarize_wifi_gate7_subgate_detail(
        event_list, wifi_gate, wifi_blocker
    )
    wifi_dhcp_frontier = summarize_wifi_dhcp_frontier(event_list)
    if wifi_dhcp_frontier is not None and (
        wifi_gate >= 9 or initial_wifi_subgate.subgate == "7e"
    ):
        wifi_gate = max(wifi_gate, 9)
        wifi_blocker, wifi_phase, wifi_blocker_line = wifi_dhcp_frontier
        wifi_exact = wifi_blocker
    wifi_deferred_resume_start = next(
        (
            event
            for event in event_list
            if "[net-console]" in event.raw.lower()
            and (
                "deferred resume reason=root-shell-ready" in event.raw.lower()
                or "deferred resume reason=root-prompt-printed" in event.raw.lower()
                or "deferred resume reason=root-prompt-delayed" in event.raw.lower()
            )
            and "action=start-wifi" in event.raw.lower()
        ),
        None,
    )
    panic_events = [
        event
        for event in event_list
        if event.domain == "kernel"
        and (
            event.fields.get("panic") == "yes"
            or event.fields.get("bootinfo_corrupted") == "yes"
        )
    ]
    panic_seen = bool(panic_events)
    panic_reason = "none"
    if panic_seen:
        panic_reason = next(
            (
                event.fields.get("reason", "root-task-panic")
                for event in panic_events
                if event.fields.get("reason") not in (None, "root-task-panic")
            ),
            panic_events[0].fields.get("reason", "root-task-panic"),
        )
    halt_events = [
        event
        for event in event_list
        if event.domain == "kernel" and event.fields.get("halt") == "yes"
    ]
    boot_halted = panic_seen or bool(halt_events)
    timer_irq27_seen = any(
        event.domain == "kernel"
        and event.fields.get("irq") == "27"
        and event.fields.get("timer_irq") == "yes"
        for event in event_list
    )
    timer_backend, timer_clock_hz, timer_el0_counter, dummy_timer_seen = (
        summarize_timer_backend(event_list)
    )
    sdio_irq158_events = [
        event
        for event in event_list
        if event.domain == "wifi"
        and (
            (
                event.fields.get("irq") == "158"
                and (
                    "sdio irq bind" in event.raw.lower()
                    or "sdio irq contract" in event.raw.lower()
                )
            )
            or (
                "[pi4-wifi] hal init" in event.raw.lower()
                and event.fields.get("irq_bound") == "true"
            )
        )
    ]
    sdio_irq158_seen = bool(sdio_irq158_events)
    sdio_irq158_bound = any(
        (
            "sdio irq contract" in event.raw.lower()
            and event.fields.get("bound") == "1"
        )
        or (
            "[pi4-wifi] hal init" in event.raw.lower()
            and event.fields.get("irq_bound") == "true"
        )
        for event in sdio_irq158_events
    )
    sdio_irq158_line = sdio_irq158_events[0].line if sdio_irq158_events else 0
    sdio_irq158_inband_proof = summarize_sdio_irq158_inband_proof(event_list)
    if panic_seen:
        boot_halt_reason = panic_reason
    elif boot_halted:
        boot_halt_reason = next(
            (
                event.fields.get("reason", "kernel-halt")
                for event in halt_events
                if event.fields.get("reason") not in (None, "kernel-halt")
            ),
            halt_events[0].fields.get("reason", "kernel-halt"),
        )
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
    root_prompt_seen = any(root_prompt_evidence(event) for event in event_list)
    root_console_ready = (
        any(root_console_ready_evidence(event) for event in event_list) or root_prompt_seen
    )
    net_active, net_addr_src, net_dhcp, net_tcp_ready, nettest_proof = (
        summarize_net_state(event_list)
    )
    cohsh_tcp_auth_proof, tcp_accepts, tcp_auth_sessions, tcp_rx_bytes = (
        summarize_cohsh_tcp_auth(event_list)
    )
    (
        wifi_data_path_tx,
        wifi_data_path_rx_preserved,
        wifi_data_path_rx_delivered,
        wifi_data_path_rx_dropped,
        wifi_data_path_last,
    ) = summarize_wifi_data_path(event_list)
    (
        wifi_service_op,
        wifi_service_reason,
        wifi_service_progress,
        wifi_service_seq,
        wifi_service_credit,
        wifi_service_channel,
        wifi_service_rframe,
        wifi_service_eapol_m1,
        wifi_service_eapol_m2,
        wifi_service_eapol_m3,
        wifi_service_eapol_m4,
    ) = summarize_wifi_service_turn(event_list)
    (
        wifi_rx_irq_preserve_count,
        wifi_rx_irq_preserve_reason,
        wifi_rx_irq_preserve_int,
        wifi_rx_irq_preserve_ack,
        wifi_rx_irq_episode_preserves,
        wifi_rx_irq_episode_rearms,
    ) = summarize_wifi_rx_irq_preserve(event_list)
    (
        wifi_rxtrace_seq,
        wifi_rxtrace_start_ticks,
        wifi_rxtrace_pre_sample_delta_ticks,
        wifi_rxtrace_transfer_delta_ticks,
        wifi_rxtrace_post_sample_delta_ticks,
    ) = summarize_wifi_rxtrace_timing(event_list)
    wifi_firmware_identity_proof, wifi_firmware_identity_blocker = (
        summarize_wifi_firmware_identity(event_list)
    )
    wifi_clm_ready_proof = any(wifi_clm_ready_step(event) for event in event_list)
    wifi_firmware_version_proof = any(
        wifi_firmware_version_step(event) for event in event_list
    )
    wifi_clm_version_proof = any(wifi_clm_version_step(event) for event in event_list)
    (
        driver_task_default_requested,
        driver_task_live_hot_paths,
        driver_task_contracts,
        driver_task_dedicated,
        driver_task_compatibility,
        driver_task_dedicated_ready,
        driver_task_serial_dedicated,
        driver_task_usb_dedicated,
        driver_task_display_dedicated,
        driver_task_net_dedicated,
        driver_task_sdio_dedicated,
        driver_task_pcie_dedicated,
        driver_task_substrate_ready,
        driver_task_failed_count,
        driver_task_capset_proof,
        driver_task_fault_proof,
        driver_task_revoke_proof,
        driver_task_sched_proof,
        driver_task_affinity_proof,
        driver_task_affinity_configured,
        driver_task_affinity_applied,
        driver_task_affinity_manifest_proof,
        driver_task_affinity_manifest_matches,
        driver_task_affinity_manifest_missing,
        driver_task_affinity_manifest_mismatches,
        driver_task_vspace_proof,
        driver_task_pointer_free_ipc_proof,
        driver_task_owner_state_proof,
        driver_task_active_net,
        driver_task_budget_overruns,
        driver_task_latency_proofs,
        serial_responsive_proof,
        usb_burst_proof,
        usb_burst_drops,
        hdmi_responsive_proof,
    ) = summarize_driver_task_proofs(event_list)
    driver_task_notification_bind_deferred = any(
        "driver_task_notification_bind_deferred" in event.raw.lower()
        for event in event_list
    )
    driver_task_ring_call_begin = sum(
        1
        for event in event_list
        if "driver_task_ring_call_begin" in event.raw.lower()
    )
    driver_task_ring_call_return = sum(
        1
        for event in event_list
        if "driver_task_ring_call_return" in event.raw.lower()
    )
    driver_task_ring_call_outstanding = max(
        0, driver_task_ring_call_begin - driver_task_ring_call_return
    )
    (
        driver_task_ring_call_timeout,
        driver_task_ring_call_unresolved_timeout,
    ) = summarize_driver_task_ring_call_timeouts(event_list)
    driver_task_ring_call_keep_active = sum(
        1
        for event in event_list
        if "driver_task_ring_call_keep_active" in event.raw.lower()
    )
    driver_task_ring_call_abort = sum(
        1
        for event in event_list
        if "driver_task_ring_call_abort" in event.raw.lower()
    )
    if driver_task_ring_call_abort:
        aborted_requests: set[int] = set()
        for event in event_list:
            raw = event.raw.lower()
            if "driver_task_ring_call_abort" not in raw:
                continue
            request = parse_hex_int(event.fields.get("request"))
            if request is not None:
                aborted_requests.add(request)
        if aborted_requests:
            open_begins = 0
            for event in event_list:
                raw = event.raw.lower()
                if "driver_task_ring_call_begin" not in raw:
                    continue
                request = parse_hex_int(event.fields.get("request"))
                if request in aborted_requests:
                    open_begins += 1
            driver_task_ring_call_outstanding = max(
                0, driver_task_ring_call_outstanding - open_begins
            )
    driver_task_bootstrap_deferred = summarize_driver_task_bootstrap_deferred(
        event_list
    )
    (
        driver_task_runtime_descriptor_seal_proofs,
        driver_task_runtime_descriptor_seal_blocker,
    ) = summarize_driver_task_runtime_descriptor_seals(
        event_list, driver_task_active_net, driver_task_active_net
    )
    driver_task_runtime_descriptor_seal_proof = (
        driver_task_runtime_descriptor_seal_proofs > 0
        and driver_task_runtime_descriptor_seal_blocker == "none"
    )
    (
        driver_task_resource_init,
        driver_task_resource_blocker,
        driver_task_resource_current_blocker,
    ) = summarize_driver_task_resource_init(event_list)
    driver_task_dma_proofs, driver_task_dma_blocker = summarize_driver_task_dma_proofs(
        event_list, driver_task_active_net, driver_task_active_net
    )
    driver_task_counter_summary = summarize_driver_task_counters(event_list)
    (
        pi4_runtime_dma_proof,
        pi4_runtime_dma_proof_reason,
        pi4_runtime_dma_counter_proof,
    ) = classify_pi4_runtime_dma_proof(
        root_prompt_seen=root_prompt_seen,
        usb_cold_boot_seen=usb_cold_boot_seen,
        driver_task_dedicated_ready=driver_task_dedicated_ready,
        driver_task_compatibility=driver_task_compatibility,
        driver_task_substrate_ready=driver_task_substrate_ready,
        driver_task_failed_count=driver_task_failed_count,
        driver_task_capset_proof=driver_task_capset_proof,
        driver_task_fault_proof=driver_task_fault_proof,
        driver_task_revoke_proof=driver_task_revoke_proof,
        driver_task_sched_proof=driver_task_sched_proof,
        driver_task_affinity_proof=driver_task_affinity_proof,
        driver_task_vspace_proof=driver_task_vspace_proof,
        driver_task_pointer_free_ipc_proof=driver_task_pointer_free_ipc_proof,
        driver_task_owner_state_proof=driver_task_owner_state_proof,
        driver_task_runtime_descriptor_seal_proof=(
            driver_task_runtime_descriptor_seal_proof
        ),
        driver_task_ring_call_outstanding=driver_task_ring_call_outstanding,
        driver_task_ring_call_unresolved_timeout=(
            driver_task_ring_call_unresolved_timeout
        ),
        driver_task_bootstrap_deferred=driver_task_bootstrap_deferred,
        driver_task_resource_blocker=driver_task_resource_blocker,
        driver_task_resource_current_blocker=driver_task_resource_current_blocker,
        driver_task_dma_proofs=driver_task_dma_proofs,
        driver_task_dma_blocker=driver_task_dma_blocker,
        timer_backend=timer_backend,
        timer_clock_hz=timer_clock_hz,
        timer_el0_counter=timer_el0_counter,
        dummy_timer_seen=dummy_timer_seen,
        driver_task_counter_summary=driver_task_counter_summary,
    )
    output_pressure_summary = summarize_output_pressure(event_list)
    usb_keyboard_pressure_summary = summarize_usb_keyboard_pressure(event_list)
    usb_runtime_queue_summary = summarize_usb_runtime_queue(event_list)
    usb_post_first_byte_blocker = summarize_usb_post_first_byte_blocker(event_list)
    usb_gate, usb_blocker, usb_runtime_queue_summary = refine_usb_gate_for_runtime_truth(
        usb_gate,
        usb_blocker,
        usb_runtime_queue_summary,
    )
    (
        usb_local_seat_state,
        usb_local_seat_reason,
        usb_busy_after_ready,
    ) = usb_local_seat_state_from_runtime(
        usb_gate,
        usb_blocker,
        usb_runtime_queue_summary,
        usb_post_first_byte_blocker,
    )
    usb_driver_task_blocker = summarize_usb_driver_task_stall(event_list)
    net_driver_task_replay_events, net_driver_task_replay_blocker = (
        summarize_driver_task_replay_status(event_list, "net_driver_task_replay_status")
    )
    sdio_driver_task_replay_events, sdio_driver_task_replay_blocker = (
        summarize_driver_task_replay_status(event_list, "sdio_driver_task_replay_status")
    )
    driver_task_frontiers = summarize_driver_task_frontiers(
        event_list,
        net_driver_task_replay_blocker,
        sdio_driver_task_replay_blocker,
    )
    if usb_driver_task_blocker is not None:
        driver_gate = usb_driver_task_blocker_gate(usb_driver_task_blocker)
        if usb_blocker in {"unknown", "missing", "none"} and usb_gate < 9:
            usb_gate = max(usb_gate, driver_gate)
            usb_blocker = usb_driver_task_blocker
        elif usb_blocker == "pcie-vl805" and usb_frontier_advances_pcie(
            driver_task_frontiers.usb_driver_task_frontier
        ):
            usb_gate = max(usb_gate, driver_gate)
            usb_blocker = usb_driver_task_blocker
    if sdio_driver_task_replay_blocker != "none":
        replay_gate, replay_blocker, replay_exact_default, replay_phase_default = (
            classify_sdio_replay_gate(sdio_driver_task_replay_blocker)
        )
        if wifi_replay_should_refine(wifi_blocker) or replay_gate > wifi_gate:
            wifi_gate = max(wifi_gate, replay_gate)
            wifi_blocker = replay_blocker
            replay_exact, replay_phase, replay_line = summarize_wifi_failure_detail(
                event_list, wifi_blocker
            )
            if replay_exact_default.startswith("wifi-pwrseq-"):
                # The replay record is the authoritative operation-level
                # failure. Later passive diagnostics can repeat only the
                # generic linked-runtime cause; do not let that erase which
                # retained Linux pwrseq transaction actually failed.
                wifi_exact = replay_exact_default
                wifi_phase = replay_phase_default
                wifi_blocker_line = next(
                    (
                        event.line
                        for event in event_list
                        if event.raw.startswith("SDIO_DRIVER_TASK_REPLAY_STATUS")
                        and event.fields.get("blocker") == replay_exact_default
                    ),
                    replay_line,
                )
            elif replay_exact in {
                "cyw43-runtime-command-rejected",
                "cyw43-transport-command-admission",
            }:
                wifi_gate = max(wifi_gate, 6)
                wifi_blocker = "cyw43-runtime-command-rejected"
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            elif replay_exact in {
                "cyw43-probe-cardctrl-read",
                "cyw43-probe-cardctrl-write",
                "cyw43-probe-pmucontrol-read",
                "cyw43-probe-pmucontrol-write",
                "cyw43-probe-function2-disable-read",
                "cyw43-probe-function2-disable-write",
            }:
                wifi_gate = 5
                wifi_blocker = replay_exact
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            elif replay_exact in {
                "cyw43-post-release-ht-clock",
                "cyw43-post-release-function2-ready",
                "cyw43-post-release-corecontrol",
                "cyw43-post-release-mailbox-ready",
                "cyw43-post-release-protocol-version",
                "cyw43-post-release-rframe-sample",
                "cyw43-post-release-dpc-activate",
            }:
                wifi_gate = max(wifi_gate, 7)
                wifi_blocker = replay_exact
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            elif replay_exact != "none":
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            else:
                wifi_exact = replay_exact_default
                wifi_phase = replay_phase_default
    elif net_driver_task_replay_blocker != "none":
        replay_gate, replay_blocker, replay_exact_default, replay_phase_default = (
            classify_net_replay_gate(net_driver_task_replay_blocker)
        )
        replay_precedes_generic_control_summary = (
            replay_blocker == "cyw43-firmware-runtime-replay"
            and wifi_blocker == "control-plane-reply-idle-loop"
            and replay_gate < wifi_gate
        )
        replay_refines_generic_rejection = (
            wifi_blocker == "cyw43-runtime-command-rejected"
            and replay_blocker.startswith("cyw43-engine-init-")
        )
        if (
            wifi_replay_should_refine(wifi_blocker)
            or replay_gate > wifi_gate
            or replay_precedes_generic_control_summary
            or replay_refines_generic_rejection
        ):
            wifi_gate = (
                replay_gate
                if replay_precedes_generic_control_summary
                else max(wifi_gate, replay_gate)
            )
            wifi_blocker = replay_blocker
            replay_exact, replay_phase, replay_line = summarize_wifi_failure_detail(
                event_list, wifi_blocker
            )
            if replay_exact in {
                "cyw43-runtime-command-rejected",
                "cyw43-transport-command-admission",
            }:
                wifi_gate = max(wifi_gate, 6)
                wifi_blocker = "cyw43-runtime-command-rejected"
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            elif replay_exact in {
                "cyw43-probe-cardctrl-read",
                "cyw43-probe-cardctrl-write",
                "cyw43-probe-pmucontrol-read",
                "cyw43-probe-pmucontrol-write",
                "cyw43-probe-function2-disable-read",
                "cyw43-probe-function2-disable-write",
            }:
                wifi_gate = 5
                wifi_blocker = replay_exact
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            elif replay_exact in {
                "cyw43-post-release-ht-clock",
                "cyw43-post-release-function2-ready",
                "cyw43-post-release-corecontrol",
                "cyw43-post-release-mailbox-ready",
                "cyw43-post-release-protocol-version",
                "cyw43-post-release-rframe-sample",
                "cyw43-post-release-dpc-activate",
            }:
                wifi_gate = max(wifi_gate, 7)
                wifi_blocker = replay_exact
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            elif replay_exact != "none":
                wifi_exact = replay_exact
                wifi_phase = replay_phase
                wifi_blocker_line = replay_line
            else:
                wifi_exact = replay_exact_default
                wifi_phase = replay_phase_default
    if (
        wifi_deferred_resume_start is not None
        and net_driver_task_replay_events == 0
        and sdio_driver_task_replay_events == 0
    ):
        wifi_gate = max(wifi_gate, 1)
        wifi_blocker = "wifi-started-no-replay"
        wifi_exact = "wifi-started-no-replay"
        wifi_blocker_line = wifi_deferred_resume_start.line
    if wifi_exact == "none" and wifi_blocker_is_exact_sdio_progress(wifi_blocker):
        wifi_exact = wifi_blocker
        wifi_phase = wifi_blocker
    if wifi_exact == "none" and wifi_blocker_is_exact_cyw43_progress(wifi_blocker):
        wifi_exact = wifi_blocker
    if wifi_exact == "none" and wifi_blocker in {
        "runtime-power-reset",
        "wifi-driver-task-runtime-unproved",
    }:
        wifi_exact = wifi_blocker
    if wifi_blocker == "runtime-power-reset" and not (
        wifi_exact == "wifi-pwrseq-failed"
        or (
            wifi_exact.startswith("wifi-pwrseq-")
            and wifi_exact.endswith("-failed")
        )
        or wifi_exact.startswith("sdio-wl-on-")
    ):
        wifi_exact = wifi_blocker
        wifi_phase = "pwrseq"
        wifi_phase = wifi_blocker
    revinfo_badarg = summarize_cyw43_control_revinfo_badarg(event_list)
    if revinfo_badarg is not None:
        wifi_gate = max(wifi_gate, 7)
        wifi_blocker = "control-plane-revinfo-badarg"
        wifi_exact, wifi_phase, wifi_blocker_line = revinfo_badarg
    txglomalign_reject = summarize_cyw43_control_txglomalign_reject(event_list)
    if txglomalign_reject is not None and wifi_gate <= 9:
        wifi_gate = 7
        wifi_exact, wifi_phase, wifi_blocker_line = txglomalign_reject
        wifi_blocker = wifi_exact
    bssid_tx_submit_fail = summarize_host_eapol_bssid_tx_submit_fail(event_list)
    host_eapol_firstread = summarize_host_eapol_firstread_status(event_list)
    if host_eapol_firstread is not None and (
        wifi_exact not in CYW43_HOST_EAPOL_KEY_INSTALL_BLOCKERS
        and (
            wifi_gate <= 7
            or wifi_exact in {
                "host-eapol-required",
                "wifi-host-eapol-pending",
                "cyw43-association-not-associated",
            }
        )
    ):
        wifi_blocker, wifi_phase, wifi_blocker_line = host_eapol_firstread
        wifi_exact = wifi_blocker
        wifi_gate = max(wifi_gate, 7)
    if (
        bssid_tx_submit_fail is not None
        and wifi_exact != "cyw43-association-not-associated"
        and (
            wifi_gate <= 7
            or cyw43_host_eapol_rx_blocker_name(wifi_exact)
            or wifi_exact in {
                "host-eapol-required",
                "wifi-host-eapol-pending",
                "cyw43-sdio-descriptor-transfer-failed",
            }
        )
    ):
        wifi_blocker, wifi_phase, wifi_blocker_line = bssid_tx_submit_fail
        wifi_exact = wifi_blocker
        wifi_gate = max(wifi_gate, 7)
    if wifi_exact in {"host-eapol-required", "wifi-host-eapol-pending"}:
        wifi_gate = max(wifi_gate, 7)
        wifi_blocker = wifi_exact
        wifi_phase = "join-security"
    if wifi_blocker == "cyw43-transport-command-admission":
        wifi_gate = max(wifi_gate, 6)
        wifi_blocker = "cyw43-runtime-command-rejected"
    control_tx_no_reply = summarize_cyw43_control_tx_no_reply(event_list)
    if control_tx_no_reply is not None and wifi_gate <= 9:
        wifi_gate = 7
        wifi_exact, wifi_phase, wifi_blocker_line = control_tx_no_reply
        wifi_blocker = wifi_exact
    split_descriptor_fault = summarize_cyw43_split_descriptor_fault(event_list)
    if (
        split_descriptor_fault is not None
        and wifi_blocker == "cyw43-sdio-descriptor-transfer-failed"
    ):
        wifi_gate = 7
        wifi_exact, wifi_phase, wifi_blocker_line = split_descriptor_fault
    terminal_card_fault: tuple[str, int] | None = None
    for event in event_list:
        raw = event.raw.lower()
        if raw.startswith("cyw43_driver_task_command_fault"):
            fault = normalize_wifi_blocker(
                event.fields.get("reason")
                or event.fields.get("detail")
                or event.raw
            )
            if fault.startswith("cyw43-transport-card-cmd"):
                terminal_card_fault = (fault, event.line)
                continue
        if (
            terminal_card_fault is not None
            and raw.startswith("driver_task_resource_init")
            and event.fields.get("stage") == "cyw43-transport-init"
            and event.fields.get("status") == "ready"
        ):
            terminal_card_fault = None
    if terminal_card_fault is not None:
        # A retained card-discovery fault is the current Gate-2 frontier even
        # when a later generic replay summary says only that firmware replay
        # failed. A subsequent transport-ready record clears this terminal
        # fault and permits the normal later-gate evidence to take precedence.
        wifi_blocker, wifi_blocker_line = terminal_card_fault
        wifi_gate = 2
        wifi_exact = wifi_blocker
        wifi_phase = "cyw43-transport-init"
    terminal_release_fault = summarize_terminal_cyw43_release_fault(event_list)
    if terminal_release_fault is not None:
        # Passive diagnostics repeat inferred later gate names and their
        # subcommand in ERR lines. Neither is evidence that Function 2 or DHCP
        # was reached, so the last unrecovered hardware RELEASE fault wins.
        wifi_blocker, wifi_phase, wifi_blocker_line = terminal_release_fault
        wifi_gate = 7 if wifi_blocker.startswith("cyw43-post-release-") else 6
        wifi_exact = wifi_blocker
    terminal_diag_failure = summarize_terminal_wifi_diag_failure(event_list)
    if terminal_diag_failure is not None:
        # The explicit dependency-aware device verdict outranks secondary
        # generation-recovery failures and later diagnostic command errors.
        wifi_gate, wifi_blocker, wifi_phase, wifi_blocker_line = (
            terminal_diag_failure
        )
        wifi_exact = wifi_blocker
    if wifi_gate >= 10 and not cohsh_tcp_auth_proof:
        wifi_gate = 9
        wifi_blocker = "tcp-auth-proof-missing"
        wifi_exact = wifi_blocker
        wifi_phase = "tcp-console"
    elif wifi_gate >= 10 and not net_tcp_ready:
        wifi_gate = 9
        wifi_blocker = "tcp-ready-proof-missing"
        wifi_exact = wifi_blocker
        wifi_phase = "tcp-console"
    elif wifi_gate >= 10 and not wifi_dpc.proof:
        wifi_gate = 9
        wifi_blocker = "wifi-dpc-proof-missing"
        wifi_exact = wifi_blocker
        wifi_phase = "cyw43-dpc"
    if driver_task_active_net == "genet" or net_active == "wired":
        wifi_gate = 0
        wifi_blocker = "not-selected"
        wifi_exact = "none"
        wifi_phase = "none"
        wifi_blocker_line = 0
        wifi_oldgood = SequenceResult(
            replay=False,
            last="none",
            missing="not-selected",
        )
    wifi_runtime_evidence_seen = any(
        event.domain in {"wifi", "driver"}
        and (
            event.raw.startswith(("wifi:", "WiFi:", "WIFI:", "CYW43_"))
            or "[pi4-wifi]" in event.raw.lower()
            or "[cyw43]" in event.raw.lower()
            or "[net-console]" in event.raw.lower()
            or "hot_path=cyw43-wifi" in event.raw.lower()
            or "hot_path=sdio-host" in event.raw.lower()
        )
        for event in event_list
    )
    if boot_halted and not wifi_runtime_evidence_seen:
        wifi_gate = 0
        wifi_blocker = "boot-halted-before-wifi"
        wifi_exact = boot_halt_reason
        wifi_phase = "root-bootstrap"
        wifi_blocker_line = halt_events[0].line if halt_events else 0
    wifi_subgate = summarize_wifi_gate7_subgate_detail(
        event_list, wifi_gate, wifi_blocker
    )
    wifi_gate7 = (
        WifiGate7Proof(missing="not-selected")
        if wifi_blocker == "not-selected"
        else summarize_wifi_gate7_proof(event_list)
    )
    return GateSummary(
        usb_gate=usb_gate,
        usb_blocker=usb_blocker,
        wifi_gate=wifi_gate,
        wifi_blocker=wifi_blocker,
        wifi_subgate=wifi_subgate.subgate,
        wifi_subgate_name=wifi_subgate.name,
        wifi_subgate_source=wifi_subgate.source,
        wifi_subgate_status=wifi_subgate.status,
        wifi_subgate_reason=wifi_subgate.reason,
        wifi_subgate_line=wifi_subgate.line,
        wifi_gate7_complete=wifi_gate7.complete,
        wifi_gate7_seen=wifi_gate7.seen,
        wifi_gate7_last=wifi_gate7.last,
        wifi_gate7_missing=wifi_gate7.missing,
        usb_oldgood_replay=usb_oldgood.replay,
        usb_oldgood_last=usb_oldgood.last,
        usb_oldgood_missing=usb_oldgood.missing,
        wifi_oldgood_replay=wifi_oldgood.replay,
        wifi_oldgood_last=wifi_oldgood.last,
        wifi_oldgood_missing=wifi_oldgood.missing,
        wifi_exact=wifi_exact,
        wifi_phase=wifi_phase,
        wifi_blocker_line=wifi_blocker_line,
        serial_clean=serial_clean(event_list),
        boot_halted=boot_halted,
        timer_irq27_seen=timer_irq27_seen,
        timer_backend=timer_backend,
        timer_clock_hz=timer_clock_hz,
        timer_el0_counter=timer_el0_counter,
        dummy_timer_seen=dummy_timer_seen,
        sdio_irq158_seen=sdio_irq158_seen,
        sdio_irq158_bound=sdio_irq158_bound,
        sdio_irq158_inband_proof=sdio_irq158_inband_proof,
        sdio_irq158_line=sdio_irq158_line,
        boot_halt_reason=boot_halt_reason,
        panic_seen=panic_seen,
        panic_reason=panic_reason,
        usb_bootloader_handoff_seen=usb_bootloader_handoff_seen,
        usb_cold_boot_seen=usb_cold_boot_seen,
        usb_stale_uefi_hint_seen=usb_stale_uefi_hint_seen,
        usb_event_ring_alive=usb_event_ring_alive,
        usb_psc_drain_count=usb_psc_drain_count,
        usb_psc_drain_mask=usb_psc_drain_mask,
        root_console_ready=root_console_ready,
        root_prompt_seen=root_prompt_seen,
        net_active=net_active,
        net_addr_src=net_addr_src,
        net_dhcp=net_dhcp,
        net_tcp_ready=net_tcp_ready,
        nettest_proof=nettest_proof,
        cohsh_tcp_auth_proof=cohsh_tcp_auth_proof,
        tcp_accepts=tcp_accepts,
        tcp_auth_sessions=tcp_auth_sessions,
        tcp_rx_bytes=tcp_rx_bytes,
        wifi_data_path_tx=wifi_data_path_tx,
        wifi_data_path_rx_preserved=wifi_data_path_rx_preserved,
        wifi_data_path_rx_delivered=wifi_data_path_rx_delivered,
        wifi_data_path_rx_dropped=wifi_data_path_rx_dropped,
        wifi_data_path_last=wifi_data_path_last,
        wifi_service_op=wifi_service_op,
        wifi_service_reason=wifi_service_reason,
        wifi_service_progress=wifi_service_progress,
        wifi_service_seq=wifi_service_seq,
        wifi_service_credit=wifi_service_credit,
        wifi_service_channel=wifi_service_channel,
        wifi_service_rframe=wifi_service_rframe,
        wifi_service_eapol_m1=wifi_service_eapol_m1,
        wifi_service_eapol_m2=wifi_service_eapol_m2,
        wifi_service_eapol_m3=wifi_service_eapol_m3,
        wifi_service_eapol_m4=wifi_service_eapol_m4,
        wifi_rx_irq_preserve_count=wifi_rx_irq_preserve_count,
        wifi_rx_irq_preserve_reason=wifi_rx_irq_preserve_reason,
        wifi_rx_irq_preserve_int=wifi_rx_irq_preserve_int,
        wifi_rx_irq_preserve_ack=wifi_rx_irq_preserve_ack,
        wifi_rx_irq_episode_preserves=wifi_rx_irq_episode_preserves,
        wifi_rx_irq_episode_rearms=wifi_rx_irq_episode_rearms,
        wifi_rxtrace_seq=wifi_rxtrace_seq,
        wifi_rxtrace_start_ticks=wifi_rxtrace_start_ticks,
        wifi_rxtrace_pre_sample_delta_ticks=wifi_rxtrace_pre_sample_delta_ticks,
        wifi_rxtrace_transfer_delta_ticks=wifi_rxtrace_transfer_delta_ticks,
        wifi_rxtrace_post_sample_delta_ticks=wifi_rxtrace_post_sample_delta_ticks,
        wifi_dpc_proof=wifi_dpc.proof,
        wifi_dpc_reason=wifi_dpc.reason,
        wifi_dpc_generation=wifi_dpc.generation,
        wifi_dpc_captures=wifi_dpc.captures,
        wifi_dpc_published=wifi_dpc.published,
        wifi_dpc_consumed=wifi_dpc.consumed,
        wifi_dpc_rearms=wifi_dpc.rearms,
        wifi_dpc_overruns=wifi_dpc.overruns,
        wifi_dpc_epoch_errors=wifi_dpc.epoch_errors,
        wifi_dpc_sequence_errors=wifi_dpc.sequence_errors,
        wifi_dpc_ack_failures=wifi_dpc.ack_failures,
        wifi_dpc_poisoned=wifi_dpc.poisoned,
        wifi_dpc_masked=wifi_dpc.masked,
        wifi_dpc_line=wifi_dpc.line,
        cyw43_bootstrap_supervisor_seen=cyw43_bootstrap_supervisor.seen,
        cyw43_bootstrap_supervisor_max_attempt=(
            cyw43_bootstrap_supervisor.max_attempt
        ),
        cyw43_bootstrap_supervisor_transient_retries=(
            cyw43_bootstrap_supervisor.transient_retries
        ),
        cyw43_bootstrap_supervisor_last_status=(
            cyw43_bootstrap_supervisor.last_status
        ),
        cyw43_bootstrap_supervisor_ready=cyw43_bootstrap_supervisor.ready,
        cyw43_bootstrap_supervisor_blocker=cyw43_bootstrap_supervisor.blocker,
        wifi_firmware_identity_proof=wifi_firmware_identity_proof,
        wifi_firmware_identity_blocker=wifi_firmware_identity_blocker,
        wifi_clm_ready_proof=wifi_clm_ready_proof,
        wifi_firmware_version_proof=wifi_firmware_version_proof,
        wifi_clm_version_proof=wifi_clm_version_proof,
        driver_task_default_requested=driver_task_default_requested,
        driver_task_live_hot_paths=driver_task_live_hot_paths,
        driver_task_contracts=driver_task_contracts,
        driver_task_dedicated=driver_task_dedicated,
        driver_task_compatibility=driver_task_compatibility,
        driver_task_dedicated_ready=driver_task_dedicated_ready,
        driver_task_serial_dedicated=driver_task_serial_dedicated,
        driver_task_usb_dedicated=driver_task_usb_dedicated,
        driver_task_display_dedicated=driver_task_display_dedicated,
        driver_task_net_dedicated=driver_task_net_dedicated,
        driver_task_sdio_dedicated=driver_task_sdio_dedicated,
        driver_task_pcie_dedicated=driver_task_pcie_dedicated,
        driver_task_substrate_ready=driver_task_substrate_ready,
        driver_task_failed_count=driver_task_failed_count,
        driver_task_capset_proof=driver_task_capset_proof,
        driver_task_fault_proof=driver_task_fault_proof,
        driver_task_revoke_proof=driver_task_revoke_proof,
        driver_task_sched_proof=driver_task_sched_proof,
        driver_task_affinity_proof=driver_task_affinity_proof,
        driver_task_affinity_configured=driver_task_affinity_configured,
        driver_task_affinity_applied=driver_task_affinity_applied,
        driver_task_affinity_manifest_proof=driver_task_affinity_manifest_proof,
        driver_task_affinity_manifest_matches=driver_task_affinity_manifest_matches,
        driver_task_affinity_manifest_missing=driver_task_affinity_manifest_missing,
        driver_task_affinity_manifest_mismatches=driver_task_affinity_manifest_mismatches,
        driver_task_notification_bind_deferred=driver_task_notification_bind_deferred,
        driver_task_vspace_proof=driver_task_vspace_proof,
        driver_task_pointer_free_ipc_proof=driver_task_pointer_free_ipc_proof,
        driver_task_owner_state_proof=driver_task_owner_state_proof,
        driver_task_runtime_descriptor_seal_proof=(
            driver_task_runtime_descriptor_seal_proof
        ),
        driver_task_runtime_descriptor_seal_proofs=(
            driver_task_runtime_descriptor_seal_proofs
        ),
        driver_task_runtime_descriptor_seal_blocker=(
            driver_task_runtime_descriptor_seal_blocker
        ),
        driver_task_active_net=driver_task_active_net,
        driver_task_budget_overruns=driver_task_budget_overruns,
        driver_task_latency_proofs=driver_task_latency_proofs,
        driver_task_ring_call_begin=driver_task_ring_call_begin,
        driver_task_ring_call_return=driver_task_ring_call_return,
        driver_task_ring_call_outstanding=driver_task_ring_call_outstanding,
        driver_task_ring_call_timeout=driver_task_ring_call_timeout,
        driver_task_ring_call_unresolved_timeout=(
            driver_task_ring_call_unresolved_timeout
        ),
        driver_task_ring_call_keep_active=driver_task_ring_call_keep_active,
        driver_task_ring_call_abort=driver_task_ring_call_abort,
        driver_task_bootstrap_deferred=driver_task_bootstrap_deferred,
        driver_task_resource_init=driver_task_resource_init,
        driver_task_resource_blocker=driver_task_resource_blocker,
        driver_task_resource_current_blocker=driver_task_resource_current_blocker,
        driver_task_counter_snapshots=driver_task_counter_summary.snapshots,
        driver_task_counter_invalid=driver_task_counter_summary.invalid,
        driver_task_counter_busy=driver_task_counter_summary.busy,
        driver_task_counter_same_request=driver_task_counter_summary.same_request,
        driver_task_counter_timeouts=driver_task_counter_summary.timeouts,
        driver_task_counter_keep_active=driver_task_counter_summary.keep_active,
        driver_task_counter_aborts=driver_task_counter_summary.aborts,
        driver_task_counter_overruns=driver_task_counter_summary.overruns,
        driver_task_counter_drops=driver_task_counter_summary.drops,
        driver_task_counter_staged_bytes=driver_task_counter_summary.staged_bytes,
        driver_task_counter_cache_ops=driver_task_counter_summary.cache_ops,
        driver_task_counter_cache_bytes=driver_task_counter_summary.cache_bytes,
        driver_task_counter_rx_frames=driver_task_counter_summary.rx_frames,
        driver_task_counter_tx_frames=driver_task_counter_summary.tx_frames,
        driver_task_counter_rx_bytes=driver_task_counter_summary.rx_bytes,
        driver_task_counter_tx_bytes=driver_task_counter_summary.tx_bytes,
        driver_task_dma_proofs=driver_task_dma_proofs,
        driver_task_dma_blocker=driver_task_dma_blocker,
        pi4_runtime_dma_proof=pi4_runtime_dma_proof,
        pi4_runtime_dma_proof_reason=pi4_runtime_dma_proof_reason,
        pi4_runtime_dma_counter_proof=pi4_runtime_dma_counter_proof,
        serial_output_tx_pending=output_pressure_summary.serial_tx_pending,
        serial_output_interactive=output_pressure_summary.serial_interactive,
        serial_output_deferred=output_pressure_summary.serial_deferred,
        serial_output_flushed=output_pressure_summary.serial_flushed,
        serial_output_backpressure=output_pressure_summary.serial_backpressure,
        hdmi_display_pending_bytes=output_pressure_summary.hdmi_pending_bytes,
        hdmi_display_pending_redraw=output_pressure_summary.hdmi_pending_redraw,
        hdmi_display_submitted=output_pressure_summary.hdmi_submitted,
        hdmi_display_deferred=output_pressure_summary.hdmi_deferred,
        hdmi_display_busy=output_pressure_summary.hdmi_busy,
        hdmi_display_no_reply=output_pressure_summary.hdmi_no_reply,
        hdmi_display_coalesced=output_pressure_summary.hdmi_coalesced,
        hdmi_display_backpressure_bytes=(
            output_pressure_summary.hdmi_backpressure_bytes
        ),
        hdmi_display_superseded_bytes=(
            output_pressure_summary.hdmi_superseded_bytes
        ),
        usb_keyboard_no_replies=usb_keyboard_pressure_summary.no_replies,
        usb_keyboard_poll_cooldown=usb_keyboard_pressure_summary.poll_cooldown,
        usb_keyboard_cooldown_skips=usb_keyboard_pressure_summary.cooldown_skips,
        usb_runtime_queued_reports=usb_runtime_queue_summary.queued_reports,
        usb_runtime_transfer_events=usb_runtime_queue_summary.transfer_events,
        usb_runtime_report_status=usb_runtime_queue_summary.report_status,
        usb_runtime_queue_valid=usb_runtime_queue_summary.queue_valid,
        usb_runtime_doorbell_pending=(
            usb_runtime_queue_summary.doorbell_pending
        ),
        usb_runtime_recovery_diag_valid=(
            usb_runtime_queue_summary.recovery_diag_valid
        ),
        usb_runtime_endpoint_recoveries=(
            usb_runtime_queue_summary.endpoint_recoveries
        ),
        usb_runtime_endpoint_recovery_failures=(
            usb_runtime_queue_summary.endpoint_recovery_failures
        ),
        usb_runtime_queue_collapse_recoveries=(
            usb_runtime_queue_summary.queue_collapse_recoveries
        ),
        usb_runtime_recovery_stage=usb_runtime_queue_summary.recovery_stage,
        usb_runtime_recovery_reason=usb_runtime_queue_summary.recovery_reason,
        usb_runtime_command_completion_blocked=(
            usb_runtime_queue_summary.command_completion_blocked
        ),
        usb_event_loop_runtime_skipped=usb_runtime_queue_summary.runtime_skipped,
        usb_post_first_byte_blocker=usb_post_first_byte_blocker,
        usb_startup_blocker_seen=usb_runtime_queue_summary.startup_blocker_seen,
        usb_active_blocker_seen=usb_runtime_queue_summary.active_blocker_seen,
        usb_recovered_from_blocker=usb_runtime_queue_summary.recovered_from_blocker,
        usb_recovery_state=usb_runtime_queue_summary.recovery_state,
        usb_local_seat_state=usb_local_seat_state,
        usb_local_seat_reason=usb_local_seat_reason,
        usb_command_ready=usb_runtime_queue_summary.command_ready,
        usb_first_report_ready=usb_runtime_queue_summary.first_report_ready,
        usb_first_byte_ready=usb_runtime_queue_summary.first_byte_ready,
        usb_busy_after_ready=usb_busy_after_ready,
        serial_driver_accepted=driver_task_frontiers.serial_driver_accepted,
        serial_fallback_active=driver_task_frontiers.serial_fallback_active,
        serial_runtime_frontier=driver_task_frontiers.serial_runtime_frontier,
        hdmi_descriptor_ready=driver_task_frontiers.hdmi_descriptor_ready,
        hdmi_engine_ready=driver_task_frontiers.hdmi_engine_ready,
        hdmi_owner_state_ready=driver_task_frontiers.hdmi_owner_state_ready,
        hdmi_runtime_frontier=driver_task_frontiers.hdmi_runtime_frontier,
        usb_driver_task_frontier=driver_task_frontiers.usb_driver_task_frontier,
        wifi_replay_frontier=driver_task_frontiers.wifi_replay_frontier,
        net_driver_task_replay_events=net_driver_task_replay_events,
        net_driver_task_replay_blocker=net_driver_task_replay_blocker,
        sdio_driver_task_replay_events=sdio_driver_task_replay_events,
        sdio_driver_task_replay_blocker=sdio_driver_task_replay_blocker,
        serial_responsive_proof=serial_responsive_proof,
        usb_burst_proof=usb_burst_proof,
        usb_burst_drops=usb_burst_drops,
        hdmi_responsive_proof=hdmi_responsive_proof,
    )


def clear_resource_blockers(
    current_blockers: dict[tuple[str, str, str], str],
    *,
    contract: str | None = None,
    hot_path: str | None = None,
) -> None:
    """Clear stale resource blockers after later owner-state or DMA proof."""

    stale_keys = [
        blocker_key
        for blocker_key in current_blockers
        if (contract is None or blocker_key[0] == contract)
        and (hot_path is None or blocker_key[1] == hot_path)
    ]
    for blocker_key in stale_keys:
        current_blockers.pop(blocker_key, None)


def summarize_driver_task_resource_init(
    events: Iterable[TraceEvent],
) -> tuple[int, str, str]:
    """Return resource-init breadcrumb count, first blocker, and current blocker."""

    non_blocking_statuses = {
        "ready",
        "deferred",
        "begin",
        "progress",
        "preserved-ready",
        "cached-ready",
        "sdio-owner-replay",
        "resume-retained-stage",
        "stale-admission-retry",
    }
    first_blocker = "none"
    current_blockers: dict[tuple[str, str, str], str] = {}
    resource_count = 0
    for event in events:
        raw = event.raw.lower()
        if "driver_task_acceptance" in raw and (
            _truthy_field(event.fields.get("dedicated_ready"))
            and field_lower(event, "proof_effect") == "acceptance-green"
        ):
            current_blockers.clear()
            continue
        if (
            "driver_task_owner_state" in raw
            and _owner_state_proven(event.fields)
            and event.fields.get("descriptor", "").lower() == "present"
            and event.fields.get("root_pointer", "").lower() == "no"
        ):
            hot_path = classify_owner_state_hot_path(event.fields)
            if hot_path is not None:
                clear_resource_blockers(
                    current_blockers,
                    contract=event.fields.get("contract", "unknown"),
                    hot_path=hot_path,
                )
            continue
        if "driver_task_dma_proof" in raw and is_ready_driver_task_dma_proof(event):
            hot_path = classify_owner_state_hot_path(event.fields)
            if hot_path is not None:
                clear_resource_blockers(
                    current_blockers,
                    contract=event.fields.get("contract", "unknown"),
                    hot_path=hot_path,
                )
            continue
        if "driver_task_resource_init" not in raw:
            continue
        resource_count += 1
        status = event.fields.get("status", "unknown").lower()
        contract = event.fields.get("contract", "unknown")
        hot_path = event.fields.get("hot_path", contract)
        stage = event.fields.get("stage", "unknown")
        key = (contract, hot_path, stage)
        if status not in non_blocking_statuses:
            blocker = f"{hot_path}:{stage}:{status}"
            if first_blocker == "none":
                first_blocker = blocker
            current_blockers.pop(key, None)
            current_blockers[key] = blocker
        else:
            current_blockers.pop(key, None)
            if status in {"ready", "preserved-ready", "cached-ready"}:
                clear_resource_blockers(
                    current_blockers,
                    contract=contract,
                    hot_path=hot_path,
                )
    current_blocker = "none"
    if current_blockers:
        current_blocker = next(reversed(current_blockers.values()))
    return resource_count, first_blocker, current_blocker


def summarize_driver_task_bootstrap_deferred(events: Iterable[TraceEvent]) -> int:
    """Return unresolved shell-first bootstrap deferrals by contract."""

    deferred_contracts: dict[str, bool] = {}
    resolved_statuses = {"ready", "resumed", "preserved-ready", "cached-ready"}
    for event in events:
        raw = event.raw.lower()
        contract = event.fields.get("contract", "").lower()
        if not contract:
            continue
        if "driver_task_bootstrap_deferred" in raw:
            deferred_contracts[contract] = True
            continue
        if (
            "driver_task_runtime_init_deferred" in raw
            and event.fields.get("status", "").lower() == "resumed"
        ):
            deferred_contracts[contract] = False
            continue
        if (
            "driver_task_owner_state" in raw
            and _owner_state_proven(event.fields)
            and event.fields.get("descriptor", "").lower() == "present"
            and event.fields.get("root_pointer", "").lower() == "no"
        ):
            deferred_contracts[contract] = False
            continue
        if "driver_task_dma_proof" in raw and is_ready_driver_task_dma_proof(event):
            deferred_contracts[contract] = False
            continue
        if "driver_task_resource_init" not in raw:
            continue
        status = event.fields.get("status", "").lower()
        stage = event.fields.get("stage", "").lower()
        if status not in resolved_statuses:
            continue
        if stage == "runtime-descriptor-replay" or stage.endswith("owner-state"):
            deferred_contracts[contract] = False
    return sum(1 for deferred in deferred_contracts.values() if deferred)


def summarize_driver_task_runtime_descriptor_seals(
    events: Iterable[TraceEvent], selected_net: str, active_net: str
) -> tuple[int, str]:
    """Return sealed runtime-descriptor proof count and current blocker."""

    required = required_driver_task_owner_hot_paths(selected_net, active_net)
    ready: set[str] = set()
    current_blockers: dict[str, str] = {}
    evidence_seen = False
    for event in events:
        raw = event.raw.lower()
        if "driver_task_owner_state" not in raw and "driver_task_dma_proof" not in raw:
            continue
        hot_path = classify_owner_state_hot_path(event.fields)
        if hot_path is None or hot_path not in required:
            continue
        owner_state_ready = (
            "driver_task_owner_state" in raw
            and _owner_state_proven(event.fields)
            and event.fields.get("descriptor", "").lower() == "present"
            and event.fields.get("root_pointer", "").lower() == "no"
        )
        dma_ready = "driver_task_dma_proof" in raw and is_ready_driver_task_dma_proof(event)
        if not owner_state_ready and not dma_ready:
            continue
        evidence_seen = True
        seal_status = driver_runtime_descriptor_seal_status(event.fields)
        if seal_status == "ready":
            ready.add(hot_path)
            current_blockers.pop(hot_path, None)
        elif hot_path not in ready:
            current_blockers[hot_path] = f"{hot_path}:{seal_status}"
    if current_blockers:
        return len(ready), next(reversed(current_blockers.values()))
    missing = sorted(required - ready)
    if evidence_seen and missing:
        return len(ready), f"missing:{missing[0]}"
    return len(ready), "none"


def is_ready_driver_task_dma_proof(event: TraceEvent) -> bool:
    """Return whether a DMA proof line satisfies the bounded Pi 4 profile."""

    status = event.fields.get("status", "unknown").lower()
    profile = event.fields.get("profile", "unknown").lower()
    descriptor = event.fields.get("descriptor", "unknown").lower()
    root_pointer = event.fields.get("root_pointer", "unknown").lower()
    bus_policy = event.fields.get("bus_address_policy", "unknown").lower()
    dma_pages = parse_hex_int(event.fields.get("dma_pages")) or 0
    cache_policy = event.fields.get("cache_policy", "unknown").lower()
    return (
        status == "ready"
        and profile == "bounded-no-iommu"
        and descriptor == "present"
        and root_pointer == "no"
        and cache_policy == "uncached-plus-root-maintenance"
        and (
            (dma_pages == 0 and bus_policy == "zero-dma")
            or (dma_pages > 0 and bus_policy == "hal-bounded-bus-address")
        )
    )


def summarize_driver_task_dma_proofs(
    events: Iterable[TraceEvent], selected_net: str, active_net: str
) -> tuple[int, str]:
    """Return ready DMA proof count and the current concrete DMA proof blocker."""

    required = required_driver_task_owner_hot_paths(selected_net, active_net)
    ready: set[str] = set()
    current_blockers: dict[str, str] = {}
    for event in events:
        if "driver_task_dma_proof" not in event.raw.lower():
            continue
        hot_path = classify_owner_state_hot_path(event.fields)
        if hot_path is None:
            continue
        bus_policy = event.fields.get("bus_address_policy", "unknown").lower()
        status = event.fields.get("status", "unknown").lower()
        if is_ready_driver_task_dma_proof(event):
            ready.add(hot_path)
            current_blockers.pop(hot_path, None)
        elif hot_path in required and hot_path not in ready:
            current_blockers[hot_path] = f"{hot_path}:{status}:{bus_policy}"
    if current_blockers:
        return len(ready), next(reversed(current_blockers.values()))
    missing = sorted(required - ready)
    if missing:
        return len(ready), f"missing:{missing[0]}"
    return len(ready), "none"


def summarize_driver_task_ring_call_timeouts(
    events: Iterable[TraceEvent],
) -> tuple[int, int]:
    """Return total timeout events and timeout frontiers not closed by returns."""

    total = 0
    unresolved: set[tuple[str, str, int]] = set()
    for event in events:
        raw = event.raw.lower()
        if (
            "driver_task_ring_call_timeout" not in raw
            and "driver_task_ring_call_return" not in raw
        ):
            continue
        request = parse_hex_int(event.fields.get("request"))
        if request is None:
            continue
        key = (
            event.fields.get("contract", "").lower(),
            event.fields.get("endpoint", "").lower(),
            request,
        )
        if "driver_task_ring_call_timeout" in raw:
            total += 1
            unresolved.add(key)
        elif "driver_task_ring_call_return" in raw:
            unresolved.discard(key)
    return total, len(unresolved)


def classify_pi4_runtime_dma_proof(
    *,
    root_prompt_seen: bool,
    usb_cold_boot_seen: bool,
    driver_task_dedicated_ready: bool,
    driver_task_compatibility: int,
    driver_task_substrate_ready: bool,
    driver_task_failed_count: int,
    driver_task_capset_proof: bool,
    driver_task_fault_proof: bool,
    driver_task_revoke_proof: bool,
    driver_task_sched_proof: bool,
    driver_task_affinity_proof: bool,
    driver_task_vspace_proof: bool,
    driver_task_pointer_free_ipc_proof: bool,
    driver_task_owner_state_proof: bool,
    driver_task_runtime_descriptor_seal_proof: bool,
    driver_task_ring_call_outstanding: int,
    driver_task_ring_call_unresolved_timeout: int,
    driver_task_bootstrap_deferred: int,
    driver_task_resource_blocker: str,
    driver_task_resource_current_blocker: str,
    driver_task_dma_proofs: int,
    driver_task_dma_blocker: str,
    timer_backend: str,
    timer_clock_hz: int,
    timer_el0_counter: str,
    dummy_timer_seen: bool,
    driver_task_counter_summary: DriverTaskCounterSummary,
) -> tuple[str, str, str]:
    """Classify Pi 4 runtime/DMA proof without inferring hardware success."""

    runtime_checks = (
        driver_task_dedicated_ready,
        driver_task_substrate_ready,
        driver_task_failed_count == 0,
        driver_task_capset_proof,
        driver_task_fault_proof,
        driver_task_revoke_proof,
        driver_task_sched_proof,
        driver_task_affinity_proof,
        driver_task_vspace_proof,
        driver_task_pointer_free_ipc_proof,
        driver_task_owner_state_proof,
        driver_task_runtime_descriptor_seal_proof,
        driver_task_compatibility == 0,
        driver_task_ring_call_outstanding == 0,
        driver_task_ring_call_unresolved_timeout == 0,
        driver_task_bootstrap_deferred == 0,
        driver_task_resource_current_blocker == "none",
        driver_task_dma_blocker == "none",
    )
    proof_evidence_seen = (
        driver_task_dedicated_ready
        or driver_task_substrate_ready
        or driver_task_failed_count != 0
        or driver_task_capset_proof
        or driver_task_fault_proof
        or driver_task_revoke_proof
        or driver_task_sched_proof
        or driver_task_affinity_proof
        or driver_task_vspace_proof
        or driver_task_pointer_free_ipc_proof
        or driver_task_owner_state_proof
        or driver_task_runtime_descriptor_seal_proof
        or driver_task_compatibility != 0
        or driver_task_ring_call_outstanding != 0
        or driver_task_ring_call_unresolved_timeout != 0
        or driver_task_bootstrap_deferred != 0
        or driver_task_resource_blocker != "none"
        or driver_task_resource_current_blocker != "none"
        or driver_task_dma_proofs != 0
        or driver_task_counter_summary.snapshots != 0
    )
    if not proof_evidence_seen:
        return "absent", "no-runtime-proof-lines", "absent"
    if not all(runtime_checks):
        return "diagnostic", "runtime-proof-incomplete", "diagnostic"

    counter_qualified = (
        timer_backend == "arch-counter"
        and timer_clock_hz == 54_000_000
        and timer_el0_counter == "vct"
        and not dummy_timer_seen
        and driver_task_counter_summary.snapshots > 0
        and driver_task_counter_summary.invalid == 0
    )
    counter_state = "counter-qualified" if counter_qualified else "diagnostic"

    if root_prompt_seen and usb_cold_boot_seen:
        return "fresh-pi", "live-pi-owner-state", counter_state
    return "qemu-or-stale-log", "missing-fresh-pi-boot-markers", counter_state


def summarize_driver_task_frontiers(
    events: Iterable[TraceEvent],
    net_replay_blocker: str = "none",
    sdio_replay_blocker: str = "none",
) -> DriverTaskFrontiers:
    """Return fail-closed frontiers for driver-task acceptance triage."""

    serial_driver_accepted = False
    serial_fallback_active = False
    serial_status: str | None = None
    serial_descriptor_blocker: str | None = None
    serial_runtime_requests: set[int] = set()
    hdmi_descriptor_ready = False
    hdmi_engine_ready = False
    hdmi_owner_state_ready = False
    hdmi_engine_blocker: str | None = None
    hdmi_boot_blocker: str | None = None
    usb_frontier = "none"
    wifi_pre_prompt_deferred = False
    non_blocking_statuses = {
        "ready",
        "deferred",
        "begin",
        "progress",
        "preserved-ready",
        "cached-ready",
        "sdio-owner-replay",
        "resume-retained-stage",
        "stale-admission-retry",
    }

    for event in events:
        raw = event.raw.lower()
        fields = event.fields
        contract = fields.get("contract", "").lower()
        hot_path = fields.get("hot_path", "").lower()
        stage = fields.get("stage", "").lower()
        status = fields.get("status", "").lower()

        if "serial_runtime_state" in raw:
            owner = fields.get("owner", "").lower()
            serial_status = status or fields.get("state", "").lower()
            if owner == "root" and serial_status in {"fallback", "cutover-deferred"}:
                serial_fallback_active = True
                serial_driver_accepted = False
            if (
                owner == "root"
                and serial_status == "cutover"
                and fields.get("acceptance", "").lower() == "green"
            ):
                serial_driver_accepted = True
                serial_fallback_active = False
            if (
                owner == "driver"
                and serial_status == "ready"
                and fields.get("acceptance", "").lower() == "green"
            ):
                serial_driver_accepted = True
                serial_fallback_active = False

        if "root-mini-uart-fallback" in raw:
            serial_fallback_active = True

        if "driver_task_owner_state" in raw:
            owner_state = fields.get("owner_state", "").lower()
            descriptor = fields.get("descriptor", "").lower()
            root_pointer = fields.get("root_pointer", "").lower()
            if (
                hot_path == "serial-console"
                and owner_state == "driver-owned"
                and descriptor == "present"
                and root_pointer == "no"
            ):
                serial_driver_accepted = True
                serial_fallback_active = False
            if (
                hot_path == "hdmi-text"
                and owner_state == "driver-owned"
                and descriptor == "present"
                and root_pointer == "no"
            ):
                hdmi_owner_state_ready = True

        if "driver_task_boot" in raw:
            owner_state = fields.get("owner_state", "").lower()
            pointer_free_ipc = fields.get("pointer_free_ipc", "").lower()
            status = fields.get("status", "").lower()
            if (
                contract == "serial"
                and owner_state == "driver-owned"
                and pointer_free_ipc == "yes"
            ):
                serial_driver_accepted = True
                serial_fallback_active = False
            if contract == "hdmi-text" and status == "failed":
                hdmi_boot_blocker = "boot-failed"

        if "driver_task_ring_call_begin" in raw and contract == "serial":
            aux0 = fields.get("aux0", "").lower()
            request = parse_hex_int(fields.get("request"))
            if aux0 == "0x53455249" and request is not None:
                serial_runtime_requests.add(request)
        if (
            (
                "driver_task_ring_call_return" in raw
                or "driver_task_ring_call_timeout" in raw
                or "driver_task_ring_call_abort" in raw
            )
            and contract == "serial"
        ):
            request = parse_hex_int(fields.get("request"))
            if request is not None:
                serial_runtime_requests.discard(request)
            if "driver_task_ring_call_timeout" in raw:
                serial_status = "no-reply"

        if "driver_task_bootstrap_deferred" in raw and contract in {
            "sdio-host",
            "cyw43455",
            "bcmgenet-v5",
        }:
            wifi_pre_prompt_deferred = True

        if "driver_task_hdmi_early_ready" in raw:
            if fields.get("engine_init", "").lower() == "yes":
                hdmi_engine_ready = True
            elif fields.get("engine_init", "").lower() == "no":
                hdmi_engine_blocker = "no-reply"
            if fields.get("owner_state", "").lower() == "yes":
                hdmi_owner_state_ready = True

        if "driver_task_resource_init" not in raw:
            continue

        if hot_path == "serial-console" and stage == "serial-runtime-init":
            serial_status = status
        if hot_path == "serial-console" and status not in non_blocking_statuses:
            serial_descriptor_blocker = f"{stage}-{status}"

        if hot_path == "hdmi-text":
            if stage == "runtime-descriptor-bootstrap" and status == "ready":
                hdmi_descriptor_ready = True
            if stage == "hdmi-engine-init":
                if status == "ready":
                    hdmi_engine_ready = True
                elif status not in non_blocking_statuses:
                    hdmi_engine_blocker = status
            if stage == "hdmi-owner-state" and status == "ready":
                hdmi_owner_state_ready = True

        if (
            contract == "pcie-root"
            and stage == "usb-prereq-pcie-replay"
            and (
                usb_frontier == "none"
                or usb_frontier.startswith("usb-prereq-")
                or usb_frontier == "usb-runtime-descriptor-bootstrap-ready"
            )
        ):
            usb_frontier = f"usb-prereq-pcie-replay-{status}"
        if (
            contract == "pcie-root"
            and stage == "usb-prereq-pcie-engine-init"
            and (
                usb_frontier == "none"
                or usb_frontier.startswith("usb-prereq-")
                or usb_frontier == "usb-runtime-descriptor-bootstrap-ready"
            )
        ):
            usb_frontier = f"usb-prereq-pcie-engine-init-{status}"
        if hot_path == "usb-keyboard" and stage:
            if status not in non_blocking_statuses:
                usb_frontier = f"{stage}-{status}"
            elif usb_frontier == "none" and stage == "runtime-descriptor-bootstrap":
                usb_frontier = "usb-runtime-descriptor-bootstrap-ready"
            elif stage in {
                "usb-owner-state",
                "usb-keyboard-first-report",
                "usb-runtime-gate10",
            } and status in {"ready", "preserved-ready"}:
                usb_frontier = f"{stage}-{status}"
            elif usb_frontier == "none" and stage.startswith("usb-"):
                usb_frontier = f"{stage}-{status}"

    if serial_driver_accepted:
        serial_frontier = "serial-driver-owner-state-ready"
    elif serial_fallback_active:
        serial_frontier = "serial-root-fallback"
    elif serial_runtime_requests:
        serial_frontier = "serial-runtime-init-outstanding"
    elif serial_status == "no-reply":
        serial_frontier = "serial-runtime-init-no-reply"
    elif serial_status:
        serial_frontier = f"serial-runtime-init-{serial_status}"
    elif serial_descriptor_blocker:
        serial_frontier = f"serial-{serial_descriptor_blocker}"
    else:
        serial_frontier = "none"

    if hdmi_owner_state_ready:
        hdmi_frontier = "hdmi-owner-state-ready"
    elif hdmi_engine_ready:
        hdmi_frontier = "hdmi-engine-ready-owner-pending"
    elif hdmi_boot_blocker:
        hdmi_frontier = f"hdmi-engine-init-{hdmi_boot_blocker}"
    elif hdmi_engine_blocker:
        hdmi_frontier = f"hdmi-engine-init-{hdmi_engine_blocker}"
    elif hdmi_descriptor_ready:
        hdmi_frontier = "hdmi-runtime-descriptor-bootstrap-ready"
    else:
        hdmi_frontier = "none"

    if sdio_replay_blocker != "none":
        wifi_frontier = "sdio-driver-task-replay"
    elif net_replay_blocker != "none":
        wifi_frontier = "cyw43-driver-task-replay"
    elif wifi_pre_prompt_deferred:
        wifi_frontier = "pre-prompt-deferred"
    else:
        wifi_frontier = "none"

    return DriverTaskFrontiers(
        serial_driver_accepted=serial_driver_accepted,
        serial_fallback_active=serial_fallback_active,
        serial_runtime_frontier=serial_frontier,
        hdmi_descriptor_ready=hdmi_descriptor_ready,
        hdmi_engine_ready=hdmi_engine_ready,
        hdmi_owner_state_ready=hdmi_owner_state_ready,
        hdmi_runtime_frontier=hdmi_frontier,
        usb_driver_task_frontier=usb_frontier,
        wifi_replay_frontier=wifi_frontier,
    )


def summarize_usb_driver_task_stall(events: Iterable[TraceEvent]) -> str | None:
    """Return a USB-local-seat driver-task blocker that can hide serial RX."""

    outstanding: dict[int, str] = {}
    latest_usb_stage: str | None = None
    latest_usb_status: str | None = None
    latest_usb_engine_detail: int | None = None
    latest_usb_blocking_call = False
    latest_pcie_prereq_blocker: str | None = None
    for event in events:
        if event.domain != "driver":
            continue
        fields = event.fields
        contract = fields.get("contract", "").lower()
        if contract not in {"usb-local-seat", "pcie-root"}:
            continue
        raw = event.raw.lower()
        if "driver_task_resource_init" in raw:
            latest_usb_stage = fields.get("stage", "").lower()
            latest_usb_status = fields.get("status", "").lower()
            detail = parse_hex_int(fields.get("detail"))
            if contract == "usb-local-seat" and detail in {
                0x0201,
                0x0203,
                0x0204,
                0x0205,
                0x0206,
                0x0207,
                0x0208,
                0x0210,
                0x0211,
                0x0218,
                0x0219,
                0x021A,
                0x0202,
                0x0500,
                0x0501,
            }:
                latest_usb_engine_detail = detail
            if contract == "pcie-root" and latest_usb_stage in {
                "usb-prereq-pcie-replay",
                "usb-prereq-pcie-engine-init",
            } and latest_usb_status not in {"ready", "deferred", "begin", "progress"}:
                latest_pcie_prereq_blocker = f"{latest_usb_stage}-{latest_usb_status}"
            continue
        request = parse_hex_int(fields.get("request"))
        if request is None:
            continue
        if "driver_task_ring_call_begin" in raw:
            flags = parse_hex_int(fields.get("flags")) or 0
            outstanding[request] = "blocking" if flags == 0 else "bounded"
            latest_usb_blocking_call = flags == 0
        elif (
            "driver_task_ring_call_return" in raw
            or "driver_task_ring_call_timeout" in raw
            or "driver_task_ring_call_abort" in raw
        ):
            outstanding.pop(request, None)
            latest_usb_blocking_call = False
            if "driver_task_ring_call_timeout" in raw:
                if contract == "usb-local-seat" and latest_usb_stage in {
                    "usb-keyboard-enumeration",
                    "usb-keyboard-enumeration-retry",
                    "usb-keyboard-first-report",
                    "usb-owner-state",
                }:
                    return "usb-keyboard-enumeration-no-reply"
                if contract == "usb-local-seat" and latest_usb_engine_detail in {
                    0x0205,
                    0x0206,
                    0x0207,
                    0x0208,
                    0x0210,
                    0x0211,
                    0x0212,
                    0x0213,
                    0x0214,
                    0x0215,
                    0x0216,
                    0x0217,
                    0x0218,
                    0x0219,
                    0x021A,
                    0x0500,
                    0x0501,
                }:
                    return "usb-keyboard-enumeration-no-reply"
                return "usb-engine-init-no-reply"

    if latest_usb_stage in {
        "usb-keyboard-enumeration",
        "usb-keyboard-enumeration-retry",
        "usb-keyboard-first-report",
        "usb-owner-state",
    }:
        if (
            latest_usb_status == "blocked-keyboard-enumeration"
            and latest_usb_engine_detail == 0x0201
        ):
            return "command-event-ring-not-proven"
        if latest_usb_engine_detail == 0x0203:
            return "enable-slot-completion-pending"
        if latest_usb_status in {
            "command-ring-ready",
            "root-port-connected",
            "device-addressed",
            "device-descriptor",
            "config-descriptor",
            "enable-slot-failed",
            "address-device-failed",
            "device-descriptor-failed",
            "config-descriptor-failed",
            "hid-attach-failed",
            "hub-attach-failed",
            "hub-topology-no-keyboard",
            "hid-endpoint-not-ready",
            "hid-interface-not-found",
            "hid-interrupt-in-not-found",
            "hid-config-descriptor-malformed",
            "hub-child-scan-no-reply",
            "hub-set-configuration-failed",
            "hub-set-configuration-no-reply",
            "hub-set-configuration-status-no-reply",
            "hub-set-configuration-complete-no-reply",
            "hub-set-configuration-status-timeout",
            "hub-set-configuration-status-event-slot-empty",
            "hub-set-configuration-status-event-cycle-mismatch",
            "hub-set-configuration-status-event-ignored",
            "hub-set-configuration-settle-no-reply",
            "hub-descriptor-failed",
            "hub-context-failed",
            "hub-child-probe-no-reply",
            "blocked-keyboard-enumeration",
        }:
            if latest_usb_stage == "usb-keyboard-first-report":
                return f"{latest_usb_stage}-{latest_usb_status}"
            return latest_usb_status
    if latest_usb_stage in {"usb-engine-init", "usb-xhci-init"}:
        if latest_usb_status in {
            "no-reply",
            "blocked",
            "blocked-pcie-runtime",
            "blocked-pcie-hal-prep",
        }:
            if latest_usb_status.startswith("blocked-pcie") and latest_pcie_prereq_blocker:
                return latest_pcie_prereq_blocker
            return f"{latest_usb_stage}-{latest_usb_status}"
        if latest_usb_status == "begin" and latest_usb_blocking_call:
            return "usb-engine-init-blocking-call-stalled"
    if any(mode == "blocking" for mode in outstanding.values()):
        return "usb-driver-task-blocking-call-stalled"
    if latest_pcie_prereq_blocker is not None:
        return latest_pcie_prereq_blocker
    return None


def summarize_driver_task_replay_status(
    events: Iterable[TraceEvent], marker: str
) -> tuple[int, str]:
    """Return replay breadcrumb count and the first non-ready replay status."""

    replay_events = [
        event for event in events if marker in event.raw.lower()
    ]
    non_blocking_statuses = {
        "ready",
        "deferred",
        "begin",
        "progress",
        "preserved-ready",
        "cached-ready",
        "sdio-owner-replay",
        "resume-retained-stage",
        "stale-admission-retry",
    }
    for event in replay_events:
        status = event.fields.get("blocker", "unknown").lower()
        if status not in non_blocking_statuses:
            role = event.fields.get("role", "unknown")
            stage = event.fields.get("stage", "unknown")
            return len(replay_events), f"{role}:{stage}:{status}"
    return len(replay_events), "none"


def classify_sdio_replay_gate(replay_blocker: str) -> tuple[int, str, str, str]:
    """Map SDIO owner replay stages onto user-facing WiFi gates."""

    parts = replay_blocker.split(":", 2)
    if len(parts) != 3:
        return 1, "sdio-driver-task-replay", replay_blocker, "sdio-driver-task-replay"
    _role, stage, status = parts
    if (
        stage.startswith("sdio-cmd")
        or stage.startswith("sdio-card-init")
        or stage.startswith("sdio-first")
    ):
        return 2, "sdio-card-select", f"{stage}-{status}", stage
    if stage == "engine-init":
        if status == "wifi-pwrseq-failed" or (
            status.startswith("wifi-pwrseq-") and status.endswith("-failed")
        ):
            return 1, status, status, stage
        blocker = f"sdio-engine-init-{status}"
        return 2, blocker, blocker, stage
    if stage in {"hal-resource-prep", "descriptor-replay"}:
        return 1, "sdio-driver-task-replay", f"{stage}-{status}", stage
    return 1, "sdio-driver-task-replay", f"{stage}-{status}", stage


def classify_net_replay_gate(replay_blocker: str) -> tuple[int, str, str, str]:
    """Map CYW43 replay stages onto user-facing WiFi gates."""

    parts = replay_blocker.split(":", 2)
    if len(parts) != 3:
        return 1, "cyw43-driver-task-replay", replay_blocker, "cyw43-driver-task-replay"
    role, stage, status = parts
    if role == "cyw43-wifi" and stage in {"engine-init", "engine-init-replay"}:
        blocker = f"cyw43-engine-init-{status}"
        return 1, blocker, blocker, stage
    if role == "cyw43-wifi" and stage == "cyw43-sdio-prereq":
        return 1, "cyw43-sdio-prereq", f"{stage}-{status}", stage
    if role == "cyw43-wifi" and stage == "cyw43-firmware":
        return 5, "cyw43-firmware-runtime-replay", f"{stage}-{status}", stage
    if role == "cyw43-wifi" and stage == "cyw43-control-plane":
        return 7, "control-plane-reply-idle-loop", f"{stage}-{status}", stage
    return 1, "cyw43-driver-task-replay", f"{stage}-{status}", stage


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


def wifi_dpc_acceptance_required(
    exact: Mapping[str, str], minimum: Mapping[str, str]
) -> bool:
    """Return true when CLI assertions request current WiFi acceptance."""

    if exact.get("WIFI_DPC_PROOF") == "yes":
        return True
    if exact.get("WIFI_OLDGOOD_REPLAY") == "yes":
        return True
    exact_gate = parse_hex_int(exact.get("WIFI_GATE"))
    if exact_gate is not None and exact_gate >= 10:
        return True
    minimum_gate = parse_hex_int(minimum.get("WIFI_GATE"))
    return minimum_gate is not None and minimum_gate >= 10


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


def boot_slices(lines: list[str]) -> list[tuple[int, list[str]]]:
    """Return all detected boot slices with original zero-based line offsets."""

    starts: list[int] = []
    latest_start = None
    latest_start_is_chain = False
    for index, line in enumerate(lines):
        clean = ANSI_RE.sub("", line).lower()
        if any(clean.startswith(marker) for marker in BOOT_CHAIN_ROOT_MARKERS):
            latest_start = index
            latest_start_is_chain = True
        elif any(clean.startswith(marker) for marker in BOOT_CHAIN_CONTINUATION_MARKERS):
            if latest_start_is_chain and latest_start is not None:
                continue
            latest_start = index
            latest_start_is_chain = True
        elif (
            any(marker in clean for marker in BOOT_START_MARKERS)
            and not latest_start_is_chain
        ):
            latest_start = index
            latest_start_is_chain = False
        else:
            continue
        if not starts or starts[-1] != latest_start:
            starts.append(latest_start)
    if not starts:
        return [(0, lines)]
    slices: list[tuple[int, list[str]]] = []
    for position, start in enumerate(starts):
        end = starts[position + 1] if position + 1 < len(starts) else len(lines)
        slices.append((start, lines[start:end]))
    return slices


def uboot_menu_save_reset_slice(lines: list[str]) -> bool:
    """Return true for U-Boot menu settings saves that reset before Cohesix runs."""

    lowered = "\n".join(lines).lower()
    saved = any(
        marker in lowered
        for marker in (
            "[cohesix] saved settings to cohesix.env",
            "[cohesix] saved and verified settings in cohesix.env",
        )
    )
    restarting = any(
        marker in lowered
        for marker in (
            "[cohesix] saved settings verified; restarting",
            "resetting ...",
        )
    )
    return (
        saved
        and restarting
        and "[cohesix:root-task] cohesix boot: root-task online" not in lowered
        and not any(marker in lowered for marker in BOOT_START_MARKERS)
        and not any(marker in lowered for marker in BOOT_CHAIN_CONTINUATION_MARKERS)
    )


def proof_boot_slices(lines: list[str]) -> list[tuple[int, list[str]]]:
    """Return boot slices that can be scored as Cohesix root-task boot evidence."""

    slices = boot_slices(lines)
    proof_slices = [
        (line_offset, boot_lines)
        for line_offset, boot_lines in slices
        if not uboot_menu_save_reset_slice(boot_lines)
    ]
    return proof_slices or slices


def latest_boot_slice(lines: list[str]) -> tuple[int, list[str]]:
    """Return the latest boot slice plus its original zero-based line offset."""

    slices = proof_boot_slices(lines)
    return slices[-1]


def boot_evidence_blockers(record: Mapping[str, object]) -> list[str]:
    """Return stable blocker labels for a boot-slice evidence score."""

    blockers: list[str] = []
    if record.get("BOOT_HALTED") == "yes":
        blockers.append("boot-halted")
    if record.get("PANIC_SEEN") == "yes":
        blockers.append("panic")
    if record.get("SERIAL_CLEAN") != "yes":
        blockers.append("serial-unclean")
    if record.get("CYW43_BOOTSTRAP_SUPERVISOR_SEEN") == "yes":
        supervisor_blocker = str(
            record.get("CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER", "ready-missing")
        )
        if supervisor_blocker != "none":
            blockers.append(f"cyw43-bootstrap-supervisor-{supervisor_blocker}")
        elif record.get("CYW43_BOOTSTRAP_SUPERVISOR_READY") != "yes":
            blockers.append("cyw43-bootstrap-supervisor-ready-missing")
    if record.get("ROOT_CONSOLE_READY") != "yes":
        blockers.append("root-console-not-ready")
    if record.get("ROOT_PROMPT_SEEN") != "yes":
        blockers.append("root-prompt-missing")
    if record.get("TIMER_BACKEND") != "arch-counter":
        blockers.append("timer-backend-not-arch-counter")
    if record.get("TIMER_EL0_COUNTER") != "vct":
        blockers.append("timer-vct-proof-missing")
    if record.get("DUMMY_TIMER_SEEN") != "no":
        blockers.append("dummy-timer-seen")
    if record.get("DRIVER_TASK_DEFAULT_REQUESTED") != "yes":
        blockers.append("driver-task-default-not-requested")
    if record.get("DRIVER_TASK_LIVE_HOT_PATHS") != "yes":
        blockers.append("driver-task-live-hot-paths-missing")
    if (parse_hex_int(str(record.get("DRIVER_TASK_CONTRACTS", "0"))) or 0) < 5:
        blockers.append("driver-task-contracts-incomplete")
    if (parse_hex_int(str(record.get("DRIVER_TASK_DEDICATED", "0"))) or 0) < 5:
        blockers.append("driver-task-dedicated-incomplete")
    if record.get("DRIVER_TASK_DEDICATED_READY") != "yes":
        blockers.append("driver-task-dedicated-not-ready")
    for role, field in (
        ("serial", "DRIVER_TASK_SERIAL_DEDICATED"),
        ("usb", "DRIVER_TASK_USB_DEDICATED"),
        ("display", "DRIVER_TASK_DISPLAY_DEDICATED"),
        ("net", "DRIVER_TASK_NET_DEDICATED"),
        ("pcie", "DRIVER_TASK_PCIE_DEDICATED"),
    ):
        if record.get(field) != "yes":
            blockers.append(f"driver-task-{role}-not-dedicated")
    if record.get("DRIVER_TASK_SUBSTRATE_READY") != "yes":
        blockers.append("driver-task-substrate-not-ready")
    if record.get("DRIVER_TASK_COMPATIBILITY") not in {0, "0"}:
        blockers.append("driver-task-compatibility-present")
    if (parse_hex_int(str(record.get("DRIVER_TASK_FAILED_COUNT", "0"))) or 0) > 0:
        blockers.append("driver-task-failed")
    if record.get("DRIVER_TASK_CAPSET_PROOF") != "yes":
        blockers.append("driver-task-capset-proof-missing")
    if record.get("DRIVER_TASK_FAULT_PROOF") != "yes":
        blockers.append("driver-task-fault-proof-missing")
    if record.get("DRIVER_TASK_REVOKE_PROOF") != "yes":
        blockers.append("driver-task-revoke-proof-missing")
    if record.get("DRIVER_TASK_SCHED_PROOF") != "yes":
        blockers.append("driver-task-sched-proof-missing")
    if record.get("DRIVER_TASK_AFFINITY_PROOF") != "yes":
        blockers.append("driver-task-affinity-proof-missing")
    if (parse_hex_int(str(record.get("DRIVER_TASK_AFFINITY_CONFIGURED", "0"))) or 0) < 5:
        blockers.append("driver-task-affinity-config-incomplete")
    if (parse_hex_int(str(record.get("DRIVER_TASK_AFFINITY_APPLIED", "0"))) or 0) < 5:
        blockers.append("driver-task-affinity-applied-incomplete")
    if record.get("DRIVER_TASK_AFFINITY_MANIFEST_PROOF") != "yes":
        blockers.append("driver-task-affinity-manifest-proof-missing")
    if (parse_hex_int(str(record.get("DRIVER_TASK_AFFINITY_MANIFEST_MATCHES", "0"))) or 0) < 5:
        blockers.append("driver-task-affinity-manifest-matches-incomplete")
    if record.get("DRIVER_TASK_AFFINITY_MANIFEST_MISSING") not in {0, "0"}:
        blockers.append("driver-task-affinity-manifest-missing")
    if record.get("DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES") not in {0, "0"}:
        blockers.append("driver-task-affinity-manifest-mismatch")
    if record.get("DRIVER_TASK_VSPACE_PROOF") != "yes":
        blockers.append("driver-task-vspace-proof-missing")
    if record.get("DRIVER_TASK_POINTER_FREE_IPC_PROOF") != "yes":
        blockers.append("driver-task-pointer-free-ipc-missing")
    if record.get("DRIVER_TASK_OWNER_STATE_PROOF") != "yes":
        blockers.append("driver-task-owner-state-missing")
    if record.get("DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF") != "yes":
        blockers.append("driver-task-runtime-descriptor-seal-missing")
    if (parse_hex_int(str(record.get("DRIVER_TASK_LATENCY_PROOFS", "0"))) or 0) < 5:
        blockers.append("driver-task-latency-proof-incomplete")
    if (
        parse_hex_int(str(record.get("DRIVER_TASK_BOOTSTRAP_DEFERRED", "0")))
        or 0
    ) > 0:
        blockers.append("driver-task-bootstrap-deferred")
    min_dma_proofs = 6 if record.get("DRIVER_TASK_ACTIVE_NET") == "cyw43" else 5
    if (parse_hex_int(str(record.get("DRIVER_TASK_DMA_PROOFS", "0"))) or 0) < min_dma_proofs:
        blockers.append("driver-task-dma-proof-incomplete")
    if record.get("DRIVER_TASK_DMA_BLOCKER") != "none":
        blockers.append("driver-task-dma-blocked")
    if record.get("PI4_RUNTIME_DMA_COUNTER_PROOF") != "counter-qualified":
        blockers.append("driver-task-dma-counter-proof-missing")
    if (parse_hex_int(str(record.get("DRIVER_TASK_BUDGET_OVERRUNS", "0"))) or 0) > 0:
        blockers.append("driver-task-budget-overrun")
    if (parse_hex_int(str(record.get("DRIVER_TASK_RING_CALL_OUTSTANDING", "0"))) or 0) > 0:
        blockers.append("driver-task-ring-call-outstanding")
    if (
        parse_hex_int(str(record.get("DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT", "0")))
        or 0
    ) > 0:
        blockers.append("driver-task-ring-call-unresolved-timeout")
    if record.get("SERIAL_RESPONSIVE_PROOF") != "yes":
        blockers.append("serial-responsive-proof-missing")
    if record.get("HDMI_RESPONSIVE_PROOF") != "yes":
        blockers.append("hdmi-responsive-proof-missing")
    if record.get("USB_BOOTLOADER_HANDOFF_SEEN") == "yes":
        blockers.append("usb-bootloader-handoff")
    if record.get("USB_COLD_BOOT_SEEN") != "yes":
        blockers.append("usb-cold-boot-proof-missing")
    net_active = str(record.get("NET_ACTIVE", "unknown"))
    if net_active == "wifi":
        if record.get("DRIVER_TASK_SDIO_DEDICATED") != "yes":
            blockers.append("driver-task-sdio-not-dedicated")
        if (parse_hex_int(str(record.get("WIFI_GATE", "0"))) or 0) < 10:
            blockers.append("wifi-gate-incomplete")
        if record.get("WIFI_BLOCKER") != "none":
            blockers.append("wifi-blocked")
        if record.get("WIFI_GATE7_COMPLETE") != "yes":
            blockers.append("wifi-gate7-subgates-incomplete")
        wifi_gate7_missing = str(record.get("WIFI_GATE7_MISSING", "7a"))
        if wifi_gate7_missing != "none":
            blockers.append(f"wifi-gate7-{wifi_gate7_missing}-missing")
        if record.get("WIFI_OLDGOOD_REPLAY") != "yes":
            blockers.append("wifi-oldgood-replay-missing")
        if record.get("WIFI_OLDGOOD_MISSING") != "none":
            blockers.append("wifi-oldgood-incomplete")
        if record.get("DRIVER_TASK_ACTIVE_NET") != "cyw43":
            blockers.append("wifi-driver-task-not-active")
        if record.get("WIFI_FIRMWARE_IDENTITY_PROOF") != "yes":
            blockers.append("wifi-firmware-identity-missing")
        if record.get("WIFI_CLM_READY_PROOF") != "yes":
            blockers.append("wifi-clm-ready-missing")
        if record.get("WIFI_FIRMWARE_VERSION_PROOF") != "yes":
            blockers.append("wifi-firmware-version-missing")
        if record.get("WIFI_CLM_VERSION_PROOF") != "yes":
            blockers.append("wifi-clm-version-missing")
        if record.get("SDIO_IRQ158_INBAND_PROOF") != "yes":
            blockers.append("wifi-sdio-irq158-inband-proof-missing")
        if record.get("WIFI_DPC_PROOF") != "yes":
            if record.get("WIFI_DPC_REASON") == "missing":
                blockers.append("wifi-dpc-proof-missing")
            else:
                blockers.append("wifi-dpc-proof-invalid")
        if record.get("NET_TCP_READY") != "yes":
            blockers.append("wifi-tcp-ready-proof-missing")
        if record.get("COHSH_TCP_AUTH_PROOF") != "yes":
            blockers.append("wifi-tcp-auth-proof-missing")
        if record.get("NETTEST_PROOF") != "yes":
            blockers.append("wifi-nettest-proof-missing")
    elif net_active == "wired":
        if record.get("DRIVER_TASK_ACTIVE_NET") != "genet":
            blockers.append("wired-driver-task-not-active")
        if record.get("NET_ADDR_SRC") not in {"dhcp-lease", "static"}:
            blockers.append("wired-address-proof-missing")
        if record.get("NET_DHCP") not in {"bound", "off"}:
            blockers.append("wired-address-not-ready")
    else:
        blockers.append("network-active-missing")
    if net_active != "wifi" and record.get("NET_TCP_READY") != "yes" and record.get(
        "NETTEST_PROOF"
    ) != "yes":
        blockers.append("network-tcp-proof-missing")
    if (parse_hex_int(str(record.get("USB_GATE", "0"))) or 0) < 10:
        blockers.append("local-seat-usb-gate-incomplete")
    if record.get("USB_BLOCKER") != "none":
        blockers.append("local-seat-usb-blocked")
    if record.get("USB_COMMAND_READY") != "yes":
        blockers.append("local-seat-usb-command-ready-missing")
    if record.get("USB_FIRST_REPORT_READY") != "yes":
        blockers.append("local-seat-usb-first-report-missing")
    if record.get("USB_FIRST_BYTE_READY") != "yes":
        blockers.append("local-seat-usb-first-byte-missing")
    if record.get("USB_LOCAL_SEAT_STATE") != "ready":
        if record.get("USB_LOCAL_SEAT_STATE") not in {"recovered", "degraded"}:
            blockers.append("local-seat-usb-state-not-ready")
    if record.get("USB_LOCAL_SEAT_STATE") in {"recovered", "degraded"}:
        blockers.append(f"local-seat-usb-{record.get('USB_LOCAL_SEAT_STATE')}")
    if record.get("USB_BUSY_AFTER_READY") != "no":
        blockers.append(
            "local-seat-usb-busy-after-ready"
            if record.get("USB_BUSY_AFTER_READY") == "yes"
            else "local-seat-usb-busy-proof-missing"
        )
    if record.get("USB_OLDGOOD_REPLAY") != "yes":
        blockers.append("local-seat-usb-oldgood-replay-missing")
    if record.get("USB_OLDGOOD_MISSING") != "none":
        blockers.append("local-seat-usb-oldgood-incomplete")
    if record.get("USB_BURST_PROOF") != "yes":
        blockers.append("local-seat-usb-burst-proof-missing")
    if record.get("USB_BURST_DROPS") not in {0, "0"}:
        blockers.append("local-seat-usb-burst-drops")
    post_first_byte_blocker = str(
        record.get("USB_POST_FIRST_BYTE_BLOCKER", "missing")
    )
    if post_first_byte_blocker != "none":
        blockers.append(
            f"local-seat-usb-post-first-byte-{post_first_byte_blocker}"
        )
    if record.get("USB_STARTUP_BLOCKER_SEEN") == "yes":
        blockers.append("local-seat-usb-startup-blocker")
    if record.get("USB_ACTIVE_BLOCKER_SEEN") == "yes":
        blockers.append("local-seat-usb-active-blocker")
    return blockers


def summarize_boot_slices(lines: list[str]) -> list[dict[str, object]]:
    """Score each boot slice independently so later success cannot hide failures."""

    summaries: list[dict[str, object]] = []
    for index, (line_offset, boot_lines) in enumerate(boot_slices(lines), start=1):
        if uboot_menu_save_reset_slice(boot_lines):
            summaries.append(
                {
                    "boot": index,
                    "line_start": line_offset + 1,
                    "line_end": line_offset + len(boot_lines),
                    "score": "skip",
                    "kind": "uboot-menu-save-reset",
                    "blockers": [],
                    "gates": {},
                }
            )
            continue
        events = parse_events(boot_lines, line_base=line_offset)
        gates = summarize_gates(events).to_record()
        blockers = boot_evidence_blockers(gates)
        summaries.append(
            {
                "boot": index,
                "line_start": line_offset + 1,
                "line_end": line_offset + len(boot_lines),
                "score": "pass" if not blockers else "fail",
                "kind": "cohesix-boot",
                "blockers": blockers,
                "gates": gates,
            }
        )
    return summaries


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
        choices=("usb", "wifi", "driver"),
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
        "--boot-summary",
        action="store_true",
        help="emit JSON scoring for every detected boot slice in the trace",
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
    if args.boot_summary:
        print(
            json.dumps(
                {"boots": summarize_boot_slices(lines)},
                indent=2,
                sort_keys=True,
            )
        )
        return 0
    if args.gate_summary or args.summary:
        line_base, lines = latest_boot_slice(lines)
    events = filter_events(parse_events(lines, line_base=line_base), set(args.domain))
    if args.gate_summary:
        gate_summary = summarize_gates(events)
        print("\n".join(gate_summary.to_env_lines()))
        exact_expectations = parse_expectations(args.expect)
        minimum_expectations = parse_expectations(args.expect_min)
        exact_ok = check_gate_expectations(
            gate_summary, exact_expectations, sys.stderr
        )
        min_ok = check_gate_min_expectations(
            gate_summary, minimum_expectations, sys.stderr
        )
        not_ok = check_gate_not_expectations(
            gate_summary, parse_expectation_pairs(args.expect_not), sys.stderr
        )
        wifi_dpc_ok = True
        if (
            wifi_dpc_acceptance_required(exact_expectations, minimum_expectations)
            and not gate_summary.wifi_dpc_proof
        ):
            print(
                "gate assertion failed: WIFI_DPC_PROOF expected yes "
                f"got no reason={gate_summary.wifi_dpc_reason}",
                file=sys.stderr,
            )
            wifi_dpc_ok = False
        if not (exact_ok and min_ok and not_ok and wifi_dpc_ok):
            return 2
    elif args.summary:
        print(json.dumps(summarize_events(events), indent=2, sort_keys=True))
    else:
        if args.expect or args.expect_min or args.expect_not:
            raise SystemExit("--expect, --expect-min, and --expect-not require --gate-summary")
        write_jsonl(events, sys.stdout)
    return 0


def run_cli(argv: list[str] | None = None) -> int:
    """Run the CLI while treating closed output pipes as a normal consumer exit."""

    try:
        return main(argv)
    except BrokenPipeError:
        try:
            sys.stdout.close()
        except OSError:
            pass
        return 0


if __name__ == "__main__":
    raise SystemExit(run_cli())
