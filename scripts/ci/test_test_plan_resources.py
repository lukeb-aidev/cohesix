#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify bounded test-plan concurrency and explicit override behavior.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import os
import pathlib
import subprocess
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
RESOURCE_SCRIPT = REPO_ROOT / "scripts/ci/test_plan_resources.sh"


class TestPlanResourceTests(unittest.TestCase):
    """Exercise resource defaults without launching real build workloads."""

    def run_shell(
        self,
        body: str,
        *,
        environment: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        for key in (
            "CARGO_BUILD_JOBS",
            "CMAKE_BUILD_PARALLEL_LEVEL",
            "MAKEFLAGS",
            "RAYON_NUM_THREADS",
            "RUST_TEST_THREADS",
            "TP_ALLOW_OVERSUBSCRIBE",
            "TP_HOST_JOBS",
            "TP_PRESERVE_PARALLEL_ENV",
            "TP_UI_WORKERS",
        ):
            env.pop(key, None)
        if environment:
            env.update(environment)
        return subprocess.run(
            [
                "bash",
                "--noprofile",
                "--norc",
                "-c",
                f"source {RESOURCE_SCRIPT!s}; {body}",
            ],
            cwd=REPO_ROOT,
            env=env,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_ten_core_host_defaults_to_five_jobs_and_two_ui_workers(
        self,
    ) -> None:
        result = self.run_shell(
            "tp_detect_logical_cpus() { printf '10\\n'; }; "
            "tp_configure_resource_limits; "
            "printf '%s %s %s %s %s %s\\n' "
            '"$TP_HOST_JOBS" "$CARGO_BUILD_JOBS" '
            '"$RUST_TEST_THREADS" "$RAYON_NUM_THREADS" '
            '"$MAKEFLAGS" "$TP_UI_WORKERS"'
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "5 5 5 5 -j5 2\n")

    def test_explicit_safe_job_limit_is_applied_consistently(self) -> None:
        result = self.run_shell(
            "tp_detect_logical_cpus() { printf '10\\n'; }; "
            "tp_configure_resource_limits; "
            'printf "%s %s %s\\n" '
            '"$TP_HOST_JOBS" "$CMAKE_BUILD_PARALLEL_LEVEL" '
            '"$RUST_TEST_THREADS"',
            environment={"TP_HOST_JOBS": "3"},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "3 3 3\n")

    def test_oversubscription_requires_an_explicit_opt_in(self) -> None:
        rejected = self.run_shell(
            "tp_detect_logical_cpus() { printf '4\\n'; }; "
            "tp_configure_resource_limits",
            environment={"TP_HOST_JOBS": "8"},
        )
        accepted = self.run_shell(
            "tp_detect_logical_cpus() { printf '4\\n'; }; "
            "tp_configure_resource_limits; printf '%s\\n' \"$TP_HOST_JOBS\"",
            environment={
                "TP_ALLOW_OVERSUBSCRIBE": "1",
                "TP_HOST_JOBS": "8",
            },
        )

        self.assertEqual(rejected.returncode, 2)
        self.assertIn("exceeds detected CPUs=4", rejected.stderr)
        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertEqual(accepted.stdout, "8\n")


if __name__ == "__main__":
    unittest.main()
