#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohrun.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-13
"""cohrun – wrapper for Cohesix demo launcher."""

import argparse
import os
import shutil
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description="Run Cohesix demo scenarios")
    parser.add_argument("--man", action="store_true", help="Show man page")
    parser.add_argument("args", nargs=argparse.REMAINDER, help="Arguments passed to rust cohrun")
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
        subprocess.run([bin_path] + opts.args, check=True)
    except FileNotFoundError:
        print("cohrun binary not found", file=sys.stderr)
        sys.exit(1)
    except subprocess.CalledProcessError as e:
        print(f"cohrun failed: {e}", file=sys.stderr)
        sys.exit(e.returncode)


if __name__ == "__main__":
    main()
