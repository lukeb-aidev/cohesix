# Author: Lukas Bower
# Purpose: Build deterministic lease/run receipt artifacts for the Cohesix Python SDK.
# Copyright 2026 Lukas Bower

"""Receipt helpers for receipt-backed GPU lease and run workflows."""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import TYPE_CHECKING, Any, Dict, List, Mapping, Optional

from .audit import CohesixAudit
from .backends import Backend
from .errors import CohesixError
from .generated import GPU_RECEIPT_ACTIONS, PEFT_RECEIPT_ACTIONS
from .worker import (
    TargetProfileContract,
    WorkerIdentity,
    normalize_worker_role,
    validate_worker_id,
)

if TYPE_CHECKING:
    from .client import GpuLeaseArgs

RECEIPT_SCHEMA = "cohesix-receipt-v1"
WORKER_GPU_RECEIPT_SCHEMA = "worker-gpu-receipt/v1"
WORKER_LORA_RECEIPT_SCHEMA = "worker-lora-receipt/v1"
MAX_WORKER_RECEIPT_BYTES = 8192
_RECEIPT_DIGEST_FIELDS = ("ticket", "idempotency", "operation", "subject", "result")


def build_lease_receipt(
    backend: Backend,
    defaults: Mapping[str, Any],
    args: "GpuLeaseArgs",
    *,
    status: str,
    error: Optional[Exception],
    audit: Optional[CohesixAudit],
) -> Dict[str, Any]:
    """Build the receipt payload for a GPU lease operation."""

    bounds = _resolve_bounds(backend, defaults)
    receipt = {
        "schema": RECEIPT_SCHEMA,
        "kind": "gpu-lease",
        "manifest_sha256": _manifest_label(defaults),
        "request": _lease_request(args),
        "status": status,
        "error": _safe_error_detail(error),
        "ack": _find_ack_line(audit, "ECHO"),
        "proc_lease": _snapshot_proc_lease(backend, bounds, include_active_entries=True),
    }
    return _strip_none(receipt)


def build_run_receipt(
    backend: Backend,
    defaults: Mapping[str, Any],
    *,
    gpu_id: str,
    command: List[str],
    status: str,
    error: Optional[Exception],
    audit: Optional[CohesixAudit],
) -> Dict[str, Any]:
    """Build the receipt payload for a lease-validated run operation."""

    bounds = _resolve_bounds(backend, defaults)
    receipt = {
        "schema": RECEIPT_SCHEMA,
        "kind": "run",
        "manifest_sha256": _manifest_label(defaults),
        "gpu_id": gpu_id,
        "command": list(command),
        "status": status,
        "error": _safe_error_detail(error),
        "acks": _extract_ack_lines(audit),
        "proc_lease": _snapshot_proc_lease(backend, bounds, include_active_entries=False),
    }
    return _strip_none(receipt)


def write_receipt_json(path: Path, payload: Mapping[str, Any]) -> None:
    """Write a receipt JSON file atomically."""

    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".partial")
    encoded = json.dumps(payload, indent=2, sort_keys=True).encode("utf-8") + b"\n"
    tmp.write_bytes(encoded)
    os.replace(tmp, path)


def _lease_request(args: "GpuLeaseArgs") -> Dict[str, Any]:
    request: Dict[str, Any] = {
        "gpu_id": args.gpu_id,
        "mem_mb": int(args.mem_mb),
        "streams": int(args.streams),
        "ttl_s": int(args.ttl_s),
        "priority": int(args.priority or 0),
    }
    if args.budget_ttl_s is not None:
        request["budget_ttl_s"] = int(args.budget_ttl_s)
    if args.budget_ops is not None:
        request["budget_ops"] = int(args.budget_ops)
    return request


def _snapshot_proc_lease(
    backend: Backend,
    bounds: Mapping[str, Any],
    *,
    include_active_entries: bool,
) -> Dict[str, Any]:
    proc_lease = (
        bounds.get("observability", {})
        .get("proc_lease", {})
    )
    summary = _read_optional_text(
        backend,
        "/proc/lease/summary",
        enabled=bool(proc_lease.get("summary", False)),
        max_bytes=int(proc_lease.get("summary_bytes", 0) or 0),
    )
    active = _read_optional_text(
        backend,
        "/proc/lease/active",
        enabled=bool(proc_lease.get("active", False)),
        max_bytes=int(proc_lease.get("active_bytes", 0) or 0),
    )
    preemptions = _read_optional_text(
        backend,
        "/proc/lease/preemptions",
        enabled=bool(proc_lease.get("preemptions", False)),
        max_bytes=int(proc_lease.get("preemptions_bytes", 0) or 0),
    )
    payload: Dict[str, Any] = {
        "summary": summary,
        "active": active,
        "preemptions": preemptions,
    }
    if include_active_entries:
        payload["active_entries"] = _parse_proc_lease_active(active or "")
    return payload


def _read_optional_text(
    backend: Backend, path: str, *, enabled: bool, max_bytes: int
) -> Optional[str]:
    if not enabled:
        return None
    if max_bytes <= 0:
        return None
    try:
        payload = backend.read_file(path, max_bytes)
    except Exception:
        return None
    try:
        return payload.decode("utf-8")
    except UnicodeDecodeError:
        return None


