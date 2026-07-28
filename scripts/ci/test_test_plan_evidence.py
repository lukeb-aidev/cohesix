# Author: Lukas Bower
# Purpose: Verify immutable test-plan evidence, resume, reuse, and redaction behavior.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import json
import os
import pathlib
import shlex
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from unittest import mock
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
CI_DIR = REPO_ROOT / "scripts" / "ci"


CATALOG = """\
# Author: Lukas Bower
# Purpose: Define focused fake actions for test-plan runner behavior tests.
# Copyright 2026 Lukas Bower

[catalog]
schema = "cohesix-test-plan-actions/v1"
minimum_python = "3.11"
default_claims = ["common-hermetic"]
claim_tiers = ["common-hermetic", "qemu-integration", "release"]

[[action]]
id = "integrity.fake"
stage = 1
tier = "common-hermetic"
scope = "common"
targets = ["qemu", "pi4"]
description = "Exercise fake integrity behavior."
command = "printf catalog-stage1"
timeout_seconds = 30
trigger_paths = ["**"]
expected_evidence = ["command-result"]
test_policy = "none"

[[action]]
id = "host.fake"
stage = 2
tier = "common-hermetic"
scope = "provisioned-target"
targets = ["qemu", "pi4"]
description = "Exercise target-bound fake host behavior."
command = "printf catalog-stage2"
timeout_seconds = 30
trigger_paths = ["**"]
expected_evidence = ["command-result"]
test_policy = "none"

[[action]]
id = "tcp.fake"
stage = 3
tier = "qemu-integration"
scope = "target"
targets = ["qemu", "pi4"]
description = "Exercise fake TCP behavior."
command = "printf catalog-stage3"
timeout_seconds = 30
trigger_paths = ["**"]
expected_evidence = ["command-result"]
test_policy = "none"

[[action]]
id = "rest.fake"
stage = 4
tier = "qemu-integration"
scope = "target"
targets = ["qemu", "pi4"]
description = "Exercise fake REST behavior."
command = "printf catalog-stage4"
timeout_seconds = 30
trigger_paths = ["**"]
expected_evidence = ["command-result"]
test_policy = "none"

[[action]]
id = "release.fake"
stage = 5
tier = "release"
scope = "target"
targets = ["qemu", "pi4"]
description = "Exercise fake release behavior."
command = "printf catalog-stage5"
timeout_seconds = 30
trigger_paths = ["**"]
expected_evidence = ["command-result"]
test_policy = "none"
"""


