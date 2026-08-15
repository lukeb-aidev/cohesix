# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Verify immutable same-artifact QEMU launch records.
# Author: Lukas Bower

"""Tests for the QEMU launch-artifact identity helper."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import stat
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "lib" / "qemu_launch_artifacts.py"
SPEC = importlib.util.spec_from_file_location("qemu_launch_artifacts", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
launch_artifacts = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = launch_artifacts
SPEC.loader.exec_module(launch_artifacts)


def _launch_tree(tmp_path: Path) -> tuple[Path, Path, Path]:
    out_dir = tmp_path / "out" / "cohesix"
    sel4_build = tmp_path / "out" / "sel4" / "qemu-smp-production"
    out_dir.mkdir(parents=True)
    sel4_build.mkdir(parents=True)
    timer_header = sel4_build / "kernel/gen_headers/plat/platform_gen.h"
    timer_header.parent.mkdir(parents=True)
    timer_header.write_text(
        "#define TIMER_CLOCK_HZ ULL_CONST(24000000)\n",
        encoding="utf-8",
    )
    for index, (_artifact_id, relative) in enumerate(
        launch_artifacts.ARTIFACTS,
        start=1,
    ):
        path = out_dir / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(bytes([index]) * (64 + index))
    qemu = tmp_path / "qemu-system-aarch64"
    qemu.write_text(
        "#!/bin/sh\n"
        "if [ \"$1\" = \"--version\" ]; then "
        "printf 'QEMU emulator version 10.1.0\\n'; exit 0; fi\n"
        "if [ \"$1\" = \"-accel\" ]; then "
        "printf 'Accelerators: hvf kvm tcg\\n'; exit 0; fi\n",
        encoding="utf-8",
    )
    qemu.chmod(qemu.stat().st_mode | stat.S_IXUSR)
    return out_dir, sel4_build, qemu


def _write(out_dir: Path, sel4_build: Path, qemu: Path) -> Path:
    return launch_artifacts.write_record(
        out_dir=out_dir,
        sel4_build_dir=sel4_build,
        profile="release",
        cargo_target="aarch64-unknown-none",
        root_task_features="release-qemu,bootstrap-trace",
        gic_version="3",
        sel4_profile="qemu_smp_production",
        qemu=str(qemu),
        accelerator="hvf",
        virtualization="off",
        machine_extra="kernel-irqchip=off",
        cpu="cortex-a57",
        smp="4,cores=4,threads=1,sockets=1",
        net_backend="virtio",
    )


def _verify(out_dir: Path, sel4_build: Path, qemu: Path) -> Path:
    return launch_artifacts.verify_record(
        out_dir=out_dir,
        sel4_build_dir=sel4_build,
        profile="release",
        cargo_target="aarch64-unknown-none",
        root_task_features="release-qemu,bootstrap-trace",
        gic_version="3",
        sel4_profile="qemu_smp_production",
        qemu=str(qemu),
        accelerator="hvf",
        virtualization="off",
        machine_extra="kernel-irqchip=off",
        cpu="cortex-a57",
        smp="4,cores=4,threads=1,sockets=1",
        net_backend="virtio",
    )


def test_launch_record_binds_exact_qemu_inputs_and_context(tmp_path: Path) -> None:
    out_dir, sel4_build, qemu = _launch_tree(tmp_path)
    record = _write(out_dir, sel4_build, qemu)

    assert _verify(out_dir, sel4_build, qemu) == record
    document = json.loads(record.read_text(encoding="utf-8"))
    assert document["schema"] == launch_artifacts.SCHEMA
    assert [row["id"] for row in document["artifacts"]] == [
        "elfloader",
        "kernel",
        "rootserver",
        "initrd",
    ]
    assert all(len(row["sha256"]) == 64 for row in document["artifacts"])
    assert document["qemu"]["binary"]["version"] == (
        "QEMU emulator version 10.1.0"
    )
    assert document["qemu"]["timer_clock_hz"] == 24_000_000
    assert document["claim"] == {
        "eligible": True,
        "tier": "qemu-integration",
        "reason": "canonical production envelope",
    }


def test_launch_record_rejects_artifact_and_context_drift(tmp_path: Path) -> None:
    out_dir, sel4_build, qemu = _launch_tree(tmp_path)
    _write(out_dir, sel4_build, qemu)
    (out_dir / "cohesix-system.cpio").write_bytes(b"tampered")

    with pytest.raises(
        launch_artifacts.LaunchArtifactError,
        match="identity mismatch: initrd",
    ):
        _verify(out_dir, sel4_build, qemu)

    out_dir, sel4_build, qemu = _launch_tree(tmp_path / "context")
    _write(out_dir, sel4_build, qemu)
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
            sel4_profile="qemu_smp_production",
            qemu=str(qemu),
            accelerator="hvf",
            virtualization="off",
            machine_extra="kernel-irqchip=off",
            cpu="cortex-a57",
            smp="4,cores=4,threads=1,sockets=1",
            net_backend="virtio",
        )


def test_launch_record_rejects_symlinks_and_types_non_gicv3_diagnostic(
    tmp_path: Path,
) -> None:
    out_dir, sel4_build, qemu = _launch_tree(tmp_path)
    elfloader = out_dir / "staging" / "elfloader"
    target = out_dir / "staging" / "elfloader.real"
    elfloader.rename(target)
    elfloader.symlink_to(target.name)

    with pytest.raises(
        launch_artifacts.LaunchArtifactError,
        match="path is a symlink",
    ):
        _write(out_dir, sel4_build, qemu)

    elfloader.unlink()
    target.rename(elfloader)
    record = launch_artifacts.write_record(
        out_dir=out_dir,
        sel4_build_dir=sel4_build,
        profile="release",
        cargo_target="aarch64-unknown-none",
        root_task_features="release-qemu,bootstrap-trace",
        gic_version="2",
        sel4_profile="qemu_smp_production",
        qemu=str(qemu),
        accelerator="hvf",
        virtualization="off",
        machine_extra="kernel-irqchip=off",
        cpu="cortex-a57",
        smp="4,cores=4,threads=1,sockets=1",
        net_backend="virtio",
    )
    claim = json.loads(record.read_text(encoding="utf-8"))["claim"]
    assert claim["tier"] == "qemu-diagnostic"
    assert "does not use GICv3" in claim["reason"]


def test_launch_record_types_tcg_as_claim_ineligible_diagnostic(
    tmp_path: Path,
) -> None:
    out_dir, sel4_build, qemu = _launch_tree(tmp_path)
    record = launch_artifacts.write_record(
        out_dir=out_dir,
        sel4_build_dir=sel4_build,
        profile="release",
        cargo_target="aarch64-unknown-none",
        root_task_features="release-qemu,bootstrap-trace",
        gic_version="3",
        sel4_profile="qemu_smp_production",
        qemu=str(qemu),
        accelerator="tcg",
        virtualization="off",
        machine_extra="kernel-irqchip=off",
        cpu="cortex-a57",
        smp="4,cores=4,threads=1,sockets=1",
        net_backend="virtio",
    )

    claim = json.loads(record.read_text(encoding="utf-8"))["claim"]
    assert claim["eligible"] is False
    assert claim["tier"] == "qemu-diagnostic"
    assert "TCG is diagnostic-only" in claim["reason"]


def test_launch_record_rejects_qemu_binary_and_timer_drift(tmp_path: Path) -> None:
    out_dir, sel4_build, qemu = _launch_tree(tmp_path)
    _write(out_dir, sel4_build, qemu)
    qemu.write_text("#!/bin/sh\nprintf 'changed\\n'\n", encoding="utf-8")

    with pytest.raises(
        launch_artifacts.LaunchArtifactError,
        match="cannot determine QEMU version|context mismatch: qemu|not advertised",
    ):
        _verify(out_dir, sel4_build, qemu)

    out_dir, sel4_build, qemu = _launch_tree(tmp_path / "timer")
    timer_header = sel4_build / "kernel/gen_headers/plat/platform_gen.h"
    timer_header.write_text(
        "#define TIMER_CLOCK_HZ ULL_CONST(62500000)\n",
        encoding="utf-8",
    )
    record = _write(out_dir, sel4_build, qemu)
    document = json.loads(record.read_text(encoding="utf-8"))
    assert document["claim"]["eligible"] is False
    assert "not 24 MHz" in document["claim"]["reason"]
