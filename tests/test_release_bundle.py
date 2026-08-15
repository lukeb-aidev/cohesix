# Author: Lukas Bower
# Purpose: Verify release gates for exact Milestone 26e Worker acceptance evidence.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "release_bundle.sh"
FIXTURE_MODULE_PATH = ROOT / "tests" / "test_worker_task_evidence.py"
SPEC = importlib.util.spec_from_file_location(
    "worker_evidence_test_support", FIXTURE_MODULE_PATH
)
assert SPEC is not None and SPEC.loader is not None
worker_support = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = worker_support
SPEC.loader.exec_module(worker_support)


def _command(qemu: tuple[Path, Path, Path], pi4: tuple[Path, Path, Path]) -> list[str]:
    return [
        str(SCRIPT),
        "--verify-worker-acceptance",
        "--worker-qemu-evidence",
        str(qemu[0]),
        "--worker-pi4-evidence",
        str(pi4[0]),
        "--worker-root-qemu-evidence",
        str(qemu[1]),
        "--worker-root-pi4-evidence",
        str(pi4[1]),
        "--worker-system-qemu-evidence",
        str(qemu[2]),
        "--worker-system-pi4-evidence",
        str(pi4[2]),
    ]


def test_release_worker_acceptance_gate_validates_exact_six_record_graph(
    tmp_path: Path,
) -> None:
    qemu = worker_support._target_graph(tmp_path, "qemu")
    pi4 = worker_support._target_graph(tmp_path, "pi4")

    result = subprocess.run(
        _command(qemu, pi4),
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr
    assert "six-record acceptance graph: PASS" in result.stdout


def test_release_worker_acceptance_gate_rejects_one_sided_or_tampered_input(
    tmp_path: Path,
) -> None:
    qemu = worker_support._target_graph(tmp_path, "qemu")
    pi4 = worker_support._target_graph(tmp_path, "pi4")
    pi4[2].write_text("{}\n", encoding="utf-8")

    result = subprocess.run(
        _command(qemu, pi4),
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "Worker-runtime acceptance graph validation failed" in result.stderr


def test_release_gate_exposes_every_required_evidence_option() -> None:
    source = SCRIPT.read_text(encoding="utf-8")
    for option in (
        "--worker-qemu-evidence",
        "--worker-pi4-evidence",
        "--worker-root-qemu-evidence",
        "--worker-root-pi4-evidence",
        "--worker-system-qemu-evidence",
        "--worker-system-pi4-evidence",
    ):
        assert option in source
    assert "scripts/worker_task_evidence.py" in source


def test_release_manifest_selects_hash_bound_python_wheel_and_contracts() -> None:
    source = SCRIPT.read_text(encoding="utf-8")
    assert "validate_python_package_inputs" in source
    assert "cohesix-python-package/v1" in source
    assert "python_projection_is_authority" in source
    assert "python/m26e-python-package.json" in source

    inventory = tomllib.loads(
        (ROOT / "configs/implementation_surfaces.toml").read_text(encoding="utf-8")
    )
    release = inventory["release"]
    assert "configs/generated/cohesix_python_qemu_smp_production.json" in release[
        "generated_configs"
    ]
    assert "configs/generated/cohesix_python_pi4_production.json" in release[
        "generated_configs"
    ]
    assert "configs/generated/root_task_topology.json" in release[
        "generated_configs"
    ]
    assert "tools/cohesix-py/cohesix/worker.py" in release["python_artifacts"]
    assert "tests/fixtures/cas/max_chunks_v1.txt" in release["cas_fixtures"]
    assert "cas/max_chunks_v1.txt.sha256" in release[
        "generated_bundle_files"
    ]
    assert "python/dist/cohesix-0.2.0a2-py3-none-any.whl" in release[
        "generated_bundle_files"
    ]
