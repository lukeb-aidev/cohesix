# Author: Lukas Bower
# Purpose: Validate high-level Cohesix orchestration controls and environment backend selection.
# Copyright 2026 Lukas Bower

"""Tests for `cohesix.orchestration`."""

from __future__ import annotations

import os
import tempfile
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.audit import CohesixAudit  # noqa: E402
from cohesix.backends import MockBackend, RestBackend  # noqa: E402
from cohesix.errors import CohesixError  # noqa: E402
from cohesix.orchestration import (  # noqa: E402
    ApprovalRequest,
    CohesixOrchestrator,
    ControlPlan,
    ExportRequest,
    HostTicketRequest,
    K8sRbacIntent,
    LeaseRequest,
    ScheduleRequest,
)


def test_execute_plan_writes_control_files() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend=backend)
        audit = CohesixAudit()

        plan = ControlPlan(
            approvals=(
                ApprovalRequest(
                    approval_id="approve-a",
                    target_path="/queen/schedule/ctl",
                ),
            ),
            schedule=(
                ScheduleRequest(
                    request_id="sched-a",
                    role="worker-gpu",
                    priority=5,
                    ticks=2,
                    budget_ms=120,
                ),
            ),
            leases=(
                LeaseRequest(
                    op="grant",
                    lease_id="lease-a",
                    subject="queen",
                    resource="gpu0",
                    ttl_s=120,
                    priority=5,
                ),
            ),
            exports=(
                ExportRequest(op="open", export_id="export-a", ttl_s=300),
            ),
        )
        result = orchestrator.execute_plan(plan=plan, dry_run=False, audit=audit)

        assert len(result.approval_writes) == 1
        assert len(result.schedule_writes) == 1
        assert len(result.lease_writes) == 1
        assert len(result.export_writes) == 1

        schedule_path = Path(tmp) / "queen" / "schedule" / "ctl"
        assert schedule_path.is_file()
        assert "sched-a" in schedule_path.read_text(encoding="utf-8")


def test_read_proc_snapshot() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "proc" / "schedule").mkdir(parents=True, exist_ok=True)
        (root / "proc" / "lease").mkdir(parents=True, exist_ok=True)
        (root / "proc" / "schedule" / "summary").write_text(
            "queue=1 dequeued=0 dropped=0 max_entries=64\n",
            encoding="utf-8",
        )
        (root / "proc" / "schedule" / "queue").write_text(
            "id=sched-a role=worker-gpu priority=4 ticks=2 budget_ms=120 seq=1\n",
            encoding="utf-8",
        )
        (root / "proc" / "lease" / "summary").write_text(
            "active=1 preemptions=0 quotas=1 max_active=64 max_preemptions=64\n",
            encoding="utf-8",
        )
        (root / "proc" / "lease" / "active").write_text(
            "id=lease-a subject=queen resource=gpu0 ttl_s=300 priority=5 state=ACTIVE seq=1\n",
            encoding="utf-8",
        )
        (root / "proc" / "lease" / "preemptions").write_text("", encoding="utf-8")

        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend=backend)
        snapshot = orchestrator.read_proc_snapshot()

        assert snapshot.schedule_summary.startswith("queue=1")
        assert len(snapshot.schedule_queue) == 1
        assert snapshot.lease_summary.startswith("active=1")
        assert len(snapshot.lease_active) == 1
        assert snapshot.lease_preemptions == []


def test_from_env_selects_mock_backend() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        original = dict(os.environ)
        try:
            os.environ["COHESIX_MOCK"] = "1"
            os.environ["COHESIX_MOCK_ROOT"] = tmp
            orchestrator = CohesixOrchestrator.from_env()
            assert isinstance(orchestrator.backend, MockBackend)
        finally:
            os.environ.clear()
            os.environ.update(original)


def test_enqueue_host_tickets_writes_spec_stream() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend=backend)
        writes = orchestrator.enqueue_host_tickets(
            [
                HostTicketRequest(
                    ticket_id="ticket-1",
                    idempotency_key="idem-1",
                    action="systemd.restart",
                    target="/host/systemd/cohesix-agent.service/restart",
                    args={"unit": "cohesix-agent.service"},
                )
            ]
        )
        assert len(writes) == 1
        spec_path = Path(tmp) / "host" / "tickets" / "spec"
        content = spec_path.read_text(encoding="utf-8").strip()
        assert '"action":"systemd.restart"' in content
        assert '"id":"ticket-1"' in content


def test_enqueue_federated_host_tickets_sets_relay_envelope() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend=backend)
        writes = orchestrator.enqueue_federated_host_tickets(
            source_hive="hive-a",
            target_hive="hive-b",
            requests=[
                HostTicketRequest(
                    ticket_id="ticket-fed-1",
                    idempotency_key="idem-fed-1",
                    action="systemd.restart",
                    target="/host/systemd/cohesix-agent.service/restart",
                )
            ],
        )
        assert len(writes) == 1
        spec_path = Path(tmp) / "host" / "tickets" / "spec"
        content = spec_path.read_text(encoding="utf-8").strip()
        assert '"source_hive":"hive-a"' in content
        assert '"target_hive":"hive-b"' in content
        assert '"relay_hop":1' in content
        assert '"relay_correlation_id":"ticket-fed-1:idem-fed-1:hive-a:hive-b"' in content


def test_host_ticket_request_requires_both_hive_fields() -> None:
    try:
        HostTicketRequest(
            ticket_id="ticket-fed-2",
            idempotency_key="idem-fed-2",
            action="systemd.restart",
            source_hive="hive-a",
        )
    except CohesixError as exc:
        assert "target_hive" in str(exc)
    else:  # pragma: no cover
        raise AssertionError("expected source_hive/target_hive validation failure")


def test_enqueue_k8s_rbac_tickets_translates_intents() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend=backend)
        writes = orchestrator.enqueue_k8s_rbac_tickets(
            [
                K8sRbacIntent(
                    intent_id="intent-1",
                    subject="ops-user",
                    namespace="edge-a",
                    node="node-1",
                    verb="cordon",
                ),
                K8sRbacIntent(
                    intent_id="intent-2",
                    subject="ops-user",
                    namespace="edge-a",
                    node="node-2",
                    verb="lease-sync",
                ),
            ]
        )
        assert len(writes) == 2
        spec_path = Path(tmp) / "host" / "tickets" / "spec"
        lines = [
            line.strip()
            for line in spec_path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        assert len(lines) == 2
        assert '"action":"k8s.cordon"' in lines[0]
        assert '"action":"k8s.lease.sync"' in lines[1]


def test_enqueue_host_tickets_checks_transport_payload_bound() -> None:
    backend = RestBackend("http://127.0.0.1:1")
    orchestrator = CohesixOrchestrator(backend=backend)
    request = HostTicketRequest(
        ticket_id="ticket-long",
        idempotency_key="idem-long",
        action="k8s.cordon",
        target="/host/k8s/node/node-1/cordon",
        args={"subject": "ops", "namespace": "edge"},
    )
    try:
        orchestrator.enqueue_host_tickets([request])
    except CohesixError as exc:
        assert "transport payload bound" in str(exc)
    else:  # pragma: no cover
        raise AssertionError("expected transport payload bound failure")
