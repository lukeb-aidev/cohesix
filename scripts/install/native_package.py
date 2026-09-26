#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Bind native installer staging to one independently selected release archive and its exact payload bytes.
# Copyright 2026 Lukas Bower
"""Prepare bounded, immutable inputs for the macOS and Ubuntu package builders."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path, PurePosixPath
import os
import re
import stat
import sys
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from release_qualify import digest, inspect_bundle

REFERENCE_SCHEMA = "cohesix-m28g-installer-reference/v1"
MAX_REFERENCE_BYTES = 64 * 1024
MAX_FILES = 512
MAX_PAYLOAD_BYTES = 512 * 1024 * 1024


def load_reference(path: Path, host: str) -> dict[str, Any]:
    """Require an explicit archive and source identity outside the payload."""
    if not path.is_file() or path.is_symlink() or path.stat().st_size > MAX_REFERENCE_BYTES:
        raise ValueError("installer reference must be a bounded regular file")
    value = json.loads(path.read_bytes())
    required = {
        "schema", "host", "bundle", "archive", "archive_sha256",
        "source_commit", "version",
    }
    if host == "macos":
        required |= {
            "signed_app", "app_binary_sha256", "app_version", "team_id",
        }
    if (
        not isinstance(value, dict)
        or set(value) != required
        or any(not isinstance(item, str) for item in value.values())
        or value["schema"] != REFERENCE_SCHEMA
        or value["host"] != host
        or host not in {"macos", "linux"}
        or not re.fullmatch(r"[0-9a-f]{40}", value["source_commit"])
        or not re.fullmatch(r"[0-9a-f]{64}", value["archive_sha256"])
        or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-beta)?", value["version"])
    ):
        raise ValueError("installer reference identity is invalid")
    bundle, archive = (Path(value[key]) for key in ("bundle", "archive"))
    if not bundle.is_absolute() or not archive.is_absolute():
        raise ValueError("installer bundle and archive must be absolute paths")
    if bundle.is_symlink() or archive.is_symlink():
        raise ValueError("installer inputs cannot be symlinks")
    if host == "macos":
        app = Path(value["signed_app"])
        if (
            not app.is_absolute() or app.is_symlink()
            or not re.fullmatch(r"[0-9a-f]{64}", value["app_binary_sha256"])
            or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value["app_version"])
            or value["app_version"] != value["version"].split("-", 1)[0]
            or not re.fullmatch(r"[A-Z0-9]{10}", value["team_id"])
        ):
            raise ValueError("Mac app reference identity is invalid")
    observed = inspect_bundle(bundle, archive)
    if (
        digest(archive) != value["archive_sha256"]
        or observed["source_commit"] != value["source_commit"]
        or observed["version"] != value["version"]
        or observed["archive"]["sha256"] != value["archive_sha256"]
        or not observed["bundle"].endswith("-MacOS" if host == "macos" else "-linux")
    ):
        raise ValueError("installer reference differs from the verified release archive")
    python_version = value["version"].replace("-beta", "b0")
    wheels = list((bundle / "python/dist").glob("cohesix-*.whl"))
    if (
        len(wheels) != 1
        or not wheels[0].name.startswith(f"cohesix-{python_version}-")
        or wheels[0].is_symlink()
    ):
        raise ValueError("installer requires one version-aligned Cohesix Python wheel")
    nemo_wheels = list((bundle / "nemo/dist").glob("cohesix_nemo_kit-*.whl"))
    if (
        len(nemo_wheels) != 1
        or nemo_wheels[0].name != f"cohesix_nemo_kit-{python_version}-py3-none-any.whl"
        or nemo_wheels[0].is_symlink()
    ):
        raise ValueError("installer requires one version-aligned Cohesix NeMo wheel")
    return value


def classify(relative: str) -> str:
    """Keep desktop and guest artifacts optional to the headless controller."""
    if relative.startswith("qemu/") or relative.startswith("image/"):
        return "optional-guest"
    if relative == "bin/swarmui" or relative.startswith("ui/swarmui/"):
        return "desktop"
    return "controller"


def stage_subset(bundle: Path, target: Path, subset: str) -> list[dict[str, Any]]:
    """Copy only verified regular files, preserving executable bits and hashes."""
    if subset not in {"controller", "desktop", "optional-guest"}:
        raise ValueError("unknown installer payload subset")
    if target.exists() or target.is_symlink():
        raise ValueError("installer staging destination already exists")
    lines = (bundle / "MANIFEST.sha256").read_text().splitlines()
    if len(lines) > MAX_FILES:
        raise ValueError("release file count exceeds installer bound")
    records: list[dict[str, Any]] = []
    total = 0
    for line in lines:
        match = re.fullmatch(r"([0-9a-f]{64})  ([^\n]+)", line)
        if match is None:
            raise ValueError("invalid release manifest entry")
        expected, relative = match.groups()
        path = PurePosixPath(relative)
        if (
            path.is_absolute() or path.as_posix() != relative
            or "." in path.parts or ".." in path.parts
            or "\\" in relative or any(ord(char) < 32 for char in relative)
        ):
            raise ValueError("unsafe installer payload path")
        if classify(relative) != subset:
            continue
        source = bundle / relative
        if (
            source.is_symlink()
            or not source.is_file()
            or not stat.S_ISREG(source.stat().st_mode)
        ):
            raise ValueError("unsafe installer payload path")
        metadata = source.stat()
        size = metadata.st_size
        total += size
        if total > MAX_PAYLOAD_BYTES:
            raise ValueError("installer payload exceeds byte bound")
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        descriptor = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        with os.fdopen(descriptor, "rb") as reader, destination.open("xb") as writer:
            opened = os.fstat(reader.fileno())
            if (opened.st_dev, opened.st_ino, opened.st_size) != (
                metadata.st_dev, metadata.st_ino, metadata.st_size
            ):
                raise ValueError("installer source changed before copy")
            digest_value = hashlib.sha256()
            copied = 0
            while block := reader.read(1024 * 1024):
                copied += len(block)
                if copied > size:
                    raise ValueError("installer source changed during copy")
                digest_value.update(block)
                writer.write(block)
            after = os.fstat(reader.fileno())
        if (opened.st_size, opened.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError("installer source changed during copy")
        if copied != size or digest_value.hexdigest() != expected:
            raise ValueError("installer source differs from release manifest")
        destination.chmod(0o755 if source.stat().st_mode & 0o111 else 0o644)
        records.append({"path": relative, "size": size, "sha256": expected})
    if not records:
        raise ValueError("installer payload subset is empty")
    return records


def write_manifest(path: Path, *, reference: dict[str, Any],
                   records: list[dict[str, Any]], package: str) -> None:
    """Record exact staged bytes separately from the archive's full manifest."""
    payload = {
        "schema": "cohesix-m28g-installed-payload/v1",
        "package": package,
        "host": reference["host"],
        "version": reference["version"],
        "source_commit": reference["source_commit"],
        "archive_sha256": reference["archive_sha256"],
        "files": sorted(records, key=lambda row: row["path"]),
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, sort_keys=True, indent=2) + "\n")


def normalize_public_modes(root: Path) -> None:
    """Make staged code readable and executable after private build staging."""
    if root.is_symlink() or not root.is_dir():
        raise ValueError("package root must be a real directory")
    for path in (root, *sorted(root.rglob("*"))):
        if path.is_symlink():
            raise ValueError("package staging cannot contain symlinks")
        mode = path.stat().st_mode
        if stat.S_ISDIR(mode):
            path.chmod(0o755)
        elif stat.S_ISREG(mode):
            path.chmod(0o755 if mode & 0o111 else 0o644)
        else:
            raise ValueError("package staging contains a special file")
