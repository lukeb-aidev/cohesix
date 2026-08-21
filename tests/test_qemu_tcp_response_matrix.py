# Author: Lukas Bower
# Purpose: Verify the direct QEMU TCP response matrix against one framed mock connection.
# Copyright 2026 Lukas Bower

"""Focused host regression for scripts/qemu_tcp_response_matrix.py."""

from __future__ import annotations

import os
from pathlib import Path
import socket
import subprocess
import sys
import threading


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "qemu_tcp_response_matrix.py"
AUTH_TOKEN = "qemu-tcp-response-matrix-fixture"

HELP_BODY = [
    "Commands:",
    "  help  - Show this help",
    "  bi    - Show bootinfo summary",
    "  caps [mcs] - Show capability slots or bounded MCS authority state",
    "  smp [activity|mcs|dump] - Show activity, MCS topology, or raw debug state",
    "  mem   - Show untyped summary",
    "  ping  - Respond with pong",
    "  test  - Self-test (host-only; use cohsh)",
    "  nettest  - Run network self-test",
    "  netstats - Show network counters",
    "  quit  - Exit the console session",
]
NETSTATS_BODY = [
    "netstats: rx_pkts=1 tx_pkts=2 rx_used=3 tx_used=4 polls=5",
    *[f"netstats: fixture_index={index}" for index in range(2, 13)],
    "netstatus: fixture=ready",
    "nettest: fixture=none",
    "nettargets: fixture=qemu",
]
SMP_BODY = [
    "[smp] activity begin source=userspace benchmark=off hdmi=high-impact-only",
    *[f"[smp] activity fixture_index={index}" for index in range(2, 16)],
    "[smp] activity end",
]
CACHELOG_BODY = [
    f"[cache] seq={index} op=clean err=0 caller=fixture:{index}"
    for index in range(9, 0, -1)
]
def recv_exact(connection: socket.socket, size: int) -> bytes:
    """Read one complete fixture field."""

    data = bytearray()
    while len(data) < size:
        chunk = connection.recv(size - len(data))
        if not chunk:
            raise AssertionError("matrix client closed before sending a full frame")
        data.extend(chunk)
    return bytes(data)


def recv_frame(connection: socket.socket) -> str:
    """Read one little-endian length-prefixed fixture command."""

    total = int.from_bytes(recv_exact(connection, 4), "little")
    assert 4 <= total <= 8192
    return recv_exact(connection, total - 4).decode("utf-8")


def send_frame(connection: socket.socket, line: str) -> None:
    """Write one little-endian length-prefixed fixture response."""

    payload = line.encode("utf-8")
    connection.sendall((len(payload) + 4).to_bytes(4, "little") + payload)


def test_matrix_preserves_complete_body_first_responses_on_one_connection() -> None:
    """Every above-eight body completes before one ACK and the next command."""

    ready = threading.Event()
    port: list[int] = []
    commands: list[str] = []
    server_errors: list[BaseException] = []
    responses = [
        (f"AUTH {AUTH_TOKEN}", [], "OK AUTH"),
        ("ATTACH queen", [], "OK ATTACH role=queen"),
        ("help", HELP_BODY, "OK HELP"),
        ("netstats", NETSTATS_BODY, "OK NETSTATS"),
        ("smp activity", SMP_BODY, "OK SMP mode=activity"),
        ("cachelog 9", CACHELOG_BODY, "OK CACHELOG"),
        ("ping", ["PONG"], "OK PING reply=pong"),
        ("quit", [], "OK QUIT"),
    ]

    def serve_matrix() -> None:
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", 0))
                listener.listen(1)
                port.append(listener.getsockname()[1])
                ready.set()
                connection, _ = listener.accept()
                listener.close()
                with connection:
                    connection.settimeout(2)
                    for expected_command, body, ack in responses:
                        command = recv_frame(connection)
                        commands.append(command)
                        assert command == expected_command
                        for line in body:
                            send_frame(connection, line)
                        send_frame(connection, ack)
                        if expected_command == "quit":
                            assert connection.recv(1) == b""
        except BaseException as exc:  # pragma: no cover - asserted in caller
            server_errors.append(exc)
            ready.set()

    server = threading.Thread(target=serve_matrix, daemon=True)
    server.start()
    assert ready.wait(timeout=1), "matrix fixture did not start"
    assert port, f"matrix fixture failed before bind: {server_errors!r}"

    environment = os.environ.copy()
    environment["COHSH_AUTH_TOKEN"] = AUTH_TOKEN
    result = subprocess.run(
        [sys.executable, str(SCRIPT), "--host", "127.0.0.1", "--port", str(port[0])],
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )

    server.join(timeout=2)
    assert not server.is_alive(), "matrix fixture did not finish"
    assert not server_errors, f"matrix fixture failed: {server_errors!r}"
    assert result.returncode == 0, result.stderr
    assert result.stderr == ""
    assert commands == [entry[0] for entry in responses]
    assert result.stdout.splitlines() == [
        "PASS HELP body_frames=11 ack=OK_HELP",
        "PASS NETSTATS body_frames=15 ack=OK_NETSTATS",
        "PASS SMP body_frames=16 ack=OK_SMP_mode=activity",
        "PASS CACHELOG body_frames=9 ack=OK_CACHELOG",
        "PASS PING body_frames=1 ack=OK_PING_reply=pong",
        "PASS QUIT body_frames=0 ack=OK_QUIT close=EOF",
        "PASS TCP_MATRIX same_connection=yes handshakes=2 commands=6",
    ]
