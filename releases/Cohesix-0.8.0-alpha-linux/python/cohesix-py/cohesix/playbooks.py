# Author: Lukas Bower
# Purpose: Define and execute high-impact Cohesix orchestration playbooks across Mac, Jetson, and mixed fleets.
# Copyright 2026 Lukas Bower

"""World-class Cohesix playbooks for 1k+ worker orchestration."""

from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Dict, Iterable, List, Optional, Sequence, Tuple

from .audit import CohesixAudit
from .client import validate_component
from .integrations import HostSnapshot, collect_host_snapshot, snapshot_to_ndjson
from .orchestration import (
    ApprovalRequest,
    CohesixOrchestrator,
    ControlPlan,
    ExportRequest,
    LeaseRequest,
    PlanExecution,
    ProcSnapshot,
    ScheduleRequest,
)


@dataclass(frozen=True)
class ProbeSpec:
    """Host integration probe selection for a playbook."""

    systemd_services: Tuple[str, ...] = ()
    include_docker: bool = True
    include_k8s: bool = True
    include_nvml: bool = True
    include_peft: bool = True
    k8s_namespace: str = "default"
    k8s_label_selector: str = ""


@dataclass(frozen=True)
class UseCasePlaybook:
    """Declarative use-case playbook composed of existing Cohesix controls."""

    playbook_id: str
    title: str
    fleet: str
    objective: str
    telemetry_device_id: str
    plan: ControlPlan
    probes: ProbeSpec

    def __post_init__(self) -> None:
        validate_component(self.playbook_id)
        validate_component(self.telemetry_device_id)


@dataclass
class PlaybookReport:
    """Execution report with control writes, `/proc` snapshot, and host probe data."""

    playbook_id: str
    title: str
    fleet: str
    objective: str
    dry_run: bool
    plan_execution: PlanExecution
    proc_snapshot: ProcSnapshot
    host_snapshot: Optional[HostSnapshot]
    telemetry_push: Optional[Dict[str, object]]

    def to_dict(self) -> Dict[str, object]:
        payload = asdict(self)
        if self.host_snapshot is None:
            payload["host_snapshot"] = None
        return payload


