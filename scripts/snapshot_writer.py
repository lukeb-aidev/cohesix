#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: snapshot_writer.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Periodic world + service snapshot writer."""
import json
import os
import time
from pathlib import Path


def collect_snapshot(worker_id: str) -> dict:
    snap = {"worker_id": worker_id, "timestamp": int(time.time())}
    world = Path("/sim/world.json")
    if world.exists():
        try:
            snap["sim"] = json.loads(world.read_text())
        except Exception:
            snap["sim"] = {}
    meta_dir = Path("/srv/agent_meta")
    if meta_dir.exists():
        snap["agent_meta"] = {}
        for item in meta_dir.iterdir():
            if item.is_file():
                snap["agent_meta"][item.name] = item.read_text()
    return snap


def write_snapshot(base: Path, worker_id: str):
    snap = collect_snapshot(worker_id)
    base.mkdir(parents=True, exist_ok=True)
    path = base / f"{worker_id}.json"
    path.write_text(json.dumps(snap, indent=2))
    return path


def main():
    worker_id = os.environ.get("WORKER_ID", "local")
    out_dir = Path(os.environ.get("SNAPSHOT_BASE", "/history/snapshots"))
    while True:
        write_snapshot(out_dir, worker_id)
        time.sleep(1)


if __name__ == "__main__":
    main()
