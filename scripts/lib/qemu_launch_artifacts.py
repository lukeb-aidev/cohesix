#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Bind and verify the immutable QEMU launch artifact set for repeated runs.
# Copyright 2026 Lukas Bower

"""Write and verify the exact Cohesix artifacts consumed by QEMU."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import tempfile
from typing import Any


SCHEMA = "cohesix-qemu-launch-artifacts/v2"
RECORD_NAME = "cohesix-qemu-launch-artifacts.json"
ARTIFACTS = (
    ("elfloader", Path("staging/elfloader")),
    ("kernel", Path("staging/kernel.elf")),
    ("rootserver", Path("staging/rootserver")),
    ("initrd", Path("cohesix-system.cpio")),
)
SHA256_HEX_LEN = 64
CANONICAL_SEL4_PROFILE = "qemu_smp_production"
CANONICAL_MACHINE = "virt"
CANONICAL_GIC_VERSION = "3"
CANONICAL_VIRTUALIZATION = "off"
CANONICAL_MACHINE_EXTRA = "kernel-irqchip=off"
CANONICAL_CPU = "cortex-a57"
CANONICAL_TIMER_CLOCK_HZ = 24_000_000
CANONICAL_SMP = "4,cores=4,threads=1,sockets=1"
CANONICAL_NET_BACKEND = "virtio"
TIMER_HEADERS = (
    Path("kernel/gen_headers/plat/platform_gen.h"),
    Path("kernel/gen_headers/plat/machine/devices_gen.h"),
)


class LaunchArtifactError(ValueError):
    """Raised when the immutable QEMU artifact record is invalid."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_regular_file(out_dir: Path, relative: Path) -> Path:
    if relative.is_absolute() or ".." in relative.parts:
        raise LaunchArtifactError(f"artifact path is not bounded: {relative}")

    current = out_dir
    for component in relative.parts:
        current = current / component
        try:
            metadata = current.lstat()
        except FileNotFoundError as error:
            raise LaunchArtifactError(
                f"launch artifact is missing: {current}"
            ) from error
        if stat.S_ISLNK(metadata.st_mode):
            raise LaunchArtifactError(f"launch artifact path is a symlink: {current}")

    metadata = current.stat()
    if not stat.S_ISREG(metadata.st_mode):
        raise LaunchArtifactError(f"launch artifact is not a regular file: {current}")
    if metadata.st_size <= 0:
        raise LaunchArtifactError(f"launch artifact is empty: {current}")
    return current


def _resolve_executable(value: str) -> Path:
    """Resolve one executable without accepting PATH or symlink drift."""

    candidate = shutil.which(value) if os.sep not in value else value
    if candidate is None:
        raise LaunchArtifactError(f"QEMU binary is not on PATH: {value}")
    try:
        resolved = Path(candidate).resolve(strict=True)
    except OSError as error:
        raise LaunchArtifactError(f"QEMU binary is missing: {value}") from error
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise LaunchArtifactError(f"QEMU binary is not executable: {resolved}")
    return resolved


