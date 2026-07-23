#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Test safe path admission for the QEMU and Pi transport batch wrapper.
# Copyright 2026 Lukas Bower

"""Focused wrapper tests for scripts/cohsh/run_regression_batch.sh."""

from __future__ import annotations

import os
from pathlib import Path
import stat
import subprocess


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "cohsh" / "run_regression_batch.sh"


def write_executable(path: Path, body: str) -> None:
    """Write one executable test fixture."""

    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def run_path_admission(
    *,
    archive: str,
    artifact: str | None = None,
    result_root: str | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run the wrapper's non-mutating path-admission mode."""

    environment = os.environ.copy()
    environment.update(
        {
            "COHSH_BATCH_PRINT_PATHS": "1",
            "COHSH_LOG_ROOT": archive,
            "TEST_PLAN_SOURCE_DIGEST": "sha256:" + ("a" * 64),
        }
    )
    if artifact is not None:
        environment["COHSH_QEMU_ARTIFACT_ROOT"] = artifact
    if result_root is not None:
        environment["COHSH_TRANSPORT_RESULT_ROOT"] = result_root
    return subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )


def test_relative_log_root_is_canonicalized_before_reset() -> None:
    """A relative caller path becomes an absolute repository-scoped path."""

    relative = "out/test-plan/path-admission"
    environment = os.environ.copy()
    environment.update(
        {
            "COHSH_BATCH_PRINT_PATHS": "1",
            "COHSH_LOG_ROOT": relative,
            "TEST_PLAN_SOURCE_DIGEST": "sha256:" + ("a" * 64),
        }
    )
    result = subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=REPO_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )
    values = dict(
        line.split("=", maxsplit=1)
        for line in result.stdout.splitlines()
        if "=" in line
    )
    archive_root = (REPO_ROOT / relative).resolve()
    assert values["ARCHIVE_ROOT"] == str(archive_root)
    assert values["QEMU_ARTIFACT_ROOT"] == str(
        archive_root / "qemu-artifacts"
    )
    assert values["TRANSPORT_RESULT_ROOT"] == str(
        archive_root / "transport-results"
    )


def test_repository_root_is_rejected_before_any_reset() -> None:
    """A broad relative log root cannot turn cleanup into repository deletion."""

    sentinel = REPO_ROOT / "AGENTS.md"
    before = sentinel.read_bytes()
    result = run_path_admission(archive=".")

    assert result.returncode != 0
    assert "unsafe archive root" in result.stderr
    assert sentinel.read_bytes() == before


def test_artifact_and_result_roots_cannot_alias_or_overlap(
    tmp_path: Path,
) -> None:
    """Independent output classes cannot delete or overwrite one another."""

    archive = tmp_path / "archive"
    aliased = run_path_admission(
        archive=str(archive),
        artifact=str(archive),
        result_root=str(tmp_path / "results"),
    )
    assert aliased.returncode != 0
    assert "must not alias the archive root" in aliased.stderr

    artifact = tmp_path / "artifacts"
    overlapping = run_path_admission(
        archive=str(archive),
        artifact=str(artifact),
        result_root=str(artifact / "results"),
    )
    assert overlapping.returncode != 0
    assert "must not alias or overlap" in overlapping.stderr


def test_prepare_only_builds_shared_base_manifest_once_and_restores_generated(
    tmp_path: Path,
) -> None:
    """Three fresh-boot base groups share one prepared immutable artifact."""

    fake_build = tmp_path / "fake-build-run"
    count_file = tmp_path / "build-count"
    write_executable(
        fake_build,
        "#!/usr/bin/env bash\n"
        "# Author: Lukas Bower\n"
        "# Purpose: Create a minimal QEMU artifact fixture for wrapper tests.\n"
        "# Copyright 2026 Lukas Bower\n"
        "set -euo pipefail\n"
        "out_dir=''\n"
        "while [[ $# -gt 0 ]]; do\n"
        "  if [[ \"$1\" == '--out-dir' ]]; then out_dir=\"$2\"; shift 2; else shift; fi\n"
        "done\n"
        "test -n \"$out_dir\"\n"
        "printf 'build\\n' >>\"$FAKE_BUILD_COUNT_FILE\"\n"
        "mkdir -p \"$out_dir/staging\" \"$out_dir/host-tools\"\n"
        "for path in staging/elfloader staging/kernel.elf staging/rootserver "
        "cohesix-system.cpio host-tools/cohsh host-tools/hive-gateway "
        "host-tools/coh; do\n"
        "  printf 'fixture:%s\\n' \"$path\" >\"$out_dir/$path\"\n"
        "done\n"
        "mkdir -p configs/generated out\n"
        "printf '{\"fixture\":true}\\n' >configs/generated/root_task_resolved.json\n"
        "printf 'fixture = true\\n' >out/cohsh_policy.toml\n",
    )
    sel4 = tmp_path / "sel4"
    config = sel4 / "kernel" / "gen_config" / "kernel_config.h"
    config.parent.mkdir(parents=True)
    config.write_text("#define CONFIG_ARM_GIC_V3 1\n", encoding="utf-8")
    archive = tmp_path / "logs"
    artifact_root = tmp_path / "artifacts"

    generated = REPO_ROOT / "configs" / "generated" / "root_task_resolved.json"
    generated_before = generated.read_bytes()
    policy = REPO_ROOT / "out" / "cohsh_policy.toml"
    policy_existed = policy.exists()
    policy_before = policy.read_bytes() if policy_existed else b""
    environment = os.environ.copy()
    environment.update(
        {
            "COHESIX_BUILD_RUN_BIN": str(fake_build),
            "COHSH_BATCH_GROUPS": "base,base-telemetry,base-shard",
            "COHSH_BATCH_PREPARE_ONLY": "1",
            "COHSH_LOG_ROOT": str(archive),
            "COHSH_QEMU_ARTIFACT_ROOT": str(artifact_root),
            "COHSH_TRANSPORT_RESULT_ROOT": str(tmp_path / "results"),
            "FAKE_BUILD_COUNT_FILE": str(count_file),
            "SEL4_BUILD_DIR": str(sel4),
            "TEST_PLAN_SOURCE_DIGEST": "sha256:" + ("a" * 64),
        }
    )
    subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=REPO_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    assert count_file.read_text(encoding="utf-8").splitlines() == ["build"]
    assert (artifact_root / "base" / "qemu-artifact.json").is_file()
    assert generated.read_bytes() == generated_before
    if policy_existed:
        assert policy.read_bytes() == policy_before
    else:
        assert not policy.exists()
