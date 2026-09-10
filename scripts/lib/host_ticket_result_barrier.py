#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Hold one authenticated terminal result while qualification retires its pinned Worker.
# Copyright 2026 Lukas Bower

"""A local test barrier in front of the existing sole Hive Gateway owner."""

from __future__ import annotations

from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import threading
from typing import Any, Callable
import urllib.error
import urllib.parse
import urllib.request


class TerminalResultBarrier:
    """Forward bounded REST requests; invoke one retirement before publication."""

    def __init__(
        self, upstream: str, token: str, ticket_id: str,
        retire: Callable[[dict[str, Any]], None],
    ) -> None:
        parsed = urllib.parse.urlsplit(upstream)
        if (
            parsed.scheme != "http" or parsed.hostname != "127.0.0.1"
            or parsed.port is None or parsed.path or parsed.query or parsed.fragment
            or parsed.username is not None or not token or not ticket_id
        ):
            raise ValueError("result barrier requires an authenticated loopback gateway")
        self.upstream = upstream
        self.token = token
        self.ticket_id = ticket_id
        self.retire = retire
        self.held = 0
        self.error: str | None = None
        self.server: HTTPServer | None = None
        self.thread: threading.Thread | None = None

    def __enter__(self) -> TerminalResultBarrier:
        barrier = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args: Any) -> None:
                """Keep request credentials and ticket bodies out of server logs."""

            def do_GET(self) -> None:
                self.forward()

            def do_POST(self) -> None:
                self.forward()

            def forward(self) -> None:
                try:
                    self.connection.settimeout(20)
                    if self.headers.get("Authorization") != f"Bearer {barrier.token}":
                        self.send_error(401)
                        return
                    if not self.path.startswith(("/v1/fs/", "/v1/meta/")):
                        self.send_error(404)
                        return
                    size = int(self.headers.get("Content-Length", "0"))
                    if not 0 <= size <= 65536 or self.headers.get("Transfer-Encoding"):
                        self.send_error(413)
                        return
                    body = self.rfile.read(size) if size else None
                    if body is not None and len(body) != size:
                        raise ValueError("incomplete REST request body")
                    if body and self.command == "POST" and self.path == "/v1/fs/echo":
                        payload = json.loads(body)
                        if payload.get("path") in {
                            "/host/tickets/status", "/host/tickets/deadletter",
                        }:
                            result = json.loads(payload.get("line", "{}"))
                            if (
                                result.get("schema") == "host-ticket-result/v2"
                                and result.get("id") == barrier.ticket_id
                                and result.get("state") in {"succeeded", "failed", "expired"}
                            ):
                                barrier.held += 1
                                if barrier.held != 1:
                                    raise ValueError("terminal result was published more than once")
                                barrier.retire(result)
                    request = urllib.request.Request(
                        barrier.upstream + self.path, data=body,
                        headers={"Authorization": f"Bearer {barrier.token}",
                                 "Content-Type": "application/json"},
                        method=self.command,
                    )
                    try:
                        response = urllib.request.urlopen(request, timeout=20)
                    except urllib.error.HTTPError as error:
                        response = error
                    with response:
                        data = response.read(2 * 1024 * 1024 + 1)
                        if len(data) > 2 * 1024 * 1024:
                            raise ValueError("gateway response exceeds the capture bound")
                        self.send_response(response.status)
                        self.send_header("Content-Type", "application/json")
                        self.send_header("Content-Length", str(len(data)))
                        self.end_headers()
                        self.wfile.write(data)
                except Exception as error:
                    barrier.error = str(error)
                    self.send_error(502, "result barrier failed")

        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        return self

    @property
    def url(self) -> str:
        if self.server is None:
            raise ValueError("result barrier has not started")
        return f"http://127.0.0.1:{self.server.server_port}"

    def verify(self) -> None:
        if self.error is not None or self.held != 1:
            raise ValueError(f"result barrier did not hold exactly one result: {self.error}")

    def __exit__(self, *_args: Any) -> None:
        if self.server is not None:
            self.server.shutdown()
            self.server.server_close()
        if self.thread is not None:
            self.thread.join(timeout=5)
            if self.thread.is_alive():
                raise ValueError("result barrier did not stop")
