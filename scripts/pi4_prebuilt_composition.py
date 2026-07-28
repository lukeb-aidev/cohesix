#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Compose a Pi 4 Cohesix image from immutable repository seL4 artifacts.
# Copyright 2026 Lukas Bower
"""Relink the tracked Pi 4 elfloader with one exact Cohesix root task.

This helper deliberately does not configure or rebuild seL4. It validates the
repository-managed ``seL4/build_UBOOT`` artifact tree, proves the selected
binutils reproduce the tracked legacy-image payload byte for byte, and then
relinks a new elfloader around an archive containing the supplied root task.
All writes are confined to a scratch directory and the requested output
directory.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from typing import Any, Sequence
import zlib

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
LIB_DIR = SCRIPT_DIR / "lib"
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(LIB_DIR))

from strip_elfloader_modules import (  # noqa: E402
    _parse_cpio,
    rewrite_rootserver_archive,
)
from sel4_profile import (  # noqa: E402
    DEFAULT_CONTRACT,
    load_contract,
    validate_repo_managed_build,
)

PROFILE_NAME = "pi4_diagnostic"
CANONICAL_BUILD_DIR = REPO_ROOT / "seL4" / "build_UBOOT"
DEFAULT_BINUTILS_PREFIX = "/opt/homebrew/bin/aarch64-linux-gnu-"
BINUTILS_ENV = "COHESIX_AARCH64_BINUTILS_PREFIX"
REQUIRED_BINUTILS = ("as", "ld", "objcopy", "readelf", "strip")

BUILD_GRAPH = Path("build.ninja")
ARCHIVE = Path("elfloader/archive.archive.o.cpio")
ARCHIVE_SOURCE = Path("elfloader/archive.o.S")
LINKER_SCRIPT = Path("elfloader/linker.lds_pp")
LIBCPIO = Path("apps/sel4test-driver/util_libs/libcpio/libcpio.a")
BASELINE_ELFLOADER = Path("elfloader/elfloader")
BASELINE_IMAGE = Path("images/sel4test-driver-image-arm-bcm2711")
PROFILE_STAMP = Path("cohesix-profile-build-inputs.json")

OUTPUT_ROOTSERVER = "rootserver"
OUTPUT_ARCHIVE = "archive.archive.o.cpio"
OUTPUT_ELFLOADER = "elfloader"
OUTPUT_PAYLOAD = "payload.bin"
OUTPUT_IMAGE = "sel4test-driver-image-arm-bcm2711"
OUTPUT_PROVENANCE = "composition-profile-build-inputs.json"

UIMAGE_MAGIC = 0x27051956
UIMAGE_HEADER_SIZE = 64
UIMAGE_LOAD_ADDRESS = 0x10000000
UIMAGE_OS_LINUX = 5
UIMAGE_ARCH_ARM64 = 22
UIMAGE_TYPE_KERNEL = 2
UIMAGE_COMPRESSION_NONE = 0
UIMAGE_NAME_BYTES = 32
UIMAGE_HEADER = struct.Struct(">7I4B32s")

ELF_MAGIC = b"\x7fELF"
ELF_MACHINE_AARCH64 = 183
PT_LOAD = 1
CPIO_BLOCK_SIZE = 512
BINUTILS_VERSION_RE = re.compile(r"(\d+(?:\.\d+){1,3})(?:\s|$)")


class CompositionError(RuntimeError):
    """Raised when immutable Pi 4 composition cannot be proven correct."""


def sha256_bytes(data: bytes) -> str:
    """Return the SHA-256 digest of *data*."""

    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    """Return the SHA-256 digest of one file without loading it all at once."""

    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def file_evidence(path: Path) -> dict[str, Any]:
    """Return path, size, and digest evidence for one required file."""

    resolved = path.resolve()
    if not resolved.is_file():
        raise CompositionError(f"required file is missing: {resolved}")
    return {
        "path": str(resolved),
        "size": resolved.stat().st_size,
        "sha256": sha256_file(resolved),
    }


def run_checked(
    argv: Sequence[str | Path],
    *,
    cwd: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run one command without a shell and retain its textual output."""

    command = [str(value) for value in argv]
    try:
        return subprocess.run(
            command,
            cwd=cwd,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except FileNotFoundError as exc:
        raise CompositionError(f"required command not found: {command[0]}") from exc
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or exc.stdout or "").strip()
        suffix = f": {detail}" if detail else ""
        raise CompositionError(
            f"command failed ({' '.join(command)}){suffix}"
        ) from exc


