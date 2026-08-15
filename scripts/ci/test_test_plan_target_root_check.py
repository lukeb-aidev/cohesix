#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify provisioned target-root checks use fresh, profile-bound component identities.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts/ci/test_plan_target_root_check.sh"


def test_target_root_check_preserves_exact_profile_and_binding_contract() -> None:
    source = SCRIPT.read_text(encoding="utf-8")

    for required in (
        "qemu:qemu_smp_production:release-qemu:24000000",
        "pi4:pi4_diagnostic:release-pi4:54000000",
        'selected_manifest="${repo_root}/configs/root_task.toml"',
        'selected_manifest="${repo_root}/configs/root_task_pi4_uboot_aarch64.toml"',
        "coh-rtc-python-profile",
        "selected-python-profile.json",
        "MANIFEST_SHA256",
        "generated root-task projection does not match selected",
        'export CARGO_TARGET_DIR="${output_dir}/cargo-target"',
        "-p nine-door-runtime",
        "-p console-network-runtime",
        "-p worker-heart",
        "-p worker-gpu",
        "-p worker-lora",
        "-p pi4-driver-runtime",
        "scripts/worker_image_manifest.py",
        "scripts/driver_runtime_manifest.py",
        "COHESIX_WORKER_IMAGE_ARCHIVE",
        "COHESIX_WORKER_IMAGE_MANIFEST",
        "COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE",
        "COHESIX_NINEDOOR_RUNTIME_IMAGE",
        "COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD",
        "COHESIX_PI4_WIFI_FIRMWARE_DIR",
    ):
        assert required in source
    assert source.index("coh-rtc-python-profile") < source.index(
        "cargo build --locked"
    )
    assert source.index("cargo build --locked") < source.index(
        "scripts/worker_image_manifest.py"
    )
    assert source.index("scripts/worker_image_manifest.py") < source.index(
        "cargo check --locked"
    )
    assert "target/aarch64-unknown-none/release" not in source


