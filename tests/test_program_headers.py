# CLASSIFICATION: COMMUNITY
# Filename: test_program_headers.py v0.1
# Author: Cohesix Builder
# Date Modified: 2028-01-21
import re
from pathlib import Path


def parse_program_headers(path: Path):
    content = path.read_text().splitlines()
    loads = []
    load_re = re.compile(r"^\s*LOAD\s+(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+(0x[0-9a-f]+)")
    for line in content:
        m = load_re.search(line)
        if m:
            offset = int(m.group(1), 16)
            vaddr = int(m.group(2), 16)
            paddr = int(m.group(3), 16)
            loads.append((offset, vaddr, paddr))
    return loads


def test_physical_range():
    diag = Path("out/diag_mmu_fault_20250718_212435/cohesix_root_program_headers.txt")
    assert diag.exists(), f"missing {diag}"
    loads = parse_program_headers(diag)
    assert loads, "no LOAD segments found"
    for _off, _vaddr, paddr in loads:
        assert 0x00400000 <= paddr < 0x05000000, f"paddr {hex(paddr)} out of range"
