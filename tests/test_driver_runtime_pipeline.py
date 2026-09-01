# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Verify deterministic MCS driver archives and fail-closed comparator binding.
# Author: Lukas Bower

"""Tests for the Milestone 26e linked-driver archive pipeline."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "driver_runtime_manifest.py"
SPEC = importlib.util.spec_from_file_location("driver_runtime_manifest", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
driver_images = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = driver_images
SPEC.loader.exec_module(driver_images)

COMPARATOR = "5a" * 32
COMPARATOR_RECORD = "6b" * 32


def test_manifest_abi_matches_rust_runtime_contract() -> None:
    abi_source = (ROOT / "crates" / "pi4-driver-abi" / "src" / "lib.rs").read_text(
        encoding="utf-8"
    )
    prefix = "pub const DRIVER_RUNTIME_INIT_VERSION: u16 = "
    declarations = [
        line.strip() for line in abi_source.splitlines() if line.startswith(prefix)
    ]

    assert len(declarations) == 1
    rust_version = int(declarations[0].removeprefix(prefix).removesuffix(";"))
    assert driver_images.RUNTIME_INIT_ABI_VERSION == rust_version


def test_repository_classic_comparator_record_is_exact_and_immutable() -> None:
    comparator, record = driver_images.load_comparator_record(
        ROOT / "configs" / "driver_runtime_classic_comparator.toml"
    )

    assert comparator == "db2e353327cde2f91b37f40a7bf17905bb5f70cd27a999ba880a9fa7c2de9835"
    assert len(record) == 64


def test_classic_comparator_record_rejects_component_tampering(tmp_path: Path) -> None:
    source = ROOT / "configs" / "driver_runtime_classic_comparator.toml"
    tampered = source.read_text(encoding="utf-8").replace(
        'name = "pi4-driver-serial"',
        'name = "pi4-driver-cyw43"',
        1,
    )
    path = tmp_path / "comparator.toml"
    path.write_text(tampered, encoding="utf-8")

    with pytest.raises(
        driver_images.DriverRuntimeManifestError,
        match="component identity",
    ):
        driver_images.load_comparator_record(path)


def _image_dir(tmp_path: Path) -> Path:
    image_dir = tmp_path / "images"
    image_dir.mkdir(parents=True)
    for index, (name, _hot_path) in enumerate(driver_images.COMPONENTS, start=1):
        (image_dir / name).write_bytes(b"\x7fELF" + bytes([index]) * (64 + index))
    return image_dir


def _build(tmp_path: Path) -> tuple[Path, Path]:
    archive = tmp_path / driver_images.ARCHIVE_NAME
    manifest = tmp_path / "cohesix-driver-runtime-manifest.json"
    driver_images.build_manifest(
        _image_dir(tmp_path),
        archive,
        manifest,
        "aarch64-unknown-none",
        "release",
        COMPARATOR,
        COMPARATOR_RECORD,
    )
    return archive, manifest


def test_archive_is_deterministic_complete_and_comparator_bound(tmp_path: Path) -> None:
    archive, manifest = _build(tmp_path)
    archive_bytes = archive.read_bytes()
    manifest_bytes = manifest.read_bytes()
    driver_images.build_manifest(
        tmp_path / "images",
        archive,
        manifest,
        "aarch64-unknown-none",
        "release",
        COMPARATOR,
        COMPARATOR_RECORD,
    )
    assert archive.read_bytes() == archive_bytes
    assert manifest.read_bytes() == manifest_bytes
    document = driver_images.verify_manifest(manifest, archive)
    assert [row["name"] for row in document["components"]] == [
        name for name, _hot_path in driver_images.COMPONENTS
    ]
    assert document["classic_comparator"]["sha256"] == COMPARATOR
    assert document["classic_comparator"]["record_sha256"] == COMPARATOR_RECORD
    assert document["runtime_init_abi_version"] == 13


def test_missing_or_noncanonical_comparator_fails_closed(tmp_path: Path) -> None:
    image_dir = _image_dir(tmp_path)
    for comparator in ("", "A" * 64, "not-a-sha256"):
        with pytest.raises(
            driver_images.DriverRuntimeManifestError, match="classic comparator"
        ):
            driver_images.build_manifest(
                image_dir,
                tmp_path / "archive",
                tmp_path / "manifest",
                "aarch64-unknown-none",
                "release",
                comparator,
                COMPARATOR_RECORD,
            )


def test_archive_and_component_manifest_tampering_fail(tmp_path: Path) -> None:
    archive, manifest = _build(tmp_path)
    tampered = bytearray(archive.read_bytes())
    tampered[512] ^= 1
    archive.write_bytes(tampered)
    with pytest.raises(driver_images.DriverRuntimeManifestError, match="identity"):
        driver_images.verify_manifest(manifest, archive)

    archive, manifest = _build(tmp_path / "second")
    document = json.loads(manifest.read_text(encoding="utf-8"))
    document["components"][0]["hot_path"] = "sdio-host"
    manifest.write_text(json.dumps(document), encoding="utf-8")
    with pytest.raises(driver_images.DriverRuntimeManifestError, match="component identity"):
        driver_images.verify_manifest(manifest, archive)


def test_qemu_build_keeps_driver_and_worker_archives_separate() -> None:
    script = (ROOT / "scripts" / "cohesix-build-run.sh").read_text(encoding="utf-8")
    build_script = (ROOT / "apps" / "root-task" / "build.rs").read_text(
        encoding="utf-8"
    )
    linker_script = (ROOT / "apps" / "root-task" / "sel4.ld").read_text(
        encoding="utf-8"
    )
    assert "driver_runtime_classic_comparator.toml" in script
    assert 'python3 "$DRIVER_MANIFEST_TOOL" build' in script
    assert 'COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD="$DRIVER_ARCHIVE_PATH"' in script
    assert 'install -m 0644 "$DRIVER_ARCHIVE_PATH" "$ARTIFACTS_DIR' not in script
    assert '"archive_path": "driver-runtimes/cohesix-driver-runtimes.cpio"' in script
    assert '"archive_path_scope": "build-output"' in script
    assert '"embedded_in_rootserver": True' in script
    assert "rootfs must not duplicate the rootserver-embedded driver archive" in script
    assert "cohesix/artifacts/cohesix-driver-runtime-manifest.json" in script
    assert "cohesix/artifacts/cohesix-worker-images.cpio" in script
    assert '"#[used]\\n\\' in build_script
    assert '.cohesix_driver_runtime_payload' in build_script
    assert "[u8; include_bytes!" in build_script
    assert "KEEP(*(.cohesix_driver_runtime_payload))" in linker_script
    driver_build = script.index('python3 "$DRIVER_MANIFEST_TOOL" build')
    root_build = script.index('COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD="$DRIVER_ARCHIVE_PATH"')
    assert driver_build < root_build


def test_qemu_build_rejects_external_output_and_target_directories(tmp_path: Path) -> None:
    script = ROOT / "scripts" / "cohesix-build-run.sh"
    common = [
        "bash",
        str(script),
        "--cargo-target",
        "aarch64-unknown-none",
        "--no-run",
    ]

    outside_output = subprocess.run(
        [*common, "--out-dir", str(tmp_path / "outside-output")],
        cwd=tmp_path,
        check=False,
        capture_output=True,
        text=True,
    )
    assert outside_output.returncode == 1
    assert "output directory must be a resolved child" in outside_output.stderr

    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = str(tmp_path / "outside-target")
    outside_target = subprocess.run(
        common,
        cwd=tmp_path,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )
    assert outside_target.returncode == 1
    assert (
        "CARGO_TARGET_DIR must resolve to the repository target directory"
        in outside_target.stderr
    )


def test_qemu_build_cleans_only_resolved_bounded_directories() -> None:
    script = (ROOT / "scripts" / "cohesix-build-run.sh").read_text(encoding="utf-8")
    assert 'cd "$PROJECT_ROOT"' in script
    assert '"$PROJECT_ROOT"/out/*)' in script
    assert '[[ -L "$OUT_DIR" ]]' in script
    assert '[[ -L "$CARGO_TARGET_DIR" ]]' in script
    assert 'find "$STAGING_DIR" -depth -mindepth 1 -delete' in script
    assert 'rm -rf "$STAGING_DIR"' not in script


def test_qemu_repeated_runs_launch_one_immutable_artifact_set() -> None:
    script = (ROOT / "scripts" / "cohesix-build-run.sh").read_text(encoding="utf-8")
    verify = 'python3 "$LAUNCH_ARTIFACT_TOOL" verify'
    write = 'python3 "$LAUNCH_ARTIFACT_TOOL" write'
    regenerate = 'cargo run -p coh-rtc -- \\\n'

    assert "--launch-existing" in script
    assert verify in script
    assert write in script
    assert script.index(verify) < script.index(regenerate)
    assert script.index(write) > script.index('cpio -o -H newc > "$CPIO_PATH"')
    assert '"$LAUNCH_EXISTING" -eq 1 && "$CLEAN_OUT_DIR" -eq 1' in script
    assert "must not replace immutable QEMU inputs or topology" in script
