# Author: Lukas Bower
# Purpose: Validate CI/SIEM integration kit examples for Cohesix evidence packs.
# Copyright 2026 Lukas Bower

"""Tests for evidence-pack integration kit examples."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


def _write_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _make_pack(root: Path) -> Path:
    pack = root / "pack"
    (pack / "audit").mkdir(parents=True, exist_ok=True)
    (pack / "proc" / "schedule").mkdir(parents=True, exist_ok=True)
    (pack / "proc" / "lease").mkdir(parents=True, exist_ok=True)
    (pack / "log").mkdir(parents=True, exist_ok=True)

    _write_json(
        pack / "meta.json",
        {
            "schema": "cohesix-evidence-pack/meta-v1",
            "manifest_sha256": "deadbeef",
            "policy_sha256": "bead",
            "redaction_ticket": "sha256",
            "with_telemetry": False,
        },
    )
    _write_json(
        pack / "bounds.json",
        {
            "manifest_sha256": "deadbeef",
            "secure9p": {"msize": 8192, "walk_depth": 8},
            "console": {
                "max_line_len": 256,
                "max_path_len": 96,
                "max_json_len": 192,
                "max_id_len": 32,
                "max_echo_len": 128,
                "max_ticket_len": 224,
            },
            "paths": {
                "queen_ctl": "/queen/ctl",
                "queen_lifecycle_ctl": "/queen/lifecycle/ctl",
                "queen_schedule_ctl": "/queen/schedule/ctl",
                "queen_lease_ctl": "/queen/lease/ctl",
                "queen_export_ctl": "/queen/export/ctl",
                "policy_ctl": "/policy/ctl",
                "log": "/log/queen.log",
            },
            "control_plane": {
                "schedule": {"enable": True, "queue_max_entries": 64, "ctl_max_bytes": 8192},
                "lease": {
                    "enable": True,
                    "active_max_entries": 64,
                    "preemptions_max_entries": 64,
                    "ctl_max_bytes": 8192,
                },
                "export": {"enable": True, "ctl_max_bytes": 2048},
            },
            "policy": {"enable": True, "queue_max_entries": 32, "queue_max_bytes": 4096, "ctl_max_bytes": 2048},
            "observability": {
                "proc_schedule": {"summary": True, "queue": True, "summary_bytes": 128, "queue_bytes": 256},
                "proc_lease": {
                    "summary": True,
                    "active": True,
                    "preemptions": True,
                    "summary_bytes": 160,
                    "active_bytes": 256,
                    "preemptions_bytes": 256,
                },
            },
        },
    )
    _write_json(
        pack / "summary.json",
        {"schema": "cohesix-evidence-pack/summary-v1", "captured": 1, "missing": 0, "errors": 0, "items": []},
    )

    (pack / "log" / "queen.log").write_text("boot ok\n", encoding="utf-8")
    (pack / "proc" / "schedule" / "summary").write_text("queue=0\n", encoding="utf-8")
    (pack / "proc" / "schedule" / "queue").write_text("", encoding="utf-8")
    (pack / "proc" / "lease" / "summary").write_text("active=0\n", encoding="utf-8")
    (pack / "proc" / "lease" / "active").write_text("", encoding="utf-8")
    (pack / "proc" / "lease" / "preemptions").write_text("", encoding="utf-8")

    # Optional audit sample (already redacted).
    (pack / "audit" / "export").write_text(
        "{\"journal_base\":0,\"journal_next\":1,\"decisions_base\":0,\"decisions_next\":0,\"replay_enabled\":false,\"replay_max_entries\":0}\n",
        encoding="utf-8",
    )
    (pack / "audit" / "journal").write_text(
        "{\"seq\":1,\"kind\":\"queen-ctl\",\"path\":\"/queen/ctl\",\"payload\":\"{}\",\"outcome\":\"ok\",\"error\":null,\"role\":\"queen\",\"ticket\":\"sha256:dead\"}\n",
        encoding="utf-8",
    )
    (pack / "audit" / "decisions").write_text("", encoding="utf-8")

    return pack


def test_ci_evidence_pack_example_runs(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[1]
    pack = _make_pack(tmp_path)
    script = root / "examples" / "ci_evidence_pack.py"

    completed = subprocess.run(
        [sys.executable, str(script), "--pack", str(pack)],
        check=False,
        capture_output=True,
        text=True,
    )
    assert completed.returncode == 0, completed.stderr
    summary = json.loads(completed.stdout)
    assert summary["ok"] is True
    assert summary["missing"] == []


def test_siem_export_example_is_deterministic(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[1]
    pack = _make_pack(tmp_path)
    script = root / "examples" / "siem_export_ndjson.py"

    def run_once() -> str:
        completed = subprocess.run(
            [sys.executable, str(script), "--pack", str(pack)],
            check=True,
            capture_output=True,
            text=True,
        )
        return completed.stdout

    first = run_once()
    second = run_once()
    assert first == second
    assert "cohesix-ticket-" not in first
    lines = [line for line in first.splitlines() if line.strip()]
    assert len(lines) >= 1
    for line in lines:
        obj = json.loads(line)
        assert obj.get("schema") == "cohesix-siem-event/v1"