def _binutils_version(version_line: str, tool_name: str) -> str:
    """Extract one GNU binutils version from a tool's first version line."""

    if "GNU Binutils" not in version_line:
        raise CompositionError(
            f"{tool_name} is not a GNU binutils tool: {version_line!r}"
        )
    matches = BINUTILS_VERSION_RE.findall(version_line)
    if not matches:
        raise CompositionError(
            f"cannot identify {tool_name} binutils version: {version_line!r}"
        )
    return matches[-1]


def resolve_binutils(prefix: str | None = None) -> dict[str, Any]:
    """Resolve one complete, version-consistent AArch64 binutils family."""

    selected = prefix or os.environ.get(BINUTILS_ENV) or DEFAULT_BINUTILS_PREFIX
    expanded = os.path.expanduser(selected)
    if not os.path.isabs(expanded):
        raise CompositionError(
            f"{BINUTILS_ENV} must name an absolute tool prefix, got {selected!r}"
        )

    tools: dict[str, dict[str, Any]] = {}
    versions: set[str] = set()
    resolved_parents: set[Path] = set()
    for name in REQUIRED_BINUTILS:
        declared_path = Path(f"{expanded}{name}")
        if not declared_path.is_file() or not os.access(declared_path, os.X_OK):
            raise CompositionError(
                "incomplete AArch64 binutils family: "
                f"missing executable {declared_path}"
            )
        resolved_path = declared_path.resolve()
        version_output = run_checked((resolved_path, "--version")).stdout
        version_line = version_output.splitlines()[0] if version_output else ""
        version = _binutils_version(version_line, name)
        versions.add(version)
        resolved_parents.add(resolved_path.parent)
        tools[name] = {
            "path": str(declared_path),
            "resolved_path": str(resolved_path),
            "sha256": sha256_file(resolved_path),
            "version_line": version_line,
            "version": version,
        }

    if len(versions) != 1:
        details = ", ".join(
            f"{name}={record['version']}" for name, record in tools.items()
        )
        raise CompositionError(
            f"AArch64 binutils family has mixed versions: {details}"
        )
    if len(resolved_parents) != 1:
        details = ", ".join(sorted(str(path) for path in resolved_parents))
        raise CompositionError(
            f"AArch64 binutils family resolves across directories: {details}"
        )

    return {
        "declared_prefix": expanded,
        "resolved_directory": str(next(iter(resolved_parents))),
        "version": next(iter(versions)),
        "tools": tools,
    }


def verify_binutils_unchanged(tools: dict[str, Any]) -> None:
    """Fail if any resolved tool binary changed after family validation."""

    for name, record in tools["tools"].items():
        resolved_path = Path(record["resolved_path"])
        if not resolved_path.is_file():
            raise CompositionError(
                f"resolved AArch64 binutils tool disappeared: {resolved_path}"
            )
        observed = sha256_file(resolved_path)
        if observed != record["sha256"]:
            raise CompositionError(
                f"resolved AArch64 binutils {name} changed during composition"
            )


def _ninja_logical_lines(text: str) -> list[str]:
    """Join the restricted Ninja continuation syntax used by build edges."""

    logical: list[str] = []
    pending = ""
    for raw_line in text.splitlines():
        if raw_line.endswith("$"):
            pending += raw_line[:-1].rstrip() + " "
            continue
        logical.append(pending + raw_line)
        pending = ""
    if pending:
        raise CompositionError("build.ninja ends with an incomplete continuation")
    return logical


def _safe_ninja_relative_path(token: str, label: str) -> Path:
    """Validate one path token before resolving it beneath the build tree."""

    if not token or any(character in token for character in ("$", "\\", ":")):
        raise CompositionError(f"unsafe {label} token in build.ninja: {token!r}")
    pure = PurePosixPath(token)
    if pure.is_absolute() or ".." in pure.parts or "." in pure.parts:
        raise CompositionError(f"unsafe {label} path in build.ninja: {token!r}")
    return Path(*pure.parts)


