# Author: Lukas Bower
# Purpose: Provide deterministic evidence-pack and timeline helpers for the Cohesix Python SDK.
# Copyright 2026 Lukas Bower

"""Evidence-pack and timeline helpers for Cohesix Python clients."""

from __future__ import annotations

import copy
import hashlib
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Mapping, Optional, Tuple

from .audit import CohesixAudit
from .backends import Backend
from .errors import CohesixError
from .worker import TargetProfileContract, WorkerObservation

EVIDENCE_META_SCHEMA = "cohesix-evidence-pack/meta-v1"
EVIDENCE_SUMMARY_SCHEMA = "cohesix-evidence-pack/summary-v1"
EVIDENCE_TIMELINE_SCHEMA = "cohesix-evidence-pack/timeline-v1"
EVIDENCE_REDACTION_TICKET = "sha256"

DEFAULT_AUDIT_EXPORT_MAX_BYTES = 1024
DEFAULT_AUDIT_FALLBACK_MAX_BYTES = 16 * 1024
DEFAULT_REPLAY_STATUS_MAX_BYTES = 1024
DEFAULT_PROC_BOOT_MAX_BYTES = 64 * 1024
DEFAULT_LOG_MAX_BYTES = 128 * 1024
WORKER_INTEGRATION_SCHEMA = "cohesix-worker-integration-evidence/v1"
PYTHON_PROJECTION_DEPENDENCY = "python-sdk-projection"
MAX_PYTHON_PROJECTION_EVIDENCE_BYTES = 64 * 1024
MAX_PYTHON_PROJECTION_RAW_ARTIFACTS = 16
_EVIDENCE_ID = re.compile(r"^[A-Za-z0-9._:-]+$")
_SHA256_FIELDS = (
    "source_sha256",
    "manifest_sha256",
    "kernel_sha256",
    "root_image_sha256",
    "driver_archive_sha256",
    "driver_manifest_sha256",
    "cyw43_coexistence_record_sha256",
    "worker_archive_sha256",
    "worker_image_manifest_sha256",
    "worker_abi_sha256",
)


@dataclass(frozen=True)
class EvidencePackSummary:
    """Summary details for an evidence pack export run."""

    captured: int
    missing: int
    errors: int
    out_dir: Path


@dataclass(frozen=True)
class TimelineSummary:
    """Summary details for timeline generation from an evidence pack."""

    events: int
    ndjson_path: Path
    markdown_path: Path


@dataclass(frozen=True)
class WorkerAcceptanceAxes:
    """Independent Python projection and acceptance axes."""

    request_admitted: bool
    worker_ready: bool
    provider_completed: bool
    worker_receipt: str
    artifact: str
    execution_proof: str
    python_projection_compatible: bool
    runtime_release_accepted: bool
    production_use_case_accepted: bool


def worker_acceptance_axes(observation: WorkerObservation) -> WorkerAcceptanceAxes:
    """Return projection state without synthesizing target or release proof."""

    return WorkerAcceptanceAxes(
        request_admitted=observation.request_admitted,
        worker_ready=observation.state.lifecycle == "ready",
        provider_completed=observation.provider_completed,
        worker_receipt=observation.state.receipt,
        artifact=observation.state.artifact,
        execution_proof=observation.state.execution_proof,
        python_projection_compatible=True,
        runtime_release_accepted=False,
        production_use_case_accepted=False,
    )


