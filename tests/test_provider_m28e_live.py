# Author: Lukas Bower
# Purpose: Reject forged A2A task identity and native result substitution in the selected live gate.
# Copyright 2026 Lukas Bower
"""Focused negative checks for A2A task and shared-ledger reconciliation."""

from __future__ import annotations

import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts" / "ci"))
from provider_m28e_live import recovery, task  # noqa: E402


def test_task_requires_original_identity_and_non_authoritative_verification(tmp_path: Path) -> None:
    path = tmp_path / "task.json"
    value = {"kind": "task", "id": "job-1", "contextId": "job-1",
             "status": {"state": "completed"}, "metadata": {
                 "admissionId": "job-1", "effectReplayAllowed": False,
                 "providerVerified": False}}

    def check() -> dict:
        path.write_text(json.dumps({"sdk": "a2a-sdk==0.3.26", "task": value}))
        return task(path, "job-1")

    assert check()["id"] == "job-1"
    value["metadata"]["admissionId"] = "job-2"
    with pytest.raises(ValueError, match="task identity"):
        check()
    value["metadata"]["admissionId"] = "job-1"
    value["metadata"]["providerVerified"] = True
    with pytest.raises(ValueError, match="task identity"):
        check()


def test_recovery_rejects_substituted_native_terminal(tmp_path: Path) -> None:
    path = tmp_path / "recover.json"
    value = {"effect_replay_allowed": False,
             "record": {"binding": {"admission_id": "job-1", "ticket_id": "ticket-1",
                                    "idempotency_key": "once-1"},
                        "execution": "confirmed", "delivery": "acknowledged",
                        "result_sha256": "a" * 64},
             "target_results": [{"id": "ticket-1", "idempotency_key": "once-1",
                                 "admission": {"admission_id": "job-1"},
                                 "state": "succeeded"}],
             "target_result_sha256": ["a" * 64]}

    def check() -> dict:
        path.write_text(json.dumps(value))
        return recovery(path, "job-1")

    assert check()["record"]["result_sha256"] == "a" * 64
    value["target_result_sha256"] = ["b" * 64]
    with pytest.raises(ValueError, match="native terminal mismatch"):
        check()
    value["target_result_sha256"] = ["a" * 64]
    value["target_results"][0]["state"] = "recovered_failure"
    with pytest.raises(ValueError, match="native terminal mismatch"):
        check()
