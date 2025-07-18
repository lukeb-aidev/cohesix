# CLASSIFICATION: COMMUNITY
# Filename: check_elf_layout.py v0.1
# Author: Cohesix Codex
# Date Modified: 2028-01-21

import sys
from pathlib import Path
from elftools.elf.elffile import ELFFile

EXPECTED_START = 0x40000000
EXPECTED_END = 0x80000000


def validate(path: Path) -> bool:
    with path.open('rb') as f:
        elf = ELFFile(f)
        for seg in elf.iter_segments():
            if seg['p_type'] != 'PT_LOAD':
                continue
            paddr = seg['p_paddr']
            vaddr = seg['p_vaddr']
            if not (EXPECTED_START <= paddr < EXPECTED_END):
                print(f"Physical address {hex(paddr)} out of range")
                return False
            if (paddr & 0xFFF) != (vaddr & 0xFFF):
                print(f"Segment misalignment {hex(paddr)} -> {hex(vaddr)}")
                return False
    return True


def main() -> None:
    if len(sys.argv) != 2:
        print("Usage: check_elf_layout.py <elf>")
        sys.exit(1)
    path = Path(sys.argv[1])
    if not path.exists():
        print(f"ELF file {path} not found")
        sys.exit(1)
    if not validate(path):
        sys.exit(1)


if __name__ == "__main__":
    main()
