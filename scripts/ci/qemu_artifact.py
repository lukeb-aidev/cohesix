#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Record, verify, launch, and attest immutable QEMU test artifacts.
# Copyright 2026 Lukas Bower

"""Manage content-addressed QEMU artifacts and transport-test evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
from typing import Any, Iterable, Mapping, Sequence
from urllib.parse import urlsplit


ARTIFACT_SCHEMA = "cohesix.test-plan.qemu-artifact.v2"
RESULT_SCHEMA = "cohesix.test-plan.transport-result.v2"
AGGREGATE_SCHEMA = "cohesix.test-plan.transport-aggregate.v2"
TARGET_EVIDENCE_SCHEMA = "cohesix.test-plan.target-evidence.v1"
SOURCE_PREFIX = "sha256:"
QEMU_INTEGRATION_TIER = "qemu-integration"
QEMU_DIAGNOSTIC_TIER = "qemu-diagnostic"
CANONICAL_SEL4_PROFILE = "qemu_smp_production"
CANONICAL_MACHINE = "virt"
CANONICAL_GIC_VERSION = "3"
CANONICAL_VIRTUALIZATION = "off"
CANONICAL_MACHINE_EXTRA = "kernel-irqchip=off"
CANONICAL_CPU = "cortex-a57"
CANONICAL_TIMER_CLOCK_HZ = 24_000_000
CANONICAL_SMP = "4,cores=4,threads=1,sockets=1"
CANONICAL_NET_BACKEND = "virtio"
QEMU_REQUIRED_FILES = (
    "staging/elfloader",
    "staging/kernel.elf",
    "staging/rootserver",
    "cohesix-system.cpio",
    "host-tools/cohsh",
    "host-tools/hive-gateway",
    "host-tools/coh",
    "host-tools/cas-tool",
    "host-tools/gpu-bridge-host",
    "host-tools/host-sidecar-bridge",
    "host-tools/host-ticket-agent",
    "host-tools/swarmui",
    "cohesix-qemu-launch-artifacts.json",
)


class EvidenceError(RuntimeError):
    """Report fail-closed artifact or evidence validation failures."""


def sha256_bytes(data: bytes) -> str:
    """Return a tagged SHA-256 digest."""

    return f"{SOURCE_PREFIX}{hashlib.sha256(data).hexdigest()}"


def sha256_file(path: Path) -> str:
    """Hash a regular file without loading it wholly into memory."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"{SOURCE_PREFIX}{digest.hexdigest()}"