def stage_script(stage: int, name: str) -> str:
    """Return a small stage that exercises the real common helper."""

    prerequisite = (
        f"tp_require_stage_done {stage - 1}" if stage > 1 else ":"
    )
    artifact_setup = ""
    if stage == 3:
        artifact_setup = """\
artifact_root="${TP_ATTEMPT_DIR}/transport"
mkdir -p \
  "${artifact_root}/transport-results" \
  "${artifact_root}/batch" \
  "${artifact_root}/qemu-artifacts/base" \
  "${artifact_root}/qemu-artifacts/gated"
printf '{"status":"pass"}\\n' >"${artifact_root}/transport-results/stage-03.json"
printf "PASS\\n" >"${artifact_root}/batch/summary.log"
printf '{"status":"pass"}\\n' >"${artifact_root}/qemu-artifacts/base/qemu-artifact.json"
printf '{"status":"pass"}\\n' \
  >"${artifact_root}/qemu-artifacts/gated/qemu-artifact.json"
printf "%s\\n" "${artifact_root#${TEST_PLAN_STATE_DIR}/}" \
  >"${TEST_PLAN_STATE_DIR}/stage_03_artifact_root.path"
"""
    elif stage == 4:
        artifact_setup = """\
artifact_root="${TP_ATTEMPT_DIR}/rest"
mkdir -p \
  "${artifact_root}/results" \
  "${artifact_root}/regression-logs/rest-regression-core" \
  "${artifact_root}/regression-logs/rest-regression-parity"
printf '{"status":"pass"}\\n' >"${artifact_root}/results/stage-04.json"
printf "PASS\\n" >"${artifact_root}/results/summary.log"
for name in boot observe session parity; do
  printf "PASS\\n" \
    >"${artifact_root}/regression-logs/rest-regression-core/${name}.log"
done
printf "%s\\n" "${artifact_root#${TEST_PLAN_STATE_DIR}/}" \
  >"${TEST_PLAN_STATE_DIR}/stage_04_artifact_root.path"
"""
    elif stage == 5:
        artifact_setup = """\
artifact_root="${TP_ATTEMPT_DIR}/governance"
mkdir -p "${artifact_root}/audit"
for name in \
  cargo-audit-version \
  cargo-audit \
  cargo-deny-version \
  cargo-deny-advisories
do
  printf "PASS\\n" >"${artifact_root}/audit/${name}.log"
done
printf "%s\\n" "${artifact_root#${TEST_PLAN_STATE_DIR}/}" \
  >"${TEST_PLAN_STATE_DIR}/stage_05_artifact_root.path"
"""
    return f"""\
#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Exercise fake test-plan stage {stage:02d}.
# Copyright 2026 Lukas Bower

set -euo pipefail
script_dir=$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)
source "${{script_dir}}/test_plan_common.sh"
tp_init
{prerequisite}
tp_stage_begin {stage} "{name}"
if [[ "${{TP_FAKE_INTERRUPT_STAGE:-}}" == "{stage}" ]]; then
  kill -TERM "$$"
fi
if [[ "${{TP_FAKE_CATALOG_ACTION:-0}}" == "1" && "{stage}" == "1" ]]; then
  tp_run_catalog_action "integrity.fake"
elif [[ "${{TP_FAKE_FAIL_STAGE:-}}" == "{stage}" ]]; then
  tp_run_cmd "fake-stage-{stage}" bash -c "exit 9"
elif [[ "${{TP_FAKE_SECRET_ACTION:-0}}" == "1" && "{stage}" == "1" ]]; then
  local_secret="manifest-derived-secret-value"
  tp_run_cmd "secret-command" bash -c \
    "AUTH_TOKEN=\\"${{local_secret}}\\" \\
       printf 'Authorization: Bearer ${{local_secret}}\\n'
     printf '%s\\n' '--rest-auth-token ${{local_secret}}'
     printf '%s\\n' '{{\\"api_token\\":\\"${{local_secret}}\\"}}'
     printf 'TOKEN=%s\\n' \\"${{HIVE_GATEWAY_REQUEST_AUTH_TOKEN}}\\""
else
  tp_run_cmd "fake-stage-{stage}" bash -c \
    'test -s "${{TEST_PLAN_ATTEMPT_MANIFEST}}"; printf stage-{stage}'
fi
{artifact_setup}tp_stage_complete {stage}
"""


