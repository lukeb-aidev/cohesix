# Author: Lukas Bower
# Purpose: Refuse nonlocal vMLX endpoints, ambiguous models, tools and unbounded responses before showing local inference.
# Copyright 2026 Lukas Bower
"""Focused vMLX wire checks; fixtures cannot establish a GPU or Cohesix outcome."""

from __future__ import annotations

import json
from pathlib import Path
import sys
from typing import Any
import urllib.error

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.vmlx_compat import VmlxClient, VmlxRefusal  # noqa: E402


def selected_reply() -> dict[str, Any]:
    return {
        "model": "selected-model",
        "choices": [{
            "finish_reason": "stop",
            "message": {"role": "assistant", "content": "local answer"},
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 3},
    }


def test_requires_explicit_loopback_model_and_bounded_prompt() -> None:
    for endpoint in (
        "https://example.com:8000", "http://0.0.0.0:8000",
        "http://127.0.0.1:8000/prefix", "http://user@localhost:8000",
        "http://localhost:bad", "http://localhost:8000/?other=1",
    ):
        with pytest.raises(VmlxRefusal, match="loopback"):
            VmlxClient(endpoint, "selected-model")
    with pytest.raises(VmlxRefusal, match="model"):
        VmlxClient("http://127.0.0.1:8000", "../other")
    with pytest.raises(VmlxRefusal, match="model"):
        VmlxClient("http://127.0.0.1:8000", None)  # type: ignore[arg-type]
    client = VmlxClient("http://127.0.0.1:8000", "selected-model")
    with pytest.raises(VmlxRefusal, match="prompt"):
        client.generate("x" * 2049)
    with pytest.raises(VmlxRefusal, match="token"):
        client.generate("hello", max_tokens=65)


def test_selected_model_response_and_private_evidence(monkeypatch: pytest.MonkeyPatch) -> None:
    client = VmlxClient("http://127.0.0.1:8000", "selected-model")
    calls: list[tuple[str, dict[str, Any] | None]] = []

    def response(path: str, body: dict[str, Any] | None = None) -> dict[str, Any]:
        calls.append((path, body))
        if path == "/health":
            return {"status": "healthy", "model_loaded": True,
                    "served_model_name": "selected-model"}
        if path == "/v1/models":
            return {"data": [{"id": "other-model"}, {"id": "selected-model"}]}
        return selected_reply()

    monkeypatch.setattr(client, "_request", response)
    reply = client.generate("private prompt")
    assert reply.text == "local answer"
    assert reply.evidence()["model"] == "selected-model"
    assert "private prompt" not in json.dumps(reply.evidence())
    assert "local answer" not in json.dumps(reply.evidence())
    assert calls[2][1] == {
        "model": "selected-model", "messages": [
            {"role": "user", "content": "private prompt"}
        ], "max_tokens": 32, "temperature": 0, "stream": False,
    }


def test_refuses_changed_model_tools_and_unbounded_usage(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = VmlxClient("http://127.0.0.1:8000", "selected-model")
    monkeypatch.setattr(client, "ready", lambda: None)
    value = selected_reply()
    monkeypatch.setattr(client, "_request", lambda *_args, **_kwargs: value)
    value["model"] = "other-model"
    with pytest.raises(VmlxRefusal, match="mismatch"):
        client.generate("hello")
    value["model"] = "selected-model"
    value["choices"][0]["message"]["tool_calls"] = [{"name": "shell"}]
    with pytest.raises(VmlxRefusal, match="tool"):
        client.generate("hello")
    del value["choices"][0]["message"]["tool_calls"]
    value["usage"]["completion_tokens"] = 1000
    with pytest.raises(VmlxRefusal, match="usage"):
        client.generate("hello")


def test_refuses_ambiguous_model_listing(monkeypatch: pytest.MonkeyPatch) -> None:
    client = VmlxClient("http://127.0.0.1:8000", "selected-model")
    monkeypatch.setattr(client, "_request", lambda path, _body=None:
                        {"status": "healthy", "model_loaded": True,
                         "served_model_name": "selected-model"}
                        if path == "/health" else
                        {"data": [{"id": "selected-model"}, {"id": "selected-model"}]})
    with pytest.raises(VmlxRefusal, match="ambiguous"):
        client.ready()


def test_refuses_nonfinite_json_and_redirect(monkeypatch: pytest.MonkeyPatch) -> None:
    class Response:
        status = 200
        headers = {"Content-Length": "15"}

        def __enter__(self) -> "Response":
            return self

        def __exit__(self, *_args: object) -> None:
            return None

        def read(self, _maximum: int) -> bytes:
            return b'{"value": NaN}'

    client = VmlxClient("http://127.0.0.1:8000", "selected-model")
    monkeypatch.setattr(client.opener, "open", lambda *_args, **_kwargs: Response())
    with pytest.raises(VmlxRefusal, match="json"):
        client._request("/health")

    def redirected(*_args: object, **_kwargs: object) -> None:
        raise urllib.error.HTTPError("http://127.0.0.1:8000/health", 302,
                                     "redirect", {}, None)

    monkeypatch.setattr(client.opener, "open", redirected)
    with pytest.raises(VmlxRefusal, match="transport"):
        client._request("/health")
