# CLASSIFICATION: COMMUNITY
# Filename: cohbuild.py v0.2
# Author: Lukas Bower
# Date Modified: 2026-06-20
"""cohbuild – orchestrate Cohesix build stages."""

import argparse
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path
from typing import List

LOG_DIR = Path(os.getenv("COHESIX_LOG", Path.home() / ".cohesix" / "log"))
LOG_DIR.mkdir(parents=True, exist_ok=True)


def cohlog(msg: str) -> None:
    with (LOG_DIR / "cli_tool.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {msg}\n")
    print(msg)


def repo_root() -> Path:
    try:
        root = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
        return Path(root)
    except Exception:
        return Path.cwd()


def run(cmd: List[str]) -> int:
    with (LOG_DIR / "cli_exec.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {' '.join(cmd)}\n")
    proc = subprocess.run(cmd)
    return proc.returncode


def build_all(root: Path) -> int:
    scripts = [
        root / "scripts" / "build_root_elf.sh",
        root / "scripts" / "make_iso.sh",
    ]
    for s in scripts:
        if not s.exists():
            cohlog(f"missing build script: {s}")
            return 1
        rc = run(["bash", str(s)])
        if rc != 0:
            cohlog(f"step failed: {s}")
            return rc
    cohlog("build sequence complete")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description="Cohesix build orchestrator")
    p.add_argument("command", choices=["all"], nargs="?", default="all")
    args = p.parse_args()
    root = repo_root()
    os.chdir(root)
    if args.command == "all":
        return build_all(root)
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        with (LOG_DIR / "cli_error.log").open("a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {e}\n")
        cohlog("Unhandled error, see cli_error.log")
        sys.exit(1)
