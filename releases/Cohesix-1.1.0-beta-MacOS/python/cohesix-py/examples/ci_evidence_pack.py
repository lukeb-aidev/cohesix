# Author: Lukas Bower
# Purpose: Validate a Cohesix evidence pack layout and emit a CI-friendly summary JSON.
# Copyright 2026 Lukas Bower

"""Validate an evidence pack directory produced by `coh evidence pack`."""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional


@dataclass(frozen=True)
class Bounds:
    proc_schedule_summary: bool
    proc_schedule_queue: bool
    proc_lease_summary: bool
    proc_lease_active: bool
    proc_lease_preemptions: bool
    proc_schedule_summary_bytes: int
    proc_schedule_queue_bytes: int
    proc_lease_summary_bytes: int
    proc_lease_active_bytes: int
    proc_lease_preemptions_bytes: int


def _read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def _load_bounds(pack_dir: Path) -> Bounds:
    raw = _read_json(pack_dir / "bounds.json")
    obs = raw.get("observability", {})
    sched = obs.get("proc_schedule", {})
    lease = obs.get("proc_lease", {})
    return Bounds(
        proc_schedule_summary=bool(sched.get("summary", False)),
        proc_schedule_queue=bool(sched.get("queue", False)),
        proc_lease_summary=bool(lease.get("summary", False)),
        proc_lease_active=bool(lease.get("active", False)),
        proc_lease_preemptions=bool(lease.get("preemptions", False)),
        proc_schedule_summary_bytes=int(sched.get("summary_bytes", 0)),
        proc_schedule_queue_bytes=int(sched.get("queue_bytes", 0)),
        proc_lease_summary_bytes=int(lease.get("summary_bytes", 0)),
        proc_lease_active_bytes=int(lease.get("active_bytes", 0)),
        proc_lease_preemptions_bytes=int(lease.get("preemptions_bytes", 0)),
    )


def _check_file(
    pack_dir: Path,
    rel_path: str,
    *,
    required: bool,
    max_bytes: Optional[int] = None,
) -> tuple[bool, Optional[str]]:
    path = pack_dir / rel_path
    if not path.is_file():
        if required:
            return False, "missing"
        return True, "na"
    if max_bytes is not None:
        size = path.stat().st_size
        if size > max_bytes:
            return False, f"size {size} exceeds bound {max_bytes}"
    return True, None


def validate_pack(pack_dir: Path) -> dict[str, Any]:
    missing: list[str] = []
    errors: list[str] = []

    def req(path: str) -> None:
        ok, detail = _check_file(pack_dir, path, required=True)
        if not ok:
            missing.append(path)
            if detail:
                errors.append(f"{path}: {detail}")

    def opt(path: str, *, max_bytes: Optional[int] = None) -> None:
        ok, detail = _check_file(pack_dir, path, required=False, max_bytes=max_bytes)
        if not ok:
            errors.append(f"{path}: {detail or 'error'}")

    req("meta.json")
    req("bounds.json")
    req("summary.json")
    req("log/queen.log")

    bounds = _load_bounds(pack_dir) if (pack_dir / "bounds.json").is_file() else None

    if bounds is not None:
        def chk(rel_path: str, *, enabled: bool, max_bytes: int) -> None:
            ok, detail = _check_file(
                pack_dir,
                rel_path,
                required=enabled,
                max_bytes=max_bytes if max_bytes > 0 else None,
            )
            if not ok:
                missing.append(rel_path)
                errors.append(f"{rel_path}: {detail or 'error'}")

        chk(
            "proc/schedule/summary",
            enabled=bounds.proc_schedule_summary,
            max_bytes=bounds.proc_schedule_summary_bytes,
        )
        chk(
            "proc/schedule/queue",
            enabled=bounds.proc_schedule_queue,
            max_bytes=bounds.proc_schedule_queue_bytes,
        )
        chk(
            "proc/lease/summary",
            enabled=bounds.proc_lease_summary,
            max_bytes=bounds.proc_lease_summary_bytes,
        )
        chk(
            "proc/lease/active",
            enabled=bounds.proc_lease_active,
            max_bytes=bounds.proc_lease_active_bytes,
        )
        chk(
            "proc/lease/preemptions",
            enabled=bounds.proc_lease_preemptions,
            max_bytes=bounds.proc_lease_preemptions_bytes,
        )

    # Audit/replay are optional (gated by manifest flags). If export exists, expect logs too.
    audit_export = pack_dir / "audit" / "export"
    if audit_export.is_file():
        req("audit/journal")
        req("audit/decisions")
        audit_journal = (pack_dir / "audit" / "journal").read_text(encoding="utf-8")
        if "cohesix-ticket-" in audit_journal:
            errors.append("audit/journal: leaked ticket token")
    opt("replay/status", max_bytes=1024)

    ok = not missing and not errors
    return {
        "schema": "cohesix-ci-evidence-pack/v1",
        "pack_dir": str(pack_dir),
        "ok": ok,
        "missing": missing,
        "errors": errors,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack", type=Path, required=True, help="evidence pack directory")
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="write summary JSON to a file (default: stdout)",
    )
    args = parser.parse_args()

    summary = validate_pack(args.pack)
    payload = json.dumps(summary, indent=2, sort_keys=True)
    if args.out is None:
        print(payload)
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(payload + "\n", encoding="utf-8")
    sys.exit(0 if summary.get("ok") else 2)


if __name__ == "__main__":
    main()
