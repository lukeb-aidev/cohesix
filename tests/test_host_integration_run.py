# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Validate strict host-integration inventory, runner, and evidence behavior.
# Author: Lukas Bower

"""Milestone 26e host-integration graph and runner tests."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts/ci/check_host_integration_inventory.py"
SPEC = importlib.util.spec_from_file_location("check_host_integration_inventory", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
host_integration = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = host_integration
SPEC.loader.exec_module(host_integration)

EVIDENCE_MODULE_PATH = ROOT / "scripts/worker_task_evidence.py"
EVIDENCE_SPEC = importlib.util.spec_from_file_location(
    "worker_task_evidence", EVIDENCE_MODULE_PATH
)
assert EVIDENCE_SPEC is not None and EVIDENCE_SPEC.loader is not None
worker_evidence = importlib.util.module_from_spec(EVIDENCE_SPEC)
sys.modules[EVIDENCE_SPEC.name] = worker_evidence
EVIDENCE_SPEC.loader.exec_module(worker_evidence)

GRAPH_PATH = ROOT / "configs/generated/host_integration_dependency.json"
MANIFEST_PATH = ROOT / "configs/generated/root_task_resolved.json"
MATRIX_PATH = ROOT / "configs/host_integration_acceptance.toml"
INVENTORY_PATH = ROOT / "configs/generated/implementation_surface_inventory.json"


def _graph() -> tuple[dict[str, object], bytes]:
    raw = GRAPH_PATH.read_bytes()
    return json.loads(raw), raw


def _artifact(row_id: str) -> dict[str, object]:
    return {"id": f"{row_id}-transcript", "sha256": "2" * 64, "bytes": 128}


def _outcome(row_id: str) -> dict[str, str]:
    return {"id": f"{row_id}-receipt", "class": "receipt", "result": "accepted"}


def _target_input(target: str = "qemu") -> dict[str, object]:
    graph, _ = _graph()
    manifest_sha256 = graph["meta"]["resolved_manifest_sha256"]
    session: dict[str, str] = {
        field: str(index) * 64
        for index, field in enumerate(
            (
                "source_sha256",
                "kernel_sha256",
                "root_image_sha256",
                "driver_archive_sha256",
                "driver_manifest_sha256",
                "cyw43_coexistence_record_sha256",
                "worker_archive_sha256",
                "worker_image_manifest_sha256",
                "worker_abi_sha256",
            ),
            start=1,
        )
    }
    session["target"] = target
    session["manifest_sha256"] = manifest_sha256
    return session


def _target_observations() -> dict[str, object]:
    graph, graph_raw = _graph()
    return {
        "schema": host_integration.OBSERVATION_SCHEMA,
        "dependency_graph_sha256": hashlib.sha256(graph_raw).hexdigest(),
        "manifest_sha256": graph["meta"]["resolved_manifest_sha256"],
        "observations": [
            {
                "dependency_id": row_id,
                "observed_mode": "live",
                "outcomes": [_outcome(row_id)],
                "raw_evidence": [_artifact(row_id)],
            }
            for row_id in host_integration.MANDATORY_TARGET_ROWS
        ],
    }


def _write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(ROOT / "scripts/ci/host_integration_run.sh"), *args],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )


def test_generated_host_integration_inventory_is_exhaustive() -> None:
    summary = host_integration.validate_repository(
        ROOT, MATRIX_PATH, GRAPH_PATH, MANIFEST_PATH, INVENTORY_PATH
    )
    assert summary["dependencies"] >= 20
    assert summary["use_cases"] == 6
    assert summary["playbooks"] == 9
    assert summary["advertised_surfaces"] > 20


def test_matrix_only_lane_emits_no_runtime_evidence(tmp_path: Path) -> None:
    result = _run(
        "--matrix",
        str(MATRIX_PATH),
        "--matrix-only",
        "--state-dir",
        str(tmp_path / "state"),
    )
    assert result.returncode == 0, result.stderr
    record = json.loads((tmp_path / "state/run.json").read_text(encoding="utf-8"))
    assert record["verdict"] == "PASS"
    assert record["mode"] == "matrix-only"
    assert record["evidence_records"] == []


def test_qemu_target_session_emits_exact_three_hash_bound_records(tmp_path: Path) -> None:
    target_input = tmp_path / "target.json"
    observations = tmp_path / "observations.json"
    _write_json(target_input, _target_input())
    _write_json(observations, _target_observations())
    result = _run(
        "--matrix",
        str(MATRIX_PATH),
        "--mode",
        "live",
        "--target",
        "qemu",
        "--target-session",
        str(target_input),
        "--observations",
        str(observations),
        "--state-dir",
        str(tmp_path / "state"),
    )
    assert result.returncode == 0, result.stderr
    record = json.loads((tmp_path / "state/run.json").read_text(encoding="utf-8"))
    assert record["verdict"] == "PASS"
    assert tuple(item["id"] for item in record["evidence_records"]) == (
        "gpu-receipt-path",
        "peft-receipt-path",
        "worker-control",
    )
    for row_id in host_integration.MANDATORY_TARGET_ROWS:
        evidence = json.loads(
            (tmp_path / f"state/integration/{row_id}.json").read_text(encoding="utf-8")
        )
        assert evidence["target_session"]["worker_abi_sha256"] == "9" * 64
        assert evidence["execution_proof"] == "qemu"
        assert evidence["observed_mode"] == "live"
        worker_evidence.validate_integration(evidence, "qemu")


def test_mock_cannot_satisfy_provider_live_row(tmp_path: Path) -> None:
    graph, graph_raw = _graph()
    observation = {
        "schema": host_integration.OBSERVATION_SCHEMA,
        "dependency_graph_sha256": hashlib.sha256(graph_raw).hexdigest(),
        "manifest_sha256": graph["meta"]["resolved_manifest_sha256"],
        "observations": [
            {
                "dependency_id": "gpu-host-provider",
                "observed_mode": "mock",
                "outcomes": [_outcome("gpu-host-provider")],
                "raw_evidence": [_artifact("gpu-host-provider")],
            }
        ],
    }
    observation_path = tmp_path / "observations.json"
    _write_json(observation_path, observation)
    result = _run(
        "--mode",
        "mock",
        "--dependency",
        "gpu-host-provider",
        "--observations",
        str(observation_path),
        "--state-dir",
        str(tmp_path / "state"),
    )
    assert result.returncode == 2
    assert "does not satisfy required mode" in result.stderr
    evidence = json.loads(
        (tmp_path / "state/integration/gpu-host-provider.json").read_text(encoding="utf-8")
    )
    assert evidence["verdict"] == "FAIL"


def test_stale_and_wrong_target_sessions_fail_closed(tmp_path: Path) -> None:
    stale_path = tmp_path / "stale-observations.json"
    stale = _target_observations()
    stale["dependency_graph_sha256"] = "f" * 64
    _write_json(stale_path, stale)
    target_path = tmp_path / "target.json"
    _write_json(target_path, _target_input())
    result = _run(
        "--mode",
        "live",
        "--target",
        "qemu",
        "--target-session",
        str(target_path),
        "--observations",
        str(stale_path),
        "--state-dir",
        str(tmp_path / "stale-state"),
    )
    assert result.returncode == 2
    assert "observation identity mismatch" in result.stderr

    wrong = _target_input("pi4")
    wrong_path = tmp_path / "wrong.json"
    _write_json(wrong_path, wrong)
    valid_observations = tmp_path / "valid-observations.json"
    _write_json(valid_observations, _target_observations())
    result = _run(
        "--mode",
        "live",
        "--target",
        "qemu",
        "--target-session",
        str(wrong_path),
        "--observations",
        str(valid_observations),
        "--state-dir",
        str(tmp_path / "wrong-state"),
    )
    assert result.returncode == 2
    assert "wrong target" in result.stderr


def test_secret_bearing_observation_is_rejected(tmp_path: Path) -> None:
    graph, graph_raw = _graph()
    observation = {
        "schema": host_integration.OBSERVATION_SCHEMA,
        "dependency_graph_sha256": hashlib.sha256(graph_raw).hexdigest(),
        "manifest_sha256": graph["meta"]["resolved_manifest_sha256"],
        "observations": [
            {
                "dependency_id": "python-sdk-projection",
                "observed_mode": "live",
                "provider_version": "authorization: Bearer abc123",
                "outcomes": [_outcome("python-sdk-projection")],
                "raw_evidence": [_artifact("python-sdk-projection")],
            }
        ],
    }
    observation_path = tmp_path / "secret.json"
    _write_json(observation_path, observation)
    result = _run(
        "--mode",
        "live",
        "--dependency",
        "python-sdk-projection",
        "--observations",
        str(observation_path),
        "--state-dir",
        str(tmp_path / "state"),
    )
    assert result.returncode == 2
    assert "sensitive material" in result.stderr


def test_use_case_cannot_promote_without_required_target_session(tmp_path: Path) -> None:
    result = _run(
        "--mode",
        "live",
        "--use-case",
        "gpu-flight-deck",
        "--state-dir",
        str(tmp_path / "state"),
    )
    assert result.returncode == 2
    assert "requires a target session" in result.stderr


def test_future_live_missing_owner_and_cycle_are_rejected() -> None:
    graph, _ = _graph()
    future = copy.deepcopy(graph)
    row = next(item for item in future["dependencies"] if item["id"] == "mcp-gateway")
    row["allowed_modes"].append("live")
    with pytest.raises(host_integration.HostIntegrationError, match="future provider"):
        host_integration.validate_graph_payload(future)

    missing_owner = copy.deepcopy(graph)
    missing_owner["dependencies"][0]["owner"] = ""
    with pytest.raises(host_integration.HostIntegrationError, match="owner"):
        host_integration.validate_graph_payload(missing_owner)

    cycle = copy.deepcopy(graph)
    worker = next(item for item in cycle["dependencies"] if item["id"] == "worker-control")
    worker["dependencies"] = ["gpu-receipt-path"]
    with pytest.raises(host_integration.HostIntegrationError, match="circular"):
        host_integration.validate_graph_payload(cycle)