def parse_elfloader_object_order(build_graph: Path) -> list[Path]:
    """Return the exact tracked object order for the elfloader link edge."""

    prefix = (
        "build elfloader/elfloader: "
        "C_EXECUTABLE_LINKER__elfloader_Debug "
    )
    matches = [
        line[len(prefix) :]
        for line in _ninja_logical_lines(build_graph.read_text(encoding="utf-8"))
        if line.startswith(prefix)
    ]
    if len(matches) != 1:
        raise CompositionError(
            "build.ninja must contain exactly one supported elfloader link edge"
        )

    tokens = matches[0].split()
    try:
        implicit_index = tokens.index("|")
    except ValueError as exc:
        raise CompositionError(
            "elfloader link edge has no implicit dependency boundary"
        ) from exc
    explicit = tokens[:implicit_index]
    implicit_end = tokens.index("||") if "||" in tokens else len(tokens)
    if implicit_end <= implicit_index:
        raise CompositionError(
            "elfloader link edge has invalid dependency-boundary ordering"
        )
    implicit = tokens[implicit_index + 1 : implicit_end]
    if not explicit or explicit[0] != "elfloader/archive.o":
        raise CompositionError(
            "elfloader link edge does not begin with elfloader/archive.o"
        )
    if str(LIBCPIO) not in implicit or str(LINKER_SCRIPT) not in implicit:
        raise CompositionError(
            "elfloader link edge lacks its canonical libcpio/linker inputs"
        )

    objects = [
        _safe_ninja_relative_path(token, "elfloader object")
        for token in explicit[1:]
    ]
    if not objects or any(path.suffix != ".obj" for path in objects):
        raise CompositionError(
            "elfloader link edge contains an invalid ordered object list"
        )
    if len(set(objects)) != len(objects):
        raise CompositionError("elfloader link edge repeats an object")
    return objects


def _validated_path(build_dir: Path, relative: Path) -> Path:
    """Resolve a known relative input without permitting a tree escape."""

    resolved_root = build_dir.resolve()
    candidate = (resolved_root / relative).resolve()
    try:
        candidate.relative_to(resolved_root)
    except ValueError as exc:
        raise CompositionError(
            f"repository build input escapes {resolved_root}: {relative}"
        ) from exc
    if not candidate.is_file():
        raise CompositionError(f"repository build input is missing: {candidate}")
    return candidate


def validate_repo_build(build_dir: Path) -> dict[str, Any]:
    """Validate the exact immutable repository-managed Pi 4 artifact tree."""

    resolved = build_dir.expanduser().resolve()
    expected = CANONICAL_BUILD_DIR.resolve()
    if resolved != expected:
        raise CompositionError(
            f"Pi 4 seL4 input must be {expected}, got {resolved}"
        )

    evidence = validate_repo_managed_build(
        load_contract(DEFAULT_CONTRACT),
        PROFILE_NAME,
        resolved,
        for_runtime=True,
    )
    errors = evidence.get("errors")
    if not evidence.get("valid") or errors:
        detail = "; ".join(str(error) for error in errors or [])
        raise CompositionError(
            f"repository-managed Pi 4 seL4 validation failed: {detail}"
        )

    git_tree = run_checked(
        (
            "git",
            "-C",
            REPO_ROOT,
            "rev-parse",
            "HEAD:seL4/build_UBOOT",
        )
    ).stdout.strip()
    return {
        "profile": PROFILE_NAME,
        "build_dir": str(resolved),
        "git_tree": git_tree,
        "profile_stamp": file_evidence(resolved / PROFILE_STAMP),
        "validator_schema": evidence.get("schema"),
        "managed_validator_schema": evidence.get("repo_managed", {}).get(
            "schema"
        ),
    }


