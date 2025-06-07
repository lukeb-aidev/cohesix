#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: fuzz_regression_tracker.py v0.1
# Date Modified: 2025-07-01
# Author: Lukas Bower

"""Track fuzz traces that cause regressions and rerun them before merge."""

import os
import subprocess
from pathlib import Path

CONFIRMED = Path("/srv/fuzz/confirmed")


def track(trace_path: str):
    CONFIRMED.mkdir(parents=True, exist_ok=True)
    p = Path(trace_path)
    target = CONFIRMED / p.name
    if not target.exists():
        target.write_text(p.read_text())
    subprocess.run(["cargo", "test"], check=False)


if __name__ == "__main__":
    import sys
    if len(sys.argv) > 1:
        track(sys.argv[1])
