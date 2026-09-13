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

if TYPE_CHECKING:
    from .client import GpuLeaseArgs

RECEIPT_SCHEMA = "cohesix-receipt-v1"


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
        "manifest_sha256": str(defaults.get("manifest_sha256", "unknown")),
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
        "manifest_sha256": str(defaults.get("manifest_sha256", "unknown")),
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
        "manifest_sha256": defaults.get("manifest_sha256", "unknown"),
        "observability": defaults.get("observability", {}),
    }


def _strip_none(payload: Dict[str, Any]) -> Dict[str, Any]:
    return {key: value for key, value in payload.items() if value is not None}