def _elf64_load_segment(elf: bytes) -> dict[str, int]:
    """Validate an AArch64 elfloader with exactly one PT_LOAD program header."""

    if len(elf) < 64 or elf[:4] != ELF_MAGIC:
        raise CompositionError("elfloader is not an ELF file")
    if elf[4] != 2:
        raise CompositionError("elfloader is not ELF64")
    if elf[5] not in (1, 2):
        raise CompositionError("elfloader has unsupported byte order")
    endian = "<" if elf[5] == 1 else ">"
    header_format = endian + "HHIQQQIHHHHHH"
    header = struct.unpack_from(header_format, elf, 16)
    machine = header[1]
    entry = header[3]
    phoff = header[4]
    phentsize = header[8]
    phnum = header[9]
    if machine != ELF_MACHINE_AARCH64:
        raise CompositionError(
            f"elfloader machine is {machine}, expected AArch64"
        )
    if entry != UIMAGE_LOAD_ADDRESS:
        raise CompositionError(
            f"elfloader entry is 0x{entry:x}, expected 0x{UIMAGE_LOAD_ADDRESS:x}"
        )

    program_format = endian + "IIQQQQQQ"
    program_size = struct.calcsize(program_format)
    if phnum != 1 or phentsize < program_size:
        raise CompositionError(
            "elfloader must contain exactly one complete program header"
        )
    if phoff + phentsize > len(elf):
        raise CompositionError("elfloader program header exceeds file bounds")
    fields = struct.unpack_from(program_format, elf, phoff)
    (
        segment_type,
        _flags,
        offset,
        virtual_address,
        physical_address,
        file_size,
        memory_size,
        alignment,
    ) = fields
    if segment_type != PT_LOAD:
        raise CompositionError("elfloader's sole program header is not PT_LOAD")
    if virtual_address != UIMAGE_LOAD_ADDRESS:
        raise CompositionError(
            "elfloader PT_LOAD virtual address is not 0x10000000"
        )
    if physical_address != UIMAGE_LOAD_ADDRESS:
        raise CompositionError(
            "elfloader PT_LOAD physical address is not 0x10000000"
        )
    if file_size == 0 or memory_size < file_size:
        raise CompositionError("elfloader PT_LOAD sizes are invalid")
    if offset + file_size > len(elf):
        raise CompositionError("elfloader PT_LOAD exceeds file bounds")
    return {
        "entry": entry,
        "offset": offset,
        "virtual_address": virtual_address,
        "physical_address": physical_address,
        "file_size": file_size,
        "memory_size": memory_size,
        "alignment": alignment,
    }


def verify_elfloader(
    elfloader: Path,
    payload: Path,
    tools: dict[str, Any],
) -> dict[str, Any]:
    """Verify sole-PT_LOAD layout and exact raw-payload extraction."""

    elf = elfloader.read_bytes()
    segment = _elf64_load_segment(elf)
    raw = payload.read_bytes()
    embedded = elf[
        segment["offset"] : segment["offset"] + segment["file_size"]
    ]
    if raw != embedded:
        raise CompositionError(
            "objcopy payload differs from the elfloader's sole PT_LOAD bytes"
        )
    readelf = Path(tools["tools"]["readelf"]["resolved_path"])
    readelf_output = run_checked((readelf, "-lW", elfloader)).stdout
    if readelf_output.count(" LOAD ") != 1:
        raise CompositionError(
            "readelf did not report exactly one elfloader LOAD segment"
        )
    return {
        **segment,
        "readelf_output_sha256": sha256_bytes(readelf_output.encode("utf-8")),
    }


def parse_uimage(image: bytes) -> tuple[dict[str, Any], bytes]:
    """Parse and verify one uncompressed legacy U-Boot image."""

    if len(image) < UIMAGE_HEADER_SIZE:
        raise CompositionError("legacy U-Boot image is truncated")
    fields = UIMAGE_HEADER.unpack_from(image)
    (
        magic,
        header_crc,
        timestamp,
        data_size,
        load_address,
        entry_point,
        data_crc,
        os_id,
        architecture,
        image_type,
        compression,
        name,
    ) = fields
    if magic != UIMAGE_MAGIC:
        raise CompositionError(
            f"legacy U-Boot image has bad magic 0x{magic:08x}"
        )
    payload = image[UIMAGE_HEADER_SIZE:]
    _validate_uimage_payload_size(len(payload))
    if data_size != len(payload):
        raise CompositionError(
            f"legacy U-Boot image size is {data_size}, actual {len(payload)}"
        )
    header_for_crc = bytearray(image[:UIMAGE_HEADER_SIZE])
    struct.pack_into(">I", header_for_crc, 4, 0)
    observed_header_crc = zlib.crc32(header_for_crc) & 0xFFFFFFFF
    observed_data_crc = zlib.crc32(payload) & 0xFFFFFFFF
    if header_crc != observed_header_crc:
        raise CompositionError(
            "legacy U-Boot image header CRC mismatch: "
            f"expected 0x{header_crc:08x}, got 0x{observed_header_crc:08x}"
        )
    if data_crc != observed_data_crc:
        raise CompositionError(
            "legacy U-Boot image data CRC mismatch: "
            f"expected 0x{data_crc:08x}, got 0x{observed_data_crc:08x}"
        )
    if (
        load_address != UIMAGE_LOAD_ADDRESS
        or entry_point != UIMAGE_LOAD_ADDRESS
        or os_id != UIMAGE_OS_LINUX
        or architecture != UIMAGE_ARCH_ARM64
        or image_type != UIMAGE_TYPE_KERNEL
        or compression != UIMAGE_COMPRESSION_NONE
    ):
        raise CompositionError("legacy U-Boot image metadata is not Pi 4 seL4")
    return (
        {
            "magic": f"0x{magic:08x}",
            "header_crc32": f"0x{header_crc:08x}",
            "timestamp": timestamp,
            "data_size": data_size,
            "load_address": f"0x{load_address:08x}",
            "entry_point": f"0x{entry_point:08x}",
            "data_crc32": f"0x{data_crc:08x}",
            "os": os_id,
            "architecture": architecture,
            "type": image_type,
            "compression": compression,
            "name": _decode_uimage_name(name),
        },
        payload,
    )


