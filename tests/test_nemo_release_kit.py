# Author: Lukas Bower
# Purpose: Verify that the Release B NeMo wheel contains exactly the compiler-selected client source.
# Copyright 2026 Lukas Bower
"""Focused offline-kit checks without claiming a Toolkit host or target run."""

from __future__ import annotations

import json
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/install"))
import build_nemo_kit as kit  # noqa: E402


def test_selected_nemo_wheel_binds_exact_source(tmp_path: Path) -> None:
    inventory = ROOT / "configs/generated/implementation_surface_inventory.json"
    report = kit.build(ROOT, inventory, tmp_path / "nemo")
    wheel = tmp_path / "nemo" / report["wheel"]["filename"]
    assert wheel.is_file()
    assert report["wheel"]["version"] == "1.2.0"
    assert report["proof_boundary"]["kit_install_is_target_proof"] is False
    expected = kit.selected_sources(json.loads(inventory.read_text()))
    assert set(report["source_files"]) == set(expected)
    assert kit.verify(ROOT, inventory, tmp_path / "nemo") == report
    changed = {**report["source_files"], "src/cohesix_nemo_kit/native.py": "0" * 64}
    with pytest.raises(ValueError, match="source digest mismatch"):
        kit.inspect_wheel(wheel, changed, "1.2.0")
    (tmp_path / "nemo/nemo-distribution.json").write_text(
        json.dumps({**report, "inventory_sha256": "0" * 64})
    )
    with pytest.raises(ValueError, match="differs from selected release source"):
        kit.verify(ROOT, inventory, tmp_path / "nemo")


def test_nemo_release_source_selection_rejects_missing_member() -> None:
    inventory = json.loads((
        ROOT / "configs/generated/implementation_surface_inventory.json"
    ).read_text())
    inventory["release"]["support_files"].remove(
        "integrations/nemo-agent-toolkit/src/cohesix_nemo_kit/native.py"
    )
    with pytest.raises(ValueError, match="incomplete"):
        kit.selected_sources(inventory)


def test_nemo_wheel_version_must_match_selected_release(tmp_path: Path) -> None:
    """A valid wheel source with a stale package version cannot enter Release B."""
    inventory = json.loads((
        ROOT / "configs/generated/implementation_surface_inventory.json"
    ).read_text())
    inventory["release"]["version"] = "1.3.0-beta"
    selected = tmp_path / "inventory.json"
    selected.write_text(json.dumps(inventory))
    with pytest.raises(ValueError, match="version differs from selected release"):
        kit.build(ROOT, selected, tmp_path / "nemo")
