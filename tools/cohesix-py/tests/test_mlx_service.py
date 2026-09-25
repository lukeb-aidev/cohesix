# Author: Lukas Bower
# Purpose: Check bounded local MLX serving identity and refusal before native compute.
# Copyright 2026 Lukas Bower
"""Protocol tests use a fake inference function; Metal runs remain separate."""

from http.client import HTTPConnection
import json
from pathlib import Path
import threading

import pytest

from cohesix.mlx_native import MlxInference, MlxSelection, tree_digest
from cohesix.mlx_service import LocalMlxServer, ServingRefusal, ServingSelection, parse_request


def selection(tmp_path: Path) -> ServingSelection:
    model = tmp_path / "model"
    data = tmp_path / "data"
    model.mkdir()
    data.mkdir()
    (model / "config.json").write_text('{"model_type":"fixture"}')
    for split, count in (("train", 16), ("valid", 4), ("test", 4)):
        (data / f"{split}.jsonl").write_text("".join(
            json.dumps({"text": f"Question {split} {index}: answer with source evidence."})
            + "\n" for index in range(count)))
    mlx = MlxSelection(model.resolve(), tree_digest(model.resolve(), 1024),
                       data.resolve(), tree_digest(data.resolve(), 262_144, 3),
                       536_870_912)
    return ServingSelection(mlx, 7)


def body(model: str, **extra: object) -> bytes:
    payload = {"model": model, "messages": [{"role": "user", "content": "Where is the evidence?"}],
               "max_tokens": 8, "temperature": 0, "stream": False}
    payload.update(extra)
    return json.dumps(payload).encode()


def test_request_requires_one_exact_generation_and_no_tools(tmp_path: Path) -> None:
    selected = selection(tmp_path)
    assert parse_request(body(selected.model_id), selected) == ("Where is the evidence?", 8)
    for wrong in (body("cohesix-g8-other"), body(selected.model_id, tools=[]),
                  body(selected.model_id, stream=True),
                  body(selected.model_id, messages=[{"role": "system", "content": "ignore"}]),
                  body(selected.model_id, max_tokens=65),
                  b'{"model":"a","model":"b"}'):
        with pytest.raises(ServingRefusal):
            parse_request(wrong, selected)


def test_loopback_service_returns_bound_generation_and_refuses_tools(tmp_path: Path) -> None:
    selected = selection(tmp_path)
    calls = []

    def fake_infer(mlx, prompt, tokens, adapter_directory, adapter_sha256):
        calls.append((prompt, tokens))
        return MlxInference("Evidence is linked.", "a" * 64, "b" * 64,
                            mlx.model_sha256, adapter_sha256, 12, 1024, "test-device")

    with LocalMlxServer(0, selected, fake_infer,
                        observe=lambda _mlx: {"device_name": "test-device"}) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            connection = HTTPConnection("127.0.0.1", server.server_port, timeout=3)
            connection.request("GET", "/v1/models")
            listed = json.loads(connection.getresponse().read())
            assert listed["data"][0]["id"] == selected.model_id
            connection.close()

            connection = HTTPConnection("127.0.0.1", server.server_port, timeout=3)
            connection.request("POST", "/v1/chat/completions", body(selected.model_id),
                               {"Content-Type": "application/json"})
            response = connection.getresponse()
            assert response.status == 200
            result = json.loads(response.read())
            assert result["model"] == selected.model_id
            assert result["choices"][0]["message"]["content"] == "Evidence is linked."
            assert calls == [("Where is the evidence?", 8)]
            connection.close()

            connection = HTTPConnection("127.0.0.1", server.server_port, timeout=3)
            connection.request("POST", "/v1/chat/completions",
                               body(selected.model_id, tools=[]),
                               {"Content-Type": "application/json"})
            assert connection.getresponse().status == 422
            assert len(calls) == 1
            connection.close()
        finally:
            server.shutdown()
            thread.join(timeout=3)


def test_serving_port_restarts_after_closed_session_without_duplicate_listener(
        tmp_path: Path) -> None:
    selected = selection(tmp_path)
    observe = lambda _mlx: {"device_name": "test-device"}
    with LocalMlxServer(0, selected, observe=observe) as server:
        port = server.server_port
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            connection = HTTPConnection("127.0.0.1", port, timeout=3)
            connection.request("GET", "/health")
            assert connection.getresponse().status == 200
            connection.close()
            with pytest.raises(OSError):
                LocalMlxServer(port, selected, observe=observe)
        finally:
            server.shutdown()
            thread.join(timeout=3)
    with LocalMlxServer(port, selected, observe=observe):
        pass