def test_target_root_check_rejects_a_cross_profile_tuple_before_build() -> None:
    result = subprocess.run(
        [
            "bash",
            str(SCRIPT),
            "--target",
            "qemu",
            "--sel4-build",
            str(REPO_ROOT / "out/sel4/profile-v2/qemu-smp-production"),
            "--profile",
            "qemu_smp_production",
            "--features",
            "release-qemu",
            "--timer-clock-hz",
            "54000000",
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "tuple is not canonical" in result.stderr


def _write_target_root_fixture(
    tmp_path: Path,
) -> tuple[Path, Path, Path, Path]:
    repo_root = tmp_path / "repo"
    script = repo_root / "scripts/ci/test_plan_target_root_check.sh"
    script.parent.mkdir(parents=True)
    shutil.copy2(SCRIPT, script)

    (repo_root / "configs/sel4").mkdir(parents=True)
    (repo_root / "configs/sel4/profiles.toml").write_text(
        "schema_version = 2\n",
        encoding="utf-8",
    )
    for manifest_name in (
        "root_task.toml",
        "root_task_pi4_uboot_aarch64.toml",
    ):
        (repo_root / "configs" / manifest_name).write_text(
            "schema_version = 1\n",
            encoding="utf-8",
        )

    generated = repo_root / "apps/root-task/src/generated/mod.rs"
    generated.parent.mkdir(parents=True)
    generated.write_text(
        'pub const MANIFEST_SHA256: &str = "' + ("a" * 64) + '";\n',
        encoding="utf-8",
    )

    qemu_build = repo_root / "out/sel4/profile-v2/qemu-smp-production"
    pi4_build = repo_root / "seL4/build_UBOOT"
    for build_dir, frequency in (
        (qemu_build, 24_000_000),
        (pi4_build, 54_000_000),
    ):
        header = build_dir / "kernel/gen_headers/plat/platform_gen.h"
        header.parent.mkdir(parents=True)
        header.write_text(
            f"#define TIMER_CLOCK_HZ ULL_CONST({frequency})\n",
            encoding="utf-8",
        )

    state_dir = repo_root / "state"
    attempt_dir = state_dir / "evidence/attempts/stage-02/attempt-id"
    attempt_dir.mkdir(parents=True)

    fake_bin = repo_root / "fake-bin"
    fake_bin.mkdir()
    fake_cargo = fake_bin / "cargo"
    fake_cargo.write_text(
        """#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Model the target-root identity boundary without building target code.
# Copyright 2026 Lukas Bower
set -euo pipefail

case "${1:-}" in
  run)
    printf 'run\n' >>"${FAKE_CARGO_LOG}"
    manifest=""
    output=""
    previous=""
    for argument in "$@"; do
      if [[ "${previous}" == "--out" ]]; then
        output="${argument}"
      fi
      case "${argument}" in
        */configs/root_task*.toml) manifest="${argument}" ;;
      esac
      previous="${argument}"
    done
    [[ -n "${manifest}" && -n "${output}" ]]
    target="qemu"
    profile="qemu_smp_production"
    if [[ "${manifest}" == *root_task_pi4_uboot_aarch64.toml ]]; then
      target="pi4"
      profile="pi4_production"
    fi
    python3 - "${output}" "${target}" "${profile}" \
      "${FAKE_SELECTED_MANIFEST_SHA}" <<'PY'
import json
from pathlib import Path
import sys

output = Path(sys.argv[1])
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(
    json.dumps(
        {
            "schema": "cohesix-python-profile/v1",
            "target": sys.argv[2],
            "target_profile": sys.argv[3],
            "manifest_sha256": sys.argv[4],
        }
    ),
    encoding="utf-8",
)
PY
    ;;
  build)
    printf 'build\n' >>"${FAKE_CARGO_LOG}"
    exit "${FAKE_BUILD_EXIT:-77}"
    ;;
  *)
    printf 'unexpected cargo command: %s\n' "$*" >&2
    exit 78
    ;;
esac
""",
        encoding="utf-8",
    )
    fake_cargo.chmod(0o755)
    return repo_root, script, state_dir, fake_bin


def _run_target_root_fixture(
    repo_root: Path,
    script: Path,
    state_dir: Path,
    fake_bin: Path,
    *,
    target: str,
    selected_manifest_sha: str,
) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    if target == "qemu":
        sel4_build = repo_root / "out/sel4/profile-v2/qemu-smp-production"
        profile = "qemu_smp_production"
        features = "release-qemu"
        timer_clock_hz = "24000000"
    else:
        sel4_build = repo_root / "seL4/build_UBOOT"
        profile = "pi4_diagnostic"
        features = "release-pi4"
        timer_clock_hz = "54000000"

    cargo_log = repo_root / "cargo.log"
    environment = os.environ.copy()
    environment.update(
        {
            "PATH": f"{fake_bin}:{environment['PATH']}",
            "TEST_PLAN_STATE_DIR": str(state_dir),
            "TEST_PLAN_ATTEMPT_ID": "attempt-id",
            "FAKE_CARGO_LOG": str(cargo_log),
            "FAKE_SELECTED_MANIFEST_SHA": selected_manifest_sha,
        }
    )
    result = subprocess.run(
        [
            "bash",
            str(script),
            "--target",
            target,
            "--sel4-build",
            str(sel4_build),
            "--profile",
            profile,
            "--features",
            features,
            "--timer-clock-hz",
            timer_clock_hz,
        ],
        cwd=repo_root,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )
    log_lines = cargo_log.read_text(encoding="utf-8").splitlines()
    return result, log_lines


def test_pi4_manifest_mismatch_stops_before_target_component_build(
    tmp_path: Path,
) -> None:
    repo_root, script, state_dir, fake_bin = _write_target_root_fixture(tmp_path)

    result, cargo_log = _run_target_root_fixture(
        repo_root,
        script,
        state_dir,
        fake_bin,
        target="pi4",
        selected_manifest_sha="b" * 64,
    )

    assert result.returncode != 0
    assert "does not match selected pi4 manifest" in result.stderr
    assert cargo_log == ["run"]


def test_qemu_matching_manifest_reaches_target_component_build(
    tmp_path: Path,
) -> None:
    repo_root, script, state_dir, fake_bin = _write_target_root_fixture(tmp_path)

    result, cargo_log = _run_target_root_fixture(
        repo_root,
        script,
        state_dir,
        fake_bin,
        target="qemu",
        selected_manifest_sha="a" * 64,
    )

    assert result.returncode == 77
    assert cargo_log == ["run", "build"]
