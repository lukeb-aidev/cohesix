#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify fixture hashes and executable test-plan inventory checks fail closed.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
TOOL = REPO_ROOT / "scripts" / "ci" / "test_plan_integrity.py"
SPEC = importlib.util.spec_from_file_location("test_plan_integrity", TOOL)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to import {TOOL}")
integrity = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = integrity
SPEC.loader.exec_module(integrity)


class TestPlanIntegrityTests(unittest.TestCase):
    def test_fixture_hash_verifier_accepts_current_document(self) -> None:
        errors: list[str] = []

        integrity.verify_fixture_hashes(
            errors,
            integrity.DOC.read_text(encoding="utf-8"),
        )

        self.assertEqual(errors, [])

    def test_runner_inventory_names_each_stage_once(self) -> None:
        errors: list[str] = []

        integrity.verify_runner_list(errors)

        self.assertEqual(errors, [])

    def test_python_lane_contains_contract_tests(self) -> None:
        errors: list[str] = []

        integrity.verify_python_lane(errors)

        self.assertEqual(errors, [])

    def test_catalog_document_projection_is_current(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "scripts/ci/test_plan_catalog.py",
                "check-doc",
                "--doc",
                "docs/TEST_PLAN.md",
            ],
            cwd=REPO_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
