#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Validate and promote hash-bound Milestone 26e Worker target evidence.
# Copyright 2026 Lukas Bower

"""Strict validation and promotion for Cohesix Worker acceptance records."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

try:
    from scripts import driver_runtime_manifest as driver_runtimes
    from scripts import worker_image_manifest as worker_images
except ImportError:  # pragma: no cover - direct script execution uses this path
    import driver_runtime_manifest as driver_runtimes
    import worker_image_manifest as worker_images

MAX_RECORD_BYTES = 256 * 1024
MAX_ARTIFACT_BYTES = 64 * 1024 * 1024
MAX_IDENTIFIER_BYTES = 128
MAX_LABEL_BYTES = 256
MAX_LIST_ITEMS = 128
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
IDENTIFIER_RE = re.compile(r"^[A-Za-z0-9_.:/-]+$")

INTEGRATION_SCHEMA = "cohesix-worker-integration-evidence/v1"
COMPONENT_OBSERVATIONS_SCHEMA = "cohesix-worker-component-observations/v1"
COMPONENT_SCHEMA = "cohesix-worker-task-evidence/v1"
GENERATED_INVENTORY_SCHEMA = "cohesix-root-tcb-generated-inventory/v1"
ROOT_OBSERVATIONS_SCHEMA = "cohesix-root-tcb-observations/v1"
ROOT_SCHEMA = "cohesix-root-tcb-acceptance/v1"
SYSTEM_INPUT_SCHEMA = "cohesix-mcs-smp-run-input/v1"
SYSTEM_SCHEMA = "cohesix-mcs-smp-system-acceptance/v1"
RELEASE_SCHEMA = "cohesix-worker-release-acceptance/v1"
REQUIRED_INTEGRATIONS = (
    "gpu-receipt-path",
    "peft-receipt-path",
    "worker-control",
)
REQUIRED_ROLES = ("worker-heartbeat", "worker-gpu", "worker-lora")
TARGET_PROOF = {"qemu": "qemu", "pi4": "fresh-pi"}
TARGET_PROFILE = {"qemu": "virt-aarch64", "pi4": "pi4-uboot-aarch64"}
INVENTORY_KEYS = (
    "tcbs",
    "cnodes",
    "vspaces",
    "page_tables",
    "asids",
    "frames",
    "endpoints",
    "notifications",
    "fault_caps",
    "timeout_fault_caps",
    "reply_objects",
    "scheduling_contexts",
    "cspace_slots",
    "untyped_bytes",
)
SENSITIVE_MARKERS = (
    "auth_token",
    "authorization:",
    "bearer ",
    "private_key",
    "secret_key",
    "password",
    "cptr",
    "capability_value",
    "raw_badge",
)
TARGET_SESSION_KEYS = {
    "target",
    "source_sha256",
    "manifest_sha256",
    "kernel_sha256",
    "root_image_sha256",
    "driver_archive_sha256",
    "driver_manifest_sha256",
    "cyw43_coexistence_record_sha256",
    "worker_archive_sha256",
    "worker_image_manifest_sha256",
    "worker_abi_sha256",
}
QEMU_LAUNCH_SCHEMA = "cohesix-qemu-launch-artifacts/v1"
QEMU_AUTH_OBSERVATION_SCHEMA = "cohesix-target-observation/v1"
QEMU_AUTH_OBSERVATION_PROFILE = "qemu_smp_production / configs/root_task.toml"
QEMU_AUTH_UART_MARKER = "[cohsh-net][auth] auth OK, session established"
QEMU_LAUNCH_ARTIFACTS = (
    ("elfloader", Path("staging/elfloader")),
    ("kernel", Path("staging/kernel.elf")),
    ("rootserver", Path("staging/rootserver")),
    ("initrd", Path("cohesix-system.cpio")),
)
QEMU_SESSION_ARTIFACTS = {
    "driver_archive": Path("driver-runtimes/cohesix-driver-runtimes.cpio"),
    "driver_manifest": Path(
        "driver-runtimes/cohesix-driver-runtime-manifest.json"
    ),
    "worker_archive": Path("worker-images/cohesix-worker-images.cpio"),
    "worker_manifest": Path(
        "worker-images/cohesix-worker-image-manifest.json"
    ),
}
WORKER_ABI_FILES = (
    Path("crates/worker-task-abi/Cargo.toml"),
    Path("crates/worker-task-abi/src/lib.rs"),
)
SOURCE_INVENTORY_SCHEMA = "cohesix-source-inventory/v1"
WORKER_ABI_IDENTITY_SCHEMA = "cohesix-worker-abi-identity/v1"
CYW43_QEMU_BINDING_SCHEMA = "cohesix-cyw43-coexistence-binding/v1"
COMPONENT_REQUIRED_OUTCOMES = (
    "bounded-control-path",
    "bounded-receipt-path",
    "budget-exhaustion-attributed",
    "combined-notification",
    "driver-liveness",
    "durable-completion-order",
    "fault-before-ready",
    "fault-during-ipc",
    "forbidden-blocking-send-refused",
    "fresh-supervisor-generation",
    "gpu-grant-confirmed-rejected-stale",
    "gpu-release-confirmed-rejected-stale",
    "gpu-renew-confirmed-rejected-stale",
    "heartbeat-progress",
    "lora-activate-confirmed-rejected-stale",
    "lora-export-confirmed-rejected-stale",
    "lora-import-confirmed-rejected-stale",
    "lora-rollback-confirmed-rejected-stale",
    "maximum-slot-refused",
    "no-post-revoke-activity",
    "operator-liveness",
    "same-role-sequential-instances",
    "stale-record-revoked",
    "teardown-zero-leak",
    "timeout-attributed",
)
ROOT_REQUIRED_OUTCOMES = (
    "console-network-fault-contained",
    "donated-time-returned-or-revoked",
    "driver-supervisor-progress",
    "emergency-progress",
    "fault-supervisor-progress",
    "ninedoor-fault-contained",
    "operator-liveness",
    "pressure-bounded",
    "root-control-progress",
    "shutdown-contained",
    "stale-authority-revoked",
    "worker-gpu-fault-contained",
    "worker-heartbeat-fault-contained",
    "worker-lora-fault-contained",
    "worker-supervisor-progress",
)
SYSTEM_REQUIRED_OUTCOMES = (
    "artifact-freeze",
    "budget-exhaustion-attributed",
    "cold-warm-boot",
    "cyw43-coexistence-record-bound",
    "driver-call-recovered",
    "fault-contained",
    "four-core-mcs-topology",
    "fresh-supervisor-generation",
    "gpu-receipt-path",
    "no-classic-scheduler",
    "normal-load-liveness",
    "operator-liveness",
    "overload-bounded",
    "peft-receipt-path",
    "protocol-regression",
    "same-harness-performance",
    "timeout-contained",
    "worker-teardown-zero-leak",
)
QEMU_PROC_KEYS = (
    "schedule_summary",
    "schedule_queue",
    "lease_summary",
    "lease_active",
    "lease_preemptions",
)
QEMU_RECEIPT_ACTIONS = {
    0x0201: ("gpu.lease.grant", "worker-gpu"),
    0x0202: ("gpu.lease.renew", "worker-gpu"),
    0x0203: ("gpu.lease.release", "worker-gpu"),
    0x0301: ("peft.export", "worker-lora"),
    0x0302: ("peft.import", "worker-lora"),
    0x0303: ("peft.activate", "worker-lora"),
    0x0304: ("peft.rollback", "worker-lora"),
}
QEMU_TERMINAL_OUTCOMES = {1: "confirmed", 2: "rejected", 8: "stale"}
QEMU_WORKER_SYMBOLS = (
    "_start",
    "cohesix_worker_qemu_evidence_control_handler",
    "cohesix_worker_qemu_evidence_standard_fault",
    "cohesix_worker_qemu_evidence_timeout_spin",
)
QEMU_SERVICE_SYMBOLS = {
    "ninedoor-service": (
        "cohesix_ninedoor_qemu_evidence_request_handler",
        "cohesix_ninedoor_qemu_evidence_standard_fault",
    ),
    "console-network": (
        "cohesix_console_network_qemu_evidence_control_handler",
        "cohesix_console_network_qemu_evidence_standard_fault",
        "cohesix_console_network_qemu_evidence_timeout_spin",
    ),
}
QEMU_NINEDOOR_ROOT_SYMBOLS = (
    "cohesix_ninedoor_qemu_evidence_post_prepare",
    "cohesix_ninedoor_qemu_evidence_request_local_revoke",
)
QEMU_NINEDOOR_ROOT_MODULE = "root_task::ninedoor"
QEMU_SERVICE_MODES = {
    "ninedoor-service": (
        "during-call-standard",
        "between-calls-revoke",
    ),
    "console-network": (
        "during-call-standard",
        "budget-exhaustion-timeout",
    ),
}
QEMU_SERVICE_EVIDENCE_PLAN = (
    ("ninedoor-service", "during-call-standard"),
    ("ninedoor-service", "between-calls-revoke"),
    ("console-network", "during-call-standard"),
    ("console-network", "budget-exhaustion-timeout"),
)
QEMU_CRITICAL_SYMBOLS = (
    "cohesix_root_fault_qemu_evidence_turn",
    "cohesix_root_emergency_qemu_evidence_wait",
    "cohesix_worker_supervisor_qemu_evidence_wait",
    "cohesix_driver_supervisor_qemu_evidence_wait",
)
QEMU_CRITICAL_DUTIES = {
    "cohesix_root_fault_qemu_evidence_turn": "root-fault",
    "cohesix_root_emergency_qemu_evidence_wait": "root-emergency",
    "cohesix_worker_supervisor_qemu_evidence_wait": "worker-supervisor",
    "cohesix_driver_supervisor_qemu_evidence_wait": "driver-supervisor",
}


class EvidenceError(ValueError):
    """A bounded evidence record violated its exact schema or hash graph."""


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate and promote Cohesix Milestone 26e Worker evidence",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    validate = commands.add_parser("validate", help="validate target Worker evidence")
    validate.add_argument("--target", choices=("qemu", "pi4"), required=True)
    validate.add_argument("--evidence", type=Path, required=True)

    emit_session = commands.add_parser(
        "emit-qemu-target-session",
        help="emit one exact QEMU target session from verified build artifacts",
    )
    emit_session.add_argument("--repo-root", type=Path, required=True)
    emit_session.add_argument("--qemu-out", type=Path, required=True)
    emit_session.add_argument("--resolved-manifest", type=Path, required=True)
    emit_session.add_argument("--topology", type=Path, required=True)
    emit_session.add_argument("--out-dir", type=Path, required=True)

    validate_root = commands.add_parser(
        "validate-root", help="validate root-TCB containment evidence"
    )
    validate_root.add_argument("--target", choices=("qemu", "pi4"), required=True)
    validate_root.add_argument("--evidence", type=Path, required=True)
    validate_root.add_argument("--worker", type=Path, required=True)

    emit_component = commands.add_parser(
        "emit-component",
        help="emit one target component from explicit direct observations",
    )
    emit_component.add_argument("--target", choices=("qemu", "pi4"), required=True)
    emit_component.add_argument("--target-session", type=Path, required=True)
    emit_component.add_argument("--generated-inventory", type=Path, required=True)
    emit_component.add_argument("--observations", type=Path, required=True)
    emit_component.add_argument("--integration-dir", type=Path, required=True)
    emit_component.add_argument("--out", type=Path, required=True)

    emit_root = commands.add_parser(
        "emit-root",
        help="emit root-TCB evidence from explicit generated and observed inputs",
    )
    emit_root.add_argument("--target", choices=("qemu", "pi4"), required=True)
    emit_root.add_argument("--target-session", type=Path, required=True)
    emit_root.add_argument("--worker", type=Path, required=True)
    emit_root.add_argument("--generated-inventory", type=Path, required=True)
    emit_root.add_argument("--observations", type=Path, required=True)
    emit_root.add_argument("--out", type=Path, required=True)

    collect_preflight = commands.add_parser(
        "collect-qemu-preflight",
        help="derive the live QEMU component needed before executable pressure",
    )
    collect_preflight.add_argument("--target-session", type=Path, required=True)
    collect_preflight.add_argument("--generated-inventory", type=Path, required=True)
    collect_preflight.add_argument("--qemu-out", type=Path, required=True)
    collect_preflight.add_argument("--auth-observation", type=Path, required=True)
    collect_preflight.add_argument("--uart", type=Path, required=True)
    collect_preflight.add_argument("--cohsh", type=Path, required=True)
    collect_preflight.add_argument(
        "--gdb-log", type=Path, action="append", required=True
    )
    collect_preflight.add_argument("--worker-archive", type=Path, required=True)
    collect_preflight.add_argument("--driver-archive", type=Path, required=True)
    collect_preflight.add_argument(
        "--worker-image-manifest", type=Path, required=True
    )
    collect_preflight.add_argument(
        "--worker-elf", action="append", required=True, metavar="ROLE=PATH"
    )
    collect_preflight.add_argument(
        "--service-elf", action="append", required=True, metavar="SERVICE=PATH"
    )
    collect_preflight.add_argument(
        "--service-gdb-log", type=Path, action="append", required=True
    )
    collect_preflight.add_argument(
        "--service-uart", type=Path, action="append", required=True
    )
    collect_preflight.add_argument("--root-elf", type=Path, required=True)
    collect_preflight.add_argument("--critical-gdb-log", type=Path, required=True)
    collect_preflight.add_argument("--integration-dir", type=Path, required=True)
    collect_preflight.add_argument("--out-dir", type=Path, required=True)

    collect_qemu = commands.add_parser(
        "collect-qemu",
        help="derive QEMU Worker/root/system evidence from immutable live artifacts",
    )
    collect_qemu.add_argument("--target-session", type=Path, required=True)
    collect_qemu.add_argument("--generated-inventory", type=Path, required=True)
    collect_qemu.add_argument("--qemu-out", type=Path, required=True)
    collect_qemu.add_argument("--auth-observation", type=Path, required=True)
    collect_qemu.add_argument("--uart", type=Path, action="append", required=True)
    collect_qemu.add_argument("--cohsh", type=Path, required=True)
    collect_qemu.add_argument("--preflight-uart", type=Path, required=True)
    collect_qemu.add_argument(
        "--preflight-gdb-log", type=Path, action="append", required=True
    )
    collect_qemu.add_argument(
        "--preflight-service-gdb-log", type=Path, action="append", required=True
    )
    collect_qemu.add_argument(
        "--preflight-service-uart", type=Path, action="append", required=True
    )
    collect_qemu.add_argument("--preflight-critical-gdb-log", type=Path, required=True)
    collect_qemu.add_argument("--gdb-log", type=Path, action="append", required=True)
    collect_qemu.add_argument(
        "--pressure",
        type=Path,
        action="append",
        required=True,
        help="immutable medium/high pressure summary; specify at least twice",
    )
    collect_qemu.add_argument(
        "--worker-elf",
        action="append",
        required=True,
        metavar="ROLE=PATH",
    )
    collect_qemu.add_argument(
        "--service-elf", action="append", required=True, metavar="SERVICE=PATH"
    )
    collect_qemu.add_argument("--root-elf", type=Path, required=True)
    collect_qemu.add_argument("--worker-archive", type=Path, required=True)
    collect_qemu.add_argument("--driver-archive", type=Path, required=True)
    collect_qemu.add_argument("--worker-image-manifest", type=Path, required=True)
    collect_qemu.add_argument("--integration-dir", type=Path, required=True)
    collect_qemu.add_argument("--run-dir", type=Path, required=True)
    collect_qemu.add_argument("--out-dir", type=Path, required=True)

    qemu_gdb = commands.add_parser(
        "qemu-gdb",
        help="run the bounded external-QEMU Worker fault injection plan",
    )
    qemu_gdb.add_argument(
        "--gdb",
        type=Path,
        default=Path(
            "out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-"
            "aarch64-none-elf/bin/aarch64-none-elf-gdb"
        ),
    )
    qemu_gdb.add_argument("--nm", type=Path)
    qemu_gdb.add_argument("--remote", required=True)
    qemu_gdb.add_argument("--target-session", type=Path, required=True)
    qemu_gdb.add_argument("--generated-inventory", type=Path, required=True)
    qemu_gdb.add_argument("--worker-image-manifest", type=Path, required=True)
    qemu_gdb.add_argument(
        "--worker-elf",
        action="append",
        required=True,
        metavar="ROLE=PATH",
    )
    qemu_gdb.add_argument(
        "--inject-role", choices=REQUIRED_ROLES, default="worker-heartbeat"
    )
    qemu_gdb.add_argument("--timeout-secs", type=int, default=300)
    qemu_gdb.add_argument("--out", type=Path, required=True)

    service_gdb = commands.add_parser(
        "qemu-service-gdb",
        help="run bounded external-QEMU service fault injection",
    )
    service_gdb.add_argument(
        "--gdb",
        type=Path,
        default=Path(
            "out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-"
            "aarch64-none-elf/bin/aarch64-none-elf-gdb"
        ),
    )
    service_gdb.add_argument("--nm", type=Path)
    service_gdb.add_argument("--remote", required=True)
    service_gdb.add_argument("--target-session", type=Path, required=True)
    service_gdb.add_argument("--generated-inventory", type=Path, required=True)
    service_gdb.add_argument("--qemu-out", type=Path, required=True)
    service_gdb.add_argument("--auth-observation", type=Path, required=True)
    service_gdb.add_argument(
        "--service", choices=tuple(QEMU_SERVICE_SYMBOLS), required=True
    )
    service_gdb.add_argument(
        "--mode",
        choices=tuple(
            mode
            for modes in QEMU_SERVICE_MODES.values()
            for mode in modes
        ),
        required=True,
    )
    service_gdb.add_argument("--service-elf", type=Path, required=True)
    service_gdb.add_argument("--root-elf", type=Path)
    service_gdb.add_argument("--timeout-secs", type=int, default=300)
    service_gdb.add_argument("--out", type=Path, required=True)

    critical_gdb = commands.add_parser(
        "qemu-critical-gdb",
        help="observe all four live QEMU critical root duty lanes",
    )
    critical_gdb.add_argument(
        "--gdb",
        type=Path,
        default=Path(
            "out/toolchain/arm-gnu-toolchain-15.2.rel1-darwin-arm64-"
            "aarch64-none-elf/bin/aarch64-none-elf-gdb"
        ),
    )
    critical_gdb.add_argument("--nm", type=Path)
    critical_gdb.add_argument("--remote", required=True)
    critical_gdb.add_argument("--target-session", type=Path, required=True)
    critical_gdb.add_argument("--generated-inventory", type=Path, required=True)
    critical_gdb.add_argument("--root-elf", type=Path, required=True)
    critical_gdb.add_argument("--timeout-secs", type=int, default=300)
    critical_gdb.add_argument("--out", type=Path, required=True)

    validate_system = commands.add_parser(
        "validate-system", help="emit and validate one target full-system record"
    )
    validate_system.add_argument("--target", choices=("qemu", "pi4"), required=True)
    validate_system.add_argument("--worker", type=Path, required=True)
    validate_system.add_argument("--root", type=Path, required=True)
    validate_system.add_argument("--run", type=Path, required=True)
    validate_system.add_argument(
        "--observations",
        type=Path,
        help="explicit full-system run input (defaults to RUN/m26e-system-input.json)",
    )
    validate_system.add_argument("--out", type=Path, required=True)

    promote = commands.add_parser(
        "promote-release", help="promote exactly two complete target graphs"
    )
    for option in (
        "worker-qemu",
        "worker-pi4",
        "root-qemu",
        "root-pi4",
        "system-qemu",
        "system-pi4",
    ):
        promote.add_argument(f"--{option}", type=Path, required=True)
    promote.add_argument("--out", type=Path, required=True)
    return parser


def _load(path: Path) -> tuple[dict[str, Any], bytes]:
    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise EvidenceError(f"cannot read evidence {path}: {exc}") from exc
    if not raw or len(raw) > MAX_RECORD_BYTES:
        raise EvidenceError(f"evidence size is outside 1..{MAX_RECORD_BYTES}: {path}")
    try:
        value = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"invalid JSON in {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise EvidenceError(f"evidence root must be an object: {path}")
    _scan_sensitive(value)
    return value, raw


def _write(path: Path, record: Mapping[str, Any]) -> bytes:
    raw = (json.dumps(record, indent=2, sort_keys=True) + "\n").encode("utf-8")
    _write_raw(path, raw)
    return raw


def _write_raw(path: Path, raw: bytes) -> None:
    if not raw or len(raw) > MAX_RECORD_BYTES:
        raise EvidenceError("generated evidence exceeds the bounded record size")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            dir=path.parent,
            prefix=f".{path.name}.",
            delete=False,
        ) as handle:
            handle.write(raw)
            handle.flush()
            temporary = Path(handle.name)
        temporary.replace(path)
    except OSError as exc:
        raise EvidenceError(f"cannot publish evidence {path}: {exc}") from exc
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _exact_keys(
    value: Mapping[str, Any],
    required: Iterable[str],
    optional: Iterable[str] = (),
    *,
    context: str,
) -> None:
    required_set = set(required)
    permitted = required_set | set(optional)
    present = set(value)
    missing = sorted(required_set - present)
    unexpected = sorted(present - permitted)
    if missing or unexpected:
        raise EvidenceError(
            f"{context} field mismatch: missing={missing} unexpected={unexpected}"
        )


def _identifier(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or not value
        or len(value.encode("utf-8")) > MAX_IDENTIFIER_BYTES
        or IDENTIFIER_RE.fullmatch(value) is None
    ):
        raise EvidenceError(f"invalid identifier for {context}")
    return value


def _label(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or not value
        or len(value.encode("utf-8")) > MAX_LABEL_BYTES
        or any(ord(character) < 32 or ord(character) == 127 for character in value)
    ):
        raise EvidenceError(f"invalid label for {context}")
    return value


def _hash(value: Any, context: str) -> str:
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise EvidenceError(f"invalid lowercase SHA-256 for {context}")
    return value


def _bounded_list(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list) or len(value) > MAX_LIST_ITEMS:
        raise EvidenceError(f"invalid bounded list for {context}")
    return value


def _sorted_unique(values: Sequence[Any], key: Any, context: str) -> None:
    projected = [key(value) for value in values]
    if projected != sorted(projected) or len(projected) != len(set(projected)):
        raise EvidenceError(f"{context} must be sorted and unique")


def _scan_sensitive(value: Any) -> None:
    rendered = json.dumps(value, sort_keys=True).lower()
    if any(marker in rendered for marker in SENSITIVE_MARKERS):
        raise EvidenceError("evidence contains prohibited secret or capability material")


def _verdict(record: Mapping[str, Any]) -> None:
    verdict = record.get("verdict")
    blockers = _bounded_list(record.get("blockers"), "blockers")
    _sorted_unique(blockers, lambda item: _label(item, "blocker"), "blockers")
    if verdict not in ("PASS", "FAIL") or (verdict == "PASS") != (not blockers):
        raise EvidenceError("verdict and blockers disagree")


def _target_session(value: Any, target: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError("target_session must be an object")
    _exact_keys(value, TARGET_SESSION_KEYS, context="target_session")
    if value["target"] != target:
        raise EvidenceError("target_session names the wrong target")
    for field in TARGET_SESSION_KEYS - {"target"}:
        _hash(value[field], f"target_session.{field}")
    return value


def _reference(value: Any, expected_kind: str | None = None) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError("evidence reference must be an object")
    _exact_keys(value, {"id", "record_kind", "sha256"}, context="evidence reference")
    _identifier(value["id"], "reference id")
    if value["record_kind"] not in (
        "worker-integration",
        "target-component",
        "root-tcb",
        "full-system",
    ):
        raise EvidenceError("unsupported referenced record kind")
    if expected_kind is not None and value["record_kind"] != expected_kind:
        raise EvidenceError(f"reference must name {expected_kind}")
    _hash(value["sha256"], "evidence reference")
    return value


def _artifacts(value: Any, context: str, *, required: bool) -> list[dict[str, Any]]:
    items = _bounded_list(value, context)
    parsed: list[dict[str, Any]] = []
    for item in items:
        if not isinstance(item, dict):
            raise EvidenceError(f"{context} entry must be an object")
        _exact_keys(item, {"id", "sha256", "bytes"}, context=f"{context} entry")
        _identifier(item["id"], f"{context} id")
        _hash(item["sha256"], f"{context} digest")
        if (
            not isinstance(item["bytes"], int)
            or isinstance(item["bytes"], bool)
            or item["bytes"] <= 0
            or item["bytes"] > 0xFFFF_FFFF_FFFF_FFFF
        ):
            raise EvidenceError(f"{context} byte count must be a positive u64")
        parsed.append(item)
    _sorted_unique(
        parsed,
        lambda item: (item["id"], item["sha256"], item["bytes"]),
        context,
    )
    if required and not parsed:
        raise EvidenceError(f"accepted {context} cannot be empty")
    return parsed


def _outcomes(
    value: Any,
    context: str,
    *,
    required: bool,
    allowed_classes: Sequence[str] = ("action", "observation", "receipt"),
) -> list[dict[str, Any]]:
    items = _bounded_list(value, context)
    parsed: list[dict[str, Any]] = []
    for item in items:
        if not isinstance(item, dict):
            raise EvidenceError(f"{context} entry must be an object")
        _exact_keys(item, {"id", "class", "result"}, context=f"{context} entry")
        _identifier(item["id"], f"{context} id")
        if item["class"] not in allowed_classes:
            raise EvidenceError(f"invalid {context} class")
        _label(item["result"], f"{context} result")
        parsed.append(item)
    _sorted_unique(
        parsed,
        lambda item: (item["id"], item["class"], item["result"]),
        context,
    )
    if required and not parsed:
        raise EvidenceError(f"accepted {context} cannot be empty")
    return parsed


def _required_pass_outcomes(
    outcomes: Sequence[Mapping[str, Any]],
    required_ids: Sequence[str],
    context: str,
) -> None:
    observed_ids = tuple(item["id"] for item in outcomes)
    if observed_ids != tuple(required_ids) or any(
        item["result"] != "pass" for item in outcomes
    ):
        raise EvidenceError(
            f"{context} must contain the exact required PASS outcome matrix"
        )


def _positive_u64(value: Any, context: str) -> int:
    if (
        not isinstance(value, int)
        or isinstance(value, bool)
        or value <= 0
        or value > 0xFFFF_FFFF_FFFF_FFFF
    ):
        raise EvidenceError(f"{context} must be a positive u64")
    return value


def _worker_scheduling_context(value: Any) -> dict[str, int]:
    if not isinstance(value, dict):
        raise EvidenceError("Worker scheduling_context must be an object")
    _exact_keys(
        value,
        {"budget_us", "period_us"},
        context="Worker scheduling_context",
    )
    budget = value["budget_us"]
    period = value["period_us"]
    if (
        not isinstance(budget, int)
        or isinstance(budget, bool)
        or not isinstance(period, int)
        or isinstance(period, bool)
        or budget <= 0
        or period <= 0
        or budget > period
        or period > 0xFFFF_FFFF
    ):
        raise EvidenceError("invalid Worker scheduling_context budget/period")
    return value


def validate_integration(record: Mapping[str, Any], target: str) -> None:
    _scan_sensitive(record)
    _exact_keys(
        record,
        {
            "schema",
            "record_kind",
            "dependency_id",
            "owner_milestone",
            "obligation",
            "observed_mode",
            "dependency_graph_sha256",
            "manifest_sha256",
            "component_sha256",
            "config_sha256",
            "host",
            "target_session",
            "execution_proof",
            "outcomes",
            "raw_evidence",
            "verdict",
            "blockers",
        },
        {"artifact_sha256"},
        context="Worker integration evidence",
    )
    if record["schema"] != INTEGRATION_SCHEMA or record["record_kind"] != "worker-integration":
        raise EvidenceError("wrong Worker integration schema/kind")
    dependency_id = _identifier(record["dependency_id"], "dependency_id")
    _identifier(record["owner_milestone"], "owner_milestone")
    if record["obligation"] not in (
        "role_required",
        "release_required",
        "use_case_required",
        "optional",
        "future",
    ):
        raise EvidenceError("invalid integration obligation")
    if record["observed_mode"] not in (
        "unknown",
        "missing",
        "disabled",
        "fixture",
        "mock",
        "dry-run",
        "live",
    ):
        raise EvidenceError("invalid observed integration mode")
    if record["obligation"] == "future" and record["observed_mode"] == "live":
        raise EvidenceError("future integration cannot be live")
    for field in (
        "dependency_graph_sha256",
        "manifest_sha256",
        "component_sha256",
        "config_sha256",
    ):
        _hash(record[field], field)
    if "artifact_sha256" in record:
        _hash(record["artifact_sha256"], "artifact_sha256")
    host = record["host"]
    if not isinstance(host, dict):
        raise EvidenceError("host must be an object")
    _exact_keys(
        host,
        {"profile", "os", "architecture"},
        {"provider_version"},
        context="host",
    )
    _identifier(host["profile"], "host profile")
    _label(host["os"], "host os")
    _label(host["architecture"], "host architecture")
    if "provider_version" in host:
        _label(host["provider_version"], "provider version")
    _target_session(record["target_session"], target)
    if record["execution_proof"] != TARGET_PROOF[target] or record["observed_mode"] != "live":
        raise EvidenceError("target-session integration must be live with exact target proof")
    integration_classes = (
        ("projection-compatibility",)
        if dependency_id == "python-sdk-projection"
        else ("action", "observation", "receipt")
    )
    _outcomes(
        record["outcomes"],
        "integration outcomes",
        required=record["verdict"] == "PASS",
        allowed_classes=integration_classes,
    )
    _artifacts(
        record["raw_evidence"],
        "integration raw evidence",
        required=record["verdict"] == "PASS",
    )
    _verdict(record)
    if dependency_id in REQUIRED_INTEGRATIONS and record["obligation"] != "role_required":
        raise EvidenceError("mandatory Worker integration must be role_required")


def validate_component(record: Mapping[str, Any], target: str) -> None:
    _scan_sensitive(record)
    _exact_keys(
        record,
        {
            "schema",
            "record_kind",
            "target",
            "target_session",
            "topology_sha256",
            "workers",
            "integration_evidence",
            "outcomes",
            "raw_evidence",
            "verdict",
            "blockers",
        },
        context="Worker component evidence",
    )
    if (
        record["schema"] != COMPONENT_SCHEMA
        or record["record_kind"] != "target-component"
        or record["target"] != target
    ):
        raise EvidenceError("wrong Worker component schema/kind/target")
    _target_session(record["target_session"], target)
    _hash(record["topology_sha256"], "topology")
    workers = _bounded_list(record["workers"], "workers")
    if len(workers) != 3:
        raise EvidenceError("target component requires exactly three Workers")
    roles: list[str] = []
    for worker in workers:
        if not isinstance(worker, dict):
            raise EvidenceError("Worker observation must be an object")
        _exact_keys(
            worker,
            {
                "identity",
                "state",
                "image_sha256",
                "ready_sequence",
                "completion_sequence",
                "endpoint_badge",
                "fault_badge",
                "core",
                "scheduling_context",
                "object_inventory",
            },
            context="Worker observation",
        )
        identity = worker["identity"]
        state = worker["state"]
        if not isinstance(identity, dict) or not isinstance(state, dict):
            raise EvidenceError("Worker identity/state must be objects")
        _exact_keys(
            identity,
            {"role", "slot", "lease_epoch", "supervisor_generation", "cap_generation"},
            context="Worker identity",
        )
        _exact_keys(
            state,
            {"declaration", "lifecycle", "artifact", "receipt", "execution_proof"},
            context="Worker state",
        )
        role = identity["role"]
        if role not in REQUIRED_ROLES:
            raise EvidenceError("component contains an unsupported Worker role")
        roles.append(role)
        for field in ("slot", "lease_epoch", "supervisor_generation", "cap_generation"):
            value = identity[field]
            maximum = 0xFFFF if field == "slot" else 0xFFFF_FFFF_FFFF_FFFF
            if (
                not isinstance(value, int)
                or isinstance(value, bool)
                or value < 0
                or value > maximum
            ):
                raise EvidenceError(f"invalid Worker identity {field}")
        if any(
            identity[field] == 0
            for field in ("lease_epoch", "supervisor_generation", "cap_generation")
        ):
            raise EvidenceError("Worker generation identity cannot be zero")
        endpoint_badge = _positive_u64(worker["endpoint_badge"], "endpoint_badge")
        fault_badge = _positive_u64(worker["fault_badge"], "fault_badge")
        if endpoint_badge == fault_badge:
            raise EvidenceError("Worker endpoint/fault badges must be disjoint")
        core = worker["core"]
        if not isinstance(core, int) or isinstance(core, bool) or not 0 <= core < 4:
            raise EvidenceError("Worker core must be in the four-core range")
        _worker_scheduling_context(worker["scheduling_context"])
        _inventory(worker["object_inventory"], "Worker object inventory")
        for sequence_field in ("ready_sequence", "completion_sequence"):
            sequence = worker[sequence_field]
            if (
                not isinstance(sequence, int)
                or isinstance(sequence, bool)
                or sequence < 0
                or sequence > 0xFFFF_FFFF_FFFF_FFFF
            ):
                raise EvidenceError(f"invalid Worker {sequence_field}")
        expected_receipt = "none" if role == "worker-heartbeat" else "confirmed"
        if record["verdict"] == "PASS" and (
            state != {
                "declaration": "executable",
                "lifecycle": "ready",
                "artifact": "verified",
                "receipt": expected_receipt,
                "execution_proof": TARGET_PROOF[target],
            }
            or worker["ready_sequence"] <= 0
            or worker["completion_sequence"] <= 0
        ):
            raise EvidenceError("accepted Worker role state is incomplete")
        _hash(worker["image_sha256"], "Worker image")
    if tuple(roles) != REQUIRED_ROLES:
        raise EvidenceError("Worker observations must use exact generated role order")
    endpoint_badges = [worker["endpoint_badge"] for worker in workers]
    fault_badges = [worker["fault_badge"] for worker in workers]
    if (
        len(endpoint_badges) != len(set(endpoint_badges))
        or len(fault_badges) != len(set(fault_badges))
        or set(endpoint_badges) & set(fault_badges)
    ):
        raise EvidenceError("Worker badge inventories overlap")
    references = _bounded_list(record["integration_evidence"], "integration evidence")
    for reference in references:
        _reference(reference, "worker-integration")
    _sorted_unique(
        references,
        lambda item: (item["id"], item["record_kind"], item["sha256"]),
        "integration evidence",
    )
    if tuple(reference["id"] for reference in references) != REQUIRED_INTEGRATIONS:
        raise EvidenceError("component lacks the exact mandatory integration graph")
    outcomes = _outcomes(
        record["outcomes"],
        "component outcomes",
        required=record["verdict"] == "PASS",
    )
    if record["verdict"] == "PASS":
        _required_pass_outcomes(
            outcomes,
            COMPONENT_REQUIRED_OUTCOMES,
            "component outcomes",
        )
    _artifacts(
        record["raw_evidence"],
        "component raw evidence",
        required=record["verdict"] == "PASS",
    )
    _verdict(record)


def _object_budget(value: Any, context: str) -> dict[str, int]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{context} must be an object")
    _exact_keys(value, set(INVENTORY_KEYS), context=context)
    for key in INVENTORY_KEYS:
        count = value[key]
        maximum = 0xFFFF_FFFF_FFFF_FFFF if key == "untyped_bytes" else 0xFFFF_FFFF
        if (
            not isinstance(count, int)
            or isinstance(count, bool)
            or count < 0
            or count > maximum
        ):
            raise EvidenceError(f"invalid {context} count: {key}")
    return value


def _inventory(value: Any, context: str) -> dict[str, int]:
    budget = _object_budget(value, context)
    for key in (
        "tcbs",
        "cnodes",
        "vspaces",
        "page_tables",
        "asids",
        "frames",
        "fault_caps",
        "timeout_fault_caps",
        "scheduling_contexts",
        "cspace_slots",
        "untyped_bytes",
    ):
        if value[key] == 0:
            raise EvidenceError(f"{context} requires nonzero {key}")
    return budget


def _canonical_json_sha256(value: Any) -> str:
    encoded = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return _sha256(encoded)


def _maximum_inventory(topology: Mapping[str, Any]) -> dict[str, int]:
    admission = topology.get("worker_resource_admission")
    if not isinstance(admission, dict) or admission.get("enabled") is not True:
        raise EvidenceError("generated topology has no enabled Worker resource admission")
    fixed = _object_budget(admission.get("fixed_objects"), "generated fixed inventory")
    role_rows = _bounded_list(admission.get("executable_roles"), "generated executable roles")
    roles: dict[str, Mapping[str, Any]] = {}
    for row in role_rows:
        if not isinstance(row, dict):
            raise EvidenceError("generated executable role must be an object")
        role = _identifier(row.get("role"), "generated executable role")
        if role in roles:
            raise EvidenceError("generated executable roles are duplicated")
        if (
            not isinstance(row.get("executable_slots"), int)
            or isinstance(row["executable_slots"], bool)
            or not 1 <= row["executable_slots"] <= 0xFFFF
        ):
            raise EvidenceError("generated executable role has invalid slot count")
        _object_budget(row.get("per_slot"), f"generated {role} per-slot inventory")
        roles[role] = row
    if tuple(roles) != REQUIRED_ROLES:
        raise EvidenceError("generated topology lacks the exact executable Worker roles")

    mixes = _bounded_list(admission.get("allowed_role_mixes"), "generated role mixes")
    maximum = [mix for mix in mixes if isinstance(mix, dict) and mix.get("maximum") is True]
    if len(maximum) != 1:
        raise EvidenceError("generated topology requires exactly one maximum role mix")
    counts = maximum[0].get("roles")
    counts = _bounded_list(counts, "generated maximum role mix")
    required = dict(fixed)
    seen: set[str] = set()
    for count_row in counts:
        if not isinstance(count_row, dict):
            raise EvidenceError("generated maximum role count must be an object")
        _exact_keys(count_row, {"role", "count"}, context="generated maximum role count")
        role = count_row["role"]
        if role not in roles or role in seen:
            raise EvidenceError("generated maximum role mix is unknown or duplicated")
        count = count_row["count"]
        if (
            not isinstance(count, int)
            or isinstance(count, bool)
            or count != roles[role]["executable_slots"]
        ):
            raise EvidenceError("generated maximum role mix is not at the exact slot bound")
        seen.add(role)
        per_slot = roles[role]["per_slot"]
        for key in INVENTORY_KEYS:
            maximum_value = 0xFFFF_FFFF_FFFF_FFFF if key == "untyped_bytes" else 0xFFFF_FFFF
            required[key] += per_slot[key] * count
            if required[key] > maximum_value:
                raise EvidenceError("generated maximum inventory arithmetic overflow")
    if seen != set(REQUIRED_ROLES):
        raise EvidenceError("generated maximum role mix omits an executable Worker role")
    return required


def _generated_inventory(
    value: Mapping[str, Any], target: str, session: Mapping[str, Any]
) -> tuple[dict[str, Any], dict[str, int]]:
    _exact_keys(
        value,
        {
            "schema",
            "profile",
            "manifest_sha256",
            "topology_sha256",
            "topology",
            "inventory",
        },
        context="generated root-TCB inventory",
    )
    if (
        value["schema"] != GENERATED_INVENTORY_SCHEMA
        or value["profile"] != TARGET_PROFILE[target]
        or value["manifest_sha256"] != session["manifest_sha256"]
    ):
        raise EvidenceError("generated inventory differs from the selected target manifest")
    topology = value["topology"]
    if not isinstance(topology, dict):
        raise EvidenceError("generated topology must be an object")
    _exact_keys(
        topology,
        {
            "profile",
            "root_task",
            "worker_runtime",
            "temporal_authority",
            "worker_resource_admission",
            "ninedoor_service",
            "console_network_service",
        },
        context="generated topology",
    )
    profile = topology["profile"]
    if not isinstance(profile, dict) or profile.get("name") != value["profile"]:
        raise EvidenceError("generated topology profile differs from its envelope")
    topology_sha256 = _hash(value["topology_sha256"], "generated topology")
    if topology_sha256 != _canonical_json_sha256(topology):
        raise EvidenceError("generated topology digest mismatch")
    inventory = _inventory(value["inventory"], "generated inventory")
    if inventory != _maximum_inventory(topology):
        raise EvidenceError("generated inventory differs from compiler topology")
    return topology, inventory


def _validate_worker_topology(
    workers: Sequence[Mapping[str, Any]], topology: Mapping[str, Any]
) -> None:
    admission = topology["worker_resource_admission"]
    roles = {row["role"]: row for row in admission["executable_roles"]}
    temporal = topology["temporal_authority"]
    tasks = temporal.get("tasks") if isinstance(temporal, dict) else None
    tasks = _bounded_list(tasks, "generated temporal tasks")
    worker_tasks = [
        task for task in tasks if isinstance(task, dict) and task.get("kind") == "worker"
    ]
    fault_range = admission.get("handoff", {}).get("worker_fault_badges")
    if not isinstance(fault_range, dict):
        raise EvidenceError("generated topology lacks Worker fault badges")
    for field in ("base", "count", "stride"):
        _positive_u64(fault_range.get(field), f"Worker fault badge {field}")
    endpoint_caps = topology.get("worker_runtime", {}).get("endpoint_caps")
    if not isinstance(endpoint_caps, dict) or endpoint_caps.get("required") is not True:
        raise EvidenceError("generated topology lacks Worker endpoint badges")
    epoch_bits = endpoint_caps.get("epoch_bits")
    attach_base = endpoint_caps.get("attach_badge_base")
    if (
        not isinstance(epoch_bits, int)
        or isinstance(epoch_bits, bool)
        or not 1 <= epoch_bits < 63
    ):
        raise EvidenceError("generated Worker endpoint epoch bits are invalid")
    _positive_u64(attach_base, "Worker attach badge base")
    role_indexes = {"worker-heartbeat": 1, "worker-gpu": 2, "worker-lora": 4}

    for worker in workers:
        identity = worker["identity"]
        role = identity["role"]
        role_row = roles[role]
        if (
            worker["core"] != role_row.get("core")
            or worker["object_inventory"] != role_row.get("per_slot")
        ):
            raise EvidenceError("Worker core/object inventory differs from generated topology")
        task_id = f"{role_row.get('task_prefix')}{identity['slot']}"
        matches = [task for task in worker_tasks if task.get("id") == task_id]
        if len(matches) != 1:
            raise EvidenceError("Worker identity does not name one generated temporal task")
        task = matches[0]
        if worker["scheduling_context"] != {
            "budget_us": task.get("budget_us"),
            "period_us": task.get("period_us"),
        }:
            raise EvidenceError("Worker scheduling context differs from generated topology")
        worker_ordinal = worker_tasks.index(task)
        expected_fault = fault_range["base"] + worker_ordinal * fault_range["stride"]
        if worker_ordinal >= fault_range["count"] or worker["fault_badge"] != expected_fault:
            raise EvidenceError("Worker fault badge differs from generated topology")
        epoch = identity["lease_epoch"]
        if epoch >= 1 << epoch_bits:
            raise EvidenceError("Worker lease epoch exceeds generated endpoint badge range")
        expected_endpoint = attach_base + ((role_indexes[role] << epoch_bits) | epoch)
        if worker["endpoint_badge"] != expected_endpoint:
            raise EvidenceError("Worker endpoint badge differs from generated topology")


def validate_root(
    record: Mapping[str, Any],
    target: str,
    worker_raw: bytes,
    worker: Mapping[str, Any] | None = None,
) -> None:
    _scan_sensitive(record)
    _exact_keys(
        record,
        {
            "schema",
            "record_kind",
            "target",
            "target_session",
            "worker_component",
            "topology_sha256",
            "generated_inventory",
            "inventory_scope",
            "observed_inventory",
            "outcomes",
            "raw_evidence",
            "verdict",
            "blockers",
        },
        context="root-TCB evidence",
    )
    if (
        record["schema"] != ROOT_SCHEMA
        or record["record_kind"] != "root-tcb"
        or record["target"] != target
    ):
        raise EvidenceError("wrong root-TCB schema/kind/target")
    _target_session(record["target_session"], target)
    if worker is not None and (
        record["target_session"] != worker["target_session"]
        or record["topology_sha256"] != worker["topology_sha256"]
    ):
        raise EvidenceError("root-TCB target session/topology differs from Worker component")
    reference = _reference(record["worker_component"], "target-component")
    if reference["sha256"] != _sha256(worker_raw):
        raise EvidenceError("root-TCB record references different Worker component bytes")
    _hash(record["topology_sha256"], "topology")
    if record["inventory_scope"] != "admitted-maximum":
        raise EvidenceError("root-TCB inventory scope is not admitted-maximum")
    generated = _inventory(record["generated_inventory"], "generated inventory")
    observed = _inventory(
        record["observed_inventory"], "observed admitted-maximum inventory"
    )
    if record["verdict"] == "PASS" and generated != observed:
        raise EvidenceError("accepted generated/observed inventory differs")
    outcomes = _outcomes(
        record["outcomes"],
        "root outcomes",
        required=record["verdict"] == "PASS",
    )
    if record["verdict"] == "PASS":
        _required_pass_outcomes(
            outcomes,
            ROOT_REQUIRED_OUTCOMES,
            "root outcomes",
        )
    _artifacts(record["raw_evidence"], "root raw evidence", required=record["verdict"] == "PASS")
    _verdict(record)


def _target_session_file(path: Path, target: str) -> tuple[dict[str, Any], bytes]:
    session, raw = _load(path)
    _target_session(session, target)
    return session, raw


def _read_frozen_artifact(path: Path, label: str) -> bytes:
    """Read one unchanged, non-symlink, bounded regular file exactly once."""
    try:
        before = path.lstat()
    except OSError as exc:
        raise EvidenceError(f"cannot inspect {label}: {path}: {exc}") from exc
    if stat.S_ISLNK(before.st_mode) or not stat.S_ISREG(before.st_mode):
        raise EvidenceError(f"{label} must be a regular non-symlink file: {path}")
    if before.st_size <= 0 or before.st_size > MAX_ARTIFACT_BYTES:
        raise EvidenceError(
            f"{label} size is outside 1..{MAX_ARTIFACT_BYTES}: {path}"
        )
    try:
        with path.open("rb") as handle:
            opened = os.fstat(handle.fileno())
            raw = handle.read(MAX_ARTIFACT_BYTES + 1)
            closed = os.fstat(handle.fileno())
        after = path.lstat()
    except OSError as exc:
        raise EvidenceError(f"cannot read {label}: {path}: {exc}") from exc
    identity = lambda value: (  # noqa: E731 - compact stable-file identity
        value.st_dev,
        value.st_ino,
        value.st_mode,
        value.st_size,
        value.st_mtime_ns,
    )
    if (
        len(raw) != before.st_size
        or len(raw) > MAX_ARTIFACT_BYTES
        or identity(before) != identity(opened)
        or identity(opened) != identity(closed)
        or identity(closed) != identity(after)
    ):
        raise EvidenceError(f"{label} changed while it was being frozen: {path}")
    return raw


def _artifact_text(raw: bytes, label: str) -> str:
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise EvidenceError(f"{label} is not exact UTF-8 text") from exc
    if any(ord(character) < 0x20 and character not in "\n\r\t" for character in text):
        raise EvidenceError(f"{label} contains prohibited control bytes")
    lowered = text.lower()
    if any(marker in lowered for marker in SENSITIVE_MARKERS):
        raise EvidenceError(f"{label} contains prohibited sensitive material")
    return text


def _marker_rows(
    text: str,
    marker: str,
    required_keys: set[str],
    *,
    required: bool = True,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    pattern = re.compile(rf"(?<![A-Z0-9_]){re.escape(marker)}(?![A-Z0-9_])")
    for line_number, line in enumerate(text.splitlines(), start=1):
        match = pattern.search(line)
        if match is None:
            continue
        payload = line[match.end() :].strip()
        values: dict[str, str] = {}
        for token in payload.split():
            if token.count("=") != 1:
                raise EvidenceError(
                    f"{marker} line {line_number} has a malformed field"
                )
            key, value = token.split("=", 1)
            if (
                not re.fullmatch(r"[a-z][a-z0-9_]*", key)
                or not value
                or key in values
            ):
                raise EvidenceError(
                    f"{marker} line {line_number} has an invalid/duplicate field"
                )
            values[key] = value
        if set(values) != required_keys:
            raise EvidenceError(
                f"{marker} line {line_number} fields differ from the exact marker schema"
            )
        rows.append({"line": line_number, **values})
    if required and not rows:
        raise EvidenceError(f"missing required live marker: {marker}")
    return rows


def _marker_uint(
    row: Mapping[str, Any],
    key: str,
    *,
    maximum: int = 0xFFFF_FFFF_FFFF_FFFF,
) -> int:
    value = row[key]
    if not isinstance(value, str) or not re.fullmatch(r"(?:0x)?[0-9A-Fa-f]+", value):
        raise EvidenceError(f"marker {key} is not an unsigned integer")
    base = 16 if value.lower().startswith("0x") else 10
    try:
        parsed = int(value, base)
    except ValueError as exc:  # pragma: no cover - guarded by the regular expression
        raise EvidenceError(f"marker {key} is not an unsigned integer") from exc
    if parsed < 0 or parsed > maximum:
        raise EvidenceError(f"marker {key} exceeds its unsigned bound")
    return parsed


def _marker_identity(row: Mapping[str, Any]) -> tuple[str, int, int, int, int]:
    role = row.get("role")
    if role not in REQUIRED_ROLES:
        raise EvidenceError("live marker names a non-executable Worker role")
    return (
        role,
        _marker_uint(row, "slot", maximum=0xFFFF),
        _marker_uint(row, "lease_epoch"),
        _marker_uint(row, "supervisor_generation"),
        _marker_uint(row, "cap_generation"),
    )


def _parse_worker_elfs(values: Sequence[str]) -> dict[str, Path]:
    paths: dict[str, Path] = {}
    for value in values:
        role, separator, path_value = value.partition("=")
        if separator != "=" or role not in REQUIRED_ROLES or not path_value or role in paths:
            raise EvidenceError(
                "--worker-elf must name each executable role exactly once as ROLE=PATH"
            )
        paths[role] = Path(path_value)
    if tuple(paths) != REQUIRED_ROLES:
        raise EvidenceError("--worker-elf must use exact generated Worker role order")
    return paths


def _parse_service_elfs(values: Sequence[str]) -> dict[str, Path]:
    paths: dict[str, Path] = {}
    services = tuple(QEMU_SERVICE_SYMBOLS)
    for value in values:
        service, separator, path_value = value.partition("=")
        if (
            separator != "="
            or service not in QEMU_SERVICE_SYMBOLS
            or not path_value
            or service in paths
        ):
            raise EvidenceError(
                "--service-elf must name each service exactly once as SERVICE=PATH"
            )
        paths[service] = Path(path_value)
    if tuple(paths) != services:
        raise EvidenceError("--service-elf must use exact NineDoor/console service order")
    return paths


def _worker_manifest_image_hashes(
    session: Mapping[str, Any],
    manifest_raw: bytes,
    elf_raw: Mapping[str, bytes],
) -> dict[str, str]:
    if _sha256(manifest_raw) != session["worker_image_manifest_sha256"]:
        raise EvidenceError("Worker manifest bytes differ from target session")
    try:
        manifest = json.loads(manifest_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("Worker image manifest is invalid JSON") from exc
    if (
        not isinstance(manifest, dict)
        or manifest.get("schema") != "cohesix-worker-image-manifest/v1"
        or manifest.get("target") != "aarch64-unknown-none"
    ):
        raise EvidenceError("Worker image manifest envelope is invalid")
    images = manifest.get("images")
    if not isinstance(images, list) or len(images) != 3:
        raise EvidenceError("Worker image manifest lacks the exact role matrix")
    result: dict[str, str] = {}
    for index, row in enumerate(images):
        role = REQUIRED_ROLES[index]
        if (
            not isinstance(row, dict)
            or row.get("role") != role
            or row.get("entry_symbol") != "_start"
            or row.get("source_sha256") != _sha256(elf_raw[role])
            or not SHA256_RE.fullmatch(str(row.get("image_sha256", "")))
        ):
            raise EvidenceError("Worker ELF/image manifest role binding is invalid")
        result[role] = row["image_sha256"]
    return result


def _validate_worker_build_artifacts(
    session: Mapping[str, Any],
    archive_raw: bytes,
    manifest_raw: bytes,
    elf_raw: Mapping[str, bytes],
) -> dict[str, str]:
    if _sha256(archive_raw) != session["worker_archive_sha256"]:
        raise EvidenceError("Worker archive bytes differ from target session")
    try:
        manifest = json.loads(manifest_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("Worker image manifest is invalid JSON") from exc
    if manifest.get("archive") != {
        "bytes": len(archive_raw),
        "sha256": _sha256(archive_raw),
    }:
        raise EvidenceError("Worker image manifest archive binding is invalid")
    return _worker_manifest_image_hashes(session, manifest_raw, elf_raw)


def _validate_driver_archive(session: Mapping[str, Any], archive_raw: bytes) -> None:
    if _sha256(archive_raw) != session["driver_archive_sha256"]:
        raise EvidenceError("external canonical driver archive differs from target session")


def _pass_outcomes(identifiers: Sequence[str]) -> list[dict[str, str]]:
    return [
        {"id": identifier, "class": "observation", "result": "pass"}
        for identifier in identifiers
    ]


def _artifact_row(identifier: str, raw: bytes) -> dict[str, Any]:
    return {"id": identifier, "sha256": _sha256(raw), "bytes": len(raw)}


def _load_frozen_json(path: Path, label: str) -> tuple[dict[str, Any], bytes]:
    raw = _read_frozen_artifact(path, label)
    try:
        value = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"invalid JSON in {label}: {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} root must be an object")
    _scan_sensitive(value)
    return value, raw


def _resolved_directory(path: Path, label: str) -> Path:
    try:
        metadata = path.lstat()
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise EvidenceError(f"cannot resolve {label}: {path}: {exc}") from exc
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise EvidenceError(f"{label} must be an existing non-symlink directory")
    return resolved


def _bounded_artifact_path(root: Path, relative: Path, label: str) -> Path:
    if relative.is_absolute() or not relative.parts or ".." in relative.parts:
        raise EvidenceError(f"{label} path is outside the verified QEMU output")
    current = root
    try:
        for component in relative.parts:
            current = current / component
            metadata = current.lstat()
            if stat.S_ISLNK(metadata.st_mode):
                raise EvidenceError(f"{label} path contains a symlink: {current}")
    except OSError as exc:
        raise EvidenceError(f"cannot inspect {label}: {current}: {exc}") from exc
    return current


def _git_output(repo_root: Path, arguments: Sequence[str], label: str) -> bytes:
    try:
        completed = subprocess.run(
            ["git", *arguments],
            cwd=repo_root,
            check=False,
            capture_output=True,
        )
    except OSError as exc:
        raise EvidenceError(f"cannot run git for {label}: {exc}") from exc
    if completed.returncode != 0:
        raise EvidenceError(f"git failed while deriving {label}")
    return completed.stdout


def _git_visible_paths(repo_root: Path) -> list[Path]:
    raw = _git_output(
        repo_root,
        ("ls-files", "-co", "--exclude-standard", "-z"),
        "source inventory",
    )
    paths: list[Path] = []
    seen: set[str] = set()
    for encoded in raw.split(b"\0"):
        if not encoded:
            continue
        relative = Path(os.fsdecode(encoded))
        rendered = relative.as_posix()
        if (
            relative.is_absolute()
            or not relative.parts
            or ".." in relative.parts
            or rendered in seen
        ):
            raise EvidenceError("git returned an invalid or duplicate source path")
        seen.add(rendered)
        paths.append(relative)
    paths.sort(key=lambda value: value.as_posix())
    return paths


def _source_inventory_row(repo_root: Path, relative: Path) -> dict[str, Any]:
    path = repo_root / relative
    try:
        before = path.lstat()
    except FileNotFoundError:
        return {
            "path": relative.as_posix(),
            "kind": "deleted",
            "mode": 0,
            "sha256": _sha256(b""),
            "bytes": 0,
        }
    except OSError as exc:
        raise EvidenceError(f"cannot inspect source entry {relative}: {exc}") from exc

    if stat.S_ISLNK(before.st_mode):
        try:
            raw = os.fsencode(os.readlink(path))
            after = path.lstat()
        except OSError as exc:
            raise EvidenceError(f"cannot read source symlink {relative}: {exc}") from exc
        identity = lambda value: (  # noqa: E731 - compact stable-file identity
            value.st_dev,
            value.st_ino,
            value.st_mode,
            value.st_size,
            value.st_mtime_ns,
        )
        if identity(before) != identity(after):
            raise EvidenceError(f"source symlink changed while frozen: {relative}")
        kind = "symlink"
    elif stat.S_ISREG(before.st_mode):
        try:
            with path.open("rb") as handle:
                opened = os.fstat(handle.fileno())
                raw = handle.read(MAX_ARTIFACT_BYTES + 1)
                closed = os.fstat(handle.fileno())
            after = path.lstat()
        except OSError as exc:
            raise EvidenceError(f"cannot read source entry {relative}: {exc}") from exc
        identity = lambda value: (  # noqa: E731 - compact stable-file identity
            value.st_dev,
            value.st_ino,
            value.st_mode,
            value.st_size,
            value.st_mtime_ns,
        )
        if (
            len(raw) != before.st_size
            or len(raw) > MAX_ARTIFACT_BYTES
            or identity(before) != identity(opened)
            or identity(opened) != identity(closed)
            or identity(closed) != identity(after)
        ):
            raise EvidenceError(f"source entry changed while frozen: {relative}")
        kind = "file"
    else:
        raise EvidenceError(f"unsupported source inventory entry: {relative}")
    return {
        "path": relative.as_posix(),
        "kind": kind,
        "mode": stat.S_IMODE(before.st_mode),
        "sha256": _sha256(raw),
        "bytes": len(raw),
    }


def _source_inventory_bytes(repo_root: Path) -> bytes:
    paths = _git_visible_paths(repo_root)
    rows = [_source_inventory_row(repo_root, relative) for relative in paths]
    if _git_visible_paths(repo_root) != paths:
        raise EvidenceError("git-visible source paths changed while frozen")
    for relative, expected in zip(paths, rows, strict=True):
        if _source_inventory_row(repo_root, relative) != expected:
            raise EvidenceError(f"source entry changed while frozen: {relative}")
    record = {
        "schema": SOURCE_INVENTORY_SCHEMA,
        "algorithm": "git-visible-paths-sha256",
        "entries": rows,
    }
    try:
        return (
            json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n"
        ).encode("utf-8")
    except UnicodeEncodeError as exc:
        raise EvidenceError("source inventory paths are not exact UTF-8") from exc


def _validate_session_output_location(
    repo_root: Path,
    qemu_out: Path,
    requested: Path,
) -> Path:
    if not requested.name or requested.name in (".", ".."):
        raise EvidenceError("target-session output directory is invalid")
    parent = _resolved_directory(requested.parent, "target-session output parent")
    output = parent / requested.name
    if output.exists() or output.is_symlink():
        raise EvidenceError("target-session output directory must not already exist")
    try:
        output.relative_to(qemu_out)
    except ValueError:
        pass
    else:
        raise EvidenceError("target-session output cannot mutate verified QEMU output")
    try:
        relative = output.relative_to(repo_root)
    except ValueError:
        return output
    try:
        completed = subprocess.run(
            ["git", "check-ignore", "-q", "--", relative.as_posix()],
            cwd=repo_root,
            check=False,
            capture_output=True,
        )
    except OSError as exc:
        raise EvidenceError(f"cannot validate target-session output: {exc}") from exc
    if completed.returncode != 0:
        raise EvidenceError(
            "target-session output inside the repository must be git-ignored"
        )
    return output


def _validate_qemu_launch_artifacts(
    qemu_out: Path,
) -> dict[str, bytes]:
    record_path = _bounded_artifact_path(
        qemu_out,
        Path("cohesix-qemu-launch-artifacts.json"),
        "QEMU launch record",
    )
    record, _ = _load_frozen_json(record_path, "QEMU launch record")
    _exact_keys(
        record,
        {
            "schema",
            "profile",
            "cargo_target",
            "root_task_features",
            "sel4_build_dir",
            "gic_version",
            "artifacts",
        },
        context="QEMU launch record",
    )
    if (
        record["schema"] != QEMU_LAUNCH_SCHEMA
        or record["profile"] != "release"
        or record["cargo_target"] != "aarch64-unknown-none"
        or record["root_task_features"] != "release-qemu,bootstrap-trace"
        or record["gic_version"] != "3"
    ):
        raise EvidenceError("QEMU launch record is not the exact pressure profile")
    build_dir_raw = record["sel4_build_dir"]
    if not isinstance(build_dir_raw, str) or not Path(build_dir_raw).is_absolute():
        raise EvidenceError("QEMU launch record has an invalid seL4 build directory")
    build_dir = _resolved_directory(Path(build_dir_raw), "selected seL4 build directory")
    if str(build_dir) != build_dir_raw:
        raise EvidenceError("QEMU launch record aliases its seL4 build directory")

    rows = record["artifacts"]
    if not isinstance(rows, list) or len(rows) != len(QEMU_LAUNCH_ARTIFACTS):
        raise EvidenceError("QEMU launch record has the wrong artifact count")
    artifacts: dict[str, bytes] = {}
    for row, (identifier, relative) in zip(
        rows, QEMU_LAUNCH_ARTIFACTS, strict=True
    ):
        if not isinstance(row, dict):
            raise EvidenceError("QEMU launch artifact row must be an object")
        _exact_keys(
            row,
            {"id", "path", "bytes", "sha256"},
            context="QEMU launch artifact row",
        )
        if row["id"] != identifier or row["path"] != relative.as_posix():
            raise EvidenceError("QEMU launch artifact order/path differs")
        path = _bounded_artifact_path(qemu_out, relative, f"QEMU {identifier}")
        raw = _read_frozen_artifact(path, f"QEMU {identifier}")
        if row["bytes"] != len(raw) or row["sha256"] != _sha256(raw):
            raise EvidenceError(f"QEMU launch artifact bytes differ: {identifier}")
        artifacts[identifier] = raw
    if len(artifacts["initrd"]) >= 4 * 1024 * 1024:
        raise EvidenceError("QEMU rootfs CPIO exceeds the 4 MiB invariant")
    return artifacts


def _validate_session_manifests(
    qemu_out: Path,
    frozen: Mapping[str, bytes],
    parsed: Mapping[str, Mapping[str, Any]],
) -> None:
    worker_archive = _bounded_artifact_path(
        qemu_out,
        QEMU_SESSION_ARTIFACTS["worker_archive"],
        "Worker archive",
    )
    worker_manifest = _bounded_artifact_path(
        qemu_out,
        QEMU_SESSION_ARTIFACTS["worker_manifest"],
        "Worker image manifest",
    )
    driver_archive = _bounded_artifact_path(
        qemu_out,
        QEMU_SESSION_ARTIFACTS["driver_archive"],
        "driver runtime archive",
    )
    driver_manifest = _bounded_artifact_path(
        qemu_out,
        QEMU_SESSION_ARTIFACTS["driver_manifest"],
        "driver runtime manifest",
    )
    try:
        verified_worker = worker_images.verify_manifest(
            worker_manifest,
            worker_archive,
        )
        verified_driver = driver_runtimes.verify_manifest(
            driver_manifest,
            driver_archive,
        )
    except (
        OSError,
        worker_images.WorkerImageError,
        driver_runtimes.DriverRuntimeManifestError,
    ) as exc:
        raise EvidenceError(f"canonical Worker/driver manifest rejected: {exc}") from exc
    if (
        verified_worker != parsed["worker_manifest"]
        or verified_driver != parsed["driver_manifest"]
        or verified_worker.get("profile") != "release"
        or verified_driver.get("profile") != "release"
    ):
        raise EvidenceError("canonical Worker/driver manifest identity differs")
    for identifier, path in (
        ("worker_archive", worker_archive),
        ("worker_manifest", worker_manifest),
        ("driver_archive", driver_archive),
        ("driver_manifest", driver_manifest),
    ):
        if _read_frozen_artifact(path, identifier.replace("_", " ")) != frozen[
            identifier
        ]:
            raise EvidenceError(f"{identifier} changed during canonical validation")


def _worker_abi_identity(
    repo_root: Path,
    topology: Mapping[str, Any],
) -> bytes:
    runtime = topology.get("worker_runtime")
    task_abi = runtime.get("task_abi") if isinstance(runtime, dict) else None
    if (
        not isinstance(task_abi, dict)
        or task_abi.get("enabled") is not True
        or task_abi.get("version") != 1
    ):
        raise EvidenceError("generated topology lacks Worker task ABI version 1")
    files = []
    for relative in WORKER_ABI_FILES:
        path = _bounded_artifact_path(repo_root, relative, "Worker ABI source")
        raw = _read_frozen_artifact(path, f"Worker ABI source {relative}")
        files.append(
            {
                "path": relative.as_posix(),
                "sha256": _sha256(raw),
                "bytes": len(raw),
            }
        )
    record = {
        "schema": WORKER_ABI_IDENTITY_SCHEMA,
        "task_abi_schema": "worker-task-abi/v1",
        "task_abi_version": 1,
        "files": files,
    }
    raw = (
        json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode("utf-8")
    return raw


def _write_exclusive_artifact(path: Path, raw: bytes) -> None:
    if not raw or len(raw) > MAX_ARTIFACT_BYTES:
        raise EvidenceError("target-session artifact exceeds its bounded size")
    temporary: Path | None = None
    try:
        descriptor, name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.")
        temporary = Path(name)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
        os.link(temporary, path)
    except FileExistsError as exc:
        raise EvidenceError(f"refusing to overwrite target-session artifact: {path}") from exc
    except OSError as exc:
        raise EvidenceError(f"cannot publish target-session artifact {path}: {exc}") from exc
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _publish_target_session(output: Path, artifacts: Mapping[str, bytes]) -> None:
    try:
        output.mkdir(mode=0o700)
    except FileExistsError as exc:
        raise EvidenceError("target-session output directory already exists") from exc
    except OSError as exc:
        raise EvidenceError(f"cannot create target-session output directory: {exc}") from exc
    published: list[Path] = []
    try:
        for name, raw in artifacts.items():
            path = output / name
            _write_exclusive_artifact(path, raw)
            published.append(path)
        descriptor = os.open(output, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    except (EvidenceError, OSError) as exc:
        for path in published:
            path.unlink(missing_ok=True)
        try:
            output.rmdir()
        except OSError:
            pass
        if isinstance(exc, EvidenceError):
            raise
        raise EvidenceError(f"cannot finalize target-session output: {exc}") from exc


def _emit_qemu_target_session(args: argparse.Namespace) -> None:
    repo_root = _resolved_directory(args.repo_root, "repository root")
    try:
        top_level = _git_output(
            repo_root,
            ("rev-parse", "--show-toplevel"),
            "repository root",
        ).decode("utf-8", errors="strict").strip()
        exact_top_level = Path(top_level).resolve(strict=True)
    except (UnicodeDecodeError, OSError) as exc:
        raise EvidenceError(f"cannot resolve exact Git worktree root: {exc}") from exc
    if exact_top_level != repo_root:
        raise EvidenceError("--repo-root is not the exact Git worktree root")
    qemu_out = _resolved_directory(args.qemu_out, "verified QEMU output")
    output = _validate_session_output_location(repo_root, qemu_out, args.out_dir)

    launch_artifacts = _validate_qemu_launch_artifacts(qemu_out)
    manifest, manifest_raw = _load_frozen_json(
        args.resolved_manifest, "resolved root-task manifest"
    )
    profile = manifest.get("profile")
    if not isinstance(profile, dict) or profile.get("name") != TARGET_PROFILE["qemu"]:
        raise EvidenceError("resolved manifest is not the QEMU target profile")
    topology_record, _topology_raw = _load_frozen_json(
        args.topology, "generated root-task topology"
    )
    manifest_sha256 = _sha256(manifest_raw)
    topology, _inventory_value = _generated_inventory(
        topology_record,
        "qemu",
        {"manifest_sha256": manifest_sha256},
    )
    abi_raw = _worker_abi_identity(repo_root, topology)

    frozen: dict[str, bytes] = {}
    parsed: dict[str, dict[str, Any]] = {}
    for identifier, relative in QEMU_SESSION_ARTIFACTS.items():
        path = _bounded_artifact_path(qemu_out, relative, identifier.replace("_", " "))
        raw = _read_frozen_artifact(path, identifier.replace("_", " "))
        frozen[identifier] = raw
        if identifier.endswith("manifest"):
            try:
                value = json.loads(raw)
            except (UnicodeDecodeError, json.JSONDecodeError) as exc:
                raise EvidenceError(f"{identifier} is invalid JSON") from exc
            if not isinstance(value, dict):
                raise EvidenceError(f"{identifier} must be a JSON object")
            _scan_sensitive(value)
            parsed[identifier] = value
    _validate_session_manifests(qemu_out, frozen, parsed)

    source_raw = _source_inventory_bytes(repo_root)
    cyw43_record = {
        "schema": CYW43_QEMU_BINDING_SCHEMA,
        "target": "qemu",
        "selected": False,
        "classification": "not-applicable-physical-driver",
        "driver_archive_sha256": _sha256(frozen["driver_archive"]),
    }
    cyw43_raw = (
        json.dumps(cyw43_record, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode("utf-8")
    session = {
        "target": "qemu",
        "source_sha256": _sha256(source_raw),
        "manifest_sha256": manifest_sha256,
        "kernel_sha256": _sha256(launch_artifacts["kernel"]),
        "root_image_sha256": _sha256(launch_artifacts["rootserver"]),
        "driver_archive_sha256": _sha256(frozen["driver_archive"]),
        "driver_manifest_sha256": _sha256(frozen["driver_manifest"]),
        "cyw43_coexistence_record_sha256": _sha256(cyw43_raw),
        "worker_archive_sha256": _sha256(frozen["worker_archive"]),
        "worker_image_manifest_sha256": _sha256(frozen["worker_manifest"]),
        "worker_abi_sha256": _sha256(abi_raw),
    }
    _target_session(session, "qemu")
    session_raw = (json.dumps(session, indent=2, sort_keys=True) + "\n").encode(
        "utf-8"
    )
    _publish_target_session(
        output,
        {
            "source-inventory.json": source_raw,
            "worker-abi-identity.json": abi_raw,
            "qemu-cyw43-coexistence.json": cyw43_raw,
            "target-session.json": session_raw,
        },
    )
    print(f"worker evidence: qemu target session PASS ({output})")


def _pressure_proc(value: Any, label: str) -> None:
    if not isinstance(value, dict) or set(value) != set(QEMU_PROC_KEYS):
        raise EvidenceError(f"{label} lacks the exact five canonical /proc projections")
    for key in QEMU_PROC_KEYS:
        row = value[key]
        if not isinstance(row, dict):
            raise EvidenceError(f"{label} {key} projection is malformed")
        _exact_keys(row, {"lines", "sha256", "bytes"}, context=f"{label} {key}")
        lines = row["lines"]
        if (
            not isinstance(lines, list)
            or not lines
            or len(lines) > MAX_LIST_ITEMS
            or any(not isinstance(line, str) for line in lines)
        ):
            raise EvidenceError(f"{label} {key} lines are empty or unbounded")
        encoded = "\n".join(lines).encode("utf-8")
        if row["sha256"] != _sha256(encoded) or row["bytes"] != len(encoded):
            raise EvidenceError(f"{label} {key} bytes/hash do not match exact lines")


def _pressure_worker_identity(row: Mapping[str, Any]) -> tuple[str, int, int, int, int]:
    return (
        row.get("role"),
        row.get("slot"),
        row.get("lease_epoch"),
        row.get("supervisor_generation"),
        row.get("cap_generation"),
    )


def _validate_pressure_worker_rows(
    value: Any,
    topology: Mapping[str, Any],
    label: str,
) -> list[dict[str, Any]]:
    if not isinstance(value, list) or len(value) != 3:
        raise EvidenceError(f"{label} requires exactly three live Worker rows")
    required = {
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
    observations: list[dict[str, Any]] = []
    for index, row in enumerate(value):
        if not isinstance(row, dict):
            raise EvidenceError(f"{label} Worker row is malformed")
        _exact_keys(row, required, context=f"{label} Worker row")
        role = row["role"]
        if role != REQUIRED_ROLES[index] or row["lifecycle"] != "ready":
            raise EvidenceError(f"{label} lacks the exact ordered READY role matrix")
        if (
            not isinstance(row["worker"], str)
            or not re.fullmatch(r"worker-[1-9][0-9]*", row["worker"])
        ):
            raise EvidenceError(f"{label} has a non-canonical Worker identity")
        for field in (
            "slot",
            "lease_epoch",
            "supervisor_generation",
            "cap_generation",
            "ready_sequence",
            "control_sequence",
            "receipt_sequence",
            "completion_sequence",
        ):
            if (
                not isinstance(row[field], int)
                or isinstance(row[field], bool)
                or row[field] < 0
            ):
                raise EvidenceError(f"{label} Worker {field} is invalid")
        expected_receipt = "none" if role == "worker-heartbeat" else "confirmed"
        if (
            row["artifact"] != "verified"
            or row["receipt"] != expected_receipt
            or row["execution_proof"] != "qemu"
        ):
            raise EvidenceError(f"{label} Worker evidence axes are incomplete")
        observations.append(
            {
                "identity": {
                    "role": role,
                    "slot": row["slot"],
                    "lease_epoch": row["lease_epoch"],
                    "supervisor_generation": row["supervisor_generation"],
                    "cap_generation": row["cap_generation"],
                },
                "state": {
                    "declaration": "executable",
                    "lifecycle": "ready",
                    "artifact": "verified",
                    "receipt": expected_receipt,
                    "execution_proof": "qemu",
                },
                "image_sha256": row["image_sha256"],
                "ready_sequence": row["ready_sequence"],
                "completion_sequence": row["completion_sequence"],
                "endpoint_badge": 1,
                "fault_badge": 2,
                "core": row["core"],
                "scheduling_context": row["scheduling_context"],
                "object_inventory": row["object_inventory"],
            }
        )
    # Badges are replaced from UART before the complete topology check. This
    # still proves the pressure-projected SC/core/object rows immediately.
    admission = topology["worker_resource_admission"]
    roles = {row["role"]: row for row in admission["executable_roles"]}
    temporal = topology["temporal_authority"]["tasks"]
    for observation in observations:
        role = observation["identity"]["role"]
        slot = observation["identity"]["slot"]
        role_row = roles[role]
        task_id = f"{role_row['task_prefix']}{slot}"
        tasks = [task for task in temporal if task.get("id") == task_id]
        if len(tasks) != 1 or (
            observation["core"] != role_row["core"]
            or observation["object_inventory"] != role_row["per_slot"]
            or observation["scheduling_context"]
            != {
                "budget_us": tasks[0]["budget_us"],
                "period_us": tasks[0]["period_us"],
            }
        ):
            raise EvidenceError(f"{label} Worker topology differs from compiler truth")
    return observations


def _pressure_reports(
    paths: Sequence[Path],
    session: Mapping[str, Any],
    session_raw: bytes,
    generated: Mapping[str, Any],
    topology: Mapping[str, Any],
    uart_artifacts: Sequence[bytes],
    gdb_artifacts: Sequence[bytes],
) -> tuple[list[dict[str, Any]], list[tuple[str, bytes]]]:
    if len(paths) < 2:
        raise EvidenceError("QEMU collection requires separate medium and high pressure reports")
    if len(uart_artifacts) != len(paths) or len(gdb_artifacts) != len(paths):
        raise EvidenceError(
            "QEMU collection requires one ordered UART/GDB artifact pair per pressure report"
        )
    reports: list[dict[str, Any]] = []
    artifacts: list[tuple[str, bytes]] = []
    intensities: list[float] = []
    session_projection = {
        key: session[key]
        for key in (
            "manifest_sha256",
            "root_image_sha256",
            "worker_archive_sha256",
            "worker_image_manifest_sha256",
            "worker_abi_sha256",
        )
    }
    for index, path in enumerate(paths):
        summary, raw = _load_frozen_json(path, f"pressure report {index + 1}")
        report = summary.get("report")
        if not isinstance(report, dict) or report.get("schema") != "cohesix-benchmark-report/v1":
            raise EvidenceError("pressure report lacks the benchmark report schema")
        if summary.get("target_session_sha256") != _sha256(session_raw):
            raise EvidenceError("pressure report targets a different target-session file")
        population = report.get("population")
        if not isinstance(population, dict):
            raise EvidenceError("pressure report lacks executable population axes")
        for key, expected in {
            "mode": "executable",
            "maximum_live_tasks": 3,
            "requested": 3,
            "discovered": 3,
            "ready": 3,
            "backend_class": "console-projection",
            "proof_class": "qemu",
        }.items():
            if population.get(key) != expected:
                raise EvidenceError(f"pressure population {key} is not {expected!r}")
        workload = report.get("workload")
        reliability = report.get("reliability")
        if (
            not isinstance(workload, dict)
            or workload.get("population_mode") != "executable"
            or workload.get("control_write_outcome") != "admitted"
            or not isinstance(reliability, dict)
            or reliability.get("error_budget_pass") is not True
        ):
            raise EvidenceError("pressure workload/error budget did not pass executable mode")
        intensity = workload.get("intensity_max")
        if not isinstance(intensity, (int, float)) or isinstance(intensity, bool):
            raise EvidenceError("pressure report has no numeric workload intensity")
        intensities.append(float(intensity))

        executable = report.get("executable_state")
        if not isinstance(executable, dict) or set(executable) != {
            "topology_sha256",
            "target_session",
            "pre",
            "post",
            "lifecycle_cycles",
            "receipt_operations",
            "fault_artifacts",
            "required_fault_markers",
        }:
            raise EvidenceError("pressure report lacks executable_state")
        if (
            executable.get("topology_sha256") != generated["topology_sha256"]
            or executable.get("target_session") != session_projection
        ):
            raise EvidenceError("pressure executable state targets different artifacts/topology")
        expected_faults = {
            "uart": {
                "sha256": _sha256(uart_artifacts[index]),
                "bytes": len(uart_artifacts[index]),
            },
            "gdb": {
                "sha256": _sha256(gdb_artifacts[index]),
                "bytes": len(gdb_artifacts[index]),
            },
        }
        if executable.get("fault_artifacts") != expected_faults:
            raise EvidenceError("pressure fault artifacts do not match exact UART/GDB bytes")
        required_markers = executable.get("required_fault_markers")
        if (
            not isinstance(required_markers, list)
            or len(required_markers) != len(set(required_markers))
            or not {
                "uart:WORKER_TASK_ADMISSION",
                "uart:WORKER_TASK_READY",
                "uart:WORKER_TASK_RECEIPT",
                "uart:WORKER_TASK_COMPLETION",
                "uart:WORKER_TASK_FAULT",
                "uart:WORKER_TASK_TEARDOWN",
                "gdb:M26E_GDB_ELF",
                "gdb:M26E_GDB_INJECTION",
            }.issubset(set(required_markers))
        ):
            raise EvidenceError("pressure report lacks the exact required fault marker index")
        for phase in ("pre", "post"):
            phase_state = executable.get(phase)
            if not isinstance(phase_state, dict) or set(phase_state) != {"workers", "proc"}:
                raise EvidenceError(f"pressure {phase} state has an unexpected schema")
            _validate_pressure_worker_rows(
                phase_state["workers"], topology, f"pressure {phase}"
            )
            _pressure_proc(phase_state["proc"], f"pressure {phase}")
        reports.append(executable)
        artifacts.append((f"pressure-{index + 1}", raw))
    if len(set(intensities)) < 2 or min(intensities) <= 0 or max(intensities) < 1.0:
        raise EvidenceError("pressure inputs do not span distinct medium-to-high intensities")
    return reports, artifacts


def _parse_live_worker_markers(text: str) -> dict[str, list[dict[str, Any]]]:
    identity = {
        "role",
        "slot",
        "lease_epoch",
        "supervisor_generation",
        "cap_generation",
    }
    inventory = set(INVENTORY_KEYS)
    return {
        "admission": _marker_rows(
            text,
            "WORKER_TASK_ADMISSION",
            identity
            | inventory
            | {
                "image_sha256",
                "endpoint_badge",
                "fault_badge",
                "core",
                "sc_budget_us",
                "sc_period_us",
                "state",
            },
        ),
        "ready": _marker_rows(
            text, "WORKER_TASK_READY", identity | {"sequence"}
        ),
        "control": _marker_rows(
            text,
            "WORKER_TASK_CONTROL",
            identity | {"action", "outcome", "sequence", "state"},
        ),
        "receipt": _marker_rows(
            text,
            "WORKER_TASK_RECEIPT",
            identity | {"action", "outcome", "sequence"},
        ),
        "completion": _marker_rows(
            text,
            "WORKER_TASK_COMPLETION",
            identity | {"action", "status", "sequence"},
        ),
        "fault": _marker_rows(
            text,
            "WORKER_TASK_FAULT",
            identity | {"class", "observed_badge", "state"},
        ),
        "teardown": _marker_rows(
            text,
            "WORKER_TASK_TEARDOWN",
            identity
            | {
                "reason",
                "tcb_suspended",
                "records_cleared",
                "scheduling_context_unbound",
                "mappings_scrubbed",
                "descendants_revoked",
                "objects_deleted",
                "generation_fenced",
                "state",
            },
        ),
    }


def _admission_observation(row: Mapping[str, Any]) -> dict[str, Any]:
    identity = _marker_identity(row)
    if row["state"] != "admitted" or not SHA256_RE.fullmatch(row["image_sha256"]):
        raise EvidenceError("Worker admission is incomplete or has an invalid image hash")
    object_inventory = {key: _marker_uint(row, key) for key in INVENTORY_KEYS}
    return {
        "identity": {
            "role": identity[0],
            "slot": identity[1],
            "lease_epoch": identity[2],
            "supervisor_generation": identity[3],
            "cap_generation": identity[4],
        },
        "state": {
            "declaration": "executable",
            "lifecycle": "ready",
            "artifact": "verified",
            "receipt": "none" if identity[0] == "worker-heartbeat" else "confirmed",
            "execution_proof": "qemu",
        },
        "image_sha256": row["image_sha256"],
        "ready_sequence": 0,
        "completion_sequence": 0,
        "endpoint_badge": _marker_uint(row, "endpoint_badge"),
        "fault_badge": _marker_uint(row, "fault_badge"),
        "core": _marker_uint(row, "core", maximum=3),
        "scheduling_context": {
            "budget_us": _marker_uint(row, "sc_budget_us", maximum=0xFFFF_FFFF),
            "period_us": _marker_uint(row, "sc_period_us", maximum=0xFFFF_FFFF),
        },
        "object_inventory": object_inventory,
    }


def _validate_marker_lifecycle(
    markers: Mapping[str, list[dict[str, Any]]],
    topology: Mapping[str, Any],
) -> tuple[
    dict[tuple[str, int, int, int, int], dict[str, Any]],
    set[tuple[int, int]],
    set[str],
    set[str],
    set[tuple[str, str]],
]:
    admissions: dict[tuple[str, int, int, int, int], dict[str, Any]] = {}
    for row in markers["admission"]:
        identity = _marker_identity(row)
        if identity in admissions:
            raise EvidenceError("duplicate Worker admission identity")
        observation = _admission_observation(row)
        _validate_worker_topology([observation], topology)
        admissions[identity] = observation
    if set(identity[0] for identity in admissions) != set(REQUIRED_ROLES):
        raise EvidenceError("UART admissions omit an executable Worker role")

    ready: dict[tuple[str, int, int, int, int], list[dict[str, Any]]] = {}
    for row in markers["ready"]:
        identity = _marker_identity(row)
        if identity not in admissions or row["line"] <= markers["admission"][
            list(admissions).index(identity)
        ]["line"]:
            raise EvidenceError("Worker READY is unbound or precedes admission")
        sequence = _marker_uint(row, "sequence")
        if sequence == 0:
            raise EvidenceError("Worker READY sequence is zero")
        ready.setdefault(identity, []).append(row)

    for kind in ("control", "receipt", "completion", "fault", "teardown"):
        for row in markers[kind]:
            if _marker_identity(row) not in admissions:
                raise EvidenceError(f"Worker {kind} marker has no exact admission identity")

    matrix: set[tuple[int, int]] = set()
    receipts = markers["receipt"]
    completions = markers["completion"]
    for receipt in receipts:
        action = _marker_uint(receipt, "action", maximum=0xFFFF)
        outcome = _marker_uint(receipt, "outcome", maximum=0xFFFF)
        if action not in QEMU_RECEIPT_ACTIONS or outcome not in QEMU_TERMINAL_OUTCOMES:
            raise EvidenceError("Worker receipt uses a non-canonical action/outcome")
        if _marker_identity(receipt)[0] != QEMU_RECEIPT_ACTIONS[action][1]:
            raise EvidenceError("Worker receipt action crossed its role boundary")
        sequence = _marker_uint(receipt, "sequence")
        matches = [
            completion
            for completion in completions
            if _marker_identity(completion) == _marker_identity(receipt)
            and _marker_uint(completion, "action", maximum=0xFFFF) == action
            and _marker_uint(completion, "status", maximum=0xFFFF) == outcome
            and _marker_uint(completion, "sequence") == sequence
            and completion["line"] > receipt["line"]
        ]
        if len(matches) != 1:
            raise EvidenceError("Worker receipt lacks one later identity-bound completion")
        matrix.add((action, outcome))
    for teardown in markers["teardown"]:
        identity = _marker_identity(teardown)
        if teardown["state"] != "terminal" or any(
            teardown[field] != "yes"
            for field in (
                "tcb_suspended",
                "records_cleared",
                "scheduling_context_unbound",
                "mappings_scrubbed",
                "descendants_revoked",
                "objects_deleted",
                "generation_fenced",
            )
        ):
            raise EvidenceError("Worker teardown lacks exact zero-leak containment")
        if any(
            row["line"] > teardown["line"] and _marker_identity(row) == identity
            for kind in ("ready", "control", "receipt", "completion")
            for row in markers[kind]
        ):
            raise EvidenceError("retired Worker identity produced post-revoke activity")

    faults_by_role = {role: [] for role in REQUIRED_ROLES}
    fault_phase_roles: set[tuple[str, str]] = set()
    temporal = {
        task["id"]: task
        for task in topology["temporal_authority"]["tasks"]
        if task.get("kind") == "worker"
    }
    for fault in markers["fault"]:
        identity = _marker_identity(fault)
        observation = admissions[identity]
        expected_badge = observation["fault_badge"]
        if fault["class"] == "Timeout":
            expected_badge = temporal[f"{identity[0]}-slot-{identity[1]}"]["timeout_badge"]
        elif fault["class"] != "Standard":
            raise EvidenceError("Worker fault class is not Standard or Timeout")
        if (
            fault["state"] != "faulted"
            or _marker_uint(fault, "observed_badge") != expected_badge
        ):
            raise EvidenceError("Worker fault did not arrive on its generated badge")
        earlier_ready = any(
            _marker_identity(row) == identity and row["line"] < fault["line"]
            for row in markers["ready"]
        )
        earlier_control = any(
            _marker_identity(row) == identity and row["line"] < fault["line"]
            for row in markers["control"]
        )
        if fault["class"] == "Standard" and not earlier_ready:
            fault_phase_roles.add((identity[0], "pre-ready"))
        if fault["class"] == "Standard" and earlier_ready and earlier_control:
            fault_phase_roles.add((identity[0], "during-ipc"))
        if fault["class"] == "Timeout" and earlier_ready and earlier_control:
            fault_phase_roles.add((identity[0], "budget-exhaustion"))
        teardowns = [
            row
            for row in markers["teardown"]
            if _marker_identity(row) == identity and row["line"] > fault["line"]
        ]
        if len(teardowns) != 1:
            raise EvidenceError("Worker fault lacks one later terminal teardown")
        faults_by_role[identity[0]].append(fault)
    sequential_roles: set[str] = set()
    for role in REQUIRED_ROLES:
        generations = sorted(
            identity[3] for identity in admissions if identity[0] == role
        )
        if len(generations) >= 2 and len(generations) == len(set(generations)):
            sequential_roles.add(role)
    return (
        admissions,
        matrix,
        {role for role, rows in faults_by_role.items() if rows},
        sequential_roles,
        fault_phase_roles,
    )


def _validate_gdb_markers(
    text: str,
    session: Mapping[str, Any],
    generated: Mapping[str, Any],
    elf_raw: Mapping[str, bytes],
    admissions: Mapping[tuple[str, int, int, int, int], Mapping[str, Any]],
    fault_phase_roles: set[tuple[str, str]],
) -> str:
    session_rows = _marker_rows(
        text,
        "M26E_QEMU_SESSION",
        {
            "target",
            "machine",
            "gic_version",
            "root_image_sha256",
            "worker_archive_sha256",
            "topology_sha256",
        },
    )
    if len(session_rows) != 1 or any(
        session_rows[0][key] != expected
        for key, expected in {
            "target": "qemu",
            "machine": "virt",
            "gic_version": "3",
            "root_image_sha256": session["root_image_sha256"],
            "worker_archive_sha256": session["worker_archive_sha256"],
            "topology_sha256": generated["topology_sha256"],
        }.items()
    ):
        raise EvidenceError("GDB session marker differs from QEMU GICv3 target truth")
    elf_rows = _marker_rows(
        text,
        "M26E_GDB_ELF",
        {"role", "elf_sha256", "image_sha256"},
    )
    if len(elf_rows) != 3 or tuple(row["role"] for row in elf_rows) != REQUIRED_ROLES:
        raise EvidenceError("GDB transcript lacks the exact ordered Worker ELF matrix")
    for row in elf_rows:
        role = row["role"]
        images = {
            observation["image_sha256"]
            for identity, observation in admissions.items()
            if identity[0] == role
        }
        if row["elf_sha256"] != _sha256(elf_raw[role]) or images != {
            row["image_sha256"]
        }:
            raise EvidenceError("GDB ELF/image marker differs from exact live Worker bytes")
    injections = _marker_rows(
        text,
        "M26E_GDB_INJECTION",
        {"role", "phase", "symbol", "action", "result"},
    )
    expected = {
        (
            "pre-ready",
            "_start",
            "zero-x0",
        ),
        (
            "during-ipc",
            "cohesix_worker_qemu_evidence_control_handler",
            "redirect-standard-fault",
        ),
        (
            "budget-exhaustion",
            "cohesix_worker_qemu_evidence_control_handler",
            "redirect-timeout-spin",
        ),
    }
    observed = {
        (row["phase"], row["symbol"], row["action"])
        for row in injections
        if row["role"] in REQUIRED_ROLES and row["result"] == "continued"
    }
    if observed != expected or len(injections) != 3:
        raise EvidenceError("GDB transcript lacks the exact three-phase injection plan")
    roles = {row["role"] for row in injections}
    if len(roles) != 1:
        raise EvidenceError("one GDB transcript must inject exactly one Worker role")
    role = roles.pop()
    if not {
        (role, "pre-ready"),
        (role, "during-ipc"),
        (role, "budget-exhaustion"),
    }.issubset(fault_phase_roles):
        raise EvidenceError("GDB injection phases lack role-matched UART faults")
    return role


def _validate_qemu_session_marker(
    text: str,
    session: Mapping[str, Any],
    generated: Mapping[str, Any],
) -> None:
    rows = _marker_rows(
        text,
        "M26E_QEMU_SESSION",
        {
            "target",
            "machine",
            "gic_version",
            "root_image_sha256",
            "worker_archive_sha256",
            "topology_sha256",
        },
    )
    expected = {
        "target": "qemu",
        "machine": "virt",
        "gic_version": "3",
        "root_image_sha256": session["root_image_sha256"],
        "worker_archive_sha256": session["worker_archive_sha256"],
        "topology_sha256": generated["topology_sha256"],
    }
    if len(rows) != 1 or any(rows[0][key] != value for key, value in expected.items()):
        raise EvidenceError("GDB session marker differs from QEMU GICv3 target truth")


def _validate_service_gdb_markers(
    texts: Sequence[str],
    session: Mapping[str, Any],
    session_raw: bytes,
    generated: Mapping[str, Any],
    service_raw: Mapping[str, bytes],
    root_raw: bytes,
) -> None:
    if len(texts) != len(QEMU_SERVICE_EVIDENCE_PLAN):
        raise EvidenceError("preflight requires the exact four service GDB transcripts")
    observed_plan: list[tuple[str, str]] = []
    for text, expected_plan in zip(
        texts, QEMU_SERVICE_EVIDENCE_PLAN, strict=True
    ):
        _validate_qemu_session_marker(text, session, generated)
        auth_rows = _marker_rows(
            text,
            "M26E_QEMU_AUTH",
            {
                "result",
                "observation_sha256",
                "observation_bytes",
                "serial_sha256",
                "serial_bytes",
                "launch_record_sha256",
                "launch_record_bytes",
                "target_session_sha256",
                "target_session_bytes",
            },
        )
        if (
            len(auth_rows) != 1
            or auth_rows[0]["result"] != "PASS"
            or auth_rows[0]["target_session_sha256"] != _sha256(session_raw)
            or _marker_uint(auth_rows[0], "target_session_bytes") != len(session_raw)
            or any(
                SHA256_RE.fullmatch(auth_rows[0][key]) is None
                for key in (
                    "observation_sha256",
                    "serial_sha256",
                    "launch_record_sha256",
                )
            )
            or any(
                _marker_uint(auth_rows[0], key) == 0
                for key in (
                    "observation_bytes",
                    "serial_bytes",
                    "launch_record_bytes",
                )
            )
        ):
            raise EvidenceError(
                "service GDB transcript lacks its exact authenticated-QEMU binding"
            )
        elf_rows = _marker_rows(
            text,
            "M26E_GDB_SERVICE_ELF",
            {"service", "mode", "elf_sha256", "elf_bytes", "root_image_sha256"},
        )
        if len(elf_rows) != 1:
            raise EvidenceError("service GDB transcript has an ambiguous ELF binding")
        service = elf_rows[0]["service"]
        mode = elf_rows[0]["mode"]
        if (
            service not in QEMU_SERVICE_SYMBOLS
            or mode not in QEMU_SERVICE_MODES[service]
            or (service, mode) != expected_plan
            or elf_rows[0]["elf_sha256"] != _sha256(service_raw[service])
            or _marker_uint(elf_rows[0], "elf_bytes") != len(service_raw[service])
            or elf_rows[0]["root_image_sha256"] != session["root_image_sha256"]
        ):
            raise EvidenceError("service GDB ELF marker differs from immutable target bytes")
        root_rows = _marker_rows(
            text,
            "M26E_GDB_SERVICE_ROOT_ELF",
            {"service", "mode", "elf_sha256", "elf_bytes", "root_image_sha256"},
            required=False,
        )
        if mode == "between-calls-revoke":
            if (
                len(root_rows) != 1
                or root_rows[0]["service"] != service
                or root_rows[0]["mode"] != mode
                or root_rows[0]["elf_sha256"] != _sha256(root_raw)
                or _marker_uint(root_rows[0], "elf_bytes") != len(root_raw)
                or root_rows[0]["root_image_sha256"]
                != session["root_image_sha256"]
                or _sha256(root_raw) != session["root_image_sha256"]
            ):
                raise EvidenceError(
                    "between-Calls transcript differs from exact root ELF/image truth"
                )
        elif root_rows:
            raise EvidenceError("only between-Calls revoke may bind a root GDB hook")
        rows = _marker_rows(
            text,
            "M26E_GDB_SERVICE_INJECTION",
            {"service", "phase", "symbol", "action", "result"},
        )
        handler = QEMU_SERVICE_SYMBOLS[service][0]
        expected = {
            "during-call-standard": {
                ("during-call", handler, "redirect-standard-fault", "continued")
            },
            "between-calls-revoke": {
                (
                    "between-calls",
                    QEMU_NINEDOOR_ROOT_SYMBOLS[0],
                    "redirect-local-revoke",
                    "continued",
                )
            },
            "budget-exhaustion-timeout": {
                (
                    "budget-exhaustion",
                    handler,
                    "redirect-timeout-spin",
                    "continued",
                )
            },
        }[mode]
        observed = {
            (row["phase"], row["symbol"], row["action"], row["result"])
            for row in rows
            if row["service"] == service
        }
        if observed != expected or len(rows) != len(expected):
            raise EvidenceError("service GDB transcript lacks its exact injection plan")
        observed_plan.append((service, mode))
    if tuple(observed_plan) != QEMU_SERVICE_EVIDENCE_PLAN:
        raise EvidenceError("service GDB transcripts must use the exact evidence order")


def _validate_critical_gdb_markers(
    text: str,
    session: Mapping[str, Any],
    generated: Mapping[str, Any],
    root_raw: bytes,
) -> None:
    _validate_qemu_session_marker(text, session, generated)
    elf_rows = _marker_rows(
        text,
        "M26E_GDB_ROOT_ELF",
        {"elf_sha256", "root_image_sha256"},
    )
    if (
        len(elf_rows) != 1
        or elf_rows[0]["elf_sha256"] != _sha256(root_raw)
        or elf_rows[0]["root_image_sha256"] != session["root_image_sha256"]
    ):
        raise EvidenceError("critical GDB transcript differs from root ELF/image truth")
    rows = _marker_rows(
        text,
        "M26E_GDB_CRITICAL_OBSERVATION",
        {"duty", "symbol", "result"},
    )
    observed = {(row["duty"], row["symbol"], row["result"]) for row in rows}
    expected = {
        (duty, symbol, "observed")
        for symbol, duty in QEMU_CRITICAL_DUTIES.items()
    }
    if observed != expected or len(rows) != len(expected):
        raise EvidenceError("critical GDB transcript omits a live root duty lane")


def _validate_root_markers(text: str, generated_inventory: Mapping[str, int]) -> None:
    inventory_rows = _marker_rows(
        text,
        "ROOT_TCB_INVENTORY",
        set(INVENTORY_KEYS) | {"scope", "state"},
    )
    observed = {
        key: _marker_uint(inventory_rows[-1], key) for key in INVENTORY_KEYS
    }
    if (
        inventory_rows[-1]["scope"] != "admitted-maximum"
        or inventory_rows[-1]["state"] != "sealed"
        or observed != dict(generated_inventory)
    ):
        raise EvidenceError("root admitted-maximum budget differs from compiler truth")
    actual_rows = _marker_rows(
        text,
        "ROOT_CRITICAL_OBJECTS",
        {
            "scope",
            "duties",
            "restricted_children",
            "tcbs",
            "scheduling_contexts",
            "reply_objects",
            "standard_fault_caps",
            "timeout_fault_caps",
            "fault_registrations",
            "state",
        },
    )
    expected_actual = {
        "scope": "constructed-actual",
        "duties": "5",
        "restricted_children": "4",
        "tcbs": "5",
        "scheduling_contexts": "5",
        "reply_objects": "6",
        "standard_fault_caps": "4",
        "timeout_fault_caps": "4",
        "fault_registrations": "10",
        "state": "sealed",
    }
    if len(actual_rows) != 1 or any(
        actual_rows[0][key] != value for key, value in expected_actual.items()
    ):
        raise EvidenceError("root actual critical object/registration counts are incomplete")
    required_literals = (
        "[critical] exact generated fault registry sealed sources=10",
        "[critical] independent fault/emergency/Worker/driver duties active",
        "[worker] target supervisor armed after exact registry and critical activation",
        "GPU_BRIDGE_FIXTURE_ADMISSION",
        "LORA_EXPORT_FIXTURE_ADMISSION",
        "mode=fixture",
        "profile=qemu",
        "gate=bootstrap-trace",
    )
    missing = [literal for literal in required_literals if literal not in text]
    if missing:
        raise EvidenceError(f"UART lacks root/service live markers: {','.join(missing)}")


def _validate_service_uart_markers(texts: Sequence[str]) -> None:
    """Validate terminal service outcomes from their distinct fresh boots."""

    if len(texts) != len(QEMU_SERVICE_EVIDENCE_PLAN):
        raise EvidenceError("preflight requires four ordered fresh service UART boots")
    fault_pattern = re.compile(
        r"\[(ninedoor-service|console-network)\] generation=([1-9][0-9]*) "
        r"terminal-fault class=(Standard|Timeout) sequence=[1-9][0-9]*"
    )
    revoke_pattern = re.compile(
        r"\[ninedoor-service\] generation=([1-9][0-9]*) "
        r"terminal-revoke state=local"
    )
    for text, (service, mode) in zip(
        texts, QEMU_SERVICE_EVIDENCE_PLAN, strict=True
    ):
        ninedoor = _marker_rows(
            text,
            "NINEDOOR_SERVICE_TEARDOWN",
            {
                "generation",
                "tcb_suspended",
                "mappings_scrubbed",
                "recovery_reply_revoked",
                "capabilities_revoked",
                "generation_fenced",
                "state",
            },
            required=service == "ninedoor-service",
        )
        console = _marker_rows(
            text,
            "CONSOLE_NETWORK_TEARDOWN",
            {
                "generation",
                "tcb_suspended",
                "scheduling_context_unbound",
                "mappings_scrubbed",
                "capabilities_revoked",
                "objects_deleted",
                "generation_fenced",
                "state",
            },
            required=service == "console-network",
        )
        if service == "ninedoor-service" and console:
            raise EvidenceError("NineDoor injection boot contains console teardown")
        if service == "console-network" and ninedoor:
            raise EvidenceError("console injection boot contains NineDoor teardown")
        for row in (*ninedoor, *console):
            if row["state"] != "terminal" or any(
                value != "yes"
                for key, value in row.items()
                if key not in {"line", "generation", "state"}
            ):
                raise EvidenceError("critical service teardown is incomplete")

        faults = {
            (observed_service, int(generation), fault_class)
            for observed_service, generation, fault_class in fault_pattern.findall(text)
        }
        revokes = {int(generation) for generation in revoke_pattern.findall(text)}
        teardown_generations = {
            _marker_uint(row, "generation") for row in (*ninedoor, *console)
        }
        if mode == "during-call-standard":
            expected = {(service, generation, "Standard") for generation in teardown_generations}
            service_teardowns = ninedoor if service == "ninedoor-service" else console
            if len(service_teardowns) != 1 or faults != expected or revokes:
                raise EvidenceError(
                    f"{service} boot lacks one standard fault and teardown"
                )
        elif mode == "between-calls-revoke":
            if len(ninedoor) != 1 or faults or revokes != teardown_generations:
                raise EvidenceError(
                    "between-Calls NineDoor boot lacks one local revoke and teardown"
                )
        elif mode == "budget-exhaustion-timeout":
            expected = {
                ("console-network", generation, "Timeout")
                for generation in teardown_generations
            }
            if len(console) != 1 or faults != expected or revokes:
                raise EvidenceError(
                    "console-network boot lacks one timeout fault and teardown"
                )
        else:  # pragma: no cover - exact evidence plan owns the mode set
            raise EvidenceError(f"unsupported service UART evidence mode: {mode}")


def _validate_cohsh_transcript(text: str) -> None:
    required = (
        "OK SPAWN",
        "OK KILL",
        "model-only",
        "worker-bus",
    )
    missing = [literal for literal in required if literal not in text]
    if missing:
        raise EvidenceError(
            f"cohsh transcript lacks actual operator outcomes: {','.join(missing)}"
        )
    if not re.search(r"ERR[^\n]*(?:slot|busy|already-live|maximum)", text, re.IGNORECASE):
        raise EvidenceError("cohsh transcript lacks simultaneous second-live-slot refusal")
    if not re.search(r"ERR[^\n]*worker-bus[^\n]*model-only", text, re.IGNORECASE):
        raise EvidenceError("cohsh transcript lacks deterministic WorkerBus rejection")


def _final_workers_from_pressure(
    reports: Sequence[Mapping[str, Any]],
    markers: Mapping[str, list[dict[str, Any]]],
    admissions: Mapping[tuple[str, int, int, int, int], dict[str, Any]],
    topology: Mapping[str, Any],
) -> list[dict[str, Any]]:
    post = reports[-1]["post"]["workers"]
    observations: list[dict[str, Any]] = []
    for pressure_row in post:
        identity = _pressure_worker_identity(pressure_row)
        if identity not in admissions:
            raise EvidenceError("final pressure Worker has no exact UART admission")
        observation = dict(admissions[identity])
        ready = [
            row
            for row in markers["ready"]
            if _marker_identity(row) == identity
            and _marker_uint(row, "sequence") == pressure_row["ready_sequence"]
        ]
        completion = [
            row
            for row in markers["completion"]
            if _marker_identity(row) == identity
            and _marker_uint(row, "sequence") == pressure_row["completion_sequence"]
        ]
        if len(ready) != 1 or len(completion) != 1:
            raise EvidenceError("final pressure Worker sequences lack exact UART records")
        observation["ready_sequence"] = pressure_row["ready_sequence"]
        observation["completion_sequence"] = pressure_row["completion_sequence"]
        if (
            observation["image_sha256"] != pressure_row["image_sha256"]
            or observation["core"] != pressure_row["core"]
            or observation["scheduling_context"] != pressure_row["scheduling_context"]
            or observation["object_inventory"] != pressure_row["object_inventory"]
        ):
            raise EvidenceError("final pressure Worker differs from UART admission truth")
        observations.append(observation)
    _validate_worker_topology(observations, topology)
    return observations


def _live_workers_from_uart(
    markers: Mapping[str, list[dict[str, Any]]],
    admissions: Mapping[tuple[str, int, int, int, int], dict[str, Any]],
    topology: Mapping[str, Any],
) -> list[dict[str, Any]]:
    workers: list[dict[str, Any]] = []
    retired = {_marker_identity(row) for row in markers["teardown"]}
    for role in REQUIRED_ROLES:
        candidates = [
            row
            for row in markers["ready"]
            if _marker_identity(row)[0] == role
            and _marker_identity(row) not in retired
        ]
        if not candidates:
            raise EvidenceError(f"preflight lacks a current live READY {role}")
        ready = max(candidates, key=lambda row: row["line"])
        identity = _marker_identity(ready)
        observation = dict(admissions[identity])
        completions = [
            row
            for row in markers["completion"]
            if _marker_identity(row) == identity
        ]
        if not completions:
            raise EvidenceError(f"preflight lacks a current completion for {role}")
        completion = max(completions, key=lambda row: _marker_uint(row, "sequence"))
        if role != "worker-heartbeat" and not any(
            _marker_identity(row) == identity
            and _marker_uint(row, "outcome", maximum=0xFFFF) == 1
            for row in markers["receipt"]
        ):
            raise EvidenceError(f"preflight lacks a confirmed receipt for {role}")
        observation["ready_sequence"] = _marker_uint(ready, "sequence")
        observation["completion_sequence"] = _marker_uint(completion, "sequence")
        workers.append(observation)
    _validate_worker_topology(workers, topology)
    return workers


def _validate_pressure_cycles_and_receipts(
    reports: Sequence[Mapping[str, Any]],
    markers: Mapping[str, list[dict[str, Any]]],
) -> None:
    cycles = [cycle for report in reports for cycle in report["lifecycle_cycles"]]
    if not cycles:
        raise EvidenceError("pressure reports lack a same-role lifecycle cycle")
    for cycle in cycles:
        if not isinstance(cycle, dict) or set(cycle) != {
            "role",
            "before",
            "after",
            "kill_admitted",
            "recreate_admitted",
            "terminal_observed",
            "ready_observed",
        }:
            raise EvidenceError("pressure lifecycle cycle has an unexpected schema")
        if cycle["role"] not in REQUIRED_ROLES or any(
            cycle[key] is not True
            for key in (
                "kill_admitted",
                "recreate_admitted",
                "terminal_observed",
                "ready_observed",
            )
        ):
            raise EvidenceError("pressure lifecycle cycle was not fully observed")
        before = cycle["before"]
        after = cycle["after"]
        if not isinstance(before, dict) or not isinstance(after, dict):
            raise EvidenceError("pressure lifecycle identities are malformed")
        keys = {"role", "slot", "lease_epoch", "supervisor_generation", "cap_generation"}
        if set(before) != keys or set(after) != keys:
            raise EvidenceError("pressure lifecycle identity schema differs")
        before_id = (
            before["role"],
            before["slot"],
            before["lease_epoch"],
            before["supervisor_generation"],
            before["cap_generation"],
        )
        after_id = (
            after["role"],
            after["slot"],
            after["lease_epoch"],
            after["supervisor_generation"],
            after["cap_generation"],
        )
        if (
            before["role"] != cycle["role"]
            or after["role"] != cycle["role"]
            or after["supervisor_generation"] <= before["supervisor_generation"]
            or not any(
                _marker_identity(row) == before_id
                and row["reason"] in {"shutdown", "revoked"}
                for row in markers["teardown"]
            )
            or not any(_marker_identity(row) == after_id for row in markers["ready"])
        ):
            raise EvidenceError("pressure lifecycle cycle differs from UART ordering")

    operations = [
        operation for report in reports for operation in report["receipt_operations"]
    ]
    if not operations:
        raise EvidenceError("pressure reports lack live receipt operations")
    retired_identities = {_marker_identity(row) for row in markers["teardown"]}
    for operation in operations:
        if not isinstance(operation, dict) or set(operation) != {
            "action",
            "role",
            "worker_id",
            "sequence_before",
            "sequence_after",
            "status",
        }:
            raise EvidenceError("pressure receipt operation has an unexpected schema")
        action = operation["action"]
        expected = next(
            (
                (raw_action, role)
                for raw_action, (label, role) in QEMU_RECEIPT_ACTIONS.items()
                if label == action
            ),
            None,
        )
        before = operation["sequence_before"]
        after = operation["sequence_after"]
        if (
            expected is None
            or operation["role"] != expected[1]
            or not isinstance(operation["worker_id"], str)
            or not re.fullmatch(r"worker-[1-9][0-9]*", operation["worker_id"])
            or operation["status"] not in {"succeeded", "failed", "expired"}
            or not isinstance(before, dict)
            or not isinstance(after, dict)
            or set(before) != {"receipt", "completion"}
            or set(after) != {"receipt", "completion"}
            or after["receipt"] <= before["receipt"]
            or after["completion"] <= before["completion"]
        ):
            raise EvidenceError("pressure receipt operation is not terminal/monotonic")
        status_outcome = {"succeeded": 1, "failed": 2, "expired": 8}[
            operation["status"]
        ]
        receipt_matches = [
            row
            for row in markers["receipt"]
            if _marker_identity(row)[0] == expected[1]
            and _marker_identity(row) not in retired_identities
            and _marker_uint(row, "action", maximum=0xFFFF) == expected[0]
            and _marker_uint(row, "outcome", maximum=0xFFFF) == status_outcome
            and _marker_uint(row, "sequence") == after["receipt"]
        ]
        completion_matches = [
            row
            for row in markers["completion"]
            if _marker_identity(row)[0] == expected[1]
            and _marker_identity(row) not in retired_identities
            and _marker_uint(row, "action", maximum=0xFFFF) == expected[0]
            and _marker_uint(row, "status", maximum=0xFFFF) == status_outcome
            and _marker_uint(row, "sequence") == after["completion"]
        ]
        if len(receipt_matches) != 1 or len(completion_matches) != 1:
            raise EvidenceError("pressure receipt operation differs from exact UART outcome")


def _core_admission_from_topology(topology: Mapping[str, Any]) -> list[dict[str, int]]:
    temporal = topology["temporal_authority"]
    rows = temporal.get("core_admission")
    if not isinstance(rows, list) or len(rows) != 4:
        raise EvidenceError("generated topology lacks four-core admission truth")
    result: list[dict[str, int]] = []
    for core, row in enumerate(rows):
        admitted = sum(
            task["budget_us"]
            for task in temporal["tasks"]
            if task.get("admitted") is True and task.get("core") == core
        )
        result.append(
            {
                "core": core,
                "capacity_us": row["capacity_us"],
                "reserve_us": row["reserve_us"],
                "admitted_us": admitted,
            }
        )
    _core_admission(result)
    return result


def _symbol_addresses(
    nm: Path,
    elf: Path,
    symbols: Sequence[str],
    label: str,
) -> dict[str, int]:
    try:
        completed = subprocess.run(
            [str(nm), "-g", "--defined-only", str(elf)],
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise EvidenceError(f"cannot inspect {label} symbols with {nm}: {exc}") from exc
    if completed.returncode != 0:
        detail = completed.stderr.strip().splitlines()[-1:] or ["unknown nm failure"]
        raise EvidenceError(f"{label} symbol inspection failed: {detail[0]}")
    addresses: dict[str, int] = {}
    expected = set(symbols)
    for line in completed.stdout.splitlines():
        match = re.fullmatch(r"([0-9A-Fa-f]+)\s+[A-Za-z]\s+(\S+)", line.strip())
        if match and match.group(2) in expected:
            symbol = match.group(2)
            if symbol in addresses:
                raise EvidenceError(f"{label} ELF duplicates evidence symbol {symbol}")
            addresses[symbol] = int(match.group(1), 16)
    if set(addresses) != expected or len(set(addresses.values())) != len(addresses):
        raise EvidenceError(f"{label} ELF lacks its distinct QEMU evidence symbols")
    return addresses


def _rust_symbol_addresses(
    nm: Path,
    elf: Path,
    symbols: Sequence[str],
    module: str,
    label: str,
) -> dict[str, int]:
    """Resolve exact retained Rust functions without unsafe exported symbols."""

    try:
        completed = subprocess.run(
            [str(nm), "-g", "--defined-only", "--demangle", str(elf)],
            check=False,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise EvidenceError(f"cannot inspect {label} symbols with {nm}: {exc}") from exc
    if completed.returncode != 0:
        detail = completed.stderr.strip().splitlines()[-1:] or ["unknown nm failure"]
        raise EvidenceError(f"{label} symbol inspection failed: {detail[0]}")

    qualified = {f"{module}::{symbol}": symbol for symbol in symbols}
    addresses: dict[str, int] = {}
    for line in completed.stdout.splitlines():
        match = re.fullmatch(r"([0-9A-Fa-f]+)\s+[A-Za-z]\s+(\S+)", line.strip())
        if not match or match.group(2) not in qualified:
            continue
        symbol = qualified[match.group(2)]
        if symbol in addresses:
            raise EvidenceError(f"{label} ELF duplicates evidence symbol {symbol}")
        addresses[symbol] = int(match.group(1), 16)
    expected = set(symbols)
    if set(addresses) != expected or len(set(addresses.values())) != len(addresses):
        raise EvidenceError(f"{label} ELF lacks its distinct QEMU evidence symbols")
    return addresses


def _worker_symbol_addresses(nm: Path, elf: Path) -> dict[str, int]:
    return _symbol_addresses(nm, elf, QEMU_WORKER_SYMBOLS, "Worker")


def _validate_remote_and_tools(
    remote: str,
    timeout_secs: int,
    gdb_path: Path,
    nm_path: Path | None,
) -> tuple[Path, Path]:
    if (
        not re.fullmatch(r"(?:127\.0\.0\.1|localhost):[1-9][0-9]{0,4}", remote)
        or int(remote.rsplit(":", 1)[1]) > 65_535
    ):
        raise EvidenceError("--remote must be a bounded loopback HOST:PORT")
    if not isinstance(timeout_secs, int) or not 1 <= timeout_secs <= 1800:
        raise EvidenceError("--timeout-secs must be in 1..1800")
    try:
        gdb = gdb_path.resolve(strict=True)
    except OSError as exc:
        raise EvidenceError(f"GDB executable does not exist: {gdb_path}") from exc
    if not gdb.is_file() or not os.access(gdb, os.X_OK):
        raise EvidenceError("--gdb must resolve to an executable regular file")
    nm = nm_path or gdb.with_name("aarch64-none-elf-nm")
    try:
        nm = nm.resolve(strict=True)
    except OSError as exc:
        raise EvidenceError(f"nm executable does not exist: {nm}") from exc
    if not nm.is_file() or not os.access(nm, os.X_OK):
        raise EvidenceError("--nm must resolve to an executable regular file")
    return gdb, nm


def _run_gdb_batch(
    gdb: Path,
    command_text: str,
    timeout_secs: int,
    prefix: str,
) -> subprocess.CompletedProcess[bytes]:
    with tempfile.TemporaryDirectory(prefix=prefix) as directory:
        command_path = Path(directory) / "evidence.gdb"
        command_path.write_text(command_text, encoding="utf-8")
        try:
            return subprocess.run(
                [str(gdb), "--nx", "--quiet", "--batch", "-x", str(command_path)],
                check=False,
                capture_output=True,
                timeout=timeout_secs,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise EvidenceError(f"QEMU GDB evidence run did not complete: {exc}") from exc


def _write_frozen_output(path: Path, raw: bytes) -> None:
    if path.exists() or path.is_symlink():
        raise EvidenceError(f"refusing to overwrite evidence artifact: {path}")
    if not raw or len(raw) > MAX_ARTIFACT_BYTES:
        raise EvidenceError("GDB transcript exceeds its bounded artifact size")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            dir=path.parent,
            prefix=f".{path.name}.",
            delete=False,
        ) as handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
            temporary = Path(handle.name)
        temporary.replace(path)
    except OSError as exc:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
        raise EvidenceError(f"cannot publish GDB transcript {path}: {exc}") from exc


def _qemu_gdb(args: argparse.Namespace) -> None:
    gdb, nm = _validate_remote_and_tools(
        args.remote, args.timeout_secs, args.gdb, args.nm
    )

    session_raw = _read_frozen_artifact(args.target_session, "target session")
    generated_raw = _read_frozen_artifact(args.generated_inventory, "generated topology")
    manifest_raw = _read_frozen_artifact(
        args.worker_image_manifest, "Worker image manifest"
    )
    try:
        session = json.loads(session_raw)
        generated = json.loads(generated_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("GDB target session/topology is invalid JSON") from exc
    _target_session(session, "qemu")
    _generated_inventory(generated, "qemu", session)
    elf_paths = _parse_worker_elfs(args.worker_elf)
    elf_raw = {
        role: _read_frozen_artifact(path, f"{role} unstripped ELF")
        for role, path in elf_paths.items()
    }
    image_hashes = _worker_manifest_image_hashes(session, manifest_raw, elf_raw)
    inject_elf = elf_paths[args.inject_role]
    addresses = _worker_symbol_addresses(nm, inject_elf)
    entry = addresses["_start"]
    control = addresses["cohesix_worker_qemu_evidence_control_handler"]
    standard = addresses["cohesix_worker_qemu_evidence_standard_fault"]
    timeout_spin = addresses["cohesix_worker_qemu_evidence_timeout_spin"]
    command_text = f"""set pagination off
