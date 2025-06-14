# CLASSIFICATION: COMMUNITY
# Filename: cohrun.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-15
"""cohrun – wrapper for Cohesix demo launcher."""

import argparse
import os
import shutil
import subprocess
import sys
import shlex
from datetime import datetime
from pathlib import Path
from typing import List
import traceback


LOG_DIR = Path("/log")
LOG_DIR.mkdir(parents=True, exist_ok=True)


def cohlog(msg: str) -> None:
    with (LOG_DIR / "cli_tool.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {msg}\n")
    print(msg)


def safe_run(cmd: List[str]) -> int:
    quoted = [shlex.quote(c) for c in cmd]
    with (LOG_DIR / "cli_exec.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {' '.join(quoted)}\n")
    result = subprocess.run(cmd)
    return result.returncode


def main():
    parser = argparse.ArgumentParser(description="Run Cohesix demo scenarios")
    parser.add_argument("--man", action="store_true", help="Show man page")
    parser.add_argument(
        "args", nargs=argparse.REMAINDER, help="Arguments passed to rust cohrun"
    )
    opts = parser.parse_args()

    if opts.man:
        man = os.path.join(os.path.dirname(__file__), "../bin/man")
        page = os.path.join(os.path.dirname(__file__), "../docs/man/cohrun.1")
        os.execv(man, [man, page])

    if not opts.args:
        parser.print_help()
        sys.exit(1)

    bin_path = os.environ.get("COHRUN_BIN")
    if not bin_path:
        bin_path = shutil.which("cohrun") or os.path.join("target", "debug", "cohrun")
    try:
        rc = safe_run([bin_path] + opts.args)
        if rc != 0:
            cohlog(f"cohrun exited with code {rc}")
            sys.exit(rc)
    except FileNotFoundError:
        cohlog("cohrun binary not found")
        sys.exit(1)
    except Exception as e:
        cohlog(f"cohrun failed: {e}")
        sys.exit(1)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        with (LOG_DIR / "cli_error.log").open("a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {traceback.format_exc()}\n")
        cohlog("Unhandled error, see cli_error.log")
        sys.exit(1)
