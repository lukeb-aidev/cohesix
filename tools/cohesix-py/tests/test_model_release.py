# Author: Lukas Bower
# Purpose: Check Python PEFT release identity, recovery, and verified-outcome boundaries.
# Copyright 2026 Lukas Bower
"""Focused contract tests for the non-authoritative PEFT release client."""

from __future__ import annotations

import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix import PeftReleaseClient  # noqa: E402
from cohesix import model_release  # noqa: E402


REQUEST_SHA = "a" * 64
GRAPH_SHA = "b" * 64


def deployment(tmp_path: Path) -> Path:
    path = tmp_path / "deployment.json"
    path.write_text(json.dumps({"request": {"operation_id": "release-1", "model_id": "private"}}))
    return path


def report(*, state: str | None = None, submitted: bool = False,
           acknowledged: bool = False) -> dict[str, object]:
    native = {
        "schema": "cohesix-peft-recipe-journal/v1",
        "operation_id": "release-1",
        "request_sha256": REQUEST_SHA,
        "state": state,
        "private": "not surfaced",
    }
    if state in {"succeeded", "recovered_failure"}:
        native["comparison"] = {
            "policy_sha256": "d" * 64, "candidate_sha256": "c" * 64,
            "baseline": {"generation": 0},
        }
        native["phases"] = [{
            "phase": "evaluate", "result": {"detail": {
                "candidate": {"metrics": {"eval_loss": 4.9}, "samples": 16},
                "baseline": {"metrics": {"eval_loss": 5.0}, "samples": 16},
            }},
        }, {
            "phase": "promote" if state == "succeeded" else "rollback",
            "result": {"succeeded": True, "detail": {"accepted": {
                "generation": 1 if state == "succeeded" else 0,
            }}},
        }]
    return {
        "schema": "cohesix-peft-report/v1",
        "authoritative": False,
        "production_use_case_accepted": False,
        "operation_id": "release-1",
        "request_sha256": REQUEST_SHA,
        "submitted": submitted,
        "acknowledged": acknowledged,
        "ambiguous": submitted and state is None,
        "result": None if state is None else {
            "state": state, "graph_sha256": GRAPH_SHA,
            "native": native,
        },
    }


def client(tmp_path: Path) -> PeftReleaseClient:
    return PeftReleaseClient(coh_binary=tmp_path / "coh", deployment=deployment(tmp_path))


def test_explicit_submit_and_verify_use_one_identity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    release = client(tmp_path)
    calls: list[tuple[str, dict[str, object]]] = []
    reports = {
        "plan": report(),
        "apply": report(submitted=True, acknowledged=True),
        "watch": report(state="succeeded", submitted=True, acknowledged=True),
        "verify": report(state="succeeded", submitted=True, acknowledged=True),
    }

    def call(lifecycle: str, **kwargs: object) -> dict[str, object]:
        calls.append((lifecycle, kwargs))
        return reports[lifecycle]

    monkeypatch.setattr(model_release, "run_peft_release", call)
    assert release.plan().state == "not_submitted"
    applied = release.apply(rest_url="https://queen.invalid", auth_ref="file:/token",
                            ticket_ref="file:/ticket")
    assert applied.state == "outcome_unknown"
    assert applied.ambiguous and not applied.requested_outcome_verified
    verified = release.inspect()
    assert verified.state == "succeeded"
    assert verified.requested_outcome_verified
    assert verified.graph_sha256 == GRAPH_SHA
    assert verified.heldout_candidate_loss == 4.9
    assert verified.heldout_baseline_loss == 5.0
    assert verified.heldout_samples == 16
    assert verified.baseline_generation == 0
    assert verified.observed_served_generation == 1
    assert [mode for mode, _ in calls] == ["plan", "apply", "watch", "verify"]
    assert calls[1][1]["ticket_ref"] == "file:/ticket"


