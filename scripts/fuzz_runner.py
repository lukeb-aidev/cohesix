#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: fuzz_runner.py v0.1
# Date Modified: 2025-06-25
# Author: Lukas Bower
"""Run trace fuzzing iterations and report failures."""

import argparse
import json
import subprocess
from pathlib import Path


def run_trace(trace: Path) -> str:
    result = subprocess.run(["python3", "scripts/cohtrace.py", "replay", str(trace)], capture_output=True)
    return result.stdout.decode("utf-8") + result.stderr.decode("utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--role", required=True)
    parser.add_argument("--iterations", type=int, default=1)
    args = parser.parse_args()

    input_path = Path(args.input)
    log_dir = Path("/srv/fuzz_log")
    log_dir.mkdir(parents=True, exist_ok=True)

    for i in range(args.iterations):
        out_file = input_path.with_suffix(f".{i}.fuzz.trc")
        subprocess.run([
            "cargo",
            "run",
            "-p",
            "cohfuzz",
            "--",
            "--input",
            str(input_path),
            "--role",
            args.role,
            "--iterations",
            "1",
        ], check=True)
        log = run_trace(out_file)
        if any(word in log for word in ["panic", "unauthorized", "trap"]):
            with open(log_dir / f"fail_{i}.log", "w") as f:
                f.write(log)


if __name__ == "__main__":
    main()
