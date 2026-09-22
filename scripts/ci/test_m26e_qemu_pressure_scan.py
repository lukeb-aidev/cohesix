# Author: Lukas Bower
# Purpose: Verify retained pressure evidence accepts only confined Test Plan log links.
# Copyright 2026 Lukas Bower

"""Boundary checks for the QEMU pressure collector's final evidence scan."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "m26e_qemu_pressure.sh"
SCANNER = SCRIPT.read_text(encoding="utf-8").rsplit(
    'python3 - "$RUN_DIR" <<\'PY\'\n', 1
)[1].split("\nPY\nunset M26E_CONSOLE_AUTH_TOKEN", 1)[0]


class RetainedEvidenceScanTests(unittest.TestCase):
    """Test real scanner code with stage logs and hostile link destinations."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.logs = self.root / "test-plan/logs"
        self.logs.mkdir(parents=True)
        self.stage_log = (
            self.root / "test-plan/evidence/attempts/stage-04/attempt-1/stage.log"
        )
        self.stage_log.parent.mkdir(parents=True)
        self.stage_log.write_text("completed\n", encoding="utf-8")

    def scan(self) -> subprocess.CompletedProcess[str]:
        """Run the collector scan against a disposable evidence tree."""

        environment = os.environ.copy()
        environment["M26E_SCAN_CONSOLE_TOKEN"] = "fixture-console-token"
        environment["M26E_SCAN_REST_TOKEN"] = "fixture-rest-token"
        return subprocess.run(
            [sys.executable, "-", str(self.root)],
            input=SCANNER,
            text=True,
            capture_output=True,
            env=environment,
            check=False,
        )

    def test_confined_stage_log_link_passes(self) -> None:
        link = self.logs / "stage-04-rest-multiplexer.log"
        link.symlink_to(os.path.relpath(self.stage_log, self.logs))
        self.assertEqual(self.scan().returncode, 0)

    def test_link_outside_attempts_fails(self) -> None:
        link = self.logs / "stage-04-rest-multiplexer.log"
        link.symlink_to(os.path.relpath(self.root / "outside.log", self.logs))
        (self.root / "outside.log").write_text("not an attempt\n", encoding="utf-8")
        self.assertNotEqual(self.scan().returncode, 0)

    def test_mismatched_stage_and_unrelated_link_fail(self) -> None:
        link = self.logs / "stage-03-qemu-tcp-regression.log"
        link.symlink_to(os.path.relpath(self.stage_log, self.logs))
        self.assertNotEqual(self.scan().returncode, 0)
        link.unlink()
        (self.root / "unrelated").symlink_to(self.stage_log)
        self.assertNotEqual(self.scan().returncode, 0)

    def test_linked_stage_log_with_credential_fails(self) -> None:
        link = self.logs / "stage-04-rest-multiplexer.log"
        link.symlink_to(os.path.relpath(self.stage_log, self.logs))
        self.stage_log.write_text("AUTH fixture-console-token\n", encoding="utf-8")
        self.assertNotEqual(self.scan().returncode, 0)


if __name__ == "__main__":
    unittest.main()