class RunnerFixture:
    """Create an isolated, tiny Git repository around the production runner."""

    def __init__(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.base = pathlib.Path(self.temporary.name)
        self.root = self.base / "repo"
        self.tools = self.base / "tools"
        (self.root / "scripts/ci").mkdir(parents=True)
        (self.root / "configs").mkdir()
        self.tools.mkdir()
        for name in (
            "test_plan_common.sh",
            "test_plan_evidence.py",
            "test_plan_resources.sh",
            "test_plan_run.sh",
            "test_plan_catalog.py",
        ):
            shutil.copy2(CI_DIR / name, self.root / "scripts/ci" / name)
        (self.root / "configs/test_plan_actions.toml").write_text(
            CATALOG,
            encoding="utf-8",
        )
        (self.root / ".gitignore").write_text(
            "out/\n__pycache__/\n*.pyc\n",
            encoding="utf-8",
        )
        (self.root / "Cargo.toml").write_text(
            textwrap.dedent(
                """\
                # Author: Lukas Bower
                # Purpose: Provide fixture workspace metadata.
                # Copyright 2026 Lukas Bower
                [workspace]
                resolver = "2"
                members = []
                """
            ),
            encoding="utf-8",
        )
        (self.root / "Cargo.lock").write_text(
            "# Author: Lukas Bower\n"
            "# Purpose: Bind fixture dependency state.\n"
            "# Copyright 2026 Lukas Bower\n"
            "version = 4\n",
            encoding="utf-8",
        )
        (self.root / "tracked.txt").write_text("initial\n", encoding="utf-8")
        names = {
            1: "integrity",
            2: "host-fast",
            3: "qemu-tcp-regression",
            4: "rest-multiplexer",
            5: "due-diligence",
        }
        for stage, name in names.items():
            path = (
                self.root
                / "scripts/ci"
                / f"test_plan_stage_{stage:02d}_{name.replace('-', '_')}.sh"
            )
            path.write_text(stage_script(stage, name), encoding="utf-8")
            path.chmod(0o755)
        for path in (self.root / "scripts/ci").iterdir():
            if path.suffix in {".sh", ".py"}:
                path.chmod(0o755)
        self._git("init", "-q")
        self._git("config", "user.name", "Runner Test")
        self._git("config", "user.email", "runner@example.invalid")
        self._git("add", ".")
        self._git("commit", "-qm", "fixture")

    def close(self) -> None:
        """Release the temporary repository."""

        self.temporary.cleanup()

    def _git(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *arguments],
            cwd=self.root,
            check=True,
            capture_output=True,
            text=True,
        )

    def state(self, name: str) -> pathlib.Path:
        """Return an ignored state directory path."""

        return self.root / "out/states" / name

    def environment(
        self,
        extra: dict[str, str] | None = None,
    ) -> dict[str, str]:
        """Return an isolated environment with optional fake tool identities."""

        environment = os.environ.copy()
        environment["TP_PYTHON_BIN"] = sys.executable
        if extra:
            environment.update(extra)
        return environment

    def run(
        self,
        state: pathlib.Path,
        *,
        stage: int,
        target: str = "qemu",
        force: bool = False,
        resume: bool = False,
        iteration: bool = False,
        reuse: pathlib.Path | None = None,
        extra_environment: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Run one stage through the production runner."""

        command = [
            "bash",
            "scripts/ci/test_plan_run.sh",
            "--target",
            target,
            "--state-dir",
            str(state),
            "--stage",
            str(stage),
        ]
        if force:
            command.append("--force")
        if resume:
            command.append("--resume")
        if iteration:
            command.append("--iteration")
        if reuse:
            command.extend(["--reuse-common-from", str(reuse)])
        return subprocess.run(
            command,
            cwd=self.root,
            env=self.environment(extra_environment),
            check=False,
            capture_output=True,
            text=True,
        )

    def manifest(
        self,
        state: pathlib.Path,
        stage: int,
        *,
        target: str | None = None,
    ) -> tuple[pathlib.Path, dict[str, Any]]:
        """Resolve an active stage manifest through its attestation refs."""

        if target:
            ref_path = state / f"stage_{stage:02d}.{target}.attestation.json"
        else:
            ref_path = state / f"stage_{stage:02d}.attestation.json"
        ref = json.loads(ref_path.read_text(encoding="utf-8"))
        path = state / ref["manifest"]
        payload = json.loads(path.read_text(encoding="utf-8"))
        if target:
            path = state / payload["stage_manifest"]
            payload = json.loads(path.read_text(encoding="utf-8"))
        return path, payload


class TestPlanEvidenceTests(unittest.TestCase):
    """Exercise runner provenance without invoking repository test workloads."""

    def setUp(self) -> None:
        self.fixture = RunnerFixture()

    def tearDown(self) -> None:
        self.fixture.close()

    def assert_success(
        self,
        result: subprocess.CompletedProcess[str],
    ) -> None:
        self.assertEqual(
            result.returncode,
            0,
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
        )

    def test_verified_default_resume_and_force_replacement(self) -> None:
        state = self.fixture.state("resume")
        first = self.fixture.run(state, stage=1)
        self.assert_success(first)
        first_ref = (state / "stage_01.attestation.json").read_bytes()
        attempts = list((state / "evidence/attempts/stage-01").iterdir())

        resumed = self.fixture.run(state, stage=1, resume=True)
        self.assert_success(resumed)
        self.assertIn("resume stage 1: verified attestation", resumed.stdout)
        self.assertEqual(
            attempts,
            list((state / "evidence/attempts/stage-01").iterdir()),
        )

        forced = self.fixture.run(state, stage=1, force=True)
        self.assert_success(forced)
        self.assertNotEqual(
            first_ref,
            (state / "stage_01.attestation.json").read_bytes(),
        )
        self.assertEqual(
            len(list((state / "evidence/attempts/stage-01").iterdir())),
            2,
        )
        self.assertFalse((state / ".test-plan.lock").exists())

    def test_stage_five_refreshes_while_other_stages_resume(self) -> None:
        state = self.fixture.state("stage-five-refresh")
        for stage in range(1, 6):
            self.assert_success(self.fixture.run(state, stage=stage))
        stage_four_attempts = list(
            (state / "evidence/attempts/stage-04").iterdir()
        )
        stage_five_attempt_count = len(
            list((state / "evidence/attempts/stage-05").iterdir())
        )

        resumed = self.fixture.run(state, stage=4)
        self.assert_success(resumed)
        self.assertIn(
            "resume stage 4: verified attestation",
            resumed.stdout,
        )
        self.assertEqual(
            stage_four_attempts,
            list((state / "evidence/attempts/stage-04").iterdir()),
        )

        refreshed = self.fixture.run(state, stage=5)
        self.assert_success(refreshed)
        self.assertIn(
            "refresh stage 5: advisory and governance evidence is "
            "time-sensitive",
            refreshed.stdout,
        )
        self.assertEqual(
            len(list((state / "evidence/attempts/stage-05").iterdir())),
            stage_five_attempt_count + 1,
        )

    def test_active_state_lock_rejects_a_second_writer(self) -> None:
        state = self.fixture.state("locked")
        lock = state / ".test-plan.lock"
        lock.mkdir(parents=True)
        (lock / "owner").write_text("first-writer\n", encoding="utf-8")

        result = self.fixture.run(state, stage=1)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("state directory is already active", result.stderr)
        self.assertIn("first-writer", result.stderr)
        self.assertFalse((state / "stage_01.done").exists())

    def test_resume_and_force_are_mutually_exclusive(self) -> None:
        state = self.fixture.state("resume-force")

        result = self.fixture.run(
            state,
            stage=1,
            resume=True,
            force=True,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("--resume and --force are mutually exclusive", result.stderr)

    def test_missing_legacy_provenance_fails_closed(self) -> None:
        state = self.fixture.state("legacy")
        state.mkdir(parents=True)
        (state / "stage_01.done").write_text("legacy\n", encoding="utf-8")

        result = self.fixture.run(state, stage=1)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("legacy active evidence", result.stderr)

    def test_missing_marker_or_fingerprint_fails_closed(self) -> None:
        for filename in (
            "stage_01.done",
            "stage_01.qemu.done",
            "stage_01.inputs.sha256",
        ):
            with self.subTest(filename=filename):
                state = self.fixture.state(f"missing-{filename}")
                self.assert_success(self.fixture.run(state, stage=1))
                (state / filename).unlink()

                result = self.fixture.run(state, stage=1)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    "stale or corrupt target-qualified evidence",
                    result.stderr,
                )

    def test_source_and_untracked_drift_block_resume(self) -> None:
        state = self.fixture.state("source-drift")
        self.assert_success(self.fixture.run(state, stage=1))
        (self.fixture.root / "tracked.txt").write_text(
            "changed\n",
            encoding="utf-8",
        )

        tracked = self.fixture.run(state, stage=1)
        self.assertNotEqual(tracked.returncode, 0)
        self.assertIn("stale or corrupt target-qualified evidence", tracked.stderr)

        self.assert_success(self.fixture.run(state, stage=1, force=True))
        (self.fixture.root / "untracked.txt").write_text(
            "new input\n",
            encoding="utf-8",
        )
        untracked = self.fixture.run(state, stage=1)
        self.assertNotEqual(untracked.returncode, 0)

    def test_toolchain_drift_blocks_resume(self) -> None:
        state = self.fixture.state("toolchain-drift")
        fake_cargo = self.fixture.tools / "cargo"
        fake_rustc = self.fixture.tools / "rustc"
        fake_cargo.write_text(
            "#!/bin/sh\nprintf 'cargo fixture 1\\n'\n",
            encoding="utf-8",
        )
        fake_rustc.write_text(
            "#!/bin/sh\nprintf 'rustc fixture 1\\n'\n",
            encoding="utf-8",
        )
        fake_cargo.chmod(0o755)
        fake_rustc.chmod(0o755)
        path = f"{self.fixture.tools}{os.pathsep}{os.environ['PATH']}"
        self.assert_success(
            self.fixture.run(
                state,
                stage=1,
                extra_environment={"PATH": path},
            )
        )
        fake_cargo.write_text(
            "#!/bin/sh\nprintf 'cargo fixture 2\\n'\n",
            encoding="utf-8",
        )

        result = self.fixture.run(
            state,
            stage=1,
            extra_environment={"PATH": path},
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stale or corrupt target-qualified evidence", result.stderr)

    def test_failed_force_run_cannot_reuse_old_pass(self) -> None:
        state = self.fixture.state("failed-force")
        self.assert_success(self.fixture.run(state, stage=1))

        failed = self.fixture.run(
            state,
            stage=1,
            force=True,
            extra_environment={"TP_FAKE_FAIL_STAGE": "1"},
        )

        self.assertEqual(failed.returncode, 9)
        self.assertFalse((state / "stage_01.done").exists())
        self.assertFalse((state / "stage_01.qemu.done").exists())
        self.assertFalse((state / "stage_01.attestation.json").exists())
        manifests = list(
            (state / "evidence/attempts/stage-01").glob("*/stage.json")
        )
        statuses = {
            json.loads(path.read_text(encoding="utf-8"))["status"]
            for path in manifests
        }
        self.assertEqual(statuses, {"pass", "failed"})

    def test_interrupted_force_run_is_terminal_and_not_passing(self) -> None:
        state = self.fixture.state("interrupted")
        self.assert_success(self.fixture.run(state, stage=1))

        result = self.fixture.run(
            state,
            stage=1,
            force=True,
            extra_environment={"TP_FAKE_INTERRUPT_STAGE": "1"},
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((state / "stage_01.attestation.json").exists())
        statuses = {
            json.loads(path.read_text(encoding="utf-8"))["status"]
            for path in (
                state / "evidence/attempts/stage-01"
            ).glob("*/stage.json")
        }
        self.assertIn("failed", statuses)

    def test_upstream_rerun_invalidates_downstream_active_evidence(self) -> None:
        state = self.fixture.state("downstream-invalidation")
        for stage in range(1, 6):
            self.assert_success(self.fixture.run(state, stage=stage))
        historical = {
            stage: len(
                list(
                    (
                        state
                        / f"evidence/attempts/stage-{stage:02d}"
                    ).iterdir()
                )
            )
            for stage in range(3, 6)
        }

        self.assert_success(self.fixture.run(state, stage=2, force=True))

        for stage in range(3, 6):
            self.assertFalse(
                (state / f"stage_{stage:02d}.attestation.json").exists()
            )
            self.assertFalse(
                (state / f"stage_{stage:02d}.qemu.attestation.json").exists()
            )
            self.assertFalse((state / f"stage_{stage:02d}.done").exists())
            self.assertFalse((state / f"stage_{stage:02d}.qemu.done").exists())
            self.assertEqual(
                historical[stage],
                len(
                    list(
                        (
                            state
                            / f"evidence/attempts/stage-{stage:02d}"
                        ).iterdir()
                    )
                ),
            )

    def test_iteration_evidence_never_changes_full_pass(self) -> None:
        state = self.fixture.state("iteration")
        self.assert_success(self.fixture.run(state, stage=1))
        generic = (state / "stage_01.attestation.json").read_bytes()
        target = (state / "stage_01.qemu.attestation.json").read_bytes()

        iteration = self.fixture.run(state, stage=1, iteration=True)

        self.assert_success(iteration)
        self.assertEqual(
            generic,
            (state / "stage_01.attestation.json").read_bytes(),
        )
        self.assertEqual(
            target,
            (state / "stage_01.qemu.attestation.json").read_bytes(),
        )
        self.assertTrue((state / "stage_01.iteration.attestation.json").is_file())
        self.assertTrue(
            (state / "stage_01.qemu.iteration.attestation.json").is_file()
        )

    def test_tampered_action_and_log_are_rejected(self) -> None:
        for artifact in ("action", "log"):
            with self.subTest(artifact=artifact):
                state = self.fixture.state(f"tamper-{artifact}")
                self.assert_success(self.fixture.run(state, stage=1))
                manifest_path, manifest = self.fixture.manifest(state, 1)
                if artifact == "action":
                    path = manifest_path.parent / manifest["actions"][0]["path"]
                else:
                    path = manifest_path.parent / manifest["log"]["path"]
                with path.open("a", encoding="utf-8") as handle:
                    handle.write("tampered\n")

                result = self.fixture.run(state, stage=1)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    "stale or corrupt target-qualified evidence",
                    result.stderr,
                )

    def test_tampered_qualified_artifact_root_is_rejected(self) -> None:
        state = self.fixture.state("tamper-artifact")
        self.assert_success(self.fixture.run(state, stage=1))
        self.assert_success(self.fixture.run(state, stage=2))
        self.assert_success(self.fixture.run(state, stage=3))
        pointer = (
            state / "stage_03_artifact_root.path"
        ).read_text(encoding="utf-8").strip()
        summary = state / pointer / "batch/summary.log"
        summary.write_text("tampered\n", encoding="utf-8")

        result = self.fixture.run(state, stage=3)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "stale or corrupt target-qualified evidence",
            result.stderr,
        )

    def test_artifact_pointer_cannot_escape_state_directory(self) -> None:
        state = self.fixture.state("pointer-escape")
        state.mkdir(parents=True)
        pointer = state / "stage_03_artifact_root.path"
        pointer.write_text("../escape\n", encoding="utf-8")

        result = subprocess.run(
            [
                sys.executable,
                "scripts/ci/test_plan_evidence.py",
                "resolve-state-pointer",
                "--state-dir",
                str(state),
                "--pointer",
                str(pointer),
            ],
            cwd=self.fixture.root,
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsafe evidence path", result.stderr)

    def test_secret_is_redacted_and_action_is_structured(self) -> None:
        state = self.fixture.state("redaction")
        secret = "super-secret-request-token"
        local_secret = "manifest-derived-secret-value"
        result = self.fixture.run(
            state,
            stage=1,
            extra_environment={
                "TP_FAKE_SECRET_ACTION": "1",
                "HIVE_GATEWAY_REQUEST_AUTH_TOKEN": secret,
            },
        )
        self.assert_success(result)
        all_evidence = b"".join(
            path.read_bytes()
            for path in state.rglob("*")
            if path.is_file()
        )
        self.assertNotIn(secret.encode(), all_evidence)
        self.assertNotIn(local_secret.encode(), all_evidence)
        manifest_path, manifest = self.fixture.manifest(state, 1)
        action_path = manifest_path.parent / manifest["actions"][0]["path"]
        action = json.loads(action_path.read_text(encoding="utf-8"))
        self.assertEqual(action["schema"], "cohesix.test-plan-action/v1")
        self.assertEqual(action["status"], "pass")
        self.assertEqual(action["exit_code"], 0)
        self.assertGreaterEqual(action["duration_ms"], 0)
        self.assertIn("<redacted>", action["command"])

    def test_context_strips_credentials_from_url_selectors(self) -> None:
        state = self.fixture.state("url-redaction")
        state.mkdir(parents=True)
        context = state / "context.json"
        credential = "gateway-password"
        environment = os.environ.copy()
        environment["COHESIX_GATEWAY_URL"] = (
            f"https://operator:{credential}@pi.example:8443/private"
            "?access_token=also-secret#private-fragment"
        )

        result = subprocess.run(
            [
                sys.executable,
                "scripts/ci/test_plan_evidence.py",
                "capture-context",
                "--root",
                str(self.fixture.root),
                "--state-dir",
                str(state),
                "--stage",
                "4",
                "--target",
                "pi4",
                "--output",
                str(context),
            ],
            cwd=self.fixture.root,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        payload = context.read_bytes()
        self.assertNotIn(credential.encode(), payload)
        self.assertNotIn(b"also-secret", payload)
        self.assertNotIn(b"private-fragment", payload)
        recorded = json.loads(payload)
        self.assertEqual(
            recorded["config"]["environment"]["COHESIX_GATEWAY_URL"],
            "https://pi.example:8443/private",
        )

    def test_catalog_shell_ignores_profiles_and_environment_hooks(self) -> None:
        hook_marker = self.fixture.base / "shell-hook-ran"
        hook = self.fixture.base / "shell-hook.sh"
        hook.write_text(
            f"printf profile-hook; touch {shlex.quote(str(hook_marker))}\n",
            encoding="utf-8",
        )
        fake_home = self.fixture.base / "home"
        fake_home.mkdir()
        for profile_name in (".bash_profile", ".bashrc"):
            (fake_home / profile_name).write_text(
                "printf profile-hook; touch "
                f"{shlex.quote(str(hook_marker))}\n",
                encoding="utf-8",
            )

        result = subprocess.run(
            [
                sys.executable,
                "scripts/ci/test_plan_evidence.py",
                "run-catalog-command",
                "--root",
                str(self.fixture.root),
                "--timeout-seconds",
                "5",
                "--test-policy",
                "none",
                "--command",
                "printf catalog-clean",
            ],
            cwd=self.fixture.root,
            env=self.fixture.environment(
                {
                    "HOME": str(fake_home),
                    "BASH_ENV": str(hook),
                    "ENV": str(hook),
                }
            ),
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "catalog-clean")
        self.assertFalse(hook_marker.exists())

    def test_catalog_output_streams_before_command_completion(self) -> None:
        release = self.fixture.base / "release-stream"
        command = (
            "printf 'early-output\\n'; "
            f"while [ ! -e {shlex.quote(str(release))} ]; "
            "do sleep 0.05; done; "
            "printf '2 passed\\n'"
        )
        process = subprocess.Popen(
            [
                sys.executable,
                "scripts/ci/test_plan_evidence.py",
                "run-catalog-command",
                "--root",
                str(self.fixture.root),
                "--timeout-seconds",
                "5",
                "--test-policy",
                "nonzero",
                "--minimum-test-count",
                "2",
                "--command",
                command,
            ],
            cwd=self.fixture.root,
            env=self.fixture.environment(),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        assert process.stdout is not None
        started = time.monotonic()
        first_line = process.stdout.readline()
        elapsed = time.monotonic() - started
        release.touch()
        remainder, error = process.communicate(timeout=10)

        self.assertEqual(process.returncode, 0, error)
        self.assertEqual(first_line, "early-output\n")
        self.assertLess(elapsed, 2)
        self.assertEqual(remainder, "2 passed\n")

    def test_catalog_nonzero_test_policy_enforces_minimum(self) -> None:
        base_command = [
            sys.executable,
            "scripts/ci/test_plan_evidence.py",
            "run-catalog-command",
            "--root",
            str(self.fixture.root),
            "--timeout-seconds",
            "5",
            "--test-policy",
            "nonzero",
            "--minimum-test-count",
            "2",
            "--command",
        ]
        rejected = subprocess.run(
            [*base_command, "printf '1 passed\\n'"],
            cwd=self.fixture.root,
            env=self.fixture.environment(),
            check=False,
            capture_output=True,
            text=True,
        )
        accepted = subprocess.run(
            [*base_command, "printf '2 passed\\n'"],
            cwd=self.fixture.root,
            env=self.fixture.environment(),
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(rejected.returncode, 1)
        self.assertIn(
            "observed=1 required=2",
            rejected.stderr,
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)

    def test_process_group_signal_tolerates_exit_race(self) -> None:
        sys.path.insert(0, str(CI_DIR))
        try:
            import test_plan_evidence
        finally:
            sys.path.pop(0)
        process = mock.Mock(pid=12345)
        with mock.patch.object(
            test_plan_evidence.os,
            "killpg",
            side_effect=ProcessLookupError,
        ):
            self.assertFalse(
                test_plan_evidence._signal_process_group(
                    process,
                    test_plan_evidence.signal.SIGTERM,
                )
            )

    def test_pi4_default_selects_repo_managed_sel4_16_profile(self) -> None:
        sys.path.insert(0, str(CI_DIR))
        try:
            import test_plan_evidence
        finally:
            sys.path.pop(0)
        root = pathlib.Path("/workspace/cohesix")
        expected = (root / "seL4/build_UBOOT").resolve(strict=False)
        with mock.patch.object(test_plan_evidence.os, "environ", {}):
            self.assertEqual(
                test_plan_evidence.selected_sel4_dir(root, "pi4"),
                expected,
            )

    def test_catalog_action_failure_is_not_swallowed(self) -> None:
        catalog = self.fixture.root / "configs/test_plan_actions.toml"
        catalog.write_text(
            CATALOG.replace(
                'command = "printf catalog-stage1"',
                'command = "exit 7"',
            ),
            encoding="utf-8",
        )
        state = self.fixture.state("catalog-failure")

        result = self.fixture.run(
            state,
            stage=1,
            extra_environment={"TP_FAKE_CATALOG_ACTION": "1"},
        )

        self.assertEqual(result.returncode, 7)
        self.assertFalse((state / "stage_01.done").exists())

    def test_common_reuse_is_content_addressed_and_cross_target(self) -> None:
        source = self.fixture.state("source-qemu")
        destination = self.fixture.state("destination-pi4")
        self.assert_success(self.fixture.run(source, stage=1, target="qemu"))

        result = self.fixture.run(
            destination,
            stage=1,
            target="pi4",
            reuse=source,
        )

        self.assert_success(result)
        self.assertIn("imported verified common evidence", result.stdout)
        self.assertFalse(
            (destination / "evidence/attempts/stage-01").exists()
        )
        imported = list(
            (destination / "evidence/imports/sha256").glob("*/stage.json")
        )
        self.assertEqual(len(imported), 1)
        self.assertTrue((destination / "stage_01.pi4.done").is_file())
        target_metadata = (destination / "target.env").read_text(
            encoding="utf-8"
        )
        self.assertIn("TEST_PLAN_TARGET=pi4", target_metadata)

    def test_stage_two_is_rerun_for_destination_target(self) -> None:
        source = self.fixture.state("source-stage2")
        destination = self.fixture.state("destination-stage2")
        self.assert_success(self.fixture.run(source, stage=1, target="qemu"))
        self.assert_success(self.fixture.run(source, stage=2, target="qemu"))

        result = self.fixture.run(
            destination,
            stage=2,
            target="pi4",
            reuse=source,
        )

        self.assert_success(result)
        _, manifest = self.fixture.manifest(destination, 2)
        self.assertEqual(manifest["scope"], "target")
        self.assertEqual(manifest["target"], "pi4")
        self.assertTrue(
            (destination / "evidence/attempts/stage-02").is_dir()
        )

    def test_target_bound_stage_cannot_be_cross_qualified(self) -> None:
        state = self.fixture.state("cross-qualification")
        self.assert_success(self.fixture.run(state, stage=1, target="qemu"))
        self.assert_success(self.fixture.run(state, stage=2, target="qemu"))

        result = subprocess.run(
            [
                sys.executable,
                "scripts/ci/test_plan_evidence.py",
                "qualify",
                "--root",
                str(self.fixture.root),
                "--state-dir",
                str(state),
                "--stage",
                "2",
                "--target",
                "pi4",
            ],
            cwd=self.fixture.root,
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "cannot qualify target-bound stage evidence for another target",
            result.stderr,
        )
        self.assertFalse(
            (state / "stage_02.pi4.attestation.json").exists()
        )

    def test_default_sel4_profile_is_bound_even_when_missing(self) -> None:
        state = self.fixture.state("profile-context")
        state.mkdir(parents=True)
        context_before = state / "before.json"
        command = [
            sys.executable,
            "scripts/ci/test_plan_evidence.py",
            "capture-context",
            "--root",
            str(self.fixture.root),
            "--state-dir",
            str(state),
            "--stage",
            "2",
            "--target",
            "qemu",
            "--output",
            str(context_before),
        ]
        first = subprocess.run(
            command,
            cwd=self.fixture.root,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(first.returncode, 0, first.stderr)
        before = json.loads(context_before.read_text(encoding="utf-8"))
        external = before["config"]["external"]
        self.assertEqual(len(external), 1)
        self.assertFalse(external[0]["exists"])
        profile = (
            self.fixture.root
            / "out/sel4/profile-v2/qemu-smp-production"
        )
        profile.mkdir(parents=True)
        (profile / "CMakeCache.txt").write_text(
            "KernelArmExportVCNTUser=ON\n",
            encoding="utf-8",
        )
        (profile / "kernel").mkdir()
        kernel = profile / "kernel/kernel.elf"
        kernel.write_bytes(b"kernel-v1")
        context_after = state / "after.json"
        command[-1] = str(context_after)
        second = subprocess.run(
            command,
            cwd=self.fixture.root,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(second.returncode, 0, second.stderr)
        after = json.loads(context_after.read_text(encoding="utf-8"))
        self.assertNotEqual(before["config_digest"], after["config_digest"])
        self.assertTrue(after["config"]["external"][0]["exists"])
        kernel.write_bytes(b"kernel-v2")
        context_changed = state / "changed.json"
        command[-1] = str(context_changed)
        third = subprocess.run(
            command,
            cwd=self.fixture.root,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(third.returncode, 0, third.stderr)
        changed = json.loads(context_changed.read_text(encoding="utf-8"))
        self.assertNotEqual(after["config_digest"], changed["config_digest"])


if __name__ == "__main__":
    unittest.main()
