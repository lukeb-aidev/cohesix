# Author: Lukas Bower
# Purpose: Export evidence pack artifacts as normalized NDJSON for SIEM ingestion.
# Copyright 2026 Lukas Bower

"""Export a Cohesix evidence pack as normalized NDJSON (offline by default)."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterable


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _iter_jsonl(path: Path) -> Iterable[dict[str, Any]]:
    for raw in _read_text(path).splitlines():
        line = raw.strip()
        if not line:
            continue
        yield json.loads(line)


def _parse_kv_line(line: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for part in line.split():
        if "=" not in part:
            continue
        key, value = part.split("=", 1)
        out[key] = value
    return out


def export_ndjson(pack_dir: Path) -> list[dict[str, Any]]:
    events: list[dict[str, Any]] = []

    journal = pack_dir / "audit" / "journal"
    if journal.is_file():
        for entry in _iter_jsonl(journal):
            events.append(
                {
                    "schema": "cohesix-siem-event/v1",
                    "source": "audit/journal",
                    "seq": entry.get("seq"),
                    "kind": entry.get("kind"),
                    "path": entry.get("path"),
                    "outcome": entry.get("outcome"),
                    "error": entry.get("error"),
                    "role": entry.get("role"),
                    "ticket": entry.get("ticket"),
                    "payload": entry.get("payload"),
                }
            )

    decisions = pack_dir / "audit" / "decisions"
    if decisions.is_file():
        for entry in _iter_jsonl(decisions):
            events.append(
                {
                    "schema": "cohesix-siem-event/v1",
                    "source": "audit/decisions",
                    "seq": entry.get("seq"),
                    "kind": entry.get("kind"),
                    "path": entry.get("path"),
                    "outcome": entry.get("outcome"),
                    "role": entry.get("role"),
                    "ticket": entry.get("ticket"),
                    "id": entry.get("id"),
                    "target": entry.get("target"),
                }
            )

    lease_active = pack_dir / "proc" / "lease" / "active"
    if lease_active.is_file():
        for raw in _read_text(lease_active).splitlines():
            line = raw.strip()
            if not line:
                continue
            fields = _parse_kv_line(line)
            events.append(
                {
                    "schema": "cohesix-siem-event/v1",
                    "source": "proc/lease/active",
                    "lease_seq": int(fields.get("seq", "0") or "0"),
                    "id": fields.get("id"),
                    "subject": fields.get("subject"),
                    "resource": fields.get("resource"),
                    "state": fields.get("state"),
                    "ttl_s": int(fields.get("ttl_s", "0") or "0"),
                    "priority": int(fields.get("priority", "0") or "0"),
                }
            )

    def sort_key(e: dict[str, Any]) -> tuple[int, int, str]:
        seq = e.get("seq")
        lease_seq = e.get("lease_seq")
        seq_key = int(seq) if isinstance(seq, int) else 2**63 - 1
        lease_key = int(lease_seq) if isinstance(lease_seq, int) else 2**63 - 1
        kind = str(e.get("kind") or e.get("source") or "")
        return (seq_key, lease_key, kind)

    events.sort(key=sort_key)
    return events


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pack", type=Path, required=True, help="evidence pack directory")
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="write NDJSON to a file (default: stdout)",
    )
    args = parser.parse_args()

    events = export_ndjson(args.pack)
    lines = [json.dumps(event, sort_keys=True) for event in events]
    payload = "\n".join(lines) + ("\n" if lines else "")

    if args.out is None:
        sys.stdout.write(payload)
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(payload, encoding="utf-8")


if __name__ == "__main__":
    main()

