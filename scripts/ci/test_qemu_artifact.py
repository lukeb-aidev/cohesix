#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Test content-addressed QEMU artifact and transport evidence handling.
# Copyright 2026 Lukas Bower

"""Focused tests for scripts/ci/qemu_artifact.py."""

from __future__ import annotations

import importlib.util
import hashlib
import json
from pathlib import Path
import shutil
import socket
import stat
import subprocess
from types import ModuleType

import pytest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "qemu_artifact.py"
REGRESSION_RUNNER_PATH = REPO_ROOT / "scripts" / "cohsh" / "run_regression_batch.sh"
CATALOG_DIGEST = "sha256:" + ("e" * 64)


def load_module() -> ModuleType:
    """Load the helper directly from its script path."""

    spec = importlib.util.spec_from_file_location("qemu_artifact", SCRIPT_PATH)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(name="helper")
def fixture_helper() -> ModuleType:
    """Return the helper module under test."""

    return load_module()


def write_executable(path: Path, body: str) -> None:
    """Write one executable fixture."""

    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def create_artifact_inputs(tmp_path: Path) -> dict[str, Path]:
    """Create the minimum canonical artifact and evidence inputs."""

    artifact = tmp_path / "artifact"
    for relative in (
        "staging/elfloader",
        "staging/kernel.elf",
        "staging/rootserver",
        "cohesix-system.cpio",
        "host-tools/cohsh",
        "host-tools/hive-gateway",
        "host-tools/coh",
        "host-tools/cas-tool",
        "host-tools/gpu-bridge-host",
        "host-tools/host-sidecar-bridge",
        "host-tools/host-ticket-agent",
        "host-tools/swarmui",
    ):
        path = artifact / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(f"fixture:{relative}".encode())

    sel4 = tmp_path / "sel4"
    config = sel4 / "kernel" / "gen_config" / "kernel_config.h"
    config.parent.mkdir(parents=True)
    config.write_text("#define CONFIG_ARM_GIC_V3 1\n", encoding="utf-8")
    timer_header = sel4 / "kernel" / "gen_headers" / "plat" / "platform_gen.h"
    timer_header.parent.mkdir(parents=True)
    timer_header.write_text(
        "#define TIMER_CLOCK_HZ ULL_CONST(24000000)\n",
        encoding="utf-8",
    )

    qemu = tmp_path / "qemu-system-aarch64"
    write_executable(
        qemu,
        "#!/bin/sh\n"
        "if [ \"$1\" = \"--version\" ]; then "
        "printf 'QEMU emulator version 10.1.0\\n'; exit 0; fi\n"
        "if [ \"$1\" = \"-accel\" ]; then "
        "printf 'Accelerators: hvf kvm tcg\\n'; exit 0; fi\n",
    )

    detector = tmp_path / "detect-gic"
    write_executable(detector, "#!/bin/sh\nprintf '3\\n'\n")
    manifest = tmp_path / "root_task.toml"
    manifest.write_text("[system]\nname = \"test\"\n", encoding="utf-8")
    resolved = tmp_path / "root_task_resolved.json"
    resolved.write_text('{"system":{"name":"test"}}\n', encoding="utf-8")
    policy = tmp_path / "cohsh_policy.toml"
    policy.write_text("manifest_hash = \"test\"\n", encoding="utf-8")
    attempt = tmp_path / "attempt.json"
    attempt.write_text('{"schema":"attempt-v1"}\n', encoding="utf-8")
    return {
        "artifact": artifact,
        "sel4": sel4,
        "detector": detector,
        "manifest": manifest,
        "resolved": resolved,
        "policy": policy,
        "attempt": attempt,
        "qemu": qemu,
    }


