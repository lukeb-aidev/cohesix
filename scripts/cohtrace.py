#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohtrace.py v0.1
# Date Modified: 2025-06-18
# Author: Cohesix Codex

"""Simple syscall trace validator for Cohesix unit tests."""

import os

TRACE_FILE = "trace.log"


def log(msg: str):
    with open(TRACE_FILE, "a") as f:
        f.write(msg + "\n")


def main():
    log("spawn")
    if os.path.exists("/srv/telemetry"):
        log("telemetry")
    log("done")


if __name__ == "__main__":
    main()

