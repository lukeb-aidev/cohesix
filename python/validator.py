# CLASSIFICATION: COMMUNITY
# Filename: validator.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Python-side validation helpers."""

import json
from pathlib import Path


def trace_integrity(path: Path) -> bool:
    """Return True if trace file contains valid events."""
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return False
    for ln in lines:
        try:
            ev = json.loads(ln)
        except json.JSONDecodeError:
            return False
        if "ts" not in ev or "event" not in ev:
            return False
    return True

__all__ = ["trace_integrity"]
