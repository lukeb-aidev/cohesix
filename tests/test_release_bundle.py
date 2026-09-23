# Author: Lukas Bower
# Purpose: Verify release selection, publication immutability, Pi payloads and Worker gates.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import importlib.util
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
import zipfile

import pytest

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


@pytest.mark.parametrize("profile", [
    "qemu_smp_production", "qemu_smp_kvm_production",
])
@pytest.mark.parametrize("selected_pi", [False, True])
def test_build_retains_selected_python_contract_sources(tmp_path, profile, selected_pi):
    """Provisioned Pi contracts follow the Pi image without changing host selection."""
    pi_manifest = tmp_path / "provisioned pi.toml"
    pi_manifest.write_text("# compiler input routed separately from QEMU\n")
    qemu_manifest = tmp_path / "provisioned qemu.toml"
    script = ROOT / "scripts/cohesix-build-run.sh"
    result = subprocess.run([
        "bash", "-c",
        'source "$1"; RTC_MANIFEST=$2; COH_RTC_PI4_MANIFEST=$3; '
        'SEL4_PROFILE=$4; cargo() { printf "%s\\0" CALL "$@"; }; '
        'generate_python_profile_contracts',
        "projection-test", str(script), str(qemu_manifest),
        str(pi_manifest) if selected_pi else "", profile,
    ], capture_output=True, check=False)
    assert result.returncode == 0, result.stderr.decode()
    calls = result.stdout.decode().split("CALL\0")[1:]
    expected_pi = (
        pi_manifest if selected_pi
        else ROOT / "configs/root_task_pi4_uboot_aarch64.toml"
    )
    assert len(calls) == 2
    for call, manifest, target in zip(
        calls, (qemu_manifest, expected_pi), (profile, "pi4_production")
    ):
        args = call.rstrip("\0").split("\0")
        assert args[args.index("--") + 1] == str(manifest)
        assert args[args.index("--profile") + 1] == target


def test_missing_selected_pi_contract_manifest_fails_before_generation(tmp_path):
    """A typo must not silently retain a default Pi projection."""
    result = subprocess.run([
        "bash", "-c",
        'source "$1"; COH_RTC_PI4_MANIFEST=$2; '
        'cargo() { printf "unexpected compiler call"; }; '
        'generate_python_profile_contracts',
        "projection-test", str(ROOT / "scripts/cohesix-build-run.sh"),
        str(tmp_path / "missing.toml"),
    ], capture_output=True, text=True, check=False)
    assert result.returncode != 0
    assert "Pi Python contract manifest is missing" in result.stderr
    assert "unexpected compiler call" not in result.stdout


def test_release_a_inventory_selects_current_notes_and_preserves_history() -> None:
    """Release A selects 1.1.0 while keeping the linked prior notes immutable."""
    inventory = tomllib.loads(
        (ROOT / "configs/implementation_surfaces.toml").read_text()
    )
    release = inventory["release"]
    assert release["version"] == "1.1.0-beta"
    assert "releases/RELEASE_NOTES-1.1.0-beta.md" in release["support_files"]
    assert "releases/RELEASE_NOTES-1.0.0-beta.md" in release["support_files"]
    for contract in (
        "cuda_recipe.json", "provider_registry.json", "use_case_evidence.json",
    ):
        assert f"configs/generated/{contract}" in release["generated_configs"]
    assert "packaging/ci/cohesix-journey.yml" in release["support_files"]
    for guide in (
        "ADOPTION", "CI_WORKFLOWS", "PRIVATE_LORA_RELEASE", "SWARMUI",
        "OPERATOR_WALKTHROUGH", "OPERATOR_RECIPES", "CAUSAL_EVIDENCE",
    ):
        assert f"docs/{guide}.md" in release["public_documents"]
    current = next(
        row for row in inventory["tracked_rules"]
        if row["id"] == "release-current-notes"
    )
    assert current["exact"] == "releases/RELEASE_NOTES-1.1.0-beta.md"


def test_publication_accepts_current_release_docs_and_refuses_historical_edits(
    tmp_path: Path,
) -> None:
    """A current documentation update cannot authorize changes to old releases."""
    spec = importlib.util.spec_from_file_location(
        "release_publication_test", ROOT / "scripts/release_publication.py"
    )
    assert spec is not None and spec.loader is not None
    publication = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(publication)
    for path in (
        "releases/RELEASE_NOTES-1.1.0-beta.md",
        "docs/audit/M27G_IMPLEMENTATION_RECORD.md",
    ):
        assert publication.classify_change(
            tmp_path, tmp_path, path, False
        ) == "release-documentation"
    for path in (
        "scripts/pi4_gate_proof.sh",
        "scripts/pi4_serial_reboot.py",
        "tests/test_pi4_gate_proof.py",
        "tests/test_pi4_serial_reboot.py",
    ):
        assert publication.classify_change(
            tmp_path, tmp_path, path, False
        ) == "physical-proof-collector"
    for path in (
        "releases/RELEASE_NOTES-1.0.0-beta.md",
        "releases/RELEASE_NOTES-1.2.0-beta.md",
        "docs/audit/M27D_IMPLEMENTATION_RECORD.md",
        "apps/root-task/src/net/mod.rs",
    ):
        with pytest.raises(
            publication.evidence.EvidenceError, match="contract changed"
        ):
            publication.classify_change(tmp_path, tmp_path, path, False)