def validate_worker_integration_evidence(
    record: Mapping[str, Any],
    *,
    contract: TargetProfileContract,
    dependency_graph_sha256: str,
    expected_target: str,
    matrix_sha256: Optional[str] = None,
    wheel_sha256: Optional[str] = None,
) -> None:
    """Validate one Python projection record without promoting target proof."""

    try:
        encoded = json.dumps(record, sort_keys=True, separators=(",", ":")).encode(
            "utf-8"
        )
    except (RecursionError, TypeError, ValueError) as exc:
        raise CohesixError("Python projection evidence is not bounded JSON") from exc
    if len(encoded) > MAX_PYTHON_PROJECTION_EVIDENCE_BYTES:
        raise CohesixError("Python projection evidence exceeds bounded size")
    _reject_worker_evidence_sensitive(record)
    if not contract.establishes_target_identity:
        raise CohesixError(
            "Python projection evidence requires a file-backed generated contract"
        )
    required = {
        "schema",
        "record_kind",
        "dependency_id",
        "owner_milestone",
        "obligation",
        "observed_mode",
        "dependency_graph_sha256",
        "manifest_sha256",
        "component_sha256",
        "config_sha256",
        "artifact_sha256",
        "host",
        "target_session",
        "execution_proof",
        "outcomes",
        "raw_evidence",
        "verdict",
        "blockers",
    }
    if set(record) != required:
        raise CohesixError("Python projection evidence fields are missing or unknown")
    if (
        record["schema"] != WORKER_INTEGRATION_SCHEMA
        or record["record_kind"] != "worker-integration"
        or record["dependency_id"] != PYTHON_PROJECTION_DEPENDENCY
        or record["owner_milestone"] != "m26e-python-library-as-built-compatibility"
        or record["obligation"] != "release_required"
    ):
        raise CohesixError("Python projection evidence schema or ownership is invalid")
    if record["observed_mode"] != "live" or record["verdict"] != "PASS":
        raise CohesixError("accepted Python projection evidence must be live PASS")
    if record["blockers"] != []:
        raise CohesixError("accepted Python projection evidence cannot have blockers")
    if record["dependency_graph_sha256"] != dependency_graph_sha256:
        raise CohesixError("Python projection evidence names the wrong dependency graph")
    for field in (
        "dependency_graph_sha256",
        "manifest_sha256",
        "component_sha256",
        "config_sha256",
        "artifact_sha256",
    ):
        _worker_evidence_hash(record[field], field)
    if record["manifest_sha256"] != contract.manifest_sha256:
        raise CohesixError("Python projection evidence names the wrong manifest")
    if record["component_sha256"] != contract.contract_sha256:
        raise CohesixError("Python projection evidence names the wrong profile contract")
    if matrix_sha256 is not None and record["config_sha256"] != matrix_sha256:
        raise CohesixError("Python projection evidence names the wrong source matrix")
    if wheel_sha256 is not None and record["artifact_sha256"] != wheel_sha256:
        raise CohesixError("Python projection evidence names the wrong wheel")
    session = record["target_session"]
    if not isinstance(session, dict) or set(session) != {"target", *_SHA256_FIELDS}:
        raise CohesixError("Python projection target_session fields are invalid")
    if session["target"] != expected_target or expected_target != contract.target:
        raise CohesixError("Python projection evidence names the wrong target")
    if session["manifest_sha256"] != contract.manifest_sha256:
        raise CohesixError("Python projection target session manifest is inconsistent")
    for field in _SHA256_FIELDS:
        _worker_evidence_hash(session[field], f"target_session.{field}")
    expected_proof = "qemu" if expected_target == "qemu" else "fresh-pi"
    if record["execution_proof"] != expected_proof:
        raise CohesixError("Python projection evidence has the wrong target proof reference")
    host = record["host"]
    if not isinstance(host, dict) or set(host) != {
        "profile",
        "os",
        "architecture",
        "provider_version",
    }:
        raise CohesixError("Python projection host identity is invalid")
    if host["profile"] not in ("macos-arm64", "linux-x86-64"):
        raise CohesixError("Python projection host profile is unknown")
    expected_host = {
        "macos-arm64": ("macos", "aarch64"),
        "linux-x86-64": ("linux", "x86_64"),
    }[host["profile"]]
    if (host["os"], host["architecture"]) != expected_host:
        raise CohesixError("Python projection host identity is inconsistent")
    if not isinstance(host["provider_version"], str) or not (
        1 <= len(host["provider_version"].encode("utf-8")) <= 256
    ):
        raise CohesixError("Python projection provider version is invalid")
    outcomes = record["outcomes"]
    if not isinstance(outcomes, list) or not outcomes:
        raise CohesixError("Python projection evidence has no outcomes")
    outcome_keys = []
    for outcome in outcomes:
        if not isinstance(outcome, dict) or set(outcome) != {"id", "class", "result"}:
            raise CohesixError("Python projection outcome is invalid")
        outcome_keys.append((outcome["id"], outcome["class"], outcome["result"]))
    if outcome_keys != sorted(set(outcome_keys)):
        raise CohesixError("Python projection outcomes are not sorted and unique")
    if set(outcome_keys) != {
        ("cpython-3-11", "projection-compatibility", "accepted"),
        ("cpython-3-13", "projection-compatibility", "accepted"),
    }:
        raise CohesixError("Python projection outcomes do not cover the exact host matrix")
    raw = record["raw_evidence"]
    if (
        not isinstance(raw, list)
        or not raw
        or len(raw) > MAX_PYTHON_PROJECTION_RAW_ARTIFACTS
    ):
        raise CohesixError("Python projection evidence has no raw artifact references")
    raw_keys = []
    raw_ids = []
    for artifact in raw:
        if not isinstance(artifact, dict) or set(artifact) != {"id", "sha256", "bytes"}:
            raise CohesixError("Python projection raw evidence entry is invalid")
        artifact_id = artifact["id"]
        if (
            not isinstance(artifact_id, str)
            or not 1 <= len(artifact_id.encode("utf-8")) <= 128
            or _EVIDENCE_ID.fullmatch(artifact_id) is None
        ):
            raise CohesixError("Python projection raw evidence id is invalid")
        _worker_evidence_hash(artifact["sha256"], "raw evidence")
        byte_count = artifact["bytes"]
        if (
            isinstance(byte_count, bool)
            or not isinstance(byte_count, int)
            or byte_count <= 0
            or byte_count > 64 * 1024 * 1024
        ):
            raise CohesixError("Python projection raw evidence byte count is invalid")
        raw_keys.append((artifact["id"], artifact["sha256"], artifact["bytes"]))
        raw_ids.append(artifact_id)
    if raw_keys != sorted(set(raw_keys)):
        raise CohesixError("Python projection raw evidence is not sorted and unique")
    if len(raw_ids) != len(set(raw_ids)):
        raise CohesixError("Python projection raw evidence ids are ambiguous")


