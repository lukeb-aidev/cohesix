# Author: Lukas Bower
# Purpose: Verify bounded private selection and honest proof classification in the local MLX workbench.
# Copyright 2026 Lukas Bower
"""Focused local workbench contract tests without requiring Metal in CI."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from cohesix.mlx_native import MlxRefusal, tree_digest
from cohesix.mlx_workbench import execute, selected_request


def request(tmp_path: Path) -> dict:
    """Select small independent local rows with exact content hashes."""
    model = tmp_path / "model"
    model.mkdir()
    (model / "config.json").write_text("{}")
    data = tmp_path / "data"
    data.mkdir()
    for name, count in (("train", 16), ("valid", 4), ("test", 4)):
        (data / f"{name}.jsonl").write_text("".join(
            json.dumps({"text": f"Distinct {name} example number {i:02d}."}) + "\n"
            for i in range(count)))
    selection = tmp_path / "selected.json"
    selection.write_text(json.dumps({
        "model_directory": str(model),
        "model_sha256": tree_digest(model, 1_073_741_824),
        "data_directory": str(data),
        "data_sha256": tree_digest(data, 262_144, 3),
        "memory_limit_bytes": 1_073_741_824,
    }))
    return {"selection_path": str(selection), "operation": "infer",
            "prompt": "What happened?", "max_tokens": 16}


def test_private_selection_and_operation_bounds(tmp_path: Path, monkeypatch) -> None:
    selected = request(tmp_path)
    selection, adapter, digest = selected_request(selected)
    assert selection.model_directory == tmp_path / "model"
    assert adapter is None and digest is None
    monkeypatch.setattr("cohesix.mlx_workbench.infer", lambda *_args: type(
        "Result", (), {"text": "Local answer", "evidence": lambda _self: {
            "model_sha256": selection.model_sha256, "device_name": "Apple GPU"}})())
    report = execute(selected)
    assert report["proof_class"] == "local_metal_observation"
    assert report["observation"]["text"] == "Local answer"
    selected["operation"] = "promote"
    with pytest.raises(MlxRefusal, match="mlx_workbench_operation"):
        execute(selected)
    selected["operation"] = "evaluate"
    with pytest.raises(MlxRefusal, match="mlx_workbench_evaluate_fields"):
        execute(selected)


def test_changed_inputs_and_unknown_fields_refuse(tmp_path: Path) -> None:
    selected = request(tmp_path)
    (tmp_path / "model" / "config.json").write_text('{"changed":true}')
    with pytest.raises(MlxRefusal, match="mlx_model_changed"):
        selected_request(selected)
    selected["unexpected"] = "ignored?"
    with pytest.raises(MlxRefusal, match="mlx_workbench_request_fields"):
        selected_request(selected)
