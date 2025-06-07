#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohtrace.py v0.1
# Date Modified: 2025-06-18
# Author: Lukas Bower

"""Simple syscall trace capture and replay."""

import argparse
import json
from pathlib import Path


def log_event(trace, event, detail):
    trace.append({'event': event, 'detail': detail})


def write_trace(path: Path, trace):
    with path.open('w') as f:
        json.dump(trace, f)


def read_trace(path: Path):
    with path.open() as f:
        return json.load(f)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=['record', 'replay'])
    parser.add_argument('file')
    args = parser.parse_args()

    path = Path(args.file)
    if args.mode == 'record':
        trace = []
        log_event(trace, 'spawn', 'shell')
        write_trace(path, trace)
    else:
        trace = read_trace(path)
        for ev in trace:
            print(f"replay {ev['event']} {ev['detail']}")

if __name__ == '__main__':
    main()

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