@pytest.mark.parametrize("profile", [
    "qemu_smp_production", "qemu_smp_kvm_production", "qemu_smp_diagnostic",
])
def test_python_wheel_inspection_binds_the_selected_native_profile(tmp_path, profile):
    """Exercise the package inspection used on both native build hosts."""
    wheel = tmp_path / "cohesix-0.1.0-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        for module in (
            "__init__", "backends", "client", "evidence", "generated",
            "orchestration", "playbooks", "receipts", "worker",
        ):
            archive.writestr(f"cohesix/{module}.py", "")
        archive.writestr("cohesix-0.1.0.dist-info/METADATA", (
            "Metadata-Version: 2.1\nName: cohesix\nVersion: 0.1.0\n"
            "Requires-Python: >=3.11\nProvides-Extra: dev\n"
            "Provides-Extra: integrations\nProvides-Extra: ml\n"
        ))
        archive.writestr("cohesix-0.1.0.dist-info/entry_points.txt", (
            "[console_scripts]\ncohesix-playbook = cohesix.playbook_cli:main\n"
        ))
    qemu = tmp_path / "qemu.json"
    pi4 = tmp_path / "pi4.json"
    qemu.write_text(json.dumps({"target": "qemu", "target_profile": profile}))
    pi4.write_text(json.dumps({"target": "pi4", "target_profile": "pi4_production"}))
    source = (ROOT / "scripts/ci/python_compat_run.sh").read_text()
    function = "inspect_wheel() {" + source.split("inspect_wheel() {", 1)[1].split(
        "\nrun_smoke() {", 1
    )[0]
    output = tmp_path / "inspection.json"
    result = subprocess.run([
        "bash", "-c", 'interpreters=("$1"); wheel=$2; qemu_contract=$3; pi4_contract=$4\n'
        + function + '\ninspect_wheel "$5"', "wheel-inspection-test",
        sys.executable, str(wheel), str(qemu), str(pi4), str(output),
    ], capture_output=True, text=True)
    if profile == "qemu_smp_diagnostic":
        assert result.returncode != 0
        assert "profile contract identity mismatch" in result.stderr
    else:
        assert result.returncode == 0, result.stderr
        record = json.loads(output.read_text())["profile_contracts"]["qemu"]
        assert record["target_profile"] == profile
        assert record["sha256"] == hashlib.sha256(qemu.read_bytes()).hexdigest()


