#!/usr/bin/env python3
# Author: Lukas Bower
"""Prepare a staging copy of seL4's elfloader with Cohesix payloads.

The upstream elfloader binary embeds the kernel image, DTB, and default C
rootserver inside an archiveSection. Cohesix supplies its own root task at
build time, so this helper rewrites the archived rootserver payload while
keeping the kernel artefacts untouched. The rewritten image is emitted to the
destination path without mutating the seL4 build tree.
"""

from __future__ import annotations

import argparse
import os
import stat
import struct
import subprocess
import sys
from pathlib import Path

ELF_MAGIC = b"\x7fELF"
CPIO_MAGIC = b"070701"

ARCHIVE_SYMBOL_START = "_archive_start"
ARCHIVE_SYMBOL_END = "_archive_end"
ROOTSERVER_ENTRY = "rootserver"


class ElfloaderError(RuntimeError):
    """Raised when the elfloader image cannot be processed."""


def _load_segment_base(data: bytes) -> tuple[int, int]:
    """Return (file_offset, vaddr) for the primary PT_LOAD segment."""

    if len(data) < len(ELF_MAGIC) or data[:4] != ELF_MAGIC:
        raise ElfloaderError("Input is not a valid ELF image")

    ei_class = data[4]
    ei_data = data[5]
    if ei_class != 2:  # 64-bit
        raise ElfloaderError("Only 64-bit elfloader images are supported")
    if ei_data not in (1, 2):
        raise ElfloaderError(f"Unsupported ELF endianness {ei_data}")

    endian = "<" if ei_data == 1 else ">"
    header_fmt = endian + "HHIQQQIHHHHHH"
    ph_fmt = endian + "IIQQQQQQ"

    header = struct.unpack_from(header_fmt, data, 16)
    e_phoff = header[4]
    e_phentsize = header[7]
    e_phnum = header[8]
    if e_phoff == 0 or e_phnum == 0:
        raise ElfloaderError("Elfloader image has no program headers")

    for index in range(e_phnum):
        offset = e_phoff + index * e_phentsize
        p_type, _, p_offset, p_vaddr, _, p_filesz, _, _ = struct.unpack_from(ph_fmt, data, offset)
        if p_type == 1 and p_filesz > 0:  # PT_LOAD
            return p_offset, p_vaddr

    raise ElfloaderError("Unable to locate PT_LOAD segment in elfloader")


def _lookup_archive_symbols(source: Path) -> tuple[int, int]:
    """Resolve _archive_start/_archive_end virtual addresses via nm."""

    try:
        output = subprocess.check_output(["nm", "-g", str(source)], text=True)
    except FileNotFoundError as exc:  # pragma: no cover - environment guard
        raise ElfloaderError("Required tool 'nm' not found in PATH") from exc
    except subprocess.CalledProcessError as exc:
        raise ElfloaderError(f"Failed to invoke nm on {source}") from exc

    archive_start = None
    archive_end = None
    for line in output.splitlines():
        parts = line.strip().split()
        if len(parts) != 3:
            continue
        addr_str, _, symbol = parts
        if symbol == ARCHIVE_SYMBOL_START:
            archive_start = int(addr_str, 16)
        elif symbol == ARCHIVE_SYMBOL_END:
            archive_end = int(addr_str, 16)

    if archive_start is None or archive_end is None:
        raise ElfloaderError("Archive boundary symbols not found in elfloader")
    if archive_end <= archive_start:
        raise ElfloaderError("Archive symbol ordering is invalid")
    return archive_start, archive_end


def _parse_cpio(blob: bytes) -> list[dict[str, object]]:
    """Decode a newc-format CPIO archive into entry dictionaries."""

    entries: list[dict[str, object]] = []
    pos = 0
    length = len(blob)
    while pos + len(CPIO_MAGIC) <= length and blob[pos : pos + len(CPIO_MAGIC)] == CPIO_MAGIC:
        fields = [int(blob[pos + i : pos + i + 8], 16) for i in range(6, 6 + 13 * 8, 8)]
        filesize = fields[6]
        namesize = fields[11]
        name_start = pos + 110
        name_bytes = blob[name_start : name_start + namesize]
        if len(name_bytes) != namesize:
            raise ElfloaderError("CPIO entry truncated while reading name")
        name = name_bytes.rstrip(b"\x00").decode()
        data_start = (name_start + namesize + 3) & ~3
        data_end = data_start + filesize
        if data_end > length:
            raise ElfloaderError(f"CPIO entry '{name}' exceeds archive bounds")
        data = blob[data_start:data_end]
        entries.append(
            {
                "name": name,
                "fields": fields,
                "data": bytearray(data),
            }
        )
        pos = (data_end + 3) & ~3
        if name == "TRAILER!!!":
            break

    if not entries or entries[-1]["name"] != "TRAILER!!!":
        raise ElfloaderError("Archive does not terminate with TRAILER!!!")
    return entries