def _decode_uimage_name(name: bytes) -> str:
    """Decode the fixed legacy-image name as bounded ASCII."""

    try:
        return name.rstrip(b"\x00").decode("ascii")
    except UnicodeDecodeError as exc:
        raise CompositionError(
            "legacy U-Boot image name is not ASCII"
        ) from exc


def _validate_uimage_payload_size(size: int) -> None:
    """Require one non-empty payload whose size fits the legacy header."""

    if not 0 < size <= 0xFFFFFFFF:
        raise CompositionError(
            "legacy U-Boot payload size must be between 1 and uint32 max"
        )


def build_uimage(payload: bytes, *, timestamp: int, name: str = "") -> bytes:
    """Build and self-verify one 64-byte-header legacy U-Boot image."""

    _validate_uimage_payload_size(len(payload))
    if not 0 <= timestamp <= 0xFFFFFFFF:
        raise CompositionError("legacy U-Boot timestamp is outside uint32")
    try:
        name_bytes = name.encode("ascii")
    except UnicodeEncodeError as exc:
        raise CompositionError("legacy U-Boot image name must be ASCII") from exc
    if len(name_bytes) > UIMAGE_NAME_BYTES:
        raise CompositionError("legacy U-Boot image name exceeds 32 bytes")
    name_field = name_bytes.ljust(UIMAGE_NAME_BYTES, b"\x00")
    data_crc = zlib.crc32(payload) & 0xFFFFFFFF
    header_without_crc = UIMAGE_HEADER.pack(
        UIMAGE_MAGIC,
        0,
        timestamp,
        len(payload),
        UIMAGE_LOAD_ADDRESS,
        UIMAGE_LOAD_ADDRESS,
        data_crc,
        UIMAGE_OS_LINUX,
        UIMAGE_ARCH_ARM64,
        UIMAGE_TYPE_KERNEL,
        UIMAGE_COMPRESSION_NONE,
        name_field,
    )
    header_crc = zlib.crc32(header_without_crc) & 0xFFFFFFFF
    header = UIMAGE_HEADER.pack(
        UIMAGE_MAGIC,
        header_crc,
        timestamp,
        len(payload),
        UIMAGE_LOAD_ADDRESS,
        UIMAGE_LOAD_ADDRESS,
        data_crc,
        UIMAGE_OS_LINUX,
        UIMAGE_ARCH_ARM64,
        UIMAGE_TYPE_KERNEL,
        UIMAGE_COMPRESSION_NONE,
        name_field,
    )
    image = header + payload
    _metadata, verified_payload = parse_uimage(image)
    if verified_payload != payload:
        raise CompositionError("legacy U-Boot image self-verification failed")
    return image


def build_rootserver_archive(
    baseline_archive: bytes,
    rootserver: bytes,
) -> tuple[bytes, dict[str, int]]:
    """Replace the rootserver exactly and preserve CPIO block alignment."""

    rebuilt, old_size, new_size, removed = rewrite_rootserver_archive(
        baseline_archive,
        rootserver,
        minimize_rootserver=False,
    )
    if removed != 0:
        raise CompositionError("exact rootserver archive unexpectedly stripped bytes")
    padding = (-len(rebuilt)) % CPIO_BLOCK_SIZE
    aligned = rebuilt + b"\x00" * padding
    entries = _parse_cpio(aligned)
    root_entries = [
        bytes(entry["data"])
        for entry in entries
        if entry["name"] == "rootserver"
    ]
    if root_entries != [rootserver]:
        raise CompositionError(
            "rebuilt archive does not contain one exact rootserver payload"
        )
    if len(aligned) % CPIO_BLOCK_SIZE != 0:
        raise CompositionError("rebuilt CPIO is not block aligned")
    return (
        aligned,
        {
            "baseline_archive_size": len(baseline_archive),
            "baseline_rootserver_size": old_size,
            "rootserver_size": new_size,
            "archive_size": len(aligned),
            "trailing_padding_size": padding,
        },
    )