@pytest.mark.parametrize("suffix", ["MacOS", "linux", "Pi4"])
def test_each_archive_preserves_quickstart_and_resolves_its_links(
    tmp_path: Path,
    suffix: str,
) -> None:
    """Exercise the actual packaging function against selected release documents."""
    release = tomllib.loads(
        (ROOT / "configs/implementation_surfaces.toml").read_text()
    )["release"]
    bundle = tmp_path / f"Cohesix-{release['version']}-{suffix}"
    for source in release["public_documents"]:
        destination = "QUICKSTART.md" if source == "docs/QUICKSTART.md" else source
        path = bundle / destination
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / source, path)

    source = SCRIPT.read_text()
    function = source.split("prepare_bundle_quickstart() {", 1)[1].split(
        "\nbundle_release() {", 1
    )[0]
    result = subprocess.run(
        [
            "bash",
            "-c",
            "prepare_bundle_quickstart() {"
            + function
            + '\nprepare_bundle_quickstart "$1"',
            "quickstart-test",
            str(bundle),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    packaged = (bundle / "QUICKSTART.md").read_text()
    original = (ROOT / "docs/QUICKSTART.md").read_text()
    # Only link destinations may differ: no platform loses its actual steps.
    links = r"\[([^]\n]+)\]\(([^)\s]+)\)"
    assert re.sub(links, r"\1", packaged) == re.sub(links, r"\1", original)
    for _, target in re.findall(links, packaged):
        if target.startswith(("#", "http:", "https:", "mailto:")):
            continue
        assert (bundle / target.split("#", 1)[0]).is_file(), target
    assert "(QUICKSTART.md)" in (bundle / "README.md").read_text()
    assert "docs/QUICKSTART.md" not in (bundle / "README.md").read_text()
    for document in ("HOST_TOOLS.md", "HARDWARE_BRINGUP.md"):
        assert "](../QUICKSTART.md)" in (bundle / "docs" / document).read_text()

    archive = tmp_path / f"{bundle.name}.tar.gz"
    with tarfile.open(archive, "w:gz") as handle:
        handle.add(bundle, arcname=bundle.name)
    with tarfile.open(archive, "r:gz") as handle:
        quickstart = handle.extractfile(f"{bundle.name}/QUICKSTART.md")
        assert quickstart is not None
        assert quickstart.read().decode() == packaged
    assert source.count('prepare_bundle_quickstart "$bundle_dir"') == 2


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
    assert (
        "configs/generated/cohesix_python_qemu_smp_production.json"
        in release["generated_configs"]
    )
    assert (
        "configs/generated/cohesix_python_pi4_production.json"
        in release["generated_configs"]
    )
    assert "configs/generated/root_task_topology.json" in release["generated_configs"]
    assert "tools/cohesix-py/cohesix/worker.py" in release["python_artifacts"]
    assert "tests/fixtures/cas/max_chunks_v1.txt" in release["cas_fixtures"]
    assert "cas/max_chunks_v1.txt.sha256" in release["generated_bundle_files"]
    assert (
        "python/dist/cohesix-0.2.0a2-py3-none-any.whl"
        in release["generated_bundle_files"]
    )


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
    assert 'expected_pi4 = set(release["pi4_stage_files"])' in source
    assert "validate_pi4_stage_identity" in source
    assert "Pi 4 SD staging set drift" in source


def test_release_creates_peer_pi4_bundle_with_portable_image() -> None:
    source = SCRIPT.read_text(encoding="utf-8")
    inventory = tomllib.loads(
        (ROOT / "configs/implementation_surfaces.toml").read_text(encoding="utf-8")
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
    for forbidden in ("merlin2.local", "/mnt/nvme", "LINUX_SYNC_USER:-ubuntu"):
        assert forbidden not in source
    assert "archive-bundle" in source
    assert "cohesix-linux-host-tools-build/v1" in source
    assert "archive_args+=(--force)" in source
    assert 'BUNDLE_DIR="$bundle_dir" python3 -' in source
    # Relative documentation links may name releases; the extraction destination
    # must come from the selected bundle directory, including custom locations.
    assert 'bundle = Path(os.environ["BUNDLE_DIR"])' in source


@pytest.mark.parametrize(
    "system,host,profile,timer,accel,cpu",
    [
        ("Darwin", "macos", "qemu_smp_production", 24000000, "hvf", "cortex-a57"),
        ("Linux", "linux", "qemu_smp_kvm_production", 31250000, "kvm", "host"),
    ],
)
def test_packaged_launcher_uses_the_native_profile_without_kvm_timer_override(
    tmp_path: Path,
    system: str,
    host: str,
    profile: str,
    timer: int,
    accel: str,
    cpu: str,
) -> None:
    """Inspect actual generated launcher argv using controlled host/QEMU fixtures."""
    source = SCRIPT.read_text()
    start = source.index("  cat <<'EOF' >")
    body = source[source.index("\n", start) + 1 : source.index("\nEOF", start)]
    # Model only availability of the KVM device, not scheduling or guest behavior.
    body = body.replace(
        'QEMU_ACCEL="$(resolve_qemu_accel)"',
        'has_kvm_device() { return 0; }\nQEMU_ACCEL="$(resolve_qemu_accel)"',
    )
    runner = tmp_path / "qemu/run.sh"
    runner.parent.mkdir()
    runner.write_text(body)
    image = tmp_path / "image"
    image.mkdir()
    for name in ("elfloader", "kernel.elf", "rootserver", "cohesix-system.cpio"):
        (image / name).write_bytes(b"guest fixture")
    (image / "gic-version.txt").write_text("3\n")
    (tmp_path / "BUILD_PROVENANCE.json").write_text(
        json.dumps(
            {
                "host": host,
                "sel4_profile": profile,
                "timer_clock_hz": timer,
            }
        )
    )
    binaries = tmp_path / "test-bin"
    binaries.mkdir()
    uname = binaries / "uname"
    uname.write_text(f"#!/bin/sh\nprintf '%s\\n' '{system}'\n")
    uname.chmod(0o755)
    qemu = binaries / "qemu-system-aarch64"
    qemu.write_text(
        "#!/usr/bin/env python3\nimport json, sys\n"
        "if sys.argv[1:] == ['-accel', 'help']: print('hvf kvm tcg')\n"
        "else: print('ARGV=' + json.dumps(sys.argv[1:]))\n"
    )
    qemu.chmod(0o755)
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(("QEMU_", "COHESIX_QEMU_"))
    }
    env.update(PATH=f"{binaries}:{env['PATH']}", QEMU_BIN=str(qemu), QEMU_ACCEL=accel)
    result = subprocess.run(
        ["bash", str(runner)], env=env, capture_output=True, text=True
    )
    assert result.returncode == 0, result.stderr
    argv = json.loads(
        next(
            line.removeprefix("ARGV=")
            for line in result.stdout.splitlines()
            if line.startswith("ARGV=")
        )
    )
    assert argv[argv.index("-cpu") + 1] == cpu
    assert argv[argv.index("-accel") + 1] == accel
    assert argv[argv.index("-smp") + 1] == "4,cores=4,threads=1,sockets=1"
    assert all("cntfrq=" not in arg for arg in argv)
