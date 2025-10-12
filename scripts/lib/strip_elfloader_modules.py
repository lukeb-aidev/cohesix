#!/usr/bin/env python3
# Author: Lukas Bower
"""Trim appended seL4 modules from an elfloader image.

The upstream seL4 build system links the kernel and default root server onto
`elfloader` as extra payloads. QEMU's `-device loader` parameters replace those
modules at runtime, but the baked-in payloads still take precedence during
boot. This helper strips the appended module data so that Cohesix-supplied
binaries become authoritative without mutating the original seL4 build tree.
"""

from __future__ import annotations

import argparse
import os
import stat
import struct
import sys
from pathlib import Path

ELF_MAGIC = b"\x7fELF"
EI_CLASS_32 = 1
EI_CLASS_64 = 2
EI_DATA_LSB = 1
EI_DATA_MSB = 2


class ElfParseError(RuntimeError):
    """Raised when the input file is not a supported ELF image."""


def _unpack(fmt: str, data: bytes, offset: int) -> tuple[int, ...]:
    try:
        return struct.unpack_from(fmt, data, offset)
    except struct.error as exc:  # pragma: no cover - defensive branch
        raise ElfParseError(f"Unable to parse ELF structure at offset {offset}") from exc


def _compute_loader_extent(data: bytes) -> int:
    if len(data) < len(ELF_MAGIC) or data[:4] != ELF_MAGIC:
        raise ElfParseError("File does not start with an ELF header")

    ei_class = data[4]
    ei_data = data[5]
    if ei_class not in (EI_CLASS_32, EI_CLASS_64):
        raise ElfParseError(f"Unsupported ELF class {ei_class}")
    if ei_data not in (EI_DATA_LSB, EI_DATA_MSB):
        raise ElfParseError(f"Unsupported ELF endianness {ei_data}")

    endian = "<" if ei_data == EI_DATA_LSB else ">"
    if ei_class == EI_CLASS_64:
        header_fmt = endian + "HHIQQQIHHHHHH"
        ph_fmt = endian + "IIQQQQQQ"
    else:
        header_fmt = endian + "HHIIIIIHHHHHH"
        ph_fmt = endian + "IIIIIIII"

    header = _unpack(header_fmt, data, 16)
    e_phoff = header[4]
    e_phentsize = header[7]
    e_phnum = header[8]

    if e_phoff == 0 or e_phnum == 0:
        raise ElfParseError("ELF image does not contain program headers")

    extent = 0
    for index in range(e_phnum):
        offset = e_phoff + index * e_phentsize
        p_header = _unpack(ph_fmt, data, offset)
        if ei_class == EI_CLASS_64:
            p_offset = p_header[2]
            p_filesz = p_header[5]
        else:
            p_offset = p_header[1]
            p_filesz = p_header[4]
        if p_filesz == 0:
            continue
        candidate = p_offset + p_filesz
        if candidate > extent:
            extent = candidate

    if extent == 0:
        raise ElfParseError("Unable to determine loader extent from program headers")
    return extent


def strip_modules(source: Path, destination: Path) -> tuple[int, int]:
    data = source.read_bytes()
    try:
        extent = _compute_loader_extent(data)
    except ElfParseError as exc:
        raise SystemExit(f"error: {exc}") from exc

    trimmed = data[:extent]
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(trimmed)

    src_mode = source.stat().st_mode
    dest_mode = stat.S_IMODE(src_mode) or 0o755
    os.chmod(destination, dest_mode)
    return len(data), len(trimmed)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Input elfloader image")
    parser.add_argument("destination", type=Path, help="Output path for sanitised image")
    args = parser.parse_args(argv)

    if not args.source.is_file():
        parser.error(f"Input file not found: {args.source}")

    original_size, stripped_size = strip_modules(args.source, args.destination)
    trimmed = original_size - stripped_size
    print(
        f"[cohesix-build] Sanitised elfloader copied to {args.destination} "
        f"(trimmed {trimmed} bytes)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
