# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Verify immutable same-artifact QEMU launch records.
# Author: Lukas Bower

"""Tests for the QEMU launch-artifact identity helper."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "lib" / "qemu_launch_artifacts.py"
SPEC = importlib.util.spec_from_file_location("qemu_launch_artifacts", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
launch_artifacts = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = launch_artifacts
SPEC.loader.exec_module(launch_artifacts)


def _launch_tree(tmp_path: Path) -> tuple[Path, Path]:
    out_dir = tmp_path / "out" / "cohesix"
    sel4_build = tmp_path / "out" / "sel4" / "qemu-smp-production"
    out_dir.mkdir(parents=True)
    sel4_build.mkdir(parents=True)
    for index, (_artifact_id, relative) in enumerate(
        launch_artifacts.ARTIFACTS,
        start=1,
    ):
        path = out_dir / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(bytes([index]) * (64 + index))
    return out_dir, sel4_build


def _write(out_dir: Path, sel4_build: Path) -> Path:
    return launch_artifacts.write_record(
        out_dir=out_dir,
        sel4_build_dir=sel4_build,
        profile="release",
        cargo_target="aarch64-unknown-none",
        root_task_features="release-qemu,bootstrap-trace",
        gic_version="3",
    )


def _verify(out_dir: Path, sel4_build: Path) -> Path:
    return launch_artifacts.verify_record(
        out_dir=out_dir,
        sel4_build_dir=sel4_build,
        profile="release",
        cargo_target="aarch64-unknown-none",
        root_task_features="release-qemu,bootstrap-trace",
        gic_version="3",
    )


def test_launch_record_binds_exact_qemu_inputs_and_context(tmp_path: Path) -> None:
    out_dir, sel4_build = _launch_tree(tmp_path)
    record = _write(out_dir, sel4_build)

    assert _verify(out_dir, sel4_build) == record
    document = json.loads(record.read_text(encoding="utf-8"))
    assert document["schema"] == launch_artifacts.SCHEMA
    assert [row["id"] for row in document["artifacts"]] == [
        "elfloader",
        "kernel",
        "rootserver",
        "initrd",
    ]
    assert all(len(row["sha256"]) == 64 for row in document["artifacts"])


def test_launch_record_rejects_artifact_and_context_drift(tmp_path: Path) -> None:
    out_dir, sel4_build = _launch_tree(tmp_path)
    _write(out_dir, sel4_build)
    (out_dir / "cohesix-system.cpio").write_bytes(b"tampered")

    with pytest.raises(
        launch_artifacts.LaunchArtifactError,
        match="identity mismatch: initrd",
    ):
        _verify(out_dir, sel4_build)

    out_dir, sel4_build = _launch_tree(tmp_path / "context")
    _write(out_dir, sel4_build)
    with pytest.raises(
        launch_artifacts.LaunchArtifactError,
        match="context mismatch: root_task_features",
    ):
        launch_artifacts.verify_record(
            out_dir=out_dir,
            sel4_build_dir=sel4_build,
            profile="release",
            cargo_target="aarch64-unknown-none",
            root_task_features="release-qemu",
            gic_version="3",
        )


def test_launch_record_rejects_symlinks_and_non_gicv3(tmp_path: Path) -> None:
    out_dir, sel4_build = _launch_tree(tmp_path)
    elfloader = out_dir / "staging" / "elfloader"
    target = out_dir / "staging" / "elfloader.real"
    elfloader.rename(target)
    elfloader.symlink_to(target.name)

    with pytest.raises(
        launch_artifacts.LaunchArtifactError,
        match="path is a symlink",
    ):
        _write(out_dir, sel4_build)

    elfloader.unlink()
    target.rename(elfloader)
    with pytest.raises(
        launch_artifacts.LaunchArtifactError,
        match="require GICv3",
    ):
        launch_artifacts.write_record(
            out_dir=out_dir,
            sel4_build_dir=sel4_build,
            profile="release",
            cargo_target="aarch64-unknown-none",
            root_task_features="release-qemu,bootstrap-trace",
            gic_version="2",
        )
