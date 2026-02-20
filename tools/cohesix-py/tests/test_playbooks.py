# Author: Lukas Bower
# Purpose: Validate built-in Cohesix world-class playbooks and execution reports.
# Copyright 2026 Lukas Bower

"""Tests for `cohesix.playbooks`."""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.backends import MockBackend  # noqa: E402
from cohesix.orchestration import CohesixOrchestrator  # noqa: E402
from cohesix.playbooks import (  # noqa: E402
    describe_playbooks,
    execute_playbook,
    load_playbook,
    playbook_ids,
    world_class_playbooks,
)


def test_world_class_playbooks_cover_expected_use_cases() -> None:
    lookup = world_class_playbooks()
    assert len(lookup) == 9
    assert "mac-release-factory" in lookup
    assert "jetson-traffic-safety" in lookup
    assert "mixed-closed-loop-ai-factory" in lookup


def test_describe_playbooks_has_plan_summary() -> None:
    rendered = describe_playbooks()
    assert len(rendered) == len(playbook_ids())
    first = rendered[0]
    assert "playbook_id" in first
    assert "plan" in first
    assert {"approvals", "schedule", "leases", "exports"} <= set(first["plan"].keys())


def test_execute_playbook_dry_run_skips_writes() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend)
        playbook = load_playbook("mac-release-factory")
        report = execute_playbook(
            orchestrator=orchestrator,
            playbook=playbook,
            dry_run=True,
            include_proc_snapshot=False,
            include_host_snapshot=False,
            push_host_snapshot=False,
        )
        assert report.dry_run
        assert len(report.plan_execution.schedule_writes) == 0


def test_execute_playbook_live_mode_writes_controls() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend)
        playbook = load_playbook("jetson-manufacturing-safety")
        report = execute_playbook(
            orchestrator=orchestrator,
            playbook=playbook,
            dry_run=False,
            include_proc_snapshot=False,
            include_host_snapshot=False,
            push_host_snapshot=False,
        )
        assert not report.dry_run
        assert len(report.plan_execution.schedule_writes) >= 1
        assert len(report.plan_execution.lease_writes) >= 1

        lease_ctl = Path(tmp) / "queen" / "lease" / "ctl"
        assert lease_ctl.is_file()
        assert "jetson-qa-lease" in lease_ctl.read_text(encoding="utf-8")


def test_execute_playbook_live_rerun_uses_unique_ids() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        orchestrator = CohesixOrchestrator(backend)
        playbook = load_playbook("mac-release-factory")
        first = execute_playbook(
            orchestrator=orchestrator,
            playbook=playbook,
            dry_run=False,
            include_proc_snapshot=False,
            include_host_snapshot=False,
            push_host_snapshot=False,
        )
        second = execute_playbook(
            orchestrator=orchestrator,
            playbook=playbook,
            dry_run=False,
            include_proc_snapshot=False,
            include_host_snapshot=False,
            push_host_snapshot=False,
        )
        assert first.run_id is not None
        assert second.run_id is not None
        assert first.run_id != second.run_id

        schedule_ctl = Path(tmp) / "queen" / "schedule" / "ctl"
        lines = [
            json.loads(line)
            for line in schedule_ctl.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        assert len(lines) >= 4
        first_ids = {entry["id"] for entry in lines[:2]}
        second_ids = {entry["id"] for entry in lines[2:4]}
        assert first_ids.isdisjoint(second_ids)