def record_artifact(
    helper: ModuleType,
    inputs: dict[str, Path],
    output: Path,
    *,
    accelerator: str | None = None,
) -> str:
    """Record one fixture artifact and return its ID."""

    source_digest = "sha256:" + ("a" * 64)
    if accelerator is None:
        accelerator = "hvf" if helper.platform.system() == "Darwin" else "kvm"
    qemu_raw = inputs["qemu"].read_bytes()
    launch_rows = []
    for identifier, relative in (
        ("elfloader", Path("staging/elfloader")),
        ("kernel", Path("staging/kernel.elf")),
        ("rootserver", Path("staging/rootserver")),
        ("initrd", Path("cohesix-system.cpio")),
    ):
        raw = (inputs["artifact"] / relative).read_bytes()
        launch_rows.append(
            {
                "id": identifier,
                "path": relative.as_posix(),
                "bytes": len(raw),
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )
    claim = helper.qemu_claim(
        host_system=helper.platform.system(),
        accelerator=accelerator,
        sel4_profile="qemu_smp_production",
        machine="virt",
        gic_version="3",
        virtualization="off",
        machine_extra="kernel-irqchip=off",
        cpu="cortex-a57",
        timer_clock_hz=24_000_000,
        smp="4,cores=4,threads=1,sockets=1",
        net_backend="virtio",
    )
    launch_record = {
        "schema": "cohesix-qemu-launch-artifacts/v2",
        "profile": "release",
        "cargo_target": "aarch64-unknown-none",
        "root_task_features": "cohesix-dev",
        "sel4_build_dir": str(inputs["sel4"].resolve()),
        "sel4_profile": "qemu_smp_production",
        "gic_version": "3",
        "qemu": {
            "host_system": helper.platform.system(),
            "binary": {
                "path": str(inputs["qemu"].resolve()),
                "bytes": len(qemu_raw),
                "sha256": hashlib.sha256(qemu_raw).hexdigest(),
                "version": "QEMU emulator version 10.1.0",
            },
            "accelerator": accelerator,
            "machine": "virt",
            "virtualization": "off",
            "machine_extra": "kernel-irqchip=off",
            "cpu": "cortex-a57",
            "timer_clock_hz": 24_000_000,
            "smp": "4,cores=4,threads=1,sockets=1",
            "net_backend": "virtio",
        },
        "claim": claim,
        "artifacts": launch_rows,
    }
    (inputs["artifact"] / "cohesix-qemu-launch-artifacts.json").write_text(
        json.dumps(launch_record),
        encoding="utf-8",
    )
    status = helper.main(
        [
            "record",
            "--artifact-dir",
            str(inputs["artifact"]),
            "--output",
            str(output),
            "--manifest",
            str(inputs["manifest"]),
            "--resolved-manifest",
            str(inputs["resolved"]),
            "--policy",
            str(inputs["policy"]),
            "--source-digest",
            source_digest,
            "--attempt-manifest",
            str(inputs["attempt"]),
            "--sel4-build",
            str(inputs["sel4"]),
            "--sel4-profile",
            "qemu_smp_production",
            "--qemu",
            str(inputs["qemu"]),
            "--accelerator",
            accelerator,
            "--root-task-features",
            "cohesix-dev",
            "--cargo-target",
            "aarch64-unknown-none",
            "--smp",
            "4,cores=4,threads=1,sockets=1",
            "--virtualization",
            "off",
            "--machine-extra",
            "kernel-irqchip=off",
            "--net-backend",
            "virtio",
            "--detect-gic-script",
            str(inputs["detector"]),
            "--action-id",
            "stage-03-qemu-tcp",
            "--catalog-action-digest",
            CATALOG_DIGEST,
        ]
    )
    assert status == 0
    return json.loads(output.read_text(encoding="utf-8"))["artifact_id"]


def test_record_is_content_addressed_and_verify_fails_after_mutation(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """An artifact ID covers every runtime file and active source digest."""

    inputs = create_artifact_inputs(tmp_path)
    manifest = tmp_path / "qemu-artifact.json"
    artifact_id = record_artifact(helper, inputs, manifest)
    capsys.readouterr()
    document = json.loads(manifest.read_text(encoding="utf-8"))
    assert document["sel4"]["profile"] == "qemu_smp_production"
    assert document["sel4"]["timer_clock_hz"] == 24_000_000
    assert document["qemu"]["machine"] == "virt"
    assert document["qemu"]["gic_version"] == "3"
    assert document["qemu"]["virtualization"] == "off"
    assert document["qemu"]["machine_extra"] == "kernel-irqchip=off"
    assert document["qemu"]["cpu"] == "cortex-a57"
    assert document["qemu"]["binary"]["version"] == (
        "QEMU emulator version 10.1.0"
    )

    assert helper.main(
        [
            "verify",
            "--artifact-manifest",
            str(manifest),
            "--source-digest",
            "sha256:" + ("a" * 64),
            "--action-id",
            "stage-03-qemu-tcp",
            "--catalog-action-digest",
            CATALOG_DIGEST,
        ]
    ) == 0
    assert artifact_id in capsys.readouterr().out

    (inputs["artifact"] / "staging" / "rootserver").write_bytes(b"mutated")
    assert helper.main(
        ["verify", "--artifact-manifest", str(manifest)]
    ) == 1
    assert "artifact size mismatch" in capsys.readouterr().err


def test_verify_rejects_incomplete_duplicate_or_mutated_packaged_host_tool(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Every packaged executable remains mandatory and content-bound."""

    inputs = create_artifact_inputs(tmp_path)
    manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, manifest)
    capsys.readouterr()

    swarmui = inputs["artifact"] / "host-tools" / "swarmui"
    swarmui.write_bytes(b"mutated")
    assert helper.main(["verify", "--artifact-manifest", str(manifest)]) == 1
    assert "artifact size mismatch" in capsys.readouterr().err

    swarmui.write_bytes(b"fixture:host-tools/swarmui")
    document = json.loads(manifest.read_text(encoding="utf-8"))
    document["files"] = [
        record
        for record in document["files"]
        if record["path"] != "host-tools/swarmui"
    ]
    document["artifact_id"] = helper.sha256_bytes(
        helper.canonical_bytes(helper.artifact_identity_material(document))
    )
    manifest.write_text(json.dumps(document), encoding="utf-8")
    assert helper.main(["verify", "--artifact-manifest", str(manifest)]) == 1
    assert "missing required file records" in capsys.readouterr().err

    record_artifact(helper, inputs, manifest)
    capsys.readouterr()
    document = json.loads(manifest.read_text(encoding="utf-8"))
    duplicate = next(
        record
        for record in document["files"]
        if record["path"] == "host-tools/swarmui"
    )
    document["files"].append(duplicate)
    document["artifact_id"] = helper.sha256_bytes(
        helper.canonical_bytes(helper.artifact_identity_material(document))
    )
    manifest.write_text(json.dumps(document), encoding="utf-8")
    assert helper.main(["verify", "--artifact-manifest", str(manifest)]) == 1
    assert "duplicate artifact file record" in capsys.readouterr().err


def test_verify_rejects_qemu_binary_drift(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    inputs = create_artifact_inputs(tmp_path)
    manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, manifest)
    capsys.readouterr()
    write_executable(
        inputs["qemu"],
        "#!/bin/sh\nprintf 'QEMU emulator version 10.2.0\\n'\n",
    )

    assert helper.main(["verify", "--artifact-manifest", str(manifest)]) == 1
    assert "QEMU binary identity changed" in capsys.readouterr().err


def test_artifact_manifest_remains_valid_after_bundle_relocation(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Artifact manifests bind content without pinning an old absolute root."""

    inputs = create_artifact_inputs(tmp_path)
    manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, manifest)
    capsys.readouterr()
    document = json.loads(manifest.read_text(encoding="utf-8"))
    assert document["artifact_root"] == "artifact"

    relocated = tmp_path / "relocated"
    relocated.mkdir()
    relocated_artifact = relocated / "artifact"
    relocated_manifest = relocated / "qemu-artifact.json"
    shutil.move(str(inputs["artifact"]), relocated_artifact)
    shutil.move(str(manifest), relocated_manifest)

    assert helper.main(
        [
            "verify",
            "--artifact-manifest",
            str(relocated_manifest),
            "--source-digest",
            "sha256:" + ("a" * 64),
            "--action-id",
            "stage-03-qemu-tcp",
            "--catalog-action-digest",
            CATALOG_DIGEST,
        ]
    ) == 0


def test_attempt_roots_are_copied_and_published_without_mutating_old_runs(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Current refs move atomically while completed attempt trees remain intact."""

    state = tmp_path / "state"
    first = state / "evidence" / "attempts" / "stage-03" / "first" / "transport"
    second = state / "evidence" / "attempts" / "stage-03" / "second" / "transport"
    first.mkdir(parents=True)
    second.mkdir(parents=True)
    source = tmp_path / "target.json"
    source.write_text('{"boot_id":"boot-one"}\n', encoding="utf-8")
    copied = first / "target-evidence.json"
    pointer = state / "stage_03_artifact_root.path"
    compat = state / "current-stage-03"

    assert helper.main(
        [
            "copy-evidence",
            "--source",
            str(source),
            "--output",
            str(copied),
        ]
    ) == 0
    assert copied.read_bytes() == source.read_bytes()
    assert helper.main(
        [
            "publish-root",
            "--state-dir",
            str(state),
            "--root",
            str(first),
            "--pointer",
            str(pointer),
            "--compat-link",
            str(compat),
            "--compat-target",
            str(first),
        ]
    ) == 0
    first_pointer = pointer.read_text(encoding="utf-8")
    assert first_pointer == (
        "evidence/attempts/stage-03/first/transport\n"
    )
    assert compat.resolve() == first.resolve()

    (second / "sentinel").write_text("new\n", encoding="utf-8")
    assert helper.main(
        [
            "publish-root",
            "--state-dir",
            str(state),
            "--root",
            str(second),
            "--pointer",
            str(pointer),
            "--compat-link",
            str(compat),
            "--compat-target",
            str(second),
        ]
    ) == 0
    capsys.readouterr()
    assert copied.read_bytes() == source.read_bytes()
    assert compat.resolve() == second.resolve()
    assert helper.main(
        [
            "resolve-root",
            "--state-dir",
            str(state),
            "--pointer",
            str(pointer),
        ]
    ) == 0
    assert str(second.resolve()) in capsys.readouterr().out

    pointer.write_text("../escape\n", encoding="utf-8")
    assert helper.main(
        [
            "resolve-root",
            "--state-dir",
            str(state),
            "--pointer",
            str(pointer),
        ]
    ) == 1
    assert "unsafe artifact root pointer" in capsys.readouterr().err


def test_launch_prints_fresh_boot_command_without_rebuilding(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The launcher consumes staged files and emits canonical TCP wiring."""

    inputs = create_artifact_inputs(tmp_path)
    manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, manifest)
    capsys.readouterr()

    assert helper.main(
        [
            "launch",
            "--artifact-manifest",
            str(manifest),
            "--qemu",
            str(inputs["qemu"]),
            "--catalog-action-digest",
            CATALOG_DIGEST,
            "--console-port",
            "41001",
            "--udp-port",
            "41002",
            "--smoke-port",
            "41003",
            "--print-command",
        ]
    ) == 0
    command = capsys.readouterr().out
    assert (
        "-machine virt,gic-version=3,virtualization=off,kernel-irqchip=off"
        in command
    )
    assert "hostfwd=tcp:127.0.0.1:41001-:31337" in command
    assert "hostfwd=udp:127.0.0.1:41002-:31338" in command
    assert "staging/rootserver" in command
    assert "cohesix-build-run" not in command


def test_linux_kvm_claim_and_command_use_host_cpu_and_kernel_gic(
    helper: ModuleType,
    tmp_path: Path,
) -> None:
    claim = helper.qemu_claim(
        host_system="Linux",
        accelerator="kvm",
        sel4_profile="qemu_smp_kvm_production",
        machine="virt",
        gic_version="3",
        virtualization="off",
        machine_extra="",
        cpu="host",
        timer_clock_hz=31_250_000,
        smp="4,cores=4,threads=1,sockets=1",
        net_backend="virtio",
    )
    assert claim["eligible"] is True

    qemu = tmp_path / "qemu-system-aarch64"
    write_executable(qemu, "#!/bin/sh\nexit 0\n")
    document = {
        "_resolved_artifact_root": str(tmp_path),
        "qemu": {
            "host_system": "Linux",
            "binary": {"path": str(qemu.resolve())},
            "accelerator": "kvm",
            "machine": "virt",
            "gic_version": "3",
            "virtualization": "off",
            "machine_extra": "",
            "cpu": "host",
            "timer_clock_hz": 31_250_000,
            "smp": "4,cores=4,threads=1,sockets=1",
            "net_backend": "virtio",
        },
        "artifacts": [
            {"id": "elfloader", "path": "elfloader"},
            {"id": "kernel", "path": "kernel.elf"},
            {"id": "rootserver", "path": "rootserver"},
            {"id": "initrd", "path": "cohesix-system.cpio"},
        ],
    }
    original = helper.qemu_accelerator
    helper.qemu_accelerator = lambda *_args, **_kwargs: "kvm"
    try:
        command = helper.build_qemu_command(
            document,
            qemu=str(qemu),
            console_port=41001,
            udp_port=41002,
            smoke_port=41003,
        )
    finally:
        helper.qemu_accelerator = original
    cpu_index = command.index("-cpu")
    machine_index = command.index("-machine")
    assert command[cpu_index + 1] == "host"
    assert command[machine_index + 1] == "virt,gic-version=3,virtualization=off"


def test_darwin_defaults_to_hvf_but_preserves_explicit_accelerator(
    helper: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Darwin uses HVF unless an accepted caller override selects another engine."""

    qemu = tmp_path / "qemu-system-aarch64"
    write_executable(
        qemu,
        "#!/bin/sh\n"
        "if [ \"$1\" = \"-accel\" ]; then printf 'Accelerators: hvf tcg\\n'; fi\n",
    )
    monkeypatch.setattr(helper.platform, "system", lambda: "Darwin")
    monkeypatch.delenv("COHESIX_QEMU_ACCEL", raising=False)
    monkeypatch.delenv("QEMU_ACCEL", raising=False)

    assert helper.qemu_accelerator(str(qemu)) == "hvf"

    monkeypatch.setenv("QEMU_ACCEL", "tcg")
    assert helper.qemu_accelerator(str(qemu)) == "tcg"

    monkeypatch.setenv("COHESIX_QEMU_ACCEL", "hvf")
    assert helper.qemu_accelerator(str(qemu)) == "hvf"


def test_explicit_tcg_is_typed_diagnostic_and_cannot_claim_integration(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    inputs = create_artifact_inputs(tmp_path)
    manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, manifest, accelerator="tcg")
    artifact = json.loads(manifest.read_text(encoding="utf-8"))
    assert artifact["qemu"]["accelerator"] == "tcg"
    assert artifact["qemu"]["claim"]["eligible"] is False
    assert artifact["qemu"]["claim"]["tier"] == "qemu-diagnostic"
    capsys.readouterr()

    log = tmp_path / "tcg.log"
    log.write_text("diagnostic boot completed\n", encoding="utf-8")
    common = [
        "result",
        "--output",
        str(tmp_path / "tcg-result.json"),
        "--action-id",
        "stage-03-qemu-tcp",
        "--catalog-action-digest",
        CATALOG_DIGEST,
        "--target",
        "qemu",
        "--source-digest",
        "sha256:" + ("a" * 64),
        "--evidence-root",
        str(tmp_path),
        "--artifact-manifest",
        str(manifest),
        "--artifact-action-id",
        "stage-03-qemu-tcp",
        "--artifact-catalog-action-digest",
        CATALOG_DIGEST,
        "--boot-id",
        "tcg-diagnostic-boot",
        "--group",
        "base",
        "--status",
        "pass",
        "--script",
        "boot_v0.coh",
        "--log",
        str(log),
    ]
    integration = common.copy()
    integration[1:1] = ["--claim-tier", "qemu-integration"]
    assert helper.main(integration) == 1
    assert "does not match" in capsys.readouterr().err

    diagnostic = common.copy()
    diagnostic[1:1] = ["--claim-tier", "qemu-diagnostic"]
    assert helper.main(diagnostic) == 0
    result = json.loads((tmp_path / "tcg-result.json").read_text(encoding="utf-8"))
    assert result["claim_tier"] == "qemu-diagnostic"
    assert result["artifact"]["claim_eligible"] is False


def test_qemu_regression_artifact_defaults_virtualization_off() -> None:
    """The canonical batch records the non-virtualized machine contract by default."""

    source = REGRESSION_RUNNER_PATH.read_text(encoding="utf-8")
    assert 'local virtualization="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-off}}"' in source
    assert 'local virtualization="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-on}}"' not in source


def test_qemu_results_aggregate_only_passing_content_addressed_records(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Passing per-boot results form one deterministic stage aggregate."""

    inputs = create_artifact_inputs(tmp_path)
    artifact_manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, artifact_manifest)
    capsys.readouterr()
    log = tmp_path / "boot.log"
    log.write_text("Cohesix console ready\nPASS\n", encoding="utf-8")
    source_digest = "sha256:" + ("a" * 64)
    results = []
    for group in ("base", "base-telemetry"):
        output = tmp_path / f"{group}.json"
        results.append(output)
        assert helper.main(
            [
                "result",
                "--output",
                str(output),
                "--action-id",
                "stage-03-qemu-tcp",
                "--catalog-action-digest",
                CATALOG_DIGEST,
                "--claim-tier",
                "qemu-integration",
                "--target",
                "qemu",
                "--source-digest",
                source_digest,
                "--evidence-root",
                str(tmp_path),
                "--artifact-manifest",
                str(artifact_manifest),
                "--artifact-action-id",
                "stage-03-qemu-tcp",
                "--artifact-catalog-action-digest",
                CATALOG_DIGEST,
                "--boot-id",
                f"boot-{group}",
                "--group",
                group,
                "--status",
                "pass",
                "--script",
                f"{group}.coh",
                "--log",
                str(log),
            ]
        ) == 0
    capsys.readouterr()

    aggregate = tmp_path / "stage-03.json"
    arguments = [
        "aggregate",
        "--output",
        str(aggregate),
        "--action-id",
        "stage-03-qemu-tcp",
        "--catalog-action-digest",
        CATALOG_DIGEST,
        "--claim-tier",
        "qemu-integration",
        "--target",
        "qemu",
        "--source-digest",
        source_digest,
        "--evidence-root",
        str(tmp_path),
    ]
    for result in results:
        arguments.extend(("--result", str(result)))
    assert helper.main(arguments) == 0
    document = json.loads(aggregate.read_text(encoding="utf-8"))
    assert document["claim_tier"] == "qemu-integration"
    assert [record["group"] for record in document["results"]] == [
        "base",
        "base-telemetry",
    ]
    verify_arguments = [
        "verify-aggregate",
        "--aggregate",
        str(aggregate),
        "--result-root",
        str(tmp_path),
        "--action-id",
        "stage-03-qemu-tcp",
        "--catalog-action-digest",
        CATALOG_DIGEST,
        "--claim-tier",
        "qemu-integration",
        "--target",
        "qemu",
        "--source-digest",
        source_digest,
        "--evidence-root",
        str(tmp_path),
        "--expected-group",
        "base",
        "--expected-group",
        "base-telemetry",
    ]
    assert helper.main(verify_arguments) == 0
    capsys.readouterr()

    log.write_text("mutated after aggregation\n", encoding="utf-8")
    assert helper.main(verify_arguments) == 1
    assert "transport log" in capsys.readouterr().err


def test_aggregate_rejects_a_readdressed_wrong_claim_tier(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A valid content ID cannot make the wrong claim tier aggregateable."""

    inputs = create_artifact_inputs(tmp_path)
    artifact_manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, artifact_manifest)
    capsys.readouterr()
    log = tmp_path / "qemu.log"
    log.write_text("PASS\n", encoding="utf-8")
    result = tmp_path / "result.json"
    source_digest = "sha256:" + ("a" * 64)
    assert helper.main(
        [
            "result",
            "--output",
            str(result),
            "--action-id",
            "stage-03-qemu-tcp",
            "--catalog-action-digest",
            CATALOG_DIGEST,
            "--claim-tier",
            "qemu-integration",
            "--target",
            "qemu",
            "--source-digest",
            source_digest,
            "--evidence-root",
            str(tmp_path),
            "--artifact-manifest",
            str(artifact_manifest),
            "--artifact-action-id",
            "stage-03-qemu-tcp",
            "--artifact-catalog-action-digest",
            CATALOG_DIGEST,
            "--boot-id",
            "boot-base",
            "--group",
            "base",
            "--status",
            "pass",
            "--script",
            "boot_v0.coh",
            "--log",
            str(log),
        ]
    ) == 0
    capsys.readouterr()
    document = json.loads(result.read_text(encoding="utf-8"))
    document["claim_tier"] = "pi4-transport"
    document["result_id"] = helper.sha256_bytes(
        helper.canonical_bytes(helper.result_identity_material(document))
    )
    result.write_text(json.dumps(document), encoding="utf-8")

    assert helper.main(
        [
            "aggregate",
            "--output",
            str(tmp_path / "aggregate.json"),
            "--action-id",
            "stage-03-qemu-tcp",
            "--catalog-action-digest",
            CATALOG_DIGEST,
            "--claim-tier",
            "qemu-integration",
            "--target",
            "qemu",
            "--source-digest",
            source_digest,
            "--evidence-root",
            str(tmp_path),
            "--result",
            str(result),
        ]
    ) == 1
    assert "tier mismatch" in capsys.readouterr().err


def test_source_digest_binds_git_head_and_executable_mode(
    helper: ModuleType,
    tmp_path: Path,
) -> None:
    """Standalone source binding changes for commit and worktree mode identity."""

    repo = tmp_path / "repo"
    repo.mkdir()
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "config", "user.email", "test@example.invalid"],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(repo), "config", "user.name", "Test User"],
        check=True,
    )
    source = repo / "source.sh"
    source.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(repo), "add", "source.sh"], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "commit", "-qm", "initial"],
        check=True,
    )
    initial = helper.source_digest(repo)

    subprocess.run(
        ["git", "-C", str(repo), "commit", "--allow-empty", "-qm", "identity"],
        check=True,
    )
    new_head = helper.source_digest(repo)
    assert new_head != initial

    source.chmod(source.stat().st_mode | stat.S_IXUSR)
    executable = helper.source_digest(repo)
    assert executable != new_head


def test_launch_refuses_an_occupied_udp_forward(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Every host-forward port is admitted before a QEMU process starts."""

    inputs = create_artifact_inputs(tmp_path)
    manifest = tmp_path / "qemu-artifact.json"
    record_artifact(helper, inputs, manifest)
    capsys.readouterr()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as occupied:
        occupied.bind(("127.0.0.1", 0))
        udp_port = occupied.getsockname()[1]
        assert helper.main(
            [
                "launch",
                "--artifact-manifest",
                str(manifest),
                "--qemu",
                str(inputs["qemu"]),
                "--catalog-action-digest",
                CATALOG_DIGEST,
                "--console-port",
                "41101",
                "--udp-port",
                str(udp_port),
                "--smoke-port",
                "41103",
                "--print-command",
            ]
        ) == 1
    assert "UDP port is unavailable" in capsys.readouterr().err


def test_external_qemu_result_binds_stage3_artifact_across_action_boundary(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """REST evidence may reuse, but cannot silently replace, the Stage 03 image."""

    inputs = create_artifact_inputs(tmp_path)
    artifact_manifest = tmp_path / "qemu-artifact.json"
    artifact_id = record_artifact(helper, inputs, artifact_manifest)
    capsys.readouterr()
    source_digest = "sha256:" + ("a" * 64)
    target_evidence = tmp_path / "qemu-target.json"
    target_document = {
        "schema": "cohesix.test-plan.target-evidence.v1",
        "claim_tier": "qemu-integration",
        "target": "qemu",
        "source_digest": source_digest,
        "boot_id": "external-rest-boot",
        "artifact_id": artifact_id,
        "target_host": "127.0.0.1",
    }
    target_evidence.write_text(json.dumps(target_document), encoding="utf-8")
    verify_arguments = [
        "verify-qemu-target",
        "--target-evidence",
        str(target_evidence),
        "--artifact-manifest",
        str(artifact_manifest),
        "--source-digest",
        source_digest,
        "--artifact-action-id",
        "stage-03-qemu-tcp",
        "--artifact-catalog-action-digest",
        CATALOG_DIGEST,
        "--gateway-url",
        "http://127.0.0.1:9080",
    ]
    assert helper.main(verify_arguments) == 0
    capsys.readouterr()
    monkeypatch.setenv("COHESIX_GATEWAY_URL", "http://127.0.0.1:9080")
    assert helper.main(verify_arguments[:-2]) == 0
    capsys.readouterr()

    log = tmp_path / "rest.log"
    log.write_text("REST PASS\n", encoding="utf-8")
    result = tmp_path / "stage-04.json"
    assert helper.main(
        [
            "result",
            "--output",
            str(result),
            "--action-id",
            "qemu.rest-regression",
            "--catalog-action-digest",
            CATALOG_DIGEST,
            "--claim-tier",
            "qemu-integration",
            "--target",
            "qemu",
            "--source-digest",
            source_digest,
            "--evidence-root",
            str(tmp_path),
            "--artifact-manifest",
            str(artifact_manifest),
            "--artifact-action-id",
            "stage-03-qemu-tcp",
            "--artifact-catalog-action-digest",
            CATALOG_DIGEST,
            "--target-evidence",
            str(target_evidence),
            "--group",
            "rest-multiplexer",
            "--status",
            "pass",
            "--script",
            "boot_v0.coh",
            "--log",
            str(log),
        ]
    ) == 0
    result_document = json.loads(result.read_text(encoding="utf-8"))
    assert result_document["boot_id"] == "external-rest-boot"
    assert result_document["artifact"]["artifact_id"] == artifact_id

    target_document["artifact_id"] = "sha256:" + ("f" * 64)
    target_evidence.write_text(json.dumps(target_document), encoding="utf-8")
    assert helper.main(verify_arguments) == 1
    assert "does not bind" in capsys.readouterr().err


def test_pi_transport_requires_boot_image_host_and_source_binding(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A TCP transcript alone cannot create Pi hardware or transport proof."""

    source_digest = "sha256:" + ("b" * 64)
    target_evidence = tmp_path / "pi4-target.json"
    target_evidence.write_text(
        json.dumps(
            {
                "schema": "cohesix.test-plan.target-evidence.v1",
                "claim_tier": "pi4-transport",
                "target": "pi4",
                "source_digest": source_digest,
                "boot_id": "boot-123",
                "image_identity": "sha256:" + ("c" * 64),
            }
        ),
        encoding="utf-8",
    )
    log = tmp_path / "pi.log"
    log.write_text("PASS boot_v0.coh\n", encoding="utf-8")
    result = tmp_path / "pi-result.json"
    common = [
        "result",
        "--output",
        str(result),
        "--action-id",
        "stage-03-qemu-tcp",
        "--catalog-action-digest",
        CATALOG_DIGEST,
        "--claim-tier",
        "pi4-transport",
        "--target",
        "pi4",
        "--source-digest",
        source_digest,
        "--evidence-root",
        str(tmp_path),
        "--target-evidence",
        str(target_evidence),
        "--boot-id",
        "boot-123",
        "--group",
        "base",
        "--status",
        "pass",
        "--script",
        "boot_v0.coh",
        "--log",
        str(log),
    ]
    assert helper.main(common) == 1
    assert "missing target_host" in capsys.readouterr().err

    document = json.loads(target_evidence.read_text(encoding="utf-8"))
    document["target_host"] = "192.0.2.10"
    target_evidence.write_text(json.dumps(document), encoding="utf-8")
    assert helper.main(common) == 0
    output = json.loads(result.read_text(encoding="utf-8"))
    assert output["claim_tier"] == "pi4-transport"
    assert output["target_evidence"]["upstream_claim_tier"] == "pi4-transport"
    continuity = [
        "verify-pi4-continuity",
        "--target-evidence",
        str(target_evidence),
        "--prior-result",
        str(result),
        "--source-digest",
        source_digest,
        "--prior-evidence-root",
        str(tmp_path),
        "--prior-action-id",
        "stage-03-qemu-tcp",
        "--prior-catalog-action-digest",
        CATALOG_DIGEST,
        "--gateway-url",
        "http://192.0.2.10:8080",
    ]
    assert helper.main(continuity) == 0
    capsys.readouterr()

    wrong_gateway = continuity[:-1] + ["http://192.0.2.99:8080"]
    assert helper.main(wrong_gateway) == 1
    assert "gateway host does not match" in capsys.readouterr().err

    document["target_host"] = "192.0.2.11"
    target_evidence.write_text(json.dumps(document), encoding="utf-8")
    assert helper.main(continuity) == 1
    assert "target evidence hash mismatch" in capsys.readouterr().err


def test_record_pi4_evidence_validates_source_tier_and_gateway_before_use(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The production generator emits only source- and gateway-bound evidence."""

    source_digest = "sha256:" + ("b" * 64)
    evidence = tmp_path / "pi4-target.json"
    record_arguments = [
        "record-pi4-evidence",
        "--output",
        str(evidence),
        "--source-digest",
        source_digest,
        "--boot-id",
        "boot-live-123",
        "--image-identity",
        "sha256:" + ("c" * 64),
        "--target-host",
        "192.0.2.10",
        "--gateway-url",
        "http://192.0.2.10:8080",
    ]
    assert helper.main(record_arguments) == 0
    document = json.loads(evidence.read_text(encoding="utf-8"))
    assert document["schema"] == "cohesix.test-plan.target-evidence.v1"
    assert document["claim_tier"] == "pi4-transport"

    verify_arguments = [
        "verify-pi4-evidence",
        "--target-evidence",
        str(evidence),
        "--source-digest",
        source_digest,
        "--gateway-url",
        "http://192.0.2.10:8080",
    ]
    assert helper.main(verify_arguments) == 0
    capsys.readouterr()

    stale = verify_arguments.copy()
    stale[stale.index(source_digest)] = "sha256:" + ("d" * 64)
    assert helper.main(stale) == 1
    assert "source digest does not match" in capsys.readouterr().err

    document["claim_tier"] = "pi4-hardware"
    evidence.write_text(json.dumps(document), encoding="utf-8")
    assert helper.main(verify_arguments) == 1
    assert "must carry pi4-transport tier" in capsys.readouterr().err

    bad_record = record_arguments.copy()
    bad_record[bad_record.index("sha256:" + ("c" * 64))] = "not-a-digest"
    assert helper.main(bad_record) == 1
    assert "image_identity" in capsys.readouterr().err


def test_qemu_result_cannot_claim_pi_hardware(
    helper: ModuleType,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """The staged transport result schema excludes hardware acceptance."""

    log = tmp_path / "log"
    log.write_text("PASS\n", encoding="utf-8")
    with pytest.raises(SystemExit):
        helper.main(
            [
                "result",
                "--output",
                str(tmp_path / "result.json"),
                "--action-id",
                "stage-03-qemu-tcp",
                "--catalog-action-digest",
                CATALOG_DIGEST,
                "--claim-tier",
                "pi4-hardware",
                "--target",
                "pi4",
                "--source-digest",
                "sha256:" + ("d" * 64),
                "--evidence-root",
                str(tmp_path),
                "--boot-id",
                "boot",
                "--group",
                "base",
                "--status",
                "pass",
                "--script",
                "boot_v0.coh",
                "--log",
                str(log),
            ]
        )
    assert "invalid choice" in capsys.readouterr().err