def _build_cpio(entries: list[dict[str, object]]) -> bytes:
    """Serialise CPIO entries back into newc format."""

    out = bytearray()
    for entry in entries:
        name = entry["name"]
        fields = list(entry["fields"])  # copy
        data = bytes(entry["data"])
        name_bytes = name.encode() + b"\x00"
        fields[6] = len(data)
        fields[11] = len(name_bytes)

        header = CPIO_MAGIC + b"".join(f"{value:08X}".encode() for value in fields)
        out.extend(header)
        out.extend(name_bytes)
        while len(out) % 4:
            out.append(0)
        out.extend(data)
        while len(out) % 4:
            out.append(0)
    return bytes(out)


def _rewrite_rootserver(entries: list[dict[str, object]], payload: bytes) -> tuple[int, int]:
    """Replace the rootserver entry data with the supplied payload."""

    for entry in entries:
        if entry["name"] == ROOTSERVER_ENTRY:
            original_size = len(entry["data"])
            entry["data"] = bytearray(payload)
            new_size = len(payload)
            return original_size, new_size
    raise ElfloaderError("rootserver entry not found in elfloader archive")


def prepare_elfloader(source: Path, destination: Path, rootserver: Path) -> tuple[int, int, int]:
    """Copy elfloader, replace the rootserver payload, and persist result.

    Returns a tuple of (original_archive_bytes, new_archive_bytes, rootserver_delta).
    """

    if not source.is_file():
        raise ElfloaderError(f"Input elfloader not found: {source}")
    if not rootserver.is_file():
        raise ElfloaderError(f"Rootserver payload not found: {rootserver}")

    data = bytearray(source.read_bytes())
    load_offset, load_vaddr = _load_segment_base(data)
    archive_start_v, archive_end_v = _lookup_archive_symbols(source)

    archive_start = archive_start_v - load_vaddr + load_offset
    archive_end = archive_end_v - load_vaddr + load_offset
    if archive_start < 0 or archive_end > len(data):
        raise ElfloaderError("Computed archive bounds fall outside elfloader image")

    archive = bytes(data[archive_start:archive_end])
    entries = _parse_cpio(archive)

    payload = rootserver.read_bytes()
    old_size, new_size = _rewrite_rootserver(entries, payload)
    rebuilt = _build_cpio(entries)

    original_len = len(archive)
    rebuilt_len = len(rebuilt)
    if rebuilt_len > original_len:
        raise ElfloaderError(
            f"Rebuilt archive ({rebuilt_len} bytes) exceeds original size ({original_len} bytes)"
        )

    padding = original_len - rebuilt_len
    data[archive_start:archive_start + rebuilt_len] = rebuilt
    if padding:
        data[archive_start + rebuilt_len : archive_end] = b"\x00" * padding

    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(data)
    src_mode = source.stat().st_mode
    os.chmod(destination, stat.S_IMODE(src_mode) or 0o755)
    return original_len, rebuilt_len, new_size - old_size


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Input elfloader image")
    parser.add_argument("destination", type=Path, help="Output path for rewritten image")
    parser.add_argument(
        "--rootserver",
        type=Path,
        required=True,
        help="Path to Cohesix root-task ELF used to replace the embedded rootserver",
    )
    args = parser.parse_args(argv)

    try:
        original_len, rebuilt_len, delta = prepare_elfloader(args.source, args.destination, args.rootserver)
    except ElfloaderError as exc:
        parser.error(str(exc))
        return 1  # pragma: no cover - argparse error path

    print(
        "[cohesix-build] Sanitised elfloader copied to "
        f"{args.destination} (archive {original_len} -> {rebuilt_len} bytes, "
        f"rootserver delta {delta:+d} bytes)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
