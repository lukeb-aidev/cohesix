#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohtrace.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-11

"""cohtrace – inspect connected workers."""

import os
import argparse
from pathlib import Path


def list_workers(base: Path):
    if not base.exists():
        print("No workers connected")
        return
    for worker in base.iterdir():
        role = (worker / "role").read_text().strip() if (worker / "role").exists() else "Unknown"
        services = []
        srv_dir = worker / "services"
        if srv_dir.exists():
            services = [p.name for p in srv_dir.iterdir()]
        print(f"{worker.name}: role={role} services={','.join(services)}")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd")
    sub.add_parser("list", help="List connected workers")
    args = parser.parse_args()
    if args.cmd == "list":
        list_workers(Path("/srv/workers"))
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