def _parse_proc_lease_active(text: str) -> List[Dict[str, Any]]:
    out: List[Dict[str, Any]] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        fields = _parse_kv_line(line)
        seq_value = _parse_int(fields.get("seq"))
        if (
            seq_value is None
            or "id" not in fields
            or "subject" not in fields
            or "resource" not in fields
            or "state" not in fields
        ):
            continue
        out.append(
            {
                "id": fields["id"],
                "subject": fields["subject"],
                "resource": fields["resource"],
                "state": fields["state"],
                "seq": seq_value,
            }
        )
    return out


def _parse_kv_line(line: str) -> Dict[str, str]:
    out: Dict[str, str] = {}
    for part in line.split():
        if "=" not in part:
            continue
        key, value = part.split("=", 1)
        out[key] = value
    return out


def _parse_int(value: Optional[str]) -> Optional[int]:
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def _find_ack_line(audit: Optional[CohesixAudit], verb: str) -> Optional[str]:
    if audit is None:
        return None
    ok_prefix = f"OK {verb} "
    err_prefix = f"ERR {verb} "
    for line in reversed(audit.lines):
        if line.startswith(ok_prefix) or line.startswith(err_prefix):
            return line
    return None


def _extract_ack_lines(audit: Optional[CohesixAudit]) -> List[str]:
    if audit is None:
        return []
    return [
        line
        for line in audit.lines
        if line.startswith("OK ") or line.startswith("ERR ")
    ]


def _safe_error_detail(error: Optional[Exception]) -> Optional[str]:
    if error is None:
        return None
    detail = str(error)
    if len(detail) <= 256:
        return detail
    return detail[:256]


def _resolve_bounds(backend: Backend, defaults: Mapping[str, Any]) -> Dict[str, Any]:
    live_bounds = backend.get_bounds()
    if isinstance(live_bounds, dict) and live_bounds:
        return live_bounds
    return {
        "manifest_sha256": _manifest_label(defaults),
        "observability": defaults.get("observability", {}),
    }


def _manifest_label(defaults: Mapping[str, Any]) -> str:
    value = defaults.get("manifest_sha256")
    return value if isinstance(value, str) and value else "unknown"


def _strip_none(payload: Dict[str, Any]) -> Dict[str, Any]:
    return {key: value for key, value in payload.items() if value is not None}


class WorkerReceipt:
    """Validated Worker receipt projection with explicit authority status."""

    def __init__(
        self,
        *,
        schema: str,
        action: str,
        public_instance_id: str,
        identity: WorkerIdentity,
        sequence: int,
        outcome: str,
        digests: Mapping[str, str],
        state: str,
        local_admitted: bool,
        authoritative: bool,
    ) -> None:
        self.schema = schema
        self.action = action
        self.public_instance_id = public_instance_id
        self.identity = identity
        self.sequence = sequence
        self.outcome = outcome
        self.digests = dict(digests)
        self.state = state
        self.local_admitted = local_admitted
        self.authoritative = authoritative


class CompatibilityReceipt:
    """Version-1 host compatibility receipt; never a Worker receipt."""

    def __init__(self, payload: Mapping[str, Any]) -> None:
        self.schema = RECEIPT_SCHEMA
        self.payload = dict(payload)
        self.state = "none"
        self.authoritative = False


