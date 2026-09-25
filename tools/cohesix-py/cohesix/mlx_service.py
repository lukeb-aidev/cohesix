# Author: Lukas Bower
# Purpose: Serve one content-bound local MLX generation through a bounded loopback inference API.
# Copyright 2026 Lukas Bower
"""Single-generation local MLX inference transport, without job authority.

The selected model and adapter are fixed when this process starts. A caller
must bind this process to an admitted Cohesix deployment before using its
response as a canary or outcome. The server has no tool endpoint or credential.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import re
from typing import Any, Callable

from .mlx_native import (
    MAX_ADAPTER_BYTES,
    MlxInference,
    MlxRefusal,
    MlxSelection,
    _adapter,
    _regular,
    infer,
    observed_metal,
    require,
    tree_digest,
)


MAX_REQUEST_BYTES = 4096
MAX_RESPONSE_BYTES = 16_384
MAX_GENERATION = 1_000_000_000
LOCAL_HOSTS = {"127.0.0.1", "localhost"}


class ServingRefusal(ValueError):
    """A request cannot be bound to this local serving generation."""


@dataclass(frozen=True)
class ServingSelection:
    """Content and generation selected before the loopback socket is opened."""

    mlx: MlxSelection
    generation: int
    adapter_directory: Path | None = None
    adapter_sha256: str | None = None

    @property
    def model_id(self) -> str:
        digest = self.adapter_sha256 or self.mlx.model_sha256
        return f"cohesix-g{self.generation}-{digest}"

    def validate(self) -> None:
        require(type(self.generation) is int and 0 <= self.generation <= MAX_GENERATION,
                "mlx_serving_generation")
        self.mlx.validate()
        require((self.adapter_directory is None) == (self.adapter_sha256 is None),
                "mlx_serving_adapter_selection")
        if self.adapter_directory is not None:
            _adapter(self.mlx, self.adapter_directory, self.adapter_sha256)


def _unique_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ServingRefusal("duplicate_json_key")
        result[key] = value
    return result


def parse_request(raw: bytes, selection: ServingSelection) -> tuple[str, int]:
    """Accept only one bounded user prompt for one fixed local model."""
    if not 1 <= len(raw) <= MAX_REQUEST_BYTES:
        raise ServingRefusal("request_size")
    try:
        payload = json.loads(raw, object_pairs_hook=_unique_pairs)
    except (UnicodeError, ValueError) as error:
        raise ServingRefusal("request_json") from error
    if not isinstance(payload, dict) or set(payload) != {
        "model", "messages", "max_tokens", "temperature", "stream"
    }:
        raise ServingRefusal("request_fields")
    if payload["model"] != selection.model_id:
        raise ServingRefusal("served_generation_mismatch")
    if payload["temperature"] != 0 or type(payload["temperature"]) is bool:
        raise ServingRefusal("deterministic_temperature_required")
    if payload["stream"] is not False:
        raise ServingRefusal("streaming_not_supported")
    tokens = payload["max_tokens"]
    if type(tokens) is not int or not 1 <= tokens <= 64:
        raise ServingRefusal("max_tokens")
    messages = payload["messages"]
    if not isinstance(messages, list) or len(messages) != 1 or not isinstance(messages[0], dict):
        raise ServingRefusal("one_user_message_required")
    message = messages[0]
    if set(message) != {"role", "content"} or message["role"] != "user":
        raise ServingRefusal("one_user_message_required")
    prompt = message["content"]
    if not isinstance(prompt, str) or not 1 <= len(prompt.encode("utf-8")) <= 2048:
        raise ServingRefusal("prompt_size")
    return prompt, tokens


class LocalMlxServer(HTTPServer):
    """Serialized requests prevent simultaneous MLX loads exceeding the cap."""

    allow_reuse_address = False

    def __init__(self, port: int, selection: ServingSelection,
                 generate: Callable[..., MlxInference] = infer,
                 observe: Callable[[MlxSelection], dict[str, Any]] = observed_metal):
        if type(port) is not int or not (port == 0 or 1024 <= port <= 65535):
            raise ServingRefusal("loopback_port")
        selection.validate()
        device = observe(selection.mlx)
        if not isinstance(device, dict) or not isinstance(device.get("device_name"), str):
            raise ServingRefusal("metal_device_observation")
        self.selection = selection
        self.generate = generate
        self.device_name = device["device_name"]
        super().__init__(("127.0.0.1", port), LocalMlxHandler)


class LocalMlxHandler(BaseHTTPRequestHandler):
    """Return bounded model metadata and inference, with no prompt logging."""

    server: LocalMlxServer
    protocol_version = "HTTP/1.1"

    def log_message(self, _format: str, *_args: Any) -> None:
        return

    def _send(self, status: int, payload: dict[str, Any]) -> None:
        encoded = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()
        if len(encoded) > MAX_RESPONSE_BYTES:
            status = 500
            encoded = b'{"error":"response_size"}'
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(encoded)
        self.close_connection = True

    def _origin_ok(self) -> bool:
        values = self.headers.get_all("Host", [])
        if len(values) != 1:
            return False
        host = values[0]
        match = re.fullmatch(r"(127\.0\.0\.1|localhost):([0-9]{4,5})", host)
        return bool(match and int(match.group(2)) == self.server.server_port
                    and self.client_address[0] == "127.0.0.1")

    def do_GET(self) -> None:
        if not self._origin_ok():
            self._send(403, {"error": "loopback_origin_required"})
        elif self.path == "/health":
            self._send(200, {"status": "ready", "model": self.server.selection.model_id,
                             "generation": self.server.selection.generation,
                             "device_name": self.server.device_name})
        elif self.path == "/v1/models":
            self._send(200, {"object": "list", "data": [{
                "id": self.server.selection.model_id, "object": "model"}]})
        else:
            self._send(404, {"error": "unknown_endpoint"})

    def do_POST(self) -> None:
        if not self._origin_ok():
            self._send(403, {"error": "loopback_origin_required"})
            return
        if self.path != "/v1/chat/completions":
            self._send(404, {"error": "unknown_endpoint"})
            return
        content_types = self.headers.get_all("Content-Type", [])
        content_lengths = self.headers.get_all("Content-Length", [])
        if (self.headers.get("Transfer-Encoding") or content_types != ["application/json"]
                or len(content_lengths) != 1 or not re.fullmatch(r"[0-9]{1,4}", content_lengths[0])):
            self._send(400, {"error": "request_encoding"})
            return
        length = int(content_lengths[0])
        if not 1 <= length <= MAX_REQUEST_BYTES:
            self._send(400, {"error": "request_size"})
            return
        self.connection.settimeout(10)
        try:
            prompt, tokens = parse_request(self.rfile.read(length), self.server.selection)
            result = self.server.generate(
                self.server.selection.mlx, prompt, tokens,
                self.server.selection.adapter_directory,
                self.server.selection.adapter_sha256,
            )
            if (result.model_sha256 != self.server.selection.mlx.model_sha256
                    or result.adapter_sha256 != self.server.selection.adapter_sha256):
                raise ServingRefusal("generated_artifact_mismatch")
            self._send(200, {
                "object": "chat.completion", "model": self.server.selection.model_id,
                "choices": [{"index": 0, "message": {
                    "role": "assistant", "content": result.text}, "finish_reason": "stop"}],
                "cohesix_observation": result.evidence(),
            })
        except (MlxRefusal, ServingRefusal, OSError, TimeoutError):
            self._send(422, {"error": "local_inference_refused"})


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selection", type=Path, required=True)
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    config = json.loads(_regular(args.selection, 8192), object_pairs_hook=_unique_pairs)
    if not isinstance(config, dict) or set(config) != {
        "generation", "model_directory", "model_sha256", "data_directory",
        "data_sha256", "memory_limit_bytes", "adapter_directory", "adapter_sha256",
    }:
        raise ServingRefusal("selection_fields")
    selection = ServingSelection(
        MlxSelection(
            Path(config["model_directory"]), config["model_sha256"],
            Path(config["data_directory"]), config["data_sha256"],
            config["memory_limit_bytes"],
        ),
        config["generation"],
        Path(config["adapter_directory"]) if config["adapter_directory"] else None,
        config["adapter_sha256"],
    )
    with LocalMlxServer(args.port, selection) as server:
        print(json.dumps({"model": selection.model_id, "port": server.server_port,
                          "pid": os.getpid()}), flush=True)
        try:
            server.serve_forever(poll_interval=0.25)
        except KeyboardInterrupt:
            return


if __name__ == "__main__":
    main()