def build_python_projection_evidence(
    *,
    contract: TargetProfileContract,
    dependency_graph_sha256: str,
    matrix_sha256: str,
    wheel_sha256: str,
    host: Mapping[str, str],
    target_session: Mapping[str, Any],
    interpreter_outcomes: List[Mapping[str, str]],
    raw_evidence: List[Mapping[str, Any]],
) -> Dict[str, Any]:
    """Emit projection evidence by referencing, never replacing, a target session."""

    proof = "qemu" if contract.target == "qemu" else "fresh-pi"
    outcomes = [dict(item) for item in interpreter_outcomes]
    outcomes.sort(key=lambda item: (item["id"], item["class"], item["result"]))
    artifacts = [dict(item) for item in raw_evidence]
    artifacts.sort(key=lambda item: (item["id"], item["sha256"], item["bytes"]))
    record: Dict[str, Any] = {
        "schema": WORKER_INTEGRATION_SCHEMA,
        "record_kind": "worker-integration",
        "dependency_id": PYTHON_PROJECTION_DEPENDENCY,
        "owner_milestone": "m26e-python-library-as-built-compatibility",
        "obligation": "release_required",
        "observed_mode": "live",
        "dependency_graph_sha256": dependency_graph_sha256,
        "manifest_sha256": contract.manifest_sha256,
        "component_sha256": contract.contract_sha256,
        "config_sha256": matrix_sha256,
        "artifact_sha256": wheel_sha256,
        "host": dict(host),
        "target_session": dict(target_session),
        "execution_proof": proof,
        "outcomes": outcomes,
        "raw_evidence": artifacts,
        "verdict": "PASS",
        "blockers": [],
    }
    validate_worker_integration_evidence(
        record,
        contract=contract,
        dependency_graph_sha256=dependency_graph_sha256,
        expected_target=contract.target,
        matrix_sha256=matrix_sha256,
        wheel_sha256=wheel_sha256,
    )
    return record


def _worker_evidence_hash(value: Any, field: str) -> str:
    text = str(value)
    if len(text) != 64 or any(char not in "0123456789abcdef" for char in text):
        raise CohesixError(f"{field} is not lowercase SHA-256")
    return text


def _reject_worker_evidence_sensitive(value: Any) -> None:
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
                "ticket",
                "token",
            }:
                raise CohesixError("Python projection evidence contains sensitive material")
            _reject_worker_evidence_sensitive(child)
    elif isinstance(value, list):
        for child in value:
            _reject_worker_evidence_sensitive(child)
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
            raise CohesixError("Python projection evidence contains sensitive material")


