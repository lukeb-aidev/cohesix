#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohcap.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-13
"""cohcap – manage demo capabilities."""

import argparse
import os
from pathlib import Path
import sys


BASE_ENV = "CAP_BASE"


def cap_dir() -> Path:
    base = Path(os.environ.get(BASE_ENV, "/srv/capabilities"))
    base.mkdir(parents=True, exist_ok=True)
    return base


def list_caps(worker: str):
    path = cap_dir() / f"{worker}.caps"
    if not path.exists():
        print("no capabilities")
        return
    for line in path.read_text().splitlines():
        print(line)


def grant_cap(worker: str, cap: str):
    path = cap_dir() / f"{worker}.caps"
    caps = set()
    if path.exists():
        caps.update(path.read_text().splitlines())
    caps.add(cap)
    path.write_text("\n".join(sorted(caps)))
    print(f"granted {cap} to {worker}")


def revoke_cap(worker: str, cap: str):
    path = cap_dir() / f"{worker}.caps"
    if not path.exists():
        print("no capabilities")
        return
    caps = [c for c in path.read_text().splitlines() if c != cap]
    path.write_text("\n".join(caps))
    print(f"revoked {cap} from {worker}")


def main():
    parser = argparse.ArgumentParser(description="Manage demo capabilities")
    parser.add_argument("--man", action="store_true", help="Show man page")
    sub = parser.add_subparsers(dest="cmd")

    l = sub.add_parser("list", help="List capabilities")
    l.add_argument("--worker", required=True)

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
    main()
