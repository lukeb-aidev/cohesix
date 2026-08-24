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
CANONICAL_DARWIN_SEL4_PROFILE = "qemu_smp_production"
CANONICAL_LINUX_SEL4_PROFILE = "qemu_smp_kvm_production"
CANONICAL_MACHINE = "virt"
CANONICAL_GIC_VERSION = "3"
CANONICAL_VIRTUALIZATION = "off"
CANONICAL_DARWIN_MACHINE_EXTRA = "kernel-irqchip=off"
CANONICAL_DARWIN_CPU = "cortex-a57"
CANONICAL_LINUX_MACHINE_EXTRA = ""
CANONICAL_LINUX_CPU = "host"
PRODUCTION_PROFILE_TIMER_CLOCK_HZ = {
    CANONICAL_DARWIN_SEL4_PROFILE: 24_000_000,
    CANONICAL_LINUX_SEL4_PROFILE: 31_250_000,
}
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
    expected_sel4_profile = {
        "Darwin": CANONICAL_DARWIN_SEL4_PROFILE,
        "Linux": CANONICAL_LINUX_SEL4_PROFILE,
    }.get(host_system)
    if expected_sel4_profile is not None and sel4_profile != expected_sel4_profile:
        reasons.append("selected seL4 profile differs from the host production envelope")
    if gic_version != CANONICAL_GIC_VERSION:
        reasons.append("machine does not use GICv3")
    if virtualization != CANONICAL_VIRTUALIZATION:
        reasons.append("machine virtualization is not off")
    expected_machine_extra = {
        "Darwin": CANONICAL_DARWIN_MACHINE_EXTRA,
        "Linux": CANONICAL_LINUX_MACHINE_EXTRA,
    }.get(host_system)
    expected_cpu = {
        "Darwin": CANONICAL_DARWIN_CPU,
        "Linux": CANONICAL_LINUX_CPU,
    }.get(host_system)
    if expected_machine_extra is not None and machine_extra != expected_machine_extra:
        reasons.append("machine extra differs from the host production envelope")
    if expected_cpu is not None and cpu != expected_cpu:
        reasons.append("CPU differs from the host production envelope")
    expected_timer_clock_hz = (
        PRODUCTION_PROFILE_TIMER_CLOCK_HZ.get(expected_sel4_profile)
        if expected_sel4_profile is not None
        else None
    )
    if (
        expected_timer_clock_hz is not None
        and timer_clock_hz != expected_timer_clock_hz
    ):
        reasons.append("selected seL4 timer differs from the host production envelope")
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


def verify_artifact_identity(out_dir: Path) -> Path:
    """Verify guest input bytes without requiring the original host context."""

    record = _require_regular_file(out_dir, Path(RECORD_NAME))
    try:
        document = json.loads(record.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise LaunchArtifactError(
            f"launch artifact record is invalid: {error}"
        ) from error
    if not isinstance(document, dict) or set(document) != {
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
    }:
        raise LaunchArtifactError("launch artifact record has an invalid shape")
    if (
        document.get("schema") != SCHEMA
        or document.get("profile") != "release"
        or document.get("cargo_target") != "aarch64-unknown-none"
        or document.get("root_task_features")
        != "release-qemu,bootstrap-trace"
        or document.get("sel4_profile") not in PRODUCTION_PROFILE_TIMER_CLOCK_HZ
        or document.get("gic_version") != CANONICAL_GIC_VERSION
    ):
        raise LaunchArtifactError("launch artifact record is not the pressure profile")
    qemu = document.get("qemu")
    if not isinstance(qemu, dict) or set(qemu) != {
        "host_system",
        "binary",
        "accelerator",
        "machine",
        "virtualization",
        "machine_extra",
        "cpu",
        "timer_clock_hz",
        "smp",
        "net_backend",
    }:
        raise LaunchArtifactError("launch artifact QEMU context has an invalid shape")
    binary = qemu.get("binary")
    if (
        qemu.get("machine") != CANONICAL_MACHINE
        or not isinstance(binary, dict)
        or set(binary) != {"path", "bytes", "sha256", "version"}
        or not isinstance(binary.get("sha256"), str)
        or len(binary["sha256"]) != SHA256_HEX_LEN
        or any(character not in "0123456789abcdef" for character in binary["sha256"])
    ):
        raise LaunchArtifactError("source host QEMU identity is invalid")
    profile = document["sel4_profile"]
    timer_clock_hz = qemu.get("timer_clock_hz")
    if timer_clock_hz != PRODUCTION_PROFILE_TIMER_CLOCK_HZ[profile]:
        raise LaunchArtifactError(
            "source guest timer does not match its production seL4 profile"
        )
    expected_claim = _claim(
        host_system=qemu["host_system"],
        accelerator=qemu["accelerator"],
        sel4_profile=document["sel4_profile"],
        timer_clock_hz=qemu["timer_clock_hz"],
        gic_version=document["gic_version"],
        virtualization=qemu["virtualization"],
        machine_extra=qemu["machine_extra"],
        cpu=qemu["cpu"],
        smp=qemu["smp"],
        net_backend=qemu["net_backend"],
    )
    if document.get("claim") != expected_claim:
        raise LaunchArtifactError("source host launch claim classification is invalid")
    expected_accelerator = {"Darwin": "hvf", "Linux": "kvm"}.get(
        qemu.get("host_system")
    )
    expected_machine_extra = {
        "Darwin": CANONICAL_DARWIN_MACHINE_EXTRA,
        "Linux": CANONICAL_LINUX_MACHINE_EXTRA,
    }.get(qemu.get("host_system"))
    expected_cpu = {
        "Darwin": CANONICAL_DARWIN_CPU,
        "Linux": CANONICAL_LINUX_CPU,
    }.get(qemu.get("host_system"))
    if (
        expected_accelerator is None
        or qemu.get("accelerator") != expected_accelerator
        or qemu.get("virtualization") != CANONICAL_VIRTUALIZATION
        or qemu.get("machine_extra") != expected_machine_extra
        or qemu.get("cpu") != expected_cpu
        or qemu.get("smp") != CANONICAL_SMP
        or qemu.get("net_backend") != CANONICAL_NET_BACKEND
    ):
        raise LaunchArtifactError(
            "source build record is outside a supported production host envelope"
        )
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
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("write", "verify"):
        context = subparsers.add_parser(command)
        context.add_argument("--out-dir", type=Path, required=True)
        context.add_argument("--sel4-build", type=Path, required=True)
        context.add_argument("--profile", required=True)
        context.add_argument("--cargo-target", required=True)
        context.add_argument("--root-task-features", default="")
        context.add_argument("--gic-version", required=True)
        context.add_argument("--sel4-profile", required=True)
        context.add_argument("--qemu", required=True)
        context.add_argument(
            "--accelerator", choices=("hvf", "kvm", "tcg"), required=True
        )
        context.add_argument(
            "--virtualization", choices=("on", "off"), required=True
        )
        context.add_argument("--machine-extra", required=True)
        context.add_argument("--cpu", required=True)
        context.add_argument("--smp", required=True)
        context.add_argument(
            "--net-backend", choices=("virtio", "rtl8139"), required=True
        )
    identity = subparsers.add_parser("verify-artifacts")
    identity.add_argument("--out-dir", type=Path, required=True)
    return parser


def main() -> int:
    """Run the artifact record writer or verifier."""

    args = _parser().parse_args()
    try:
        if args.command == "verify-artifacts":
            record = verify_artifact_identity(args.out_dir)
        else:
            operation = write_record if args.command == "write" else verify_record
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
