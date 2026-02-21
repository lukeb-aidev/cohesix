# Author: Lukas Bower
# Purpose: Provide high-level, typed orchestration helpers for Cohesix Python workflows.
# Copyright 2026 Lukas Bower

"""Typed orchestration helpers for large Cohesix fleets.

This module stays non-authoritative: it only composes existing append-only control
files and read-only observability paths.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
from dataclasses import dataclass, field, replace
from typing import Any, Dict, Iterable, List, Optional, Sequence

from .audit import CohesixAudit
from .auth import resolve_tcp_auth_token
from .backends import Backend, FilesystemBackend, MockBackend, RestBackend, TcpBackend
from .client import CohesixClient
from .defaults import DEFAULTS
from .errors import CohesixError
from .paths import validate_path

_TOKEN_PATTERN = re.compile(r"^[A-Za-z0-9._-]+$")
_TOKEN_PATTERN_WITH_COLON = re.compile(r"^[A-Za-z0-9._:-]+$")


def _env_bool(name: str, default: bool = False) -> bool:
    raw = os.environ.get(name)
    if raw is None:
        return default
    return raw.strip().lower() in {"1", "true", "yes", "on"}


def _env_int(name: str, default: int) -> int:
    raw = os.environ.get(name)
    if raw is None:
        return default
    try:
        value = int(raw)
    except ValueError as exc:
        raise CohesixError(f"environment variable {name} must be an integer") from exc
    return value


def _env_float(name: str, default: float) -> float:
    raw = os.environ.get(name)
    if raw is None:
        return default
    try:
        value = float(raw)
    except ValueError as exc:
        raise CohesixError(f"environment variable {name} must be a number") from exc
    return value


def _normalize_token(
    field_name: str,
    value: str,
    max_bytes: int = 128,
    allow_colon: bool = False,
) -> str:
    token = value.strip()
    if not token:
        raise CohesixError(f"{field_name} must not be empty")
    if len(token.encode("utf-8")) > max_bytes:
        raise CohesixError(f"{field_name} exceeds {max_bytes} bytes")
    pattern = _TOKEN_PATTERN_WITH_COLON if allow_colon else _TOKEN_PATTERN
    if not pattern.match(token):
        charset = "[A-Za-z0-9._:-]" if allow_colon else "[A-Za-z0-9._-]"
        raise CohesixError(
            f"{field_name} must use ASCII token characters {charset}"
        )
    return token


def _require_positive(field_name: str, value: int) -> int:
    if value <= 0:
        raise CohesixError(f"{field_name} must be > 0")
    return value


@dataclass(frozen=True)
class ScheduleRequest:
    """Single scheduler queue request for `/queen/schedule/ctl`."""

    request_id: str
    role: str
    priority: int
    ticks: int
    budget_ms: int

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "request_id", _normalize_token("request_id", self.request_id)
        )
        object.__setattr__(self, "role", _normalize_token("role", self.role))
        object.__setattr__(self, "priority", _require_positive("priority", self.priority))
        object.__setattr__(self, "ticks", _require_positive("ticks", self.ticks))
        object.__setattr__(
            self, "budget_ms", _require_positive("budget_ms", self.budget_ms)
        )

    def to_payload(self) -> Dict[str, object]:
        return {
            "id": self.request_id,
            "role": self.role,
            "priority": self.priority,
            "ticks": self.ticks,
            "budget_ms": self.budget_ms,
        }


@dataclass(frozen=True)
class LeaseRequest:
    """Single lease control request for `/queen/lease/ctl`."""

    op: str
    lease_id: Optional[str] = None
    subject: Optional[str] = None
    resource: Optional[str] = None
    ttl_s: Optional[int] = None
    priority: Optional[int] = None
    reason: Optional[str] = None
    max_active: Optional[int] = None
    max_preemptions: Optional[int] = None

    def __post_init__(self) -> None:
        normalized_op = _normalize_token("op", self.op, max_bytes=16).lower()
        object.__setattr__(self, "op", normalized_op)
        if normalized_op not in {"grant", "renew", "preempt", "quota"}:
            raise CohesixError("lease op must be one of grant|renew|preempt|quota")

        if normalized_op in {"grant", "renew", "preempt"}:
            if self.lease_id is None:
                raise CohesixError(f"lease_id is required for op={normalized_op}")
            object.__setattr__(
                self, "lease_id", _normalize_token("lease_id", self.lease_id)
            )

        if normalized_op == "grant":
            if self.subject is None or self.resource is None or self.ttl_s is None:
                raise CohesixError("grant requires subject, resource, and ttl_s")
            object.__setattr__(
                self, "subject", _normalize_token("subject", self.subject)
            )
            object.__setattr__(
                self, "resource", _normalize_token("resource", self.resource)
            )
            object.__setattr__(self, "ttl_s", _require_positive("ttl_s", self.ttl_s))
            if self.priority is not None:
                object.__setattr__(
                    self, "priority", _require_positive("priority", self.priority)
                )

        if normalized_op == "renew":
            if self.ttl_s is None:
                raise CohesixError("renew requires ttl_s")
            object.__setattr__(self, "ttl_s", _require_positive("ttl_s", self.ttl_s))
            if self.priority is not None:
                object.__setattr__(
                    self, "priority", _require_positive("priority", self.priority)
                )

        if normalized_op == "preempt":
            if self.reason is None:
                raise CohesixError("preempt requires reason")
            object.__setattr__(self, "reason", _normalize_token("reason", self.reason))

        if normalized_op == "quota":
            if self.subject is None or self.resource is None:
                raise CohesixError("quota requires subject and resource")
            if self.max_active is None or self.max_preemptions is None:
                raise CohesixError("quota requires max_active and max_preemptions")
            object.__setattr__(
                self, "subject", _normalize_token("subject", self.subject)
            )
            object.__setattr__(
                self, "resource", _normalize_token("resource", self.resource)
            )
            object.__setattr__(
                self,
                "max_active",
                _require_positive("max_active", self.max_active),
            )
            object.__setattr__(
                self,
                "max_preemptions",
                _require_positive("max_preemptions", self.max_preemptions),
            )

    def to_payload(self) -> Dict[str, object]:
        payload: Dict[str, object] = {"op": self.op.strip()}
        if self.lease_id is not None:
            payload["id"] = self.lease_id
        if self.subject is not None:
            payload["subject"] = self.subject
        if self.resource is not None:
            payload["resource"] = self.resource
        if self.ttl_s is not None:
            payload["ttl_s"] = self.ttl_s
        if self.priority is not None:
            payload["priority"] = self.priority
        if self.reason is not None:
            payload["reason"] = self.reason
        if self.max_active is not None:
            payload["max_active"] = self.max_active
        if self.max_preemptions is not None:
            payload["max_preemptions"] = self.max_preemptions
        return payload


@dataclass(frozen=True)
class ExportRequest:
    """Single export control request for `/queen/export/ctl`."""

    op: str
    export_id: str
    ttl_s: Optional[int] = None
    reason: Optional[str] = None

    def __post_init__(self) -> None:
        op = _normalize_token("op", self.op, max_bytes=16).lower()
        object.__setattr__(self, "op", op)
        if op not in {"open", "close"}:
            raise CohesixError("export op must be one of open|close")
        object.__setattr__(self, "export_id", _normalize_token("export_id", self.export_id))
        if op == "open":
            if self.ttl_s is None:
                raise CohesixError("open export request requires ttl_s")
            object.__setattr__(self, "ttl_s", _require_positive("ttl_s", self.ttl_s))
        if op == "close":
            if self.reason is None:
                raise CohesixError("close export request requires reason")
            object.__setattr__(self, "reason", _normalize_token("reason", self.reason))

    def to_payload(self) -> Dict[str, object]:
        payload: Dict[str, object] = {"op": self.op.strip(), "id": self.export_id}
        if self.ttl_s is not None:
            payload["ttl_s"] = self.ttl_s
        if self.reason is not None:
            payload["reason"] = self.reason
        return payload


@dataclass(frozen=True)
class ApprovalRequest:
    """Queue approval entry for policy-gated writes."""

    approval_id: str
    target_path: str
    decision: str = "approve"

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "approval_id", _normalize_token("approval_id", self.approval_id)
        )
        target_path = self.target_path.strip()
        validate_path(target_path)
        object.__setattr__(self, "target_path", target_path)
        decision = _normalize_token("decision", self.decision, max_bytes=16).lower()
        if decision not in {"approve", "deny"}:
            raise CohesixError("decision must be approve or deny")
        object.__setattr__(self, "decision", decision)

    def to_payload(self) -> Dict[str, object]:
        return {
            "id": self.approval_id,
            "target": self.target_path,
            "decision": self.decision,
        }


@dataclass(frozen=True)
class HostTicketRequest:
    """Single host control ticket for `/host/tickets/spec`."""

    ticket_id: str
    idempotency_key: str
    action: str
    target: Optional[str] = None
    args: Dict[str, object] = field(default_factory=dict)
    expires_unix_ms: Optional[int] = None
    source_hive: Optional[str] = None
    target_hive: Optional[str] = None
    relay_hop: Optional[int] = None
    relay_correlation_id: Optional[str] = None

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "ticket_id", _normalize_token("ticket_id", self.ticket_id, max_bytes=128)
        )
        object.__setattr__(
            self,
            "idempotency_key",
            _normalize_token("idempotency_key", self.idempotency_key, max_bytes=128),
        )
        object.__setattr__(
            self, "action", _normalize_token("action", self.action, max_bytes=64)
        )
        if self.target is not None:
            target = self.target.strip()
            validate_path(target)
            object.__setattr__(self, "target", target)
        if not isinstance(self.args, dict):
            raise CohesixError("host ticket args must be a JSON object")
        if self.expires_unix_ms is not None and self.expires_unix_ms <= 0:
            raise CohesixError("expires_unix_ms must be > 0 when set")
        if self.source_hive is None and self.target_hive is not None:
            raise CohesixError("source_hive is required when target_hive is set")
        if self.target_hive is None and self.source_hive is not None:
            raise CohesixError("target_hive is required when source_hive is set")
        if self.source_hive is not None:
            object.__setattr__(
                self,
                "source_hive",
                _normalize_token("source_hive", self.source_hive, max_bytes=64),
            )
        if self.target_hive is not None:
            object.__setattr__(
                self,
                "target_hive",
                _normalize_token("target_hive", self.target_hive, max_bytes=64),
            )
        if self.relay_hop is not None:
            hop = _require_positive("relay_hop", self.relay_hop)
            if hop > 32:
                raise CohesixError("relay_hop must be <= 32")
            object.__setattr__(self, "relay_hop", hop)
        if self.relay_correlation_id is not None:
            object.__setattr__(
                self,
                "relay_correlation_id",
                _normalize_token(
                    "relay_correlation_id",
                    self.relay_correlation_id,
                    max_bytes=256,
                    allow_colon=True,
                ),
            )

    def to_payload(self, schema: str = "host-ticket/v1") -> Dict[str, object]:
        payload: Dict[str, object] = {
            "schema": schema,
            "id": self.ticket_id,
            "idempotency_key": self.idempotency_key,
            "action": self.action,
        }
        if self.target is not None:
            payload["target"] = self.target
        if self.args:
            payload["args"] = self.args
        if self.expires_unix_ms is not None:
            payload["expires_unix_ms"] = self.expires_unix_ms
        if self.source_hive is not None:
            payload["source_hive"] = self.source_hive
        if self.target_hive is not None:
            payload["target_hive"] = self.target_hive
        if self.relay_hop is not None:
            payload["relay_hop"] = self.relay_hop
        if self.relay_correlation_id is not None:
            payload["relay_correlation_id"] = self.relay_correlation_id
        return payload


@dataclass(frozen=True)
class K8sRbacIntent:
    """RBAC-scoped Kubernetes coexistence intent translated into one host ticket."""

    intent_id: str
    subject: str
    namespace: str
    node: str
    verb: str
    reason: Optional[str] = None
    ttl_s: Optional[int] = None

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "intent_id", _normalize_token("intent_id", self.intent_id, max_bytes=128)
        )
        object.__setattr__(self, "subject", _normalize_token("subject", self.subject, max_bytes=128))
        object.__setattr__(
            self, "namespace", _normalize_token("namespace", self.namespace, max_bytes=128)
        )
        object.__setattr__(self, "node", _normalize_token("node", self.node, max_bytes=128))
        normalized = _normalize_token("verb", self.verb, max_bytes=32).lower()
        if normalized not in {"cordon", "drain", "lease-sync"}:
            raise CohesixError("verb must be one of cordon|drain|lease-sync")
        object.__setattr__(self, "verb", normalized)
        if self.reason is not None:
            object.__setattr__(
                self, "reason", _normalize_token("reason", self.reason, max_bytes=128)
            )
        if self.ttl_s is not None:
            object.__setattr__(self, "ttl_s", _require_positive("ttl_s", self.ttl_s))

    def to_ticket_request(self) -> HostTicketRequest:
        action_map = {
            "cordon": "k8s.cordon",
            "drain": "k8s.drain",
            "lease-sync": "k8s.lease.sync",
        }
        action = action_map[self.verb]
        idempotency_seed = f"{self.intent_id}.{self.verb}.{self.node}"
        idempotency_key = "k8s" + hashlib.sha256(
            idempotency_seed.encode("utf-8")
        ).hexdigest()[:8]
        args: Dict[str, object] = {"node": self.node}
        if self.reason is not None:
            args["reason"] = self.reason
        if self.ttl_s is not None:
            args["ttl_s"] = self.ttl_s
        return HostTicketRequest(
            ticket_id=self.intent_id,
            idempotency_key=idempotency_key,
            action=action,
            target=None,
            args=args,
        )


@dataclass(frozen=True)
class ControlWriteResult:
    """Result of writing a single control payload."""

    path: str
    payload: str
    bytes_written: int


@dataclass
class ProcSnapshot:
    """Read-only observability snapshot from `/proc/schedule/*` and `/proc/lease/*`."""

    schedule_summary: str = ""
    schedule_queue: List[str] = field(default_factory=list)
    lease_summary: str = ""
    lease_active: List[str] = field(default_factory=list)
    lease_preemptions: List[str] = field(default_factory=list)


@dataclass(frozen=True)
class ControlPlan:
    """Declarative, bounded control plan for orchestrator execution."""

    approvals: Sequence[ApprovalRequest] = ()
    schedule: Sequence[ScheduleRequest] = ()
    leases: Sequence[LeaseRequest] = ()
    exports: Sequence[ExportRequest] = ()


@dataclass
class PlanExecution:
    """Execution report for a control plan."""

    approval_writes: List[ControlWriteResult] = field(default_factory=list)
    schedule_writes: List[ControlWriteResult] = field(default_factory=list)
    lease_writes: List[ControlWriteResult] = field(default_factory=list)
    export_writes: List[ControlWriteResult] = field(default_factory=list)


class CohesixOrchestrator:
    """High-level orchestration surface for large fleet workflows.

    The orchestrator wraps `CohesixClient` and exposes typed control-file writes,
    with the same bounds and semantics as existing Cohesix host tools.
    """

    def __init__(self, backend: Backend, defaults: Optional[Dict[str, object]] = None) -> None:
        self.backend = backend
        self.defaults = defaults or DEFAULTS
        self.client = CohesixClient(backend=backend, defaults=self.defaults)
        self.console = self.defaults.get("console", {})
        self.paths = self.defaults.get("paths", {})
        self.control_plane = self.defaults.get("control_plane", {})
        self.observability = self.defaults.get("observability", {})
        self.host_tickets = self.defaults.get("host_tickets", {})

    @classmethod
    def from_env(
        cls,
        include_mig: bool = False,
        defaults: Optional[Dict[str, object]] = None,
    ) -> "CohesixOrchestrator":
        """Construct an orchestrator using environment-driven backend selection.

        Resolution order:
        1) Mock backend (`COHESIX_MOCK=1`)
        2) Filesystem mount (`COHESIX_MOUNT_ROOT`)
        3) REST gateway (`COH_REST_URL`, `HIVE_GATEWAY_URL`, `COHESIX_REST_URL`)
        4) TCP console (default)
        """

        if _env_bool("COHESIX_MOCK", default=False):
            root = os.environ.get("COHESIX_MOCK_ROOT", "out/examples/mockfs")
            return cls(
                MockBackend(root=root, include_mig=include_mig),
                defaults=defaults,
            )

        mount_root = os.environ.get("COHESIX_MOUNT_ROOT")
        if mount_root:
            return cls(FilesystemBackend(mount_root), defaults=defaults)

        rest_url = (
            os.environ.get("COH_REST_URL")
            or os.environ.get("HIVE_GATEWAY_URL")
            or os.environ.get("COHESIX_REST_URL")
        )
        timeout_s = _env_float("COHESIX_TIMEOUT_S", 2.0)
        if rest_url:
            return cls(RestBackend(rest_url, timeout_s=timeout_s), defaults=defaults)

        host = os.environ.get("COH_TCP_HOST") or os.environ.get("COHSH_TCP_HOST") or "127.0.0.1"
        port = _env_int("COH_TCP_PORT", _env_int("COHSH_TCP_PORT", 31337))
        try:
            auth_token = resolve_tcp_auth_token()
        except ValueError as exc:
            raise CohesixError(str(exc)) from exc
        role = os.environ.get("COH_ROLE") or os.environ.get("COHSH_ROLE") or "queen"
        ticket = os.environ.get("COH_TICKET") or os.environ.get("COHSH_TICKET")
        max_retries = _env_int("COHESIX_MAX_RETRIES", 3)

        return cls(
            TcpBackend(
                host=host,
                port=port,
                auth_token=auth_token,
                role=role,
                ticket=ticket,
                timeout_s=timeout_s,
                max_retries=max_retries,
            ),
            defaults=defaults,
        )

    def close(self) -> None:
        close_fn = getattr(self.backend, "close", None)
        if callable(close_fn):
            close_fn()

    def __enter__(self) -> "CohesixOrchestrator":
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        self.close()

    def queue_approvals(
        self,
        approvals: Iterable[ApprovalRequest],
        audit: Optional[CohesixAudit] = None,
    ) -> List[ControlWriteResult]:
        payloads = [json.dumps(item.to_payload(), separators=(",", ":")) for item in approvals]
        return self._append_json_lines("/actions/queue", payloads, 2048, audit)

    def enqueue_schedule(
        self,
        requests: Iterable[ScheduleRequest],
        audit: Optional[CohesixAudit] = None,
    ) -> List[ControlWriteResult]:
        path = str(self.paths.get("queen_schedule_ctl", "/queen/schedule/ctl"))
        max_bytes = int(
            self.control_plane.get("schedule", {}).get("ctl_max_bytes", 8192)
        )
        payloads = [json.dumps(item.to_payload(), separators=(",", ":")) for item in requests]
        return self._append_json_lines(path, payloads, max_bytes, audit)

    def apply_leases(
        self,
        requests: Iterable[LeaseRequest],
        audit: Optional[CohesixAudit] = None,
    ) -> List[ControlWriteResult]:
        path = str(self.paths.get("queen_lease_ctl", "/queen/lease/ctl"))
        max_bytes = int(self.control_plane.get("lease", {}).get("ctl_max_bytes", 8192))
        payloads = [json.dumps(item.to_payload(), separators=(",", ":")) for item in requests]
        return self._append_json_lines(path, payloads, max_bytes, audit)

    def apply_exports(
        self,
        requests: Iterable[ExportRequest],
        audit: Optional[CohesixAudit] = None,
    ) -> List[ControlWriteResult]:
        path = str(self.paths.get("queen_export_ctl", "/queen/export/ctl"))
        max_bytes = int(
            self.control_plane.get("export", {}).get("ctl_max_bytes", 2048)
        )
        payloads = [json.dumps(item.to_payload(), separators=(",", ":")) for item in requests]
        return self._append_json_lines(path, payloads, max_bytes, audit)

    def execute_plan(
        self,
        plan: ControlPlan,
        dry_run: bool = False,
        audit: Optional[CohesixAudit] = None,
    ) -> PlanExecution:
        """Execute a bounded control plan in approval -> schedule -> lease -> export order."""

        if dry_run:
            return PlanExecution()

        return PlanExecution(
            approval_writes=self.queue_approvals(plan.approvals, audit),
            schedule_writes=self.enqueue_schedule(plan.schedule, audit),
            lease_writes=self.apply_leases(plan.leases, audit),
            export_writes=self.apply_exports(plan.exports, audit),
        )

    def enqueue_host_tickets(
        self,
        requests: Iterable[HostTicketRequest],
        audit: Optional[CohesixAudit] = None,
    ) -> List[ControlWriteResult]:
        """Append host control tickets to `/host/tickets/spec`."""

        path = str(self.paths.get("host_tickets_spec", "/host/tickets/spec"))
        schema = str(self.host_tickets.get("request_schema", "host-ticket/v1"))
        max_bytes = int(self.host_tickets.get("max_line_bytes", 2048))
        allowlist = {
            str(action)
            for action in self.host_tickets.get("action_allowlist", [])
            if str(action).strip()
        }

        payloads: List[str] = []
        transport_bound = self._transport_payload_bound(path)
        for request in requests:
            if allowlist and request.action not in allowlist:
                raise CohesixError(
                    f"ticket action {request.action!r} is not in host ticket allowlist"
                )
            payload = request.to_payload(schema=schema)
            payload_text = json.dumps(payload, separators=(",", ":"))
            payload_bytes = len(payload_text.encode("utf-8"))
            if max_bytes > 0 and payload_bytes + 1 > max_bytes:
                raise CohesixError(
                    f"host ticket payload for {request.ticket_id} exceeds bound {max_bytes} bytes"
                )
            if (
                transport_bound is not None
                and payload_bytes > transport_bound
            ):
                raise CohesixError(
                    f"host ticket payload for {request.ticket_id} exceeds transport payload bound "
                    f"{transport_bound} bytes for {path}"
                )
            payloads.append(payload_text)
        return self._append_json_lines(path, payloads, max_bytes, audit)

    def enqueue_k8s_rbac_tickets(
        self,
        intents: Iterable[K8sRbacIntent],
        audit: Optional[CohesixAudit] = None,
    ) -> List[ControlWriteResult]:
        """Translate RBAC intents into host tickets and append them in order."""

        tickets = [intent.to_ticket_request() for intent in intents]
        return self.enqueue_host_tickets(tickets, audit)

    def enqueue_federated_host_tickets(
        self,
        source_hive: str,
        target_hive: str,
        requests: Iterable[HostTicketRequest],
        audit: Optional[CohesixAudit] = None,
    ) -> List[ControlWriteResult]:
        """Append relay-enveloped host tickets for cross-hive forwarding."""

        source_hive_norm = _normalize_token("source_hive", source_hive, max_bytes=64)
        target_hive_norm = _normalize_token("target_hive", target_hive, max_bytes=64)
        if source_hive_norm == target_hive_norm:
            raise CohesixError("source_hive and target_hive must differ")
        enriched: List[HostTicketRequest] = []
        for request in requests:
            correlation = (
                request.relay_correlation_id
                or f"{request.ticket_id}:{request.idempotency_key}:{source_hive_norm}:{target_hive_norm}"
            )
            hop = request.relay_hop if request.relay_hop is not None else 1
            enriched.append(
                replace(
                    request,
                    source_hive=source_hive_norm,
                    target_hive=target_hive_norm,
                    relay_hop=hop,
                    relay_correlation_id=correlation,
                )
            )
        return self.enqueue_host_tickets(enriched, audit)

    def read_proc_snapshot(self, audit: Optional[CohesixAudit] = None) -> ProcSnapshot:
        """Read scheduler and lease observability files into a typed snapshot."""

        proc_schedule = self.observability.get("proc_schedule", {})
        proc_lease = self.observability.get("proc_lease", {})

        schedule_summary_lines = self._read_proc_lines(
            "/proc/schedule/summary",
            enabled=bool(proc_schedule.get("summary", True)),
            max_bytes=int(proc_schedule.get("summary_bytes", 128)),
            audit=audit,
        )
        schedule_queue_lines = self._read_proc_lines(
            "/proc/schedule/queue",
            enabled=bool(proc_schedule.get("queue", True)),
            max_bytes=int(proc_schedule.get("queue_bytes", 256)),
            audit=audit,
        )
        lease_summary_lines = self._read_proc_lines(
            "/proc/lease/summary",
            enabled=bool(proc_lease.get("summary", True)),
            max_bytes=int(proc_lease.get("summary_bytes", 160)),
            audit=audit,
        )
        lease_active_lines = self._read_proc_lines(
            "/proc/lease/active",
            enabled=bool(proc_lease.get("active", True)),
            max_bytes=int(proc_lease.get("active_bytes", 256)),
            audit=audit,
        )
        lease_preemption_lines = self._read_proc_lines(
            "/proc/lease/preemptions",
            enabled=bool(proc_lease.get("preemptions", True)),
            max_bytes=int(proc_lease.get("preemptions_bytes", 256)),
            audit=audit,
        )

        return ProcSnapshot(
            schedule_summary=schedule_summary_lines[0] if schedule_summary_lines else "",
            schedule_queue=schedule_queue_lines,
            lease_summary=lease_summary_lines[0] if lease_summary_lines else "",
            lease_active=lease_active_lines,
            lease_preemptions=lease_preemption_lines,
        )

    def _append_json_lines(
        self,
        path: str,
        payloads: Iterable[str],
        max_bytes: int,
        audit: Optional[CohesixAudit],
    ) -> List[ControlWriteResult]:
        validate_path(path)
        results: List[ControlWriteResult] = []
        transport_bound = self._transport_payload_bound(path)
        for payload in payloads:
            line = payload.strip()
            if not line:
                raise CohesixError("control payload must not be empty")
            line_bytes = len(line.encode("utf-8"))
            if (
                transport_bound is not None
                and line_bytes > transport_bound
            ):
                raise CohesixError(
                    f"control payload for {path} exceeds transport payload bound {transport_bound} bytes"
                )
            encoded = (line + "\n").encode("utf-8")
            if max_bytes > 0 and len(encoded) > max_bytes:
                raise CohesixError(
                    f"control payload for {path} exceeds bound {max_bytes} bytes"
                )
            written = self.backend.write_append(path, encoded)
            result = ControlWriteResult(path=path, payload=line, bytes_written=written)
            results.append(result)
            if audit is not None:
                audit.push_ack("OK", "ECHO", f"path={path} bytes={written}")
        return results

    def _read_proc_lines(
        self,
        path: str,
        enabled: bool,
        max_bytes: int,
        audit: Optional[CohesixAudit],
    ) -> List[str]:
        if not enabled:
            return []
        payload = self.backend.read_file(path, max_bytes)
        if audit is not None:
            audit.push_ack("OK", "CAT", f"path={path}")
        lines = payload.decode("utf-8").splitlines()
        return [line.strip() for line in lines if line.strip()]

    def _transport_payload_bound(self, path: str) -> Optional[int]:
        if not isinstance(self.backend, (TcpBackend, RestBackend)):
            return None
        max_echo_len = int(self.console.get("max_echo_len", 0) or 0)
        max_line_len = int(self.console.get("max_line_len", 0) or 0)
        bound = max_echo_len if max_echo_len > 0 else None
        if max_line_len > 0:
            overhead = len("ECHO ") + len(path) + 1
            line_bound = max(0, max_line_len - overhead)
            bound = line_bound if bound is None else min(bound, line_bound)
        return bound
