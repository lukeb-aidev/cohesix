#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify provisioned target-root checks use fresh, profile-bound component identities.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

import pytest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts/ci/test_plan_target_root_check.sh"


def test_target_root_check_preserves_exact_profile_and_binding_contract() -> None:
    source = SCRIPT.read_text(encoding="utf-8")

    for required in (
        "qemu:qemu_smp_production:release-qemu:24000000",
        "pi4:pi4_production:release-pi4:54000000",
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
        "scripts/pi4-image-build.sh",
        "selected-image-binding.json",
        "cohesix-root-task-resolved.json",
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
            "schema": "cohesix-python-profile/v2",
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
    python3 - "$@" <<'PY'
import json
import os
from pathlib import Path
import sys

Path(os.environ['FAKE_CARGO_LOG'] + '.args.json').write_text(json.dumps(sys.argv[1:]))
PY
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
        profile = "pi4_production"
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


def test_qemu_manifest_mismatch_stops_before_target_component_build(
    tmp_path: Path,
) -> None:
    repo_root, script, state_dir, fake_bin = _write_target_root_fixture(tmp_path)

    result, cargo_log = _run_target_root_fixture(
        repo_root,
        script,
        state_dir,
        fake_bin,
        target="qemu",
        selected_manifest_sha="b" * 64,
    )

    assert result.returncode != 0
    assert "does not match selected qemu manifest" in result.stderr
    assert cargo_log == ["run"]


@pytest.mark.parametrize("target,network_feature,configs", [
    ("qemu", "console-network-runtime/direct-virtio", []),
])
def test_matching_manifest_selects_production_network_component(
    tmp_path: Path,
    target: str,
    network_feature: str,
    configs: list[str],
) -> None:
    """Build the selected transport without depending on diagnostic features."""
    repo_root, script, state_dir, fake_bin = _write_target_root_fixture(tmp_path)

    result, cargo_log = _run_target_root_fixture(
        repo_root,
        script,
        state_dir,
        fake_bin,
        target=target,
        selected_manifest_sha="a" * 64,
    )

    assert result.returncode == 77
    assert cargo_log == ["run", "build"]
    arguments = json.loads((repo_root / "cargo.log.args.json").read_text())
    assert arguments[arguments.index("--features") + 1] == network_feature
    assert [arguments[i + 1] for i, value in enumerate(arguments)
            if value == "--config"] == configs


PI_RESOLVED = b'{"selected":"pi4-production"}\n'


def _install_pi_builder_fixture(repo_root: Path, fake_bin: Path, case: str) -> None:
    """Provide compiler/stager outputs without simulating target execution."""
    config = {"case": case, "resolved": PI_RESOLVED.decode()}
    (repo_root / "pi-builder-case.json").write_text(json.dumps(config))
    builder = repo_root / "scripts/pi4-image-build.sh"
    builder.write_text("""#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Supply exact stager outputs for host workflow contract tests.
# Copyright 2026 Lukas Bower
import json
from pathlib import Path
import sys
root = Path(__file__).resolve().parents[1]
case = json.loads((root / 'pi-builder-case.json').read_text())
args = dict(zip(sys.argv[1::2], sys.argv[2::2], strict=True))
(root / 'pi-builder-args.json').write_text(json.dumps(args))
if case['case'] == 'build-failed':
    raise SystemExit(77)
stage = Path(args['--stage-dir'])
stage.mkdir(parents=True)
(stage / 'cohesix-root-task-resolved.json').write_text(case['resolved'])
(stage / 'cohesix-image-arm-bcm2711').write_bytes(b'fixture-only')
identity = {'schema': 'cohesix-pi4-image-identity/v2', 'git_commit': 'd' * 40,
            'source_tree_clean': True, 'image_id': '1' * 64,
            'image_sha256': '2' * 64, 'size_bytes': 12,
            'build_marker_sha256': '3' * 64}
(root / 'verified-image-fixture.json').write_text(json.dumps(identity))
if case['case'] == 'dirty':
    identity['source_tree_clean'] = False
if case['case'] == 'wrong-source':
    identity['git_commit'] = 'e' * 40
if case['case'] == 'wrong-image':
    identity['image_sha256'] = '4' * 64
if case['case'] == 'wrong-schema':
    identity['schema'] = 'legacy'
(stage / 'pi4-image-identity.json').write_text(json.dumps(identity))
""")
    builder.chmod(0o755)
    verifier = repo_root / "scripts/pi4_image_identity.py"
    verifier.write_text("""# Author: Lukas Bower
# Purpose: Return a controlled independent image-verifier fixture.
# Copyright 2026 Lukas Bower
from pathlib import Path
import sys
assert sys.argv[1:3] == ['verify', '--image']
print((Path(__file__).resolve().parents[1] / 'verified-image-fixture.json').read_text())
""")
    git = fake_bin / "git"
    git.write_text("#!/usr/bin/env python3\n# Author: Lukas Bower\n"
                   "# Purpose: Supply the controlled current commit for a host fixture.\n"
                   "# Copyright 2026 Lukas Bower\nimport sys\n"
                   "assert sys.argv[1:] == ['rev-parse', 'HEAD']\nprint('d' * 40)\n")
    git.chmod(0o755)


@pytest.mark.parametrize(
    "case",
    ["valid", "build-failed", "wrong-manifest", "dirty", "wrong-source",
     "wrong-image", "wrong-schema"],
)
def test_pi_stage_uses_selected_build_and_rejects_bad_binding(
    tmp_path: Path, case: str,
) -> None:
    """The QEMU default module cannot be a Pi build oracle; exact staged truth can."""
    repo_root, script, state_dir, fake_bin = _write_target_root_fixture(tmp_path)
    _install_pi_builder_fixture(repo_root, fake_bin, case)
    selected = hashlib.sha256(PI_RESOLVED).hexdigest()
    if case == "wrong-manifest":
        selected = "b" * 64
    result, cargo_log = _run_target_root_fixture(
        repo_root, script, state_dir, fake_bin, target="pi4",
        selected_manifest_sha=selected,
    )
    assert cargo_log == ["run"]
    arguments = json.loads((repo_root / "pi-builder-args.json").read_text())
    assert arguments["--manifest"] == str(
        repo_root / "configs/root_task_pi4_uboot_aarch64.toml"
    )
    assert arguments["--sel4-build-dir"] == str(repo_root / "seL4/build_UBOOT")
    assert arguments["--root-task-features"] == "release-pi4"
    assert set(arguments) == {
        "--manifest", "--sel4-build-dir", "--root-task-features", "--stage-dir",
    }
    if case == "valid":
        assert result.returncode == 0, result.stderr
        binding_path = (
            Path(arguments["--stage-dir"]).parent / "selected-image-binding.json"
        )
        binding = json.loads(binding_path.read_text())
        assert binding["manifest_sha256"] == selected
        assert binding["proof"] == "build-only"
        assert binding["source_commit"] == "d" * 40
        assert "PASS target=pi4" in result.stdout
    else:
        assert result.returncode != 0
        assert "PASS target=pi4" not in result.stdout
