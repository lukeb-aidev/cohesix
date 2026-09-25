# Author: Lukas Bower
# Purpose: Execute one selected local MLX observation for the SwarmUI developer workbench.
# Copyright 2026 Lukas Bower
"""Bounded local inference and evaluation; results carry no Cohesix admission."""

from __future__ import annotations

import json
from pathlib import Path
import sys
from typing import Any

from cohesix.mlx_native import (MAX_DATA_FILE_BYTES, MlxRefusal, MlxSelection,
                                _regular, evaluate_heldout, infer, require)

SCHEMA = "cohesix-local-mlx/v1"


def selected_request(payload: dict[str, Any]) -> tuple[MlxSelection, Path | None, str | None]:
    """Load only pinned, private local inputs from a bounded selection file."""
    require(isinstance(payload, dict) and set(payload) ==
            {"selection_path", "operation", "prompt", "max_tokens"},
            "mlx_workbench_request_fields")
    path = payload["selection_path"]
    require(isinstance(path, str) and 0 < len(path) <= 4096,
            "mlx_workbench_selection_path")
    raw = _regular(Path(path), MAX_DATA_FILE_BYTES)
    try:
        selected = json.loads(raw)
    except (UnicodeError, ValueError) as error:
        raise MlxRefusal("mlx_workbench_selection_json") from error
    required = {"model_directory", "model_sha256", "data_directory",
                "data_sha256", "memory_limit_bytes"}
    optional = {"adapter_directory", "adapter_sha256"}
    require(isinstance(selected, dict) and required <= set(selected)
            and set(selected) <= required | optional,
            "mlx_workbench_selection_fields")
    for key in required - {"memory_limit_bytes"}:
        require(isinstance(selected[key], str) and 0 < len(selected[key]) <= 4096,
                "mlx_workbench_selection_value")
    selection = MlxSelection(Path(selected["model_directory"]),
                             selected["model_sha256"],
                             Path(selected["data_directory"]),
                             selected["data_sha256"],
                             selected["memory_limit_bytes"])
    adapter_path = selected.get("adapter_directory")
    adapter_sha = selected.get("adapter_sha256")
    require((adapter_path is None) == (adapter_sha is None),
            "mlx_adapter_selection")
    if adapter_path is not None:
        require(isinstance(adapter_path, str) and 0 < len(adapter_path) <= 4096
                and isinstance(adapter_sha, str), "mlx_adapter_selection")
    selection.validate()
    return selection, Path(adapter_path) if adapter_path else None, adapter_sha


def execute(payload: dict[str, Any]) -> dict[str, Any]:
    """Return a local observation with native identity and an explicit proof class."""
    selection, adapter, adapter_sha = selected_request(payload)
    operation = payload["operation"]
    require(operation in {"infer", "evaluate"}, "mlx_workbench_operation")
    if operation == "infer":
        result = infer(selection, payload["prompt"], payload["max_tokens"],
                       adapter, adapter_sha)
        observation = {"text": result.text, **result.evidence()}
    else:
        require(payload["prompt"] == "" and payload["max_tokens"] == 0,
                "mlx_workbench_evaluate_fields")
        observation = evaluate_heldout(selection, adapter, adapter_sha)
    return {"schema": SCHEMA, "proof_class": "local_metal_observation",
            "operation": operation, "observation": observation}


def main() -> None:
    """Read one private JSON request on stdin and emit one bounded JSON result."""
    try:
        raw = sys.stdin.buffer.read(8193)
        require(0 < len(raw) <= 8192, "mlx_workbench_request_size")
        result = execute(json.loads(raw))
    except (MlxRefusal, ValueError, TypeError, KeyError) as error:
        result = {"schema": SCHEMA, "proof_class": "local_refusal",
                  "reason": str(error)[:128]}
    print(json.dumps(result, sort_keys=True, allow_nan=False))


if __name__ == "__main__":
    main()
