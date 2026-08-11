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
import stat
import tempfile
from typing import Any


SCHEMA = "cohesix-qemu-launch-artifacts/v1"
RECORD_NAME = "cohesix-qemu-launch-artifacts.json"
ARTIFACTS = (
    ("elfloader", Path("staging/elfloader")),
    ("kernel", Path("staging/kernel.elf")),
    ("rootserver", Path("staging/rootserver")),
    ("initrd", Path("cohesix-system.cpio")),
)
SHA256_HEX_LEN = 64


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


def _context(
    *,
    out_dir: Path,
    sel4_build_dir: Path,
    profile: str,
    cargo_target: str,
    root_task_features: str,
    gic_version: str,
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
    if gic_version != "3":
        raise LaunchArtifactError("immutable operational QEMU launches require GICv3")
    return {
        "schema": SCHEMA,
        "profile": profile,
        "cargo_target": cargo_target,
        "root_task_features": root_task_features,
        "sel4_build_dir": str(sel4_build_dir.resolve(strict=True)),
        "gic_version": gic_version,
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
) -> Path:
    """Atomically write the immutable QEMU launch-artifact record."""

    document = _context(
        out_dir=out_dir,
        sel4_build_dir=sel4_build_dir,
        profile=profile,
        cargo_target=cargo_target,
        root_task_features=root_task_features,
        gic_version=gic_version,
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
) -> Path:
    """Verify context and byte identity for the exact QEMU launch set."""

    expected = _context(
        out_dir=out_dir,
        sel4_build_dir=sel4_build_dir,
        profile=profile,
        cargo_target=cargo_target,
        root_task_features=root_task_features,
        gic_version=gic_version,
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
        )
    except LaunchArtifactError as error:
        raise SystemExit(f"qemu-launch-artifacts: error: {error}") from error
    print(record)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
