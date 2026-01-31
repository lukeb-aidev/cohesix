"""High-level Cohesix Python client operations."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Tuple

from .audit import CohesixAudit
from .backends import Backend, TcpBackend
from .defaults import DEFAULTS
from .errors import CohesixError
from .paths import validate_path

_CONSOLE = DEFAULTS.get("console", {})
_TELEMETRY_PUSH = DEFAULTS.get("telemetry_push", {})
MAX_JSON_LEN = int(_CONSOLE.get("max_json_len", 192))
MAX_ECHO_LEN = int(_CONSOLE.get("max_echo_len", 128))
TELEMETRY_RECORD_MAX_BYTES = int(_TELEMETRY_PUSH.get("max_record_bytes", 4096))
TELEMETRY_PUSH_SCHEMA = _TELEMETRY_PUSH.get(
    "schema", "cohsh-telemetry-push/v1"
)

MAX_DIR_LIST_BYTES = 64 * 1024
MAX_GPU_INFO_BYTES = 16 * 1024
MAX_GPU_STATUS_BYTES = 64 * 1024

BREADCRUMB_EVENT_START = "START"
BREADCRUMB_EVENT_EXIT = "EXIT"
BREADCRUMB_STATUS_RUNNING = "RUNNING"
BREADCRUMB_STATUS_OK = "OK"
BREADCRUMB_STATUS_ERR = "ERR"


@dataclass
class GpuLeaseArgs:
    gpu_id: str
    mem_mb: int
    streams: int
    ttl_s: int
    priority: Optional[int] = None
    budget_ttl_s: Optional[int] = None
    budget_ops: Optional[int] = None


class CohesixClient:
    """Thin Cohesix client that mirrors coh host tool semantics."""

    def __init__(self, backend: Backend, defaults: Optional[Dict[str, object]] = None) -> None:
        self.backend = backend
        self.defaults = defaults or DEFAULTS
        self.paths = self.defaults.get("paths", {})
        self.policy = self.defaults.get("coh", {})
        self.retry = self.defaults.get("retry", {})

    def gpu_list(self, audit: Optional[CohesixAudit] = None) -> List[Dict[str, object]]:
        entries = self.backend.list_dir("/gpu")
        if audit is not None:
            audit.push_ack("OK", "LS", "path=/gpu")
        gpus = [entry for entry in entries if entry not in ("models", "telemetry")]
        if not gpus and audit is not None:
            audit.push_line("gpu: none")
            return []
        output = []
        for gpu_id in sorted(gpus):
            info_path = f"/gpu/{gpu_id}/info"
            payload = self.backend.read_file(info_path, MAX_GPU_INFO_BYTES)
            if audit is not None:
                audit.push_ack("OK", "CAT", f"path={info_path}")
            try:
                info = json.loads(payload.decode("utf-8"))
            except Exception as exc:  # pragma: no cover - indicates invalid backend data
                raise CohesixError(f"invalid gpu info JSON in {info_path}") from exc
            output.append(info)
            if audit is not None:
                audit.push_line(
                    "gpu id={id} name={name} mem_mb={memory_mb} sm={sm_count} driver={driver_version} runtime={runtime_version}".format(
                        **info
                    )
                )
        return output

    def gpu_status(self, gpu_id: str, audit: Optional[CohesixAudit] = None) -> str:
        gpu_id = gpu_id.strip()
        if not gpu_id:
            raise CohesixError("gpu id must not be empty")
        status_path = f"/gpu/{gpu_id}/status"
        payload = self.backend.read_file(status_path, MAX_GPU_STATUS_BYTES)
        if audit is not None:
            audit.push_ack("OK", "CAT", f"path={status_path}")
        text = payload.decode("utf-8")
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        return lines[-1] if lines else "EMPTY"

    def gpu_lease(self, args: GpuLeaseArgs, audit: Optional[CohesixAudit] = None) -> None:
        payload = build_spawn_payload("gpu", args)
        payload = normalise_payload(payload, MAX_JSON_LEN)
        path = self.paths.get("queen_ctl", "/queen/ctl")
        written = self.backend.write_append(path, payload.encode("utf-8"))
        if audit is not None:
            audit.push_ack("OK", "ECHO", f"path={path} bytes={written}")
            audit.push_line(
                f"lease requested gpu_id={args.gpu_id} mem_mb={args.mem_mb} streams={args.streams} ttl_s={args.ttl_s}"
            )

    def telemetry_pull(self, out_dir: Path, audit: Optional[CohesixAudit] = None) -> Tuple[int, int, int]:
        telemetry = self.policy.get("telemetry", {})
        root = telemetry.get("root", "/queen/telemetry")
        max_devices = int(telemetry.get("max_devices", 0) or 0)
        max_segments = int(telemetry.get("max_segments_per_device", 0) or 0)
        max_segment_bytes = int(telemetry.get("max_bytes_per_segment", 0) or 0)
        max_total_bytes = int(telemetry.get("max_total_bytes_per_device", 0) or 0)
        out_dir.mkdir(parents=True, exist_ok=True)

        device_entries = self.backend.list_dir(root)
        if audit is not None:
            audit.push_ack("OK", "LS", f"path={root}")
        if max_devices and len(device_entries) > max_devices:
            raise CohesixError(
                f"telemetry devices {len(device_entries)} exceeds max_devices {max_devices}"
            )
        devices = 0
        segments = 0
        bytes_total = 0
        for device_id in sorted(device_entries):
            validate_component(device_id)
            seg_root = f"{root}/{device_id}/seg"
            seg_entries = self.backend.list_dir(seg_root)
            if audit is not None:
                audit.push_ack("OK", "LS", f"path={seg_root}")
            if max_segments and len(seg_entries) > max_segments:
                raise CohesixError(
                    f"telemetry segments {len(seg_entries)} exceeds max_segments_per_device {max_segments} for device {device_id}"
                )
            device_bytes = 0
            for seg_id in sorted(seg_entries):
                validate_component(seg_id)
                seg_path = f"{seg_root}/{seg_id}"
                payload = self.backend.read_file(seg_path, max_segment_bytes or MAX_DIR_LIST_BYTES)
                if audit is not None:
                    audit.push_ack("OK", "CAT", f"path={seg_path}")
                device_bytes += len(payload)
                if max_total_bytes and device_bytes > max_total_bytes:
                    raise CohesixError(
                        f"telemetry bytes {device_bytes} exceeds max_total_bytes_per_device {max_total_bytes} for device {device_id}"
                    )
                relative = Path(device_id) / "seg" / seg_id
                output_path = out_dir / relative
                write_segment(output_path, payload)
                if audit is not None:
                    audit.push_line(
                        f"telemetry device={device_id} segment={seg_id} bytes={len(payload)} saved={relative}"
                    )
                segments += 1
            devices += 1
            bytes_total += device_bytes
        if devices == 0 and audit is not None:
            audit.push_line("telemetry: none")
        return devices, segments, bytes_total

    def telemetry_push(
        self,
        device_id: str,
        payload: str,
        mime: str = "text/plain",
        audit: Optional[CohesixAudit] = None,
    ) -> Dict[str, object]:
        ingest = self.defaults.get("telemetry_ingest", {})
        max_segments = int(ingest.get("max_segments_per_device", 0) or 0)
        max_segment_bytes = int(ingest.get("max_bytes_per_segment", 0) or 0)
        max_total_bytes = int(ingest.get("max_total_bytes_per_device", 0) or 0)
        if max_segments <= 0 or max_segment_bytes <= 0 or max_total_bytes <= 0:
            raise CohesixError("telemetry ingest is disabled in policy")

        device_id = device_id.strip()
        if not device_id:
            raise CohesixError("telemetry device id must not be empty")
        validate_component(device_id)

        seg_root = f"/queen/telemetry/{device_id}/seg"
        existing_segments: List[str] = []
        try:
            existing_segments = self.backend.list_dir(seg_root)
        except Exception:
            existing_segments = []
        if max_segments and len(existing_segments) >= max_segments:
            raise CohesixError(
                f"telemetry segments {len(existing_segments)} exceeds max_segments_per_device {max_segments}"
            )

        ctl_payload = build_telemetry_ctl_payload(mime)
        ctl_path = f"/queen/telemetry/{device_id}/ctl"
        written = self.backend.write_append(ctl_path, ctl_payload.encode("utf-8"))
        if audit is not None:
            audit.push_ack("OK", "ECHO", f"path={ctl_path} bytes={written}")

        latest_path = f"/queen/telemetry/{device_id}/latest"
        latest_bytes = self.backend.read_file(latest_path, MAX_DIR_LIST_BYTES)
        seg_id = last_non_empty_line(latest_bytes)
        if seg_id is None:
            raise CohesixError("telemetry push could not resolve latest segment id")

        records = build_telemetry_records(payload, mime, TELEMETRY_RECORD_MAX_BYTES)
        total_bytes = sum(len(record) for record in records)
        if total_bytes > max_segment_bytes:
            raise CohesixError(
                f"telemetry payload exceeds max_bytes_per_segment {max_segment_bytes}"
            )
        if max_total_bytes:
            current_bytes = 0
            for seg_id in existing_segments:
                seg_path = f"{seg_root}/{seg_id}"
                payload_bytes = self.backend.read_file(seg_path, max_segment_bytes)
                current_bytes += len(payload_bytes)
            if current_bytes + total_bytes > max_total_bytes:
                raise CohesixError(
                    f"telemetry bytes {current_bytes + total_bytes} exceeds max_total_bytes_per_device {max_total_bytes}"
                )
        seg_path = f"/queen/telemetry/{device_id}/seg/{seg_id}"
        for record in records:
            if isinstance(self.backend, TcpBackend) and len(record) > MAX_ECHO_LEN:
                raise CohesixError(
                    f"telemetry record exceeds console echo max {MAX_ECHO_LEN}"
                )
            written = self.backend.write_append(seg_path, record)
            if audit is not None:
                audit.push_ack("OK", "ECHO", f"path={seg_path} bytes={written}")
        if audit is not None:
            audit.push_line(
                f"telemetry push device={device_id} seg_id={seg_id} records={len(records)} bytes={total_bytes}"
            )
        return {
            "device_id": device_id,
            "seg_id": seg_id,
            "records": len(records),
            "bytes": total_bytes,
        }

    def run_command(
        self,
        gpu_id: str,
        command: List[str],
        audit: Optional[CohesixAudit] = None,
    ) -> None:
        gpu_id = gpu_id.strip()
        if not gpu_id:
            raise CohesixError("gpu id must not be empty")
        validate_component(gpu_id)
        if not command or not command[0].strip():
            raise CohesixError("command must not be empty")

        run_policy = self.policy.get("run", {})
        lease_policy = run_policy.get("lease", {})
        breadcrumb_policy = run_policy.get("breadcrumb", {})

        lease_path = f"/gpu/{gpu_id}/lease"
        max_bytes = int(lease_policy.get("max_bytes", 0) or 0)
        lease_bytes = self.backend.read_file(lease_path, max_bytes or MAX_GPU_STATUS_BYTES)
        if audit is not None:
            audit.push_ack("OK", "CAT", f"path={lease_path}")
        lease_line = last_non_empty_line(lease_bytes)
        if lease_line is None:
            raise CohesixError(f"no active lease for gpu {gpu_id}")
        lease_entry = parse_lease_entry(lease_line)
        validate_lease(lease_entry, lease_policy, gpu_id)

        status_path = f"/gpu/{gpu_id}/status"
        command_line = " ".join(command)
        start_line = build_breadcrumb_line(
            breadcrumb_policy,
            BREADCRUMB_EVENT_START,
            BREADCRUMB_STATUS_RUNNING,
            command_line,
            None,
        )
        written = self.backend.write_append(status_path, start_line)
        if audit is not None:
            audit.push_ack("OK", "ECHO", f"path={status_path} bytes={written}")

        try:
            result = subprocess.run(command, check=False)
        except Exception as exc:
            exit_line = build_breadcrumb_line(
                breadcrumb_policy,
                BREADCRUMB_EVENT_EXIT,
                BREADCRUMB_STATUS_ERR,
                command_line,
                None,
            )
            written = self.backend.write_append(status_path, exit_line)
            if audit is not None:
                audit.push_ack("OK", "ECHO", f"path={status_path} bytes={written}")
            raise CohesixError(f"command failed: {exc}") from exc

        status_label = BREADCRUMB_STATUS_OK if result.returncode == 0 else BREADCRUMB_STATUS_ERR
        exit_line = build_breadcrumb_line(
            breadcrumb_policy,
            BREADCRUMB_EVENT_EXIT,
            status_label,
            command_line,
            result.returncode,
        )
        written = self.backend.write_append(status_path, exit_line)
        if audit is not None:
            audit.push_ack("OK", "ECHO", f"path={status_path} bytes={written}")
        if result.returncode != 0:
            raise CohesixError(f"command exited with code {result.returncode}")

    def queen_kill(self, worker_id: str, audit: Optional[CohesixAudit] = None) -> None:
        worker_id = worker_id.strip()
        if not worker_id:
            raise CohesixError("worker id must not be empty")
        validate_component(worker_id)
        payload = json.dumps({"kill": worker_id}, separators=(",", ":"))
        payload = normalise_payload(payload, MAX_JSON_LEN)
        path = self.paths.get("queen_ctl", "/queen/ctl")
        written = self.backend.write_append(path, payload.encode("utf-8"))
        if audit is not None:
            audit.push_ack("OK", "ECHO", f"path={path} bytes={written}")
            audit.push_line(f"kill requested worker_id={worker_id}")

    def peft_export(
        self, job_id: str, out_dir: Path, audit: Optional[CohesixAudit] = None
    ) -> None:
        validate_component(job_id)
        export_policy = self.policy.get("peft", {}).get("export", {})
        root = export_policy.get("root", "/queen/export/lora_jobs")
        job_root = f"{root}/{job_id}"
        out_dir.mkdir(parents=True, exist_ok=True)

        entries = self.backend.list_dir(root)
        if audit is not None:
            audit.push_ack("OK", "LS", f"path={root}")
        if job_id not in entries:
            raise CohesixError(f"missing export job {job_id}")
        job_entries = self.backend.list_dir(job_root)
        if audit is not None:
            audit.push_ack("OK", "LS", f"path={job_root}")

        for name, max_bytes in (
            ("telemetry.cbor", int(export_policy.get("max_telemetry_bytes", 0) or 0)),
            ("base_model.ref", int(export_policy.get("max_base_model_bytes", 0) or 0)),
            ("policy.toml", int(export_policy.get("max_policy_bytes", 0) or 0)),
        ):
            path = f"{job_root}/{name}"
            payload = self.backend.read_file(path, max_bytes or MAX_GPU_STATUS_BYTES)
            if audit is not None:
                audit.push_ack("OK", "CAT", f"path={path}")
            target = out_dir / job_id / name
            write_segment(target, payload)

    def peft_import(
        self,
        model_id: str,
        adapter_dir: Path,
        export_root: Path,
        job_id: str,
        registry_root: Path,
        audit: Optional[CohesixAudit] = None,
    ) -> None:
        validate_component(model_id)
        validate_component(job_id)
        peft_policy = self.policy.get("peft", {})
        export_policy = peft_policy.get("export", {})
        import_policy = peft_policy.get("import", {})
        activate_policy = peft_policy.get("activate", {})

        enforce_id_bytes(model_id, int(activate_policy.get("max_model_id_bytes", 0) or 0))
        enforce_id_bytes(job_id, int(activate_policy.get("max_model_id_bytes", 0) or 0))

        adapter_dir = adapter_dir
        if not adapter_dir.is_dir():
            raise CohesixError(f"adapter directory {adapter_dir} does not exist")
        export_dir = export_root / job_id
        if not export_dir.is_dir():
            raise CohesixError(f"export job directory {export_dir} does not exist")

        base_model = read_single_line(export_dir / "base_model.ref", int(export_policy.get("max_base_model_bytes", 0) or 0))
        policy_hash = hash_file(export_dir / "policy.toml", int(export_policy.get("max_policy_bytes", 0) or 0))
        telemetry_hash = hash_file(export_dir / "telemetry.cbor", int(export_policy.get("max_telemetry_bytes", 0) or 0))

        adapter_path = adapter_dir / "adapter.safetensors"
        lora_path = adapter_dir / "lora.json"
        if not adapter_path.is_file():
            raise CohesixError(f"missing adapter file {adapter_path}")
        if not lora_path.is_file():
            raise CohesixError(f"missing lora metadata file {lora_path}")

        target_dir = registry_root / "available" / model_id
        if (target_dir / "manifest.toml").exists():
            raise CohesixError(f"model {model_id} already imported")
        target_dir.mkdir(parents=True, exist_ok=True)

        adapter_hash = copy_with_hash(adapter_path, target_dir / "adapter.safetensors", int(import_policy.get("max_adapter_bytes", 0) or 0))
        lora_hash = copy_with_hash(lora_path, target_dir / "lora.json", int(import_policy.get("max_lora_bytes", 0) or 0))

        metrics_path = adapter_dir / "metrics.json"
        metrics_hash = None
        if metrics_path.is_file():
            metrics_hash = copy_with_hash(metrics_path, target_dir / "metrics.json", int(import_policy.get("max_metrics_bytes", 0) or 0))

        manifest = render_manifest(
            model_id,
            base_model,
            job_id,
            adapter_hash,
            lora_hash,
            metrics_hash,
            policy_hash,
            telemetry_hash,
        )
        max_manifest_bytes = int(import_policy.get("max_manifest_bytes", 0) or 0)
        if max_manifest_bytes and len(manifest.encode("utf-8")) > max_manifest_bytes:
            raise CohesixError(
                f"manifest bytes {len(manifest.encode('utf-8'))} exceeds max_manifest_bytes {max_manifest_bytes}"
            )
        write_atomic(target_dir / "manifest.toml", manifest.encode("utf-8"))
        if audit is not None:
            audit.push_line(f"peft import model={model_id} adapter_bytes={adapter_hash['bytes']}")

    def peft_activate(
        self, model_id: str, registry_root: Path, audit: Optional[CohesixAudit] = None
    ) -> None:
        validate_component(model_id)
        activate_policy = self.policy.get("peft", {}).get("activate", {})
        enforce_id_bytes(model_id, int(activate_policy.get("max_model_id_bytes", 0) or 0))

        manifest_path = registry_root / "available" / model_id / "manifest.toml"
        if not manifest_path.is_file():
            raise CohesixError(f"model {model_id} is not available")

        state = load_state(registry_root, activate_policy)
        previous = state.get("current") or None
        state["previous"] = previous
        state["current"] = model_id

        write_atomic(registry_root / "active", f"{model_id}\n".encode("utf-8"))
        write_state(registry_root, activate_policy, state)

        path = "/gpu/models/active"
        payload = f"{model_id}\n".encode("utf-8")
        written = self.backend.write_append(path, payload)
        if audit is not None:
            audit.push_ack("OK", "ECHO", f"path={path} bytes={written}")
            audit.push_line(f"peft activated model={model_id}")

    def peft_rollback(
        self, registry_root: Path, audit: Optional[CohesixAudit] = None
    ) -> None:
        activate_policy = self.policy.get("peft", {}).get("activate", {})
        state = load_state(registry_root, activate_policy)
        previous = state.get("previous")
        if not previous:
            raise CohesixError("no previous model available for rollback")
        manifest_path = registry_root / "available" / previous / "manifest.toml"
        if not manifest_path.is_file():
            raise CohesixError(f"model {previous} is not available")

        current = state.get("current")
        state["current"] = previous
        state["previous"] = current
        write_atomic(registry_root / "active", f"{previous}\n".encode("utf-8"))
        write_state(registry_root, activate_policy, state)

        path = "/gpu/models/active"
        payload = f"{previous}\n".encode("utf-8")
        written = self.backend.write_append(path, payload)
        if audit is not None:
            audit.push_ack("OK", "ECHO", f"path={path} bytes={written}")
            audit.push_line(f"peft rollback from={current} to={previous}")


# Helper functions


def validate_component(component: str) -> None:
    if not component:
        raise CohesixError("path component must not be empty")
    if component in (".", ".."):
        raise CohesixError(f"path component '{component}' is not permitted")
    if "/" in component:
        raise CohesixError(f"path component '{component}' contains '/'")
    if "\x00" in component:
        raise CohesixError("path component contains NUL byte")


def normalise_payload(payload: str, max_bytes: Optional[int] = None) -> str:
    trimmed = payload.strip()
    if not trimmed:
        raise CohesixError("payload must not be empty")
    if trimmed.startswith("\"") and trimmed.endswith("\"") and len(trimmed) >= 2:
        trimmed = trimmed[1:-1]
    if trimmed.startswith("'") and trimmed.endswith("'") and len(trimmed) >= 2:
        trimmed = trimmed[1:-1]
    if any(ch in trimmed for ch in ("\n", "\r", "\x00")):
        raise CohesixError("payload must be a single line of text")
    if max_bytes is not None and len(trimmed.encode("utf-8")) > max_bytes:
        raise CohesixError(f"payload exceeds {max_bytes} bytes")
    if not trimmed.endswith("\n"):
        trimmed += "\n"
    return trimmed


def build_spawn_payload(role: str, args: GpuLeaseArgs) -> str:
    if role.lower() not in ("gpu", "worker-gpu"):
        raise CohesixError("unsupported spawn role")
    if not args.gpu_id:
        raise CohesixError("gpu_id required")
    payload = {
        "spawn": "gpu",
        "lease": {
            "gpu_id": args.gpu_id,
            "mem_mb": args.mem_mb,
            "streams": args.streams,
            "ttl_s": args.ttl_s,
        },
    }
    if args.priority is not None:
        payload["lease"]["priority"] = args.priority
    if args.budget_ttl_s is not None or args.budget_ops is not None:
        budget: Dict[str, int] = {}
        if args.budget_ttl_s is not None:
            budget["ttl_s"] = int(args.budget_ttl_s)
        if args.budget_ops is not None:
            budget["ops"] = int(args.budget_ops)
        payload["budget"] = budget
    return json.dumps(payload, separators=(",", ":"))


def build_telemetry_ctl_payload(mime: str) -> str:
    command = {"new": "segment", "mime": mime}
    payload = json.dumps(command, separators=(",", ":"))
    return payload + "\n"


def build_telemetry_records(payload: str, mime: str, max_record_bytes: int) -> List[bytes]:
    if not payload:
        raise CohesixError("telemetry payload is empty")
    remaining = payload
    seq = 1
    records: List[bytes] = []
    while remaining:
        payload_len = select_telemetry_payload_len(
            remaining, seq, mime, max_record_bytes
        )
        if payload_len == 0:
            raise CohesixError(
                f"telemetry record exceeds max_record_bytes {max_record_bytes}"
            )
        chunk = truncate_to_bytes(remaining, payload_len)
        record = build_telemetry_record(seq, mime, chunk)
        if len(record) > max_record_bytes:
            raise CohesixError(
                f"telemetry record exceeds max_record_bytes {max_record_bytes}"
            )
        records.append(record)
        remaining = remaining[len(chunk) :]
        seq += 1
    return records


def select_telemetry_payload_len(
    remaining: str, seq: int, mime: str, max_record_bytes: int
) -> int:
    low = 0
    high = len(remaining.encode("utf-8"))
    while low < high:
        mid = (low + high + 1) // 2
        candidate = truncate_to_bytes(remaining, mid)
        record = build_telemetry_record(seq, mime, candidate)
        if len(record) <= max_record_bytes:
            low = mid
        else:
            high = mid - 1
    return low


def build_telemetry_record(seq: int, mime: str, payload: str) -> bytes:
    envelope = {
        "schema": TELEMETRY_PUSH_SCHEMA,
        "seq": int(seq),
        "mime": mime,
        "payload": payload,
    }
    encoded = json.dumps(envelope, separators=(",", ":"))
    return (encoded + "\n").encode("utf-8")


def write_segment(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        return
    tmp_path = path.with_suffix(path.suffix + ".partial")
    tmp_path.write_bytes(payload)
    os.replace(tmp_path, path)


def last_non_empty_line(payload: bytes) -> Optional[str]:
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise CohesixError("lease file is not UTF-8") from exc
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    return lines[-1] if lines else None


def parse_lease_entry(line: str) -> Dict[str, object]:
    try:
        return json.loads(line)
    except Exception as exc:
        raise CohesixError("invalid lease JSON") from exc


def validate_lease(entry: Dict[str, object], lease_policy: Dict[str, object], gpu_id: str) -> None:
    schema = lease_policy.get("schema")
    active_state = lease_policy.get("active_state")
    if entry.get("schema") != schema:
        raise CohesixError(f"lease schema mismatch: expected {schema} got {entry.get('schema')}")
    if entry.get("state") != active_state:
        raise CohesixError(f"no active lease for gpu {gpu_id}")
    if entry.get("gpu_id") != gpu_id:
        raise CohesixError(
            f"lease gpu_id mismatch: expected {gpu_id} got {entry.get('gpu_id')}"
        )
    if not entry.get("worker_id"):
        raise CohesixError("lease worker_id must not be empty")


def build_breadcrumb_line(
    policy: Dict[str, object],
    event: str,
    status: str,
    command: str,
    exit_code: Optional[int],
) -> bytes:
    max_line_bytes = int(policy.get("max_line_bytes", 0) or 0)
    max_command_bytes = int(policy.get("max_command_bytes", 0) or 0)
    if max_command_bytes <= 0:
        max_command_bytes = len(command)
    cmd_limit = min(max_command_bytes, len(command))
    while True:
        trimmed = truncate_to_bytes(command, cmd_limit)
        entry = {
            "schema": policy.get("schema"),
            "event": event,
            "command": trimmed,
            "status": status,
        }
        if exit_code is not None:
            entry["exit_code"] = int(exit_code)
        json_payload = json.dumps(entry, separators=(",", ":"))
        if not max_line_bytes or len(json_payload) <= max_line_bytes:
            return (json_payload + "\n").encode("utf-8")
        if cmd_limit == 0:
            raise CohesixError(
                f"breadcrumb line exceeds max_line_bytes {max_line_bytes}"
            )
        cmd_limit -= 1


def truncate_to_bytes(text: str, max_bytes: int) -> str:
    if len(text.encode("utf-8")) <= max_bytes:
        return text
    out = ""
    count = 0
    for ch in text:
        encoded = ch.encode("utf-8")
        if count + len(encoded) > max_bytes:
            break
        out += ch
        count += len(encoded)
    return out


def hash_file(path: Path, max_bytes: int) -> Dict[str, object]:
    if max_bytes <= 0:
        max_bytes = 1 << 30
    hasher = hashlib.sha256()
    total = 0
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(8192)
            if not chunk:
                break
            total += len(chunk)
            if total > max_bytes:
                raise CohesixError(f"{path} exceeds max bytes {max_bytes}")
            hasher.update(chunk)
    if total == 0:
        raise CohesixError(f"{path} is empty")
    return {"sha256": hasher.hexdigest(), "bytes": total}


def copy_with_hash(src: Path, dest: Path, max_bytes: int) -> Dict[str, object]:
    if max_bytes <= 0:
        max_bytes = 1 << 30
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".partial")
    hasher = hashlib.sha256()
    total = 0
    with open(src, "rb") as reader, open(tmp, "wb") as writer:
        while True:
            chunk = reader.read(8192)
            if not chunk:
                break
            total += len(chunk)
            if total > max_bytes:
                raise CohesixError(f"{src} exceeds max bytes {max_bytes}")
            hasher.update(chunk)
            writer.write(chunk)
    if total == 0:
        raise CohesixError(f"{src} is empty")
    os.replace(tmp, dest)
    return {"sha256": hasher.hexdigest(), "bytes": total}


def read_single_line(path: Path, max_bytes: int) -> str:
    data = path.read_bytes()
    if max_bytes and len(data) > max_bytes:
        raise CohesixError(f"{path} exceeds max bytes {max_bytes}")
    text = data.decode("utf-8")
    for line in text.splitlines():
        line = line.strip()
        if line:
            return line
    raise CohesixError(f"{path} is empty")


def render_manifest(
    model_id: str,
    base_model: str,
    job_id: str,
    adapter: Dict[str, object],
    lora: Dict[str, object],
    metrics: Optional[Dict[str, object]],
    policy_hash: Dict[str, object],
    telemetry_hash: Dict[str, object],
) -> str:
    out = []
    out.append("[model]")
    out.append(f"id = \"{model_id}\"")
    out.append(f"base = \"{base_model}\"")
    out.append('adapter = "adapter.safetensors"')
    out.append('lora = "lora.json"')
    if metrics is not None:
        out.append('metrics = "metrics.json"')
    out.append("")
    out.append("[provenance]")
    out.append(f"job_id = \"{job_id}\"")
    out.append("approval = \"pending\"")
    out.append("")
    out.append("[hashes]")
    out.append(f"adapter_sha256 = \"{adapter['sha256']}\"")
    out.append(f"adapter_bytes = {adapter['bytes']}")
    out.append(f"lora_sha256 = \"{lora['sha256']}\"")
    out.append(f"lora_bytes = {lora['bytes']}")
    if metrics is not None:
        out.append(f"metrics_sha256 = \"{metrics['sha256']}\"")
        out.append(f"metrics_bytes = {metrics['bytes']}")
    out.append(f"policy_sha256 = \"{policy_hash['sha256']}\"")
    out.append(f"policy_bytes = {policy_hash['bytes']}")
    out.append(f"telemetry_sha256 = \"{telemetry_hash['sha256']}\"")
    out.append(f"telemetry_bytes = {telemetry_hash['bytes']}")
    return "\n".join(out) + "\n"


def write_atomic(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".partial")
    tmp.write_bytes(payload)
    os.replace(tmp, path)


def load_state(root: Path, policy: Dict[str, object]) -> Dict[str, Optional[str]]:
    state_path = root / "active_state.toml"
    if not state_path.is_file():
        current = read_active_pointer(root, policy)
        return {"current": current, "previous": None}
    max_state_bytes = int(policy.get("max_state_bytes", 0) or 0)
    payload = state_path.read_bytes()
    if max_state_bytes and len(payload) > max_state_bytes:
        raise CohesixError(
            f"state bytes {len(payload)} exceeds max_state_bytes {max_state_bytes}"
        )
    text = payload.decode("utf-8")
    current = None
    previous = None
    for line in text.splitlines():
        if line.strip().startswith("current"):
            current = _parse_toml_string(line)
        if line.strip().startswith("previous"):
            previous = _parse_toml_string(line)
    return {"current": current, "previous": previous}


def _parse_toml_string(line: str) -> Optional[str]:
    if "=" not in line:
        return None
    _, value = line.split("=", 1)
    value = value.strip().strip("\"")
    return value if value else None


def write_state(root: Path, policy: Dict[str, object], state: Dict[str, Optional[str]]) -> None:
    payload = []
    current = state.get("current") or ""
    previous = state.get("previous")
    payload.append(f"current = \"{current}\"")
    if previous is None:
        payload.append("previous = \"\"")
    else:
        payload.append(f"previous = \"{previous}\"")
    data = "\n".join(payload) + "\n"
    max_state_bytes = int(policy.get("max_state_bytes", 0) or 0)
    if max_state_bytes and len(data.encode("utf-8")) > max_state_bytes:
        raise CohesixError(
            f"state bytes {len(data.encode('utf-8'))} exceeds max_state_bytes {max_state_bytes}"
        )
    write_atomic(root / "active_state.toml", data.encode("utf-8"))


def read_active_pointer(root: Path, policy: Dict[str, object]) -> str:
    path = root / "active"
    if not path.is_file():
        return ""
    max_bytes = int(policy.get("max_model_id_bytes", 0) or 0)
    payload = path.read_bytes()
    if max_bytes and len(payload) > max_bytes + 1:
        raise CohesixError(
            f"active pointer bytes {len(payload)} exceeds max_model_id_bytes {max_bytes}"
        )
    text = payload.decode("utf-8")
    for line in text.splitlines():
        line = line.strip()
        if line:
            return line
    return ""


def enforce_id_bytes(value: str, max_bytes: int) -> None:
    if max_bytes and len(value.encode("utf-8")) > max_bytes:
        raise CohesixError(f"id length exceeds max {max_bytes}")