def _assemble_archive(
    *,
    scratch: Path,
    archive_source: Path,
    archive: bytes,
    tools: dict[str, Any],
) -> Path:
    """Assemble an archive object in scratch without touching the build tree."""

    shutil.copyfile(archive_source, scratch / ARCHIVE_SOURCE.name)
    (scratch / ARCHIVE.name).write_bytes(archive)
    archive_object = scratch / "archive.o"
    assembler = Path(tools["tools"]["as"]["resolved_path"])
    run_checked(
        (
            assembler,
            "-march=armv8-a+crc",
            "-o",
            archive_object.name,
            ARCHIVE_SOURCE.name,
        ),
        cwd=scratch,
    )
    return archive_object


def _link_elfloader(
    *,
    build_dir: Path,
    scratch: Path,
    archive_object: Path,
    object_order: list[Path],
    tools: dict[str, Any],
    output_name: str,
) -> tuple[Path, Path, dict[str, Any]]:
    """Link one elfloader and extract its sole raw load segment."""

    linker = Path(tools["tools"]["ld"]["resolved_path"])
    objcopy = Path(tools["tools"]["objcopy"]["resolved_path"])
    linker_script = _validated_path(build_dir, LINKER_SCRIPT)
    libcpio = _validated_path(build_dir, LIBCPIO)
    objects = [_validated_path(build_dir, relative) for relative in object_order]
    elfloader = scratch / output_name
    payload = scratch / f"{output_name}.payload.bin"
    run_checked(
        (
            linker,
            "-T",
            linker_script,
            "-nostdlib",
            "-static",
            "--build-id=none",
            "-o",
            elfloader,
            archive_object,
            *objects,
            libcpio,
        )
    )
    run_checked((objcopy, "-O", "binary", elfloader, payload))
    segment = verify_elfloader(elfloader, payload, tools)
    return elfloader, payload, segment


def run_baseline_oracle(
    build_dir: Path,
    object_order: list[Path],
    tools: dict[str, Any],
    scratch: Path,
) -> dict[str, Any]:
    """Prove selected tools reproduce the tracked image payload byte for byte."""

    baseline_archive = _validated_path(build_dir, ARCHIVE).read_bytes()
    archive_object = _assemble_archive(
        scratch=scratch,
        archive_source=_validated_path(build_dir, ARCHIVE_SOURCE),
        archive=baseline_archive,
        tools=tools,
    )
    elfloader, payload, segment = _link_elfloader(
        build_dir=build_dir,
        scratch=scratch,
        archive_object=archive_object,
        object_order=object_order,
        tools=tools,
        output_name="baseline-elfloader",
    )
    baseline_image_path = _validated_path(build_dir, BASELINE_IMAGE)
    image_metadata, expected_payload = parse_uimage(
        baseline_image_path.read_bytes()
    )
    actual_payload = payload.read_bytes()
    if actual_payload != expected_payload:
        raise CompositionError(
            "binutils baseline oracle differs from the tracked legacy-image "
            "payload"
        )
    baseline_elfloader_path = _validated_path(build_dir, BASELINE_ELFLOADER)
    return {
        "passed": True,
        "tracked_image": file_evidence(baseline_image_path),
        "tracked_elfloader": file_evidence(baseline_elfloader_path),
        "tracked_archive": file_evidence(_validated_path(build_dir, ARCHIVE)),
        "relinked_elfloader": file_evidence(elfloader),
        "relinked_payload": file_evidence(payload),
        "payload_sha256": sha256_bytes(actual_payload),
        "image_metadata": image_metadata,
        "elf_layout": segment,
    }


def _write_json(path: Path, value: Any) -> None:
    """Write canonical, human-readable JSON with one trailing newline."""

    path.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def _ensure_output_is_external(build_dir: Path, output_dir: Path) -> None:
    """Reject output paths that could mutate the immutable build tree."""

    build_resolved = build_dir.resolve()
    output_resolved = output_dir.expanduser().resolve()
    try:
        output_resolved.relative_to(build_resolved)
    except ValueError:
        return
    raise CompositionError(
        f"output directory must not be inside immutable {build_resolved}"
    )


