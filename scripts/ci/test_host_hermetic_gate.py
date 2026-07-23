# Author: Lukas Bower
# Purpose: Verify host gate catalog selection is complete, unique, and target-qualified.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import re
import subprocess
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
GATE = REPO_ROOT / "scripts" / "ci" / "host_hermetic_gate.sh"
SELECT_RE = re.compile(
    r"^\[host-gate\] SELECT scope=(?P<scope>[^ ]+)"
    r"(?: target=(?P<target>[^ ]+))? action=(?P<action>[^ ]+)$"
)


class HostHermeticGateTests(unittest.TestCase):
    def run_gate(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(GATE), *arguments, "--list"],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    def selections(
        self, result: subprocess.CompletedProcess[str]
    ) -> list[tuple[str, str | None, str]]:
        self.assertEqual(result.returncode, 0, result.stderr)
        selected = []
        for line in result.stdout.splitlines():
            match = SELECT_RE.fullmatch(line)
            self.assertIsNotNone(match, line)
            selected.append(
                (
                    match.group("scope"),
                    match.group("target"),
                    match.group("action"),
                )
            )
        return selected

    def test_common_lane_contains_no_provisioned_actions(self) -> None:
        selected = self.selections(self.run_gate("--common-only"))

        self.assertGreater(len(selected), 10)
        self.assertTrue(all(scope == "common" for scope, _, _ in selected))
        self.assertTrue(all(target is None for _, target, _ in selected))
        action_ids = [action for _, _, action in selected]
        self.assertEqual(len(action_ids), len(set(action_ids)))
        self.assertTrue(all(action.startswith("host.") for action in action_ids))

    def test_integrity_lane_contains_only_integrity_actions(self) -> None:
        selected = self.selections(self.run_gate("--integrity-only"))

        self.assertEqual(
            [action for _, _, action in selected],
            [
                "integrity.cargo-metadata",
                "integrity.generated-contracts",
            ],
        )

    def test_qemu_lane_adds_only_qemu_provisioned_actions(self) -> None:
        selected = self.selections(self.run_gate("--target", "qemu"))
        provisioned = [
            (target, action)
            for scope, target, action in selected
            if scope == "provisioned-target"
        ]

        self.assertEqual(
            provisioned,
            [
                ("qemu", "target.qemu-profile"),
                ("qemu", "target.root-task-qemu-release"),
            ],
        )
        self.assertNotIn(
            "target.root-task-pi4-release",
            [action for _, _, action in selected],
        )

    def test_pi4_lane_adds_only_pi4_provisioned_actions(self) -> None:
        selected = self.selections(self.run_gate("--target", "pi4"))
        provisioned = [
            (target, action)
            for scope, target, action in selected
            if scope == "provisioned-target"
        ]

        self.assertEqual(
            provisioned,
            [
                ("pi4", "target.pi4-profile"),
                ("pi4", "target.root-task-pi4-release"),
            ],
        )
        self.assertNotIn(
            "target.root-task-qemu-release",
            [action for _, _, action in selected],
        )

    def test_unknown_target_fails_closed(self) -> None:
        result = self.run_gate("--target", "other")

        self.assertEqual(result.returncode, 2)
        self.assertIn("unsupported test-plan target", result.stderr)

    def test_execution_initializes_and_enforces_catalog_policy(self) -> None:
        source = GATE.read_text(encoding="utf-8")

        self.assertIn('TP_EVIDENCE_TOOL="${TEST_PLAN_ROOT}/', source)
        self.assertIn("tp_catalog_execute \\", source)


if __name__ == "__main__":
    unittest.main()