set confirm off
set architecture aarch64
file {_gdb_file_argument(inject_elf)}
target remote {args.remote}
delete breakpoints
set $m26e_control_hits = 0
hbreak *0x{entry:x}
commands 1
  silent
  printf "M26E_GDB_INJECTION role={args.inject_role} phase=pre-ready symbol=_start action=zero-x0 result=continued\\n"
  set $x0 = 0
  disable 1
  continue
end
hbreak *0x{control:x}
commands 2
  silent
  set $m26e_control_hits = $m26e_control_hits + 1
  if $m26e_control_hits == 1
    printf "M26E_GDB_INJECTION role={args.inject_role} phase=during-ipc symbol=cohesix_worker_qemu_evidence_control_handler action=redirect-standard-fault result=continued\\n"
    set $pc = 0x{standard:x}
    continue
  end
  if $m26e_control_hits == 2
    printf "M26E_GDB_INJECTION role={args.inject_role} phase=budget-exhaustion symbol=cohesix_worker_qemu_evidence_control_handler action=redirect-timeout-spin result=continued\\n"
    set $pc = 0x{timeout_spin:x}
    disable 2
    detach
    quit
  end
  printf "M26E_GDB_ABORT reason=unexpected-control-hit result=failed\\n"
  detach
  quit
end
continue
"""
    header_lines = [
        _qemu_session_header(session, generated),
        *(
            "M26E_GDB_ELF "
            f"role={role} elf_sha256={_sha256(elf_raw[role])} "
            f"image_sha256={image_hashes[role]}"
            for role in REQUIRED_ROLES
        ),
    ]
    completed = _run_gdb_batch(
        gdb, command_text, args.timeout_secs, "cohesix-m26e-worker-gdb-"
    )
    transcript = (
        ("\n".join(header_lines) + "\n").encode("utf-8")
        + completed.stdout
        + completed.stderr
    )
    if completed.returncode != 0:
        raise EvidenceError(
            "QEMU GDB injection failed; no acceptance transcript was published"
        )
    text = _artifact_text(transcript, "GDB transcript")
    if "M26E_GDB_ABORT" in text:
        raise EvidenceError("QEMU GDB injection hit an unexpected control turn")
    injection_rows = _marker_rows(
        text,
        "M26E_GDB_INJECTION",
        {"role", "phase", "symbol", "action", "result"},
    )
    if len(injection_rows) != 3 or {
        (row["phase"], row["symbol"], row["action"], row["result"])
        for row in injection_rows
    } != {
        ("pre-ready", "_start", "zero-x0", "continued"),
        (
            "during-ipc",
            "cohesix_worker_qemu_evidence_control_handler",
            "redirect-standard-fault",
            "continued",
        ),
        (
            "budget-exhaustion",
            "cohesix_worker_qemu_evidence_control_handler",
            "redirect-timeout-spin",
            "continued",
        ),
    }:
        raise EvidenceError("QEMU GDB runner did not complete its exact injection plan")
    _write_frozen_output(args.out, transcript)
    print(f"worker evidence: qemu GDB injection PASS ({args.out})")


def _qemu_session_header(
    session: Mapping[str, Any], generated: Mapping[str, Any]
) -> str:
    return (
        "M26E_QEMU_SESSION "
        "target=qemu machine=virt gic_version=3 "
        f"root_image_sha256={session['root_image_sha256']} "
        f"worker_archive_sha256={session['worker_archive_sha256']} "
        f"topology_sha256={generated['topology_sha256']}"
    )


def _validate_emitted_target_session_bundle(
    path: Path,
    session: Mapping[str, Any],
    session_raw: bytes,
) -> None:
    """Require the canonical four-file output emitted for one target session."""

    parent = _resolved_directory(path.parent, "emitted target-session directory")
    try:
        resolved_session = path.resolve(strict=True)
    except OSError as exc:
        raise EvidenceError(f"cannot resolve emitted target session: {path}: {exc}") from exc
    if path.name != "target-session.json" or resolved_session != parent / path.name:
        raise EvidenceError("--target-session must name the exact emitted target-session.json")
    if _sha256(session_raw) != _sha256(
        _read_frozen_artifact(resolved_session, "emitted target session")
    ):
        raise EvidenceError("emitted target session changed during validation")
    siblings = (
        ("source-inventory.json", "source_sha256"),
        ("worker-abi-identity.json", "worker_abi_sha256"),
        (
            "qemu-cyw43-coexistence.json",
            "cyw43_coexistence_record_sha256",
        ),
    )
    for name, field in siblings:
        raw = _read_frozen_artifact(parent / name, f"emitted target-session {name}")
        if _sha256(raw) != session[field]:
            raise EvidenceError(
                f"emitted target-session {name} differs from target-session identity"
            )


def _observation_file(
    value: Any,
    label: str,
    *,
    expected: Path | None = None,
) -> tuple[Path, bytes]:
    if not isinstance(value, dict):
        raise EvidenceError(f"authenticated QEMU observation {label} is not a file record")
    _exact_keys(
        value,
        {"path", "present", "size_bytes", "sha256"},
        context=f"authenticated QEMU observation {label}",
    )
    raw_path = value["path"]
    if not isinstance(raw_path, str) or not raw_path or not Path(raw_path).is_absolute():
        raise EvidenceError(
            f"authenticated QEMU observation {label} path is not canonical"
        )
    path = Path(raw_path)
    try:
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise EvidenceError(
            f"cannot resolve authenticated QEMU observation {label}: {path}: {exc}"
        ) from exc
    if resolved != path or (expected is not None and resolved != expected):
        raise EvidenceError(
            f"authenticated QEMU observation {label} aliases the expected artifact"
        )
    raw = _read_frozen_artifact(resolved, f"authenticated QEMU {label}")
    if (
        value["present"] is not True
        or value["size_bytes"] != len(raw)
        or value["sha256"] != _sha256(raw)
    ):
        raise EvidenceError(
            f"authenticated QEMU observation {label} bytes differ from its record"
        )
    return resolved, raw


def _validate_authenticated_qemu_observation(
    observation_path: Path,
    qemu_out_path: Path,
    target_session_path: Path,
    session: Mapping[str, Any],
    session_raw: bytes,
) -> tuple[dict[str, str], dict[str, bytes]]:
    """Bind service injection to one prior authenticated exact-artifact PASS."""

    qemu_out = _resolved_directory(qemu_out_path, "authenticated QEMU output")
    launch_artifacts = _validate_qemu_launch_artifacts(qemu_out)
    if (
        _sha256(launch_artifacts["kernel"]) != session["kernel_sha256"]
        or _sha256(launch_artifacts["rootserver"])
        != session["root_image_sha256"]
    ):
        raise EvidenceError(
            "authenticated QEMU launch bytes differ from the emitted target session"
        )

    observation, observation_raw = _load_frozen_json(
        observation_path, "authenticated QEMU target observation"
    )
    _exact_keys(
        observation,
        {
            "schema",
            "banner",
            "claiming",
            "result",
            "first_failing_proof_layer",
            "detail",
            "target",
            "focus",
            "run_id",
            "profile",
            "serial_log",
            "serial_source_log",
            "built_image",
            "image_identity",
            "operation_script",
        },
        context="authenticated QEMU target observation",
    )
    if (
        observation["schema"] != QEMU_AUTH_OBSERVATION_SCHEMA
        or observation["banner"] != "NON-CLAIMING TARGET DIAGNOSTIC"
        or observation["claiming"] is not False
        or observation["result"] != "PASS"
        or observation["first_failing_proof_layer"] is not None
        or observation["target"] != "qemu"
        or observation["focus"] != "ninedoor"
        or observation["profile"] != QEMU_AUTH_OBSERVATION_PROFILE
        or not isinstance(observation["run_id"], str)
        or not observation["run_id"]
        or not isinstance(observation["detail"], str)
        or not observation["detail"]
    ):
        raise EvidenceError(
            "service injection requires a prior exact ninedoor QEMU PASS observation"
        )

    launch_record = qemu_out / "cohesix-qemu-launch-artifacts.json"
    initrd = qemu_out / "cohesix-system.cpio"
    _built_path, built_raw = _observation_file(
        observation["built_image"], "built image", expected=initrd
    )
    _identity_path, launch_record_raw = _observation_file(
        observation["image_identity"],
        "launch identity",
        expected=launch_record,
    )
    if built_raw != launch_artifacts["initrd"]:
        raise EvidenceError("authenticated QEMU image differs from launch record bytes")
    serial_path, serial_raw = _observation_file(
        observation["serial_log"], "UART transcript"
    )
    if observation["serial_source_log"] != str(serial_path):
        raise EvidenceError("authenticated QEMU UART source aliases its frozen transcript")
    serial_text = _artifact_text(serial_raw, "authenticated QEMU UART transcript")
    if QEMU_AUTH_UART_MARKER not in serial_text:
        raise EvidenceError("prior QEMU PASS lacks a live authenticated cohsh session")
    operation_path, _operation_raw = _observation_file(
        observation["operation_script"], "operation script"
    )
    if operation_path.parts[-3:] != ("scripts", "cohsh", "9p_batch.coh"):
        raise EvidenceError("prior QEMU PASS did not use the canonical NineDoor operation")

    _validate_emitted_target_session_bundle(
        target_session_path, session, session_raw
    )
    return (
        {
            "observation_sha256": _sha256(observation_raw),
            "observation_bytes": str(len(observation_raw)),
            "serial_sha256": _sha256(serial_raw),
            "serial_bytes": str(len(serial_raw)),
            "launch_record_sha256": _sha256(launch_record_raw),
            "launch_record_bytes": str(len(launch_record_raw)),
            "target_session_sha256": _sha256(session_raw),
            "target_session_bytes": str(len(session_raw)),
        },
        {
            "authenticated-qemu-observation": observation_raw,
            "authenticated-qemu-uart": serial_raw,
            "authenticated-qemu-launch-record": launch_record_raw,
            "authenticated-qemu-system-cpio": built_raw,
        },
    )


def _gdb_file_argument(path: Path) -> str:
    value = str(path)
    if not value or any(ord(character) < 0x20 for character in value) or '"' in value:
        raise EvidenceError("ELF path cannot be represented safely in a GDB command file")
    return f'"{value}"'


def _qemu_service_gdb(args: argparse.Namespace) -> None:
    if args.mode not in QEMU_SERVICE_MODES[args.service]:
        raise EvidenceError(
            f"{args.service} does not support qemu-service-gdb mode {args.mode}"
        )
    between_calls = (
        args.service == "ninedoor-service" and args.mode == "between-calls-revoke"
    )
    if between_calls != (args.root_elf is not None):
        raise EvidenceError(
            "--root-elf is required only for ninedoor-service between-calls-revoke"
        )
    gdb, nm = _validate_remote_and_tools(
        args.remote, args.timeout_secs, args.gdb, args.nm
    )
    session_raw = _read_frozen_artifact(args.target_session, "target session")
    generated_raw = _read_frozen_artifact(args.generated_inventory, "generated topology")
    service_raw = _read_frozen_artifact(args.service_elf, f"{args.service} ELF")
    try:
        session = json.loads(session_raw)
        generated = json.loads(generated_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("service GDB target session/topology is invalid JSON") from exc
    _target_session(session, "qemu")
    _generated_inventory(generated, "qemu", session)
    auth, _auth_artifacts = _validate_authenticated_qemu_observation(
        args.auth_observation,
        args.qemu_out,
        args.target_session,
        session,
        session_raw,
    )
    root_raw: bytes | None = None
    if between_calls:
        root_elf = args.root_elf
        if root_elf is None:  # pragma: no cover - paired-mode guard above
            raise EvidenceError("between-Calls revoke requires the root-task ELF")
        root_raw = _read_frozen_artifact(root_elf, "unstripped root-task ELF")
        if _sha256(root_raw) != session["root_image_sha256"]:
            raise EvidenceError(
                "between-Calls revoke root ELF differs from target-session root image"
            )
        symbols = QEMU_NINEDOOR_ROOT_SYMBOLS
        addresses = _rust_symbol_addresses(
            nm,
            root_elf,
            symbols,
            QEMU_NINEDOOR_ROOT_MODULE,
            "root-task NineDoor evidence",
        )
        post_prepare = addresses[symbols[0]]
        request_revoke = addresses[symbols[1]]
        command_file = root_elf
        command_body = f"""set $m26e_ninedoor_prepare_hits = 0
