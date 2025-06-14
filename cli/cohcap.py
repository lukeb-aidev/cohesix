# CLASSIFICATION: COMMUNITY
# Filename: cohcap.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-15
"""cohcap – manage demo capabilities."""

import argparse
import os
from pathlib import Path
import sys
import shlex
import subprocess
from datetime import datetime
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


BASE_ENV = "CAP_BASE"


def cap_dir() -> Path:
    base = Path(os.environ.get(BASE_ENV, "/srv/capabilities"))
    base.mkdir(parents=True, exist_ok=True)
    return base


def list_caps(worker: str):
    path = cap_dir() / f"{worker}.caps"
    if not path.exists():
        cohlog("no capabilities")
        return
    for line in path.read_text().splitlines():
        cohlog(line)


def grant_cap(worker: str, cap: str):
    path = cap_dir() / f"{worker}.caps"
    caps = set()
    if path.exists():
        caps.update(path.read_text().splitlines())
    caps.add(cap)
    path.write_text("\n".join(sorted(caps)))
    cohlog(f"granted {cap} to {worker}")


def revoke_cap(worker: str, cap: str):
    path = cap_dir() / f"{worker}.caps"
    if not path.exists():
        cohlog("no capabilities")
        return
    caps = [c for c in path.read_text().splitlines() if c != cap]
    path.write_text("\n".join(caps))
    cohlog(f"revoked {cap} from {worker}")


def main():
    parser = argparse.ArgumentParser(description="Manage demo capabilities")
    parser.add_argument("--man", action="store_true", help="Show man page")
    sub = parser.add_subparsers(dest="cmd")

    list_parser = sub.add_parser("list", help="List capabilities")
    list_parser.add_argument("--worker", required=True)

    g = sub.add_parser("grant", help="Grant capability")
    g.add_argument("cap")
    g.add_argument("--to", dest="worker", required=True)

    r = sub.add_parser("revoke", help="Revoke capability")
    r.add_argument("cap")
    r.add_argument("--from", dest="worker", required=True)

    args = parser.parse_args()

    if args.man:
        man = os.path.join(os.path.dirname(__file__), "../bin/man")
        page = os.path.join(os.path.dirname(__file__), "../docs/man/cohcap.1")
        os.execv(man, [man, page])

    if args.cmd == "list":
        list_caps(args.worker)
    elif args.cmd == "grant":
        grant_cap(args.worker, args.cap)
    elif args.cmd == "revoke":
        revoke_cap(args.worker, args.cap)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        with (LOG_DIR / "cli_error.log").open("a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {traceback.format_exc()}\n")
        cohlog("Unhandled error, see cli_error.log")
        sys.exit(1)
