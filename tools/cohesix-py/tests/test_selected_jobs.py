# Author: Lukas Bower
# Purpose: Verify Python selected-job calls preserve exact ids and use the shared REST custody point for approved starts.
# Copyright 2026 Lukas Bower

"""Selected job control surface tests."""

from __future__ import annotations

import json

import pytest

from cohesix.backends import RestBackend
from cohesix.errors import CohesixError


def test_selected_job_paths_keep_one_exact_admission_identity(monkeypatch) -> None:
    backend = RestBackend("http://127.0.0.1:8080")
    calls: list[tuple[str, str, dict | None]] = []

    def respond(method: str, path: str, query=None, body=None) -> bytes:
        calls.append((method, path, body))
        return json.dumps({"record": {"binding": {"admission_id": "admit-1"}}}).encode()

    monkeypatch.setattr(backend, "_request_payload", respond)
    binding = {"admission_id": "admit-1"}
    ticket = {"id": "ticket-1"}
    assert backend.submit_selected_job(binding, ticket)["record"]["binding"]["admission_id"] == "admit-1"
    backend.start_approved_job("scope-1", "request-1")
    backend.selected_job_status("admit-1")
    backend.request_selected_job_cancel("admit-1")
    backend.reconcile_selected_job("admit-1")
    backend.inspect_standing_scope("scope-1")
    backend.revoke_standing_scope("scope-1")
    assert calls == [
        ("POST", "/v1/jobs", {"binding": binding, "ticket": ticket}),
        ("POST", "/v1/jobs/approved/scope-1/start", {"request_id": "request-1"}),
        ("GET", "/v1/jobs/admit-1", None),
        ("POST", "/v1/jobs/admit-1/cancel", {}),
        ("POST", "/v1/jobs/admit-1/reconcile", {}),
        ("POST", "/v1/standing/scopes/scope-1/inspect", {}),
        ("POST", "/v1/standing/scopes/scope-1/revoke", {}),
    ]


@pytest.mark.parametrize("identifier", ["../job", "-job", "", "a" * 129])
def test_invalid_job_identifiers_refuse_before_transport(monkeypatch, identifier: str) -> None:
    backend = RestBackend("http://127.0.0.1:8080")
    monkeypatch.setattr(
        backend, "_request_payload", lambda *args, **kwargs: pytest.fail("transport called")
    )
    with pytest.raises(CohesixError):
        backend.selected_job_status(identifier)
    with pytest.raises(CohesixError):
        backend.revoke_standing_scope(identifier)
    with pytest.raises(CohesixError):
        backend.start_approved_job("scope-1", identifier)


def test_approved_request_id_bound_precedes_transport(monkeypatch) -> None:
    backend = RestBackend("http://127.0.0.1:8080")
    monkeypatch.setattr(
        backend, "_request_payload", lambda *args, **kwargs: pytest.fail("transport called")
    )
    with pytest.raises(CohesixError, match="ELIMIT approved request id"):
        backend.start_approved_job("scope-1", "a" * 97)
