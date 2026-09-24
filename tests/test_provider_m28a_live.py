# Author: Lukas Bower
# Purpose: Check M28a live preflight binds registered package, immutable request and independent output before effects.
# Copyright 2026 Lukas Bower

"""Pure refusal tests for the live runner; these do not simulate target proof."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts" / "ci"))
import provider_m28a_live as live  # noqa: E402


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fixture(root: Path) -> tuple[dict, str]:
    graph = "a" * 64
    device = "b" * 32
    package = root / "batch-edges"
    package.write_bytes(b"binary")
    inputs = root / "inputs"
    inputs.mkdir()
    pixels = bytes(range(16))
    input_hash = digest(pixels)
    input_path = inputs / input_hash
    input_path.write_bytes(pixels)
    registration = {
        "schema": "cohesix-cuda-registration/v1", "id": "batch-edges",
        "device_uuid": device, "package_sha256": digest(b"binary"),
        "package": str(package), "input_root": str(inputs),
    }
    registration_bytes = json.dumps(registration).encode()
    registration_hash = digest(registration_bytes)
    state = root / "state"
    installed = state / "registrations"
    installed.mkdir(parents=True)
    registration_path = installed / f"{registration_hash}.json"
    registration_path.write_bytes(registration_bytes)
    native = {
        "schema": "cohesix-registered-cuda-request/v1",
        "ticket_id": "ticket-1", "device_uuid": device,
        "provider_graph_sha256": graph,
        "registration_sha256": registration_hash,
        "input_sha256": input_hash,
        "inventory_observed_unix_ms": 1000,
        "memory_budget_bytes": 4096, "deadline_ms": 1000,
        "parameters": {"width": 4, "height": 4, "frames": 1,
                       "iterations": 1},
    }
    workload = {
        "schema": "cohesix-gpu-workload-input/v2",
        "artifact_sha256": "c" * 64,
        "expected_output_sha256": digest(
            live.batch_edges_expected(pixels, 4, 4, 1)),
        "request": native,
    }
    cas_bytes = json.dumps(workload).encode()
    cas_hash = digest(cas_bytes)
    cas = root / f"{cas_hash}.json"
    cas.write_bytes(cas_bytes)
    binding = {
        "schema": "cohesix-job-binding/v1",
        "action": "gpu.workload.submit", "ticket_id": "ticket-1",
        "idempotency_key": "once-1", "policy_sha256": graph,
        "units": 1, "target": "/gpu/GPU-0/workload",
        "input_sha256": cas_hash,
    }
    ticket = {
        "action": "gpu.workload.submit", "id": "ticket-1",
        "idempotency_key": "once-1", "subject_ref": "GPU-0",
        "receipt_worker_id": "worker-gpu-1",
        "receipt_supervisor_generation": 2,
        "receipt_cap_generation": 3, "writer_epoch": 1,
        "args": {"request_sha256": cas_hash, "lease_id": "lease-1"},
    }
    selected = root / "selected.json"
    selected_bytes = json.dumps({"binding": binding, "ticket": ticket}).encode()
    selected.write_bytes(selected_bytes)
    config = {
        "selected_request": str(selected), "selected_sha256": digest(selected_bytes),
        "input_cas": str(cas), "registration": str(registration_path),
        "registration_sha256": registration_hash,
        "package_binary": str(package), "package_sha256": digest(b"binary"),
        "input_file": str(input_path), "executor_state_root": str(state),
        "gpu_device_uuid": device, "cuda_helper_sha256": "c" * 64,
    }
    return config, graph


def test_live_preflight_requires_fresh_pinned_request_and_output(tmp_path):
    config, graph = fixture(tmp_path)
    _, dimensions = live.selected_workload(config, graph, 1001)
    assert dimensions == {"width": 4, "height": 4, "frames": 1,
                          "iterations": 1}
    with pytest.raises(ValueError, match="freshness"):
        live.selected_workload(config, graph, 6000)
    with pytest.raises(ValueError, match="request changed"):
        live.selected_workload({**config, "selected_sha256": "0" * 64},
                               graph, 1001)
    Path(config["package_binary"]).write_bytes(b"unapproved binary")
    with pytest.raises(ValueError, match="enrolled package"):
        live.selected_workload(config, graph, 1001)


def test_recovery_requires_separate_exact_cancel_identity(tmp_path):
    config, graph = fixture(tmp_path)
    original = json.loads(Path(config["selected_request"]).read_text())
    cancellation = {
        "binding": {
            **original["binding"],
            "action": "gpu.workload.cancel",
            "admission_id": "cancel-admit-1",
            "ticket_id": "cancel-ticket-1",
            "idempotency_key": "cancel-once-1",
            "subject": "operator-1",
        },
        "ticket": {
            **original["ticket"],
            "action": "gpu.workload.cancel",
            "id": "cancel-ticket-1",
            "idempotency_key": "cancel-once-1",
            "args": {"job_id": original["binding"]["ticket_id"]},
        },
    }
    cancellation["binding"]["input_sha256"] = digest(
        json.dumps(cancellation["ticket"]["args"], sort_keys=True,
                   separators=(",", ":")).encode())
    original["binding"]["subject"] = "operator-1"
    original["binding"]["admission_id"] = "submit-admit-1"
    path = tmp_path / "cancel.json"
    data = json.dumps(cancellation).encode()
    path.write_bytes(data)
    config["cancel_selected_request"] = str(path)
    config["cancel_selected_sha256"] = digest(data)
    assert live.selected_cancel(config, original, graph) == cancellation
    cancellation["ticket"]["args"]["job_id"] = "some-other-job"
    cancellation["binding"]["input_sha256"] = digest(
        json.dumps(cancellation["ticket"]["args"], sort_keys=True,
                   separators=(",", ":")).encode())
    altered = json.dumps(cancellation).encode()
    path.write_bytes(altered)
    config["cancel_selected_sha256"] = digest(altered)
    with pytest.raises(ValueError, match="original native job"):
        live.selected_cancel(config, original, graph)
    cancellation["ticket"]["args"]["job_id"] = "ticket-1"
    cancellation["ticket"]["receipt_worker_id"] = "another-worker"
    cancellation["binding"]["input_sha256"] = digest(
        json.dumps(cancellation["ticket"]["args"], sort_keys=True,
                   separators=(",", ":")).encode())
    changed = json.dumps(cancellation).encode()
    path.write_bytes(changed)
    config["cancel_selected_sha256"] = digest(changed)
    with pytest.raises(ValueError, match="original native job"):
        live.selected_cancel(config, original, graph)
