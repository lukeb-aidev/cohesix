# CLASSIFICATION: COMMUNITY
# Filename: test_boot_cpio_order.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-10-06

from pathlib import Path
from typing import List

import pytest


def _parse_newc_entries(archive: Path, limit: int) -> List[str]:
    entries: List[str] = []
    with archive.open("rb") as fh:
        while len(entries) < limit:
            header = fh.read(110)
            if len(header) < 110:
                break
            if not header.startswith(b"070701"):
                raise ValueError("invalid newc header magic")
            namesize = int(header[94:102], 16)
            filesize = int(header[102:110], 16)
            name_bytes = fh.read(namesize)
            if len(name_bytes) != namesize:
                raise ValueError("truncated newc entry name")
            name = name_bytes.rstrip(b"\x00").decode()
            entries.append(name)
            pad = (4 - (namesize % 4)) % 4
            if pad:
                fh.read(pad)
            fh.seek(filesize, 1)
            pad = (4 - (filesize % 4)) % 4
            if pad:
                fh.read(pad)
            if name == "TRAILER!!!":
                break
    return entries


def test_cpio_rootserver_order():
    archive = Path("boot/cohesix.cpio")
    if not archive.exists():
        pytest.skip(f"{archive} not built")

    entries = _parse_newc_entries(archive, limit=3)
    assert entries[:3] == ["kernel.elf", "kernel.dtb", "cohesix_root.elf"]
