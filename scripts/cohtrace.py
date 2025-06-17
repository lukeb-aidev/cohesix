# CLASSIFICATION: COMMUNITY
# Filename: cohtrace.py v0.2
# Date Modified: 2025-09-09
# Author: Lukas Bower

"""Simple syscall trace capture and replay."""

import argparse
import json
from pathlib import Path
from jsonschema import validate, ValidationError

TRACE_EVENT_SCHEMA = {
    "type": "object",
    "required": ["event"],
    "properties": {"ts": {"type": "number"}, "event": {"type": "string"}, "detail": {}},
}

TRACE_SCHEMA = {"type": "array", "items": TRACE_EVENT_SCHEMA}


def log_event(trace, event, detail):
    trace.append({"event": event, "detail": detail})


def write_trace(path: Path, trace):
    with path.open("w") as f:
        json.dump(trace, f)


def read_trace(path: Path):
    try:
        data = json.loads(path.read_text())
        validate(data, TRACE_SCHEMA)
        return data
    except (OSError, json.JSONDecodeError, ValidationError) as exc:
        raise RuntimeError(f"invalid trace {path}: {exc}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["record", "replay"])
    parser.add_argument("file")
    args = parser.parse_args()

    path = Path(args.file)
    if args.mode == "record":
        trace = []
        log_event(trace, "spawn", "shell")
        write_trace(path, trace)
    else:
        trace = read_trace(path)
        for ev in trace:
            print(f"replay {ev['event']} {ev['detail']}")


if __name__ == "__main__":
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