TelemetryPullFn = Callable[[Path, Optional[CohesixAudit]], Tuple[int, int, int]]


def export_evidence_pack(
    backend: Backend,
    defaults: Mapping[str, Any],
    out_dir: Path,
    *,
    with_telemetry: bool,
    telemetry_pull: Optional[TelemetryPullFn],
    audit: Optional[CohesixAudit] = None,
) -> EvidencePackSummary:
    """Export a deterministic evidence pack using existing Cohesix surfaces."""

    out_dir.mkdir(parents=True, exist_ok=True)
    manifest_sha256 = _manifest_label(defaults)
    policy_sha256 = _policy_sha256(defaults)
    bounds = _resolve_bounds(backend, defaults)

    meta = {
        "schema": EVIDENCE_META_SCHEMA,
        "manifest_sha256": manifest_sha256,
        "policy_sha256": policy_sha256,
        "redaction_ticket": EVIDENCE_REDACTION_TICKET,
        "with_telemetry": bool(with_telemetry),
    }
    _write_json_atomic(out_dir / "meta.json", meta)
    _write_json_atomic(out_dir / "bounds.json", bounds)

    items: List[Dict[str, Any]] = []
    _capture_file(
        backend,
        out_dir,
        "/proc/boot",
        "CAT",
        DEFAULT_PROC_BOOT_MAX_BYTES,
        items,
        audit=audit,
    )

    _capture_proc_schedule(backend, out_dir, bounds, items, audit)
    _capture_proc_lease(backend, out_dir, bounds, items, audit)

    log_path = str(bounds.get("paths", {}).get("log", "/log/queen.log"))
    _capture_file(
        backend,
        out_dir,
        log_path,
        "TAIL",
        DEFAULT_LOG_MAX_BYTES,
        items,
        audit=audit,
    )

    _capture_audit(backend, out_dir, items, audit)

    replay_payload = _read_optional(
        backend,
        "/replay/status",
        DEFAULT_REPLAY_STATUS_MAX_BYTES,
        "CAT",
        items,
        audit=audit,
    )
    if replay_payload is not None:
        _write_payload_atomic(out_dir, "/replay/status", replay_payload)

    if with_telemetry:
        if telemetry_pull is None:
            items.append(
                {
                    "path": "/queen/telemetry",
                    "saved_as": "telemetry/",
                    "verb": "PULL",
                    "status": "error",
                    "bytes": None,
                    "detail": "telemetry pull callback is not configured",
                }
            )
        else:
            try:
                devices, segments, bytes_total = telemetry_pull(out_dir / "telemetry", audit)
            except Exception as exc:
                items.append(
                    {
                        "path": "/queen/telemetry",
                        "saved_as": "telemetry/",
                        "verb": "PULL",
                        "status": "error",
                        "bytes": None,
                        "detail": _safe_detail(exc),
                    }
                )
            else:
                if audit is not None:
                    audit.push_line(
                        "evidence telemetry devices={devices} segments={segments} "
                        "bytes={bytes_total} saved=telemetry/".format(
                            devices=devices,
                            segments=segments,
                            bytes_total=bytes_total,
                        )
                    )

    items.sort(key=lambda item: str(item.get("saved_as", "")))
    captured = sum(1 for item in items if item.get("status") == "captured")
    missing = sum(1 for item in items if item.get("status") == "missing")
    errors = sum(1 for item in items if item.get("status") == "error")
    _write_json_atomic(
        out_dir / "summary.json",
        {
            "schema": EVIDENCE_SUMMARY_SCHEMA,
            "captured": captured,
            "missing": missing,
            "errors": errors,
            "items": items,
        },
    )

    if audit is not None:
        audit.push_line(
            "evidence pack saved={saved} captured={captured} "
            "missing={missing} errors={errors}".format(
                saved=out_dir,
                captured=captured,
                missing=missing,
                errors=errors,
            )
        )
    return EvidencePackSummary(
        captured=captured,
        missing=missing,
        errors=errors,
        out_dir=out_dir,
    )


