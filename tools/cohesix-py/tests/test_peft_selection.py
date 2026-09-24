# Author: Lukas Bower
# Purpose: Keep M28b training and held-out selections disjoint, bounded and offline before native profile creation.
# Copyright 2026 Lukas Bower
"""Pure selection checks; GPU execution and generated capabilities are separate proof."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location(
    "private_lora_release", ROOT / "tools/cohesix-py/examples/private_lora_release.py")
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def fixture(root: Path) -> Path:
    train = root / "train.json"
    heldout = root / "heldout.json"
    train.write_text(json.dumps([f"Licensed training paragraph number {i}" for i in range(32)]))
    heldout.write_text(json.dumps([f"Separate held-out paragraph number {i}" for i in range(16)]))
    template = (ROOT / "configs/peft_m28b_selection.example.toml").read_text()
    template = template.replace("/absolute/private/train.json", str(train)).replace(
        "/absolute/private/heldout.json", str(heldout))
    selection = root / "selection.toml"
    selection.write_text(template)
    return selection


def test_selected_inputs_are_disjoint_and_offline(tmp_path: Path) -> None:
    selected, train, eval_rows = MODULE.selection_data(fixture(tmp_path))
    assert len(train) == 32 and len(eval_rows) == 16
    assert selected["settings"]["checkpoint_interval"] == 8
    selected_file = tmp_path / "selection.toml"
    selected_file.write_text(selected_file.read_text().replace("upload = false", "upload = true"))
    with pytest.raises(ValueError, match="offline"):
        MODULE.selection_data(selected_file)
    selected_file = fixture(tmp_path)
    (tmp_path / "heldout.json").write_text(json.dumps(train[:16]))
    with pytest.raises(ValueError, match="distinct"):
        MODULE.selection_data(selected_file)
