# Author: Lukas Bower
# Purpose: Verify workload control bounds and caller/root identity separation before backend writes.
# Copyright 2026 Lukas Bower

import copy
import hashlib
import importlib.util
import json
from argparse import Namespace
from pathlib import Path
import pytest
from cohesix.errors import CohesixError
from cohesix.workload import (batch_edges_expected, enqueue, recovery_guidance,
                              verify_batch_edges)


class Backend:
    def __init__(self):
        self.writes = []

    def write_append(self, path, data):
        self.writes.append((path, data))
        return len(data)


def test_workload_submission_cannot_invent_admission_or_provider_result():
    spec = {"schema": "host-ticket/v2", "id": "job", "idempotency_key": "once",
            "writer_epoch": 1, "action": "gpu.workload.submit",
            "args": {"lease_id": "lease-1", "request_sha256": "a" * 64},
            "expires_unix_ms": 1000, "receipt_mode": "worker",
            "operation_id": "work", "subject_ref": "GPU-0",
            "receipt_worker_role": "worker-gpu", "receipt_worker_id": "gpu-worker-1",
            "receipt_supervisor_generation": 2, "receipt_cap_generation": 3}
    backend = Backend()
    result = enqueue(backend, spec)
    assert result["authoritative"] is False
    assert result["execution"] == "unverified"
    assert backend.writes[0][0] == "/host/tickets/spec"
    for extra in ({"resolved_lease_epoch": 1}, {"admission_sequence": 2},
                  {"action": "gpu.lease.grant"}, {"writer_epoch": True},
                  {"args": {"lease_id": "../lease", "request_sha256": "a" * 64}},
                  {"args": {"job_id": "job", "command": "echo ok"}}):
        bad = copy.deepcopy(spec)
        bad.update(extra)
        with pytest.raises(CohesixError):
            enqueue(backend, bad)
    assert len(backend.writes) == 1


def test_batch_edges_independent_verifier_checks_all_pixels(tmp_path):
    width, height, frames = 8, 7, 2
    data = bytes((index * 17 + index // width) % 256
                 for index in range(width * height * frames))
    expected = batch_edges_expected(data, width, height, frames)
    source, output = tmp_path / "source.bin", tmp_path / "output.bin"
    source.write_bytes(data)
    output.write_bytes(expected)
    report = verify_batch_edges(source, output, width, height, frames)
    assert report["pixels_checked"] == width * height * frames
    assert report["maximum_absolute_error"] == report["tolerance"] == 0
    altered = bytearray(expected)
    altered[width + 1] ^= 1
    output.write_bytes(altered)
    with pytest.raises(CohesixError, match="output_mismatch"):
        verify_batch_edges(source, output, width, height, frames)
    for values in ((0, height, frames), (width, 2, frames),
                   (width, height, 5), (width, height, True)):
        with pytest.raises(CohesixError, match="ELIMIT"):
            batch_edges_expected(data, *values)
    source.unlink()
    source.symlink_to(output)
    with pytest.raises(OSError):
        verify_batch_edges(source, output, width, height, frames)


def test_example_prepares_immutable_reference_and_independent_adaptation(tmp_path):
    example = Path(__file__).resolve().parents[1] / "examples/cuda_batch_edges.py"
    spec = importlib.util.spec_from_file_location("cuda_batch_edges_example", example)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    package = tmp_path / "batch-edges"
    package.write_bytes(b"reviewed executable")
    root = tmp_path / "inputs"
    root.mkdir()
    for name, width, height in (("reference", 8, 8), ("adaptation", 16, 16)):
        data = bytes((index * 17 + width) % 256
                     for index in range(width * height))
        source = tmp_path / f"{name}.bin"
        source.write_bytes(data)
        registration = tmp_path / f"{name}-registration.json"
        request = tmp_path / f"{name}-request.json"
        args = Namespace(
            device_uuid="a" * 32, cuda_helper_sha256="b" * 64,
            topology_sha256="c" * 64, provider_graph_sha256="d" * 64,
            ticket_id=f"ticket-{name}", package=package, input=source,
            input_root=root, registration_out=registration,
            request_out=request, inventory_observed_unix_ms=1000,
            iterations=2, width=width, height=height, frames=1,
        )
        module.prepare(args)
        record = json.loads(registration.read_bytes())
        prepared = json.loads(request.read_bytes())
        assert record["package_sha256"] == hashlib.sha256(
            package.read_bytes()).hexdigest()
        assert prepared["request"]["registration_sha256"] == hashlib.sha256(
            registration.read_bytes()).hexdigest()
        assert prepared["request"]["input_sha256"] == hashlib.sha256(data).hexdigest()
        assert prepared["expected_output_sha256"] == hashlib.sha256(
            batch_edges_expected(data, width, height, 1)).hexdigest()
        assert (root / prepared["request"]["input_sha256"]).read_bytes() == data
        with pytest.raises(FileExistsError):
            module.prepare(args)


def test_recovery_guidance_preserves_uncertainty_and_pending_delivery():
    uncertain = recovery_guidance({"execution": "uncertain", "delivery": "pending"})
    assert "original admission" in uncertain["next_step"]
    assert "capacity reserved" in uncertain["next_step"]
    assert "new authorization" in uncertain["restart"]
    delivered = recovery_guidance({"execution": "confirmed", "delivery": "pending"})
    assert "pending result delivery" in delivered["next_step"]
    for value in ({"execution": "succeeded", "delivery": "pending"},
                  {"execution": "confirmed", "delivery": "unknown"}):
        with pytest.raises(CohesixError):
            recovery_guidance(value)
