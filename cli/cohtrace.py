#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohtrace.py v0.3
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


def push_trace(worker_id: str, path: Path):
    dest_dir = Path("/trace") / worker_id
    os.makedirs(dest_dir, exist_ok=True)
    dest = dest_dir / "sim.json"
    import shutil
    shutil.copy(path, dest)
    print(f"Trace pushed to {dest}")
    try:
        from cohesix.trace.validator import validate_trace
        validate_trace(str(dest), worker_id)
    except Exception as e:
        print(f"Validation failed: {e}")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd")
    sub.add_parser("list", help="List connected workers")
    push = sub.add_parser("push_trace", help="Push a simulation trace to the Queen")
    push.add_argument("worker_id")
    push.add_argument("path")
    sub.add_parser("kiosk_ping", help="Simulate kiosk card insertion")
    args = parser.parse_args()
    if args.cmd == "list":
        list_workers(Path("/srv/workers"))
    elif args.cmd == "push_trace":
        push_trace(args.worker_id, Path(args.path))
    elif args.cmd == "kiosk_ping":
        path = Path("/srv/kiosk_federation.json")
        data = {"pings": []}
        if path.exists():
            import json
            data = json.loads(path.read_text())
        import time, json
        data.setdefault("pings", []).append(int(time.time()))
        path.write_text(json.dumps(data))
        print("kiosk ping logged")
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