def compose(
    *,
    build_dir: Path,
    rootserver: Path,
    output_dir: Path,
    timestamp: int,
    binutils_prefix: str | None = None,
) -> dict[str, Any]:
    """Compose, verify, and publish one exact Pi 4 image artifact set."""

    build_dir = build_dir.expanduser().resolve()
    rootserver = rootserver.expanduser().resolve()
    output_dir = output_dir.expanduser().resolve()
    if not rootserver.is_file():
        raise CompositionError(f"rootserver is missing: {rootserver}")
    _ensure_output_is_external(build_dir, output_dir)

    rootserver_source_before = file_evidence(rootserver)
    rootserver_bytes = rootserver.read_bytes()
    rootserver_input = {
        "path": str(rootserver),
        "size": len(rootserver_bytes),
        "sha256": sha256_bytes(rootserver_bytes),
    }
    if rootserver_input != rootserver_source_before:
        raise CompositionError("rootserver changed while it was being read")
    build_before = validate_repo_build(build_dir)
    tools = resolve_binutils(binutils_prefix)
    object_order = parse_elfloader_object_order(
        _validated_path(build_dir, BUILD_GRAPH)
    )
    input_paths = (
        ARCHIVE,
        ARCHIVE_SOURCE,
        LINKER_SCRIPT,
        LIBCPIO,
        BASELINE_ELFLOADER,
        BASELINE_IMAGE,
        BUILD_GRAPH,
        PROFILE_STAMP,
        *object_order,
    )
    input_evidence_before = {
        str(relative): file_evidence(_validated_path(build_dir, relative))
        for relative in input_paths
    }

    output_parent = output_dir.parent
    output_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=".pi4-prebuilt-composition-",
        dir=output_parent,
    ) as temporary:
        scratch_root = Path(temporary)
        oracle_scratch = scratch_root / "oracle"
        composition_scratch = scratch_root / "composition"
        publish_scratch = scratch_root / "publish"
        oracle_scratch.mkdir()
        composition_scratch.mkdir()
        publish_scratch.mkdir()

        oracle = run_baseline_oracle(
            build_dir,
            object_order,
            tools,
            oracle_scratch,
        )
        archive_bytes, archive_record = build_rootserver_archive(
            _validated_path(build_dir, ARCHIVE).read_bytes(),
            rootserver_bytes,
        )
        archive_object = _assemble_archive(
            scratch=composition_scratch,
            archive_source=_validated_path(build_dir, ARCHIVE_SOURCE),
            archive=archive_bytes,
            tools=tools,
        )
        elfloader, payload, elf_layout = _link_elfloader(
            build_dir=build_dir,
            scratch=composition_scratch,
            archive_object=archive_object,
            object_order=object_order,
            tools=tools,
            output_name=OUTPUT_ELFLOADER,
        )
        image_bytes = build_uimage(payload.read_bytes(), timestamp=timestamp)
        image_metadata, verified_payload = parse_uimage(image_bytes)
        if verified_payload != payload.read_bytes():
            raise CompositionError(
                "published image payload differs from the relinked elfloader"
            )

        publish_paths = {
            OUTPUT_ROOTSERVER: publish_scratch / OUTPUT_ROOTSERVER,
            OUTPUT_ARCHIVE: publish_scratch / OUTPUT_ARCHIVE,
            OUTPUT_ELFLOADER: publish_scratch / OUTPUT_ELFLOADER,
            OUTPUT_PAYLOAD: publish_scratch / OUTPUT_PAYLOAD,
            OUTPUT_IMAGE: publish_scratch / OUTPUT_IMAGE,
        }
        publish_paths[OUTPUT_ROOTSERVER].write_bytes(rootserver_bytes)
        publish_paths[OUTPUT_ARCHIVE].write_bytes(archive_bytes)
        shutil.copyfile(elfloader, publish_paths[OUTPUT_ELFLOADER])
        shutil.copyfile(payload, publish_paths[OUTPUT_PAYLOAD])
        publish_paths[OUTPUT_IMAGE].write_bytes(image_bytes)
        os.chmod(publish_paths[OUTPUT_ROOTSERVER], 0o755)
        os.chmod(publish_paths[OUTPUT_ELFLOADER], 0o755)

        input_evidence_after = {
            str(relative): file_evidence(_validated_path(build_dir, relative))
            for relative in input_paths
        }
        if input_evidence_after != input_evidence_before:
            raise CompositionError(
                "repository-managed seL4 input changed during composition"
            )
        build_after = validate_repo_build(build_dir)
        if build_after != build_before:
            raise CompositionError(
                "repository-managed seL4 validation changed during composition"
            )
        rootserver_source_after = file_evidence(rootserver)
        if rootserver_source_after != rootserver_input:
            raise CompositionError("rootserver changed during composition")
        verify_binutils_unchanged(tools)

        provenance: dict[str, Any] = {
            "schema": "cohesix-pi4-prebuilt-composition/v1",
            "status": "complete",
            "profile": PROFILE_NAME,
            "composition": "immutable-artifact-relink",
            "timestamp": timestamp,
            "repository_build": build_after,
            "binutils": tools,
            "inputs": input_evidence_after,
            "rootserver_input": rootserver_input,
            "object_order": [str(path) for path in object_order],
            "linker_script": str(LINKER_SCRIPT),
            "libcpio": str(LIBCPIO),
            "baseline_oracle": oracle,
            "archive": archive_record,
            "elf_layout": elf_layout,
            "image_metadata": image_metadata,
            "artifacts": {
                name: {
                    **file_evidence(path),
                    "path": str(output_dir / name),
                }
                for name, path in publish_paths.items()
            },
        }
        provenance_path = publish_scratch / OUTPUT_PROVENANCE
        _write_json(provenance_path, provenance)

        output_dir.mkdir(parents=True, exist_ok=True)
        (output_dir / OUTPUT_PROVENANCE).unlink(missing_ok=True)
        for name, source in publish_paths.items():
            os.replace(source, output_dir / name)
        for name, expected in provenance["artifacts"].items():
            observed = file_evidence(output_dir / name)
            if observed != expected:
                raise CompositionError(
                    f"published composition artifact failed rehash: {name}"
                )
        os.replace(provenance_path, output_dir / OUTPUT_PROVENANCE)
        try:
            published_provenance = json.loads(
                (output_dir / OUTPUT_PROVENANCE).read_text(encoding="utf-8")
            )
        except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
            (output_dir / OUTPUT_PROVENANCE).unlink(missing_ok=True)
            raise CompositionError(
                "published composition provenance cannot be verified"
            ) from exc
        if published_provenance != provenance:
            (output_dir / OUTPUT_PROVENANCE).unlink(missing_ok=True)
            raise CompositionError(
                "published composition provenance differs from verified record"
            )

    return provenance


