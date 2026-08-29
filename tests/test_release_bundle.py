# Author: Lukas Bower
# Purpose: Verify exact Milestone 26e release, Pi 4 payload, and Worker acceptance gates.
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


def test_release_manifest_selects_exact_pi4_sd_payload() -> None:
    source = SCRIPT.read_text(encoding="utf-8")
    inventory = tomllib.loads(
        (ROOT / "configs/implementation_surfaces.toml").read_text(encoding="utf-8")
    )
    release = inventory["release"]
    pi4_files = set(release["pi4_stage_files"])

    assert {
        "cohesix-image-arm-bcm2711",
        "sel4test-driver-image-arm-bcm2711",
        "pi4-image-identity.json",
        "u-boot.bin",
        "start4.elf",
        "fixup4.dat",
        "config.txt",
        "boot.scr.uimg",
        "bcm2711-rpi-4-b.dtb",
        "overlays/upstream-pi4.dtbo",
        "cohesix-driver-runtimes.cpio.uimg",
        "cohesix-root-task-topology.json",
    }.issubset(pi4_files)
    assert "expected_pi4 = set(release[\"pi4_stage_files\"])" in source
    assert "validate_pi4_stage_identity" in source
    assert "Pi 4 SD staging set drift" in source


def test_release_creates_peer_pi4_bundle_with_portable_image() -> None:
    source = SCRIPT.read_text(encoding="utf-8")
    inventory = tomllib.loads(
        (ROOT / "configs/implementation_surfaces.toml").read_text(
            encoding="utf-8"
        )
    )
    release = inventory["release"]

    assert 'PI4_BUNDLE_NAME="${RELEASE_NAME}-Pi4"' in source
    assert 'bundle_pi4_release "${PI4_BUNDLE_NAME}"' in source
    assert "scripts/pi4_release_image.sh" in source
    assert "expected_pi4_bundle_files" in source
    assert all(not path.startswith("pi4-sd/") for path in release["target_images"])
    assert set(release["pi4_generated_bundle_files"]) == {
        "MANIFEST.sha256",
        "VERSION.txt",
        "image/cohesix-pi4-sd.img",
        "image/cohesix-pi4-sd.img.sha256",
        "image/cohesix-pi4-sd.json",
    }


def test_release_linux_builder_locations_are_argument_driven() -> None:
    source = SCRIPT.read_text(encoding="utf-8")
    for option in (
        "--linux-builder-host",
        "--linux-builder-user",
        "--linux-builder-key",
        "--linux-builder-build-dir",
        "--linux-builder-release-dir",
        "--linux-builder-cargo",
        "--linux-builder-cargo-home",
        "--linux-builder-max-glibc",
        "--linux-host-tools-dir",
        "--linux-host-tools-manifest",
    ):
        assert option in source
    for forbidden in ("merlin2.local", "/mnt/nvme", 'LINUX_SYNC_USER:-ubuntu'):
        assert forbidden not in source
    assert "archive-bundle" in source
    assert "cohesix-linux-host-tools-build/v1" in source
    assert "archive_args+=(--force)" in source
    assert 'BUNDLE_DIR="$bundle_dir" python3 -' in source
    assert 'Path("releases")' not in source
