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


@dataclass
class RetryCapture:
    requests: int = 0


def _start_auth_server(expected_token: str) -> tuple[ThreadingHTTPServer, str, AuthCapture]:
    capture = AuthCapture(expected_token=expected_token)

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
            parsed = urlparse(self.path)
            if parsed.path == "/v1/meta/bounds":
                if not self._validate_auth("BOUNDS", "/v1/meta/bounds"):
                    return
                return self._send_json(
                    200,
                    {
                        "manifest_sha256": "deadbeef",
                        "secure9p": {"msize": 8192, "walk_depth": 8},
                        "console": {
                            "max_line_len": 256,
                            "max_path_len": 96,
                            "max_json_len": 192,
                            "max_id_len": 32,
                            "max_echo_len": 128,
                            "max_ticket_len": 224,
                        },
                        "paths": {
                            "queen_ctl": "/queen/ctl",
                            "queen_lifecycle_ctl": "/queen/lifecycle/ctl",
                            "queen_schedule_ctl": "/queen/schedule/ctl",
                            "queen_lease_ctl": "/queen/lease/ctl",
                            "queen_export_ctl": "/queen/export/ctl",
                            "policy_ctl": "/policy/ctl",
                            "log": "/log/queen.log",
                        },
                        "control_plane": {
                            "schedule": {"enable": True, "queue_max_entries": 64, "ctl_max_bytes": 8192},
                            "lease": {
                                "enable": True,
                                "active_max_entries": 64,
                                "preemptions_max_entries": 64,
                                "ctl_max_bytes": 8192,
                            },
                            "export": {"enable": True, "ctl_max_bytes": 2048},
                        },
                        "policy": {
                            "enable": True,
                            "queue_max_entries": 32,
                            "queue_max_bytes": 4096,
                            "ctl_max_bytes": 2048,
                        },
                        "observability": {
                            "proc_schedule": {
                                "summary": True,
                                "queue": True,
                                "summary_bytes": 128,
                                "queue_bytes": 256,
                            },
                            "proc_lease": {
                                "summary": True,
                                "active": True,
                                "preemptions": True,
                                "summary_bytes": 160,
                                "active_bytes": 256,
                                "preemptions_bytes": 256,
                            },
                        },
                    },
                )
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
            if parsed.path == "/v1/fs/tail":
                query = parse_qs(parsed.query)
                path = (query.get("path") or ["/log/queen.log"])[0]
                if not self._validate_auth("TAIL", path):
                    return
                return self._send_ok(
                    "TAIL",
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


def _start_retry_server(
    failures_before_success: int,
) -> tuple[ThreadingHTTPServer, str, RetryCapture]:
    capture = RetryCapture()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
            parsed = urlparse(self.path)
            if parsed.path != "/v1/fs/ls":
                self.send_error(404)
                return
            capture.requests += 1
            if capture.requests <= failures_before_success:
                return self._send_json(
                    503,
                    {
                        "status": "ERR",
                        "verb": "LS",
                        "path": "/",
                        "end": True,
                        "error": "temporary unavailable",
                        "lines": [],
                    },
                )
            return self._send_json(
                200,
                {
                    "status": "OK",
                    "verb": "LS",
                    "path": "/",
                    "end": True,
                    "lines": ["gpu", "proc"],
                },
            )

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


def test_rest_backend_tail_and_bounds_with_auth_headers() -> None:
    server, base_url, capture = _start_auth_server(expected_token="explicit-token")
    try:
        backend = RestBackend(base_url, request_auth_token="explicit-token")
        tail = backend.tail_file("/log/queen.log", 4096)
        bounds = backend.get_bounds()
    finally:
        server.shutdown()
        server.server_close()

    assert tail == b"queen online"
    assert isinstance(bounds, dict)
    assert bounds.get("manifest_sha256") == "deadbeef"
    assert capture.authorization_values == ["Bearer explicit-token"] * 2
    assert capture.request_auth_values == ["explicit-token"] * 2


def test_rest_backend_retries_transient_http_failures() -> None:
    server, base_url, capture = _start_retry_server(failures_before_success=1)
    try:
        backend = RestBackend(
            base_url,
            max_attempts=3,
            backoff_ms=1,
            backoff_ceiling_ms=2,
        )
        entries = backend.list_dir("/")
    finally:
        server.shutdown()
        server.server_close()

    assert entries == ["gpu", "proc"]
    assert capture.requests == 2
