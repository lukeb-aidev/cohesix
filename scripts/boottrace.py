# CLASSIFICATION: COMMUNITY
# Filename: boottrace.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-06-17

#!/usr/bin/env python3  # noqa: E265
"""Boot-time event logger."""

import os
import sys
import time

SRV_DIR = os.environ.get("SRV_DIR", "/srv")
LOG_FILE = os.path.join(SRV_DIR, "boottrace.log")


def log_event(evt: str) -> None:
    ts = time.perf_counter()
    os.makedirs(SRV_DIR, exist_ok=True)
    with open(LOG_FILE, "a", encoding="utf-8") as fh:
        fh.write(f"{ts:.6f} {evt}\n")


def main():
    if len(sys.argv) < 2:
        print("usage: boottrace.py <event> [...]", file=sys.stderr)
        sys.exit(1)
    for e in sys.argv[1:]:
        log_event(e)


if __name__ == "__main__":
    main()
