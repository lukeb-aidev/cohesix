#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify elfloader archive rewriting helpers preserve boot-critical ELF layout.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import struct
import unittest

from strip_elfloader_modules import _load_segment_base, _minimize_elf_for_boot


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
        2,  # e_type: executable
        183,  # e_machine: AArch64
        1,  # e_version
        0x400000,  # e_entry
        e_phoff,
        e_shoff,
        0,  # e_flags
        64,  # e_ehsize
        e_phentsize,
        e_phnum,
        64,  # e_shentsize
        2,  # e_shnum
        1,  # e_shstrndx
    )
    data[16 : 16 + len(header)] = header

    ph_null = struct.pack(endian + "IIQQQQQQ", 0, 0, 0, 0, 0, 0, 0, 0)
    ph_load = struct.pack(
        endian + "IIQQQQQQ",
        1,  # PT_LOAD
        5,  # PF_R | PF_X
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


class ElfloaderModuleTests(unittest.TestCase):
    def test_load_segment_base_honors_program_header_entry_size(self) -> None:
        offset, vaddr = _load_segment_base(_synthetic_elf64())

        self.assertEqual(offset, 0x100)
        self.assertEqual(vaddr, 0x400000)

    def test_minimize_elf_for_boot_removes_section_metadata(self) -> None:
        minimized, removed = _minimize_elf_for_boot(_synthetic_elf64())

        self.assertEqual(len(minimized), 0x120)
        self.assertGreater(removed, 0)
        self.assertEqual(struct.unpack_from("<Q", minimized, 40)[0], 0)
        self.assertEqual(struct.unpack_from("<H", minimized, 58)[0], 0)
        self.assertEqual(struct.unpack_from("<H", minimized, 60)[0], 0)
        self.assertEqual(struct.unpack_from("<H", minimized, 62)[0], 0)
        self.assertEqual(minimized[0x100:0x120], b"\xAA" * 0x20)


if __name__ == "__main__":
    unittest.main()