def world_class_playbooks() -> Dict[str, UseCasePlaybook]:
    """Return built-in playbooks that map directly to high-impact use cases."""

    shared_approvals = (
        ApprovalRequest(approval_id="approve-queen-ctl", target_path="/queen/ctl"),
        ApprovalRequest(approval_id="approve-schedule", target_path="/queen/schedule/ctl"),
        ApprovalRequest(approval_id="approve-lease", target_path="/queen/lease/ctl"),
    )
    return {
        "mac-release-factory": UseCasePlaybook(
            playbook_id="mac-release-factory",
            title="Mac App Release Factory",
            fleet="mac",
            objective="Orchestrate deterministic release waves with auditable scheduling and lease state.",
            telemetry_device_id="mac-release-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="mac-rel-wave-1",
                        role="worker-heartbeat",
                        priority=4,
                        ticks=10,
                        budget_ms=250,
                    ),
                    ScheduleRequest(
                        request_id="mac-rel-wave-2",
                        role="worker-heartbeat",
                        priority=5,
                        ticks=12,
                        budget_ms=300,
                    ),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("cohesix-agent.service",),
                include_docker=True,
                include_k8s=False,
                include_nvml=False,
                include_peft=False,
            ),
        ),
        "mac-private-peft-grid": UseCasePlaybook(
            playbook_id="mac-private-peft-grid",
            title="Mac Private PEFT Grid",
            fleet="mac",
            objective="Coordinate LoRA adapter waves and export windows across private training pools.",
            telemetry_device_id="mac-peft-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="mac-peft-train-1",
                        role="worker-gpu",
                        priority=6,
                        ticks=8,
                        budget_ms=200,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="grant",
                        lease_id="mac-peft-lease-1",
                        subject="queen",
                        resource="gpu0",
                        ttl_s=600,
                        priority=6,
                    ),
                ),
                exports=(
                    ExportRequest(op="open", export_id="mac-peft-export", ttl_s=900),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("cohesix-agent.service",),
                include_docker=True,
                include_k8s=True,
                include_nvml=True,
                include_peft=True,
                k8s_namespace="ml",
                k8s_label_selector="app=trainer",
            ),
        ),
        "mac-endpoint-compliance": UseCasePlaybook(
            playbook_id="mac-endpoint-compliance",
            title="Mac Endpoint Compliance",
            fleet="mac",
            objective="Run periodic compliance sweeps with auditable scheduling and exception capture.",
            telemetry_device_id="mac-compliance-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="mac-comp-scan",
                        role="worker-heartbeat",
                        priority=5,
                        ticks=6,
                        budget_ms=180,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="quota",
                        subject="queen",
                        resource="gpu0",
                        max_active=2,
                        max_preemptions=4,
                    ),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("cohesix-agent.service",),
                include_docker=False,
                include_k8s=False,
                include_nvml=False,
                include_peft=False,
            ),
        ),
        "jetson-traffic-safety": UseCasePlaybook(
            playbook_id="jetson-traffic-safety",
            title="Jetson Traffic Safety Mesh",
            fleet="jetson",
            objective="Schedule edge inference lanes and lease governance for smart corridor operations.",
            telemetry_device_id="jetson-traffic-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="jetson-traffic-wave-1",
                        role="worker-gpu",
                        priority=7,
                        ticks=8,
                        budget_ms=160,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="grant",
                        lease_id="jetson-traffic-lease",
                        subject="queen",
                        resource="gpu0",
                        ttl_s=300,
                        priority=7,
                    ),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("docker.service",),
                include_docker=True,
                include_k8s=True,
                include_nvml=True,
                include_peft=False,
                k8s_namespace="edge",
                k8s_label_selector="app=traffic",
            ),
        ),
        "jetson-manufacturing-safety": UseCasePlaybook(
            playbook_id="jetson-manufacturing-safety",
            title="Jetson Manufacturing Safety + QA",
            fleet="jetson",
            objective="Coordinate visual QA and safety detectors with bounded lease and preemption controls.",
            telemetry_device_id="jetson-factory-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="jetson-qa-line-1",
                        role="worker-gpu",
                        priority=8,
                        ticks=10,
                        budget_ms=190,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="grant",
                        lease_id="jetson-qa-lease",
                        subject="queen",
                        resource="gpu0",
                        ttl_s=420,
                        priority=8,
                    ),
                    LeaseRequest(
                        op="quota",
                        subject="queen",
                        resource="gpu0",
                        max_active=8,
                        max_preemptions=8,
                    ),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("docker.service",),
                include_docker=True,
                include_k8s=False,
                include_nvml=True,
                include_peft=False,
            ),
        ),
        "jetson-critical-infra": UseCasePlaybook(
            playbook_id="jetson-critical-infra",
            title="Jetson Critical Infrastructure Mesh",
            fleet="jetson",
            objective="Apply resilient lease governance for distributed critical-infrastructure sensing.",
            telemetry_device_id="jetson-infra-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="jetson-infra-scan",
                        role="worker-gpu",
                        priority=9,
                        ticks=12,
                        budget_ms=220,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="grant",
                        lease_id="jetson-infra-lease",
                        subject="queen",
                        resource="gpu0",
                        ttl_s=480,
                        priority=9,
                    ),
                ),
                exports=(
                    ExportRequest(op="open", export_id="jetson-infra-export", ttl_s=1200),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("cohesix-agent.service",),
                include_docker=True,
                include_k8s=True,
                include_nvml=True,
                include_peft=False,
                k8s_namespace="infra",
                k8s_label_selector="tier=critical",
            ),
        ),
        "mixed-closed-loop-ai-factory": UseCasePlaybook(
            playbook_id="mixed-closed-loop-ai-factory",
            title="Mixed Closed-Loop AI Factory",
            fleet="mixed",
            objective="Coordinate Mac training and Jetson inference with export and lease lifecycle linkage.",
            telemetry_device_id="mixed-closed-loop-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="mixed-train-wave",
                        role="worker-gpu",
                        priority=6,
                        ticks=9,
                        budget_ms=210,
                    ),
                    ScheduleRequest(
                        request_id="mixed-infer-wave",
                        role="worker-heartbeat",
                        priority=5,
                        ticks=9,
                        budget_ms=180,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="grant",
                        lease_id="mixed-lease-1",
                        subject="queen",
                        resource="gpu0",
                        ttl_s=540,
                        priority=6,
                    ),
                ),
                exports=(
                    ExportRequest(op="open", export_id="mixed-export", ttl_s=900),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("cohesix-agent.service", "docker.service"),
                include_docker=True,
                include_k8s=True,
                include_nvml=True,
                include_peft=True,
                k8s_namespace="ai",
                k8s_label_selector="pipeline=closed-loop",
            ),
        ),
        "mixed-medical-edge-ai": UseCasePlaybook(
            playbook_id="mixed-medical-edge-ai",
            title="Mixed Medical Edge AI",
            fleet="mixed",
            objective="Enforce medically auditable control flow with explicit export windows and lease governance.",
            telemetry_device_id="mixed-medical-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="medical-edge-wave",
                        role="worker-gpu",
                        priority=9,
                        ticks=6,
                        budget_ms=160,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="grant",
                        lease_id="medical-lease-1",
                        subject="queen",
                        resource="gpu0",
                        ttl_s=360,
                        priority=9,
                    ),
                    LeaseRequest(
                        op="quota",
                        subject="queen",
                        resource="gpu0",
                        max_active=2,
                        max_preemptions=2,
                    ),
                ),
                exports=(
                    ExportRequest(op="open", export_id="medical-export", ttl_s=1800),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("cohesix-agent.service",),
                include_docker=True,
                include_k8s=True,
                include_nvml=True,
                include_peft=True,
                k8s_namespace="medical",
                k8s_label_selector="compliance=hipaa",
            ),
        ),
        "mixed-logistics-digital-twin": UseCasePlaybook(
            playbook_id="mixed-logistics-digital-twin",
            title="Mixed Logistics Digital Twin",
            fleet="mixed",
            objective="Coordinate planning and perception workers for ports and logistics operations with full audit history.",
            telemetry_device_id="mixed-logistics-audit",
            plan=ControlPlan(
                approvals=shared_approvals,
                schedule=(
                    ScheduleRequest(
                        request_id="logistics-plan-wave",
                        role="worker-heartbeat",
                        priority=6,
                        ticks=7,
                        budget_ms=175,
                    ),
                    ScheduleRequest(
                        request_id="logistics-edge-wave",
                        role="worker-gpu",
                        priority=7,
                        ticks=7,
                        budget_ms=175,
                    ),
                ),
                leases=(
                    LeaseRequest(
                        op="grant",
                        lease_id="logistics-lease-1",
                        subject="queen",
                        resource="gpu0",
                        ttl_s=420,
                        priority=7,
                    ),
                    LeaseRequest(
                        op="preempt",
                        lease_id="logistics-lease-1",
                        reason="maintenance",
                    ),
                ),
            ),
            probes=ProbeSpec(
                systemd_services=("cohesix-agent.service", "docker.service"),
                include_docker=True,
                include_k8s=True,
                include_nvml=True,
                include_peft=False,
                k8s_namespace="logistics",
                k8s_label_selector="app=digital-twin",
            ),
        ),
    }


