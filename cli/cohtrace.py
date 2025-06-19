# CLASSIFICATION: COMMUNITY
# Filename: cohtrace.py v0.8
# Author: Lukas Bower
# Date Modified: 2025-08-01

"""cohtrace – inspect connected workers."""

import os
import argparse
from pathlib import Path
import subprocess
import shlex
import json
import time
from typing import List
from datetime import datetime
import traceback
import sys


LOG_DIR = Path(os.getenv("COHESIX_LOG", Path.home() / ".cohesix" / "log"))
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


def list_workers(base: Path):
    if not base.exists():
        cohlog("No workers connected")
        return
    for worker in base.iterdir():
        role = (
            (worker / "role").read_text().strip()
            if (worker / "role").exists()
            else "Unknown"
        )
        services = []
        srv_dir = worker / "services"
        if srv_dir.exists():
            services = [p.name for p in srv_dir.iterdir()]
        cohlog(f"{worker.name}: role={role} services={','.join(services)}")


def push_trace(worker_id: str, path: Path):
    dest_dir = Path("/trace") / worker_id
    os.makedirs(dest_dir, exist_ok=True)
    dest = dest_dir / "sim.json"
    import shutil

    shutil.copy(path, dest)
    cohlog(f"Trace pushed to {dest}")
    try:
        from cohesix.trace.validator import validate_trace

        validate_trace(str(dest), worker_id)
    except Exception as e:
        cohlog(f"Validation failed: {e}")


def verify_trace(path: Path) -> bool:
    """Validate that *path* contains required boot events."""
    required = {"boot_success", "namespace_mount"}
    try:
        events = json.loads(path.read_text())
    except Exception:
        cohlog("failed to parse trace file")
        return False
    names = {e.get("event") for e in events if isinstance(e, dict)}
    missing = required - names
    if missing:
        cohlog("missing events: " + ",".join(sorted(missing)))
        return False
    cohlog("trace OK")
    return True


def compare_traces(expected: Path, actual: Path) -> bool:
    """Compare two traces, printing differences."""
    try:
        exp = json.loads(expected.read_text())
        act = json.loads(actual.read_text())
    except Exception:
        cohlog("failed to parse traces")
        return False
    exp_names = [str(e["event"]) for e in exp if isinstance(e, dict) and "event" in e]
    act_names = [str(e["event"]) for e in act if isinstance(e, dict) and "event" in e]
    missing = [e for e in exp_names if e not in act_names]
    unexpected = [e for e in act_names if e not in exp_names]
    mismatched_ts = []
    for idx, (ee, ae) in enumerate(zip(exp, act)):
        if not (isinstance(ee, dict) and isinstance(ae, dict)):
            continue
        if ee.get("event") != ae.get("event"):
            continue
        if "ts" in ee and "ts" in ae:
            if abs(float(ee["ts"]) - float(ae["ts"])) > 1.0:
                mismatched_ts.append(str(ee.get("event")))
    if missing:
        cohlog("missing events: " + ",".join(missing))
    if unexpected:
        cohlog("unexpected events: " + ",".join(unexpected))
    if mismatched_ts:
        cohlog("timestamp mismatches: " + ",".join(mismatched_ts))
    return not (missing or unexpected or mismatched_ts)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--man", action="store_true", help="Show man page")
    parser.add_argument("--verify-trace", help="Validate a boot trace file")
    sub = parser.add_subparsers(dest="cmd")
    sub.add_parser("list", help="List connected workers")
    push = sub.add_parser("push_trace", help="Push a simulation trace to the Queen")
    push.add_argument("worker_id")
    push.add_argument("path")
    sub.add_parser("kiosk_ping", help="Simulate kiosk card insertion")
    sub.add_parser("trust_check", help="Show worker trust levels")
    view = sub.add_parser("view_snapshot", help="View world snapshot for worker")
    view.add_argument("worker_id")
    compare = sub.add_parser("compare", help="Compare two trace files")
    compare.add_argument("--expected", required=True)
    compare.add_argument("--actual", required=True)

    args = parser.parse_args()
    if args.man:
        man = os.path.join(os.path.dirname(__file__), "../bin/man")
        page = os.path.join(os.path.dirname(__file__), "../docs/man/cohtrace.1")
        os.execv(man, [man, page])
    if args.verify_trace:
        ok = verify_trace(Path(args.verify_trace))
        sys.exit(0 if ok else 1)
    if args.cmd == "list":
        list_workers(Path("/srv/workers"))
    elif args.cmd == "push_trace":
        push_trace(args.worker_id, Path(args.path))
    elif args.cmd == "kiosk_ping":
        path = Path("/srv/kiosk_federation.json")
        events = []
        if path.exists():
            try:
                events = json.loads(path.read_text())
            except Exception:
                events = []
        events.append({"timestamp": int(time.time()), "event": "ping"})
        path.write_text(json.dumps(events))
        cohlog("kiosk ping logged")
    elif args.cmd == "trust_check":
        base = Path("/srv/trust_zones")
        if not base.exists():
            cohlog("no trust zone data")
        else:
            for ent in base.iterdir():
                level = ent.read_text().strip()
                cohlog(f"{ent.name}: {level}")
    elif args.cmd == "view_snapshot":
        base = Path(os.environ.get("SNAPSHOT_BASE", "/history/snapshots"))
        path = base / f"{args.worker_id}.json"
        if path.exists():
            cohlog(path.read_text())
        else:
            cohlog("snapshot not found")
    elif args.cmd == "compare":
        ok = compare_traces(Path(args.expected), Path(args.actual))
        sys.exit(0 if ok else 1)
    else:
        parser.print_help()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        with (LOG_DIR / "cli_error.log").open("a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {traceback.format_exc()}\n")
        cohlog("Unhandled error, see cli_error.log")
        sys.exit(1)
