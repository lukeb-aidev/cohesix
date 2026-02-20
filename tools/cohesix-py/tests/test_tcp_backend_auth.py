# Author: Lukas Bower
# Purpose: Validate TCP backend AUTH handshake error handling against real socket behavior.
# Copyright 2026 Lukas Bower

"""Tests for `cohesix.backends.TcpBackend` auth handshake behavior."""

from __future__ import annotations

import socket
import struct
import threading
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.backends import TcpBackend  # noqa: E402
from cohesix.errors import CohesixError  # noqa: E402


def _read_exact(conn: socket.socket, size: int) -> bytes:
    buf = bytearray()
    while len(buf) < size:
        chunk = conn.recv(size - len(buf))
        if not chunk:
            raise ConnectionError("connection closed while reading frame")
        buf.extend(chunk)
    return bytes(buf)


def _read_frame(conn: socket.socket) -> str:
    header = _read_exact(conn, 4)
    total_len = struct.unpack("<I", header)[0]
    if total_len < 4 or total_len > 8192:
        raise ConnectionError(f"invalid frame length {total_len}")
    payload = _read_exact(conn, total_len - 4)
    return payload.decode("utf-8")


def _send_frame(conn: socket.socket, line: str) -> None:
    payload = line.encode("utf-8")
    frame = struct.pack("<I", len(payload) + 4) + payload
    conn.sendall(frame)


def _start_server(handler):
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port = listener.getsockname()[1]

    def run() -> None:
        try:
            conn, _addr = listener.accept()
            with conn:
                handler(conn)
        finally:
            listener.close()

    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    return port, thread


def test_tcp_backend_auth_close_without_response_maps_to_rejection() -> None:
    def handler(conn: socket.socket) -> None:
        _ = _read_frame(conn)
        # Close immediately without sending any AUTH response frame.
        conn.shutdown(socket.SHUT_RDWR)

    port, thread = _start_server(handler)
    try:
        try:
            TcpBackend(
                host="127.0.0.1",
                port=port,
                auth_token="bootstrap",
                role="queen",
                ticket=None,
                timeout_s=0.2,
                max_retries=1,
            )
        except CohesixError as exc:
            assert "authentication rejected" in str(exc)
        else:  # pragma: no cover
            raise AssertionError("expected auth rejection when server closes without response")
    finally:
        thread.join(timeout=1.0)


def test_tcp_backend_auth_error_line_is_preserved() -> None:
    def handler(conn: socket.socket) -> None:
        _ = _read_frame(conn)
        _send_frame(conn, "ERR AUTH reason=invalid-token")
        conn.shutdown(socket.SHUT_RDWR)

    port, thread = _start_server(handler)
    try:
        try:
            TcpBackend(
                host="127.0.0.1",
                port=port,
                auth_token="bootstrap",
                role="queen",
                ticket=None,
                timeout_s=0.2,
                max_retries=1,
            )
        except CohesixError as exc:
            assert "ERR AUTH reason=invalid-token" in str(exc)
        else:  # pragma: no cover
            raise AssertionError("expected ERR AUTH to propagate")
    finally:
        thread.join(timeout=1.0)


def test_tcp_backend_auth_reset_without_response_maps_to_rejection() -> None:
    def handler(conn: socket.socket) -> None:
        _ = _read_frame(conn)
        # Force a TCP reset so recv() raises OSError/ConnectionResetError.
        linger = struct.pack("ii", 1, 0)
        conn.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, linger)
        conn.close()

    port, thread = _start_server(handler)
    try:
        try:
            TcpBackend(
                host="127.0.0.1",
                port=port,
                auth_token="bootstrap",
                role="queen",
                ticket=None,
                timeout_s=0.2,
                max_retries=1,
            )
        except CohesixError as exc:
            assert "authentication rejected" in str(exc)
        else:  # pragma: no cover
            raise AssertionError("expected auth rejection when server resets connection")
    finally:
        thread.join(timeout=1.0)
