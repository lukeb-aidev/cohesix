#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Seal and verify content-derived identities in staged Pi 4 boot images.
# Copyright 2026 Lukas Bower

"""Bind a root-task UART marker to one complete Pi 4 U-Boot image.

A boot image cannot literally contain its own ordinary SHA-256 digest. Cohesix
therefore defines a normalized image identity: hash every byte of the complete
legacy U-Boot image after replacing only the fixed-width ``image-id`` marker
field and the U-Boot header/data CRC fields with zeroes. The staging step writes
that digest into the marker and repairs both CRCs. Re-normalizing the sealed
image yields the same digest, while any other kernel, elfloader, root-task, or
header byte changes the identity.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import struct
import sys
import tempfile
import zlib
from dataclasses import asdict, dataclass, fields, replace
from datetime import datetime
from pathlib import Path


SCHEMA = "cohesix-pi4-image-identity/v2"
IDENTITY_DOMAIN = b"cohesix-pi4-legacy-image-id/v1\0"
BUILD_ID_DOMAIN = b"cohesix-pi4-build-id/v1\0"
UIMAGE_MAGIC = 0x27051956
UIMAGE_HEADER_BYTES = 64
UIMAGE_HEADER_CRC_OFFSET = 4
UIMAGE_DATA_SIZE_OFFSET = 12
UIMAGE_DATA_CRC_OFFSET = 24
CRC_BYTES = 4
IMAGE_ID_BYTES = 64
MAX_UIMAGE_PAYLOAD_BYTES = 64 * 1024 * 1024
EXPECTED_UIMAGE_LOAD_ADDRESS = 0x1000_0000
EXPECTED_UIMAGE_ENTRY_POINT = 0x1000_0000
UIMAGE_OS_LINUX = 5
UIMAGE_ARCH_ARM64 = 22
UIMAGE_TYPE_KERNEL = 2
UIMAGE_COMPRESSION_NONE = 0
ELF_SECTION_NAME = b".cohesix_build_marker"
ELF_CLASS_64 = 2
ELF_DATA_LITTLE_ENDIAN = 1
ELF_VERSION_CURRENT = 1
ELF_ET_EXEC = 2
ELF_EM_AARCH64 = 183
ELF_PT_LOAD = 1
ELF_PF_R = 0x4
ELF_SHT_PROGBITS = 1
ELF_SHT_STRTAB = 3
ELF_SHF_ALLOC = 0x2
ELF_HEADER_BYTES = 64
ELF_PROGRAM_HEADER_BYTES = 56
ELF_SECTION_HEADER_BYTES = 64
NEWC_MAGIC = b"070701"
NEWC_HEADER_BYTES = 110
NEWC_TRAILER = b"TRAILER!!!"
NEWC_REGULAR_FILE = 0o100000
NEWC_FILE_TYPE_MASK = 0o170000
ROOTSERVER_MEMBER = b"rootserver"
UNSEALED_IMAGE_ID = "0" * IMAGE_ID_BYTES
FULL_GIT_COMMIT_RE = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})")
SHA256_RE = re.compile(r"[0-9a-f]{64}")
CRC32_RE = re.compile(r"[0-9a-f]{8}")
BUILD_MARKER_RE = re.compile(
    rb"\[BUILD\] (?P<embedded_commit>[0-9a-f]{7,64})(?P<dirty>-dirty)? "
    rb"(?P<build_timestamp>[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:"
    rb"[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?(?:Z|[+-][0-9]{2}:[0-9]{2})) "
    rb"image-id=(?P<image_id>[0-9a-f]{64}) "
    rb"features=\[kernel:[01] bootstrap-trace:[01] serial-console:[01] net:[01] "
    rb"net-console:[01] qemu-driver-task-smoke:[01]\]"
)


class ImageIdentityError(ValueError):
    """Raised when a Pi image cannot be safely sealed or verified."""


@dataclass(frozen=True)
class StableFileSnapshot:
    """Bytes and descriptor-derived identity from one stable regular-file read."""

    data: bytes
    resolved_path: str
    device: int
    inode: int
    mode: int
    size_bytes: int
    mtime_ns: int
    ctime_ns: int


@dataclass(frozen=True)
class BuildMarker:
    """Validated fields carried by the sole canonical root-task marker."""

    text: str
    embedded_commit: str
    dirty: bool
    build_timestamp: str
    image_id: str


@dataclass(frozen=True)
class ImageIdentity:
    """Verified content, provenance, and stable file metadata for one image."""

    schema: str
    path: str
    image_id: str
    image_sha256: str
    build_marker: str
    build_marker_sha256: str
    size_bytes: int
    device: int
    inode: int
    mtime_ns: int
    ctime_ns: int
    uimage_header_crc32: str
    uimage_data_crc32: str
    git_commit: str | None
    embedded_git_commit: str
    source_tree_clean: bool | None
    build_timestamp: str
    build_id: str | None
    rootserver_sha256: str | None
    rootserver_cpio_sha256: str | None
    rootserver_member: str | None


@dataclass(frozen=True)
class NewcMember:
    """One strictly parsed newc member and its exact data location."""

    name: bytes
    mode: int
    data_offset: int
    data: bytes


@dataclass(frozen=True)
class ArchiveMembership:
    """Proof that one exact rootserver archive is embedded in one image."""

    build_marker: str
    sealed: bool
    archive_offset: int
    rootserver_data_offset: int
    rootserver_sha256: str
    rootserver_cpio_sha256: str


def _u32_be(data: bytes | bytearray, offset: int) -> int:
    """Read one big-endian 32-bit integer from a checked image buffer."""

    return struct.unpack_from(">I", data, offset)[0]


def _put_u32_be(data: bytearray, offset: int, value: int) -> None:
    """Write one big-endian 32-bit integer into an image buffer."""

    struct.pack_into(">I", data, offset, value)


def _validate_uimage(data: bytes | bytearray) -> tuple[int, int]:
    """Validate the exact legacy U-Boot envelope and return stored CRCs."""

    if len(data) < UIMAGE_HEADER_BYTES:
        raise ImageIdentityError("image is shorter than the U-Boot header")
    if _u32_be(data, 0) != UIMAGE_MAGIC:
        raise ImageIdentityError("image is not a legacy U-Boot uImage")

    payload_size = _u32_be(data, UIMAGE_DATA_SIZE_OFFSET)
    if payload_size == 0 or payload_size > MAX_UIMAGE_PAYLOAD_BYTES:
        raise ImageIdentityError(
            f"U-Boot payload size is outside the supported bound: {payload_size}"
        )
    if payload_size != len(data) - UIMAGE_HEADER_BYTES:
        raise ImageIdentityError(
            "U-Boot payload size does not cover the complete image: "
            f"header={payload_size} actual={len(data) - UIMAGE_HEADER_BYTES}"
        )

    load_address = _u32_be(data, 16)
    entry_point = _u32_be(data, 20)
    if load_address != EXPECTED_UIMAGE_LOAD_ADDRESS:
        raise ImageIdentityError(
            f"unexpected U-Boot load address: 0x{load_address:08x}"
        )
    if entry_point != EXPECTED_UIMAGE_ENTRY_POINT:
        raise ImageIdentityError(
            f"unexpected U-Boot entry point: 0x{entry_point:08x}"
        )
    envelope = tuple(data[28:32])
    expected_envelope = (
        UIMAGE_OS_LINUX,
        UIMAGE_ARCH_ARM64,
        UIMAGE_TYPE_KERNEL,
        UIMAGE_COMPRESSION_NONE,
    )
    if envelope != expected_envelope:
        raise ImageIdentityError(
            "unexpected U-Boot OS/architecture/type/compression: "
            f"actual={envelope} expected={expected_envelope}"
        )

    stored_header_crc = _u32_be(data, UIMAGE_HEADER_CRC_OFFSET)
    stored_data_crc = _u32_be(data, UIMAGE_DATA_CRC_OFFSET)
    header = bytearray(data[:UIMAGE_HEADER_BYTES])
    header[UIMAGE_HEADER_CRC_OFFSET : UIMAGE_HEADER_CRC_OFFSET + CRC_BYTES] = (
        b"\0" * CRC_BYTES
    )
    actual_header_crc = zlib.crc32(header) & 0xFFFF_FFFF
    actual_data_crc = zlib.crc32(data[UIMAGE_HEADER_BYTES:]) & 0xFFFF_FFFF
    if actual_header_crc != stored_header_crc:
        raise ImageIdentityError(
            "U-Boot header CRC mismatch: "
            f"stored={stored_header_crc:08x} actual={actual_header_crc:08x}"
        )
    if actual_data_crc != stored_data_crc:
        raise ImageIdentityError(
            "U-Boot data CRC mismatch: "
            f"stored={stored_data_crc:08x} actual={actual_data_crc:08x}"
        )
    return stored_header_crc, stored_data_crc


def _validate_build_timestamp(value: str) -> None:
    """Require one timezone-qualified RFC3339-compatible build timestamp."""

    normalized = f"{value[:-1]}+00:00" if value.endswith("Z") else value
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as error:
        raise ImageIdentityError(f"invalid build timestamp: {value}") from error
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise ImageIdentityError("build timestamp must include a timezone")


def _parse_build_marker(match: re.Match[bytes]) -> BuildMarker:
    """Decode and validate the structured fields of one canonical marker."""

    text = match.group(0).decode("ascii")
    timestamp = match.group("build_timestamp").decode("ascii")
    _validate_build_timestamp(timestamp)
    return BuildMarker(
        text=text,
        embedded_commit=match.group("embedded_commit").decode("ascii"),
        dirty=match.group("dirty") is not None,
        build_timestamp=timestamp,
        image_id=match.group("image_id").decode("ascii"),
    )


def _single_marker(data: bytes | bytearray) -> re.Match[bytes]:
    """Return the sole canonical build marker in an image, failing closed."""

    matches = list(BUILD_MARKER_RE.finditer(data))
    if len(matches) != 1:
        raise ImageIdentityError(
            f"expected exactly one canonical build marker, found {len(matches)}"
        )
    if data.count(b"image-id=") != 1:
        raise ImageIdentityError(
            "expected exactly one image-id normalization candidate"
        )
    if data.count(b"[BUILD] ") != 1:
        raise ImageIdentityError("expected exactly one build marker prefix")
    marker = matches[0]
    if marker.start() < UIMAGE_HEADER_BYTES and data[:4] == struct.pack(
        ">I", UIMAGE_MAGIC
    ):
        raise ImageIdentityError("build marker is outside the declared U-Boot payload")
    _parse_build_marker(marker)
    return marker


def _normalized_image_id(
    data: bytes | bytearray,
    image_id_start: int,
    image_id_end: int,
) -> str:
    """Hash the image after normalizing only the self-reference and CRCs."""

    if image_id_end - image_id_start != IMAGE_ID_BYTES:
        raise ImageIdentityError("build marker image-id field is not 64 bytes")
    normalized = bytearray(data)
    normalized[UIMAGE_HEADER_CRC_OFFSET : UIMAGE_HEADER_CRC_OFFSET + CRC_BYTES] = (
        b"\0" * CRC_BYTES
    )
    normalized[UIMAGE_DATA_CRC_OFFSET : UIMAGE_DATA_CRC_OFFSET + CRC_BYTES] = (
        b"\0" * CRC_BYTES
    )
    normalized[image_id_start:image_id_end] = UNSEALED_IMAGE_ID.encode("ascii")
    digest = hashlib.sha256()
    digest.update(IDENTITY_DOMAIN)
    digest.update(normalized)
    return digest.hexdigest()


def canonical_build_id(git_commit: str, build_timestamp: str, image_id: str) -> str:
    """Derive the canonical build ID from commit, timestamp, and image identity."""

    if FULL_GIT_COMMIT_RE.fullmatch(git_commit) is None:
        raise ImageIdentityError("Git commit must be a full lowercase hex object ID")
    _validate_build_timestamp(build_timestamp)
    if SHA256_RE.fullmatch(image_id) is None or image_id == UNSEALED_IMAGE_ID:
        raise ImageIdentityError("build ID requires a sealed image identity")
    digest = hashlib.sha256()
    digest.update(BUILD_ID_DOMAIN)
    for value in (git_commit, build_timestamp, image_id):
        digest.update(value.encode("ascii"))
        digest.update(b"\0")
    return digest.hexdigest()


def seal_image_bytes(data: bytes) -> tuple[bytes, str, str]:
    """Seal one valid unsealed uImage and return bytes, ID, and marker."""

    _validate_uimage(data)
    marker = _single_marker(data)
    current_id = marker.group("image_id").decode("ascii")
    if current_id != UNSEALED_IMAGE_ID:
        raise ImageIdentityError(
            "image marker is already sealed or has a nonzero placeholder"
        )

    image_id_start, image_id_end = marker.span("image_id")
    image_id = _normalized_image_id(data, image_id_start, image_id_end)
    sealed = bytearray(data)
    sealed[image_id_start:image_id_end] = image_id.encode("ascii")

    data_crc = zlib.crc32(sealed[UIMAGE_HEADER_BYTES:]) & 0xFFFF_FFFF
    _put_u32_be(sealed, UIMAGE_DATA_CRC_OFFSET, data_crc)
    sealed[UIMAGE_HEADER_CRC_OFFSET : UIMAGE_HEADER_CRC_OFFSET + CRC_BYTES] = (
        b"\0" * CRC_BYTES
    )
    header_crc = zlib.crc32(sealed[:UIMAGE_HEADER_BYTES]) & 0xFFFF_FFFF
    _put_u32_be(sealed, UIMAGE_HEADER_CRC_OFFSET, header_crc)

    _validate_uimage(sealed)
    sealed_marker = _single_marker(sealed)
    sealed_id = sealed_marker.group("image_id").decode("ascii")
    normalized_id = _normalized_image_id(
        sealed, *sealed_marker.span("image_id")
    )
    if sealed_id != image_id or normalized_id != image_id:
        raise ImageIdentityError("sealed image failed normalized identity verification")
    return bytes(sealed), image_id, sealed_marker.group(0).decode("ascii")


def inspect_image_bytes(
    data: bytes,
    *,
    path: str = "<memory>",
    device: int = 0,
    inode: int = 0,
    mtime_ns: int = 0,
    ctime_ns: int = 0,
) -> ImageIdentity:
    """Verify one sealed image buffer and return its complete base identity."""

    header_crc, data_crc = _validate_uimage(data)
    marker_match = _single_marker(data)
    marker = _parse_build_marker(marker_match)
    if marker.image_id == UNSEALED_IMAGE_ID:
        raise ImageIdentityError("image marker still contains the unsealed identity")
    normalized_id = _normalized_image_id(data, *marker_match.span("image_id"))
    if marker.image_id != normalized_id:
        raise ImageIdentityError(
            "image marker identity does not match normalized image SHA-256: "
            f"marker={marker.image_id} calculated={normalized_id}"
        )

    marker_bytes = marker_match.group(0)
    return ImageIdentity(
        schema=SCHEMA,
        path=path,
        image_id=marker.image_id,
        image_sha256=hashlib.sha256(data).hexdigest(),
        build_marker=marker.text,
        build_marker_sha256=hashlib.sha256(marker_bytes).hexdigest(),
        size_bytes=len(data),
        device=device,
        inode=inode,
        mtime_ns=mtime_ns,
        ctime_ns=ctime_ns,
        uimage_header_crc32=f"{header_crc:08x}",
        uimage_data_crc32=f"{data_crc:08x}",
        git_commit=None,
        embedded_git_commit=marker.embedded_commit,
        source_tree_clean=None,
        build_timestamp=marker.build_timestamp,
        build_id=None,
        rootserver_sha256=None,
        rootserver_cpio_sha256=None,
        rootserver_member=None,
    )


def _resolved_path(path: Path) -> str:
    """Return a deterministic resolved-path alias identity."""

    try:
        return str(path.resolve(strict=False))
    except (OSError, RuntimeError, ValueError):
        return str(path.absolute())


def paths_alias(first: Path, second: Path) -> bool:
    """Return whether paths are equal, resolve equally, or name one inode."""

    if first == second or _resolved_path(first) == _resolved_path(second):
        return True
    try:
        return first.samefile(second)
    except (OSError, ValueError):
        return False


def read_stable_regular_file(path: Path) -> StableFileSnapshot:
    """Open once, read once, and reject any descriptor identity mutation."""

    flags = os.O_RDONLY
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NONBLOCK", 0)
    descriptor: int | None = None
    try:
        descriptor = os.open(path, flags)
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ImageIdentityError(f"input is not a regular file: {path}")
        chunks: list[bytes] = []
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            chunks.append(chunk)
        data = b"".join(chunks)
        after = os.fstat(descriptor)
    except ImageIdentityError:
        raise
    except (OSError, ValueError) as error:
        raise ImageIdentityError(f"failed to read {path}: {error}") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)

    if not stat.S_ISREG(after.st_mode):
        raise ImageIdentityError(f"input ceased to be a regular file: {path}")
    stable_fields = (
        before.st_dev == after.st_dev,
        before.st_ino == after.st_ino,
        before.st_mode == after.st_mode,
        before.st_size == after.st_size == len(data),
        before.st_mtime_ns == after.st_mtime_ns,
        before.st_ctime_ns == after.st_ctime_ns,
    )
    if not all(stable_fields):
        raise ImageIdentityError(f"file changed while it was being read: {path}")
    return StableFileSnapshot(
        data=data,
        resolved_path=_resolved_path(path),
        device=after.st_dev,
        inode=after.st_ino,
        mode=after.st_mode,
        size_bytes=after.st_size,
        mtime_ns=after.st_mtime_ns,
        ctime_ns=after.st_ctime_ns,
    )


def inspect_image(path: Path) -> ImageIdentity:
    """Read and verify one stable image through a single open descriptor."""

    snapshot = read_stable_regular_file(path)
    return inspect_image_bytes(
        snapshot.data,
        path=str(path),
        device=snapshot.device,
        inode=snapshot.inode,
        mtime_ns=snapshot.mtime_ns,
        ctime_ns=snapshot.ctime_ns,
    )


def inspect_unsealed_marker(path: Path) -> str:
    """Verify that one build artifact carries exactly one zeroed marker."""

    snapshot = read_stable_regular_file(path)
    marker = _single_marker(snapshot.data)
    if marker.group("image_id").decode("ascii") != UNSEALED_IMAGE_ID:
        raise ImageIdentityError(
            f"artifact marker is not the unsealed placeholder: {path}"
        )
    return marker.group(0).decode("ascii")


def _bounded_region(data: bytes, offset: int, size: int, label: str) -> memoryview:
    """Return one checked file region without accepting wrap or truncation."""

    if offset < 0 or size < 0 or offset > len(data) or size > len(data) - offset:
        raise ImageIdentityError(f"ELF {label} is outside the file")
    return memoryview(data)[offset : offset + size]


def _verify_unsealed_elf_marker_bytes(data: bytes, *, path: Path) -> str:
    """Validate one ELF64 AArch64 executable marker and its exact load mapping."""

    marker = _single_marker(data)
    if marker.group("image_id").decode("ascii") != UNSEALED_IMAGE_ID:
        raise ImageIdentityError(f"ELF marker is not the unsealed placeholder: {path}")
    if len(data) < ELF_HEADER_BYTES or data[:4] != b"\x7fELF":
        raise ImageIdentityError(f"artifact is not an ELF file: {path}")
    if tuple(data[4:7]) != (
        ELF_CLASS_64,
        ELF_DATA_LITTLE_ENDIAN,
        ELF_VERSION_CURRENT,
    ):
        raise ImageIdentityError(f"artifact is not little-endian ELF64: {path}")
    if struct.unpack_from("<H", data, 16)[0] != ELF_ET_EXEC:
        raise ImageIdentityError("ELF rootserver is not an executable")
    if struct.unpack_from("<H", data, 18)[0] != ELF_EM_AARCH64:
        raise ImageIdentityError("ELF rootserver is not AArch64")
    if struct.unpack_from("<I", data, 20)[0] != ELF_VERSION_CURRENT:
        raise ImageIdentityError("ELF header version is invalid")

    program_offset = struct.unpack_from("<Q", data, 32)[0]
    section_offset = struct.unpack_from("<Q", data, 40)[0]
    header_bytes = struct.unpack_from("<H", data, 52)[0]
    program_entry_bytes = struct.unpack_from("<H", data, 54)[0]
    program_count = struct.unpack_from("<H", data, 56)[0]
    section_entry_bytes = struct.unpack_from("<H", data, 58)[0]
    section_count = struct.unpack_from("<H", data, 60)[0]
    string_section_index = struct.unpack_from("<H", data, 62)[0]
    if header_bytes != ELF_HEADER_BYTES:
        raise ImageIdentityError("ELF header size is invalid")
    if program_entry_bytes != ELF_PROGRAM_HEADER_BYTES or program_count == 0:
        raise ImageIdentityError("ELF has no valid program-header table")
    if section_entry_bytes != ELF_SECTION_HEADER_BYTES or section_count == 0:
        raise ImageIdentityError("ELF has no valid section-header table")
    if string_section_index == 0 or string_section_index >= section_count:
        raise ImageIdentityError("ELF section-name table index is invalid")

    _bounded_region(
        data,
        program_offset,
        program_entry_bytes * program_count,
        "program-header table",
    )
    _bounded_region(
        data,
        section_offset,
        section_entry_bytes * section_count,
        "section-header table",
    )

    string_header = section_offset + string_section_index * section_entry_bytes
    if struct.unpack_from("<I", data, string_header + 4)[0] != ELF_SHT_STRTAB:
        raise ImageIdentityError("ELF section-name table is not SHT_STRTAB")
    string_offset = struct.unpack_from("<Q", data, string_header + 24)[0]
    string_size = struct.unpack_from("<Q", data, string_header + 32)[0]
    strings = bytes(
        _bounded_region(data, string_offset, string_size, "section-name table")
    )

    marker_sections: list[tuple[int, int, int, int, int]] = []
    for section_index in range(section_count):
        header = section_offset + section_index * section_entry_bytes
        name_offset = struct.unpack_from("<I", data, header)[0]
        if name_offset >= len(strings):
            raise ImageIdentityError("ELF section name offset is invalid")
        name_end = strings.find(b"\0", name_offset)
        if name_end < 0:
            raise ImageIdentityError("ELF section name is unterminated")
        if strings[name_offset:name_end] != ELF_SECTION_NAME:
            continue
        section_type = struct.unpack_from("<I", data, header + 4)[0]
        flags = struct.unpack_from("<Q", data, header + 8)[0]
        address = struct.unpack_from("<Q", data, header + 16)[0]
        file_offset = struct.unpack_from("<Q", data, header + 24)[0]
        file_size = struct.unpack_from("<Q", data, header + 32)[0]
        marker_sections.append(
            (section_type, flags, address, file_offset, file_size)
        )

    if len(marker_sections) != 1:
        raise ImageIdentityError(
            "ELF must contain exactly one .cohesix_build_marker section"
        )
    section_type, flags, address, file_offset, file_size = marker_sections[0]
    if section_type != ELF_SHT_PROGBITS:
        raise ImageIdentityError("ELF build-marker section is not SHT_PROGBITS")
    if flags & ELF_SHF_ALLOC == 0:
        raise ImageIdentityError("ELF build-marker section is not allocated")
    section_data = bytes(
        _bounded_region(data, file_offset, file_size, "build-marker section")
    )
    if file_offset != marker.start() or file_size != len(marker.group(0)):
        raise ImageIdentityError(
            "canonical marker does not exactly occupy its dedicated ELF section"
        )
    if section_data != marker.group(0):
        raise ImageIdentityError("ELF build-marker section content is inconsistent")

    containing_loads = 0
    for program_index in range(program_count):
        header = program_offset + program_index * program_entry_bytes
        program_type = struct.unpack_from("<I", data, header)[0]
        if program_type != ELF_PT_LOAD:
            continue
        program_flags = struct.unpack_from("<I", data, header + 4)[0]
        load_offset = struct.unpack_from("<Q", data, header + 8)[0]
        load_address = struct.unpack_from("<Q", data, header + 16)[0]
        load_file_size = struct.unpack_from("<Q", data, header + 32)[0]
        load_memory_size = struct.unpack_from("<Q", data, header + 40)[0]
        load_alignment = struct.unpack_from("<Q", data, header + 48)[0]
        if load_file_size > load_memory_size:
            raise ImageIdentityError("ELF PT_LOAD p_filesz exceeds p_memsz")
        _bounded_region(data, load_offset, load_file_size, "PT_LOAD")
        if load_alignment not in (0, 1):
            if load_alignment & (load_alignment - 1):
                raise ImageIdentityError("ELF PT_LOAD alignment is not a power of two")
            if load_offset % load_alignment != load_address % load_alignment:
                raise ImageIdentityError("ELF PT_LOAD offset/address alignment differs")
        file_covered = (
            file_offset >= load_offset
            and file_size <= load_file_size
            and file_offset - load_offset <= load_file_size - file_size
        )
        memory_covered = (
            address >= load_address
            and file_size <= load_memory_size
            and address - load_address <= load_memory_size - file_size
        )
        if not (file_covered and memory_covered):
            continue
        if program_flags & ELF_PF_R == 0:
            raise ImageIdentityError("ELF marker PT_LOAD is not readable")
        if address - load_address != file_offset - load_offset:
            raise ImageIdentityError(
                "ELF marker section file/address mapping is incongruent"
            )
        containing_loads += 1
    if containing_loads != 1:
        raise ImageIdentityError(
            "ELF build-marker section must belong to exactly one readable PT_LOAD"
        )
    return marker.group(0).decode("ascii")


def verify_unsealed_elf_marker(path: Path) -> str:
    """Require the sole placeholder in one strict AArch64 PT_LOAD section."""

    snapshot = read_stable_regular_file(path)
    return _verify_unsealed_elf_marker_bytes(snapshot.data, path=path)


def _align4(value: int) -> int:
    """Round one checked newc offset up to a four-byte boundary."""

    return (value + 3) & ~3


def _parse_newc_hex(field: bytes, label: str) -> int:
    """Parse one exact eight-character newc hexadecimal field."""

    if len(field) != 8 or re.fullmatch(rb"[0-9A-Fa-f]{8}", field) is None:
        raise ImageIdentityError(f"newc {label} field is not eight hexadecimal bytes")
    return int(field, 16)


def parse_newc(data: bytes) -> list[NewcMember]:
    """Strictly parse one complete SVR4 newc archive."""

    members: list[NewcMember] = []
    offset = 0
    trailer_seen = False
    while not trailer_seen:
        if offset % 4 != 0:
            raise ImageIdentityError("newc header is not four-byte aligned")
        if offset > len(data) or NEWC_HEADER_BYTES > len(data) - offset:
            raise ImageIdentityError("newc archive is truncated before its trailer")
        header = data[offset : offset + NEWC_HEADER_BYTES]
        if header[:6] != NEWC_MAGIC:
            raise ImageIdentityError("newc member has an invalid magic")
        labels = (
            "ino",
            "mode",
            "uid",
            "gid",
            "nlink",
            "mtime",
            "filesize",
            "devmajor",
            "devminor",
            "rdevmajor",
            "rdevminor",
            "namesize",
            "check",
        )
        values = [
            _parse_newc_hex(header[6 + index * 8 : 14 + index * 8], label)
            for index, label in enumerate(labels)
        ]
        mode = values[1]
        file_size = values[6]
        name_size = values[11]
        checksum = values[12]
        if checksum != 0:
            raise ImageIdentityError("newc non-CRC archive has a nonzero check field")
        if name_size < 2:
            raise ImageIdentityError("newc member name is empty")
        name_offset = offset + NEWC_HEADER_BYTES
        if name_offset > len(data) or name_size > len(data) - name_offset:
            raise ImageIdentityError("newc member name is truncated")
        name_field = data[name_offset : name_offset + name_size]
        if name_field[-1:] != b"\0" or b"\0" in name_field[:-1]:
            raise ImageIdentityError("newc member name is not singly NUL-terminated")
        name = name_field[:-1]
        name_padding_end = _align4(name_offset + name_size)
        if name_padding_end > len(data):
            raise ImageIdentityError("newc member name padding is truncated")
        if any(data[name_offset + name_size : name_padding_end]):
            raise ImageIdentityError("newc member name padding is nonzero")
        data_offset = name_padding_end
        if data_offset > len(data) or file_size > len(data) - data_offset:
            raise ImageIdentityError("newc member data is truncated")
        member_data = data[data_offset : data_offset + file_size]
        data_padding_end = _align4(data_offset + file_size)
        if data_padding_end > len(data):
            raise ImageIdentityError("newc member data padding is truncated")
        if any(data[data_offset + file_size : data_padding_end]):
            raise ImageIdentityError("newc member data padding is nonzero")
        offset = data_padding_end

        if name == NEWC_TRAILER:
            if file_size != 0:
                raise ImageIdentityError("newc TRAILER!!! member has data")
            trailer_seen = True
            if any(data[offset:]):
                raise ImageIdentityError(
                    "newc archive has nonzero bytes after TRAILER!!!"
                )
            continue
        members.append(
            NewcMember(
                name=name,
                mode=mode,
                data_offset=data_offset,
                data=member_data,
            )
        )
    return members


def _single_rootserver_member(
    cpio_data: bytes,
    root_data: bytes,
) -> NewcMember:
    """Require exactly one regular rootserver equal to the expected ELF."""

    members = parse_newc(cpio_data)
    root_members = [member for member in members if member.name == ROOTSERVER_MEMBER]
    if len(root_members) != 1:
        raise ImageIdentityError(
            "newc archive must contain exactly one rootserver, "
            f"found {len(root_members)}"
        )
    root_member = root_members[0]
    if root_member.mode & NEWC_FILE_TYPE_MASK != NEWC_REGULAR_FILE:
        raise ImageIdentityError("newc rootserver member is not a regular file")
    if root_member.data != root_data:
        raise ImageIdentityError(
            "newc rootserver member is not byte-identical to the expected ELF"
        )
    return root_member


def _find_exactly_once(haystack: bytes, needle: bytes, start: int, label: str) -> int:
    """Return one exact occurrence at or after start, rejecting zero or many."""

    first = haystack.find(needle, start)
    if first < 0:
        raise ImageIdentityError(f"{label} is absent from the wrapper payload")
    if haystack.find(needle, first + 1) >= 0:
        raise ImageIdentityError(
            f"{label} occurs more than once in the wrapper payload"
        )
    return first


def verify_uimage_root_archive(
    image_path: Path,
    root_elf_path: Path,
    root_cpio_path: Path,
) -> ArchiveMembership:
    """Bind one image marker to one exact rootserver member in one exact newc."""

    root_snapshot = read_stable_regular_file(root_elf_path)
    root_marker = _verify_unsealed_elf_marker_bytes(
        root_snapshot.data, path=root_elf_path
    )
    root_marker_bytes = root_marker.encode("ascii")
    root_marker_offset = root_snapshot.data.find(root_marker_bytes)
    if root_marker_offset < 0 or root_snapshot.data.find(
        root_marker_bytes, root_marker_offset + 1
    ) >= 0:
        raise ImageIdentityError("validated root ELF marker location is ambiguous")

    cpio_snapshot = read_stable_regular_file(root_cpio_path)
    root_member = _single_rootserver_member(
        cpio_snapshot.data, root_snapshot.data
    )
    image_snapshot = read_stable_regular_file(image_path)
    _validate_uimage(image_snapshot.data)
    image_marker = _single_marker(image_snapshot.data)
    marker_fields = _parse_build_marker(image_marker)
    sealed = marker_fields.image_id != UNSEALED_IMAGE_ID
    normalized_image = bytearray(image_snapshot.data)
    if sealed:
        inspect_image_bytes(image_snapshot.data)
        image_id_start, image_id_end = image_marker.span("image_id")
        normalized_image[image_id_start:image_id_end] = UNSEALED_IMAGE_ID.encode(
            "ascii"
        )
    normalized_bytes = bytes(normalized_image)
    archive_offset = _find_exactly_once(
        normalized_bytes,
        cpio_snapshot.data,
        UIMAGE_HEADER_BYTES,
        "exact rootserver newc archive",
    )
    expected_root_offset = archive_offset + root_member.data_offset
    actual_root_offset = _find_exactly_once(
        normalized_bytes,
        root_snapshot.data,
        UIMAGE_HEADER_BYTES,
        "exact validated root ELF",
    )
    if actual_root_offset != expected_root_offset:
        raise ImageIdentityError(
            "validated root ELF is a decoy outside the exact newc rootserver member"
        )
    expected_marker_offset = expected_root_offset + root_marker_offset
    if image_marker.start() != expected_marker_offset:
        raise ImageIdentityError(
            "wrapper marker is not the marker from the newc rootserver member"
        )
    if normalized_bytes[
        image_marker.start() : image_marker.end()
    ] != root_marker_bytes:
        raise ImageIdentityError("wrapper and validated root ELF markers differ")
    return ArchiveMembership(
        build_marker=image_marker.group(0).decode("ascii"),
        sealed=sealed,
        archive_offset=archive_offset,
        rootserver_data_offset=expected_root_offset,
        rootserver_sha256=hashlib.sha256(root_snapshot.data).hexdigest(),
        rootserver_cpio_sha256=hashlib.sha256(cpio_snapshot.data).hexdigest(),
    )


def verify_unsealed_uimage_embeds_root(
    image_path: Path,
    root_elf_path: Path,
    root_cpio_path: Path | None = None,
) -> str:
    """Compatibility entry point requiring real newc membership."""

    if root_cpio_path is None:
        raise ImageIdentityError(
            "rootserver verification requires the exact newc archive"
        )
    membership = verify_uimage_root_archive(
        image_path, root_elf_path, root_cpio_path
    )
    if membership.sealed:
        raise ImageIdentityError("wrapper marker is not the unsealed placeholder")
    return membership.build_marker


def _enrich_identity(
    identity: ImageIdentity,
    git_commit: str,
    source_tree_clean: bool,
    membership: ArchiveMembership,
) -> ImageIdentity:
    """Attach fail-closed source and rootserver provenance to an identity."""

    if FULL_GIT_COMMIT_RE.fullmatch(git_commit) is None:
        raise ImageIdentityError("Git commit must be a full lowercase hex object ID")
    if not source_tree_clean:
        raise ImageIdentityError("acceptance identity requires a clean source tree")
    try:
        marker_bytes = identity.build_marker.encode("ascii")
    except UnicodeEncodeError as error:
        raise ImageIdentityError(
            "metadata build marker is not ASCII"
        ) from error
    marker_match = BUILD_MARKER_RE.fullmatch(marker_bytes)
    if marker_match is None:
        raise ImageIdentityError("identity build marker is not canonical")
    marker = _parse_build_marker(marker_match)
    if marker.dirty:
        raise ImageIdentityError("acceptance identity marker contains -dirty")
    if not git_commit.startswith(marker.embedded_commit):
        raise ImageIdentityError(
            "embedded marker commit is not a prefix of the expected full Git commit"
        )
    if not membership.sealed:
        raise ImageIdentityError("metadata publication requires a sealed image")
    return replace(
        identity,
        git_commit=git_commit,
        embedded_git_commit=marker.embedded_commit,
        source_tree_clean=True,
        build_timestamp=marker.build_timestamp,
        build_id=canonical_build_id(
            git_commit, marker.build_timestamp, identity.image_id
        ),
        rootserver_sha256=membership.rootserver_sha256,
        rootserver_cpio_sha256=membership.rootserver_cpio_sha256,
        rootserver_member=ROOTSERVER_MEMBER.decode("ascii"),
    )


def _atomic_write(path: Path, data: bytes, mode: int) -> None:
    """Replace one file atomically without exposing partial output."""

    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb", dir=path.parent, prefix=f".{path.name}.", delete=False
        ) as temporary:
            temporary_path = temporary.name
            temporary.write(data)
            temporary.flush()
            os.fsync(temporary.fileno())
        os.chmod(temporary_path, stat.S_IMODE(mode))
        os.replace(temporary_path, path)
        temporary_path = None
    finally:
        if temporary_path is not None:
            try:
                os.unlink(temporary_path)
            except FileNotFoundError:
                pass


def seal_image(path: Path) -> ImageIdentity:
    """Atomically seal one staged image and verify its on-disk result."""

    snapshot = read_stable_regular_file(path)
    sealed, _image_id, _marker = seal_image_bytes(snapshot.data)
    _atomic_write(path, sealed, snapshot.mode)
    return inspect_image(path)


def write_metadata(path: Path, identity: ImageIdentity) -> None:
    """Atomically write deterministic JSON metadata for a verified image."""

    rendered = (json.dumps(asdict(identity), indent=2, sort_keys=True) + "\n").encode()
    mode = 0o644
    if path.exists():
        mode = path.stat().st_mode
    _atomic_write(path, rendered, mode)


def _require_int(record: dict[str, object], key: str) -> int:
    """Return one nonnegative JSON integer without accepting booleans."""

    value = record.get(key)
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise ImageIdentityError(f"metadata field {key} must be a nonnegative integer")
    return value


def parse_metadata_bytes(data: bytes, *, source: str = "metadata bytes") -> ImageIdentity:
    """Strictly validate one v2 identity sidecar already read from stable storage."""

    try:
        decoded = data.decode("utf-8")
        record = json.loads(decoded)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ImageIdentityError(
            f"identity metadata is not valid UTF-8 JSON: {source}"
        ) from error
    if not isinstance(record, dict):
        raise ImageIdentityError("identity metadata must be a JSON object")
    expected_keys = {field.name for field in fields(ImageIdentity)}
    actual_keys = set(record)
    if actual_keys != expected_keys:
        missing = sorted(expected_keys - actual_keys)
        extra = sorted(actual_keys - expected_keys)
        raise ImageIdentityError(
            f"identity metadata fields differ: missing={missing} extra={extra}"
        )
    for key in (
        "path",
        "image_id",
        "image_sha256",
        "build_marker",
        "build_marker_sha256",
        "uimage_header_crc32",
        "uimage_data_crc32",
        "git_commit",
        "embedded_git_commit",
        "build_timestamp",
        "build_id",
        "rootserver_sha256",
        "rootserver_cpio_sha256",
        "rootserver_member",
        "schema",
    ):
        if not isinstance(record.get(key), str) or not record[key]:
            raise ImageIdentityError(f"metadata field {key} must be a nonempty string")
    for key in ("size_bytes", "device", "inode", "mtime_ns", "ctime_ns"):
        _require_int(record, key)
    if record.get("source_tree_clean") is not True:
        raise ImageIdentityError("identity metadata source tree is not clean")
    identity = ImageIdentity(**record)  # type: ignore[arg-type]
    if identity.schema != SCHEMA:
        raise ImageIdentityError(
            f"unsupported identity metadata schema: {identity.schema}"
        )
    for key, value in (
        ("image_id", identity.image_id),
        ("image_sha256", identity.image_sha256),
        ("build_marker_sha256", identity.build_marker_sha256),
        ("build_id", identity.build_id or ""),
        ("rootserver_sha256", identity.rootserver_sha256 or ""),
        ("rootserver_cpio_sha256", identity.rootserver_cpio_sha256 or ""),
    ):
        if SHA256_RE.fullmatch(value) is None:
            raise ImageIdentityError(f"metadata field {key} is not lowercase SHA-256")
    for key, value in (
        ("uimage_header_crc32", identity.uimage_header_crc32),
        ("uimage_data_crc32", identity.uimage_data_crc32),
    ):
        if CRC32_RE.fullmatch(value) is None:
            raise ImageIdentityError(f"metadata field {key} is not lowercase CRC-32")
    if FULL_GIT_COMMIT_RE.fullmatch(identity.git_commit or "") is None:
        raise ImageIdentityError(
            "metadata Git commit is not a full lowercase object ID"
        )
    try:
        marker_bytes = identity.build_marker.encode("ascii")
    except UnicodeEncodeError as error:
        raise ImageIdentityError(
            "metadata build marker is not ASCII"
        ) from error
    marker_match = BUILD_MARKER_RE.fullmatch(marker_bytes)
    if marker_match is None:
        raise ImageIdentityError("metadata build marker is not canonical")
    marker = _parse_build_marker(marker_match)
    if marker.dirty:
        raise ImageIdentityError("metadata build marker contains -dirty")
    if marker.image_id != identity.image_id:
        raise ImageIdentityError("metadata marker image ID differs from metadata")
    if marker.embedded_commit != identity.embedded_git_commit:
        raise ImageIdentityError("metadata embedded commit differs from marker")
    if not (identity.git_commit or "").startswith(marker.embedded_commit):
        raise ImageIdentityError("metadata full commit does not match marker prefix")
    if marker.build_timestamp != identity.build_timestamp:
        raise ImageIdentityError("metadata build timestamp differs from marker")
    if hashlib.sha256(marker_bytes).hexdigest() != (
        identity.build_marker_sha256
    ):
        raise ImageIdentityError("metadata build marker SHA-256 is invalid")
    if canonical_build_id(
        identity.git_commit or "", identity.build_timestamp, identity.image_id
    ) != identity.build_id:
        raise ImageIdentityError("metadata canonical build ID is invalid")
    if identity.rootserver_member != ROOTSERVER_MEMBER.decode("ascii"):
        raise ImageIdentityError("metadata rootserver member name is invalid")
    return identity


def read_metadata(path: Path) -> ImageIdentity:
    """Read and strictly validate one v2 identity metadata sidecar."""

    snapshot = read_stable_regular_file(path)
    return parse_metadata_bytes(snapshot.data, source=str(path))


def publish_metadata(
    image_path: Path,
    metadata_path: Path,
    identity: ImageIdentity,
    *,
    git_commit: str,
    source_tree_clean: bool,
    root_elf_path: Path,
    root_cpio_path: Path,
) -> ImageIdentity:
    """Publish v2 metadata, then prove publication did not mutate the image."""

    if paths_alias(image_path, metadata_path):
        raise ImageIdentityError("metadata path aliases the image path")
    membership = verify_uimage_root_archive(
        image_path, root_elf_path, root_cpio_path
    )
    enriched = _enrich_identity(
        identity, git_commit, source_tree_clean, membership
    )
    write_metadata(metadata_path, enriched)
    if read_metadata(metadata_path) != enriched:
        raise ImageIdentityError("published identity metadata differs from its source")

    post_identity = inspect_image(image_path)
    post_membership = verify_uimage_root_archive(
        image_path, root_elf_path, root_cpio_path
    )
    post_enriched = _enrich_identity(
        post_identity, git_commit, source_tree_clean, post_membership
    )
    if post_enriched != enriched:
        raise ImageIdentityError("sealed image changed during metadata publication")
    return post_enriched


def _require_root_pair(args: argparse.Namespace) -> tuple[Path, Path] | None:
    """Return a complete root ELF/CPIO pair or reject a partial pair."""

    root_elf = getattr(args, "expected_root_elf", None)
    root_cpio = getattr(args, "expected_root_cpio", None)
    if (root_elf is None) != (root_cpio is None):
        raise ImageIdentityError(
            "--expected-root-elf and --expected-root-cpio must be supplied together"
        )
    if root_elf is None:
        return None
    return root_elf, root_cpio


def _add_root_pair(parser: argparse.ArgumentParser) -> None:
    """Add exact rootserver membership arguments to one parser."""

    parser.add_argument("--expected-root-elf", type=Path)
    parser.add_argument("--expected-root-cpio", type=Path)


def build_parser() -> argparse.ArgumentParser:
    """Build the seal/verify command-line interface."""

    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("seal", "verify"):
        command_parser = subparsers.add_parser(command)
        command_parser.add_argument("--image", required=True, type=Path)
        command_parser.add_argument("--metadata", type=Path)
        command_parser.add_argument("--git-commit")
        command_parser.add_argument("--source-tree-clean", action="store_true")
        _add_root_pair(command_parser)
    unsealed_parser = subparsers.add_parser("verify-unsealed-marker")
    unsealed_parser.add_argument("--artifact", required=True, type=Path)
    unsealed_parser.add_argument(
        "--require-elf-load-section",
        action="store_true",
        help="require the marker to exactly occupy one allocated PT_LOAD ELF section",
    )
    _add_root_pair(unsealed_parser)

    metadata_parser = subparsers.add_parser("verify-metadata")
    metadata_parser.add_argument("--image", required=True, type=Path)
    metadata_parser.add_argument("--metadata", required=True, type=Path)
    metadata_parser.add_argument("--expected-git-commit", required=True)
    metadata_parser.add_argument("--expected-build-id", required=True)
    _add_root_pair(metadata_parser)
    return parser


def _verify_metadata_command(args: argparse.Namespace) -> ImageIdentity:
    """Verify one sidecar against one exact image and rootserver archive."""

    root_pair = _require_root_pair(args)
    if root_pair is None:
        raise ImageIdentityError(
            "verify-metadata requires --expected-root-elf and --expected-root-cpio"
        )
    metadata = read_metadata(args.metadata)
    if metadata.git_commit != args.expected_git_commit:
        raise ImageIdentityError("metadata Git commit differs from the expected commit")
    if metadata.build_id != args.expected_build_id:
        raise ImageIdentityError("metadata build ID differs from the expected build ID")
    image = inspect_image(args.image)
    membership = verify_uimage_root_archive(args.image, *root_pair)
    expected = _enrich_identity(image, args.expected_git_commit, True, membership)
    if metadata != expected:
        raise ImageIdentityError("identity metadata does not match the exact image")
    return metadata


def main(argv: list[str] | None = None) -> int:
    """Seal or verify an image and print its deterministic identity JSON."""

    args = build_parser().parse_args(argv)
    try:
        if args.command == "verify-unsealed-marker":
            root_pair = _require_root_pair(args)
            if args.require_elf_load_section and root_pair is not None:
                raise ImageIdentityError(
                    "--require-elf-load-section and root archive checks are exclusive"
                )
            if root_pair is not None:
                marker = verify_unsealed_uimage_embeds_root(
                    args.artifact, *root_pair
                )
            elif args.require_elf_load_section:
                marker = verify_unsealed_elf_marker(args.artifact)
            else:
                marker = inspect_unsealed_marker(args.artifact)
            marker_match = BUILD_MARKER_RE.fullmatch(marker.encode("ascii"))
            if marker_match is None:
                raise ImageIdentityError("unsealed build marker is not canonical")
            marker_fields = _parse_build_marker(marker_match)
            print(
                json.dumps(
                    {
                        "artifact": str(args.artifact),
                        "build_marker": marker,
                        "build_marker_sha256": hashlib.sha256(
                            marker.encode("ascii")
                        ).hexdigest(),
                        "build_timestamp": marker_fields.build_timestamp,
                        "embedded_git_commit": marker_fields.embedded_commit,
                        "marker_dirty": marker_fields.dirty,
                        "marker_image_id": marker_fields.image_id,
                        "schema": SCHEMA,
                        "state": "unsealed",
                    },
                    sort_keys=True,
                )
            )
            return 0
        if args.command == "verify-metadata":
            identity = _verify_metadata_command(args)
            print(json.dumps(asdict(identity), sort_keys=True))
            return 0

        if args.metadata is not None and paths_alias(args.image, args.metadata):
            raise ImageIdentityError("metadata path aliases the image path")
        root_pair = _require_root_pair(args)
        if args.metadata is not None:
            if args.git_commit is None or not args.source_tree_clean:
                raise ImageIdentityError(
                    "metadata publication requires --git-commit and --source-tree-clean"
                )
            if root_pair is None:
                raise ImageIdentityError(
                    "metadata publication requires exact root ELF and newc inputs"
                )
        elif args.git_commit is not None or args.source_tree_clean:
            raise ImageIdentityError(
                "Git provenance arguments require --metadata"
            )

        identity = (
            seal_image(args.image)
            if args.command == "seal"
            else inspect_image(args.image)
        )
        if root_pair is not None:
            verify_uimage_root_archive(args.image, *root_pair)
        if args.metadata is not None:
            identity = publish_metadata(
                args.image,
                args.metadata,
                identity,
                git_commit=args.git_commit,
                source_tree_clean=args.source_tree_clean,
                root_elf_path=root_pair[0],
                root_cpio_path=root_pair[1],
            )
    except (ImageIdentityError, OSError) as error:
        print(f"pi4-image-identity: {error}", file=sys.stderr)
        return 2
    print(json.dumps(asdict(identity), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