def _timestamp(value: str) -> int:
    """Parse an explicit uint32 image timestamp."""

    try:
        parsed = int(value, 10)
    except ValueError as exc:
        raise argparse.ArgumentTypeError(
            f"timestamp must be an integer, got {value!r}"
        ) from exc
    if not 0 <= parsed <= 0xFFFFFFFF:
        raise argparse.ArgumentTypeError("timestamp must fit uint32")
    return parsed


def main(argv: list[str]) -> int:
    """Compose one immutable-repository Pi 4 image from command-line inputs."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--sel4-build-dir",
        type=Path,
        required=True,
        help="canonical immutable repository seL4/build_UBOOT directory",
    )
    parser.add_argument(
        "--rootserver",
        type=Path,
        required=True,
        help="exact Cohesix root-task ELF to embed and publish",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        required=True,
        help="external directory receiving the verified composition outputs",
    )
    parser.add_argument(
        "--timestamp",
        type=_timestamp,
        required=True,
        help="exact legacy U-Boot header timestamp as uint32 seconds",
    )
    args = parser.parse_args(argv)

    try:
        provenance = compose(
            build_dir=args.sel4_build_dir,
            rootserver=args.rootserver,
            output_dir=args.output_dir,
            timestamp=args.timestamp,
        )
    except CompositionError as exc:
        parser.error(str(exc))
        return 2  # pragma: no cover - argparse exits

    image = provenance["artifacts"][OUTPUT_IMAGE]
    print(
        "[pi4-compose] PASS "
        f"image={image['path']} size={image['size']} sha256={image['sha256']} "
        f"oracle_payload={provenance['baseline_oracle']['payload_sha256']}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
