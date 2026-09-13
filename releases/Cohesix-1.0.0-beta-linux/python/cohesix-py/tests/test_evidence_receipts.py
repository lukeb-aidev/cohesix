# Author: Lukas Bower
# Purpose: Validate Python SDK evidence-pack, timeline, and receipt-backed workflows.
# Copyright 2026 Lukas Bower

"""Tests for Cohesix Python evidence and receipt APIs."""

from __future__ import annotations

import copy
import hashlib
import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.audit import CohesixAudit  # noqa: E402
from cohesix.backends import MockBackend  # noqa: E402
from cohesix.client import CohesixClient, GpuLeaseArgs  # noqa: E402
from cohesix.errors import CohesixError  # noqa: E402
from cohesix.evidence import (  # noqa: E402
    build_python_projection_evidence,
    validate_worker_integration_evidence,
)
from cohesix.receipts import (  # noqa: E402
    CompatibilityReceipt,
    WORKER_GPU_RECEIPT_SCHEMA,
    WORKER_LORA_RECEIPT_SCHEMA,
    parse_receipt,
    receipt_actions_for_role,
)
from cohesix.worker import WorkerIdentity, load_profile_contract  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
QEMU_CONTRACT = (
    REPO_ROOT / "configs/generated/cohesix_python_qemu_smp_production.json"
)


