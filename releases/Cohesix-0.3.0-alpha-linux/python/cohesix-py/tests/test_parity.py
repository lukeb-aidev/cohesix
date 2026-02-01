"""Parity tests for the Cohesix Python client."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.audit import CohesixAudit
from cohesix.backends import MockBackend
from cohesix.client import CohesixClient, GpuLeaseArgs
from cohesix.ticket import TicketError, normalize_ticket


def repo_root() -> Path:
    path = Path(__file__).resolve()
    for parent in path.parents:
        if (parent / "tests" / "fixtures" / "transcripts").is_dir():
            return parent
    raise RuntimeError("repo root not found")


def load_fixture(scenario: str, name: str) -> list[str]:
    path = repo_root() / "tests" / "fixtures" / "transcripts" / scenario / name
    return [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def normalize_lines(lines: list[str]) -> list[str]:
    out: list[str] = []
    for line in lines:
        if line == "END":
            out.append(line)
            continue
        if line.startswith("OK AUTH") or line.startswith("ERR AUTH"):
            continue
        if line.startswith("OK ") or line.startswith("ERR "):
            out.append(line)
    return out


def lease_entry(state: str) -> str:
    return (
        f"{{\"schema\":\"gpu-lease/v1\",\"state\":\"{state}\",\"gpu_id\":\"GPU-0\","
        f"\"worker_id\":\"worker-1\",\"mem_mb\":1024,\"streams\":1,\"ttl_s\":60,\"priority\":1}}\n"
    )


def test_cohesix_parity_converge() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        client = CohesixClient(backend)
        audit = CohesixAudit()

        client.gpu_list(audit)
        lease_args = GpuLeaseArgs(
            gpu_id="GPU-0",
            mem_mb=4096,
            streams=2,
            ttl_s=120,
            priority=1,
        )
        client.gpu_lease(lease_args, audit)
        client.telemetry_pull(Path(tmp) / "telemetry", audit)

        expected = load_fixture("converge_v0", "coh.txt")
        assert normalize_lines(audit.lines) == expected


def test_cohesix_parity_run_demo() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        client = CohesixClient(backend)
        audit = CohesixAudit()

        lease_path = "/gpu/GPU-0/lease"
        payload = lease_entry("ACTIVE").encode("utf-8")
        written = backend.write_append(lease_path, payload)
        audit.push_ack("OK", "ECHO", f"path={lease_path} bytes={written}")

        client.run_command("GPU-0", ["echo", "ok"], audit)

        status_path = "/gpu/GPU-0/status"
        _ = backend.read_file(status_path, 65536)
        audit.push_ack("OK", "CAT", f"path={status_path}")

        payload = lease_entry("RELEASED").encode("utf-8")
        written = backend.write_append(lease_path, payload)
        audit.push_ack("OK", "ECHO", f"path={lease_path} bytes={written}")

        expected = load_fixture("run_demo_v0", "cohsh.txt")
        assert normalize_lines(audit.lines) == expected


def test_cohesix_parity_peft_roundtrip() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        client = CohesixClient(backend)
        audit = CohesixAudit()

        export_out = Path(tmp) / "export"
        adapter_dir = Path(tmp) / "adapter"
        registry_root = Path(tmp) / "registry"
        adapter_dir.mkdir(parents=True, exist_ok=True)
        registry_root.mkdir(parents=True, exist_ok=True)

        (adapter_dir / "adapter.safetensors").write_bytes(b"adapter-bytes")
        (adapter_dir / "lora.json").write_bytes(b"{\"rank\":8}")

        client.peft_export("job_8932", export_out, audit)
        model_id = "llama3-edge-v7"
        previous_model_id = "llama3-edge-v6"

        client.peft_import(
            model_id=model_id,
            adapter_dir=adapter_dir,
            export_root=export_out,
            job_id="job_8932",
            registry_root=registry_root,
            audit=audit,
        )
        client.peft_import(
            model_id=previous_model_id,
            adapter_dir=adapter_dir,
            export_root=export_out,
            job_id="job_8932",
            registry_root=registry_root,
            audit=None,
        )
        client.peft_activate(previous_model_id, registry_root, None)

        manifest_path = f"/gpu/models/available/{model_id}/manifest.toml"
        _ = backend.read_file(manifest_path, 8192)
        audit.push_ack("OK", "CAT", f"path={manifest_path}")

        client.peft_activate(model_id, registry_root, audit)
        client.peft_rollback(registry_root, audit)

        expected = load_fixture("peft_roundtrip_v0", "cohsh.txt")
        assert normalize_lines(audit.lines) == expected


def test_invalid_ticket_rejected() -> None:
    try:
        normalize_ticket("worker-gpu", "invalid-ticket", queen_validate=True)
    except TicketError as exc:
        assert "ticket" in str(exc)
    else:  # pragma: no cover
        raise AssertionError("invalid ticket was accepted")