def parse_receipt(
    payload: bytes | str | Mapping[str, Any],
    *,
    contract: TargetProfileContract,
    expected_identity: Optional[WorkerIdentity] = None,
    expected_instance_id: Optional[str] = None,
    source: str = "untrusted",
) -> WorkerReceipt | CompatibilityReceipt:
    """Parse a v1 compatibility receipt or exact local-only v2 Worker receipt."""

    value = _receipt_object(payload)
    schema = value.get("schema")
    if schema == RECEIPT_SCHEMA:
        if len(_canonical_receipt_bytes(value)) > MAX_WORKER_RECEIPT_BYTES:
            raise CohesixError("compatibility receipt exceeds bounded size")
        return CompatibilityReceipt(value)
    if schema not in (WORKER_GPU_RECEIPT_SCHEMA, WORKER_LORA_RECEIPT_SCHEMA):
        raise CohesixError("unsupported Worker receipt schema")
    if source != "local-admitted":
        raise CohesixError("version-2 Worker receipts are local-admitted only")
    exact_keys = {
        "schema",
        "action",
        "public_instance_id",
        "identity",
        "sequence",
        "committed_sequence",
        "outcome",
        "digests",
        "manifest_sha256",
    }
    if set(value) != exact_keys:
        raise CohesixError("Worker receipt fields are missing or unknown")
    if value["manifest_sha256"] != contract.manifest_sha256:
        raise CohesixError("Worker receipt manifest differs from target contract")
    public_instance_id = validate_worker_id(str(value["public_instance_id"]))
    identity_value = value["identity"]
    if not isinstance(identity_value, dict) or set(identity_value) != {
        "role",
        "slot",
        "lease_epoch",
        "supervisor_generation",
        "cap_generation",
    }:
        raise CohesixError("Worker receipt identity is invalid")
    role = normalize_worker_role(str(identity_value["role"]))
    identity = WorkerIdentity(
        role=role,
        slot=_receipt_int(identity_value["slot"], "slot", allow_zero=True),
        lease_epoch=_receipt_int(identity_value["lease_epoch"], "lease_epoch"),
        supervisor_generation=_receipt_int(
            identity_value["supervisor_generation"], "supervisor_generation"
        ),
        cap_generation=_receipt_int(identity_value["cap_generation"], "cap_generation"),
    )
    action = str(value["action"])
    if schema == WORKER_GPU_RECEIPT_SCHEMA:
        if role != "worker-gpu" or action not in GPU_RECEIPT_ACTIONS:
            raise CohesixError("GPU receipt role/action is invalid")
    elif role != "worker-lora" or action not in PEFT_RECEIPT_ACTIONS:
        raise CohesixError("PEFT receipt role/action is invalid")
    sequence = _receipt_int(value["sequence"], "sequence")
    committed_sequence = _receipt_int(value["committed_sequence"], "committed_sequence")
    if sequence != committed_sequence:
        raise CohesixError("Worker receipt is not sequence-last committed")
    outcome = str(value["outcome"])
    if outcome not in ("confirmed", "rejected"):
        raise CohesixError("Worker receipt outcome is not terminal")
    digest_value = value["digests"]
    if not isinstance(digest_value, dict) or set(digest_value) != set(_RECEIPT_DIGEST_FIELDS):
        raise CohesixError("Worker receipt digest set is incomplete or unordered")
    digests: Dict[str, str] = {}
    for field in _RECEIPT_DIGEST_FIELDS:
        digest = str(digest_value[field])
        if len(digest) != 64 or any(ch not in "0123456789abcdef" for ch in digest):
            raise CohesixError(f"Worker receipt digest {field} is not lowercase SHA-256")
        digests[field] = digest
    stale = (expected_identity is not None and identity != expected_identity) or (
        expected_instance_id is not None
        and public_instance_id != validate_worker_id(expected_instance_id)
    )
    return WorkerReceipt(
        schema=schema,
        action=action,
        public_instance_id=public_instance_id,
        identity=identity,
        sequence=sequence,
        outcome=outcome,
        digests=digests,
        state="stale" if stale else outcome,
        local_admitted=True,
        authoritative=False,
    )


def receipt_actions_for_role(role: str) -> tuple[str, ...]:
    """Return the exact compiler-generated receipt action matrix for a role."""

    canonical = normalize_worker_role(role)
    if canonical == "worker-gpu":
        return tuple(GPU_RECEIPT_ACTIONS)
    if canonical == "worker-lora":
        return tuple(PEFT_RECEIPT_ACTIONS)
    if canonical in ("worker-heartbeat", "worker-bus"):
        return ()
    raise CohesixError(f"unsupported Worker receipt role {canonical}")


def _receipt_object(payload: bytes | str | Mapping[str, Any]) -> Dict[str, Any]:
    if isinstance(payload, Mapping):
        value = dict(payload)
        raw = _canonical_receipt_bytes(value)
    else:
        raw = payload.encode("utf-8") if isinstance(payload, str) else payload
        if len(raw) > MAX_WORKER_RECEIPT_BYTES:
            raise CohesixError("Worker receipt exceeds bounded size")
        try:
            decoded = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise CohesixError("Worker receipt is not valid UTF-8 JSON") from exc
        if not isinstance(decoded, dict):
            raise CohesixError("Worker receipt must be a JSON object")
        value = decoded
    if len(raw) > MAX_WORKER_RECEIPT_BYTES:
        raise CohesixError("Worker receipt exceeds bounded size")
    _reject_receipt_sensitive(value)
    return value


def _canonical_receipt_bytes(value: Mapping[str, Any]) -> bytes:
    try:
        return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise CohesixError("Worker receipt is not bounded JSON") from exc


def _receipt_int(value: Any, field: str, *, allow_zero: bool = False) -> int:
    minimum = 0 if allow_zero else 1
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value < 2**64:
        raise CohesixError(f"Worker receipt {field} is invalid")
    return value


def _reject_receipt_sensitive(value: Any) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            lowered = str(key).lower()
            if lowered in {
                "authorization",
                "capability",
                "capability_value",
                "cptr",
                "raw_badge",
                "secret",
                "ticket_secret",
                "token",
            }:
                raise CohesixError("Worker receipt contains prohibited authority data")
            _reject_receipt_sensitive(child)
    elif isinstance(value, list):
        for child in value:
            _reject_receipt_sensitive(child)
    elif isinstance(value, str):
        lowered = value.lower()
        if any(
            marker in lowered
            for marker in (
                "authorization: bearer ",
                "bearer ey",
                "capability_value=",
                "cohesix-ticket-",
                "raw_badge=",
                "secret=",
                "token=",
            )
        ):
            raise CohesixError("Worker receipt contains prohibited authority data")
