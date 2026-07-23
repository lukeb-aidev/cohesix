# Author: Lukas Bower
# Purpose: Verify the SwarmUI UI gate's lockfile binding and non-zero JUnit policy.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
GATE = REPO_ROOT / "scripts" / "ci" / "swarmui_ui_gate.sh"


class SwarmUiGateTests(unittest.TestCase):
    def run_gate(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(GATE), *arguments],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_list_contract_is_lockfile_bound_and_complete(self) -> None:
        result = self.run_gate("--list")

        self.assertEqual(result.returncode, 0, result.stderr)
        fields = dict(
            line.split("=", maxsplit=1)
            for line in result.stdout.splitlines()
            if "=" in line
        )
        self.assertRegex(fields["playwright_version"], r"^\d+\.\d+\.\d+$")
        self.assertRegex(fields["lock_sha256"], r"^[0-9a-f]{64}$")
        self.assertEqual(fields["browsers"], "webkit,chromium")
        self.assertEqual(fields["chromium_mode"], "new-headless-no-shell")
        self.assertEqual(
            fields["projects"],
            "webkit-desktop,webkit-narrow,chromium-tablet",
        )
        self.assertEqual(fields["source_root"], "apps/swarmui/frontend")

    def verify_fixture(self, xml: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as temp_dir:
            junit_path = Path(temp_dir) / "junit.xml"
            junit_path.write_text(xml, encoding="utf-8")
            return self.run_gate("--verify-junit", str(junit_path))

    def test_nonzero_clean_junit_passes(self) -> None:
        result = self.verify_fixture(
            '<testsuites><testsuite tests="7" failures="0" errors="0"/>'
            "</testsuites>"
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("SWARMUI_UI_TEST_COUNT=7", result.stdout)
        self.assertIn("SWARMUI_UI_PASS_COUNT=7", result.stdout)

    def test_zero_test_junit_fails_closed(self) -> None:
        result = self.verify_fixture(
            '<testsuites><testsuite tests="0" failures="0" errors="0"/>'
            "</testsuites>"
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("zero tests", result.stderr)

    def test_failing_junit_fails_closed(self) -> None:
        result = self.verify_fixture(
            '<testsuites><testsuite tests="7" failures="1" errors="0"/>'
            "</testsuites>"
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not clean", result.stderr)

    def test_all_skipped_junit_fails_closed(self) -> None:
        result = self.verify_fixture(
            '<testsuites><testsuite tests="7" failures="0" errors="0" '
            'skipped="7"/></testsuites>'
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("zero passing tests", result.stderr)


if __name__ == "__main__":
    unittest.main()
