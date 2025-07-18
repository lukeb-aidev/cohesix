# CLASSIFICATION: COMMUNITY
# Filename: test_elf_layout.py v0.1
# Author: Cohesix Codex
# Date Modified: 2028-01-21

from pathlib import Path
import sys
import pathlib
sys.path.append(str(pathlib.Path(__file__).resolve().parents[1]))
from tools.check_elf_layout import validate


def test_elf_layout():
    elf = Path("target/sel4-aarch64/release/cohesix_root")
    if not elf.exists():
        import pytest
        pytest.skip(f"{elf} not built")
    assert validate(elf)