def _qemu_version(binary: Path) -> str:
    try:
        completed = subprocess.run(
            [str(binary), "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise LaunchArtifactError(
            f"cannot execute QEMU binary {binary}: {error}"
        ) from error
    first_line = completed.stdout.splitlines()[:1]
    if completed.returncode != 0 or not first_line or not first_line[0].strip():
        raise LaunchArtifactError(f"cannot determine QEMU version from {binary}")
    return first_line[0].strip()


def _qemu_identity(value: str) -> dict[str, Any]:
    binary = _resolve_executable(value)
    return {
        "path": str(binary),
        "bytes": binary.stat().st_size,
        "sha256": _sha256(binary),
        "version": _qemu_version(binary),
    }


def _require_qemu_accelerator(value: str, accelerator: str) -> None:
    binary = _resolve_executable(value)
    try:
        completed = subprocess.run(
            [str(binary), "-accel", "help"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise LaunchArtifactError(
            f"cannot inspect QEMU accelerators from {binary}: {error}"
        ) from error
    advertised = f"{completed.stdout}\n{completed.stderr}".split()
    if accelerator not in advertised:
        raise LaunchArtifactError(
            f"QEMU accelerator {accelerator!r} is not advertised by {binary}"
        )


def _timer_clock_hz(sel4_build_dir: Path) -> int:
    timer_pattern = re.compile(
        r"^\s*#define\s+TIMER_CLOCK_HZ\s+"
        r"(?:ULL_CONST\(\s*)?([0-9]+)(?:\s*\))?\s*$",
        flags=re.MULTILINE,
    )
    for relative in TIMER_HEADERS:
        header = sel4_build_dir / relative
        if not header.is_file():
            continue
        try:
            matches = timer_pattern.findall(header.read_text(encoding="utf-8"))
        except (OSError, UnicodeError) as error:
            raise LaunchArtifactError(
                f"cannot read selected seL4 timer header {header}: {error}"
            ) from error
        if len(matches) != 1:
            raise LaunchArtifactError(
                "selected seL4 timer header must define TIMER_CLOCK_HZ exactly once"
            )
        return int(matches[0])
    raise LaunchArtifactError(
        f"cannot find selected seL4 timer header under {sel4_build_dir}"
    )


def _claim(
    *,
    host_system: str,
    accelerator: str,
    sel4_profile: str,
    timer_clock_hz: int,
    gic_version: str,
    virtualization: str,
    machine_extra: str,
    cpu: str,
    smp: str,
    net_backend: str,
) -> dict[str, Any]:
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
        "tier": "qemu-integration" if eligible else "qemu-diagnostic",
        "reason": "canonical production envelope" if eligible else "; ".join(reasons),
    }


def _context(
    *,
    out_dir: Path,
    sel4_build_dir: Path,
    profile: str,
    cargo_target: str,
    root_task_features: str,
    gic_version: str,
    sel4_profile: str,
    qemu: str,
    accelerator: str,
    virtualization: str,
    machine_extra: str,
    cpu: str,
    smp: str,
    net_backend: str,
) -> dict[str, Any]:
    if not out_dir.is_absolute() or out_dir != out_dir.resolve(strict=True):
        raise LaunchArtifactError("output directory must be an existing resolved path")
    if not sel4_build_dir.is_absolute() or not sel4_build_dir.is_dir():
        raise LaunchArtifactError(
            "seL4 build directory must be an existing absolute path"
        )
    if not profile:
        raise LaunchArtifactError("Cargo profile must not be empty")
    if not cargo_target:
        raise LaunchArtifactError("Cargo target must not be empty")
    if not sel4_profile:
        raise LaunchArtifactError("selected seL4 profile must not be empty")
    if accelerator not in {"hvf", "kvm", "tcg"}:
        raise LaunchArtifactError(f"unsupported QEMU accelerator: {accelerator}")
    if virtualization not in {"on", "off"}:
        raise LaunchArtifactError(
            f"unsupported QEMU virtualization setting: {virtualization}"
        )
    if not smp:
        raise LaunchArtifactError("QEMU SMP topology must not be empty")
    if net_backend not in {"virtio", "rtl8139"}:
        raise LaunchArtifactError(f"unsupported QEMU network backend: {net_backend}")
    _require_qemu_accelerator(qemu, accelerator)
    timer_clock_hz = _timer_clock_hz(sel4_build_dir)
    host_system = platform.system()
    return {
        "schema": SCHEMA,
        "profile": profile,
        "cargo_target": cargo_target,
        "root_task_features": root_task_features,
        "sel4_build_dir": str(sel4_build_dir.resolve(strict=True)),
        "sel4_profile": sel4_profile,
        "gic_version": gic_version,
        "qemu": {
            "host_system": host_system,
            "binary": _qemu_identity(qemu),
            "accelerator": accelerator,
            "machine": CANONICAL_MACHINE,
            "virtualization": virtualization,
            "machine_extra": machine_extra,
            "cpu": cpu,
            "timer_clock_hz": timer_clock_hz,
            "smp": smp,
            "net_backend": net_backend,
        },
        "claim": _claim(
            host_system=host_system,
            accelerator=accelerator,
            sel4_profile=sel4_profile,
            timer_clock_hz=timer_clock_hz,
            gic_version=gic_version,
            virtualization=virtualization,
            machine_extra=machine_extra,
            cpu=cpu,
            smp=smp,
            net_backend=net_backend,
        ),
    }


def _artifact_rows(out_dir: Path) -> list[dict[str, Any]]:
    rows = []
    for artifact_id, relative in ARTIFACTS:
        path = _require_regular_file(out_dir, relative)
        rows.append(
            {
                "id": artifact_id,
                "path": relative.as_posix(),
                "bytes": path.stat().st_size,
                "sha256": _sha256(path),
            }
        )
    return rows


def write_record(
    *,
    out_dir: Path,
    sel4_build_dir: Path,
    profile: str,
    cargo_target: str,
    root_task_features: str,
    gic_version: str,
    sel4_profile: str,
    qemu: str,
    accelerator: str,
    virtualization: str,
    machine_extra: str,
    cpu: str,
    smp: str,
    net_backend: str,
) -> Path:
    """Atomically write the immutable QEMU launch-artifact record."""

    document = _context(
        out_dir=out_dir,
        sel4_build_dir=sel4_build_dir,
        profile=profile,
        cargo_target=cargo_target,
        root_task_features=root_task_features,
        gic_version=gic_version,
        sel4_profile=sel4_profile,
        qemu=qemu,
        accelerator=accelerator,
        virtualization=virtualization,
        machine_extra=machine_extra,
        cpu=cpu,
        smp=smp,
        net_backend=net_backend,
    )
    document["artifacts"] = _artifact_rows(out_dir)
    record = out_dir / RECORD_NAME
    encoded = (json.dumps(document, indent=2) + "\n").encode("utf-8")
    descriptor, temporary_name = tempfile.mkstemp(
        dir=out_dir,
        prefix=f".{RECORD_NAME}.",
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        temporary.chmod(0o644)
        os.replace(temporary, record)
    finally:
        temporary.unlink(missing_ok=True)
    return record


def verify_record(
    *,
    out_dir: Path,
    sel4_build_dir: Path,
    profile: str,
    cargo_target: str,
    root_task_features: str,
    gic_version: str,
    sel4_profile: str,
    qemu: str,
    accelerator: str,
    virtualization: str,
    machine_extra: str,
    cpu: str,
    smp: str,
    net_backend: str,
) -> Path:
    """Verify context and byte identity for the exact QEMU launch set."""

    expected = _context(
        out_dir=out_dir,
        sel4_build_dir=sel4_build_dir,
        profile=profile,
        cargo_target=cargo_target,
        root_task_features=root_task_features,
        gic_version=gic_version,
        sel4_profile=sel4_profile,
        qemu=qemu,
        accelerator=accelerator,
        virtualization=virtualization,
        machine_extra=machine_extra,
        cpu=cpu,
        smp=smp,
        net_backend=net_backend,
    )
    record = _require_regular_file(out_dir, Path(RECORD_NAME))
    try:
        document = json.loads(record.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise LaunchArtifactError(
            f"launch artifact record is invalid: {error}"
        ) from error

    if not isinstance(document, dict):
        raise LaunchArtifactError("launch artifact record must be a JSON object")
    exact_keys = {*expected, "artifacts"}
    if set(document) != exact_keys:
        raise LaunchArtifactError(
            "launch artifact record has unexpected or missing fields"
        )
    for key, value in expected.items():
        if document.get(key) != value:
            raise LaunchArtifactError(f"launch artifact context mismatch: {key}")

    rows = document.get("artifacts")
    actual_rows = _artifact_rows(out_dir)
    if not isinstance(rows, list) or len(rows) != len(actual_rows):
        raise LaunchArtifactError("launch artifact record has the wrong artifact count")
    for expected_row, actual_row in zip(rows, actual_rows, strict=True):
        if not isinstance(expected_row, dict) or set(expected_row) != {
            "id",
            "path",
            "bytes",
            "sha256",
        }:
            raise LaunchArtifactError("launch artifact row has an invalid shape")
        digest = expected_row.get("sha256")
        if (
            not isinstance(digest, str)
            or len(digest) != SHA256_HEX_LEN
            or any(character not in "0123456789abcdef" for character in digest)
        ):
            raise LaunchArtifactError("launch artifact row has an invalid SHA-256")
        if expected_row != actual_row:
            raise LaunchArtifactError(
                f"launch artifact identity mismatch: {actual_row['id']}"
            )
    return record


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("write", "verify"))
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--sel4-build", type=Path, required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--cargo-target", required=True)
    parser.add_argument("--root-task-features", default="")
    parser.add_argument("--gic-version", required=True)
    parser.add_argument("--sel4-profile", required=True)
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--accelerator", choices=("hvf", "kvm", "tcg"), required=True)
    parser.add_argument("--virtualization", choices=("on", "off"), required=True)
    parser.add_argument("--machine-extra", required=True)
    parser.add_argument("--cpu", required=True)
    parser.add_argument("--smp", required=True)
    parser.add_argument("--net-backend", choices=("virtio", "rtl8139"), required=True)
    return parser


def main() -> int:
    """Run the artifact record writer or verifier."""

    args = _parser().parse_args()
    operation = write_record if args.command == "write" else verify_record
    try:
        record = operation(
            out_dir=args.out_dir,
            sel4_build_dir=args.sel4_build,
            profile=args.profile,
            cargo_target=args.cargo_target,
            root_task_features=args.root_task_features,
            gic_version=args.gic_version,
            sel4_profile=args.sel4_profile,
            qemu=args.qemu,
            accelerator=args.accelerator,
            virtualization=args.virtualization,
            machine_extra=args.machine_extra,
            cpu=args.cpu,
            smp=args.smp,
            net_backend=args.net_backend,
        )
    except LaunchArtifactError as error:
        raise SystemExit(f"qemu-launch-artifacts: error: {error}") from error
    print(record)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