def canonical_bytes(value: Any) -> bytes:
    """Encode JSON deterministically for content-addressed identifiers."""

    return json.dumps(
        value,
        ensure_ascii=True,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def require_tagged_digest(value: str, label: str) -> str:
    """Validate a tagged SHA-256 digest supplied by another evidence layer."""

    if not value.startswith(SOURCE_PREFIX):
        raise EvidenceError(f"{label} must start with {SOURCE_PREFIX}")
    digest = value.removeprefix(SOURCE_PREFIX)
    if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
        raise EvidenceError(f"{label} is not a lowercase SHA-256 digest")
    return value


def read_json(path: Path) -> dict[str, Any]:
    """Read one JSON object or fail with a concise diagnostic."""

    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"cannot read JSON evidence {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise EvidenceError(f"JSON evidence must be an object: {path}")
    return value


def atomic_write_json(path: Path, value: Mapping[str, Any]) -> None:
    """Publish JSON atomically so interrupted runs cannot leave valid-looking data."""

    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f".{path.name}.",
        suffix=".tmp",
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        temporary.replace(path)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise


def atomic_write_bytes(path: Path, value: bytes) -> None:
    """Publish arbitrary evidence bytes atomically and durably."""

    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f".{path.name}.",
        suffix=".tmp",
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(value)
            handle.flush()
            os.fsync(handle.fileno())
        temporary.replace(path)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise


def require_file(path: Path, label: str) -> Path:
    """Resolve and validate one required regular file."""

    try:
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise EvidenceError(f"{label} is missing: {path}") from exc
    if not resolved.is_file():
        raise EvidenceError(f"{label} is not a regular file: {resolved}")
    return resolved


def safe_relative_file(root: Path, relative: str) -> Path:
    """Resolve an artifact-relative file without permitting path escape."""

    candidate = (root / relative).resolve()
    try:
        candidate.relative_to(root)
    except ValueError as exc:
        raise EvidenceError(f"artifact path escapes artifact root: {relative}") from exc
    return require_file(candidate, f"artifact file {relative}")


def file_record(path: Path, relative: str) -> dict[str, Any]:
    """Create a stable evidence record for one file."""

    return {
        "path": relative,
        "sha256": sha256_file(path),
        "size": path.stat().st_size,
    }


def copy_evidence_file(source: Path, destination: Path) -> Path:
    """Copy a build input into the immutable artifact evidence directory."""

    source = require_file(source, "artifact evidence input")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return destination.resolve(strict=True)


def find_sel4_config(sel4_build: Path) -> Path:
    """Locate the selected seL4 kernel configuration used to infer the GIC."""

    candidates = (
        "kernel/gen_config/kernel_config.h",
        "kernel/gen_config/kernel/gen_config.h",
        "kernel/include/autoconf.h",
        "kernel/autoconf/autoconf.h",
    )
    for relative in candidates:
        candidate = sel4_build / relative
        if candidate.is_file():
            return candidate.resolve()
    raise EvidenceError(
        f"cannot find seL4 kernel configuration under {sel4_build}"
    )


def detect_gic(config: Path, detector: Path) -> str:
    """Run the repository's canonical GIC detector."""

    detector = require_file(detector, "GIC detector")
    try:
        result = subprocess.run(
            [str(detector), str(config)],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise EvidenceError(f"cannot infer GIC version from {config}: {exc}") from exc
    value = result.stdout.strip()
    if value not in {"2", "3"}:
        raise EvidenceError(f"invalid GIC version from {detector}: {value!r}")
    return value


def find_sel4_timer_header(sel4_build: Path) -> Path:
    """Locate the generated platform header that owns TIMER_CLOCK_HZ."""

    candidates = (
        "kernel/gen_headers/plat/platform_gen.h",
        "kernel/gen_headers/plat/machine/devices_gen.h",
    )
    for relative in candidates:
        candidate = sel4_build / relative
        if candidate.is_file():
            return candidate.resolve()
    raise EvidenceError(
        f"cannot find selected seL4 timer header under {sel4_build}"
    )


def detect_timer_clock_hz(header: Path) -> int:
    """Read the single generated TIMER_CLOCK_HZ definition."""

    try:
        contents = header.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        raise EvidenceError(f"cannot read seL4 timer header {header}: {exc}") from exc
    matches = re.findall(
        r"^\s*#define\s+TIMER_CLOCK_HZ\s+"
        r"(?:ULL_CONST\(\s*)?([0-9]+)(?:\s*\))?\s*$",
        contents,
        flags=re.MULTILINE,
    )
    if len(matches) != 1:
        raise EvidenceError(
            "selected seL4 timer header must define TIMER_CLOCK_HZ exactly once"
        )
    return int(matches[0])


def resolve_qemu_binary(value: str) -> Path:
    """Resolve one executable QEMU binary for immutable identity binding."""

    candidate = shutil.which(value) if os.sep not in value else value
    if candidate is None:
        raise EvidenceError(f"QEMU binary is not on PATH: {value}")
    binary = require_file(Path(candidate), "QEMU binary")
    if not os.access(binary, os.X_OK):
        raise EvidenceError(f"QEMU binary is not executable: {binary}")
    return binary


def qemu_version(binary: Path) -> str:
    """Return the exact first QEMU version line or fail closed."""

    try:
        completed = subprocess.run(
            [str(binary), "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise EvidenceError(f"cannot execute QEMU binary {binary}: {exc}") from exc
    lines = completed.stdout.splitlines()[:1]
    if completed.returncode != 0 or not lines or not lines[0].strip():
        raise EvidenceError(f"cannot determine QEMU version from {binary}")
    return lines[0].strip()


def qemu_binary_record(value: str) -> dict[str, Any]:
    """Bind path, bytes, digest, and reported version for QEMU."""

    binary = resolve_qemu_binary(value)
    return {
        "path": str(binary),
        "size": binary.stat().st_size,
        "sha256": sha256_file(binary),
        "version": qemu_version(binary),
    }


def qemu_claim(
    *,
    host_system: str,
    accelerator: str,
    sel4_profile: str,
    machine: str,
    gic_version: str,
    virtualization: str,
    machine_extra: str,
    cpu: str,
    timer_clock_hz: int,
    smp: str,
    net_backend: str,
) -> dict[str, Any]:
    """Classify the exact launch envelope without upgrading diagnostics."""

    reasons = []
    if host_system == "Darwin" and accelerator != "hvf":
        reasons.append("Darwin claiming runs require HVF")
    elif host_system == "Linux" and accelerator != "kvm":
        reasons.append("Linux claiming runs require KVM")
    elif host_system not in {"Darwin", "Linux"}:
        reasons.append(f"unsupported claiming host {host_system}")
    if accelerator == "tcg":
        reasons.append("TCG is diagnostic-only")
    if sel4_profile != CANONICAL_SEL4_PROFILE:
        reasons.append("selected seL4 profile is not qemu_smp_production")
    if machine != CANONICAL_MACHINE:
        reasons.append("machine type is not virt")
    if gic_version != CANONICAL_GIC_VERSION:
        reasons.append("machine does not use GICv3")
    if virtualization != CANONICAL_VIRTUALIZATION:
        reasons.append("machine virtualization is not off")
    if machine_extra != CANONICAL_MACHINE_EXTRA:
        reasons.append("machine does not use exact kernel-irqchip=off envelope")
    if cpu != CANONICAL_CPU:
        reasons.append("CPU is not cortex-a57")
    if timer_clock_hz != CANONICAL_TIMER_CLOCK_HZ:
        reasons.append("selected seL4 timer is not 24 MHz")
    if smp != CANONICAL_SMP:
        reasons.append("QEMU SMP topology is not the four-core production envelope")
    if net_backend != CANONICAL_NET_BACKEND:
        reasons.append("QEMU network backend is not virtio")
    eligible = not reasons
    return {
        "eligible": eligible,
        "tier": QEMU_INTEGRATION_TIER if eligible else QEMU_DIAGNOSTIC_TIER,
        "reason": "canonical production envelope" if eligible else "; ".join(reasons),
    }


def source_digest(repo_root: Path) -> str:
    """Hash Git identity, modes, submodules, and present checkout sources."""

    repo_root = repo_root.resolve(strict=True)
    try:
        file_result = subprocess.run(
            [
                "git",
                "-C",
                str(repo_root),
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ],
            check=True,
            capture_output=True,
        )
        head_result = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "--verify", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        )
        index_result = subprocess.run(
            ["git", "-C", str(repo_root), "ls-files", "--stage", "-z"],
            check=True,
            capture_output=True,
        )
        submodule_result = subprocess.run(
            ["git", "-C", str(repo_root), "submodule", "status", "--recursive"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise EvidenceError(f"cannot bind repository source state: {exc}") from exc

    index_records: list[dict[str, str]] = []
    index_by_path: dict[str, dict[str, str]] = {}
    for raw_entry in index_result.stdout.split(b"\0"):
        if not raw_entry:
            continue
        metadata, raw_path = raw_entry.split(b"\t", maxsplit=1)
        mode, object_id, stage = metadata.decode("ascii").split(" ", maxsplit=2)
        relative = raw_path.decode("utf-8", errors="strict")
        record = {
            "path": relative,
            "mode": mode,
            "object_id": object_id,
            "stage": stage,
        }
        index_records.append(record)
        if stage == "0":
            index_by_path[relative] = record
    index_records.sort(
        key=lambda record: (record["path"], record["stage"])
    )

    records: list[dict[str, Any]] = []
    for raw_relative in file_result.stdout.split(b"\0"):
        if not raw_relative:
            continue
        relative = raw_relative.decode("utf-8", errors="strict")
        path = repo_root / relative
        index = index_by_path.get(relative)
        if path.is_symlink():
            records.append(
                {
                    "path": relative,
                    "kind": "symlink",
                    "target": os.readlink(path),
                    "index": index,
                }
            )
        elif path.is_file():
            records.append(
                {
                    "path": relative,
                    "kind": "file",
                    "executable": bool(path.stat().st_mode & 0o111),
                    "sha256": sha256_file(path),
                    "index": index,
                }
            )
        elif index is not None and index["mode"] == "160000":
            records.append(
                {
                    "path": relative,
                    "kind": "submodule",
                    "index": index,
                }
            )
        else:
            records.append(
                {
                    "path": relative,
                    "kind": "missing",
                    "index": index,
                }
            )
    records.sort(key=lambda record: str(record["path"]))
    binding = {
        "git_head": head_result.stdout.strip(),
        "git_index": index_records,
        "git_submodules": sorted(
            line.strip()
            for line in submodule_result.stdout.splitlines()
            if line.strip()
        ),
        "working_files": records,
    }
    return sha256_bytes(canonical_bytes(binding))


def artifact_identity_material(document: Mapping[str, Any]) -> dict[str, Any]:
    """Select path-independent artifact fields covered by the content ID."""

    return {
        "schema": document["schema"],
        "action_id": document["action_id"],
        "catalog_action_digest": document["catalog_action_digest"],
        "source_digest": document["source_digest"],
        "attempt_manifest": document.get("attempt_manifest"),
        "input_manifest": document["input_manifest"],
        "resolved_manifest": document["resolved_manifest"],
        "policy": document["policy"],
        "sel4": document["sel4"],
        "build": document["build"],
        "qemu": document["qemu"],
        "files": document["files"],
    }


def verify_file_record(root: Path, record: Mapping[str, Any]) -> None:
    """Verify one size-and-hash record beneath an artifact root."""

    relative = str(record.get("path", ""))
    if not relative:
        raise EvidenceError("artifact file record has no path")
    path = safe_relative_file(root, relative)
    expected_size = record.get("size")
    if path.stat().st_size != expected_size:
        raise EvidenceError(
            f"artifact size mismatch for {relative}: "
            f"expected {expected_size}, got {path.stat().st_size}"
        )
    expected_digest = str(record.get("sha256", ""))
    actual_digest = sha256_file(path)
    if actual_digest != expected_digest:
        raise EvidenceError(
            f"artifact hash mismatch for {relative}: "
            f"expected {expected_digest}, got {actual_digest}"
        )


def verify_qemu_launch_contract(document: Mapping[str, Any]) -> dict[str, Any]:
    """Verify immutable host, binary, accelerator, and machine claim truth."""

    qemu = document.get("qemu")
    if not isinstance(qemu, dict):
        raise EvidenceError("artifact QEMU launch context must be an object")
    expected_keys = {
        "host_system",
        "binary",
        "accelerator",
        "machine",
        "gic_version",
        "virtualization",
        "machine_extra",
        "cpu",
        "timer_clock_hz",
        "net_backend",
        "smp",
        "claim",
    }
    if set(qemu) != expected_keys:
        raise EvidenceError("artifact QEMU launch context has invalid fields")
    host_system = qemu.get("host_system")
    if host_system != platform.system():
        raise EvidenceError(
            "artifact QEMU host differs from the verifying host: "
            f"expected {host_system}, got {platform.system()}"
        )
    binary = qemu.get("binary")
    if not isinstance(binary, dict) or set(binary) != {
        "path",
        "size",
        "sha256",
        "version",
    }:
        raise EvidenceError("artifact QEMU binary identity has invalid fields")
    binary_path = binary.get("path")
    if not isinstance(binary_path, str) or not Path(binary_path).is_absolute():
        raise EvidenceError("artifact QEMU binary path must be absolute")
    resolved_binary = resolve_qemu_binary(binary_path)
    if (
        str(resolved_binary) != binary_path
        or resolved_binary.stat().st_size != binary.get("size")
        or sha256_file(resolved_binary) != binary.get("sha256")
        or not isinstance(binary.get("version"), str)
        or not str(binary["version"]).strip()
    ):
        raise EvidenceError("artifact QEMU binary identity changed")
    accelerator = qemu.get("accelerator")
    if accelerator not in {"hvf", "kvm", "tcg"}:
        raise EvidenceError(f"unsupported recorded QEMU accelerator: {accelerator!r}")

    sel4 = document.get("sel4")
    if not isinstance(sel4, Mapping):
        raise EvidenceError("artifact seL4 context must be an object")
    timer_clock_hz = sel4.get("timer_clock_hz")
    if qemu.get("timer_clock_hz") != timer_clock_hz:
        raise EvidenceError("artifact QEMU and seL4 timer clocks differ")
    claim = qemu_claim(
        host_system=str(host_system),
        accelerator=str(accelerator),
        sel4_profile=str(sel4.get("profile", "")),
        machine=str(qemu.get("machine", "")),
        gic_version=str(qemu.get("gic_version", "")),
        virtualization=str(qemu.get("virtualization", "")),
        machine_extra=str(qemu.get("machine_extra", "")),
        cpu=str(qemu.get("cpu", "")),
        timer_clock_hz=timer_clock_hz if isinstance(timer_clock_hz, int) else -1,
        smp=str(qemu.get("smp", "")),
        net_backend=str(qemu.get("net_backend", "")),
    )
    if qemu.get("claim") != claim:
        raise EvidenceError("artifact QEMU claim classification is invalid")
    return claim


def verify_embedded_launch_record(
    document: Mapping[str, Any],
    artifact_root: Path,
) -> None:
    """Require the reusable launch record to describe the identical envelope."""

    path = safe_relative_file(
        artifact_root,
        "cohesix-qemu-launch-artifacts.json",
    )
    launch = read_json(path)
    if set(launch) != {
        "schema",
        "profile",
        "cargo_target",
        "root_task_features",
        "sel4_build_dir",
        "sel4_profile",
        "gic_version",
        "qemu",
        "claim",
        "artifacts",
    } or launch.get("schema") != "cohesix-qemu-launch-artifacts/v2":
        raise EvidenceError("embedded QEMU launch record is not schema v2")
    launch_qemu = launch.get("qemu")
    launch_binary = launch_qemu.get("binary") if isinstance(launch_qemu, dict) else None
    qemu = document["qemu"]
    binary = qemu["binary"]
    if not isinstance(launch_binary, Mapping):
        raise EvidenceError("embedded QEMU launch record has no binary identity")
    binary_matches = (
        launch_binary.get("path") == binary.get("path")
        and launch_binary.get("bytes") == binary.get("size")
        and f"{SOURCE_PREFIX}{launch_binary.get('sha256')}" == binary.get("sha256")
        and launch_binary.get("version") == binary.get("version")
    )
    launch_qemu_matches = isinstance(launch_qemu, Mapping) and all(
        launch_qemu.get(key) == qemu.get(key)
        for key in (
            "host_system",
            "accelerator",
            "machine",
            "virtualization",
            "machine_extra",
            "cpu",
            "timer_clock_hz",
            "smp",
            "net_backend",
        )
    )
    sel4 = document["sel4"]
    build = document["build"]
    if (
        not binary_matches
        or not launch_qemu_matches
        or launch.get("gic_version") != qemu.get("gic_version")
        or launch.get("claim") != qemu.get("claim")
        or launch.get("sel4_build_dir") != sel4.get("build_dir")
        or launch.get("sel4_profile") != sel4.get("profile")
        or launch_qemu.get("timer_clock_hz") != sel4.get("timer_clock_hz")
        or launch.get("profile") != build.get("cargo_profile")
        or launch.get("cargo_target") != build.get("cargo_target")
        or launch.get("root_task_features") != build.get("root_task_features")
    ):
        raise EvidenceError(
            "embedded QEMU launch record differs from the artifact launch context"
        )


def verify_artifact_document(
    path: Path,
    *,
    expected_source_digest: str | None = None,
    expected_action_id: str | None = None,
    expected_catalog_action_digest: str | None = None,
) -> dict[str, Any]:
    """Validate an artifact document and every file it covers."""

    document = read_json(require_file(path, "QEMU artifact manifest"))
    if document.get("schema") != ARTIFACT_SCHEMA:
        raise EvidenceError(
            f"unsupported QEMU artifact schema: {document.get('schema')!r}"
        )
    source = require_tagged_digest(
        str(document.get("source_digest", "")),
        "artifact source_digest",
    )
    if expected_source_digest is not None and source != expected_source_digest:
        raise EvidenceError(
            "artifact source digest does not match the active test attempt"
        )
    if (
        expected_action_id is not None
        and document.get("action_id") != expected_action_id
    ):
        raise EvidenceError(
            f"artifact action mismatch: expected {expected_action_id}, "
            f"got {document.get('action_id')}"
        )
    catalog_action_digest = require_tagged_digest(
        str(document.get("catalog_action_digest", "")),
        "artifact catalog_action_digest",
    )
    if (
        expected_catalog_action_digest is not None
        and catalog_action_digest != expected_catalog_action_digest
    ):
        raise EvidenceError(
            "artifact catalog action digest does not match the active action"
        )

    artifact_root_text = document.get("artifact_root")
    if not isinstance(artifact_root_text, str) or not artifact_root_text:
        raise EvidenceError("artifact manifest has no artifact_root")
    artifact_root_value = Path(artifact_root_text)
    if artifact_root_value.is_absolute():
        artifact_root = artifact_root_value.resolve(strict=True)
    else:
        artifact_root = (path.parent / artifact_root_value).resolve(strict=True)
    if not artifact_root.is_dir():
        raise EvidenceError(f"artifact root is not a directory: {artifact_root}")

    records = document.get("files")
    if not isinstance(records, list) or not records:
        raise EvidenceError("artifact manifest has no file records")
    record_paths: set[str] = set()
    for record in records:
        if not isinstance(record, dict):
            raise EvidenceError("artifact file record must be an object")
        relative = str(record.get("path", ""))
        if relative in record_paths:
            raise EvidenceError(f"duplicate artifact file record: {relative}")
        record_paths.add(relative)
        verify_file_record(artifact_root, record)
    missing_required = sorted(set(QEMU_REQUIRED_FILES) - record_paths)
    if missing_required:
        raise EvidenceError(
            "artifact manifest is missing required file records: "
            + ", ".join(missing_required)
        )

    verify_qemu_launch_contract(document)
    verify_embedded_launch_record(document, artifact_root)

    expected_id = sha256_bytes(canonical_bytes(artifact_identity_material(document)))
    if document.get("artifact_id") != expected_id:
        raise EvidenceError(
            f"artifact ID mismatch: expected {expected_id}, "
            f"got {document.get('artifact_id')}"
        )
    # Keep serialized paths relocatable. This verified, process-local field is
    # deliberately excluded from the content identity.
    document["_resolved_artifact_root"] = str(artifact_root)
    return document


def command_source_digest(args: argparse.Namespace) -> int:
    """Print the deterministic source digest for a checkout."""

    print(source_digest(args.repo_root))
    return 0


def command_copy_evidence(args: argparse.Namespace) -> int:
    """Copy one validated JSON evidence object into an immutable attempt tree."""

    source = require_file(args.source, "target evidence")
    read_json(source)
    atomic_write_bytes(args.output, source.read_bytes())
    print(sha256_file(args.output))
    return 0


def confined_destination(state: Path, value: Path, label: str) -> Path:
    """Resolve a destination parent without following its final component."""

    state = state.resolve(strict=True)
    try:
        parent = value.parent.resolve(strict=True)
    except OSError as exc:
        raise EvidenceError(f"{label} parent is missing: {value.parent}") from exc
    destination = parent / value.name
    try:
        destination.relative_to(state)
    except ValueError as exc:
        raise EvidenceError(
            f"{label} escapes test-plan state: {destination}"
        ) from exc
    return destination


def confined_state_path(state: Path, value: Path, label: str) -> Path:
    """Resolve one path and require it to remain beneath a state directory."""

    state = state.resolve(strict=True)
    resolved = value.resolve(strict=True)
    try:
        resolved.relative_to(state)
    except ValueError as exc:
        raise EvidenceError(f"{label} escapes test-plan state: {resolved}") from exc
    return resolved


def command_publish_root(args: argparse.Namespace) -> int:
    """Atomically publish a current pointer without mutating old attempt roots."""

    state = args.state_dir.resolve(strict=True)
    root = confined_state_path(state, args.root, "artifact root")
    if not root.is_dir():
        raise EvidenceError(f"artifact root is not a directory: {root}")
    relative = root.relative_to(state).as_posix()
    pointer = confined_destination(state, args.pointer, "artifact pointer")
    atomic_write_bytes(pointer, f"{relative}\n".encode("utf-8"))

    if args.compat_link is not None:
        if args.compat_target is None:
            raise EvidenceError("--compat-link requires --compat-target")
        target = confined_state_path(state, args.compat_target, "compat target")
        link = confined_destination(state, args.compat_link, "compat link")
        if link.exists() and not link.is_symlink():
            raise EvidenceError(
                f"compat link destination exists and is not a symlink: {link}"
            )
        link.parent.mkdir(parents=True, exist_ok=True)
        relative_target = os.path.relpath(target, start=link.parent)
        temporary = link.parent / f".{link.name}.{os.getpid()}.tmp"
        temporary.unlink(missing_ok=True)
        os.symlink(relative_target, temporary)
        os.replace(temporary, link)
    print(relative)
    return 0


def command_resolve_root(args: argparse.Namespace) -> int:
    """Resolve a state-relative current pointer and reject path traversal."""

    state = args.state_dir.resolve(strict=True)
    pointer = require_file(args.pointer, "artifact root pointer")
    relative_text = pointer.read_text(encoding="utf-8").strip()
    relative = Path(relative_text)
    if relative.is_absolute() or ".." in relative.parts or not relative.parts:
        raise EvidenceError(f"unsafe artifact root pointer: {relative_text!r}")
    root = (state / relative).resolve(strict=True)
    try:
        root.relative_to(state)
    except ValueError as exc:
        raise EvidenceError(f"artifact root pointer escapes state: {root}") from exc
    if not root.is_dir():
        raise EvidenceError(f"artifact root pointer is not a directory: {root}")
    print(root)
    return 0


def command_record(args: argparse.Namespace) -> int:
    """Record a built QEMU artifact and its canonical build inputs."""

    artifact_root = args.artifact_dir.resolve(strict=True)
    if not artifact_root.is_dir():
        raise EvidenceError(f"artifact directory is missing: {artifact_root}")
    source = require_tagged_digest(args.source_digest, "source_digest")
    input_manifest = require_file(args.manifest, "source manifest")
    sel4_build = args.sel4_build.resolve(strict=True)
    if not sel4_build.is_dir():
        raise EvidenceError(f"seL4 build directory is missing: {sel4_build}")

    evidence_dir = artifact_root / "evidence"
    resolved_copy = copy_evidence_file(
        args.resolved_manifest,
        evidence_dir / "root_task_resolved.json",
    )
    policy_copy = copy_evidence_file(
        args.policy,
        evidence_dir / "cohsh_policy.toml",
    )
    manifest_copy = copy_evidence_file(
        input_manifest,
        evidence_dir / "source_manifest.toml",
    )
    attempt_record: dict[str, Any] | None = None
    if args.attempt_manifest is not None:
        attempt_copy = copy_evidence_file(
            args.attempt_manifest,
            evidence_dir / "test_plan_attempt.json",
        )
        attempt_record = file_record(
            attempt_copy,
            "evidence/test_plan_attempt.json",
        )

    sel4_config = find_sel4_config(sel4_build)
    gic_version = detect_gic(sel4_config, args.detect_gic_script)
    timer_header = find_sel4_timer_header(sel4_build)
    timer_clock_hz = detect_timer_clock_hz(timer_header)
    selected_accelerator = qemu_accelerator(
        args.qemu,
        requested=args.accelerator,
    )
    binary_record = qemu_binary_record(args.qemu)
    host_system = platform.system()
    claim = qemu_claim(
        host_system=host_system,
        accelerator=selected_accelerator,
        sel4_profile=args.sel4_profile,
        machine=args.machine,
        gic_version=gic_version,
        virtualization=args.virtualization,
        machine_extra=args.machine_extra,
        cpu=args.cpu,
        timer_clock_hz=timer_clock_hz,
        smp=args.smp,
        net_backend=args.net_backend,
    )
    sel4_config_copy = copy_evidence_file(
        sel4_config,
        evidence_dir / "sel4_kernel_config.h",
    )
    timer_header_copy = copy_evidence_file(
        timer_header,
        evidence_dir / "sel4_platform_gen.h",
    )

    relative_files = list(QEMU_REQUIRED_FILES)
    relative_files.extend(
        (
            "evidence/source_manifest.toml",
            "evidence/root_task_resolved.json",
            "evidence/cohsh_policy.toml",
            "evidence/sel4_kernel_config.h",
            "evidence/sel4_platform_gen.h",
        )
    )
    if attempt_record is not None:
        relative_files.append("evidence/test_plan_attempt.json")

    records = [
        file_record(safe_relative_file(artifact_root, relative), relative)
        for relative in sorted(relative_files)
    ]
    input_record = file_record(
        manifest_copy,
        "evidence/source_manifest.toml",
    )
    resolved_record = file_record(
        resolved_copy,
        "evidence/root_task_resolved.json",
    )
    policy_record = file_record(policy_copy, "evidence/cohsh_policy.toml")
    sel4_record = file_record(
        sel4_config_copy,
        "evidence/sel4_kernel_config.h",
    )
    timer_record = file_record(
        timer_header_copy,
        "evidence/sel4_platform_gen.h",
    )

    document: dict[str, Any] = {
        "schema": ARTIFACT_SCHEMA,
        "action_id": args.action_id,
        "catalog_action_digest": require_tagged_digest(
            args.catalog_action_digest,
            "catalog_action_digest",
        ),
        "artifact_root": os.path.relpath(artifact_root, start=args.output.parent),
        "source_digest": source,
        "attempt_manifest": attempt_record,
        "input_manifest": input_record,
        "resolved_manifest": resolved_record,
        "policy": policy_record,
        "sel4": {
            "profile": args.sel4_profile,
            "build_dir": str(sel4_build),
            "kernel_config": sel4_record,
            "timer_header": timer_record,
            "timer_clock_hz": timer_clock_hz,
        },
        "build": {
            "cargo_profile": args.cargo_profile,
            "cargo_target": args.cargo_target,
            "root_task_features": args.root_task_features,
        },
        "qemu": {
            "host_system": host_system,
            "binary": binary_record,
            "accelerator": selected_accelerator,
            "machine": args.machine,
            "gic_version": gic_version,
            "machine_extra": args.machine_extra,
            "net_backend": args.net_backend,
            "smp": args.smp,
            "virtualization": args.virtualization,
            "cpu": args.cpu,
            "timer_clock_hz": timer_clock_hz,
            "claim": claim,
        },
        "files": records,
    }
    document["artifact_id"] = sha256_bytes(
        canonical_bytes(artifact_identity_material(document))
    )
    atomic_write_json(args.output, document)
    verify_artifact_document(
        args.output,
        expected_source_digest=source,
        expected_action_id=args.action_id,
        expected_catalog_action_digest=args.catalog_action_digest,
    )
    print(document["artifact_id"])
    return 0


def command_verify(args: argparse.Namespace) -> int:
    """Verify one QEMU artifact and print its content ID."""

    expected_source = args.source_digest
    if expected_source is not None:
        require_tagged_digest(expected_source, "expected source_digest")
    document = verify_artifact_document(
        args.artifact_manifest,
        expected_source_digest=expected_source,
        expected_action_id=args.action_id,
        expected_catalog_action_digest=args.catalog_action_digest,
    )
    print(document["artifact_id"])
    return 0


def qemu_accelerator(qemu: str, requested: str | None = None) -> str:
    """Select an available accelerator without silent claim-tier fallback."""

    selected = requested or os.environ.get("COHESIX_QEMU_ACCEL") or os.environ.get(
        "QEMU_ACCEL"
    )
    if not selected:
        host_system = platform.system()
        selected = "hvf" if host_system == "Darwin" else "tcg"
        if host_system == "Linux" and os.access("/dev/kvm", os.R_OK | os.W_OK):
            selected = "kvm"
    binary = resolve_qemu_binary(qemu)
    try:
        result = subprocess.run(
            [str(binary), "-accel", "help"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise EvidenceError(f"cannot execute QEMU binary {binary}: {exc}") from exc
    help_text = f"{result.stdout}\n{result.stderr}"
    if not help_text.strip() or selected not in help_text.split():
        raise EvidenceError(
            f"QEMU accelerator {selected!r} is not advertised by {binary}; "
            "select tcg explicitly only for diagnostic evidence"
        )
    return selected


def validate_port(value: int, label: str) -> None:
    """Validate one host-forward port."""

    if value < 1 or value > 65535:
        raise EvidenceError(f"{label} must be between 1 and 65535")


def require_port_available(port: int, label: str, socket_type: int) -> None:
    """Fail before launch when a requested host-forward port is occupied."""

    validate_port(port, label)
    try:
        with socket.socket(socket.AF_INET, socket_type) as listener:
            listener.bind(("127.0.0.1", port))
            if socket_type == socket.SOCK_STREAM:
                listener.listen(1)
    except OSError as exc:
        raise EvidenceError(
            f"{label} is unavailable on 127.0.0.1:{port}: {exc}"
        ) from exc


def build_qemu_command(
    document: Mapping[str, Any],
    *,
    qemu: str,
    console_port: int,
    udp_port: int,
    smoke_port: int,
) -> list[str]:
    """Build the launch command for an already-verified artifact."""

    validate_port(console_port, "console port")
    validate_port(udp_port, "UDP port")
    validate_port(smoke_port, "smoke port")
    if len({console_port, udp_port, smoke_port}) != 3:
        raise EvidenceError("QEMU host-forward ports must be distinct")

    root = Path(str(document["_resolved_artifact_root"]))
    qemu_record = document["qemu"]
    recorded_binary = Path(str(qemu_record["binary"]["path"]))
    requested_binary = resolve_qemu_binary(qemu)
    if requested_binary != recorded_binary:
        raise EvidenceError(
            "launch QEMU binary differs from the artifact record: "
            f"expected {recorded_binary}, got {requested_binary}"
        )
    accelerator = str(qemu_record["accelerator"])
    qemu_accelerator(str(recorded_binary), requested=accelerator)
    machine = (
        f"{qemu_record['machine']},"
        f"gic-version={qemu_record['gic_version']},"
        f"virtualization={qemu_record['virtualization']}"
    )
    machine_extra = str(qemu_record.get("machine_extra", "")).strip()
    if machine_extra:
        machine = f"{machine},{machine_extra}"

    cpu = str(qemu_record["cpu"])
    if accelerator == "tcg":
        cpu = f"{cpu},cntfrq={qemu_record['timer_clock_hz']}"
    command = [
        str(recorded_binary),
        "-accel",
        accelerator,
        "-machine",
        machine,
        "-cpu",
        cpu,
        "-m",
        "1024",
        "-smp",
        str(qemu_record["smp"]),
        "-serial",
        "mon:stdio",
        "-display",
        "none",
        "-kernel",
        str(root / "staging/elfloader"),
        "-initrd",
        str(root / "cohesix-system.cpio"),
        "-device",
        (
            f"loader,file={root / 'staging/kernel.elf'},"
            "addr=0x70000000,force-raw=on"
        ),
        "-device",
        (
            f"loader,file={root / 'staging/rootserver'},"
            "addr=0x80000000,force-raw=on"
        ),
    ]
    if qemu_record["net_backend"] == "virtio":
        command.extend(("-global", "virtio-mmio.force-legacy=off"))
    command.extend(
        (
            "-netdev",
            (
                "user,id=net0,"
                f"hostfwd=tcp:127.0.0.1:{console_port}-:31337,"
                f"hostfwd=udp:127.0.0.1:{udp_port}-:31338,"
                f"hostfwd=tcp:127.0.0.1:{smoke_port}-:31339"
            ),
            "-device",
        )
    )
    if qemu_record["net_backend"] == "virtio":
        command.append(
            "virtio-net-device,netdev=net0,"
            "mac=52:55:00:d1:55:01,bus=virtio-mmio-bus.0"
        )
    else:
        command.append("rtl8139,netdev=net0,mac=52:55:00:d1:55:01")
    return command


def command_launch(args: argparse.Namespace) -> int:
    """Verify then launch a fresh QEMU instance from a recorded artifact."""

    document = verify_artifact_document(
        args.artifact_manifest,
        expected_source_digest=args.source_digest,
        expected_action_id=args.action_id,
        expected_catalog_action_digest=args.catalog_action_digest,
    )
    require_port_available(args.console_port, "console port", socket.SOCK_STREAM)
    require_port_available(args.udp_port, "UDP port", socket.SOCK_DGRAM)
    require_port_available(args.smoke_port, "smoke port", socket.SOCK_STREAM)
    command = build_qemu_command(
        document,
        qemu=args.qemu,
        console_port=args.console_port,
        udp_port=args.udp_port,
        smoke_port=args.smoke_port,
    )
    if args.print_command:
        print(shlex.join(command))
        return 0
    print(
        "qemu-artifact: launching fresh boot "
        f"artifact_id={document['artifact_id']}",
        file=sys.stderr,
    )
    os.execvp(command[0], command)
    raise AssertionError("os.execvp unexpectedly returned")


def evidence_lookup(
    document: Mapping[str, Any],
    candidates: Iterable[Sequence[str]],
) -> Any:
    """Find the first present scalar at one of several schema-compatible paths."""

    for candidate in candidates:
        value: Any = document
        for key in candidate:
            if not isinstance(value, Mapping) or key not in value:
                break
            value = value[key]
        else:
            if value is not None and value != "":
                return value
    return None


def normalized_gateway_url(value: str) -> tuple[str, str, int, str]:
    """Normalize one credential-free HTTP gateway URL for evidence comparison."""

    parsed = urlsplit(value)
    if parsed.scheme.lower() not in {"http", "https"} or not parsed.hostname:
        raise EvidenceError("invalid gateway URL in target evidence")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise EvidenceError(
            "gateway evidence URLs must not contain credentials, query, or fragment"
        )
    default_port = 443 if parsed.scheme.lower() == "https" else 80
    try:
        port = parsed.port or default_port
    except ValueError as exc:
        raise EvidenceError("invalid gateway URL port") from exc
    path = parsed.path.rstrip("/") or "/"
    return parsed.scheme.lower(), parsed.hostname.lower(), port, path


def require_nonempty_scalar(value: str, label: str) -> str:
    """Validate one bounded, single-line evidence identifier."""

    normalized = value.strip()
    if (
        not normalized
        or normalized != value
        or len(normalized) > 512
        or any(character in normalized for character in "\r\n\0")
    ):
        raise EvidenceError(f"{label} must be a non-empty single-line value")
    return normalized


def require_host(value: str, label: str) -> str:
    """Validate a hostname or address without accepting URL syntax."""

    normalized = require_nonempty_scalar(value, label)
    if any(
        character.isspace() or character in "/@?#"
        for character in normalized
    ):
        raise EvidenceError(f"{label} must be a hostname or address")
    return normalized


def command_record_pi4_evidence(args: argparse.Namespace) -> int:
    """Write validated, machine-readable Pi transport target evidence."""

    source = require_tagged_digest(args.source_digest, "source_digest")
    image_identity = require_tagged_digest(
        args.image_identity,
        "image_identity",
    )
    boot_id = require_nonempty_scalar(args.boot_id, "boot_id")
    target_host = require_host(args.target_host, "target_host")
    document: dict[str, Any] = {
        "schema": TARGET_EVIDENCE_SCHEMA,
        "claim_tier": "pi4-transport",
        "target": "pi4",
        "source_digest": source,
        "boot_id": boot_id,
        "image_identity": image_identity,
        "target_host": target_host,
    }
    if args.gateway_target_host is not None:
        document["gateway_target_host"] = require_host(
            args.gateway_target_host,
            "gateway_target_host",
        )
    if args.gateway_url is not None:
        normalized = normalized_gateway_url(args.gateway_url)
        if (
            args.gateway_target_host is not None
            and normalized[1]
            != str(document["gateway_target_host"]).strip().lower()
        ):
            raise EvidenceError(
                "gateway_url host does not match gateway_target_host"
            )
        document["gateway_url"] = args.gateway_url
    atomic_write_json(args.output, document)
    print(sha256_file(args.output))
    return 0


def command_verify_pi4_evidence(args: argparse.Namespace) -> int:
    """Validate Pi target evidence before starting a live regression batch."""

    source = require_tagged_digest(
        args.source_digest,
        "Pi 4 evidence source_digest",
    )
    document = verify_pi4_transport_evidence(
        args.target_evidence,
        expected_source_digest=source,
    )
    if args.gateway_url is not None:
        verify_gateway_binding(document, args.gateway_url)
    print(
        evidence_lookup(
            document,
            (("boot_id",), ("boot", "id")),
        )
    )
    return 0


def verify_gateway_binding(
    document: Mapping[str, Any],
    gateway_url: str,
) -> None:
    """Require target evidence to identify the exact supplied REST gateway."""

    supplied = normalized_gateway_url(gateway_url)
    evidence_url = evidence_lookup(
        document,
        (
            ("gateway_url",),
            ("gateway", "url"),
            ("transport", "gateway_url"),
        ),
    )
    if evidence_url is not None:
        if normalized_gateway_url(str(evidence_url)) != supplied:
            raise EvidenceError(
                "supplied gateway URL does not match target evidence gateway_url"
            )
        return

    evidence_host = evidence_lookup(
        document,
        (
            ("gateway_target_host",),
            ("gateway", "target_host"),
            ("transport", "gateway_host"),
            ("target_host",),
            ("target", "host"),
        ),
    )
    if evidence_host is None:
        raise EvidenceError(
            "target evidence is missing gateway_url or gateway target host"
        )
    if str(evidence_host).strip().lower() != supplied[1]:
        raise EvidenceError(
            "supplied gateway host does not match target evidence; "
            "record gateway_url or gateway_target_host for a host-side proxy"
        )


def required_gateway_url(args: argparse.Namespace) -> str:
    """Read a gateway URL from an argument or the inherited environment."""

    value = args.gateway_url or os.environ.get("COHESIX_GATEWAY_URL", "")
    if not value:
        raise EvidenceError(
            "gateway URL is required via --gateway-url or COHESIX_GATEWAY_URL"
        )
    normalized_gateway_url(value)
    return value


def verify_pi4_transport_evidence(
    path: Path,
    *,
    expected_source_digest: str,
) -> dict[str, Any]:
    """Validate the minimum machine-generated binding for a Pi transport run."""

    document = read_json(require_file(path, "Pi 4 target evidence"))
    if document.get("schema") != TARGET_EVIDENCE_SCHEMA:
        raise EvidenceError("unsupported Pi 4 target-evidence schema")
    tier = evidence_lookup(
        document,
        (("claim_tier",), ("claim", "tier"), ("tier",)),
    )
    if tier != "pi4-transport":
        raise EvidenceError(
            "Pi 4 staged transport evidence must carry pi4-transport tier; "
            "pi4-hardware requires the separate machine-validated hardware bundle"
        )
    target = evidence_lookup(
        document,
        (("target",), ("target", "name"), ("platform",)),
    )
    if isinstance(target, Mapping):
        target = target.get("name")
    if target != "pi4":
        raise EvidenceError("Pi 4 target evidence does not identify target=pi4")
    source = evidence_lookup(
        document,
        (("source_digest",), ("source", "digest")),
    )
    if source != expected_source_digest:
        raise EvidenceError(
            "Pi 4 target evidence source digest does not match this attempt"
        )
    required = {
        "boot_id": (("boot_id",), ("boot", "id")),
        "image_identity": (
            ("image_identity",),
            ("image", "identity"),
            ("image", "artifact_id"),
        ),
        "target_host": (
            ("target_host",),
            ("transport", "host"),
            ("target", "host"),
        ),
    }
    for label, candidates in required.items():
        value = evidence_lookup(document, candidates)
        if value is None:
            raise EvidenceError(f"Pi 4 target evidence is missing {label}")
        if label == "image_identity":
            require_tagged_digest(str(value), label)
        elif label == "target_host":
            require_host(str(value), label)
        else:
            require_nonempty_scalar(str(value), label)
    gateway_url = evidence_lookup(
        document,
        (("gateway_url",), ("gateway", "url"), ("transport", "gateway_url")),
    )
    if gateway_url is not None:
        normalized_gateway_url(str(gateway_url))
    gateway_host = evidence_lookup(
        document,
        (
            ("gateway_target_host",),
            ("gateway", "target_host"),
            ("transport", "gateway_host"),
        ),
    )
    if gateway_host is not None:
        require_host(str(gateway_host), "gateway_target_host")
    return document


def verify_qemu_target_evidence(
    path: Path,
    *,
    expected_source_digest: str,
    expected_artifact_id: str,
) -> dict[str, Any]:
    """Validate a machine-generated external-QEMU boot binding."""

    document = read_json(require_file(path, "QEMU target evidence"))
    if document.get("schema") != TARGET_EVIDENCE_SCHEMA:
        raise EvidenceError("unsupported QEMU target-evidence schema")
    tier = evidence_lookup(
        document,
        (("claim_tier",), ("claim", "tier"), ("tier",)),
    )
    if tier != "qemu-integration":
        raise EvidenceError(
            "QEMU target evidence must carry qemu-integration tier"
        )
    target = evidence_lookup(
        document,
        (("target",), ("target", "name"), ("platform",)),
    )
    if isinstance(target, Mapping):
        target = target.get("name")
    if target != "qemu":
        raise EvidenceError("QEMU target evidence does not identify target=qemu")
    source = evidence_lookup(
        document,
        (("source_digest",), ("source", "digest")),
    )
    if source != expected_source_digest:
        raise EvidenceError(
            "QEMU target evidence source digest does not match this attempt"
        )
    artifact_id = evidence_lookup(
        document,
        (
            ("artifact_id",),
            ("artifact", "id"),
            ("image", "artifact_id"),
        ),
    )
    if artifact_id != expected_artifact_id:
        raise EvidenceError(
            "QEMU target evidence does not bind the selected Stage 03 artifact"
        )
    boot_id = evidence_lookup(document, (("boot_id",), ("boot", "id")))
    if boot_id is None:
        raise EvidenceError("QEMU target evidence is missing boot_id")
    require_nonempty_scalar(str(boot_id), "boot_id")
    target_host = evidence_lookup(
        document,
        (("target_host",), ("transport", "host"), ("target", "host")),
    )
    gateway_url = evidence_lookup(
        document,
        (("gateway_url",), ("gateway", "url"), ("transport", "gateway_url")),
    )
    if target_host is None and gateway_url is None:
        raise EvidenceError("QEMU target evidence is missing target_host")
    if target_host is not None:
        require_host(str(target_host), "target_host")
    if gateway_url is not None:
        normalized_gateway_url(str(gateway_url))
    return document


def result_identity_material(document: Mapping[str, Any]) -> dict[str, Any]:
    """Select stable result fields covered by the result ID."""

    return {
        "schema": document["schema"],
        "action_id": document["action_id"],
        "catalog_action_digest": document["catalog_action_digest"],
        "claim_tier": document["claim_tier"],
        "target": document["target"],
        "source_digest": document["source_digest"],
        "evidence_root": document["evidence_root"],
        "artifact": document.get("artifact"),
        "target_evidence": document.get("target_evidence"),
        "boot_id": document["boot_id"],
        "group": document["group"],
        "status": document["status"],
        "scripts": document["scripts"],
        "logs": document["logs"],
    }


def command_result(args: argparse.Namespace) -> int:
    """Write a content-addressed transport result."""

    source = require_tagged_digest(args.source_digest, "result source_digest")
    evidence_root = args.evidence_root.resolve(strict=True)
    if not evidence_root.is_dir():
        raise EvidenceError(
            f"transport evidence root is not a directory: {evidence_root}"
        )
    output_parent = args.output.parent.resolve(strict=True)
    try:
        output_parent.relative_to(evidence_root)
    except ValueError as exc:
        raise EvidenceError(
            f"transport result output escapes evidence root: {args.output}"
        ) from exc
    serialized_evidence_root = os.path.relpath(
        evidence_root,
        start=output_parent,
    )

    def evidence_relative(path: Path, label: str) -> tuple[Path, str]:
        resolved = require_file(path, label)
        try:
            relative = resolved.relative_to(evidence_root).as_posix()
        except ValueError as exc:
            raise EvidenceError(
                f"{label} escapes transport evidence root: {resolved}"
            ) from exc
        return resolved, relative

    boot_id = args.boot_id
    if args.target == "qemu":
        if args.claim_tier not in {QEMU_INTEGRATION_TIER, QEMU_DIAGNOSTIC_TIER}:
            raise EvidenceError(
                "QEMU results must use qemu-integration or qemu-diagnostic tier"
            )
        if args.artifact_manifest is None:
            raise EvidenceError("QEMU result requires --artifact-manifest")
        if not args.artifact_action_id or not args.artifact_catalog_action_digest:
            raise EvidenceError(
                "QEMU result requires artifact action ID and catalog digest"
            )
        artifact = verify_artifact_document(
            args.artifact_manifest,
            expected_source_digest=source,
            expected_action_id=args.artifact_action_id,
            expected_catalog_action_digest=args.artifact_catalog_action_digest,
        )
        artifact_claim = artifact["qemu"]["claim"]
        if artifact_claim["tier"] != args.claim_tier:
            raise EvidenceError(
                "QEMU result tier does not match the immutable launch record: "
                f"expected {artifact_claim['tier']}, got {args.claim_tier}"
            )
        if (
            args.claim_tier == QEMU_INTEGRATION_TIER
            and artifact_claim["eligible"] is not True
        ):
            raise EvidenceError("claim-ineligible QEMU launch cannot produce integration PASS")
        artifact_record: dict[str, Any] | None = {
            "artifact_id": artifact["artifact_id"],
            "action_id": artifact["action_id"],
            "catalog_action_digest": artifact["catalog_action_digest"],
            "manifest_sha256": sha256_file(args.artifact_manifest),
            "claim_tier": artifact_claim["tier"],
            "claim_eligible": artifact_claim["eligible"],
        }
        try:
            artifact_manifest = require_file(
                args.artifact_manifest,
                "QEMU artifact manifest",
            )
            artifact_record["manifest_path"] = artifact_manifest.relative_to(
                evidence_root
            ).as_posix()
        except ValueError:
            if artifact["action_id"] == args.action_id:
                raise EvidenceError(
                    "same-action QEMU artifact manifest must be inside "
                    "the transport evidence root"
                )
        target_record = None
        if args.target_evidence is not None:
            if args.claim_tier != QEMU_INTEGRATION_TIER:
                raise EvidenceError(
                    "qemu-diagnostic results cannot consume integration target evidence"
                )
            target_document = verify_qemu_target_evidence(
                args.target_evidence,
                expected_source_digest=source,
                expected_artifact_id=str(artifact["artifact_id"]),
            )
            evidence_boot_id = str(
                evidence_lookup(
                    target_document,
                    (("boot_id",), ("boot", "id")),
                )
            )
            if boot_id is not None and boot_id != evidence_boot_id:
                raise EvidenceError(
                    "QEMU result boot_id does not match target evidence"
                )
            boot_id = evidence_boot_id
            target_path, target_relative = evidence_relative(
                args.target_evidence,
                "QEMU target evidence",
            )
            target_record = {
                "path": target_relative,
                "evidence_sha256": sha256_file(target_path),
                "upstream_claim_tier": QEMU_INTEGRATION_TIER,
            }
        elif boot_id is None or not boot_id.strip():
            raise EvidenceError("QEMU result boot_id must not be empty")
    else:
        if args.claim_tier != "pi4-transport":
            raise EvidenceError(
                "Stage transport results for Pi 4 must use pi4-transport; "
                "hardware acceptance is a separate claim"
            )
        if args.target_evidence is None:
            raise EvidenceError("Pi 4 result requires --target-evidence")
        target_document = verify_pi4_transport_evidence(
            args.target_evidence,
            expected_source_digest=source,
        )
        evidence_boot_id = str(
            evidence_lookup(
                target_document,
                (("boot_id",), ("boot", "id")),
            )
        )
        if boot_id is not None and boot_id != evidence_boot_id:
            raise EvidenceError(
                "Pi 4 result boot_id does not match target evidence"
            )
        boot_id = evidence_boot_id
        artifact_record = None
        target_path, target_relative = evidence_relative(
            args.target_evidence,
            "Pi 4 target evidence",
        )
        target_record = {
            "path": target_relative,
            "evidence_sha256": sha256_file(target_path),
            "upstream_claim_tier": evidence_lookup(
                target_document,
                (("claim_tier",), ("claim", "tier"), ("tier",)),
            ),
        }

    log_records: list[dict[str, Any]] = []
    for log in sorted(args.log, key=lambda value: str(value)):
        path, relative = evidence_relative(log, "transport result log")
        log_records.append(
            {
                "name": path.name,
                "path": relative,
                "sha256": sha256_file(path),
                "size": path.stat().st_size,
            }
        )

    scripts = sorted(set(args.script))
    if not scripts:
        raise EvidenceError("transport result must name at least one script")
    if not log_records:
        raise EvidenceError("transport result must bind at least one log")
    if boot_id is None or not boot_id.strip():
        raise EvidenceError("transport result boot_id must not be empty")

    document: dict[str, Any] = {
        "schema": RESULT_SCHEMA,
        "action_id": args.action_id,
        "catalog_action_digest": require_tagged_digest(
            args.catalog_action_digest,
            "result catalog_action_digest",
        ),
        "claim_tier": args.claim_tier,
        "target": args.target,
        "source_digest": source,
        "evidence_root": serialized_evidence_root,
        "artifact": artifact_record,
        "target_evidence": target_record,
        "boot_id": boot_id,
        "group": args.group,
        "status": args.status,
        "scripts": scripts,
        "logs": log_records,
    }
    document["result_id"] = sha256_bytes(
        canonical_bytes(result_identity_material(document))
    )
    atomic_write_json(args.output, document)
    print(document["result_id"])
    return 0


def verify_result_document(
    path: Path,
    *,
    expected_source_digest: str,
    expected_target: str,
    expected_tier: str,
    expected_action_id: str,
    expected_catalog_action_digest: str,
    expected_evidence_root: Path,
) -> dict[str, Any]:
    """Validate one transport result for aggregation."""

    document = read_json(require_file(path, "transport result"))
    if document.get("schema") != RESULT_SCHEMA:
        raise EvidenceError(f"unsupported transport result schema in {path}")
    require_tagged_digest(
        str(document.get("catalog_action_digest", "")),
        f"transport result catalog_action_digest in {path}",
    )
    if document.get("source_digest") != expected_source_digest:
        raise EvidenceError(f"transport result source mismatch in {path}")
    if document.get("action_id") != expected_action_id:
        raise EvidenceError(f"transport result action mismatch in {path}")
    if document.get("catalog_action_digest") != expected_catalog_action_digest:
        raise EvidenceError(f"transport result catalog digest mismatch in {path}")
    if document.get("target") != expected_target:
        raise EvidenceError(f"transport result target mismatch in {path}")
    if document.get("claim_tier") != expected_tier:
        raise EvidenceError(f"transport result tier mismatch in {path}")
    if document.get("status") != "pass":
        raise EvidenceError(f"transport result is not PASS: {path}")
    evidence_root_text = document.get("evidence_root")
    if not isinstance(evidence_root_text, str) or not evidence_root_text:
        raise EvidenceError(f"transport result has no evidence root: {path}")
    evidence_root_value = Path(evidence_root_text)
    if evidence_root_value.is_absolute():
        raise EvidenceError(f"transport evidence root must be relative: {path}")
    evidence_root = (path.parent / evidence_root_value).resolve(strict=True)
    expected_root = expected_evidence_root.resolve(strict=True)
    if evidence_root != expected_root:
        raise EvidenceError(
            f"transport result evidence root mismatch in {path}: "
            f"expected {expected_root}, got {evidence_root}"
        )
    try:
        path.resolve(strict=True).relative_to(evidence_root)
    except ValueError as exc:
        raise EvidenceError(
            f"transport result escapes its evidence root: {path}"
        ) from exc

    logs = document.get("logs")
    if not isinstance(logs, list) or not logs:
        raise EvidenceError(f"transport result has no log records: {path}")
    for record in logs:
        if not isinstance(record, Mapping):
            raise EvidenceError(f"transport log record is invalid in {path}")
        relative = str(record.get("path", ""))
        log = safe_relative_file(evidence_root, relative)
        if log.stat().st_size != record.get("size"):
            raise EvidenceError(f"transport log size mismatch: {relative}")
        if sha256_file(log) != record.get("sha256"):
            raise EvidenceError(f"transport log hash mismatch: {relative}")

    target_record = document.get("target_evidence")
    if target_record is not None:
        if not isinstance(target_record, Mapping):
            raise EvidenceError(f"target evidence record is invalid in {path}")
        target_path = safe_relative_file(
            evidence_root,
            str(target_record.get("path", "")),
        )
        if sha256_file(target_path) != target_record.get("evidence_sha256"):
            raise EvidenceError(f"target evidence hash mismatch in {path}")

    artifact_record = document.get("artifact")
    if expected_target == "qemu" and not isinstance(artifact_record, Mapping):
        raise EvidenceError(f"QEMU result has no artifact binding in {path}")
    if artifact_record is not None:
        if not isinstance(artifact_record, Mapping):
            raise EvidenceError(f"artifact record is invalid in {path}")
        if expected_target == "qemu":
            expected_eligibility = expected_tier == QEMU_INTEGRATION_TIER
            if (
                artifact_record.get("claim_tier") != expected_tier
                or artifact_record.get("claim_eligible") is not expected_eligibility
            ):
                raise EvidenceError(
                    f"artifact claim classification mismatch in {path}"
                )
        manifest_relative = artifact_record.get("manifest_path")
        if artifact_record.get("action_id") == expected_action_id:
            if not isinstance(manifest_relative, str) or not manifest_relative:
                raise EvidenceError(
                    f"same-action artifact path is missing in {path}"
                )
        if isinstance(manifest_relative, str) and manifest_relative:
            artifact_manifest = safe_relative_file(
                evidence_root,
                manifest_relative,
            )
            if sha256_file(artifact_manifest) != artifact_record.get(
                "manifest_sha256"
            ):
                raise EvidenceError(f"artifact manifest hash mismatch in {path}")
            artifact = verify_artifact_document(
                artifact_manifest,
                expected_source_digest=expected_source_digest,
                expected_action_id=str(artifact_record.get("action_id", "")),
                expected_catalog_action_digest=str(
                    artifact_record.get("catalog_action_digest", "")
                ),
            )
            if artifact.get("artifact_id") != artifact_record.get("artifact_id"):
                raise EvidenceError(f"artifact ID mismatch in {path}")
            artifact_claim = artifact["qemu"]["claim"]
            if (
                artifact_claim.get("tier") != artifact_record.get("claim_tier")
                or artifact_claim.get("eligible")
                is not artifact_record.get("claim_eligible")
            ):
                raise EvidenceError(f"artifact launch claim mismatch in {path}")
    expected_id = sha256_bytes(canonical_bytes(result_identity_material(document)))
    if document.get("result_id") != expected_id:
        raise EvidenceError(f"transport result ID mismatch in {path}")
    return document


def command_aggregate(args: argparse.Namespace) -> int:
    """Aggregate a complete set of passing transport results."""

    source = require_tagged_digest(args.source_digest, "aggregate source_digest")
    records: list[dict[str, Any]] = []
    groups: set[str] = set()
    for result_path in args.result:
        document = verify_result_document(
            result_path,
            expected_source_digest=source,
            expected_target=args.target,
            expected_tier=args.claim_tier,
            expected_action_id=args.action_id,
            expected_catalog_action_digest=args.catalog_action_digest,
            expected_evidence_root=args.evidence_root,
        )
        group = str(document["group"])
        if group in groups:
            raise EvidenceError(f"duplicate transport result group: {group}")
        groups.add(group)
        records.append(
            {
                "group": group,
                "result_id": document["result_id"],
                "sha256": sha256_file(result_path),
            }
        )
    if not records:
        raise EvidenceError("transport aggregate requires at least one result")
    records.sort(key=lambda record: str(record["group"]))
    document: dict[str, Any] = {
        "schema": AGGREGATE_SCHEMA,
        "action_id": args.action_id,
        "catalog_action_digest": require_tagged_digest(
            args.catalog_action_digest,
            "aggregate catalog_action_digest",
        ),
        "claim_tier": args.claim_tier,
        "target": args.target,
        "source_digest": source,
        "results": records,
    }
    identity_material = {
        key: document[key]
        for key in (
            "schema",
            "action_id",
            "catalog_action_digest",
            "claim_tier",
            "target",
            "source_digest",
            "results",
        )
    }
    document["aggregate_id"] = sha256_bytes(canonical_bytes(identity_material))
    atomic_write_json(args.output, document)
    print(document["aggregate_id"])
    return 0


def command_verify_result(args: argparse.Namespace) -> int:
    """Verify one passing content-addressed transport result."""

    source = require_tagged_digest(args.source_digest, "result source_digest")
    catalog_digest = require_tagged_digest(
        args.catalog_action_digest,
        "result catalog_action_digest",
    )
    document = verify_result_document(
        args.result,
        expected_source_digest=source,
        expected_target=args.target,
        expected_tier=args.claim_tier,
        expected_action_id=args.action_id,
        expected_catalog_action_digest=catalog_digest,
        expected_evidence_root=args.evidence_root,
    )
    print(document["result_id"])
    return 0


def command_verify_pi4_continuity(args: argparse.Namespace) -> int:
    """Bind REST transport evidence to the exact Stage 03 Pi boot evidence."""

    source = require_tagged_digest(
        args.source_digest,
        "Pi 4 continuity source_digest",
    )
    catalog_digest = require_tagged_digest(
        args.prior_catalog_action_digest,
        "prior catalog_action_digest",
    )
    target = verify_pi4_transport_evidence(
        args.target_evidence,
        expected_source_digest=source,
    )
    prior = verify_result_document(
        args.prior_result,
        expected_source_digest=source,
        expected_target="pi4",
        expected_tier="pi4-transport",
        expected_action_id=args.prior_action_id,
        expected_catalog_action_digest=catalog_digest,
        expected_evidence_root=args.prior_evidence_root,
    )
    target_record = prior.get("target_evidence")
    if not isinstance(target_record, Mapping):
        raise EvidenceError("prior Pi 4 result has no target evidence binding")
    if target_record.get("evidence_sha256") != sha256_file(args.target_evidence):
        raise EvidenceError(
            "Pi 4 target evidence changed between Stage 03 and Stage 04"
        )
    boot_id = str(
        evidence_lookup(
            target,
            (("boot_id",), ("boot", "id")),
        )
    )
    if prior.get("boot_id") != boot_id:
        raise EvidenceError("Pi 4 boot changed between Stage 03 and Stage 04")
    verify_gateway_binding(target, required_gateway_url(args))
    print(boot_id)
    return 0


def command_verify_qemu_target(args: argparse.Namespace) -> int:
    """Verify an external QEMU boot against a recorded artifact."""

    source = require_tagged_digest(
        args.source_digest,
        "QEMU target source_digest",
    )
    catalog_digest = require_tagged_digest(
        args.artifact_catalog_action_digest,
        "artifact catalog_action_digest",
    )
    artifact = verify_artifact_document(
        args.artifact_manifest,
        expected_source_digest=source,
        expected_action_id=args.artifact_action_id,
        expected_catalog_action_digest=catalog_digest,
    )
    if artifact["qemu"]["claim"] != {
        "eligible": True,
        "tier": QEMU_INTEGRATION_TIER,
        "reason": "canonical production envelope",
    }:
        raise EvidenceError(
            "external QEMU integration evidence requires a claim-eligible launch"
        )
    target = verify_qemu_target_evidence(
        args.target_evidence,
        expected_source_digest=source,
        expected_artifact_id=str(artifact["artifact_id"]),
    )
    verify_gateway_binding(target, required_gateway_url(args))
    print(
        evidence_lookup(
            target,
            (("boot_id",), ("boot", "id")),
        )
    )
    return 0


def command_verify_aggregate(args: argparse.Namespace) -> int:
    """Verify an aggregate and every content-addressed result it references."""

    source = require_tagged_digest(
        args.source_digest,
        "aggregate source_digest",
    )
    catalog_digest = require_tagged_digest(
        args.catalog_action_digest,
        "aggregate catalog_action_digest",
    )
    document = read_json(require_file(args.aggregate, "transport aggregate"))
    if document.get("schema") != AGGREGATE_SCHEMA:
        raise EvidenceError("unsupported transport aggregate schema")
    expected_fields = {
        "action_id": args.action_id,
        "catalog_action_digest": catalog_digest,
        "claim_tier": args.claim_tier,
        "target": args.target,
        "source_digest": source,
    }
    for field, expected in expected_fields.items():
        if document.get(field) != expected:
            raise EvidenceError(
                f"transport aggregate {field} mismatch: "
                f"expected {expected}, got {document.get(field)}"
            )
    identity_material = {
        key: document[key]
        for key in (
            "schema",
            "action_id",
            "catalog_action_digest",
            "claim_tier",
            "target",
            "source_digest",
            "results",
        )
    }
    expected_id = sha256_bytes(canonical_bytes(identity_material))
    if document.get("aggregate_id") != expected_id:
        raise EvidenceError("transport aggregate ID mismatch")

    result_root = args.result_root.resolve(strict=True)
    if not result_root.is_dir():
        raise EvidenceError(f"transport result root is not a directory: {result_root}")
    records = document.get("results")
    if not isinstance(records, list) or not records:
        raise EvidenceError("transport aggregate has no results")
    observed_groups: set[str] = set()
    for record in records:
        if not isinstance(record, Mapping):
            raise EvidenceError("transport aggregate result must be an object")
        group = str(record.get("group", ""))
        if not group or "/" in group or group in {".", ".."}:
            raise EvidenceError(f"unsafe transport result group: {group!r}")
        if group in observed_groups:
            raise EvidenceError(f"duplicate transport aggregate group: {group}")
        observed_groups.add(group)
        result_path = result_root / f"{group}.json"
        if sha256_file(require_file(result_path, "transport result")) != record.get(
            "sha256"
        ):
            raise EvidenceError(f"transport aggregate result hash mismatch: {group}")
        result = verify_result_document(
            result_path,
            expected_source_digest=source,
            expected_target=args.target,
            expected_tier=args.claim_tier,
            expected_action_id=args.action_id,
            expected_catalog_action_digest=catalog_digest,
            expected_evidence_root=args.evidence_root,
        )
        if result.get("result_id") != record.get("result_id"):
            raise EvidenceError(f"transport aggregate result ID mismatch: {group}")
    expected_groups = set(args.expected_group)
    if expected_groups and observed_groups != expected_groups:
        raise EvidenceError(
            "transport aggregate groups mismatch: "
            f"expected={sorted(expected_groups)} "
            f"observed={sorted(observed_groups)}"
        )
    print(document["aggregate_id"])
    return 0


def build_parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""

    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    digest_parser = subparsers.add_parser(
        "source-digest",
        help="hash the current tracked and non-ignored source tree",
    )
    digest_parser.add_argument("--repo-root", type=Path, required=True)
    digest_parser.set_defaults(function=command_source_digest)

    copy_parser = subparsers.add_parser(
        "copy-evidence",
        help="copy validated JSON evidence into an attempt tree",
    )
    copy_parser.add_argument("--source", type=Path, required=True)
    copy_parser.add_argument("--output", type=Path, required=True)
    copy_parser.set_defaults(function=command_copy_evidence)

    pi_record_parser = subparsers.add_parser(
        "record-pi4-evidence",
        help="write validated Pi 4 transport target evidence",
    )
    pi_record_parser.add_argument("--output", type=Path, required=True)
    pi_record_parser.add_argument("--source-digest", required=True)
    pi_record_parser.add_argument("--boot-id", required=True)
    pi_record_parser.add_argument("--image-identity", required=True)
    pi_record_parser.add_argument("--target-host", required=True)
    pi_record_parser.add_argument("--gateway-url")
    pi_record_parser.add_argument("--gateway-target-host")
    pi_record_parser.set_defaults(function=command_record_pi4_evidence)

    pi_verify_parser = subparsers.add_parser(
        "verify-pi4-evidence",
        help="validate Pi 4 evidence before a live transport run",
    )
    pi_verify_parser.add_argument(
        "--target-evidence",
        type=Path,
        required=True,
    )
    pi_verify_parser.add_argument("--source-digest", required=True)
    pi_verify_parser.add_argument("--gateway-url")
    pi_verify_parser.set_defaults(function=command_verify_pi4_evidence)

    publish_parser = subparsers.add_parser(
        "publish-root",
        help="publish an immutable attempt root as current",
    )
    publish_parser.add_argument("--state-dir", type=Path, required=True)
    publish_parser.add_argument("--root", type=Path, required=True)
    publish_parser.add_argument("--pointer", type=Path, required=True)
    publish_parser.add_argument("--compat-link", type=Path)
    publish_parser.add_argument("--compat-target", type=Path)
    publish_parser.set_defaults(function=command_publish_root)

    resolve_parser = subparsers.add_parser(
        "resolve-root",
        help="resolve a confined state-relative artifact pointer",
    )
    resolve_parser.add_argument("--state-dir", type=Path, required=True)
    resolve_parser.add_argument("--pointer", type=Path, required=True)
    resolve_parser.set_defaults(function=command_resolve_root)

    record_parser = subparsers.add_parser(
        "record",
        help="record a completed QEMU artifact",
    )
    record_parser.add_argument("--artifact-dir", type=Path, required=True)
    record_parser.add_argument("--output", type=Path, required=True)
    record_parser.add_argument("--manifest", type=Path, required=True)
    record_parser.add_argument("--resolved-manifest", type=Path, required=True)
    record_parser.add_argument("--policy", type=Path, required=True)
    record_parser.add_argument("--source-digest", required=True)
    record_parser.add_argument("--attempt-manifest", type=Path)
    record_parser.add_argument("--sel4-build", type=Path, required=True)
    record_parser.add_argument("--sel4-profile", required=True)
    record_parser.add_argument(
        "--qemu",
        default=os.environ.get("QEMU_BIN", "qemu-system-aarch64"),
    )
    record_parser.add_argument(
        "--accelerator",
        choices=("hvf", "kvm", "tcg"),
    )
    record_parser.add_argument("--machine", default=CANONICAL_MACHINE)
    record_parser.add_argument("--cpu", default=CANONICAL_CPU)
    record_parser.add_argument("--cargo-profile", default="release")
    record_parser.add_argument("--root-task-features", required=True)
    record_parser.add_argument("--cargo-target", required=True)
    record_parser.add_argument("--smp", required=True)
    record_parser.add_argument(
        "--virtualization",
        choices=("on", "off"),
        required=True,
    )
    record_parser.add_argument("--machine-extra", default="")
    record_parser.add_argument(
        "--net-backend",
        choices=("virtio", "rtl8139"),
        required=True,
    )
    record_parser.add_argument("--detect-gic-script", type=Path, required=True)
    record_parser.add_argument("--action-id", required=True)
    record_parser.add_argument("--catalog-action-digest", required=True)
    record_parser.set_defaults(function=command_record)

    verify_parser = subparsers.add_parser(
        "verify",
        help="verify a recorded QEMU artifact",
    )
    verify_parser.add_argument("--artifact-manifest", type=Path, required=True)
    verify_parser.add_argument("--source-digest")
    verify_parser.add_argument("--action-id")
    verify_parser.add_argument("--catalog-action-digest")
    verify_parser.set_defaults(function=command_verify)

    launch_parser = subparsers.add_parser(
        "launch",
        help="launch a fresh QEMU boot from a verified artifact",
    )
    launch_parser.add_argument("--artifact-manifest", type=Path, required=True)
    launch_parser.add_argument("--source-digest")
    launch_parser.add_argument("--action-id")
    launch_parser.add_argument("--catalog-action-digest")
    launch_parser.add_argument(
        "--qemu",
        default=os.environ.get("QEMU_BIN", "qemu-system-aarch64"),
    )
    launch_parser.add_argument("--console-port", type=int, required=True)
    launch_parser.add_argument("--udp-port", type=int, required=True)
    launch_parser.add_argument("--smoke-port", type=int, required=True)
    launch_parser.add_argument("--print-command", action="store_true")
    launch_parser.set_defaults(function=command_launch)

    result_parser = subparsers.add_parser(
        "result",
        help="write one content-addressed transport result",
    )
    result_parser.add_argument("--output", type=Path, required=True)
    result_parser.add_argument("--action-id", required=True)
    result_parser.add_argument("--catalog-action-digest", required=True)
    result_parser.add_argument(
        "--claim-tier",
        choices=(QEMU_INTEGRATION_TIER, QEMU_DIAGNOSTIC_TIER, "pi4-transport"),
        required=True,
    )
    result_parser.add_argument("--target", choices=("qemu", "pi4"), required=True)
    result_parser.add_argument("--source-digest", required=True)
    result_parser.add_argument("--evidence-root", type=Path, required=True)
    result_parser.add_argument("--artifact-manifest", type=Path)
    result_parser.add_argument("--artifact-action-id")
    result_parser.add_argument("--artifact-catalog-action-digest")
    result_parser.add_argument("--target-evidence", type=Path)
    result_parser.add_argument("--boot-id")
    result_parser.add_argument("--group", required=True)
    result_parser.add_argument(
        "--status",
        choices=("pass", "fail"),
        required=True,
    )
    result_parser.add_argument("--script", action="append", default=[])
    result_parser.add_argument("--log", type=Path, action="append", default=[])
    result_parser.set_defaults(function=command_result)

    aggregate_parser = subparsers.add_parser(
        "aggregate",
        help="aggregate passing transport results",
    )
    aggregate_parser.add_argument("--output", type=Path, required=True)
    aggregate_parser.add_argument("--action-id", required=True)
    aggregate_parser.add_argument("--catalog-action-digest", required=True)
    aggregate_parser.add_argument(
        "--claim-tier",
        choices=(QEMU_INTEGRATION_TIER, QEMU_DIAGNOSTIC_TIER, "pi4-transport"),
        required=True,
    )
    aggregate_parser.add_argument(
        "--target",
        choices=("qemu", "pi4"),
        required=True,
    )
    aggregate_parser.add_argument("--source-digest", required=True)
    aggregate_parser.add_argument("--evidence-root", type=Path, required=True)
    aggregate_parser.add_argument(
        "--result",
        type=Path,
        action="append",
        default=[],
    )
    aggregate_parser.set_defaults(function=command_aggregate)

    result_verify_parser = subparsers.add_parser(
        "verify-result",
        help="verify one passing transport result",
    )
    result_verify_parser.add_argument("--result", type=Path, required=True)
    result_verify_parser.add_argument("--action-id", required=True)
    result_verify_parser.add_argument(
        "--catalog-action-digest",
        required=True,
    )
    result_verify_parser.add_argument(
        "--claim-tier",
        choices=(QEMU_INTEGRATION_TIER, QEMU_DIAGNOSTIC_TIER, "pi4-transport"),
        required=True,
    )
    result_verify_parser.add_argument(
        "--target",
        choices=("qemu", "pi4"),
        required=True,
    )
    result_verify_parser.add_argument("--source-digest", required=True)
    result_verify_parser.add_argument(
        "--evidence-root",
        type=Path,
        required=True,
    )
    result_verify_parser.set_defaults(function=command_verify_result)

    continuity_parser = subparsers.add_parser(
        "verify-pi4-continuity",
        help="bind Stage 04 to the exact Stage 03 Pi target evidence",
    )
    continuity_parser.add_argument(
        "--target-evidence",
        type=Path,
        required=True,
    )
    continuity_parser.add_argument(
        "--prior-result",
        type=Path,
        required=True,
    )
    continuity_parser.add_argument("--source-digest", required=True)
    continuity_parser.add_argument(
        "--prior-evidence-root",
        type=Path,
        required=True,
    )
    continuity_parser.add_argument("--prior-action-id", required=True)
    continuity_parser.add_argument(
        "--prior-catalog-action-digest",
        required=True,
    )
    continuity_parser.add_argument("--gateway-url")
    continuity_parser.set_defaults(function=command_verify_pi4_continuity)

    qemu_target_parser = subparsers.add_parser(
        "verify-qemu-target",
        help="bind an external QEMU boot to a recorded artifact",
    )
    qemu_target_parser.add_argument(
        "--target-evidence",
        type=Path,
        required=True,
    )
    qemu_target_parser.add_argument(
        "--artifact-manifest",
        type=Path,
        required=True,
    )
    qemu_target_parser.add_argument("--source-digest", required=True)
    qemu_target_parser.add_argument("--artifact-action-id", required=True)
    qemu_target_parser.add_argument(
        "--artifact-catalog-action-digest",
        required=True,
    )
    qemu_target_parser.add_argument("--gateway-url")
    qemu_target_parser.set_defaults(function=command_verify_qemu_target)

    aggregate_verify_parser = subparsers.add_parser(
        "verify-aggregate",
        help="verify an aggregate and all referenced results",
    )
    aggregate_verify_parser.add_argument(
        "--aggregate",
        type=Path,
        required=True,
    )
    aggregate_verify_parser.add_argument(
        "--result-root",
        type=Path,
        required=True,
    )
    aggregate_verify_parser.add_argument("--action-id", required=True)
    aggregate_verify_parser.add_argument(
        "--catalog-action-digest",
        required=True,
    )
    aggregate_verify_parser.add_argument(
        "--claim-tier",
        choices=(QEMU_INTEGRATION_TIER, QEMU_DIAGNOSTIC_TIER, "pi4-transport"),
        required=True,
    )
    aggregate_verify_parser.add_argument(
        "--target",
        choices=("qemu", "pi4"),
        required=True,
    )
    aggregate_verify_parser.add_argument("--source-digest", required=True)
    aggregate_verify_parser.add_argument(
        "--evidence-root",
        type=Path,
        required=True,
    )
    aggregate_verify_parser.add_argument(
        "--expected-group",
        action="append",
        default=[],
    )
    aggregate_verify_parser.set_defaults(function=command_verify_aggregate)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """Execute the selected subcommand."""

    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return int(args.function(args))
    except EvidenceError as exc:
        print(f"qemu-artifact: error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
