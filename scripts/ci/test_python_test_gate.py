# Author: Lukas Bower
# Purpose: Verify the consolidated Python test gate selects complete, stable test and example lanes.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import os
import re
import subprocess
import sys
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
GATE = REPO_ROOT / "scripts" / "ci" / "python_test_gate.sh"


class PythonTestGateTests(unittest.TestCase):
    def run_gate(self, mode: str) -> subprocess.CompletedProcess[str]:
        environment = os.environ.copy()
        environment["TP_PYTHON_BIN"] = sys.executable
        return subprocess.run(
            ["bash", str(GATE), mode],
            cwd=REPO_ROOT,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_test_lane_uses_broad_discovery_and_ci_contract_tests(self) -> None:
        result = self.run_gate("--list-tests")

        self.assertEqual(result.returncode, 0, result.stderr)
        selected = result.stdout.splitlines()
        self.assertEqual(selected[:2], ["tests", "tools/cohesix-py/tests"])
        expected_contracts = {
            str(path.relative_to(REPO_ROOT))
            for path in (REPO_ROOT / "scripts" / "ci").glob("test_*.py")
            if path.name != "test_rust_risk_gate.py"
        }
        self.assertTrue(expected_contracts)
        self.assertTrue(expected_contracts.issubset(set(selected)))
        self.assertNotIn("scripts/ci/test_rust_risk_gate.py", selected)

    def test_example_lane_contains_exactly_four_smokes(self) -> None:
        result = self.run_gate("--list-examples")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout.splitlines(),
            [
                "lease-run",
                "peft-roundtrip",
                "telemetry-write-pull",
                "mixed-closed-loop-ai-factory",
            ],
        )

    def test_cache_key_is_bound_to_python_and_locked_requirements(self) -> None:
        first = self.run_gate("--cache-key")
        second = self.run_gate("--cache-key")

        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(first.stdout, second.stdout)
        self.assertRegex(
            first.stdout.strip(),
            re.compile(
                r"^python-tests-(CPython|PyPy|GraalVM)-\d+\.\d+\.\d+-[0-9a-f]{16}$"
            ),
        )

    def test_unknown_mode_fails_closed(self) -> None:
        result = self.run_gate("--unknown")

        self.assertEqual(result.returncode, 2)
        self.assertIn("Usage:", result.stderr)


if __name__ == "__main__":
    unittest.main()
