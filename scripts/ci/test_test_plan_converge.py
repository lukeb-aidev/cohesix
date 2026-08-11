#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify convergence routing, non-claiming evidence, and fail-fast behavior.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CATALOG_PATH = REPO_ROOT / "configs" / "test_plan_actions.toml"
CATALOG_TOOL = REPO_ROOT / "scripts" / "ci" / "test_plan_catalog.py"
CONVERGE_TOOL = REPO_ROOT / "scripts" / "ci" / "test_plan_converge.py"
TARGET_CANARY = REPO_ROOT / "scripts" / "ci" / "test_plan_target_canary.sh"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


catalog = load_module("test_plan_catalog", CATALOG_TOOL)
converge = load_module("convergence_test_runner", CONVERGE_TOOL)


def fake_catalog(*, fail_entry: bool = False) -> str:
    entry_command = "exit 7" if fail_entry else "printf entry-ok"
    return textwrap.dedent(
        f"""\
        # Author: Lukas Bower
        # Purpose: Define fake convergence actions for runner tests.
        # Copyright 2026 Lukas Bower

        [catalog]
        schema = "cohesix-test-plan-actions/v2"
        minimum_python = "3.11"
        default_claims = ["common-hermetic"]
        claim_tiers = ["common-hermetic", "qemu-integration", "release"]

        [[convergence_focus]]
        id = "root-mcs"
        targets = ["qemu"]
        description = "Fake root target focus."
        profile = "fake-qemu-profile"
        authoritative_evidence = "fake target observation"
        priority = 10
        trigger_paths = ["apps/root-task/**"]

        [[action]]
        id = "accept.stage1"
        stage = 1
        tier = "common-hermetic"
        scope = "common"
        targets = ["qemu"]
        description = "Fake acceptance stage one."
        command = "printf acceptance-one"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage2"
        stage = 2
        tier = "qemu-integration"
        scope = "provisioned-target"
        targets = ["qemu"]
        description = "Fake acceptance stage two."
        command = "printf acceptance-two"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage3"
        stage = 3
        tier = "qemu-integration"
        scope = "target"
        targets = ["qemu"]
        description = "Fake acceptance stage three."
        command = "printf acceptance-three"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage4"
        stage = 4
        tier = "qemu-integration"
        scope = "target"
        targets = ["qemu"]
        description = "Fake acceptance stage four."
        command = "printf acceptance-four"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage5"
        stage = 5
        tier = "release"
        scope = "target"
        targets = ["qemu"]
        description = "Fake acceptance stage five."
        command = "printf acceptance-five"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "diagnostic.entry"
        stage = 0
        tier = "non-claiming"
        evidence_class = "diagnostic"
        scope = "conditional"
        targets = ["qemu"]
        description = "Fake target-entry action."
        command = "{entry_command}"
        timeout_seconds = 30
        trigger_paths = ["apps/root-task/**"]
        expected_evidence = ["command-result"]
        test_policy = "none"
        convergence_focuses = ["root-mcs"]
        convergence_phase = "target-entry"

        [[action]]
        id = "diagnostic.canary"
        stage = 0
        tier = "non-claiming"
        evidence_class = "diagnostic"
        scope = "conditional"
        targets = ["qemu"]
        description = "Write one fake target observation."
        command = '''printf '%s\n' '{{"schema":"cohesix-target-observation/v1","claiming":false,"result":"PASS"}}' > "$TEST_PLAN_TARGET_OBSERVATION"'''
        timeout_seconds = 30
        trigger_paths = ["apps/root-task/**"]
        expected_evidence = ["non-claiming-target-observation"]
        test_policy = "none"
        convergence_focuses = ["root-mcs"]
        convergence_phase = "target-canary"

        [[action]]
        id = "diagnostic.guard"
        stage = 0
        tier = "non-claiming"
        evidence_class = "diagnostic"
        scope = "conditional"
        targets = ["qemu"]
        description = "Run one fake focused regression."
        command = "printf 'test result: ok. 1 passed\\n'"
        timeout_seconds = 30
        trigger_paths = ["apps/root-task/**"]
        expected_evidence = ["focused-regression-result"]
        test_policy = "nonzero"
        minimum_test_count = 1
        convergence_focuses = ["root-mcs"]
        convergence_phase = "focused-regression"
        """
    )


class ConvergenceSelectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.data = catalog.load_catalog(CATALOG_PATH)

    def selected(self, target: str, path: str) -> str:
        focus, _ = catalog.select_convergence_focus(
            self.data,
            target=target,
            changed_paths=[path],
        )
        return focus["id"]

    def test_representative_changed_paths_select_first_authority(self) -> None:
        self.assertEqual(
            self.selected("qemu", "apps/root-task/src/kernel.rs"),
            "root-mcs",
        )
        self.assertEqual(
            self.selected("pi4", "apps/pi4-driver-runtime/src/lib.rs"),
            "pi4-driver",
        )
        self.assertEqual(
            self.selected("qemu", "tools/cohesix-py/cohesix/client.py"),
            "python-sdk",
        )
        self.assertEqual(self.selected("qemu", "docs/SECURITY.md"), "docs")

    def test_pi_driver_change_rejects_qemu_as_first_authority(self) -> None:
        with self.assertRaisesRegex(
            catalog.CatalogError,
            "requires target pi4",
        ):
            catalog.select_convergence_focus(
                self.data,
                target="qemu",
                changed_paths=["apps/pi4-driver-runtime/src/lib.rs"],
            )

    def test_root_plan_runs_canary_before_focused_guard(self) -> None:
        actions = catalog.convergence_actions(
            self.data,
            target="qemu",
            focus_id="root-mcs",
        )
        phases = [action["convergence_phase"] for action in actions]
        self.assertLess(phases.index("target-canary"), phases.index("focused-regression"))
        self.assertNotIn("host.workspace-tests", [action["id"] for action in actions])


class ConvergenceEvidenceTests(unittest.TestCase):
    def invoke(self, catalog_text: str):
        out_root = REPO_ROOT / "out"
        out_root.mkdir(exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=out_root)
        parent = Path(temporary.name)
        catalog_path = parent / "catalog.toml"
        state = parent / "state"
        catalog_path.write_text(catalog_text, encoding="utf-8")
        result = subprocess.run(
            [
                sys.executable,
                str(CONVERGE_TOOL),
                "--catalog",
                str(catalog_path),
                "--target",
                "qemu",
                "--focus",
                "root-mcs",
                "--path",
                "apps/root-task/src/kernel.rs",
                "--state-dir",
                str(state),
            ],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        return temporary, state, result

    def test_run_needs_no_acceptance_attestation_and_is_non_claiming(self) -> None:
        temporary, state, result = self.invoke(fake_catalog())
        self.addCleanup(temporary.cleanup)
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(
            (state / "convergence-result.json").read_text(encoding="utf-8")
        )
        self.assertEqual(payload["schema"], converge.RESULT_SCHEMA)
        self.assertFalse(payload["claiming"])
        self.assertFalse(payload["promotion_eligible"])
        self.assertEqual(payload["result"], "PASS")
        self.assertEqual(
            [action["id"] for action in payload["actions"]],
            ["diagnostic.entry", "diagnostic.canary", "diagnostic.guard"],
        )
        self.assertIn(converge.BANNER, result.stdout)
        self.assertFalse(list(state.glob("stage_*.done")))
        self.assertNotIn("cohesix.test-plan-stage", json.dumps(payload))
        self.assertIn("git_commit", payload["source"])
        self.assertIn("source_digest", payload["source"])
        self.assertIn("uart_serial_log", payload)
        self.assertIn("built_image", payload)
        self.assertIn("image_identity", payload)

    def test_first_failure_stops_before_target_and_guard(self) -> None:
        temporary, state, result = self.invoke(fake_catalog(fail_entry=True))
        self.addCleanup(temporary.cleanup)
        self.assertEqual(result.returncode, 1, result.stderr)
        payload = json.loads(
            (state / "convergence-result.json").read_text(encoding="utf-8")
        )
        self.assertEqual(payload["result"], "FAIL")
        self.assertEqual(payload["first_failing_proof_layer"], "target-entry")
        self.assertEqual(len(payload["actions"]), 1)

    def test_tampered_candidate_result_is_rejected(self) -> None:
        temporary, state, result = self.invoke(fake_catalog())
        self.addCleanup(temporary.cleanup)
        self.assertEqual(result.returncode, 0, result.stderr)
        result_path = state / "convergence-result.json"
        payload = json.loads(result_path.read_text(encoding="utf-8"))
        payload["promotion_eligible"] = True
        result_path.write_text(json.dumps(payload), encoding="utf-8")
        with self.assertRaisesRegex(
            converge.ConvergenceError,
            "promotion_eligible=false",
        ):
            converge.validate_result(result_path)

    def test_pi_canary_without_hardware_inputs_is_blocked(self) -> None:
        out_root = REPO_ROOT / "out"
        out_root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=out_root) as temporary:
            state = Path(temporary)
            observation = state / "target-observation.json"
            environment = os.environ.copy()
            environment.update(
                {
                    "TEST_PLAN_CONVERGENCE": "1",
                    "TEST_PLAN_CONVERGENCE_FOCUS": "pi4-driver",
                    "TEST_PLAN_CONVERGENCE_RUN_ID": "blocked-test",
                    "TEST_PLAN_CONVERGENCE_STATE_DIR": str(state),
                    "TEST_PLAN_TARGET_OBSERVATION": str(observation),
                }
            )
            result = subprocess.run(
                ["bash", str(TARGET_CANARY), "--target", "pi4"],
                cwd=REPO_ROOT,
                env=environment,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(result.returncode, 3, result.stderr)
            payload = json.loads(observation.read_text(encoding="utf-8"))
            self.assertEqual(payload["result"], "BLOCKED")
            self.assertFalse(payload["claiming"])


if __name__ == "__main__":
    unittest.main()