def execute_playbook(
    orchestrator: CohesixOrchestrator,
    playbook: UseCasePlaybook,
    dry_run: bool = False,
    include_proc_snapshot: bool = True,
    include_host_snapshot: bool = True,
    push_host_snapshot: bool = True,
    audit: Optional[CohesixAudit] = None,
) -> PlaybookReport:
    """Execute a playbook via existing control-plane semantics."""

    plan_execution = orchestrator.execute_plan(playbook.plan, dry_run=dry_run, audit=audit)

    proc_snapshot = ProcSnapshot()
    if include_proc_snapshot and not dry_run:
        proc_snapshot = orchestrator.read_proc_snapshot(audit=audit)

    host_snapshot: Optional[HostSnapshot] = None
    telemetry_push: Optional[Dict[str, object]] = None
    if include_host_snapshot:
        host_snapshot = collect_host_snapshot(
            systemd_services=playbook.probes.systemd_services,
            include_docker=playbook.probes.include_docker,
            include_k8s=playbook.probes.include_k8s,
            include_nvml=playbook.probes.include_nvml,
            include_peft=playbook.probes.include_peft,
            k8s_namespace=playbook.probes.k8s_namespace,
            k8s_label_selector=playbook.probes.k8s_label_selector,
        )
        if push_host_snapshot and not dry_run:
            payload = snapshot_to_ndjson(host_snapshot)
            if payload.strip():
                telemetry_push = orchestrator.client.telemetry_push(
                    device_id=playbook.telemetry_device_id,
                    payload=payload,
                    mime="application/x-ndjson",
                    audit=audit,
                )

    return PlaybookReport(
        playbook_id=playbook.playbook_id,
        title=playbook.title,
        fleet=playbook.fleet,
        objective=playbook.objective,
        dry_run=dry_run,
        plan_execution=plan_execution,
        proc_snapshot=proc_snapshot,
        host_snapshot=host_snapshot,
        telemetry_push=telemetry_push,
    )


def playbook_ids() -> List[str]:
    """List built-in playbook ids in deterministic order."""

    return sorted(world_class_playbooks().keys())


def load_playbook(playbook_id: str) -> UseCasePlaybook:
    """Resolve a playbook by id or raise a clear error."""

    lookup = world_class_playbooks()
    key = playbook_id.strip()
    if key not in lookup:
        known = ", ".join(sorted(lookup.keys()))
        raise ValueError(f"unknown playbook '{playbook_id}'. expected one of: {known}")
    return lookup[key]


def summarize_plan(plan: ControlPlan) -> Dict[str, int]:
    """Summarize plan complexity for UX and dry-run output."""

    return {
        "approvals": len(plan.approvals),
        "schedule": len(plan.schedule),
        "leases": len(plan.leases),
        "exports": len(plan.exports),
    }


def describe_playbooks(playbooks: Optional[Sequence[UseCasePlaybook]] = None) -> List[Dict[str, object]]:
    """Render concise playbook metadata for UI/CLI listing."""

    items = list(playbooks) if playbooks is not None else list(world_class_playbooks().values())
    rendered: List[Dict[str, object]] = []
    for item in sorted(items, key=lambda value: value.playbook_id):
        rendered.append(
            {
                "playbook_id": item.playbook_id,
                "title": item.title,
                "fleet": item.fleet,
                "objective": item.objective,
                "plan": summarize_plan(item.plan),
            }
        )
    return rendered


def iter_playbooks() -> Iterable[UseCasePlaybook]:
    """Yield playbooks in deterministic id order."""

    for key in playbook_ids():
        yield load_playbook(key)
