# Author: Lukas Bower
# Purpose: Verify workload control bounds and caller/root identity separation before backend writes.
# Copyright 2026 Lukas Bower

import copy
import pytest
from cohesix.errors import CohesixError
from cohesix.workload import enqueue


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
