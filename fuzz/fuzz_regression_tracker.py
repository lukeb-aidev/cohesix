#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: fuzz_regression_tracker.py v0.2
# Date Modified: 2025-07-03
# Author: Lukas Bower

"""Track fuzz traces that cause regressions and rerun them before merge."""

import subprocess
from pathlib import Path

CONFIRMED = Path("/srv/fuzz/confirmed")


def track(trace_path: str):
    CONFIRMED.mkdir(parents=True, exist_ok=True)
    p = Path(trace_path)
    res = subprocess.run(["cargo", "test", "--", p.name], capture_output=True)
    if res.returncode != 0 or b"panic" in res.stdout or b"panic" in res.stderr:
        target = CONFIRMED / p.name
        if not target.exists():
            tmp = target.with_suffix(".tmp")
            tmp.write_text(p.read_text())
            if tmp.stat().st_size == 0:
                tmp.unlink()
                raise RuntimeError("regression trace empty")
            tmp.replace(target)


def rerun_confirmed() -> bool:
    if not CONFIRMED.exists():
        return True
    success = True
    for trace in CONFIRMED.iterdir():
        res = subprocess.run(["cargo", "test", "--", trace.name], check=False)
        success &= res.returncode == 0
    return success


if __name__ == "__main__":
    import sys

    if len(sys.argv) > 1:
        track(sys.argv[1])