hbreak *0x{post_prepare:x}
commands 1
  silent
  set $m26e_ninedoor_prepare_hits = $m26e_ninedoor_prepare_hits + 1
  if $m26e_ninedoor_prepare_hits == 1
    continue
  end
  if $m26e_ninedoor_prepare_hits == 2
    printf "M26E_GDB_SERVICE_INJECTION service=ninedoor-service phase=between-calls symbol={symbols[0]} action=redirect-local-revoke result=continued\\n"
    set $pc = 0x{request_revoke:x}
    disable 1
    detach
    quit
  end
  printf "M26E_GDB_ABORT reason=unexpected-ninedoor-prepare-hit result=failed\\n"
  detach
  quit
end
"""
        expected = {
            (
                "between-calls",
                symbols[0],
                "redirect-local-revoke",
                "continued",
            )
        }
    else:
        symbols = QEMU_SERVICE_SYMBOLS[args.service]
        addresses = _symbol_addresses(nm, args.service_elf, symbols, args.service)
        handler = addresses[symbols[0]]
        standard = addresses[symbols[1]]
        command_file = args.service_elf

    if args.mode == "during-call-standard":
        command_body = f"""hbreak *0x{handler:x}
commands 1
  silent
  printf "M26E_GDB_SERVICE_INJECTION service={args.service} phase=during-call symbol={symbols[0]} action=redirect-standard-fault result=continued\\n"
  set $pc = 0x{standard:x}
  disable 1
  detach
  quit