def write_evidence_timeline(pack_dir: Path) -> TimelineSummary:
    """Generate deterministic timeline artifacts from an evidence pack directory."""

    events = _build_timeline_events(pack_dir)
    ndjson_path = pack_dir / "timeline.ndjson"
    markdown_path = pack_dir / "timeline.md"
    _write_timeline_ndjson(ndjson_path, events)
    _write_timeline_markdown(markdown_path, events)
    return TimelineSummary(
        events=len(events),
        ndjson_path=ndjson_path,
        markdown_path=markdown_path,
    )


def _capture_proc_schedule(
    backend: Backend,
    out_dir: Path,
    bounds: Mapping[str, Any],
    items: List[Dict[str, Any]],
    audit: Optional[CohesixAudit],
) -> None:
    sched = (
        bounds.get("observability", {})
        .get("proc_schedule", {})
    )
    if bool(sched.get("summary", False)):
        _capture_file(
            backend,
            out_dir,
            "/proc/schedule/summary",
            "CAT",
            int(sched.get("summary_bytes", 0) or 0),
            items,
            audit=audit,
        )
    else:
        items.append(_missing_item("/proc/schedule/summary", "proc/schedule/summary"))

    if bool(sched.get("queue", False)):
        _capture_file(
            backend,
            out_dir,
            "/proc/schedule/queue",
            "CAT",
            int(sched.get("queue_bytes", 0) or 0),
            items,
            audit=audit,
        )
    else:
        items.append(_missing_item("/proc/schedule/queue", "proc/schedule/queue"))


def _capture_proc_lease(
    backend: Backend,
    out_dir: Path,
    bounds: Mapping[str, Any],
    items: List[Dict[str, Any]],
    audit: Optional[CohesixAudit],
) -> None:
    lease = (
        bounds.get("observability", {})
        .get("proc_lease", {})
    )
    if bool(lease.get("summary", False)):
        _capture_file(
            backend,
            out_dir,
            "/proc/lease/summary",
            "CAT",
            int(lease.get("summary_bytes", 0) or 0),
            items,
            audit=audit,
        )
    else:
        items.append(_missing_item("/proc/lease/summary", "proc/lease/summary"))

    if bool(lease.get("active", False)):
        _capture_file(
            backend,
            out_dir,
            "/proc/lease/active",
            "CAT",
            int(lease.get("active_bytes", 0) or 0),
            items,
            audit=audit,
        )
    else:
        items.append(_missing_item("/proc/lease/active", "proc/lease/active"))

    if bool(lease.get("preemptions", False)):
        _capture_file(
            backend,
            out_dir,
            "/proc/lease/preemptions",
            "CAT",
            int(lease.get("preemptions_bytes", 0) or 0),
            items,
            audit=audit,
        )
    else:
        items.append(_missing_item("/proc/lease/preemptions", "proc/lease/preemptions"))


def _capture_audit(
    backend: Backend,
    out_dir: Path,
    items: List[Dict[str, Any]],
    audit: Optional[CohesixAudit],
) -> None:
    export_payload = _read_optional(
        backend,
        "/audit/export",
        DEFAULT_AUDIT_EXPORT_MAX_BYTES,
        "CAT",
        items,
        audit=audit,
    )
    if export_payload is None:
        return
    _write_payload_atomic(out_dir, "/audit/export", export_payload)

    journal_max, decisions_max = _parse_audit_export_bounds(export_payload)
    if journal_max <= 0:
        journal_max = DEFAULT_AUDIT_FALLBACK_MAX_BYTES
    if decisions_max <= 0:
        decisions_max = DEFAULT_AUDIT_FALLBACK_MAX_BYTES

    journal_payload = _read_optional(
        backend,
        "/audit/journal",
        journal_max,
        "CAT",
        items,
        audit=audit,
    )
    if journal_payload is not None:
        redacted = _redact_ticket_json_lines(journal_payload)
        _write_payload_atomic(out_dir, "/audit/journal", redacted)

    decisions_payload = _read_optional(
        backend,
        "/audit/decisions",
        decisions_max,
        "CAT",
        items,
        audit=audit,
    )
    if decisions_payload is not None:
        redacted = _redact_ticket_json_lines(decisions_payload)
        _write_payload_atomic(out_dir, "/audit/decisions", redacted)


