# Author: Lukas Bower
# Purpose: Validate Python SDK evidence-pack, timeline, and receipt-backed workflows.
# Copyright 2026 Lukas Bower

"""Tests for Cohesix Python evidence and receipt APIs."""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.audit import CohesixAudit  # noqa: E402
from cohesix.backends import MockBackend  # noqa: E402
from cohesix.client import CohesixClient, GpuLeaseArgs  # noqa: E402


def _seed_evidence_sources(root: Path) -> None:
    (root / "proc" / "schedule").mkdir(parents=True, exist_ok=True)
    (root / "proc" / "lease").mkdir(parents=True, exist_ok=True)
    (root / "audit").mkdir(parents=True, exist_ok=True)
    (root / "log").mkdir(parents=True, exist_ok=True)
    (root / "replay").mkdir(parents=True, exist_ok=True)

    (root / "proc" / "boot").write_text("boot_manifest=d1880bfe\n", encoding="utf-8")
    (root / "proc" / "schedule" / "summary").write_text(
        "queue=1 dequeued=0 dropped=0 max_entries=256\n",
        encoding="utf-8",
    )
    (root / "proc" / "schedule" / "queue").write_text(
        "id=sched-1 role=worker-gpu priority=4 ticks=2 budget_ms=120 seq=1\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "summary").write_text(
        "active=1 preemptions=0 quotas=1 max_active=256 max_preemptions=256\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "active").write_text(
        "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=7\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "preemptions").write_text("", encoding="utf-8")
    (root / "log" / "queen.log").write_text(
        "boot ok\nscheduler ok\n",
        encoding="utf-8",
    )
    (root / "replay" / "status").write_text(
        "{\"enabled\":false,\"entries\":0}\n",
        encoding="utf-8",
    )
    (root / "audit" / "export").write_text(
        json.dumps(
            {
                "journal_base": 0,
                "journal_next": 4096,
                "decisions_base": 0,
                "decisions_next": 1024,
                "replay_enabled": False,
                "replay_max_entries": 0,
            }
        )
        + "\n",
        encoding="utf-8",
    )
    (root / "audit" / "journal").write_text(
        json.dumps(
            {
                "seq": 2,
                "kind": "queen-ctl",
                "path": "/queen/ctl",
                "payload": "{}",
                "outcome": "ok",
                "error": None,
                "role": "queen",
                "ticket": "cohesix-ticket-raw-secret",
            }
        )
        + "\n",
        encoding="utf-8",
    )
    (root / "audit" / "decisions").write_text(
        json.dumps(
            {
                "seq": 3,
                "kind": "policy-gate",
                "outcome": "approve",
                "id": "decision-1",
                "target": "/queen/ctl",
                "path": "/queen/ctl",
                "role": "queen",
                "ticket": "cohesix-ticket-raw-secret",
            }
        )
        + "\n",
        encoding="utf-8",
    )


def _seed_proc_lease(root: Path) -> None:
    (root / "proc" / "lease").mkdir(parents=True, exist_ok=True)
    (root / "proc" / "lease" / "summary").write_text(
        "active=1 preemptions=0 quotas=1 max_active=256 max_preemptions=256\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "active").write_text(
        "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=11\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "preemptions").write_text("", encoding="utf-8")


def test_evidence_pack_export_and_redaction(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    _seed_evidence_sources(root)

    client = CohesixClient(backend)
    audit = CohesixAudit()
    pack_dir = tmp_path / "evidence_pack"
    summary = client.evidence_pack(pack_dir, with_telemetry=False, audit=audit)

    assert summary.captured >= 8
    assert summary.errors == 0
    assert (pack_dir / "meta.json").is_file()
    assert (pack_dir / "bounds.json").is_file()
    assert (pack_dir / "summary.json").is_file()
    assert (pack_dir / "log" / "queen.log").is_file()

    journal = (pack_dir / "audit" / "journal").read_text(encoding="utf-8")
    assert "cohesix-ticket-raw-secret" not in journal
    assert "sha256:" in journal


def test_evidence_timeline_stable_ordering(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    client = CohesixClient(backend)
    pack_dir = tmp_path / "pack"
    (pack_dir / "audit").mkdir(parents=True, exist_ok=True)
    (pack_dir / "proc" / "lease").mkdir(parents=True, exist_ok=True)

    (pack_dir / "audit" / "journal").write_text(
        "\n".join(
            [
                json.dumps(
                    {
                        "seq": 2,
                        "kind": "queen-ctl",
                        "path": "/queen/ctl",
                        "payload": "{}",
                        "outcome": "ok",
                        "error": None,
                        "role": "queen",
                        "ticket": "sha256:dead",
                    }
                ),
                json.dumps(
                    {
                        "seq": 1,
                        "kind": "queen-ctl",
                        "path": "/queen/ctl",
                        "payload": "{}",
                        "outcome": "ok",
                        "error": None,
                        "role": "queen",
                        "ticket": "sha256:beef",
                    }
                ),
                "",
            ]
        ),
        encoding="utf-8",
    )
    (pack_dir / "audit" / "decisions").write_text(
        json.dumps(
            {
                "seq": 3,
                "kind": "policy-gate",
                "outcome": "approve",
                "id": "a1",
                "target": "/queen/ctl",
                "path": "/queen/ctl",
                "role": "queen",
                "ticket": "sha256:cafe",
            }
        )
        + "\n",
        encoding="utf-8",
    )
    (pack_dir / "proc" / "lease" / "active").write_text(
        "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=7\n",
        encoding="utf-8",
    )

    timeline = client.evidence_timeline(pack_dir)
    assert timeline.events == 4
    lines = [
        line
        for line in timeline.ndjson_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    assert len(lines) == 4
    first = json.loads(lines[0])
    second = json.loads(lines[1])
    assert first.get("seq") == 1
    assert second.get("seq") == 2


def test_gpu_lease_receipt_contains_proc_snapshot(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    _seed_proc_lease(root)

    client = CohesixClient(backend)
    audit = CohesixAudit()
    receipt_path = tmp_path / "lease_receipt.json"
    args = GpuLeaseArgs(
        gpu_id="GPU-0",
        mem_mb=1024,
        streams=1,
        ttl_s=60,
        priority=1,
    )
    receipt = client.gpu_lease_with_receipt(args, receipt_path, audit)
    assert receipt["kind"] == "gpu-lease"
    payload = json.loads(receipt_path.read_text(encoding="utf-8"))
    entries = payload["proc_lease"]["active_entries"]
    assert isinstance(entries, list)
    assert any(entry.get("id") == "lease-1" for entry in entries)
    assert "cohesix-ticket-" not in receipt_path.read_text(encoding="utf-8")


def test_run_receipt_no_secrets(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    _seed_proc_lease(root)
    (root / "gpu" / "GPU-0" / "lease").write_text(
        json.dumps(
            {
                "schema": "gpu-lease/v1",
                "state": "ACTIVE",
                "gpu_id": "GPU-0",
                "worker_id": "worker-1",
                "mem_mb": 1,
                "streams": 1,
                "ttl_s": 60,
                "priority": 1,
            }
        )
        + "\n",
        encoding="utf-8",
    )

    client = CohesixClient(backend)
    audit = CohesixAudit()
    receipt_path = tmp_path / "run_receipt.json"
    receipt = client.run_command_with_receipt(
        gpu_id="GPU-0",
        command=[sys.executable, "-c", "print('ok')"],
        receipt_out=receipt_path,
        audit=audit,
    )
    assert receipt["kind"] == "run"
    payload = json.loads(receipt_path.read_text(encoding="utf-8"))
    assert payload["status"] == "ok"
    assert isinstance(payload.get("acks"), list)
    assert payload["acks"]
    text = receipt_path.read_text(encoding="utf-8")
    assert "cohesix-ticket-" not in text
    assert "changeme" not in text
