#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify acceptance and convergence catalog validation, routing, and documentation.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import copy
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CATALOG_TOOL = REPO_ROOT / "scripts" / "ci" / "test_plan_catalog.py"
CATALOG_PATH = REPO_ROOT / "configs" / "test_plan_actions.toml"

SPEC = importlib.util.spec_from_file_location("test_plan_catalog", CATALOG_TOOL)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to import {CATALOG_TOOL}")
catalog = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = catalog
SPEC.loader.exec_module(catalog)


class TestPlanCatalogTests(unittest.TestCase):
    def test_repository_catalog_is_valid_and_covers_every_stage(self) -> None:
        data = catalog.load_catalog(CATALOG_PATH)

        self.assertGreater(len(data["action"]), 20)
        for stage in range(1, 6):
            self.assertTrue(catalog.select_actions(data, stage=stage))
        self.assertTrue(catalog.select_actions(data, stage=0))
        self.assertTrue(
            all(
                action.get("evidence_class", "acceptance") == "acceptance"
                for action in catalog.select_actions(data, stage=0)
            )
        )

    def test_exact_duplicate_command_is_rejected(self) -> None:
        data = catalog.load_catalog(CATALOG_PATH)
        modified = copy.deepcopy(data)
        duplicate = copy.deepcopy(modified["action"][0])
        duplicate["id"] = "integrity.duplicate"
        modified["action"].append(duplicate)

        with self.assertRaisesRegex(catalog.CatalogError, "duplicates command"):
            catalog.validate_catalog(modified, CATALOG_PATH)

    def test_filtered_library_test_is_rejected(self) -> None:
        data = catalog.load_catalog(CATALOG_PATH)
        modified = copy.deepcopy(data)
        action = next(
            item
            for item in modified["action"]
            if item["id"] == "host.root-task-qemu-features"
        )
        action["command"] += " drivers::virtio"

        with self.assertRaisesRegex(
            catalog.CatalogError, "filtered --lib tests are forbidden"
        ):
            catalog.validate_catalog(modified, CATALOG_PATH)

    def test_workspace_and_default_package_overlap_is_rejected(self) -> None:
        data = catalog.load_catalog(CATALOG_PATH)
        modified = copy.deepcopy(data)
        workspace = next(
            item
            for item in modified["action"]
            if item["id"] == "host.workspace-tests"
        )
        workspace["command"] = workspace["command"].replace(
            " --exclude swarmui",
            "",
        )

        with self.assertRaisesRegex(
            catalog.CatalogError,
            "default-feature package tests overlap host.workspace-tests",
        ):
            catalog.validate_catalog(modified, CATALOG_PATH)

    def test_missing_repository_command_path_is_rejected(self) -> None:
        data = catalog.load_catalog(CATALOG_PATH)
        modified = copy.deepcopy(data)
        action = next(
            item
            for item in modified["action"]
            if item["id"] == "host.swarmui-dependency-policy"
        )
        action["command"] = "python3 scripts/ci/does_not_exist.py"

        with self.assertRaisesRegex(
            catalog.CatalogError, "references missing repository file"
        ):
            catalog.validate_catalog(modified, CATALOG_PATH)

    def test_unmatched_path_selects_full_catalog_fail_closed(self) -> None:
        data = catalog.load_catalog(CATALOG_PATH)

        actions, unmatched = catalog.matching_actions(
            data, ["unknown-surface/new.file"]
        )

        self.assertEqual(unmatched, ["unknown-surface/new.file"])
        acceptance_count = sum(
            action.get("evidence_class", "acceptance") == "acceptance"
            for action in data["action"]
        )
        self.assertEqual(len(actions), acceptance_count)
        self.assertNotIn(
            "diagnostic.qemu-canary",
            [action["id"] for action in actions],
        )

    def test_recommend_accepts_nul_delimited_paths(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                str(CATALOG_TOOL),
                "recommend",
                "--stdin0",
                "--format",
                "tiers",
            ],
            cwd=REPO_ROOT,
            input="apps/swarmui/src/lib.rs\0",
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("common-hermetic", result.stdout.splitlines())
        self.assertIn("ui", result.stdout.splitlines())

    def test_recommend_rejects_an_empty_change_set(self) -> None:
        result = subprocess.run(
            [sys.executable, str(CATALOG_TOOL), "recommend", "--stdin0"],
            cwd=REPO_ROOT,
            input="",
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires changed paths", result.stderr)

    def test_document_check_rejects_a_stale_generated_block(self) -> None:
        data = catalog.load_catalog(CATALOG_PATH)
        rendered = catalog.markdown_catalog(data)
        convergence_rendered = catalog.markdown_convergence(data)
        with tempfile.TemporaryDirectory() as temporary:
            document = Path(temporary) / "TEST_PLAN.md"
            document.write_text(
                "# Test\n\n"
                f"{catalog.DOC_START}\n"
                "stale\n"
                f"{catalog.DOC_END}\n"
                f"{catalog.CONVERGENCE_DOC_START}\n"
                "stale\n"
                f"{catalog.CONVERGENCE_DOC_END}\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(CATALOG_TOOL),
                    "check-doc",
                    "--doc",
                    str(document),
                ],
                cwd=REPO_ROOT,
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("generated action catalog is stale", result.stderr)

            document.write_text(
                f"# Test\n\n{rendered}\n\n{convergence_rendered}\n",
                encoding="utf-8",
            )
            current = subprocess.run(
                [
                    sys.executable,
                    str(CATALOG_TOOL),
                    "check-doc",
                    "--doc",
                    str(document),
                ],
                cwd=REPO_ROOT,
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(current.returncode, 0, current.stderr)


if __name__ == "__main__":
    unittest.main()