end
"""
        expected = {
            (
                "during-call",
                symbols[0],
                "redirect-standard-fault",
                "continued",
            )
        }
    elif args.mode == "budget-exhaustion-timeout":
        timeout_spin = addresses[symbols[2]]
        command_body = f"""hbreak *0x{handler:x}
commands 1
  silent
  printf "M26E_GDB_SERVICE_INJECTION service=console-network phase=budget-exhaustion symbol={symbols[0]} action=redirect-timeout-spin result=continued\\n"
  set $pc = 0x{timeout_spin:x}
  disable 1
  detach
  quit
end
"""
        expected = {
            (
                "budget-exhaustion",
                symbols[0],
                "redirect-timeout-spin",
                "continued",
            ),
        }
    command_text = f"""set pagination off
set confirm off
set architecture aarch64
file {_gdb_file_argument(command_file)}
target remote {args.remote}
delete breakpoints
{command_body}continue
"""
    completed = _run_gdb_batch(
        gdb, command_text, args.timeout_secs, "cohesix-m26e-service-gdb-"
    )
    header = [
        _qemu_session_header(session, generated),
        "M26E_QEMU_AUTH "
        f"result=PASS observation_sha256={auth['observation_sha256']} "
        f"observation_bytes={auth['observation_bytes']} "
        f"serial_sha256={auth['serial_sha256']} "
        f"serial_bytes={auth['serial_bytes']} "
        f"launch_record_sha256={auth['launch_record_sha256']} "
        f"launch_record_bytes={auth['launch_record_bytes']} "
        f"target_session_sha256={auth['target_session_sha256']} "
        f"target_session_bytes={auth['target_session_bytes']}",
        "M26E_GDB_SERVICE_ELF "
        f"service={args.service} mode={args.mode} "
        f"elf_sha256={_sha256(service_raw)} "
        f"elf_bytes={len(service_raw)} "
        f"root_image_sha256={session['root_image_sha256']}",
    ]
    if root_raw is not None:
        header.append(
            "M26E_GDB_SERVICE_ROOT_ELF "
            f"service={args.service} mode={args.mode} "
            f"elf_sha256={_sha256(root_raw)} "
            f"elf_bytes={len(root_raw)} "
            f"root_image_sha256={session['root_image_sha256']}"
        )
    transcript = (
        ("\n".join(header) + "\n").encode("utf-8")
        + completed.stdout
        + completed.stderr
    )
    if completed.returncode != 0:
        raise EvidenceError(
            "QEMU service GDB injection failed; no acceptance transcript was published"
        )
    text = _artifact_text(transcript, "service GDB transcript")
    if "M26E_GDB_ABORT" in text:
        raise EvidenceError("QEMU service GDB injection hit an unexpected turn")
    rows = _marker_rows(
        text,
        "M26E_GDB_SERVICE_INJECTION",
        {"service", "phase", "symbol", "action", "result"},
    )
    observed = {
        (row["phase"], row["symbol"], row["action"], row["result"])
        for row in rows
        if row["service"] == args.service
    }
    if observed != expected or len(rows) != len(expected):
        raise EvidenceError("QEMU service GDB runner did not complete its exact plan")
    _write_frozen_output(args.out, transcript)
    print(
        f"worker evidence: qemu {args.service}/{args.mode} "
        f"GDB injection PASS ({args.out})"
    )


def _qemu_critical_gdb(args: argparse.Namespace) -> None:
    gdb, nm = _validate_remote_and_tools(
        args.remote, args.timeout_secs, args.gdb, args.nm
    )
    session_raw = _read_frozen_artifact(args.target_session, "target session")
    generated_raw = _read_frozen_artifact(args.generated_inventory, "generated topology")
    root_raw = _read_frozen_artifact(args.root_elf, "unstripped root-task ELF")
    try:
        session = json.loads(session_raw)
        generated = json.loads(generated_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("critical GDB target session/topology is invalid JSON") from exc
    _target_session(session, "qemu")
    _generated_inventory(generated, "qemu", session)
    addresses = _symbol_addresses(nm, args.root_elf, QEMU_CRITICAL_SYMBOLS, "root-task")
    command_lines = [
        "set pagination off",
        "set confirm off",
        "set architecture aarch64",
        f"file {_gdb_file_argument(args.root_elf)}",
        f"target remote {args.remote}",
        "delete breakpoints",
        "set $m26e_critical_seen = 0",
    ]
    for number, symbol in enumerate(QEMU_CRITICAL_SYMBOLS, start=1):
        duty = QEMU_CRITICAL_DUTIES[symbol]
        command_lines.extend(
            (
                f"hbreak *0x{addresses[symbol]:x}",
                f"commands {number}",
                "  silent",
                "  set $m26e_critical_seen = $m26e_critical_seen + 1",
                "  printf \"M26E_GDB_CRITICAL_OBSERVATION "
                f"duty={duty} symbol={symbol} result=observed\\n\"",
                f"  disable {number}",
                "  if $m26e_critical_seen == 4",
                "    detach",
                "    quit",
                "  end",
                "  continue",
                "end",
            )
        )
    command_lines.append("continue")
    completed = _run_gdb_batch(
        gdb,
        "\n".join(command_lines) + "\n",
        args.timeout_secs,
        "cohesix-m26e-critical-gdb-",
    )
    header = [
        _qemu_session_header(session, generated),
        "M26E_GDB_ROOT_ELF "
        f"elf_sha256={_sha256(root_raw)} "
        f"root_image_sha256={session['root_image_sha256']}",
    ]
    transcript = (
        ("\n".join(header) + "\n").encode("utf-8")
        + completed.stdout
        + completed.stderr
    )
    if completed.returncode != 0:
        raise EvidenceError(
            "QEMU critical-lane GDB observation failed; no transcript was published"
        )
    text = _artifact_text(transcript, "critical GDB transcript")
    rows = _marker_rows(
        text,
        "M26E_GDB_CRITICAL_OBSERVATION",
        {"duty", "symbol", "result"},
    )
    observed = {(row["duty"], row["symbol"], row["result"]) for row in rows}
    expected = {
        (duty, symbol, "observed")
        for symbol, duty in QEMU_CRITICAL_DUTIES.items()
    }
    if observed != expected or len(rows) != len(expected):
        raise EvidenceError("QEMU critical GDB runner omitted a root duty lane")
    _write_frozen_output(args.out, transcript)
    print(f"worker evidence: qemu critical duties PASS ({args.out})")


def _collect_qemu_preflight(args: argparse.Namespace) -> None:
    if args.out_dir.exists() and any(args.out_dir.iterdir()):
        raise EvidenceError("QEMU preflight output directory must be absent or empty")
    session_raw = _read_frozen_artifact(args.target_session, "target session")
    generated_raw = _read_frozen_artifact(args.generated_inventory, "generated topology")
    try:
        session = json.loads(session_raw)
        generated = json.loads(generated_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("preflight session/topology is invalid JSON") from exc
    _target_session(session, "qemu")
    topology, generated_inventory = _generated_inventory(generated, "qemu", session)
    _auth, auth_artifacts = _validate_authenticated_qemu_observation(
        args.auth_observation,
        args.qemu_out,
        args.target_session,
        session,
        session_raw,
    )
    uart_raw = _read_frozen_artifact(args.uart, "preflight UART transcript")
    cohsh_raw = _read_frozen_artifact(args.cohsh, "preflight cohsh transcript")
    gdb_artifacts = [
        _read_frozen_artifact(path, f"preflight GDB transcript {index + 1}")
        for index, path in enumerate(args.gdb_log)
    ]
    service_gdb_artifacts = [
        _read_frozen_artifact(path, f"service GDB transcript {index + 1}")
        for index, path in enumerate(args.service_gdb_log)
    ]
    service_uart_artifacts = [
        _read_frozen_artifact(path, f"service UART transcript {index + 1}")
        for index, path in enumerate(args.service_uart)
    ]
    critical_gdb_raw = _read_frozen_artifact(
        args.critical_gdb_log, "critical GDB transcript"
    )
    archive_raw = _read_frozen_artifact(args.worker_archive, "Worker archive")
    driver_archive_raw = _read_frozen_artifact(
        args.driver_archive, "external canonical driver archive"
    )
    manifest_raw = _read_frozen_artifact(
        args.worker_image_manifest, "Worker image manifest"
    )
    uart = _artifact_text(uart_raw, "preflight UART transcript")
    cohsh = _artifact_text(cohsh_raw, "preflight cohsh transcript")
    gdb_texts = [
        _artifact_text(raw, f"preflight GDB transcript {index + 1}")
        for index, raw in enumerate(gdb_artifacts)
    ]
    service_gdb_texts = [
        _artifact_text(raw, f"service GDB transcript {index + 1}")
        for index, raw in enumerate(service_gdb_artifacts)
    ]
    service_uart_texts = [
        _artifact_text(raw, f"service UART transcript {index + 1}")
        for index, raw in enumerate(service_uart_artifacts)
    ]
    critical_gdb_text = _artifact_text(critical_gdb_raw, "critical GDB transcript")
    elf_paths = _parse_worker_elfs(args.worker_elf)
    elf_raw = {
        role: _read_frozen_artifact(path, f"{role} unstripped ELF")
        for role, path in elf_paths.items()
    }
    service_paths = _parse_service_elfs(args.service_elf)
    service_raw = {
        service: _read_frozen_artifact(path, f"{service} unstripped ELF")
        for service, path in service_paths.items()
    }
    root_raw = _read_frozen_artifact(args.root_elf, "unstripped root-task ELF")
    image_hashes = _validate_worker_build_artifacts(
        session, archive_raw, manifest_raw, elf_raw
    )
    _validate_driver_archive(session, driver_archive_raw)
    markers = _parse_live_worker_markers(uart)
    admissions, matrix, fault_roles, sequential_roles, fault_phase_roles = (
        _validate_marker_lifecycle(markers, topology)
    )
    expected_matrix = {
        (action, outcome)
        for action in QEMU_RECEIPT_ACTIONS
        for outcome in QEMU_TERMINAL_OUTCOMES
    }
    if (
        matrix != expected_matrix
        or fault_roles != set(REQUIRED_ROLES)
        or sequential_roles != set(REQUIRED_ROLES)
        or fault_phase_roles
        != {
            (role, phase)
            for role in REQUIRED_ROLES
            for phase in ("pre-ready", "during-ipc", "budget-exhaustion")
        }
    ):
        raise EvidenceError(
            "preflight lacks the full receipt/fault/sequential-recreation observations"
        )
    if any(
        observation["image_sha256"] != image_hashes[identity[0]]
        for identity, observation in admissions.items()
    ):
        raise EvidenceError("preflight UART image hashes differ from packaged Worker images")
    injection_roles = {
        _validate_gdb_markers(
            gdb,
            session,
            generated,
            elf_raw,
            admissions,
            fault_phase_roles,
        )
        for gdb in gdb_texts
    }
    if injection_roles != set(REQUIRED_ROLES) or len(gdb_texts) != 3:
        raise EvidenceError("preflight requires one exact GDB injection transcript per role")
    _validate_service_gdb_markers(
        service_gdb_texts,
        session,
        session_raw,
        generated,
        service_raw,
        root_raw,
    )
    _validate_service_uart_markers(service_uart_texts)
    _validate_critical_gdb_markers(
        critical_gdb_text, session, generated, root_raw
    )
    _validate_root_markers(uart, generated_inventory)
    _validate_cohsh_transcript(cohsh)
    workers = _live_workers_from_uart(markers, admissions, topology)
    raw_evidence = [
        _artifact_row("preflight-uart-transcript", uart_raw),
        _artifact_row("preflight-cohsh-transcript", cohsh_raw),
        *(
            _artifact_row(f"preflight-gdb-transcript-{index + 1}", raw)
            for index, raw in enumerate(gdb_artifacts)
        ),
        _artifact_row("worker-image-archive", archive_raw),
        _artifact_row("driver-runtime-archive", driver_archive_raw),
        _artifact_row("worker-image-manifest", manifest_raw),
        *(
            _artifact_row(identifier, raw)
            for identifier, raw in auth_artifacts.items()
        ),
        *(
            _artifact_row(f"{role}-unstripped-elf", raw)
            for role, raw in elf_raw.items()
        ),
        *(
            _artifact_row(f"{service}-unstripped-elf", raw)
            for service, raw in service_raw.items()
        ),
        *(
            _artifact_row(f"service-uart-transcript-{index + 1}", raw)
            for index, raw in enumerate(service_uart_artifacts)
        ),
        *(
            _artifact_row(f"service-gdb-transcript-{index + 1}", raw)
            for index, raw in enumerate(service_gdb_artifacts)
        ),
        _artifact_row("critical-gdb-transcript", critical_gdb_raw),
        _artifact_row("root-task-unstripped-elf", root_raw),
    ]
    raw_evidence.sort(key=lambda row: row["id"])
    observations = {
        "schema": COMPONENT_OBSERVATIONS_SCHEMA,
        "target": "qemu",
        "target_session_sha256": _sha256(session_raw),
        "workers": workers,
        "outcomes": _pass_outcomes(COMPONENT_REQUIRED_OUTCOMES),
        "raw_evidence": raw_evidence,
        "verdict": "PASS",
        "blockers": [],
    }
    args.out_dir.mkdir(parents=True, exist_ok=True)
    observations_path = args.out_dir / "component-observations.json"
    _write(observations_path, observations)
    component_path = args.out_dir / "worker-task-evidence.json"
    _emit_component(
        argparse.Namespace(
            target="qemu",
            target_session=args.target_session,
            generated_inventory=args.generated_inventory,
            observations=observations_path,
            integration_dir=args.integration_dir,
            out=component_path,
        )
    )
    print(f"worker evidence: qemu live preflight PASS ({component_path})")


def _collect_qemu(args: argparse.Namespace) -> None:
    if args.out_dir.exists() and any(args.out_dir.iterdir()):
        raise EvidenceError("QEMU collection output directory must be absent or empty")
    session_raw = _read_frozen_artifact(args.target_session, "target session")
    try:
        session = json.loads(session_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("target session is invalid JSON") from exc
    _target_session(session, "qemu")
    generated_raw = _read_frozen_artifact(args.generated_inventory, "generated topology")
    try:
        generated = json.loads(generated_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise EvidenceError("generated topology is invalid JSON") from exc
    topology, generated_inventory = _generated_inventory(generated, "qemu", session)
    _auth, auth_artifacts = _validate_authenticated_qemu_observation(
        args.auth_observation,
        args.qemu_out,
        args.target_session,
        session,
        session_raw,
    )

    if len(args.uart) != len(args.pressure) or len(args.gdb_log) != len(args.pressure):
        raise EvidenceError(
            "collect-qemu requires one --uart and --gdb-log per --pressure, in order"
        )
    uart_artifacts = [
        _read_frozen_artifact(path, f"UART transcript {index + 1}")
        for index, path in enumerate(args.uart)
    ]
    cohsh_raw = _read_frozen_artifact(args.cohsh, "cohsh transcript")
    gdb_artifacts = [
        _read_frozen_artifact(path, f"GDB transcript {index + 1}")
        for index, path in enumerate(args.gdb_log)
    ]
    preflight_uart_raw = _read_frozen_artifact(
        args.preflight_uart, "preflight UART transcript"
    )
    preflight_gdb_artifacts = [
        _read_frozen_artifact(path, f"preflight GDB transcript {index + 1}")
        for index, path in enumerate(args.preflight_gdb_log)
    ]
    preflight_service_gdb_artifacts = [
        _read_frozen_artifact(path, f"preflight service GDB transcript {index + 1}")
        for index, path in enumerate(args.preflight_service_gdb_log)
    ]
    preflight_service_uart_artifacts = [
        _read_frozen_artifact(path, f"preflight service UART transcript {index + 1}")
        for index, path in enumerate(args.preflight_service_uart)
    ]
    preflight_critical_gdb_raw = _read_frozen_artifact(
        args.preflight_critical_gdb_log, "preflight critical GDB transcript"
    )
    uart_texts = [
        _artifact_text(raw, f"UART transcript {index + 1}")
        for index, raw in enumerate(uart_artifacts)
    ]
    cohsh = _artifact_text(cohsh_raw, "cohsh transcript")
    gdb_texts = [
        _artifact_text(raw, f"GDB transcript {index + 1}")
        for index, raw in enumerate(gdb_artifacts)
    ]
    preflight_uart = _artifact_text(
        preflight_uart_raw, "preflight UART transcript"
    )
    preflight_gdb_texts = [
        _artifact_text(raw, f"preflight GDB transcript {index + 1}")
        for index, raw in enumerate(preflight_gdb_artifacts)
    ]
    preflight_service_gdb_texts = [
        _artifact_text(raw, f"preflight service GDB transcript {index + 1}")
        for index, raw in enumerate(preflight_service_gdb_artifacts)
    ]
    preflight_service_uart_texts = [
        _artifact_text(raw, f"preflight service UART transcript {index + 1}")
        for index, raw in enumerate(preflight_service_uart_artifacts)
    ]
    preflight_critical_gdb_text = _artifact_text(
        preflight_critical_gdb_raw, "preflight critical GDB transcript"
    )
    elf_paths = _parse_worker_elfs(args.worker_elf)
    elf_raw = {
        role: _read_frozen_artifact(path, f"{role} unstripped ELF")
        for role, path in elf_paths.items()
    }
    service_paths = _parse_service_elfs(args.service_elf)
    service_raw = {
        service: _read_frozen_artifact(path, f"{service} unstripped ELF")
        for service, path in service_paths.items()
    }
    root_raw = _read_frozen_artifact(args.root_elf, "unstripped root-task ELF")
    archive_raw = _read_frozen_artifact(args.worker_archive, "Worker archive")
    driver_archive_raw = _read_frozen_artifact(
        args.driver_archive, "external canonical driver archive"
    )
    manifest_raw = _read_frozen_artifact(
        args.worker_image_manifest, "Worker image manifest"
    )
    image_hashes = _validate_worker_build_artifacts(
        session, archive_raw, manifest_raw, elf_raw
    )
    _validate_driver_archive(session, driver_archive_raw)
    marker_sets: list[dict[str, list[dict[str, Any]]]] = []
    admission_sets: list[dict[tuple[str, int, int, int, int], dict[str, Any]]] = []
    observed_matrix: set[tuple[int, int]] = set()
    fault_roles: set[str] = set()
    sequential_roles: set[str] = set()
    fault_phase_roles: set[tuple[str, str]] = set()
    preflight_markers = _parse_live_worker_markers(preflight_uart)
    (
        preflight_admissions,
        preflight_matrix,
        preflight_fault_roles,
        preflight_sequential_roles,
        preflight_fault_phase_roles,
    ) = _validate_marker_lifecycle(preflight_markers, topology)
    if any(
        observation["image_sha256"] != image_hashes[identity[0]]
        for identity, observation in preflight_admissions.items()
    ):
        raise EvidenceError("preflight UART image hashes differ from packaged images")
    preflight_injection_roles = {
        _validate_gdb_markers(
            text,
            session,
            generated,
            elf_raw,
            preflight_admissions,
            preflight_fault_phase_roles,
        )
        for text in preflight_gdb_texts
    }
    if (
        len(preflight_gdb_texts) != 3
        or preflight_injection_roles != set(REQUIRED_ROLES)
    ):
        raise EvidenceError("final collection requires one preflight GDB log per role")
    _validate_service_gdb_markers(
        preflight_service_gdb_texts,
        session,
        session_raw,
        generated,
        service_raw,
        root_raw,
    )
    _validate_service_uart_markers(preflight_service_uart_texts)
    _validate_critical_gdb_markers(
        preflight_critical_gdb_text, session, generated, root_raw
    )
    _validate_root_markers(preflight_uart, generated_inventory)
    observed_matrix.update(preflight_matrix)
    fault_roles.update(preflight_fault_roles)
    sequential_roles.update(preflight_sequential_roles)
    fault_phase_roles.update(preflight_fault_phase_roles)

    for uart_text, gdb_text in zip(uart_texts, gdb_texts, strict=True):
        markers = _parse_live_worker_markers(uart_text)
        (
            admissions,
            matrix,
            boot_fault_roles,
            boot_sequential_roles,
            boot_fault_phase_roles,
        ) = _validate_marker_lifecycle(markers, topology)
        if any(
            observation["image_sha256"] != image_hashes[identity[0]]
            for identity, observation in admissions.items()
        ):
            raise EvidenceError("UART image hashes differ from packaged Worker images")
        _validate_gdb_markers(
            gdb_text,
            session,
            generated,
            elf_raw,
            admissions,
            boot_fault_phase_roles,
        )
        marker_sets.append(markers)
        admission_sets.append(admissions)
        observed_matrix.update(matrix)
        fault_roles.update(boot_fault_roles)
        sequential_roles.update(boot_sequential_roles)
        fault_phase_roles.update(boot_fault_phase_roles)
    expected_matrix = {
        (action, outcome)
        for action in QEMU_RECEIPT_ACTIONS
        for outcome in QEMU_TERMINAL_OUTCOMES
    }
    if observed_matrix != expected_matrix:
        raise EvidenceError("UART inputs lack the exact canonical seven-action receipt matrix")
    if fault_roles != set(REQUIRED_ROLES):
        raise EvidenceError("UART inputs omit live fault containment for an executable role")
    if sequential_roles != set(REQUIRED_ROLES):
        raise EvidenceError("UART inputs omit same-role sequential recreation")
    if fault_phase_roles != {
        (role, phase)
        for role in REQUIRED_ROLES
        for phase in ("pre-ready", "during-ipc", "budget-exhaustion")
    }:
        raise EvidenceError("UART inputs omit a required live fault-injection phase")
    _validate_cohsh_transcript(cohsh)
    reports, pressure_artifacts = _pressure_reports(
        args.pressure,
        session,
        session_raw,
        generated,
        topology,
        uart_artifacts,
        gdb_artifacts,
    )
    for report, markers in zip(reports, marker_sets, strict=True):
        _validate_pressure_cycles_and_receipts([report], markers)
    workers = _final_workers_from_pressure(
        reports,
        marker_sets[-1],
        admission_sets[-1],
        topology,
    )

    raw_evidence = [
        _artifact_row("cohsh-transcript", cohsh_raw),
        _artifact_row("preflight-uart-transcript", preflight_uart_raw),
        *(
            _artifact_row(f"preflight-gdb-transcript-{index + 1}", raw)
            for index, raw in enumerate(preflight_gdb_artifacts)
        ),
        *(
            _artifact_row(f"preflight-service-gdb-transcript-{index + 1}", raw)
            for index, raw in enumerate(preflight_service_gdb_artifacts)
        ),
        *(
            _artifact_row(f"preflight-service-uart-transcript-{index + 1}", raw)
            for index, raw in enumerate(preflight_service_uart_artifacts)
        ),
        _artifact_row("preflight-critical-gdb-transcript", preflight_critical_gdb_raw),
        _artifact_row("worker-image-archive", archive_raw),
        _artifact_row("driver-runtime-archive", driver_archive_raw),
        _artifact_row("worker-image-manifest", manifest_raw),
        *(
            _artifact_row(identifier, raw)
            for identifier, raw in auth_artifacts.items()
        ),
        *(
            _artifact_row(f"uart-transcript-{index + 1}", raw)
            for index, raw in enumerate(uart_artifacts)
        ),
        *(
            _artifact_row(f"gdb-transcript-{index + 1}", raw)
            for index, raw in enumerate(gdb_artifacts)
        ),
        *(
            _artifact_row(f"{role}-unstripped-elf", raw)
            for role, raw in elf_raw.items()
        ),
        *(
            _artifact_row(f"{service}-unstripped-elf", raw)
            for service, raw in service_raw.items()
        ),
        _artifact_row("root-task-unstripped-elf", root_raw),
        *(_artifact_row(identifier, raw) for identifier, raw in pressure_artifacts),
    ]
    raw_evidence.sort(key=lambda row: row["id"])
    component_observations = {
        "schema": COMPONENT_OBSERVATIONS_SCHEMA,
        "target": "qemu",
        "target_session_sha256": _sha256(session_raw),
        "workers": workers,
        "outcomes": _pass_outcomes(COMPONENT_REQUIRED_OUTCOMES),
        "raw_evidence": raw_evidence,
        "verdict": "PASS",
        "blockers": [],
    }
    args.out_dir.mkdir(parents=True, exist_ok=True)
    component_observations_path = args.out_dir / "component-observations.json"
    _write(component_observations_path, component_observations)
    worker_path = args.out_dir / "worker-task-evidence.json"
    _emit_component(
        argparse.Namespace(
            target="qemu",
            target_session=args.target_session,
            generated_inventory=args.generated_inventory,
            observations=component_observations_path,
            integration_dir=args.integration_dir,
            out=worker_path,
        )
    )

    root_observations = {
        "schema": ROOT_OBSERVATIONS_SCHEMA,
        "target": "qemu",
        "target_session_sha256": _sha256(session_raw),
        "topology_sha256": generated["topology_sha256"],
        "inventory_scope": "admitted-maximum",
        "observed_inventory": generated_inventory,
        "outcomes": _pass_outcomes(ROOT_REQUIRED_OUTCOMES),
        "raw_evidence": raw_evidence,
        "verdict": "PASS",
        "blockers": [],
    }
    root_observations_path = args.out_dir / "root-observations.json"
    _write(root_observations_path, root_observations)
    root_path = args.out_dir / "root-tcb-acceptance.json"
    _emit_root(
        argparse.Namespace(
            target="qemu",
            target_session=args.target_session,
            worker=worker_path,
            generated_inventory=args.generated_inventory,
            observations=root_observations_path,
            out=root_path,
        )
    )
    worker, worker_raw = _load(worker_path)
    root, root_raw = _load(root_path)
    system_input = {
        "schema": SYSTEM_INPUT_SCHEMA,
        "target": "qemu",
        "target_session": session,
        "worker_component_sha256": _sha256(worker_raw),
        "root_tcb_sha256": _sha256(root_raw),
        "topology_sha256": generated["topology_sha256"],
        "core_admission": _core_admission_from_topology(topology),
        "outcomes": _pass_outcomes(SYSTEM_REQUIRED_OUTCOMES),
        "raw_evidence": raw_evidence,
        "verdict": "PASS",
        "blockers": [],
    }
    system_input_path = args.out_dir / "system-observations.json"
    _write(system_input_path, system_input)
    system = _system_from_run(
        "qemu",
        worker,
        worker_raw,
        root,
        root_raw,
        args.run_dir,
        system_input_path,
    )
    system_path = args.out_dir / "system-acceptance.json"
    _write(system_path, system)
    print(f"worker evidence: qemu live collection PASS ({args.out_dir})")


def _input_artifact(identifier: str, raw: bytes) -> dict[str, Any]:
    return {"id": identifier, "sha256": _sha256(raw), "bytes": len(raw)}


def _merge_input_artifacts(
    artifacts: Any,
    context: str,
    *bound_inputs: tuple[str, bytes],
) -> list[dict[str, Any]]:
    merged = list(_artifacts(artifacts, context, required=True))
    merged.extend(_input_artifact(identifier, raw) for identifier, raw in bound_inputs)
    merged.sort(key=lambda item: (item["id"], item["sha256"], item["bytes"]))
    _artifacts(merged, context, required=True)
    identifiers = [item["id"] for item in merged]
    if len(identifiers) != len(set(identifiers)):
        raise EvidenceError(f"{context} ids must be unique")
    return merged


def _emit_component(args: argparse.Namespace) -> None:
    session, session_raw = _target_session_file(args.target_session, args.target)
    generated, generated_raw = _load(args.generated_inventory)
    topology, _ = _generated_inventory(generated, args.target, session)
    observations, observations_raw = _load(args.observations)
    _exact_keys(
        observations,
        {
            "schema",
            "target",
            "target_session_sha256",
            "workers",
            "outcomes",
            "raw_evidence",
            "verdict",
            "blockers",
        },
        context="Worker component observations",
    )
    if (
        observations["schema"] != COMPONENT_OBSERVATIONS_SCHEMA
        or observations["target"] != args.target
        or observations["target_session_sha256"] != _sha256(session_raw)
    ):
        raise EvidenceError("Worker observations do not bind the exact target session")
    if observations["verdict"] != "PASS" or observations["blockers"]:
        raise EvidenceError("target-component emission requires complete PASS observations")
    _validate_worker_topology(observations["workers"], topology)

    references: list[dict[str, Any]] = []
    integration_inputs: list[tuple[str, bytes]] = []
    for dependency_id in REQUIRED_INTEGRATIONS:
        path = args.integration_dir / f"{dependency_id}.json"
        integration, raw = _load(path)
        validate_integration(integration, args.target)
        if (
            integration["dependency_id"] != dependency_id
            or integration["verdict"] != "PASS"
            or integration["target_session"] != session
            or integration["manifest_sha256"] != session["manifest_sha256"]
        ):
            raise EvidenceError(
                f"integration does not bind the accepted target session: {path}"
            )
        references.append(
            {
                "id": dependency_id,
                "record_kind": "worker-integration",
                "sha256": _sha256(raw),
            }
        )
        integration_inputs.append((dependency_id, raw))

    record = {
        "schema": COMPONENT_SCHEMA,
        "record_kind": "target-component",
        "target": args.target,
        "target_session": session,
        "topology_sha256": generated["topology_sha256"],
        "workers": observations["workers"],
        "integration_evidence": references,
        "outcomes": observations["outcomes"],
        "raw_evidence": _merge_input_artifacts(
            observations["raw_evidence"],
            "component raw evidence",
            ("component-observations-input", observations_raw),
            ("generated-inventory-input", generated_raw),
            ("target-session-input", session_raw),
        ),
        "verdict": "PASS",
        "blockers": [],
    }
    validate_component(record, args.target)
    for dependency_id, raw in integration_inputs:
        _write_raw(args.out.parent / "integration" / f"{dependency_id}.json", raw)
    _write(args.out, record)
    print(f"worker evidence: {args.target} target-component PASS ({args.out})")


def _emit_root(args: argparse.Namespace) -> None:
    session, session_raw = _target_session_file(args.target_session, args.target)
    worker, worker_raw = _load(args.worker)
    validate_component(worker, args.target)
    if worker["verdict"] != "PASS" or worker["target_session"] != session:
        raise EvidenceError("root-TCB emission requires matching accepted Worker evidence")

    generated, generated_raw = _load(args.generated_inventory)
    _, generated_inventory = _generated_inventory(generated, args.target, session)
    observations, observations_raw = _load(args.observations)
    _exact_keys(
        observations,
        {
            "schema",
            "target",
            "target_session_sha256",
            "topology_sha256",
            "inventory_scope",
            "observed_inventory",
            "outcomes",
            "raw_evidence",
            "verdict",
            "blockers",
        },
        context="root-TCB observations",
    )
    session_sha256 = _sha256(session_raw)
    if (
        observations["schema"] != ROOT_OBSERVATIONS_SCHEMA
        or observations["target"] != args.target
        or observations["target_session_sha256"] != session_sha256
        or generated["topology_sha256"] != observations["topology_sha256"]
        or worker["topology_sha256"] != generated["topology_sha256"]
    ):
        raise EvidenceError("root-TCB inputs do not bind one exact target topology/session")
    if observations["inventory_scope"] != "admitted-maximum":
        raise EvidenceError("root-TCB inventory scope is not admitted-maximum")
    if observations["verdict"] != "PASS" or observations["blockers"]:
        raise EvidenceError("root-TCB emission requires complete PASS observations")

    record = {
        "schema": ROOT_SCHEMA,
        "record_kind": "root-tcb",
        "target": args.target,
        "target_session": session,
        "worker_component": {
            "id": f"worker-component-{args.target}",
            "record_kind": "target-component",
            "sha256": _sha256(worker_raw),
        },
        "topology_sha256": generated["topology_sha256"],
        "generated_inventory": generated_inventory,
        "inventory_scope": observations["inventory_scope"],
        "observed_inventory": observations["observed_inventory"],
        "outcomes": observations["outcomes"],
        "raw_evidence": _merge_input_artifacts(
            observations["raw_evidence"],
            "root raw evidence",
            ("generated-inventory-input", generated_raw),
            ("root-observations-input", observations_raw),
            ("target-session-input", session_raw),
        ),
        "verdict": "PASS",
        "blockers": [],
    }
    validate_root(record, args.target, worker_raw, worker)
    _write(args.out, record)
    print(f"worker evidence: {args.target} root-tcb PASS ({args.out})")


def _validate_stage_markers(run_dir: Path, target: str) -> None:
    if not run_dir.is_dir():
        raise EvidenceError(f"test-plan state directory is missing: {run_dir}")
    for stage in range(1, 6):
        for marker in (f"stage_{stage:02d}.done", f"stage_{stage:02d}.{target}.done"):
            if not (run_dir / marker).is_file():
                raise EvidenceError(f"missing target-qualified test-plan marker: {marker}")
    if list(run_dir.glob("stage_*.incomplete")) or (run_dir / "incomplete").exists():
        raise EvidenceError("test-plan state contains incomplete evidence")


def _core_admission(value: Any) -> list[dict[str, Any]]:
    rows = _bounded_list(value, "core admission")
    if len(rows) != 4:
        raise EvidenceError("full-system evidence requires exactly four cores")
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            raise EvidenceError("core admission row must be an object")
        _exact_keys(
            row,
            {"core", "capacity_us", "reserve_us", "admitted_us"},
            context="core admission row",
        )
        if row["core"] != index:
            raise EvidenceError("core admission rows must be ordered 0..3")
        for field in ("capacity_us", "reserve_us", "admitted_us"):
            if (
                not isinstance(row[field], int)
                or isinstance(row[field], bool)
                or row[field] < 0
                or row[field] > 0xFFFF_FFFF
            ):
                raise EvidenceError(f"invalid core admission {field}")
        if row["capacity_us"] == 0 or row["reserve_us"] + row["admitted_us"] > row["capacity_us"]:
            raise EvidenceError("per-core admission exceeds capacity")
    return rows


def validate_system(
    record: Mapping[str, Any],
    target: str,
    worker: Mapping[str, Any],
    worker_raw: bytes,
    root: Mapping[str, Any],
    root_raw: bytes,
) -> None:
    _scan_sensitive(record)
    _exact_keys(
        record,
        {
            "schema",
            "record_kind",
            "target",
            "target_session",
            "worker_component",
            "root_tcb",
            "topology_sha256",
            "core_admission",
            "outcomes",
            "raw_evidence",
            "verdict",
            "blockers",
        },
        context="full-system evidence",
    )
    if (
        record["schema"] != SYSTEM_SCHEMA
        or record["record_kind"] != "full-system"
        or record["target"] != target
    ):
        raise EvidenceError("wrong full-system schema/kind/target")
    session = _target_session(record["target_session"], target)
    if session != worker["target_session"] or session != root["target_session"]:
        raise EvidenceError("full-system target session differs across evidence layers")
    worker_reference = _reference(record["worker_component"], "target-component")
    root_reference = _reference(record["root_tcb"], "root-tcb")
    if (
        worker_reference["sha256"] != _sha256(worker_raw)
        or root_reference["sha256"] != _sha256(root_raw)
    ):
        raise EvidenceError("full-system evidence references stale component/root bytes")
    if root["worker_component"]["sha256"] != worker_reference["sha256"]:
        raise EvidenceError("root-TCB Worker reference differs from full-system graph")
    topology = _hash(record["topology_sha256"], "topology")
    if topology != worker["topology_sha256"] or topology != root["topology_sha256"]:
        raise EvidenceError("full-system topology differs across evidence layers")
    _core_admission(record["core_admission"])
    outcomes = _outcomes(
        record["outcomes"],
        "system outcomes",
        required=record["verdict"] == "PASS",
    )
    if record["verdict"] == "PASS":
        _required_pass_outcomes(
            outcomes,
            SYSTEM_REQUIRED_OUTCOMES,
            "system outcomes",
        )
    _artifacts(record["raw_evidence"], "system raw evidence", required=record["verdict"] == "PASS")
    _verdict(record)


def _system_from_run(
    target: str,
    worker: Mapping[str, Any],
    worker_raw: bytes,
    root: Mapping[str, Any],
    root_raw: bytes,
    run_dir: Path,
    observations_path: Path | None = None,
) -> dict[str, Any]:
    _validate_stage_markers(run_dir, target)
    run, run_raw = _load(observations_path or run_dir / "m26e-system-input.json")
    _exact_keys(
        run,
        {
            "schema",
            "target",
            "target_session",
            "worker_component_sha256",
            "root_tcb_sha256",
            "topology_sha256",
            "core_admission",
            "outcomes",
            "raw_evidence",
            "verdict",
            "blockers",
        },
        context="full-system run input",
    )
    if run["schema"] != SYSTEM_INPUT_SCHEMA or run["target"] != target:
        raise EvidenceError("wrong full-system run-input schema/target")
    if (
        _hash(run["worker_component_sha256"], "run-input Worker component")
        != _sha256(worker_raw)
        or _hash(run["root_tcb_sha256"], "run-input root TCB")
        != _sha256(root_raw)
    ):
        raise EvidenceError("full-system run input references stale component/root bytes")
    record = {
        "schema": SYSTEM_SCHEMA,
        "record_kind": "full-system",
        "target": target,
        "target_session": run["target_session"],
        "worker_component": {
            "id": f"worker-component-{target}",
            "record_kind": "target-component",
            "sha256": _sha256(worker_raw),
        },
        "root_tcb": {
            "id": f"root-tcb-{target}",
            "record_kind": "root-tcb",
            "sha256": _sha256(root_raw),
        },
        "topology_sha256": run["topology_sha256"],
        "core_admission": run["core_admission"],
        "outcomes": run["outcomes"],
        "raw_evidence": _merge_input_artifacts(
            run["raw_evidence"],
            "system raw evidence",
            ("system-observations-input", run_raw),
        ),
        "verdict": run["verdict"],
        "blockers": run["blockers"],
    }
    validate_system(record, target, worker, worker_raw, root, root_raw)
    return record


def _load_integration_references(
    component_path: Path,
    component: Mapping[str, Any],
    target: str,
) -> list[dict[str, Any]]:
    references: list[dict[str, Any]] = []
    for reference in component["integration_evidence"]:
        path = component_path.parent / "integration" / f"{reference['id']}.json"
        record, raw = _load(path)
        validate_integration(record, target)
        if record["dependency_id"] != reference["id"] or _sha256(raw) != reference["sha256"]:
            raise EvidenceError(f"integration record does not match component reference: {path}")
        if record["target_session"] != component["target_session"]:
            raise EvidenceError("integration target session differs from component")
        references.append(
            {
                "id": f"{target}:{reference['id']}",
                "record_kind": "worker-integration",
                "sha256": reference["sha256"],
            }
        )
    return references


def _promote(args: argparse.Namespace) -> None:
    paths = {
        "qemu": (args.worker_qemu, args.root_qemu, args.system_qemu),
        "pi4": (args.worker_pi4, args.root_pi4, args.system_pi4),
    }
    acceptance: list[dict[str, Any]] = []
    integrations: list[dict[str, Any]] = []
    for target, (worker_path, root_path, system_path) in paths.items():
        worker, worker_raw = _load(worker_path)
        root, root_raw = _load(root_path)
        system, system_raw = _load(system_path)
        validate_component(worker, target)
        validate_root(root, target, worker_raw, worker)
        validate_system(system, target, worker, worker_raw, root, root_raw)
        if any(record["verdict"] != "PASS" for record in (worker, root, system)):
            raise EvidenceError(f"cannot promote failed {target} evidence")
        acceptance.extend(
            (
                {
                    "id": f"{target}:full-system",
                    "record_kind": "full-system",
                    "sha256": _sha256(system_raw),
                },
                {
                    "id": f"{target}:root-tcb",
                    "record_kind": "root-tcb",
                    "sha256": _sha256(root_raw),
                },
                {
                    "id": f"{target}:target-component",
                    "record_kind": "target-component",
                    "sha256": _sha256(worker_raw),
                },
            )
        )
        integrations.extend(_load_integration_references(worker_path, worker, target))
    acceptance.sort(key=lambda item: (item["id"], item["record_kind"], item["sha256"]))
    integrations.sort(key=lambda item: (item["id"], item["record_kind"], item["sha256"]))
    release = {
        "schema": RELEASE_SCHEMA,
        "record_kind": "release",
        "scope": "worker-runtime",
        "acceptance_records": acceptance,
        "integration_evidence": integrations,
        "verdict": "PASS",
        "blockers": [],
    }
    _write(args.out, release)
    print(f"worker evidence: release graph PASS ({args.out})")


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "validate":
            record, _ = _load(args.evidence)
            validate_component(record, args.target)
            print(f"worker evidence: {args.target} target-component {record['verdict']}")
        elif args.command == "emit-qemu-target-session":
            _emit_qemu_target_session(args)
        elif args.command == "emit-component":
            _emit_component(args)
        elif args.command == "collect-qemu-preflight":
            _collect_qemu_preflight(args)
        elif args.command == "collect-qemu":
            _collect_qemu(args)
        elif args.command == "qemu-gdb":
            _qemu_gdb(args)
        elif args.command == "qemu-service-gdb":
            _qemu_service_gdb(args)
        elif args.command == "qemu-critical-gdb":
            _qemu_critical_gdb(args)
        elif args.command == "validate-root":
            worker, worker_raw = _load(args.worker)
            root, _ = _load(args.evidence)
            validate_component(worker, args.target)
            validate_root(root, args.target, worker_raw, worker)
            print(f"worker evidence: {args.target} root-tcb {root['verdict']}")
        elif args.command == "emit-root":
            _emit_root(args)
        elif args.command == "validate-system":
            worker, worker_raw = _load(args.worker)
            root, root_raw = _load(args.root)
            validate_component(worker, args.target)
            validate_root(root, args.target, worker_raw, worker)
            record = _system_from_run(
                args.target,
                worker,
                worker_raw,
                root,
                root_raw,
                args.run,
                args.observations,
            )
            _write(args.out, record)
            print(f"worker evidence: {args.target} full-system {record['verdict']}")
        elif args.command == "promote-release":
            _promote(args)
        else:  # pragma: no cover - argparse guarantees a supported command
            raise EvidenceError(f"unsupported command: {args.command}")
    except EvidenceError as exc:
        print(f"worker evidence: FAIL: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
