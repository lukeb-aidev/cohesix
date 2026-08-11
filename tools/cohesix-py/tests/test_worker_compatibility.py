# Author: Lukas Bower
# Purpose: Verify Worker lifecycle projections across every Cohesix Python backend class.
# Copyright 2026 Lukas Bower

"""Backend-parity and refusal tests for the non-authoritative Worker API."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Callable

import pytest

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = PACKAGE_ROOT.parents[1]
sys.path.insert(0, str(PACKAGE_ROOT))

from cohesix.backends import (  # noqa: E402
    Backend,
    FilesystemBackend,
    MockBackend,
    RestBackend,
    TcpBackend,
)
from cohesix.client import CohesixClient  # noqa: E402
from cohesix.errors import CohesixError  # noqa: E402
from cohesix.evidence import worker_acceptance_axes  # noqa: E402
from cohesix.worker import (  # noqa: E402
    TargetProfileContract,
    WorkerIdentity,
    load_profile_contract,
)

QEMU_CONTRACT = (
    REPO_ROOT / "configs/generated/cohesix_python_qemu_smp_production.json"
)
ROLE_CASES = (
    ("heartbeat", "worker-heartbeat", "heartbeat-1"),
    ("gpu", "worker-gpu", "gpu-1"),
    ("lora", "worker-lora", "lora-1"),
)


def _observation(role: str, worker_id: str, generation: int = 1) -> bytes:
    value = {
        "schema": "cohesix-worker-observation/v1",
        "public_instance_id": worker_id,
        "identity": {
            "role": role,
            "slot": 0,
            "lease_epoch": 1,
            "supervisor_generation": generation,
            "cap_generation": 1,
        },
        "state": {
            "declaration": "executable",
            "lifecycle": "ready",
            "artifact": "missing",
            "receipt": "none",
            "execution_proof": "none",
        },
        "request_admitted": True,
        "provider_completed": False,
        "receipt_sequence": 0,
    }
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def _seed_projection(
    backend: FilesystemBackend, contract: TargetProfileContract
) -> None:
    for _alias, role, worker_id in ROLE_CASES:
        path = Path(backend.root + contract.telemetry_path(worker_id))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(_observation(role, worker_id) + b"\n")


class _TransportFixture:
    """Use one filesystem fixture behind concrete TCP/REST backend classes."""

    fixture: FilesystemBackend

    def list_dir(self, path: str) -> list[str]:
        return self.fixture.list_dir(path)

    def read_file(self, path: str, max_bytes: int) -> bytes:
        return self.fixture.read_file(path, max_bytes)

    def tail_file(self, path: str, max_bytes: int) -> bytes:
        return self.fixture.tail_file(path, max_bytes)

    def write_append(self, path: str, payload: bytes) -> int:
        return self.fixture.write_append(path, payload)


class FixtureTcpBackend(_TransportFixture, TcpBackend):
    """TCP class fixture preserving the same namespace payloads."""

    def __init__(self, root: Path) -> None:
        self.fixture = FilesystemBackend(str(root))


class FixtureRestBackend(_TransportFixture, RestBackend):
    """REST class fixture preserving the same namespace payloads."""

    def __init__(self, root: Path) -> None:
        self.fixture = FilesystemBackend(str(root))


def _filesystem(root: Path) -> Backend:
    return FilesystemBackend(str(root))


def _mock(root: Path) -> Backend:
    return MockBackend(str(root))


def _tcp(root: Path) -> Backend:
    return FixtureTcpBackend(root)


def _rest(root: Path) -> Backend:
    return FixtureRestBackend(root)


@pytest.mark.parametrize(
    ("backend_name", "factory"),
    (
        ("mock", _mock),
        ("filesystem", _filesystem),
        ("tcp", _tcp),
        ("rest", _rest),
    ),
)
def test_worker_lifecycle_is_backend_parity(
    tmp_path: Path,
    backend_name: str,
    factory: Callable[[Path], Backend],
) -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    backend = factory(tmp_path / backend_name)
    if not isinstance(backend, MockBackend):
        fixture = backend.fixture if isinstance(backend, _TransportFixture) else backend
        assert isinstance(fixture, FilesystemBackend)
        _seed_projection(fixture, contract)
    client = CohesixClient(backend, profile_contract=contract)

    for alias, canonical_role, worker_id in ROLE_CASES:
        admitted = client.worker_spawn(alias, worker_id)
        assert admitted.request_admitted
        assert admitted.lifecycle == "queued"

        ready = client.worker_wait_ready(alias, worker_id, timeout_s=0.2)
        assert ready.identity is not None
        assert ready.identity.role == canonical_role
        assert ready.state.lifecycle == "ready"
        expected_proof = "host-model" if backend_name == "mock" else "none"
        assert ready.state.execution_proof == expected_proof

        axes = worker_acceptance_axes(ready)
        assert axes.request_admitted
        assert axes.worker_ready
        assert not axes.provider_completed
        assert axes.worker_receipt == "none"
        assert axes.artifact == "missing"
        assert axes.python_projection_compatible
        assert not axes.runtime_release_accepted
        assert not axes.production_use_case_accepted

        closing = client.worker_teardown(alias, worker_id)
        assert closing.request_admitted
        assert closing.lifecycle == "closing"

    with pytest.raises(CohesixError, match="model-only"):
        client.worker_spawn("bus", "bus-1")
    with pytest.raises(CohesixError, match="model-only"):
        client.worker_teardown("bus", "bus-1")


def test_stale_worker_generation_is_rejected(tmp_path: Path) -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    backend = FilesystemBackend(str(tmp_path / "mount"))
    path = Path(backend.root + contract.telemetry_path("gpu-stale"))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(_observation("worker-gpu", "gpu-stale", generation=2))
    client = CohesixClient(backend, profile_contract=contract)
    old_identity = WorkerIdentity(
        role="worker-gpu",
        slot=0,
        lease_epoch=1,
        supervisor_generation=1,
        cap_generation=1,
    )

    with pytest.raises(CohesixError, match="stale or wrong generation"):
        client.worker_observe(
            "gpu",
            "gpu-stale",
            expected_identity=old_identity,
        )


def test_profile_contract_is_required_for_worker_api(tmp_path: Path) -> None:
    client = CohesixClient(MockBackend(str(tmp_path / "mock")))
    with pytest.raises(CohesixError, match="explicit target-qualified"):
        client.worker_spawn("heartbeat", "heartbeat-1")


def test_backend_classes_and_optional_rest_bounds_do_not_create_proof(
    tmp_path: Path,
) -> None:
    assert FilesystemBackend(str(tmp_path / "mount")).get_backend_class() == "unknown"
    assert MockBackend(str(tmp_path / "mock")).get_backend_class() == "host-model"

    class MetadataRestBackend(RestBackend):
        def __init__(self, *, include_metadata: bool) -> None:
            self.include_metadata = include_metadata

        def get_bounds(self):
            if not self.include_metadata:
                return {"manifest_sha256": "a" * 64}
            return {
                "manifest_sha256": "a" * 64,
                "worker_runtime_bounds": {
                    "maximum_live_tasks": 3,
                    "telemetry_path_template": "/shard/<label>/worker/<id>/telemetry",
                },
            }

        def _request_payload(self, method, path, query=None, body=None):
            assert method == "GET"
            assert path == "/v1/meta/status"
            value = (
                {"connected": True, "backend_class": "console-projection"}
                if self.include_metadata
                else {"connected": True}
            )
            return json.dumps(value).encode("utf-8")

    absent = MetadataRestBackend(include_metadata=False)
    assert absent.get_worker_runtime_bounds() is None
    assert absent.get_backend_class() == "unknown"

    declared = MetadataRestBackend(include_metadata=True)
    assert declared.get_worker_runtime_bounds() == {
        "maximum_live_tasks": 3,
        "telemetry_path_template": "/shard/<label>/worker/<id>/telemetry",
    }
    assert declared.get_backend_class() == "console-projection"