def _capture_file(
    backend: Backend,
    out_dir: Path,
    path: str,
    verb: str,
    max_bytes: int,
    items: List[Dict[str, Any]],
    *,
    audit: Optional[CohesixAudit],
    saved_as: Optional[str] = None,
) -> None:
    if max_bytes <= 0:
        max_bytes = 1
    try:
        if verb == "TAIL":
            payload = backend.tail_file(path, max_bytes)
        else:
            payload = backend.read_file(path, max_bytes)
    except Exception as exc:
        save_path = saved_as or _strip_leading_slash(path)
        if _is_missing_error(exc):
            items.append(_missing_item(path, save_path))
            return
        items.append(_error_item(path, save_path, verb, exc))
        raise

    if audit is not None:
        audit.push_ack("OK", verb, f"path={path}")
    _write_payload_atomic(out_dir, path, payload, saved_as=saved_as)
    items.append(
        {
            "path": path,
            "saved_as": saved_as or _strip_leading_slash(path),
            "verb": verb,
            "status": "captured",
            "bytes": len(payload),
            "detail": None,
        }
    )


def _read_optional(
    backend: Backend,
    path: str,
    max_bytes: int,
    verb: str,
    items: List[Dict[str, Any]],
    *,
    audit: Optional[CohesixAudit],
) -> Optional[bytes]:
    if max_bytes <= 0:
        max_bytes = 1
    try:
        payload = backend.tail_file(path, max_bytes) if verb == "TAIL" else backend.read_file(path, max_bytes)
    except Exception as exc:
        if _is_missing_error(exc):
            items.append(_missing_item(path, _strip_leading_slash(path)))
            return None
        items.append(_error_item(path, _strip_leading_slash(path), verb, exc))
        return None
    if audit is not None:
        audit.push_ack("OK", verb, f"path={path}")
    items.append(
        {
            "path": path,
            "saved_as": _strip_leading_slash(path),
            "verb": verb,
            "status": "captured",
            "bytes": len(payload),
            "detail": None,
        }
    )
    return payload


def _resolve_bounds(backend: Backend, defaults: Mapping[str, Any]) -> Dict[str, Any]:
    live_bounds = backend.get_bounds()
    if isinstance(live_bounds, dict) and live_bounds:
        return live_bounds
    return _default_bounds_snapshot(defaults)


def _default_bounds_snapshot(defaults: Mapping[str, Any]) -> Dict[str, Any]:
    snapshot: Dict[str, Any] = {
        "manifest_sha256": _manifest_label(defaults),
        "secure9p": defaults.get("secure9p", {}),
        "console": defaults.get("console", {}),
        "paths": defaults.get("paths", {}),
        "control_plane": defaults.get("control_plane", {}),
        "policy": defaults.get("policy", {}),
        "observability": defaults.get("observability", {}),
    }
    return copy.deepcopy(snapshot)


def _manifest_label(defaults: Mapping[str, Any]) -> str:
    value = defaults.get("manifest_sha256")
    return value if isinstance(value, str) and value else "unknown"


def _policy_sha256(defaults: Mapping[str, Any]) -> str:
    try:
        encoded = json.dumps(defaults, sort_keys=True, separators=(",", ":")).encode("utf-8")
    except TypeError:
        encoded = json.dumps(str(defaults), sort_keys=True).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _parse_audit_export_bounds(payload: bytes) -> Tuple[int, int]:
    try:
        parsed = json.loads(payload.decode("utf-8").strip())
    except Exception:
        return DEFAULT_AUDIT_FALLBACK_MAX_BYTES, DEFAULT_AUDIT_FALLBACK_MAX_BYTES
    if not isinstance(parsed, dict):
        return DEFAULT_AUDIT_FALLBACK_MAX_BYTES, DEFAULT_AUDIT_FALLBACK_MAX_BYTES

    journal_base = int(parsed.get("journal_base", 0) or 0)
    journal_next = int(parsed.get("journal_next", 0) or 0)
    decisions_base = int(parsed.get("decisions_base", 0) or 0)
    decisions_next = int(parsed.get("decisions_next", 0) or 0)
    journal_window = max(0, journal_next - journal_base)
    decisions_window = max(0, decisions_next - decisions_base)
    return journal_window, decisions_window