def test_external_mcp_admission_projects_shared_verified_result(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    release = client(tmp_path)
    monkeypatch.setattr(
        model_release, "run_peft_release",
        lambda *_a, **_k: report(state="succeeded", submitted=False),
    )
    verified = release.verify()
    assert verified.state == "succeeded"
    assert verified.requested_outcome_verified
    assert verified.graph_sha256 == GRAPH_SHA
    assert not verified.submitted
    assert not verified.acknowledged


@pytest.mark.parametrize("state", ["failed", "recovered_failure", "rollback_failed"])
def test_failed_outcome_never_becomes_verified(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, state: str,
) -> None:
    release = client(tmp_path)
    calls: list[str] = []

    def call(lifecycle: str, **_: object) -> dict[str, object]:
        calls.append(lifecycle)
        return report(state=state, submitted=True, acknowledged=True)

    monkeypatch.setattr(model_release, "run_peft_release", call)
    observed = release.inspect()
    assert observed.state == state
    assert not observed.requested_outcome_verified
    if state == "recovered_failure":
        assert observed.rollback_observed
        assert observed.observed_restored_generation == 0
    assert calls == ["watch"]


def test_lost_apply_response_requires_read_only_recovery(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    release = client(tmp_path)
    calls: list[str] = []

    def call(lifecycle: str, **_: object) -> dict[str, object]:
        calls.append(lifecycle)
        if lifecycle == "apply":
            raise TimeoutError("lost response")
        return report(submitted=True, acknowledged=False)

    monkeypatch.setattr(model_release, "run_peft_release", call)
    with pytest.raises(TimeoutError):
        release.apply(rest_url="https://queen.invalid", auth_ref="file:/token",
                      ticket_ref="file:/ticket")
    recovered = release.recover()
    assert recovered.state == "outcome_unknown"
    assert recovered.ambiguous
    assert calls == ["apply", "recover"]


@pytest.mark.parametrize("change", [
    {"operation_id": "release-2"},
    {"request_sha256": "invalid"},
    {"authoritative": True},
    {"submitted": False, "acknowledged": True},
    {"submitted": False, "result": {"state": "succeeded", "graph_sha256": GRAPH_SHA}},
    {"submitted": True, "ambiguous": False, "result": None},
    {"submitted": True, "result": {"state": "succeeded", "graph_sha256": "invalid"}},
    {"submitted": True, "result": {"state": "succeeded", "graph_sha256": GRAPH_SHA}},
    {"submitted": True, "result": {
        "state": "succeeded", "graph_sha256": GRAPH_SHA,
        "native": {"schema": "cohesix-peft-recipe-journal/v1", "operation_id": "other"},
    }},
])
def test_substituted_or_contradictory_report_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, change: dict[str, object],
) -> None:
    release = client(tmp_path)
    supplied = report()
    supplied.update(change)
    monkeypatch.setattr(model_release, "run_peft_release", lambda *_a, **_k: supplied)
    with pytest.raises(ValueError):
        release.watch()


def test_deployment_request_drift_refused_before_cli(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    release = client(tmp_path)
    release.deployment.write_text(json.dumps({"request": {
        "operation_id": "release-1", "model_id": "changed",
    }}))
    monkeypatch.setattr(model_release, "run_peft_release",
                        lambda *_a, **_k: pytest.fail("CLI must not run"))
    with pytest.raises(ValueError, match="request changed"):
        release.watch()


def test_deployment_request_drift_during_cli_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    release = client(tmp_path)

    def call(*_args: object, **_kwargs: object) -> dict[str, object]:
        release.deployment.write_text(json.dumps({"request": {
            "operation_id": "release-1", "model_id": "changed",
        }}))
        return report()

    monkeypatch.setattr(model_release, "run_peft_release", call)
    with pytest.raises(ValueError, match="changed during CLI operation"):
        release.watch()


def test_pending_report_cannot_pass_verify(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    release = client(tmp_path)
    monkeypatch.setattr(model_release, "run_peft_release",
                        lambda *_a, **_k: report(submitted=True, acknowledged=True))
    with pytest.raises(ValueError, match="pending state"):
        release.verify()


def test_verify_response_must_match_watched_request(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    release = client(tmp_path)

    def call(lifecycle: str, **_: object) -> dict[str, object]:
        supplied = report(state="succeeded", submitted=True, acknowledged=True)
        if lifecycle == "verify":
            supplied["request_sha256"] = "c" * 64
            result = supplied["result"]
            assert isinstance(result, dict)
            native = result["native"]
            assert isinstance(native, dict)
            native["request_sha256"] = "c" * 64
        return supplied

    monkeypatch.setattr(model_release, "run_peft_release", call)
    with pytest.raises(ValueError, match="request identity changed"):
        release.inspect()


@pytest.mark.parametrize("damage", ["missing_promotion", "nonfinite_loss", "sample_mismatch"])
def test_signed_projection_refuses_missing_serving_or_invalid_comparison(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str,
) -> None:
    release = client(tmp_path)
    supplied = report(state="succeeded", submitted=True, acknowledged=True)
    result = supplied["result"]
    assert isinstance(result, dict)
    native = result["native"]
    assert isinstance(native, dict)
    phases = native["phases"]
    assert isinstance(phases, list)
    if damage == "missing_promotion":
        phases.pop()
    else:
        evaluation = phases[0]["result"]["detail"]
        if damage == "nonfinite_loss":
            evaluation["candidate"]["metrics"]["eval_loss"] = float("nan")
        else:
            evaluation["baseline"]["samples"] = 15
    monkeypatch.setattr(model_release, "run_peft_release", lambda *_a, **_k: supplied)
    with pytest.raises(ValueError):
        release.inspect()
