#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify elfloader archive rewriting helpers preserve boot-critical ELF layout.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import struct
import sys
from pathlib import Path


LIB_DIR = Path(__file__).resolve().parents[1] / "scripts" / "lib"
sys.path.insert(0, str(LIB_DIR))

from strip_elfloader_modules import (  # noqa: E402
    _load_segment_base,
    _minimize_elf_for_boot,
)


def _synthetic_elf64() -> bytes:
    """Build a tiny ELF64 image with one non-leading PT_LOAD segment."""

    endian = "<"
    data = bytearray(0x280)
    data[:16] = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9)

    e_phoff = 64
    e_shoff = 0x180
    e_phentsize = 56
    e_phnum = 2
    header = struct.pack(
        endian + "HHIQQQIHHHHHH",
        2,
        183,
        1,
        0x400000,
        e_phoff,
        e_shoff,
        0,
        64,
        e_phentsize,
        e_phnum,
        64,
        2,
        1,
    )
    data[16 : 16 + len(header)] = header

    ph_null = struct.pack(endian + "IIQQQQQQ", 0, 0, 0, 0, 0, 0, 0, 0)
    ph_load = struct.pack(
        endian + "IIQQQQQQ",
        1,
        5,
        0x100,
        0x400000,
        0x400000,
        0x20,
        0x20,
        0x1000,
    )
    data[e_phoff : e_phoff + e_phentsize] = ph_null
    data[e_phoff + e_phentsize : e_phoff + e_phentsize * 2] = ph_load
    data[0x100:0x120] = b"\xAA" * 0x20
    data[e_shoff : e_shoff + 0x80] = b"\xBB" * 0x80
    return bytes(data)


def test_load_segment_base_honors_program_header_entry_size() -> None:
    offset, vaddr = _load_segment_base(_synthetic_elf64())

    assert offset == 0x100
    assert vaddr == 0x400000


def test_minimize_elf_for_boot_removes_section_metadata() -> None:
    minimized, removed = _minimize_elf_for_boot(_synthetic_elf64())

    assert len(minimized) == 0x120
    assert removed > 0
    assert struct.unpack_from("<Q", minimized, 40)[0] == 0
    assert struct.unpack_from("<H", minimized, 58)[0] == 0
    assert struct.unpack_from("<H", minimized, 60)[0] == 0
    assert struct.unpack_from("<H", minimized, 62)[0] == 0
    assert minimized[0x100:0x120] == b"\xAA" * 0x20
