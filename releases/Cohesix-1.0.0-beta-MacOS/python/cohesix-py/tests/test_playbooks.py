# Author: Lukas Bower
# Purpose: Validate Cohesix playbooks, generated mappings, and reports.
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
    built_in_playbooks,
    describe_playbooks,
    execute_playbook,
    load_playbook,
    playbook_ids,
    world_class_playbooks,
)

REPO_ROOT = Path(__file__).resolve().parents[3]
PROBE_DEPENDENCIES = {
    "docker-provider",
    "gpu-host-provider",
    "kubernetes-provider",
    "peft-host-provider",
    "systemd-provider",
}


def test_built_in_playbooks_cover_expected_use_cases() -> None:
    lookup = built_in_playbooks()
    assert len(lookup) == 9
    assert "mac-release-factory" in lookup
    assert "jetson-traffic-safety" in lookup
    assert "mixed-closed-loop-ai-factory" in lookup
    assert world_class_playbooks() == lookup


def test_playbook_use_cases_and_probes_match_generated_graph() -> None:
    graph = json.loads(
        (REPO_ROOT / "configs/generated/host_integration_dependency.json").read_text(
            encoding="utf-8"
        )
    )
    graph_playbooks = {row["id"]: row for row in graph["playbooks"]}
    dependencies = {row["id"]: row for row in graph["dependencies"]}

    for playbook_id, playbook in built_in_playbooks().items():
        graph_playbook = graph_playbooks[playbook_id]
        assert playbook.use_case_id == graph_playbook["use_case"]
        expected_probes = {
            dependency_id
            for dependency_id in PROBE_DEPENDENCIES
            if playbook_id in dependencies[dependency_id]["playbooks"]
        }
        assert set(playbook.probes.dependency_ids()) == expected_probes


def test_describe_playbooks_has_plan_summary() -> None:
    rendered = describe_playbooks()
    assert len(rendered) == len(playbook_ids())
    first = rendered[0]
    assert "playbook_id" in first
    assert first["workflow_kind"] == "control-model"
    assert first["next_milestone"] == "m27b-live-reference-workflows"
    assert "capability_summary" in first
    assert "provider_probes" in first
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
        assert report.workflow_kind == "control-model"
        assert report.plan_summary["schedule"] == 2
        assert report.next_milestone == "m27b-live-reference-workflows"
        assert not report.production_use_case_accepted
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