def _redact_ticket_json_lines(payload: bytes) -> bytes:
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise CohesixError("audit payload must be UTF-8") from exc

    lines_out: List[str] = []
    for index, raw_line in enumerate(text.splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        try:
            value = json.loads(line)
        except Exception as exc:
            raise CohesixError(
                f"audit JSONL line {index} is not valid JSON; refusing unsanitized payload"
            ) from exc
        if isinstance(value, dict) and isinstance(value.get("ticket"), str):
            ticket = value["ticket"]
            if ticket != "none":
                value["ticket"] = "sha256:" + hashlib.sha256(
                    ticket.encode("utf-8")
                ).hexdigest()
        lines_out.append(json.dumps(value, separators=(",", ":")))
    payload_out = "\n".join(lines_out)
    if payload_out:
        payload_out += "\n"
    return payload_out.encode("utf-8")


def _build_timeline_events(pack_dir: Path) -> List[Dict[str, Any]]:
    events: List[Dict[str, Any]] = []

    journal_path = pack_dir / "audit" / "journal"
    if journal_path.is_file():
        for entry in _parse_jsonl(journal_path, "audit/journal"):
            event = {
                "schema": EVIDENCE_TIMELINE_SCHEMA,
                "kind": entry.get("kind"),
                "source": "audit/journal",
                "seq": entry.get("seq"),
                "lease_seq": None,
                "path": entry.get("path"),
                "outcome": entry.get("outcome"),
                "error": entry.get("error"),
                "role": entry.get("role"),
                "ticket": entry.get("ticket"),
                "payload": entry.get("payload"),
                "id": None,
                "target": None,
                "subject": None,
                "resource": None,
                "state": None,
                "ttl_s": None,
                "priority": None,
            }
            events.append(_strip_none(event))

    decisions_path = pack_dir / "audit" / "decisions"
    if decisions_path.is_file():
        for entry in _parse_jsonl(decisions_path, "audit/decisions"):
            event = {
                "schema": EVIDENCE_TIMELINE_SCHEMA,
                "kind": entry.get("kind"),
                "source": "audit/decisions",
                "seq": entry.get("seq"),
                "lease_seq": None,
                "path": entry.get("path"),
                "outcome": entry.get("outcome"),
                "error": None,
                "role": entry.get("role"),
                "ticket": entry.get("ticket"),
                "payload": None,
                "id": entry.get("id"),
                "target": entry.get("target"),
                "subject": None,
                "resource": None,
                "state": None,
                "ttl_s": None,
                "priority": None,
            }
            events.append(_strip_none(event))

    lease_active_path = pack_dir / "proc" / "lease" / "active"
    if lease_active_path.is_file():
        for entry in _parse_lease_active(lease_active_path):
            event = {
                "schema": EVIDENCE_TIMELINE_SCHEMA,
                "kind": "lease.active",
                "source": "proc/lease/active",
                "seq": None,
                "lease_seq": entry.get("seq"),
                "path": None,
                "outcome": None,
                "error": None,
                "role": None,
                "ticket": None,
                "payload": None,
                "id": entry.get("id"),
                "target": None,
                "subject": entry.get("subject"),
                "resource": entry.get("resource"),
                "state": entry.get("state"),
                "ttl_s": entry.get("ttl_s"),
                "priority": entry.get("priority"),
            }
            events.append(_strip_none(event))

    def _key(event: Mapping[str, Any]) -> Tuple[int, int, str]:
        seq = event.get("seq")
        lease_seq = event.get("lease_seq")
        seq_value = int(seq) if isinstance(seq, int) else (2**63 - 1)
        lease_value = int(lease_seq) if isinstance(lease_seq, int) else (2**63 - 1)
        kind = str(event.get("kind") or "")
        return seq_value, lease_value, kind

    events.sort(key=_key)
    return events


def _parse_jsonl(path: Path, label: str) -> List[Dict[str, Any]]:
    try:
        payload = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        raise CohesixError(f"{label} is not UTF-8 ({path})") from exc
    out: List[Dict[str, Any]] = []
    for index, raw_line in enumerate(payload.splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        try:
            value = json.loads(line)
        except Exception as exc:
            raise CohesixError(f"{label} line {index} is not valid JSON") from exc
        if isinstance(value, dict):
            out.append(value)
    return out


def _parse_lease_active(path: Path) -> List[Dict[str, Any]]:
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        raise CohesixError(f"proc/lease/active is not UTF-8 ({path})") from exc

    out: List[Dict[str, Any]] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        fields = _parse_kv_line(line)
        if not {"id", "subject", "resource"} <= set(fields.keys()):
            continue
        out.append(
            {
                "id": fields.get("id", ""),
                "subject": fields.get("subject", ""),
                "resource": fields.get("resource", ""),
                "state": fields.get("state", ""),
                "ttl_s": _parse_int(fields.get("ttl_s"), default=0),
                "priority": _parse_int(fields.get("priority"), default=0),
                "seq": _parse_int(fields.get("seq"), default=0),
            }
        )
    return out


def _write_timeline_ndjson(path: Path, events: List[Dict[str, Any]]) -> None:
    lines = [json.dumps(event, sort_keys=True) for event in events]
    payload = "\n".join(lines)
    if payload:
        payload += "\n"
    _write_atomic(path, payload.encode("utf-8"))


def _write_timeline_markdown(path: Path, events: List[Dict[str, Any]]) -> None:
    out: List[str] = []
    out.append("# Evidence timeline")
    out.append("")
    out.append(f"events: {len(events)}")
    out.append("")
    for event in events:
        seq = event.get("seq")
        lease_seq = event.get("lease_seq")
        if isinstance(seq, int):
            out.append(
                "- seq={seq} kind={kind} source={source} outcome={outcome} path={path_value}".format(
                    seq=seq,
                    kind=event.get("kind", ""),
                    source=event.get("source", ""),
                    outcome=event.get("outcome", ""),
                    path_value=event.get("path", ""),
                )
            )
            continue
        if isinstance(lease_seq, int):
            out.append(
                "- lease_seq={lease_seq} id={entry_id} subject={subject} "
                "resource={resource} state={state}".format(
                    lease_seq=lease_seq,
                    entry_id=event.get("id", ""),
                    subject=event.get("subject", ""),
                    resource=event.get("resource", ""),
                    state=event.get("state", ""),
                )
            )
    out.append("")
    _write_atomic(path, "\n".join(out).encode("utf-8"))


def _write_payload_atomic(
    out_dir: Path, remote_path: str, payload: bytes, *, saved_as: Optional[str] = None
) -> None:
    relative = saved_as or _strip_leading_slash(remote_path)
    target = out_dir / _safe_relative_path(relative)
    _write_atomic(target, payload)


def _write_json_atomic(path: Path, payload: Mapping[str, Any]) -> None:
    encoded = json.dumps(payload, indent=2, sort_keys=True).encode("utf-8") + b"\n"
    _write_atomic(path, encoded)


def _write_atomic(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".partial")
    tmp.write_bytes(payload)
    os.replace(tmp, path)


def _strip_leading_slash(path: str) -> str:
    return path[1:] if path.startswith("/") else path


def _safe_relative_path(value: str) -> Path:
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise CohesixError(f"invalid evidence pack relative path: {value}")
    return path


def _is_missing_error(exc: Exception) -> bool:
    message = str(exc).lower()
    return (
        "not found" in message
        or "404" in message
        or "does not exist" in message
        or "is not a file" in message
        or "invalid-path" in message
        or "disabled" in message
    )


def _safe_detail(exc: Exception) -> str:
    detail = str(exc)
    return detail if len(detail) <= 256 else detail[:256]


def _missing_item(path: str, saved_as: str) -> Dict[str, Any]:
    return {
        "path": path,
        "saved_as": saved_as,
        "verb": "CAT",
        "status": "missing",
        "bytes": None,
        "detail": "not-found",
    }


def _error_item(path: str, saved_as: str, verb: str, exc: Exception) -> Dict[str, Any]:
    return {
        "path": path,
        "saved_as": saved_as,
        "verb": verb,
        "status": "error",
        "bytes": None,
        "detail": _safe_detail(exc),
    }


def _parse_kv_line(line: str) -> Dict[str, str]:
    out: Dict[str, str] = {}
    for part in line.split():
        if "=" not in part:
            continue
        key, value = part.split("=", 1)
        out[key] = value
    return out


def _parse_int(value: Optional[str], *, default: int) -> int:
    if value is None:
        return default
    try:
        return int(value)
    except ValueError:
        return default


def _strip_none(payload: Dict[str, Any]) -> Dict[str, Any]:
    return {key: value for key, value in payload.items() if value is not None}
