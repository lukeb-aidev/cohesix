# Author: Lukas Bower
# Purpose: Exercise dual-interpreter wheel and QEMU projection evidence gates.
# Copyright 2026 Lukas Bower

"""Integration tests for scripts/ci/python_compat_run.sh."""

from __future__ import annotations

import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = PACKAGE_ROOT.parents[1]
RUNNER = REPO_ROOT / "scripts/ci/python_compat_run.sh"
QEMU_CONTRACT = (
    REPO_ROOT / "configs/generated/cohesix_python_qemu_smp_production.json"
)


def _python(version: str) -> str | None:
    candidate = shutil.which(f"python{version}")
    if candidate:
        return candidate
    homebrew = Path(f"/opt/homebrew/bin/python{version}")
    return str(homebrew) if homebrew.is_file() else None


def _run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(RUNNER), *args],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )


@pytest.mark.skipif(
    _python("3.11") is None or _python("3.13") is None,
    reason="Python 3.11 and 3.13 are required for the compatibility matrix",
)
def test_wheel_smoke_and_qemu_projection_are_hash_bound(tmp_path: Path) -> None:
    wheel_dir = tmp_path / "wheels"
    package_manifest = tmp_path / "python-package.json"
    wheel_state = tmp_path / "wheel-state"
    wheel_dir.mkdir()
    build = subprocess.run(
        [
            sys.executable,
            "-m",
            "pip",
            "wheel",
            "--disable-pip-version-check",
            "--no-build-isolation",
            "--no-deps",
            "--wheel-dir",
            str(wheel_dir),
            str(PACKAGE_ROOT),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    assert build.returncode == 0, build.stderr

    smoke = _run(
        "--wheel-smoke",
        "--wheel-dir",
        str(wheel_dir),
        "--package-manifest",
        str(package_manifest),
        "--state-dir",
        str(wheel_state),
    )
    assert smoke.returncode == 0, smoke.stderr
    package = json.loads(package_manifest.read_text(encoding="utf-8"))
    assert package["schema"] == "cohesix-python-package/v1"
    assert package["proof_boundary"]["python_projection_is_authority"] is False
    assert {item["python_version"][:4] for item in package["interpreters"]} == {
        "3.11",
        "3.13",
    }

    contract = json.loads(QEMU_CONTRACT.read_text(encoding="utf-8"))
    hashes = {
        "source_sha256": "1" * 64,
        "manifest_sha256": contract["manifest_sha256"],
        "kernel_sha256": "2" * 64,
        "root_image_sha256": "3" * 64,
        "driver_archive_sha256": "4" * 64,
        "driver_manifest_sha256": "5" * 64,
        "cyw43_coexistence_record_sha256": "6" * 64,
        "worker_archive_sha256": "7" * 64,
        "worker_image_manifest_sha256": "8" * 64,
        "worker_abi_sha256": "9" * 64,
    }
    target_session = {"target": "qemu", **hashes}
    accepted_target_record = {
        "schema": "cohesix-worker-integration-evidence/v1",
        "record_kind": "worker-integration",
        "dependency_id": "worker-control",
        "owner_milestone": "m26e-host-worker-integration",
        "obligation": "role_required",
        "observed_mode": "live",
        "dependency_graph_sha256": hashlib.sha256(
            (
                REPO_ROOT / "configs/generated/host_integration_dependency.json"
            ).read_bytes()
        ).hexdigest(),
        "manifest_sha256": contract["manifest_sha256"],
        "component_sha256": "a" * 64,
        "config_sha256": "b" * 64,
        "host": {
            "profile": "macos-arm64",
            "os": "macos",
            "architecture": "aarch64",
        },
        "target_session": target_session,
        "execution_proof": "qemu",
        "outcomes": [
            {"id": "worker-control", "class": "observation", "result": "accepted"}
        ],
        "raw_evidence": [
            {"id": "qemu-transcript", "sha256": "c" * 64, "bytes": 128}
        ],
        "verdict": "PASS",
        "blockers": [],
    }
    target_session_path = tmp_path / "qemu-target-session.json"
    target_session_path.write_text(
        json.dumps(accepted_target_record, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    qemu_state = tmp_path / "qemu-state"
    projection = _run(
        "--python-matrix",
        "3.11,3.13",
        "--target",
        "qemu",
        "--profile-contract",
        str(QEMU_CONTRACT),
        "--package-manifest",
        str(package_manifest),
        "--wheel-dir",
        str(wheel_dir),
        "--matrix",
        str(REPO_ROOT / "configs/host_integration_acceptance.toml"),
        "--target-session",
        str(target_session_path),
        "--state-dir",
        str(qemu_state),
    )
    assert projection.returncode == 0, projection.stderr
    evidence = json.loads(
        (qemu_state / "python-sdk-projection.json").read_text(encoding="utf-8")
    )
    assert evidence["dependency_id"] == "python-sdk-projection"
    assert evidence["target_session"] == target_session
    assert evidence["execution_proof"] == "qemu"
    assert evidence["verdict"] == "PASS"


def test_matrix_refuses_missing_target_session(tmp_path: Path) -> None:
    source = RUNNER.read_text(encoding="utf-8")
    assert "requires an existing regular --target-session record" in source
    assert "qemu-system-aarch64" not in source
    assert "worker_task_evidence.py" in source
