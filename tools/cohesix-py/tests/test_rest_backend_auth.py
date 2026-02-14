# Author: Lukas Bower
# Purpose: Verify RestBackend request-auth header behavior for explicit and environment token flows.
# Copyright 2026 Lukas Bower

"""Auth header tests for `cohesix.backends.RestBackend`."""

from __future__ import annotations

import json
import os
import sys
import threading
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.backends import RestBackend  # noqa: E402


@dataclass
class AuthCapture:
    expected_token: str
    authorization_values: list[str] = field(default_factory=list)
    request_auth_values: list[str] = field(default_factory=list)


def _start_auth_server(expected_token: str) -> tuple[ThreadingHTTPServer, str, AuthCapture]:
    capture = AuthCapture(expected_token=expected_token)

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
            parsed = urlparse(self.path)
            if parsed.path == "/v1/fs/ls":
                query = parse_qs(parsed.query)
                path = (query.get("path") or ["/"])[0]
                if not self._validate_auth("LS", path):
                    return
                return self._send_ok("LS", path, lines=["log", "queen.log"], bytes_written=None)
            if parsed.path == "/v1/fs/cat":
                query = parse_qs(parsed.query)
                path = (query.get("path") or ["/log/queen.log"])[0]
                if not self._validate_auth("CAT", path):
                    return
                return self._send_ok(
                    "CAT",
                    path,
                    lines=["queen online"],
                    bytes_written=len("queen online".encode("utf-8")),
                )
            self.send_error(404)

        def do_POST(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
            parsed = urlparse(self.path)
            if parsed.path != "/v1/fs/echo":
                self.send_error(404)
                return
            length = int(self.headers.get("Content-Length", "0") or "0")
            payload = self.rfile.read(length) if length > 0 else b"{}"
            data = json.loads(payload.decode("utf-8"))
            path = str(data.get("path") or "")
            line = str(data.get("line") or "")
            if not self._validate_auth("ECHO", path):
                return
            self._send_ok("ECHO", path, lines=[], bytes_written=len(line.encode("utf-8")))

        def _validate_auth(self, verb: str, path: str) -> bool:
            auth = self.headers.get("Authorization", "")
            request_auth = self.headers.get("x-cohesix-auth", "")
            capture.authorization_values.append(auth)
            capture.request_auth_values.append(request_auth)
            expected_auth = f"Bearer {capture.expected_token}"
            if auth == expected_auth and request_auth == capture.expected_token:
                return True
            self._send_error(verb, path, "invalid request auth token", 401)
            return False

        def _send_ok(
            self,
            verb: str,
            path: str,
            lines: list[str],
            bytes_written: int | None,
        ) -> None:
            body: dict[str, object] = {
                "status": "OK",
                "verb": verb,
                "path": path,
                "end": True,
                "lines": lines,
            }
            if bytes_written is not None:
                body["bytes"] = bytes_written
            self._send_json(200, body)

        def _send_error(self, verb: str, path: str, error: str, code: int) -> None:
            body = {
                "status": "ERR",
                "verb": verb,
                "path": path,
                "end": True,
                "error": error,
                "lines": [],
            }
            self._send_json(code, body)

        def _send_json(self, code: int, body: dict[str, object]) -> None:
            payload = json.dumps(body).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *_args) -> None:  # type: ignore[override]
            return

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base_url = f"http://127.0.0.1:{server.server_address[1]}"
    return server, base_url, capture


def test_rest_backend_sends_explicit_request_auth_headers() -> None:
    server, base_url, capture = _start_auth_server(expected_token="explicit-token")
    try:
        backend = RestBackend(base_url, request_auth_token="explicit-token")
        entries = backend.list_dir("/")
        payload = backend.read_file("/log/queen.log", 4096)
        written = backend.write_append("/queen/ctl", b'{"op":"noop"}')
    finally:
        server.shutdown()
        server.server_close()

    assert entries == ["log", "queen.log"]
    assert payload == b"queen online"
    assert written == len('{"op":"noop"}'.encode("utf-8"))
    assert capture.authorization_values == ["Bearer explicit-token"] * 3
    assert capture.request_auth_values == ["explicit-token"] * 3


def test_rest_backend_uses_env_request_auth_header() -> None:
    original = os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN")
    os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"] = "env-token"
    server, base_url, capture = _start_auth_server(expected_token="env-token")
    try:
        backend = RestBackend(base_url)
        entries = backend.list_dir("/")
    finally:
        server.shutdown()
        server.server_close()
        if original is None:
            del os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"]
        else:
            os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"] = original

    assert entries == ["log", "queen.log"]
    assert capture.authorization_values == ["Bearer env-token"]
    assert capture.request_auth_values == ["env-token"]
