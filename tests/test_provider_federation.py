# Author: Lukas Bower
# Purpose: Refuse incorrectly correlated terminal results and credential-unsafe federation conformance endpoints.
# Copyright 2026 Lukas Bower
from __future__ import annotations

import json
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT / "scripts/ci"), str(ROOT / "tools/cohesix-py")]
from provider_federation import endpoint, terminal  # noqa: E402


def test_terminal_return_requires_unique_exact_native_correlation() -> None:
    request = dict(
        id="read-1",
        idempotency_key="once-1",
        action="systemd.status-check",
        writer_epoch=7,
        source_hive="hive-a",
        target_hive="hive-b",
    )
    result = dict(
        request,
        schema="host-ticket-result/v1",
        state="succeeded",
        relay_hop=1,
        relay_correlation_id="read-1:once-1:hive-a:hive-b",
    )
    raw = json.dumps(result).encode()
    assert terminal(b"", request) is None
    assert terminal(raw + b"\n\n", request) == result
    with pytest.raises(ValueError, match="duplicate"):
        terminal(raw + b"\n" + raw, request)
    for field, changed in [
        ("idempotency_key", "wrong"),
        ("action", "systemd.stop"),
        ("writer_epoch", 6),
        ("target_hive", "hive-c"),
        ("relay_hop", 2),
        ("relay_correlation_id", "wrong"),
    ]:
        with pytest.raises(ValueError, match="correlation"):
            terminal(json.dumps(dict(result, **{field: changed})).encode(), request)
    with pytest.raises(ValueError, match="schema"):
        terminal(b'{"schema":"cohesix-operation-report/v1"}', request)


def test_federation_credentials_require_tls_or_loopback() -> None:
    assert endpoint("https://hive.example:8443").hostname == "hive.example"
    assert endpoint("http://127.0.0.1:8081", relay=True).port == 8081
    for value in [
        "http://hive.example:8081",
        "https://user:secret@hive.example",
        "https://hive.example/?token=x",
        "https://hive.example/#fragment",
        "file:///tmp/gateway",
        "https://hive.example/path",
    ]:
        with pytest.raises(ValueError):
            endpoint(value)
    with pytest.raises(ValueError):
        endpoint("https://hive.example", relay=True)
