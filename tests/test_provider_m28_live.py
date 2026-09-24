# Author: Lukas Bower
# Purpose: Verify M28 live evidence rejects changed native bytes and mismatched job identity.
# Copyright 2026 Lukas Bower

"""Focused live-runner evidence verification without simulating target acceptance."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts" / "ci"))
import provider_m28_live as live  # noqa: E402
from provider_m28_live import native_evidence, observe_terminal  # noqa: E402


def native_fixture(root: Path, action: str) -> tuple[dict, dict, Path]:
    binding = {
        "ticket_id": "ticket-1",
        "idempotency_key": "once-1",
        "action": action,
    }
    observation = (
        {
            "before": {"invocation_id": "old"},
            "after": {
                "invocation_id": "new",
                "active_state": "active",
                "service_result": "success",
            },
            "job": 42,
        }
        if action == "systemd.restart"
        else {"state": "succeeded", "terminal_unix_ms": 1234}
    )
    object_value = {
        "schema": "cohesix-native-provider-evidence/v1",
        "proof_class": "native_provider_operation",
        "authoritative": False,
        "ticket_id": binding["ticket_id"],
        "idempotency_key": binding["idempotency_key"],
        "action": action,
        "provider_graph_sha256": "a" * 64,
        "observation": observation,
    }
    encoded = json.dumps(object_value, separators=(",", ":")).encode()
    digest = hashlib.sha256(encoded).hexdigest()
    path = root / f"{digest}.json"
    path.write_bytes(encoded)
    terminal = {"message": f"native_observation=sha256:{digest}"}
    return binding, terminal, path


@pytest.mark.parametrize("action", ["systemd.restart", "gpu.workload.submit"])
def test_exact_native_object_is_inspectable_and_changed_bytes_refuse(
    tmp_path: Path, action: str
) -> None:
    binding, terminal, path = native_fixture(tmp_path, action)
    result = native_evidence(tmp_path, terminal, binding, "a" * 64)
    assert result["sha256"] == path.stem
    path.write_bytes(path.read_bytes() + b" ")
    with pytest.raises(ValueError, match="digest"):
        native_evidence(tmp_path, terminal, binding, "a" * 64)


def test_native_object_requires_exact_ticket_and_provider_graph(tmp_path: Path) -> None:
    binding, terminal, _ = native_fixture(tmp_path, "systemd.restart")
    with pytest.raises(ValueError, match="correlation"):
        native_evidence(tmp_path, terminal, {**binding, "ticket_id": "other"}, "a" * 64)
    with pytest.raises(ValueError, match="correlation"):
        native_evidence(tmp_path, terminal, binding, "b" * 64)


def test_reconciliation_requires_the_exact_retained_target_line_digest() -> None:
    binding = {"admission_id": "admit-1", "ticket_id": "ticket-1",
               "idempotency_key": "once-1"}
    record = {"binding": binding, "execution": "confirmed",
              "delivery": "acknowledged", "result_sha256": "a" * 64}
    result = {"id": "ticket-1", "idempotency_key": "once-1",
              "admission": {"admission_id": "admit-1"}, "state": "succeeded"}

    class Backend:
        def selected_job_status(self, admission_id: str) -> dict:
            assert admission_id == "admit-1"
            return record

        def reconcile_selected_job(self, admission_id: str) -> dict:
            assert admission_id == "admit-1"
            return {"record": record, "target_results": [result],
                    "target_result_sha256": [self.hash],
                    "effect_replay_allowed": False}

    backend = Backend()
    backend.hash = "b" * 64
    with pytest.raises(ValueError, match="target terminal mismatch"):
        observe_terminal(backend, "admit-1", 1)
    backend.hash = "a" * 64
    assert observe_terminal(backend, "admit-1", 1) == (record, result)


def test_service_request_requires_exact_unit_target_and_input(
    tmp_path: Path,
) -> None:
    args = {"unit": "cohesix-m28-probe.service"}
    binding = {
        "schema": "cohesix-job-binding/v1",
        "action": "systemd.restart",
        "ticket_id": "ticket-1",
        "idempotency_key": "once-1",
        "target": "/host/systemd/cohesix-m28-probe.service/restart",
        "policy_sha256": "a" * 64,
        "units": 1,
        "input_sha256": hashlib.sha256(
            json.dumps(args, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest(),
    }
    ticket = {
        "action": "systemd.restart",
        "id": "ticket-1",
        "idempotency_key": "once-1",
        "target": binding["target"],
        "args": args,
    }
    path = tmp_path / "service.json"
    selected = {"binding": binding, "ticket": ticket}
    path.write_text(json.dumps(selected), encoding="utf-8")
    config = {
        "service_request_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "service_unit": args["unit"],
    }
    assert live.request(path, "systemd.restart", "a" * 64, config) == selected
    selected["ticket"]["target"] = "/host/systemd/other.service/restart"
    path.write_text(json.dumps(selected), encoding="utf-8")
    config["service_request_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    with pytest.raises(ValueError, match="exact service request"):
        live.request(path, "systemd.restart", "a" * 64, config)


def test_gpu_request_checks_native_cas_identity_and_generated_limits(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    contract = {
        "contract": {"gpu_executor": {
            "entrypoints": ["cuda-reference"],
            "maximum_device_allocation_bytes": 4096,
            "maximum_runtime_ms": 1000,
        }}
    }
    monkeypatch.setattr(live, "registry", lambda: contract)
    workload = {
        "schema": "cohesix-gpu-workload-input/v1",
        "artifact_sha256": "c" * 64,
        "expected_output_sha256": "d" * 64,
        "request": {
            "ticket_id": "ticket-2", "device_uuid": "e" * 32,
            "provider_graph_sha256": "a" * 64,
            "entrypoint": "cuda-reference", "memory_budget_bytes": 4096,
            "deadline_ms": 1000,
        },
    }
    encoded = json.dumps(workload).encode()
    cas_hash = hashlib.sha256(encoded).hexdigest()
    cas = tmp_path / f"{cas_hash}.json"
    cas.write_bytes(encoded)
    selected = {"binding": {
        "schema": "cohesix-job-binding/v1",
        "action": "gpu.workload.submit", "ticket_id": "ticket-2",
        "idempotency_key": "once-2", "target": "/gpu/GPU-0/workload",
        "policy_sha256": "a" * 64, "units": 1, "input_sha256": cas_hash,
    }, "ticket": {
        "action": "gpu.workload.submit", "id": "ticket-2",
        "idempotency_key": "once-2", "subject_ref": "GPU-0",
        "args": {"request_sha256": cas_hash},
    }}
    path = tmp_path / "gpu.json"
    path.write_text(json.dumps(selected), encoding="utf-8")
    config = {
        "gpu_request_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "gpu_input_cas": str(cas), "cuda_helper_sha256": "c" * 64,
        "gpu_device_uuid": "e" * 32,
    }
    assert live.request(path, "gpu.workload.submit", "a" * 64, config) == selected
    workload["request"]["memory_budget_bytes"] = 4097
    changed = json.dumps(workload).encode()
    changed_hash = hashlib.sha256(changed).hexdigest()
    changed_cas = tmp_path / f"{changed_hash}.json"
    changed_cas.write_bytes(changed)
    selected["binding"]["input_sha256"] = changed_hash
    selected["ticket"]["args"]["request_sha256"] = changed_hash
    path.write_text(json.dumps(selected), encoding="utf-8")
    config["gpu_request_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    config["gpu_input_cas"] = str(changed_cas)
    with pytest.raises(ValueError, match="resource and runtime bounds"):
        live.request(path, "gpu.workload.submit", "a" * 64, config)


def test_evidence_reader_refuses_symlink_and_oversize(tmp_path: Path) -> None:
    regular = tmp_path / "evidence.json"
    regular.write_bytes(b"12345")
    assert live.read_artifact(regular, 5) == b"12345"
    with pytest.raises(ValueError, match="exceeds bound"):
        live.read_artifact(regular, 4)
    alias = tmp_path / "alias.json"
    alias.symlink_to(regular)
    with pytest.raises(OSError):
        live.read_artifact(alias, 5)
    with pytest.raises(OSError):
        live.digest(alias)


def test_linux_profile_derivation_cannot_hide_source_code_changes() -> None:
    assert live.is_generated_derivation("configs/generated/root_task_resolved.json")
    assert live.is_generated_derivation("apps/root-task/src/generated/bootstrap.rs")
    assert not live.is_generated_derivation("configs/root_task.toml")
    assert not live.is_generated_derivation("apps/hive-gateway/src/main.rs")
    assert not live.is_generated_derivation("docs/BUILD_PLAN.md")
