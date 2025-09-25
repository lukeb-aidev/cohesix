# CLASSIFICATION: COMMUNITY
# Filename: autorun_tests.py v0.2
# Author: Lukas Bower
# Date Modified: 2029-10-27

#!/usr/bin/env python3  # noqa: E265
"""Continuous test runner for Cohesix."""

from __future__ import annotations

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
    m: dict[str, float] = {}
    for path in Path(".").rglob("*"):
        if path.is_file():
            try:
                m[str(path)] = path.stat().st_mtime
            except OSError:
                continue
    return m


def watch(interval: float) -> None:
    """Poll for file changes and run tests when modifications occur."""
    previous = snapshot()
    try:
        while True:
            time.sleep(interval)
            current = snapshot()
            if current != previous:
                run_tests()
                previous = current
    except KeyboardInterrupt:
        pass


def main() -> None:
    parser = argparse.ArgumentParser(description="Auto-run tests on changes")
    parser.add_argument(
        "--interval", type=float, default=1.0, help="Polling interval in seconds"
    )
    args = parser.parse_args()
    watch(args.interval)


if __name__ == "__main__":
    main()
