# Author: Lukas Bower
# Purpose: Protect MLX local artifact, dataset and request bounds before native Metal execution.
# Copyright 2026 Lukas Bower
"""Pure refusal checks complement separately retained device-qualified MLX runs."""

from pathlib import Path
import json

import pytest

from cohesix.mlx_native import (MlxRefusal, MlxSelection, evaluate_heldout,
                                infer, train_lora, tree_digest)


def selection(tmp_path: Path) -> MlxSelection:
    """Build a distinct held-out local fixture with immutable selected hashes."""
    model = tmp_path / "model"
    data = tmp_path / "data"
    model.mkdir()
    data.mkdir()
    (model / "config.json").write_text('{"model_type":"fixture"}')
    for split, count in (("train", 16), ("valid", 4), ("test", 4)):
        rows = [{"text": f"Question {split} {index}: what is observed? Answer: exact evidence."}
                for index in range(count)]
        (data / f"{split}.jsonl").write_text(
            "".join(json.dumps(row) + "\n" for row in rows))
    return MlxSelection(model.resolve(), tree_digest(model.resolve(), 1024),
                        data.resolve(), tree_digest(data.resolve(), 262_144, 3),
                        536_870_912)


def test_selection_rejects_changed_model_and_cross_split_data(tmp_path: Path) -> None:
    selected = selection(tmp_path)
    selected.validate()
    (selected.model_directory / "config.json").write_text('{"model_type":"changed"}')
    with pytest.raises(MlxRefusal, match="mlx_model_changed"):
        selected.validate()
    (selected.model_directory / "config.json").write_text('{"model_type":"fixture"}')
    train = selected.data_directory / "train.jsonl"
    test = selected.data_directory / "test.jsonl"
    test.write_text(test.read_text().replace("Question test 0", "Question train 0"))
    changed = MlxSelection(selected.model_directory, selected.model_sha256,
                           selected.data_directory,
                           tree_digest(selected.data_directory, 262_144, 3),
                           selected.memory_limit_bytes)
    with pytest.raises(MlxRefusal, match="mlx_data_leakage"):
        changed.validate()
    assert train.is_file()


def test_symlinked_artifact_and_unbounded_requests_refuse(tmp_path: Path) -> None:
    selected = selection(tmp_path)
    (selected.model_directory / "outside.txt").symlink_to(tmp_path / "outside")
    with pytest.raises(MlxRefusal, match="mlx_file_name_or_kind"):
        tree_digest(selected.model_directory, 1024)
    (selected.model_directory / "outside.txt").unlink()
    with pytest.raises(MlxRefusal, match="mlx_prompt_bound"):
        infer(selected, "", 4)
    with pytest.raises(MlxRefusal, match="mlx_token_bound"):
        infer(selected, "prompt", 65)
    with pytest.raises(MlxRefusal, match="mlx_adapter_selection"):
        infer(selected, "prompt", 4, tmp_path)
    with pytest.raises(MlxRefusal, match="mlx_adapter_selection"):
        evaluate_heldout(selected, tmp_path)


def test_training_bounds_refuse_before_native_work(tmp_path: Path) -> None:
    selected = selection(tmp_path)
    output = tmp_path / "output"
    output.mkdir(mode=0o700)
    with pytest.raises(MlxRefusal, match="mlx_training_bounds"):
        train_lora(selected, output, steps=0, rank=2, seed=1,
                   deadline_unix_ms=1, cancelled=lambda: False)
    output.chmod(0o755)
    with pytest.raises(MlxRefusal, match="mlx_empty_private_output"):
        train_lora(selected, output, steps=2, rank=2, seed=1,
                   deadline_unix_ms=3_000_000_000_000, cancelled=lambda: False)
    output.chmod(0o700)
    with pytest.raises(MlxRefusal, match="mlx_deadline"):
        train_lora(selected, output, steps=2, rank=2, seed=1,
                   deadline_unix_ms=1, cancelled=lambda: False)


def test_adapter_requires_selected_provenance_before_loading(tmp_path: Path) -> None:
    selected = selection(tmp_path)
    adapter = tmp_path / "adapter"
    adapter.mkdir()
    (adapter / "adapters.safetensors").write_bytes(b"diagnostic only")
    config = {
        "fine_tune_type": "lora", "num_layers": 4,
        "lora_parameters": {"rank": 2, "scale": 20.0, "dropout": 0.0},
        "cohesix_model_sha256": "0" * 64,
        "cohesix_data_sha256": selected.data_sha256,
        "cohesix_versions": {"mlx": "0.32.2", "mlx-lm": "0.31.3"},
    }
    (adapter / "adapter_config.json").write_text(json.dumps(config))
    digest = tree_digest(adapter.resolve(), 1024, 4)
    with pytest.raises(MlxRefusal, match="mlx_adapter_provenance"):
        infer(selected, "prompt", 4, adapter.resolve(), digest)
    config["cohesix_model_sha256"] = selected.model_sha256
    config["lora_parameters"]["rank"] = 2048
    (adapter / "adapter_config.json").write_text(json.dumps(config))
    digest = tree_digest(adapter.resolve(), 1024, 4)
    with pytest.raises(MlxRefusal, match="mlx_adapter_parameters"):
        infer(selected, "prompt", 4, adapter.resolve(), digest)
