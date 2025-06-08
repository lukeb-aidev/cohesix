# CLASSIFICATION: COMMUNITY
# Filename: autorun_tests.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-14

#!/usr/bin/env python3
"""Continuous test runner for Cohesix.

Watches the workspace for file modifications and automatically
executes `cargo test`, `go test`, and `pytest`.
"""

import argparse
import subprocess
import time
from pathlib import Path


def run_tests() -> None:
    """Execute the standard test suite without halting on failure."""
    subprocess.run(["cargo", "test", "--workspace"], check=False)
    subprocess.run(["go", "test", "./go/..."], check=False)
    subprocess.run(["pytest", "-q"], check=False)


def snapshot() -> dict[str, float]:
    """Return modification times for all files in the repository."""
    m = {}
    for p in Path(".").rglob("*"):
        if p.is_file():
            try:
                m[str(p)] = p.stat().st_mtime
            except OSError:
                continue
    return m


def watch(interval: float) -> None:
    """Poll for file changes and run tests when modifications occur."""
    prev = snapshot()
    while True:
        time.sleep(interval)
        cur = snapshot()
        if any(cur.get(k) != prev.get(k) for k in cur.keys()):
            run_tests()
            prev = cur


def main() -> None:
    parser = argparse.ArgumentParser(description="Auto-run tests on changes")
    parser.add_argument("--interval", type=float, default=1.0,
                        help="Polling interval in seconds")
    args = parser.parse_args()
    watch(args.interval)


if __name__ == "__main__":
    main()
