#!/usr/bin/env python3
# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Build and verify deterministic MCS linked-driver archives and identity manifests.
# Author: Lukas Bower

"""Build the separate, deterministic Milestone 26e linked-driver archive."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tomllib
from typing import Iterable, Sequence


SCHEMA = "cohesix-driver-runtime-manifest/v1"
COMPARATOR_SCHEMA = "cohesix-driver-classic-comparator/v1"
EXPECTED_TARGET = "aarch64-unknown-none"
RUNTIME_INIT_ABI_VERSION = 9
SHA256_HEX_LENGTH = 64
NEWC_MAGIC = b"070701"
NEWC_HEADER_BYTES = 110
ARCHIVE_NAME = "cohesix-driver-runtimes.cpio"
COMPONENTS = (
    ("pi4-driver-serial", "serial-console"),
    ("pi4-driver-usb", "usb-keyboard"),
    ("pi4-driver-hdmi", "hdmi-text"),
    ("pi4-driver-genet", "genet-nic"),
    ("pi4-driver-cyw43", "cyw43-wifi"),
    ("pi4-driver-sdio", "sdio-host"),
    ("pi4-driver-pcie", "pcie-root"),
)


def load_comparator_record(path: Path) -> tuple[str, str]:
    """Validate the immutable classic comparator record and return both hashes."""

    try:
        raw = path.read_bytes()
        document = tomllib.loads(raw.decode("utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        raise DriverRuntimeManifestError(
            f"cannot read classic comparator record: {error}"
        ) from error
    expected_top = {
        "schema",
        "source_commit",
        "source_profile",
        "source_root_task_sha256",
        "source_kernel_sha256",
        "source_elfloader_sha256",
        "source_system_cpio_sha256",
        "canonical_archive_format",
        "canonical_archive_bytes",
        "canonical_archive_sha256",
        "components",
    }
    if not isinstance(document, dict) or set(document) != expected_top:
        raise DriverRuntimeManifestError("classic comparator record fields are invalid")
    source_commit = document.get("source_commit")
    if (
        document.get("schema") != COMPARATOR_SCHEMA
        or document.get("canonical_archive_format") != "newc-deterministic-v1"
        or not isinstance(source_commit, str)
        or len(source_commit) != 40
        or any(character not in "0123456789abcdef" for character in source_commit)
        or not isinstance(document.get("source_profile"), str)
        or not document["source_profile"]
        or not isinstance(document.get("canonical_archive_bytes"), int)
        or document["canonical_archive_bytes"] <= 0
    ):
        raise DriverRuntimeManifestError("classic comparator record identity is invalid")
    for field in (
        "source_root_task_sha256",
        "source_kernel_sha256",
        "source_elfloader_sha256",
        "source_system_cpio_sha256",
        "canonical_archive_sha256",
    ):
        value = document.get(field)
        if not isinstance(value, str) or not _valid_sha256(value):
            raise DriverRuntimeManifestError(
                f"classic comparator record {field} is invalid"
            )
    components = document.get("components")
    if not isinstance(components, list) or len(components) != len(COMPONENTS):
        raise DriverRuntimeManifestError("classic comparator component set is incomplete")
    expected_fields = {"name", "archive_path", "bytes", "sha256"}
    for row, (name, _hot_path) in zip(components, COMPONENTS):
        if (
            not isinstance(row, dict)
            or set(row) != expected_fields
            or row.get("name") != name
            or row.get("archive_path") != _component_path(name)
            or not isinstance(row.get("bytes"), int)
            or row["bytes"] <= 0
            or not isinstance(row.get("sha256"), str)
            or not _valid_sha256(row["sha256"])
        ):
            raise DriverRuntimeManifestError(
                "classic comparator component identity is invalid"
            )
    return str(document["canonical_archive_sha256"]), _sha256(raw)


class DriverRuntimeManifestError(ValueError):
    """Raised when an archive or its evidence identity fails closed."""


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _valid_sha256(value: str) -> bool:
    return (
        len(value) == SHA256_HEX_LENGTH
        and value == value.lower()
        and all(character in "0123456789abcdef" for character in value)
    )


def _pad4(value: int) -> int:
    return (-value) & 3


def _checked_slice(data: bytes, offset: int, size: int, label: str) -> bytes:
    if offset < 0 or size < 0 or offset > len(data) or size > len(data) - offset:
        raise DriverRuntimeManifestError(f"{label} lies outside the archive")
    return data[offset : offset + size]


def _newc_entry(name: str, data: bytes, mode: int) -> bytes:
    try:
        name_bytes = name.encode("ascii") + b"\0"
    except UnicodeEncodeError as error:
        raise DriverRuntimeManifestError("archive member names must be ASCII") from error
    fields = (0, mode, 0, 0, 1, 0, len(data), 0, 0, 0, 0, len(name_bytes), 0)
    header = NEWC_MAGIC + b"".join(
        f"{value:08x}".encode("ascii") for value in fields
    )
    if len(header) != NEWC_HEADER_BYTES:
        raise DriverRuntimeManifestError("internal newc header size mismatch")
    return (
        header
        + name_bytes
        + bytes(_pad4(len(header) + len(name_bytes)))
        + data
        + bytes(_pad4(len(data)))
    )


def build_newc(files: Sequence[tuple[str, bytes]]) -> bytes:
    """Build byte-stable newc bytes with fixed metadata and sorted members."""

    names = [name for name, _data in files]
    if names != sorted(names) or len(names) != len(set(names)):
        raise DriverRuntimeManifestError("archive members must be unique and sorted")
    directories: set[str] = set()
    for name in names:
        parent = Path(name).parent
        while parent != Path("."):
            directories.add(parent.as_posix())
            parent = parent.parent
    file_map = dict(files)
    output = bytearray()
    for name in sorted((*directories, *names)):
        if name in file_map:
            output.extend(_newc_entry(name, file_map[name], stat.S_IFREG | 0o555))
        else:
            output.extend(_newc_entry(name, b"", stat.S_IFDIR | 0o555))
    output.extend(_newc_entry("TRAILER!!!", b"", 0))
    output.extend(bytes((-len(output)) & 511))
    return bytes(output)


def parse_newc(data: bytes) -> dict[str, bytes]:
    """Strictly parse the deterministic newc subset used by this pipeline."""

    offset = 0
    last_name = ""
    entries: dict[str, bytes] = {}
    while True:
        header = _checked_slice(data, offset, NEWC_HEADER_BYTES, "newc header")
        if header[:6] != NEWC_MAGIC:
            raise DriverRuntimeManifestError("archive is not canonical newc")
        try:
            fields = tuple(
                int(header[6 + index * 8 : 14 + index * 8], 16)
                for index in range(13)
            )
        except ValueError as error:
            raise DriverRuntimeManifestError("archive has a malformed newc field") from error
        mode = fields[1]
        file_size = fields[6]
        name_size = fields[11]
        if name_size < 2:
            raise DriverRuntimeManifestError("archive member name is empty")
        offset += NEWC_HEADER_BYTES
        raw_name = _checked_slice(data, offset, name_size, "newc member name")
        if raw_name[-1] != 0 or b"\0" in raw_name[:-1]:
            raise DriverRuntimeManifestError("archive member name is not canonical")
        try:
            name = raw_name[:-1].decode("ascii")
        except UnicodeDecodeError as error:
            raise DriverRuntimeManifestError("archive member name is not ASCII") from error
        offset += name_size + _pad4(NEWC_HEADER_BYTES + name_size)
        payload = _checked_slice(data, offset, file_size, f"newc member {name}")
        offset += file_size + _pad4(file_size)
        if name == "TRAILER!!!":
            if payload or any(data[offset:]):
                raise DriverRuntimeManifestError("archive trailer is not canonical")
            return entries
        if name <= last_name or name in entries:
            raise DriverRuntimeManifestError("archive members are duplicated or unsorted")
        last_name = name
        if stat.S_ISDIR(mode):
            if payload:
                raise DriverRuntimeManifestError("archive directory has payload bytes")
            continue
        if not stat.S_ISREG(mode):
            raise DriverRuntimeManifestError("archive has a non-regular member")
        entries[name] = payload


def _write_atomic(path: Path, data: bytes, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_bytes(data)
    temporary.chmod(mode)
    os.replace(temporary, path)


def _component_path(name: str) -> str:
    return f"cohesix/bin/{name}"


def build_manifest(
    image_dir: Path,
    archive_path: Path,
    manifest_path: Path,
    target: str,
    profile: str,
    classic_comparator_sha256: str,
    classic_comparator_record_sha256: str,
) -> dict[str, object]:
    """Build the exact seven-runtime MCS archive and evidence manifest."""

    if target != EXPECTED_TARGET:
        raise DriverRuntimeManifestError(f"unsupported driver target: {target}")
    if not profile or any(character.isspace() for character in profile):
        raise DriverRuntimeManifestError("driver profile is invalid")
    if not _valid_sha256(classic_comparator_sha256):
        raise DriverRuntimeManifestError(
            "classic comparator must be one explicit lowercase SHA-256"
        )
    if not _valid_sha256(classic_comparator_record_sha256):
        raise DriverRuntimeManifestError(
            "classic comparator record must have one explicit lowercase SHA-256"
        )
    rows: list[dict[str, object]] = []
    members: list[tuple[str, bytes]] = []
    for name, hot_path in COMPONENTS:
        source = image_dir / name
        if not source.is_file():
            raise DriverRuntimeManifestError(f"required driver image is missing: {source}")
        data = source.read_bytes()
        if not data:
            raise DriverRuntimeManifestError(f"required driver image is empty: {source}")
        archive_member = _component_path(name)
        rows.append(
            {
                "name": name,
                "hot_path": hot_path,
                "archive_path": archive_member,
                "bytes": len(data),
                "sha256": _sha256(data),
            }
        )
        members.append((archive_member, data))
    archive_bytes = build_newc(tuple(sorted(members)))
    _write_atomic(archive_path, archive_bytes)
    document: dict[str, object] = {
        "schema": SCHEMA,
        "target": target,
        "profile": profile,
        "scheduler": "mcs-active-sc",
        "runtime_init_abi_version": RUNTIME_INIT_ABI_VERSION,
        "archive": {
            "name": ARCHIVE_NAME,
            "bytes": len(archive_bytes),
            "sha256": _sha256(archive_bytes),
        },
        "classic_comparator": {
            "provenance": "retired-26d-classic-driver-archive",
            "sha256": classic_comparator_sha256,
            "record_sha256": classic_comparator_record_sha256,
        },
        "components": rows,
    }
    _write_atomic(
        manifest_path,
        (json.dumps(document, indent=2, sort_keys=True) + "\n").encode("utf-8"),
    )
    verify_manifest(manifest_path, archive_path)
    return document


def verify_manifest(manifest_path: Path, archive_path: Path) -> dict[str, object]:
    """Verify exact schema, component set, archive identity, and comparator."""

    try:
        document = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise DriverRuntimeManifestError(f"cannot read driver manifest: {error}") from error
    expected_top = {
        "schema",
        "target",
        "profile",
        "scheduler",
        "runtime_init_abi_version",
        "archive",
        "classic_comparator",
        "components",
    }
    if not isinstance(document, dict) or set(document) != expected_top:
        raise DriverRuntimeManifestError("driver manifest fields are invalid")
    if (
        document.get("schema") != SCHEMA
        or document.get("target") != EXPECTED_TARGET
        or document.get("scheduler") != "mcs-active-sc"
        or document.get("runtime_init_abi_version") != RUNTIME_INIT_ABI_VERSION
    ):
        raise DriverRuntimeManifestError("driver manifest contract is invalid")
    profile = document.get("profile")
    if not isinstance(profile, str) or not profile:
        raise DriverRuntimeManifestError("driver manifest profile is missing")
    archive = document.get("archive")
    comparator = document.get("classic_comparator")
    components = document.get("components")
    if not isinstance(archive, dict) or set(archive) != {"name", "bytes", "sha256"}:
        raise DriverRuntimeManifestError("driver archive identity is invalid")
    if archive.get("name") != ARCHIVE_NAME:
        raise DriverRuntimeManifestError("driver archive name is invalid")
    if (
        not isinstance(comparator, dict)
        or set(comparator) != {"provenance", "sha256", "record_sha256"}
        or comparator.get("provenance") != "retired-26d-classic-driver-archive"
        or not isinstance(comparator.get("sha256"), str)
        or not _valid_sha256(str(comparator["sha256"]))
        or not isinstance(comparator.get("record_sha256"), str)
        or not _valid_sha256(str(comparator["record_sha256"]))
    ):
        raise DriverRuntimeManifestError("classic comparator identity is invalid")
    archive_bytes = archive_path.read_bytes()
    if archive.get("bytes") != len(archive_bytes) or archive.get("sha256") != _sha256(
        archive_bytes
    ):
        raise DriverRuntimeManifestError("driver archive identity does not match")
    entries = parse_newc(archive_bytes)
    expected_paths = [_component_path(name) for name, _hot_path in COMPONENTS]
    if sorted(entries) != sorted(expected_paths):
        raise DriverRuntimeManifestError("driver archive member set is invalid")
    if not isinstance(components, list) or len(components) != len(COMPONENTS):
        raise DriverRuntimeManifestError("driver component inventory is incomplete")
    expected_fields = {"name", "hot_path", "archive_path", "bytes", "sha256"}
    for row, (name, hot_path) in zip(components, COMPONENTS):
        if not isinstance(row, dict) or set(row) != expected_fields:
            raise DriverRuntimeManifestError("driver component row fields are invalid")
        member_path = _component_path(name)
        member = entries[member_path]
        if (
            row.get("name") != name
            or row.get("hot_path") != hot_path
            or row.get("archive_path") != member_path
            or row.get("bytes") != len(member)
            or row.get("sha256") != _sha256(member)
        ):
            raise DriverRuntimeManifestError("driver component identity does not match")
    return document


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("build", help="build and verify the driver archive")
    build.add_argument("--image-dir", required=True, type=Path)
    build.add_argument("--archive", required=True, type=Path)
    build.add_argument("--manifest", required=True, type=Path)
    build.add_argument("--target", required=True)
    build.add_argument("--profile", required=True)
    build.add_argument("--classic-comparator-record", required=True, type=Path)
    verify = commands.add_parser("verify", help="verify an existing driver archive")
    verify.add_argument("--archive", required=True, type=Path)
    verify.add_argument("--manifest", required=True, type=Path)
    return parser


def main(argv: Iterable[str] | None = None) -> int:
    """Run the deterministic archive command."""

    arguments = _parser().parse_args(list(argv) if argv is not None else None)
    try:
        if arguments.command == "build":
            comparator_sha256, comparator_record_sha256 = load_comparator_record(
                arguments.classic_comparator_record
            )
            build_manifest(
                arguments.image_dir,
                arguments.archive,
                arguments.manifest,
                arguments.target,
                arguments.profile,
                comparator_sha256,
                comparator_record_sha256,
            )
        else:
            verify_manifest(arguments.manifest, arguments.archive)
    except (DriverRuntimeManifestError, OSError) as error:
        print(f"driver-runtime-manifest: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
