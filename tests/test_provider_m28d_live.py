# Author: Lukas Bower
# Purpose: Reject substituted M28d MCP terminal identities and result digests in the live evidence gate.
# Copyright 2026 Lukas Bower
"""Pure negative checks for the retained MCP reconciliation contract."""

from __future__ import annotations

import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts" / "ci"))
from provider_m28d_live import mcp_result  # noqa: E402


def test_mcp_recovery_rejects_replayed_or_substituted_terminal(tmp_path: Path) -> None:
    result = {
        "effect_replay_allowed": False,
        "record": {
            "binding": {"admission_id": "admit-1", "ticket_id": "ticket-1",
                        "idempotency_key": "once-1"},
            "execution": "confirmed", "delivery": "acknowledged",
            "result_sha256": "a" * 64,
        },
        "target_results": [{"id": "ticket-1", "idempotency_key": "once-1",
                            "admission": {"admission_id": "admit-1"},
                            "state": "succeeded"}],
        "target_result_sha256": ["a" * 64],
    }
    path = tmp_path / "recovery.json"

    def check(value: dict) -> dict:
        path.write_text(json.dumps({"response": {"isError": False,
                                                   "structuredContent": value}}))
        return mcp_result(path, "succeeded")

    assert check(result)["record"]["binding"]["admission_id"] == "admit-1"
    result["target_result_sha256"] = ["b" * 64]
    with pytest.raises(ValueError, match="original target result"):
        check(result)
    result["target_result_sha256"] = ["a" * 64]
    result["target_results"][0]["id"] = "ticket-2"
    with pytest.raises(ValueError, match="original target result"):
        check(result)
    result["target_results"][0]["id"] = "ticket-1"
    result["effect_replay_allowed"] = True
    with pytest.raises(ValueError, match="original target result"):
        check(result)