def _seed_evidence_sources(root: Path) -> None:
    (root / "proc" / "schedule").mkdir(parents=True, exist_ok=True)
    (root / "proc" / "lease").mkdir(parents=True, exist_ok=True)
    (root / "audit").mkdir(parents=True, exist_ok=True)
    (root / "log").mkdir(parents=True, exist_ok=True)
    (root / "replay").mkdir(parents=True, exist_ok=True)

    (root / "proc" / "boot").write_text("boot_manifest=d1880bfe\n", encoding="utf-8")
    (root / "proc" / "schedule" / "summary").write_text(
        "queue=1 dequeued=0 dropped=0 max_entries=256\n",
        encoding="utf-8",
    )
    (root / "proc" / "schedule" / "queue").write_text(
        "id=sched-1 role=worker-gpu priority=4 ticks=2 budget_ms=120 seq=1\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "summary").write_text(
        "active=1 preemptions=0 quotas=1 max_active=256 max_preemptions=256\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "active").write_text(
        "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=7\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "preemptions").write_text("", encoding="utf-8")
    (root / "log" / "queen.log").write_text(
        "boot ok\nscheduler ok\n",
        encoding="utf-8",
    )
    (root / "replay" / "status").write_text(
        "{\"enabled\":false,\"entries\":0}\n",
        encoding="utf-8",
    )
    (root / "audit" / "export").write_text(
        json.dumps(
            {
                "journal_base": 0,
                "journal_next": 4096,
                "decisions_base": 0,
                "decisions_next": 1024,
                "replay_enabled": False,
                "replay_max_entries": 0,
            }
        )
        + "\n",
        encoding="utf-8",
    )
    (root / "audit" / "journal").write_text(
        json.dumps(
            {
                "seq": 2,
                "kind": "queen-ctl",
                "path": "/queen/ctl",
                "payload": "{}",
                "outcome": "ok",
                "error": None,
                "role": "queen",
                "ticket": "cohesix-ticket-raw-secret",
            }
        )
        + "\n",
        encoding="utf-8",
    )
    (root / "audit" / "decisions").write_text(
        json.dumps(
            {
                "seq": 3,
                "kind": "policy-gate",
                "outcome": "approve",
                "id": "decision-1",
                "target": "/queen/ctl",
                "path": "/queen/ctl",
                "role": "queen",
                "ticket": "cohesix-ticket-raw-secret",
            }
        )
        + "\n",
        encoding="utf-8",
    )


def _seed_proc_lease(root: Path) -> None:
    (root / "proc" / "lease").mkdir(parents=True, exist_ok=True)
    (root / "proc" / "lease" / "summary").write_text(
        "active=1 preemptions=0 quotas=1 max_active=256 max_preemptions=256\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "active").write_text(
        "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=11\n",
        encoding="utf-8",
    )
    (root / "proc" / "lease" / "preemptions").write_text("", encoding="utf-8")


def test_evidence_pack_export_and_redaction(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    _seed_evidence_sources(root)

    client = CohesixClient(backend)
    audit = CohesixAudit()
    pack_dir = tmp_path / "evidence_pack"
    summary = client.evidence_pack(pack_dir, with_telemetry=False, audit=audit)

    assert summary.captured >= 8
    assert summary.errors == 0
    assert (pack_dir / "meta.json").is_file()
    assert (pack_dir / "bounds.json").is_file()
    assert (pack_dir / "summary.json").is_file()
    assert (pack_dir / "log" / "queen.log").is_file()
    assert json.loads((pack_dir / "meta.json").read_text(encoding="utf-8"))[
        "manifest_sha256"
    ] == "unknown"
    assert json.loads((pack_dir / "bounds.json").read_text(encoding="utf-8"))[
        "manifest_sha256"
    ] == "unknown"

    journal = (pack_dir / "audit" / "journal").read_text(encoding="utf-8")
    assert "cohesix-ticket-raw-secret" not in journal
    assert "sha256:" in journal


def test_evidence_timeline_stable_ordering(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    client = CohesixClient(backend)
    pack_dir = tmp_path / "pack"
    (pack_dir / "audit").mkdir(parents=True, exist_ok=True)
    (pack_dir / "proc" / "lease").mkdir(parents=True, exist_ok=True)

    (pack_dir / "audit" / "journal").write_text(
        "\n".join(
            [
                json.dumps(
                    {
                        "seq": 2,
                        "kind": "queen-ctl",
                        "path": "/queen/ctl",
                        "payload": "{}",
                        "outcome": "ok",
                        "error": None,
                        "role": "queen",
                        "ticket": "sha256:dead",
                    }
                ),
                json.dumps(
                    {
                        "seq": 1,
                        "kind": "queen-ctl",
                        "path": "/queen/ctl",
                        "payload": "{}",
                        "outcome": "ok",
                        "error": None,
                        "role": "queen",
                        "ticket": "sha256:beef",
                    }
                ),
                "",
            ]
        ),
        encoding="utf-8",
    )
    (pack_dir / "audit" / "decisions").write_text(
        json.dumps(
            {
                "seq": 3,
                "kind": "policy-gate",
                "outcome": "approve",
                "id": "a1",
                "target": "/queen/ctl",
                "path": "/queen/ctl",
                "role": "queen",
                "ticket": "sha256:cafe",
            }
        )
        + "\n",
        encoding="utf-8",
    )
    (pack_dir / "proc" / "lease" / "active").write_text(
        "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=7\n",
        encoding="utf-8",
    )

    timeline = client.evidence_timeline(pack_dir)
    assert timeline.events == 4
    lines = [
        line
        for line in timeline.ndjson_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    assert len(lines) == 4
    first = json.loads(lines[0])
    second = json.loads(lines[1])
    assert first.get("seq") == 1
    assert second.get("seq") == 2


def test_gpu_lease_receipt_contains_proc_snapshot(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    _seed_proc_lease(root)

    client = CohesixClient(backend)
    audit = CohesixAudit()
    receipt_path = tmp_path / "lease_receipt.json"
    args = GpuLeaseArgs(
        gpu_id="GPU-0",
        mem_mb=1024,
        streams=1,
        ttl_s=60,
        priority=1,
    )
    receipt = client.gpu_lease_with_receipt(args, receipt_path, audit)
    assert receipt["kind"] == "gpu-lease"
    assert receipt["manifest_sha256"] == "unknown"
    payload = json.loads(receipt_path.read_text(encoding="utf-8"))
    entries = payload["proc_lease"]["active_entries"]
    assert isinstance(entries, list)
    assert any(entry.get("id") == "lease-1" for entry in entries)
    assert "cohesix-ticket-" not in receipt_path.read_text(encoding="utf-8")


def test_run_receipt_no_secrets(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    _seed_proc_lease(root)
    (root / "gpu" / "GPU-0" / "lease").write_text(
        json.dumps(
            {
                "schema": "gpu-lease/v1",
                "state": "ACTIVE",
                "gpu_id": "GPU-0",
                "worker_id": "worker-1",
                "mem_mb": 1,
                "streams": 1,
                "ttl_s": 60,
                "priority": 1,
            }
        )
        + "\n",
        encoding="utf-8",
    )

    client = CohesixClient(backend)
    audit = CohesixAudit()
    receipt_path = tmp_path / "run_receipt.json"
    receipt = client.run_command_with_receipt(
        gpu_id="GPU-0",
        command=[sys.executable, "-c", "print('ok')"],
        receipt_out=receipt_path,
        audit=audit,
    )
    assert receipt["kind"] == "run"
    assert receipt["manifest_sha256"] == "unknown"
    payload = json.loads(receipt_path.read_text(encoding="utf-8"))
    assert payload["status"] == "ok"
    assert isinstance(payload.get("acks"), list)
    assert payload["acks"]
    text = receipt_path.read_text(encoding="utf-8")
    assert "cohesix-ticket-" not in text
    assert "changeme" not in text


def test_gpu_list_skips_non_gpu_entries_without_info(tmp_path: Path) -> None:
    backend = MockBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    (root / "gpu" / "bridge").mkdir(parents=True, exist_ok=True)

    client = CohesixClient(backend)
    gpus = client.gpu_list()
    ids = {str(item["id"]) for item in gpus}
    assert ids == {"GPU-0", "GPU-1"}


def test_evidence_pack_missing_replay_invalid_path_is_non_fatal(tmp_path: Path) -> None:
    class ReplayInvalidPathBackend(MockBackend):
        def read_file(self, path: str, max_bytes: int) -> bytes:
            if path == "/replay/status":
                raise CohesixError("invalid-path")
            return super().read_file(path, max_bytes)

    backend = ReplayInvalidPathBackend(root=str(tmp_path / "mockfs"))
    root = Path(backend.root)
    _seed_evidence_sources(root)
    (root / "replay" / "status").unlink(missing_ok=True)

    client = CohesixClient(backend)
    summary = client.evidence_pack(tmp_path / "pack", with_telemetry=False)
    assert summary.errors == 0
    payload = json.loads((tmp_path / "pack" / "summary.json").read_text(encoding="utf-8"))
    replay_items = [item for item in payload["items"] if item.get("path") == "/replay/status"]
    assert replay_items
    assert replay_items[0]["status"] == "missing"


def _identity(role: str, *, generation: int = 7) -> WorkerIdentity:
    return WorkerIdentity(
        role=role,
        slot=1,
        lease_epoch=3,
        supervisor_generation=generation,
        cap_generation=9,
    )


def _worker_receipt(
    action: str,
    role: str,
    schema: str,
    manifest_sha256: str,
    *,
    generation: int = 7,
) -> dict[str, object]:
    identity = _identity(role, generation=generation)
    return {
        "schema": schema,
        "action": action,
        "public_instance_id": "receipt-worker-1",
        "identity": {
            "role": identity.role,
            "slot": identity.slot,
            "lease_epoch": identity.lease_epoch,
            "supervisor_generation": identity.supervisor_generation,
            "cap_generation": identity.cap_generation,
        },
        "sequence": 11,
        "committed_sequence": 11,
        "outcome": "confirmed",
        "digests": {
            "ticket": "1" * 64,
            "idempotency": "2" * 64,
            "operation": "3" * 64,
            "subject": "4" * 64,
            "result": "5" * 64,
        },
        "manifest_sha256": manifest_sha256,
    }


def test_exact_gpu_and_peft_receipt_actions_are_non_authoritative_projections() -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    cases = (
        *(
            (action, "worker-gpu", WORKER_GPU_RECEIPT_SCHEMA)
            for action in receipt_actions_for_role("gpu")
        ),
        *(
            (action, "worker-lora", WORKER_LORA_RECEIPT_SCHEMA)
            for action in receipt_actions_for_role("lora")
        ),
    )
    assert [case[0] for case in cases] == [
        "gpu.lease.grant",
        "gpu.lease.renew",
        "gpu.lease.release",
        "peft.export",
        "peft.import",
        "peft.activate",
        "peft.rollback",
    ]
    for action, role, schema in cases:
        payload = _worker_receipt(action, role, schema, contract.manifest_sha256)
        with pytest.raises(CohesixError, match="local-admitted only"):
            parse_receipt(payload, contract=contract)
        receipt = parse_receipt(
            payload,
            contract=contract,
            expected_identity=_identity(role),
            expected_instance_id="receipt-worker-1",
            source="local-admitted",
        )
        assert receipt.action == action
        assert receipt.state == "confirmed"
        assert receipt.local_admitted
        assert not receipt.authoritative


def test_v1_receipt_remains_compatibility_only_and_v2_can_be_stale() -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    compatibility = parse_receipt(
        {"schema": "cohesix-receipt-v1", "kind": "gpu-lease", "status": "ok"},
        contract=contract,
    )
    assert isinstance(compatibility, CompatibilityReceipt)
    assert compatibility.state == "none"
    assert not compatibility.authoritative

    payload = _worker_receipt(
        "gpu.lease.grant",
        "worker-gpu",
        WORKER_GPU_RECEIPT_SCHEMA,
        contract.manifest_sha256,
        generation=8,
    )
    stale = parse_receipt(
        payload,
        contract=contract,
        expected_identity=_identity("worker-gpu", generation=7),
        source="local-admitted",
    )
    assert stale.state == "stale"
    assert not stale.authoritative


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (
            lambda value: value.update({"manifest_sha256": "f" * 64}),
            "manifest differs",
        ),
        (
            lambda value: value.update({"committed_sequence": 12}),
            "sequence-last committed",
        ),
        (
            lambda value: value["digests"].update({"result": "BAD"}),
            "not lowercase SHA-256",
        ),
        (
            lambda value: value.update({"authorization": "Bearer secret"}),
            "prohibited authority data",
        ),
        (
            lambda value: value.update({"public_instance_id": "../escape"}),
            "bounded ASCII token",
        ),
    ],
)
def test_malformed_worker_receipts_fail_closed(mutation, message: str) -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    payload = _worker_receipt(
        "gpu.lease.grant",
        "worker-gpu",
        WORKER_GPU_RECEIPT_SCHEMA,
        contract.manifest_sha256,
    )
    mutation(payload)
    with pytest.raises(CohesixError, match=message):
        parse_receipt(payload, contract=contract, source="local-admitted")


def _target_session(manifest_sha256: str) -> dict[str, str]:
    return {
        "target": "qemu",
        "source_sha256": "1" * 64,
        "manifest_sha256": manifest_sha256,
        "kernel_sha256": "2" * 64,
        "root_image_sha256": "3" * 64,
        "driver_archive_sha256": "4" * 64,
        "driver_manifest_sha256": "5" * 64,
        "cyw43_coexistence_record_sha256": "6" * 64,
        "worker_archive_sha256": "7" * 64,
        "worker_image_manifest_sha256": "8" * 64,
        "worker_abi_sha256": "9" * 64,
    }


def test_python_projection_evidence_references_target_session_without_promotion() -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    graph_sha256 = hashlib.sha256(
        (REPO_ROOT / "configs/generated/host_integration_dependency.json").read_bytes()
    ).hexdigest()
    matrix_sha256 = hashlib.sha256(
        (REPO_ROOT / "configs/host_integration_acceptance.toml").read_bytes()
    ).hexdigest()
    wheel_sha256 = "a" * 64
    record = build_python_projection_evidence(
        contract=contract,
        dependency_graph_sha256=graph_sha256,
        matrix_sha256=matrix_sha256,
        wheel_sha256=wheel_sha256,
        host={
            "profile": "macos-arm64",
            "os": "macos",
            "architecture": "aarch64",
            "provider_version": "CPython-3.11.13,CPython-3.13.7",
        },
        target_session=_target_session(contract.manifest_sha256),
        interpreter_outcomes=[
            {
                "id": "cpython-3-13",
                "class": "projection-compatibility",
                "result": "accepted",
            },
            {
                "id": "cpython-3-11",
                "class": "projection-compatibility",
                "result": "accepted",
            },
        ],
        raw_evidence=[
            {"id": "python-3.13.json", "sha256": "b" * 64, "bytes": 128},
            {"id": "python-3.11.json", "sha256": "c" * 64, "bytes": 128},
        ],
    )
    assert record["execution_proof"] == "qemu"
    assert record["dependency_id"] == "python-sdk-projection"
    assert record["target_session"]["worker_abi_sha256"] == "9" * 64
    assert record["verdict"] == "PASS"
    validate_worker_integration_evidence(
        record,
        contract=contract,
        dependency_graph_sha256=graph_sha256,
        expected_target="qemu",
        matrix_sha256=matrix_sha256,
        wheel_sha256=wheel_sha256,
    )

    for key, value, message in (
        ("component_sha256", "d" * 64, "wrong profile contract"),
        ("config_sha256", "d" * 64, "wrong source matrix"),
        ("artifact_sha256", "d" * 64, "wrong wheel"),
    ):
        tampered = copy.deepcopy(record)
        tampered[key] = value
        with pytest.raises(CohesixError, match=message):
            validate_worker_integration_evidence(
                tampered,
                contract=contract,
                dependency_graph_sha256=graph_sha256,
                expected_target="qemu",
                matrix_sha256=matrix_sha256,
                wheel_sha256=wheel_sha256,
            )


def test_python_projection_evidence_rejects_wrong_target_host_and_secret() -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    base = {
        "contract": contract,
        "dependency_graph_sha256": "a" * 64,
        "matrix_sha256": "b" * 64,
        "wheel_sha256": "c" * 64,
        "host": {
            "profile": "macos-arm64",
            "os": "macos",
            "architecture": "aarch64",
            "provider_version": "CPython-3.11.13,CPython-3.13.7",
        },
        "target_session": _target_session(contract.manifest_sha256),
        "interpreter_outcomes": [
            {
                "id": "cpython-3-11",
                "class": "projection-compatibility",
                "result": "accepted",
            },
            {
                "id": "cpython-3-13",
                "class": "projection-compatibility",
                "result": "accepted",
            },
        ],
        "raw_evidence": [
            {"id": "smoke.json", "sha256": "d" * 64, "bytes": 1}
        ],
    }
    wrong_target = copy.deepcopy(base)
    wrong_target["target_session"]["target"] = "pi4"
    with pytest.raises(CohesixError, match="wrong target"):
        build_python_projection_evidence(**wrong_target)

    wrong_host = copy.deepcopy(base)
    wrong_host["host"]["architecture"] = "x86_64"
    with pytest.raises(CohesixError, match="host identity is inconsistent"):
        build_python_projection_evidence(**wrong_host)

    secret = copy.deepcopy(base)
    secret["host"]["provider_version"] = "authorization: Bearer abc123"
    with pytest.raises(CohesixError, match="sensitive material"):
        build_python_projection_evidence(**secret)
