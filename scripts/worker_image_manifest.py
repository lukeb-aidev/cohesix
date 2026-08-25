#!/usr/bin/env python3
# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Canonicalize, validate, archive, and describe isolated Worker images.
# Author: Lukas Bower

"""Build and verify deterministic Worker image archives.

The image contract intentionally depends only on ELF program headers plus the
fixed ``.cohesix.worker`` admission record.  Canonical images discard section
tables and other non-loadable bytes so debug metadata and host tool versions do
not affect the bytes mapped into a Worker VSpace.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from dataclasses import dataclass
from pathlib import Path
import stat
import struct
import sys
from typing import Iterable, Sequence


SCHEMA = "cohesix-worker-image-manifest/v1"
ARCHIVE_MEMBER_ROOT = "cohesix/worker"
EXPECTED_TARGET = "aarch64-unknown-none"
EXPECTED_NAMES = ("worker-heart", "worker-gpu", "worker-lora")
ROLE_BY_NAME = {
    "worker-heart": ("worker-heartbeat", 1),
    "worker-gpu": ("worker-gpu", 2),
    "worker-lora": ("worker-lora", 3),
}
ELF_MAGIC = b"\x7fELF"
ELF_CLASS_64 = 2
ELF_DATA_LSB = 1
ELF_VERSION = 1
ELF_OSABI_SYSV = 0
ELF_OSABI_GNU = 3
ET_EXEC = 2
EM_AARCH64 = 183
PT_LOAD = 1
PF_X = 1
PF_W = 2
PF_R = 4
SHF_WRITE = 1
SHF_ALLOC = 2
SHF_EXECINSTR = 4
SHF_GNU_RETAIN = 0x200000
SHT_SYMTAB = 2
SHT_STRTAB = 3
SHT_DYNSYM = 11
WORKER_METADATA_NAME = ".cohesix.worker"
WORKER_METADATA_MAGIC = 0x574B4D32
WORKER_ABI_VERSION = 2
WORKER_ENTRY_VERSION = 2
WORKER_METADATA_FLAGS = 3
WORKER_METADATA_BYTES = 64
WORKER_ENTRY_SYMBOL = b"_start"
PAGE_BYTES = 4096
IPC_BUFFER_BYTES = 1024
STACK_BYTES = 16 * 1024
MAX_LOAD_SPAN = 2 * 1024 * 1024
MAX_IMAGE_BYTES = 512 * 1024
SHA256_HEX_BYTES = 64
NEWC_MAGIC = b"070701"
NEWC_HEADER_BYTES = 110


class WorkerImageError(ValueError):
    """Raised when an image, archive, or manifest fails closed."""


@dataclass(frozen=True)
class ProgramHeader:
    """One ELF64 program header."""

    kind: int
    flags: int
    offset: int
    vaddr: int
    filesz: int
    memsz: int
    align: int


@dataclass(frozen=True)
class SectionHeader:
    """One ELF64 section header."""

    name_offset: int
    kind: int
    flags: int
    address: int
    offset: int
    size: int
    link: int
    entry_size: int


@dataclass(frozen=True)
class ParsedElf:
    """Validated ELF structure needed by Worker admission."""

    entry: int
    program_headers: tuple[ProgramHeader, ...]
    section_headers: tuple[SectionHeader, ...]
    section_names: tuple[str, ...]
    header_bytes: int
    program_header_offset: int
    program_header_bytes: int


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _checked_slice(data: bytes, offset: int, size: int, label: str) -> bytes:
    if offset < 0 or size < 0 or offset > len(data) or size > len(data) - offset:
        raise WorkerImageError(f"{label} lies outside the ELF file")
    return data[offset : offset + size]


def _cstring(data: bytes, offset: int, label: str) -> str:
    if offset < 0 or offset >= len(data):
        raise WorkerImageError(f"{label} string offset is invalid")
    end = data.find(b"\0", offset)
    if end < 0:
        raise WorkerImageError(f"{label} string is not NUL terminated")
    try:
        value = data[offset:end].decode("ascii")
    except UnicodeDecodeError as error:
        raise WorkerImageError(f"{label} string is not ASCII") from error
    return value


def _parse_elf(data: bytes) -> ParsedElf:
    if len(data) < 64 or data[:4] != ELF_MAGIC:
        raise WorkerImageError("image is not an ELF file")
    ident = data[:16]
    if ident[4] != ELF_CLASS_64 or ident[5] != ELF_DATA_LSB:
        raise WorkerImageError("Worker image must be little-endian ELF64")
    if ident[6] != ELF_VERSION or ident[7] not in (ELF_OSABI_SYSV, ELF_OSABI_GNU):
        raise WorkerImageError("Worker image has an unsupported ELF identity")
    fields = struct.unpack_from("<HHIQQQIHHHHHH", data, 16)
    (
        elf_type,
        machine,
        version,
        entry,
        program_offset,
        section_offset,
        _flags,
        header_size,
        program_entry_size,
        program_count,
        section_entry_size,
        section_count,
        section_name_index,
    ) = fields
    if elf_type != ET_EXEC or machine != EM_AARCH64 or version != ELF_VERSION:
        raise WorkerImageError("Worker image must be an AArch64 ET_EXEC ELF")
    if header_size != 64 or program_entry_size != 56 or program_count == 0:
        raise WorkerImageError("Worker image has an unsupported ELF header layout")
    program_table_size = program_entry_size * program_count
    program_bytes = _checked_slice(
        data, program_offset, program_table_size, "program-header table"
    )
    program_headers: list[ProgramHeader] = []
    for index in range(program_count):
        raw = struct.unpack_from("<IIQQQQQQ", program_bytes, index * 56)
        kind, flags, offset, vaddr, _paddr, filesz, memsz, align = raw
        if filesz > memsz:
            raise WorkerImageError("ELF segment file size exceeds memory size")
        _checked_slice(data, offset, filesz, f"program header {index}")
        program_headers.append(
            ProgramHeader(kind, flags, offset, vaddr, filesz, memsz, align)
        )

    section_headers: list[SectionHeader] = []
    section_names: list[str] = []
    if section_count == 0 or section_offset == 0:
        raise WorkerImageError("source Worker ELF must retain its section table")
    if section_entry_size != 64 or section_name_index >= section_count:
        raise WorkerImageError("Worker image has an unsupported section table")
    section_bytes = _checked_slice(
        data,
        section_offset,
        section_entry_size * section_count,
        "section-header table",
    )
    for index in range(section_count):
        raw = struct.unpack_from("<IIQQQQIIQQ", section_bytes, index * 64)
        name, kind, flags, address, offset, size, link, _info, _align, entsize = raw
        if kind != 8:  # SHT_NOBITS has no file bytes.
            _checked_slice(data, offset, size, f"section header {index}")
        section_headers.append(
            SectionHeader(name, kind, flags, address, offset, size, link, entsize)
        )
    name_header = section_headers[section_name_index]
    if name_header.kind != SHT_STRTAB:
        raise WorkerImageError("ELF section-name table is not a string table")
    name_bytes = _checked_slice(
        data, name_header.offset, name_header.size, "section-name table"
    )
    for index, header in enumerate(section_headers):
        section_names.append(_cstring(name_bytes, header.name_offset, f"section {index}"))

    return ParsedElf(
        entry=entry,
        program_headers=tuple(program_headers),
        section_headers=tuple(section_headers),
        section_names=tuple(section_names),
        header_bytes=header_size,
        program_header_offset=program_offset,
        program_header_bytes=program_table_size,
    )


def _load_segments(parsed: ParsedElf) -> tuple[ProgramHeader, ...]:
    loads = tuple(header for header in parsed.program_headers if header.kind == PT_LOAD)
    if not loads:
        raise WorkerImageError("Worker image has no loadable segments")
    ordered = sorted(loads, key=lambda header: (header.vaddr, header.offset))
    previous_end = 0
    executable_entry = False
    for index, header in enumerate(ordered):
        if header.align < PAGE_BYTES or header.align & (header.align - 1):
            raise WorkerImageError("load segment alignment is not a page power-of-two")
        if header.flags & PF_W and header.flags & PF_X:
            raise WorkerImageError("Worker image contains a writable executable segment")
        if not header.flags & PF_R:
            raise WorkerImageError("every Worker load segment must be readable")
        end = header.vaddr + header.memsz
        if end < header.vaddr:
            raise WorkerImageError("load segment address wraps")
        if index and header.vaddr < previous_end:
            raise WorkerImageError("Worker load segments overlap")
        previous_end = end
        if header.flags & PF_X and header.vaddr <= parsed.entry < end:
            executable_entry = True
    if not executable_entry:
        raise WorkerImageError("ELF entrypoint is outside executable load memory")
    low = min(header.vaddr for header in ordered)
    high = max(header.vaddr + header.memsz for header in ordered)
    if high - low > MAX_LOAD_SPAN:
        raise WorkerImageError("Worker image exceeds the bounded load span")
    return tuple(ordered)


def _section(parsed: ParsedElf, name: str) -> SectionHeader:
    matches = [
        header
        for header, candidate in zip(parsed.section_headers, parsed.section_names)
        if candidate == name
    ]
    if len(matches) != 1:
        raise WorkerImageError(f"Worker image must contain exactly one {name} section")
    return matches[0]


def _validate_start_symbol(data: bytes, parsed: ParsedElf) -> None:
    matches: list[int] = []
    for header in parsed.section_headers:
        if header.kind not in (SHT_SYMTAB, SHT_DYNSYM):
            continue
        if header.entry_size != 24 or header.link >= len(parsed.section_headers):
            raise WorkerImageError("ELF symbol table has an invalid layout")
        strings = parsed.section_headers[header.link]
        if strings.kind != SHT_STRTAB:
            raise WorkerImageError("ELF symbol table does not reference strings")
        string_bytes = _checked_slice(data, strings.offset, strings.size, "symbol strings")
        symbols = _checked_slice(data, header.offset, header.size, "symbol table")
        if len(symbols) % 24:
            raise WorkerImageError("ELF symbol table is not record aligned")
        for offset in range(0, len(symbols), 24):
            name_offset, info, _other, section_index, value, _size = struct.unpack_from(
                "<IBBHQQ", symbols, offset
            )
            if section_index == 0 or info & 0x0F != 2:  # STT_FUNC
                continue
            name = _cstring(string_bytes, name_offset, "symbol")
            if name == WORKER_ENTRY_SYMBOL.decode("ascii"):
                matches.append(value)
    if matches != [parsed.entry]:
        raise WorkerImageError("ELF _start symbol is missing, duplicated, or not the entrypoint")


def _validate_metadata(
    data: bytes, parsed: ParsedElf, expected_role: int
) -> tuple[SectionHeader, bytes]:
    header = _section(parsed, WORKER_METADATA_NAME)
    if header.size != WORKER_METADATA_BYTES:
        raise WorkerImageError("Worker metadata record must be exactly 64 bytes")
    if header.flags != SHF_ALLOC | SHF_GNU_RETAIN:
        raise WorkerImageError("Worker metadata must be retained allocatable read-only data")
    metadata = _checked_slice(data, header.offset, header.size, "Worker metadata")
    magic, abi, length, role, entry_version, flags, symbol, reserved = struct.unpack(
        "<IHHHHI32s16s", metadata
    )
    if magic != WORKER_METADATA_MAGIC or length != WORKER_METADATA_BYTES:
        raise WorkerImageError("Worker metadata magic or length is invalid")
    if abi != WORKER_ABI_VERSION or entry_version != WORKER_ENTRY_VERSION:
        raise WorkerImageError("Worker metadata ABI or entry version is invalid")
    if role != expected_role:
        raise WorkerImageError("Worker metadata role does not match its image name")
    if flags != WORKER_METADATA_FLAGS or reserved != bytes(16):
        raise WorkerImageError("Worker metadata flags or reserved bytes are invalid")
    if symbol != WORKER_ENTRY_SYMBOL + bytes(32 - len(WORKER_ENTRY_SYMBOL)):
        raise WorkerImageError("Worker metadata entry symbol is invalid")
    containing = [
        segment
        for segment in _load_segments(parsed)
        if segment.offset <= header.offset
        and header.offset + header.size <= segment.offset + segment.filesz
    ]
    if len(containing) != 1 or containing[0].flags & (PF_W | PF_X):
        raise WorkerImageError("Worker metadata is not retained in one read-only segment")
    expected_vaddr = containing[0].vaddr + header.offset - containing[0].offset
    if expected_vaddr != header.address:
        raise WorkerImageError("Worker metadata file and virtual addresses disagree")
    return header, metadata


def _canonicalize(data: bytes, parsed: ParsedElf) -> bytes:
    loads = _load_segments(parsed)
    end = max(
        parsed.header_bytes,
        parsed.program_header_offset + parsed.program_header_bytes,
        *(header.offset + header.filesz for header in loads),
    )
    if end > MAX_IMAGE_BYTES:
        raise WorkerImageError("canonical Worker image exceeds its byte bound")
    canonical = bytearray(data[:end])
    struct.pack_into("<Q", canonical, 40, 0)  # e_shoff
    struct.pack_into("<HHH", canonical, 58, 0, 0, 0)
    return bytes(canonical)


def inspect_image(path: Path, name: str) -> tuple[dict[str, object], bytes]:
    """Validate one source image and return its manifest row and canonical bytes."""

    if name not in ROLE_BY_NAME:
        raise WorkerImageError(f"unknown executable Worker image name: {name}")
    data = path.read_bytes()
    parsed = _parse_elf(data)
    loads = _load_segments(parsed)
    _validate_start_symbol(data, parsed)
    metadata_header, metadata = _validate_metadata(data, parsed, ROLE_BY_NAME[name][1])
    canonical = _canonicalize(data, parsed)
    reparsed = _parse_canonical_elf(canonical)
    canonical_loads = _load_segments(reparsed)
    if canonical_loads != loads or reparsed.entry != parsed.entry:
        raise WorkerImageError("canonicalization changed load or entry semantics")
    low = min(header.vaddr for header in loads)
    high = max(header.vaddr + header.memsz for header in loads)
    role, _role_number = ROLE_BY_NAME[name]
    row: dict[str, object] = {
        "name": name,
        "role": role,
        "abi_version": WORKER_ABI_VERSION,
        "entry_version": WORKER_ENTRY_VERSION,
        "entry_symbol": WORKER_ENTRY_SYMBOL.decode("ascii"),
        "entry_vaddr": parsed.entry,
        "flags": ["pointer-free", "init-page-in-x0"],
        "archive_path": f"{ARCHIVE_MEMBER_ROOT}/{name}",
        "source_sha256": _sha256(data),
        "image_sha256": _sha256(canonical),
        "image_bytes": len(canonical),
        "load_base_vaddr": low,
        "load_limit_vaddr": high,
        "load_span_bytes": high - low,
        "metadata_vaddr": metadata_header.address,
        "metadata_sha256": _sha256(metadata),
        "stack_bytes": STACK_BYTES,
        "ipc_buffer_bytes": IPC_BUFFER_BYTES,
        "shared_page_bytes": PAGE_BYTES,
    }
    return row, canonical


def _parse_canonical_elf(data: bytes) -> ParsedElf:
    """Parse the sectionless canonical subset without weakening source checks."""

    if len(data) < 64 or data[:4] != ELF_MAGIC:
        raise WorkerImageError("canonical image is not ELF")
    fields = struct.unpack_from("<HHIQQQIHHHHHH", data, 16)
    (
        elf_type,
        machine,
        version,
        entry,
        program_offset,
        section_offset,
        _flags,
        header_size,
        program_entry_size,
        program_count,
        section_entry_size,
        section_count,
        section_name_index,
    ) = fields
    if (
        data[4] != ELF_CLASS_64
        or data[5] != ELF_DATA_LSB
        or elf_type != ET_EXEC
        or machine != EM_AARCH64
        or version != ELF_VERSION
        or header_size != 64
        or program_entry_size != 56
        or program_count == 0
        or section_offset != 0
        or section_entry_size != 0
        or section_count != 0
        or section_name_index != 0
    ):
        raise WorkerImageError("canonical Worker ELF header is invalid")
    table = _checked_slice(
        data, program_offset, program_count * program_entry_size, "program headers"
    )
    programs: list[ProgramHeader] = []
    for index in range(program_count):
        kind, flags, offset, vaddr, _paddr, filesz, memsz, align = struct.unpack_from(
            "<IIQQQQQQ", table, index * 56
        )
        if filesz > memsz:
            raise WorkerImageError("canonical segment file size exceeds memory size")
        _checked_slice(data, offset, filesz, f"canonical program header {index}")
        programs.append(ProgramHeader(kind, flags, offset, vaddr, filesz, memsz, align))
    return ParsedElf(
        entry=entry,
        program_headers=tuple(programs),
        section_headers=(),
        section_names=(),
        header_bytes=header_size,
        program_header_offset=program_offset,
        program_header_bytes=program_count * program_entry_size,
    )


def _pad4(value: int) -> int:
    return (-value) & 3


def _newc_entry(name: str, data: bytes, mode: int) -> bytes:
    try:
        name_bytes = name.encode("ascii") + b"\0"
    except UnicodeEncodeError as error:
        raise WorkerImageError("CPIO member name must be ASCII") from error
    fields = (
        0,
        mode,
        0,
        0,
        1,
        0,
        len(data),
        0,
        0,
        0,
        0,
        len(name_bytes),
        0,
    )
    header = NEWC_MAGIC + b"".join(f"{value:08x}".encode("ascii") for value in fields)
    if len(header) != NEWC_HEADER_BYTES:
        raise WorkerImageError("internal CPIO header length mismatch")
    return (
        header
        + name_bytes
        + bytes(_pad4(len(header) + len(name_bytes)))
        + data
        + bytes(_pad4(len(data)))
    )


def build_newc(files: Sequence[tuple[str, bytes]]) -> bytes:
    """Create byte-stable newc bytes with fixed ownership and timestamps."""

    names = [name for name, _data in files]
    if names != sorted(names) or len(names) != len(set(names)):
        raise WorkerImageError("CPIO members must be unique and sorted")
    output = bytearray()
    directories: set[str] = set()
    for name in names:
        parent = Path(name).parent
        while parent != Path("."):
            directories.add(parent.as_posix())
            parent = parent.parent
    file_map = dict(files)
    for name in sorted((*directories, *names)):
        if name in file_map:
            output.extend(_newc_entry(name, file_map[name], stat.S_IFREG | 0o555))
        else:
            output.extend(_newc_entry(name, b"", stat.S_IFDIR | 0o555))
    output.extend(_newc_entry("TRAILER!!!", b"", 0))
    output.extend(bytes((-len(output)) & 511))
    return bytes(output)


def parse_newc(data: bytes) -> dict[str, bytes]:
    """Strictly parse the deterministic newc subset."""

    offset = 0
    entries: dict[str, bytes] = {}
    last_name = ""
    while True:
        header = _checked_slice(data, offset, NEWC_HEADER_BYTES, "CPIO header")
        if header[:6] != NEWC_MAGIC:
            raise WorkerImageError("archive is not canonical newc")
        try:
            fields = tuple(
                int(header[6 + index * 8 : 14 + index * 8], 16) for index in range(13)
            )
        except ValueError as error:
            raise WorkerImageError("archive contains a malformed newc field") from error
        mode = fields[1]
        filesize = fields[6]
        namesize = fields[11]
        if namesize < 2:
            raise WorkerImageError("archive member name is empty")
        offset += NEWC_HEADER_BYTES
        raw_name = _checked_slice(data, offset, namesize, "CPIO member name")
        if raw_name[-1] != 0 or b"\0" in raw_name[:-1]:
            raise WorkerImageError("archive member name is not canonical")
        try:
            name = raw_name[:-1].decode("ascii")
        except UnicodeDecodeError as error:
            raise WorkerImageError("archive member name is not ASCII") from error
        offset += namesize
        offset += _pad4(NEWC_HEADER_BYTES + namesize)
        payload = _checked_slice(data, offset, filesize, f"CPIO member {name}")
        offset += filesize
        offset += _pad4(filesize)
        if name == "TRAILER!!!":
            if filesize != 0:
                raise WorkerImageError("CPIO trailer is not empty")
            if any(data[offset:]):
                raise WorkerImageError("CPIO archive has nonzero trailing bytes")
            return entries
        if name <= last_name or name in entries:
            raise WorkerImageError("CPIO archive members are duplicated or unsorted")
        last_name = name
        if stat.S_ISDIR(mode):
            if payload:
                raise WorkerImageError("CPIO directory contains payload bytes")
            continue
        if not stat.S_ISREG(mode):
            raise WorkerImageError("CPIO archive contains a non-regular member")
        entries[name] = payload


def _load_manifest(path: Path) -> dict[str, object]:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise WorkerImageError(f"cannot read Worker image manifest: {error}") from error
    if not isinstance(document, dict):
        raise WorkerImageError("Worker image manifest must be a JSON object")
    return document


def verify_manifest(manifest_path: Path, archive_path: Path) -> dict[str, object]:
    """Verify exact schema, hashes, role matrix, canonical images, and archive."""

    document = _load_manifest(manifest_path)
    expected_top = {"schema", "target", "profile", "archive", "images"}
    if set(document) != expected_top or document.get("schema") != SCHEMA:
        raise WorkerImageError("Worker image manifest schema or fields are invalid")
    if document.get("target") != EXPECTED_TARGET:
        raise WorkerImageError("Worker image manifest target is not AArch64 none")
    profile = document.get("profile")
    if not isinstance(profile, str) or not profile:
        raise WorkerImageError("Worker image manifest profile is missing")
    archive = document.get("archive")
    images = document.get("images")
    if not isinstance(archive, dict) or set(archive) != {"bytes", "sha256"}:
        raise WorkerImageError("Worker image archive identity is invalid")
    if not isinstance(images, list) or len(images) != len(EXPECTED_NAMES):
        raise WorkerImageError("Worker image manifest must contain three images")
    archive_bytes = archive_path.read_bytes()
    if archive.get("bytes") != len(archive_bytes) or archive.get("sha256") != _sha256(
        archive_bytes
    ):
        raise WorkerImageError("Worker image archive identity does not match")
    entries = parse_newc(archive_bytes)
    expected_paths = [f"{ARCHIVE_MEMBER_ROOT}/{name}" for name in EXPECTED_NAMES]
    if sorted(entries) != sorted(expected_paths):
        raise WorkerImageError("Worker archive member set is incomplete or unexpected")
    names: list[str] = []
    roles: list[str] = []
    expected_row_fields = {
        "name",
        "role",
        "abi_version",
        "entry_version",
        "entry_symbol",
        "entry_vaddr",
        "flags",
        "archive_path",
        "source_sha256",
        "image_sha256",
        "image_bytes",
        "load_base_vaddr",
        "load_limit_vaddr",
        "load_span_bytes",
        "metadata_vaddr",
        "metadata_sha256",
        "stack_bytes",
        "ipc_buffer_bytes",
        "shared_page_bytes",
    }
    for row in images:
        if not isinstance(row, dict) or set(row) != expected_row_fields:
            raise WorkerImageError("Worker image row fields are invalid")
        name = row.get("name")
        role = row.get("role")
        if not isinstance(name, str) or name not in ROLE_BY_NAME:
            raise WorkerImageError("Worker image name is invalid")
        if role != ROLE_BY_NAME[name][0]:
            raise WorkerImageError("Worker image role is invalid")
        names.append(name)
        roles.append(role)
        member_path = row.get("archive_path")
        if member_path != f"{ARCHIVE_MEMBER_ROOT}/{name}":
            raise WorkerImageError("Worker archive path is invalid")
        member = entries[str(member_path)]
        if row.get("image_bytes") != len(member) or row.get("image_sha256") != _sha256(
            member
        ):
            raise WorkerImageError("Worker image bytes do not match their manifest row")
        parsed = _parse_canonical_elf(member)
        loads = _load_segments(parsed)
        if parsed.entry != row.get("entry_vaddr"):
            raise WorkerImageError("Worker image entrypoint differs from its manifest")
        low = min(header.vaddr for header in loads)
        high = max(header.vaddr + header.memsz for header in loads)
        if (
            row.get("load_base_vaddr") != low
            or row.get("load_limit_vaddr") != high
            or row.get("load_span_bytes") != high - low
        ):
            raise WorkerImageError("Worker image load span differs from its manifest")
        if (
            row.get("abi_version") != WORKER_ABI_VERSION
            or row.get("entry_version") != WORKER_ENTRY_VERSION
            or row.get("entry_symbol") != WORKER_ENTRY_SYMBOL.decode("ascii")
            or row.get("flags") != ["pointer-free", "init-page-in-x0"]
            or row.get("stack_bytes") != STACK_BYTES
            or row.get("ipc_buffer_bytes") != IPC_BUFFER_BYTES
            or row.get("shared_page_bytes") != PAGE_BYTES
        ):
            raise WorkerImageError("Worker image ABI bounds differ from version 2")
        metadata_vaddr = row.get("metadata_vaddr")
        metadata_sha = row.get("metadata_sha256")
        if not isinstance(metadata_vaddr, int) or not isinstance(metadata_sha, str):
            raise WorkerImageError("Worker metadata identity is invalid")
        metadata_bytes = None
        for load in loads:
            if load.vaddr <= metadata_vaddr and metadata_vaddr + 64 <= load.vaddr + load.filesz:
                file_offset = load.offset + metadata_vaddr - load.vaddr
                metadata_bytes = _checked_slice(member, file_offset, 64, "Worker metadata")
                if load.flags & (PF_W | PF_X):
                    raise WorkerImageError("canonical Worker metadata is not read-only")
                break
        if metadata_bytes is None or _sha256(metadata_bytes) != metadata_sha:
            raise WorkerImageError("canonical Worker metadata hash differs")
        magic, abi, length, role_number, entry_version, flags, symbol, reserved = struct.unpack(
            "<IHHHHI32s16s", metadata_bytes
        )
        if (
            magic != WORKER_METADATA_MAGIC
            or abi != WORKER_ABI_VERSION
            or length != 64
            or role_number != ROLE_BY_NAME[name][1]
            or entry_version != WORKER_ENTRY_VERSION
            or flags != WORKER_METADATA_FLAGS
            or symbol != WORKER_ENTRY_SYMBOL + bytes(26)
            or reserved != bytes(16)
        ):
            raise WorkerImageError("canonical Worker metadata record is invalid")
    if tuple(names) != EXPECTED_NAMES or len(set(roles)) != len(roles):
        raise WorkerImageError("Worker role matrix is missing, duplicated, or out of order")
    return document


def _write_atomic(path: Path, data: bytes, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_bytes(data)
    temporary.chmod(mode)
    os.replace(temporary, path)


def build_manifest(
    image_dir: Path,
    output_dir: Path,
    archive_path: Path,
    manifest_path: Path,
    target: str,
    profile: str,
) -> dict[str, object]:
    """Build the exact mandatory Worker archive and manifest."""

    if target != EXPECTED_TARGET:
        raise WorkerImageError(f"unsupported Worker image target: {target}")
    if not profile or any(character.isspace() for character in profile):
        raise WorkerImageError("Worker image profile is invalid")
    rows: list[dict[str, object]] = []
    members: list[tuple[str, bytes]] = []
    output_dir.mkdir(parents=True, exist_ok=True)
    for name in EXPECTED_NAMES:
        source = image_dir / name
        if not source.is_file():
            raise WorkerImageError(f"required Worker image is missing: {source}")
        row, canonical = inspect_image(source, name)
        staged = output_dir / name
        _write_atomic(staged, canonical, 0o555)
        rows.append(row)
        members.append((f"{ARCHIVE_MEMBER_ROOT}/{name}", canonical))
    archive_bytes = build_newc(tuple(sorted(members)))
    _write_atomic(archive_path, archive_bytes)
    document: dict[str, object] = {
        "schema": SCHEMA,
        "target": target,
        "profile": profile,
        "archive": {"bytes": len(archive_bytes), "sha256": _sha256(archive_bytes)},
        "images": rows,
    }
    manifest_bytes = (json.dumps(document, indent=2, sort_keys=True) + "\n").encode("utf-8")
    _write_atomic(manifest_path, manifest_bytes)
    verify_manifest(manifest_path, archive_path)
    return document


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    build = subparsers.add_parser("build", help="build and verify a Worker archive")
    build.add_argument("--image-dir", required=True, type=Path)
    build.add_argument("--output-dir", required=True, type=Path)
    build.add_argument("--archive", required=True, type=Path)
    build.add_argument("--manifest", required=True, type=Path)
    build.add_argument("--target", required=True)
    build.add_argument("--profile", required=True)
    verify = subparsers.add_parser("verify", help="verify an existing Worker archive")
    verify.add_argument("--archive", required=True, type=Path)
    verify.add_argument("--manifest", required=True, type=Path)
    return parser


def main(argv: Iterable[str] | None = None) -> int:
    """Run the command-line validator."""

    arguments = _parser().parse_args(list(argv) if argv is not None else None)
    try:
        if arguments.command == "build":
            document = build_manifest(
                arguments.image_dir,
                arguments.output_dir,
                arguments.archive,
                arguments.manifest,
                arguments.target,
                arguments.profile,
            )
        else:
            document = verify_manifest(arguments.manifest, arguments.archive)
    except (OSError, WorkerImageError) as error:
        print(f"worker-image-manifest: FAIL: {error}", file=sys.stderr)
        return 1
    print(
        "worker-image-manifest: PASS "
        f"images={len(document['images'])} "
        f"archive_sha256={document['archive']['sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
