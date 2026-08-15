#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check lossless synchronous response framing on one QEMU TCP console connection.
# Copyright 2026 Lukas Bower

"""Run the bounded QEMU TCP response matrix without a reconnecting client."""

from __future__ import annotations

import argparse
import os
import re
import socket
import sys
import time
from collections.abc import Sequence


FRAME_HEADER_BYTES = 4
MAX_FRAME_BYTES = 8192
MAX_COMMAND_BYTES = 256
MAX_MATRIX_RESPONSE_FRAMES = 64
RESPONSE_TIMEOUT_SECONDS = 30.0
QUIT_CLOSE_TIMEOUT_SECONDS = 30.0
CACHELOG_MATRIX_COUNT = 9

HELP_BODY_FRAMES = 11
NETSTATS_BODY_FRAMES = 15
SMP_ACTIVITY_BODY_FRAMES = 16

ACK_RE = re.compile(r"^(?:OK|ERR) [A-Z][A-Z0-9_-]*(?: |$)")
CACHE_SEQUENCE_RE = re.compile(r"^\[cache\] seq=(\d+) ")


class MatrixError(RuntimeError):
    """Deterministic target-matrix protocol failure."""


def parse_port(value: str) -> int:
    """Parse one valid TCP port."""

    try:
        port = int(value, 10)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("port must be an integer") from exc
    if not 1 <= port <= 65535:
        raise argparse.ArgumentTypeError("port must be in 1..=65535")
    return port


def validate_host(value: str) -> str:
    """Reject empty or whitespace-bearing host values."""

    if not value or value != value.strip() or any(char.isspace() for char in value):
        raise MatrixError("TCP host must be a non-empty value without whitespace")
    return value


def load_auth_token() -> str:
    """Load the console credential without accepting it on the command line."""

    token = os.environ.get("COHSH_AUTH_TOKEN", os.environ.get("COH_AUTH_TOKEN", ""))
    if not token:
        raise MatrixError("COHSH_AUTH_TOKEN or COH_AUTH_TOKEN is required")
    if token != token.strip() or "\r" in token or "\n" in token:
        raise MatrixError("console auth token must not contain surrounding whitespace or CR/LF")
    auth_bytes = f"AUTH {token}".encode("utf-8")
    if len(auth_bytes) > MAX_COMMAND_BYTES:
        raise MatrixError("console auth token exceeds the bounded command frame")
    return token


def remaining_seconds(deadline: float, label: str) -> float:
    """Return the positive time remaining before a fixed deadline."""

    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise MatrixError(f"timeout waiting for {label}")
    return remaining


def recv_exact(
    connection: socket.socket,
    size: int,
    deadline: float,
    label: str,
) -> bytes:
    """Read exactly ``size`` bytes before one absolute deadline."""

    data = bytearray()
    while len(data) < size:
        connection.settimeout(remaining_seconds(deadline, label))
        try:
            chunk = connection.recv(size - len(data))
        except socket.timeout as exc:
            raise MatrixError(f"timeout waiting for {label}") from exc
        if not chunk:
            raise MatrixError(
                f"connection closed while reading {label}: "
                f"received={len(data)} expected={size}"
            )
        data.extend(chunk)
    return bytes(data)


def send_frame(connection: socket.socket, line: str, deadline: float) -> None:
    """Send one validated little-endian length-prefixed command line."""

    if not line or "\r" in line or "\n" in line:
        raise MatrixError("outbound command must be one non-empty line")
    payload = line.encode("utf-8")
    if len(payload) > MAX_COMMAND_BYTES:
        raise MatrixError("outbound command exceeds the bounded command frame")
    total = len(payload) + FRAME_HEADER_BYTES
    connection.settimeout(remaining_seconds(deadline, "command send"))
    try:
        connection.sendall(total.to_bytes(FRAME_HEADER_BYTES, "little") + payload)
    except socket.timeout as exc:
        raise MatrixError("timeout sending command frame") from exc


def recv_frame(connection: socket.socket, deadline: float, label: str) -> str:
    """Receive and validate one bounded UTF-8 response frame."""

    header = recv_exact(connection, FRAME_HEADER_BYTES, deadline, f"{label} frame header")
    total = int.from_bytes(header, "little")
    if total < FRAME_HEADER_BYTES or total > MAX_FRAME_BYTES:
        raise MatrixError(f"{label} declared invalid frame length {total}")
    payload = recv_exact(
        connection,
        total - FRAME_HEADER_BYTES,
        deadline,
        f"{label} frame payload",
    )
    try:
        line = payload.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise MatrixError(f"{label} returned non-UTF-8 frame data") from exc
    if "\r" in line or "\n" in line:
        raise MatrixError(f"{label} returned a response frame containing CR/LF")
    return line


def exchange(
    connection: socket.socket,
    command: str,
    expected_ack: str,
    expected_body_frames: int,
    *,
    response_timeout_seconds: float = RESPONSE_TIMEOUT_SECONDS,
    response_frame_limit: int = MAX_MATRIX_RESPONSE_FRAMES,
) -> tuple[str, ...]:
    """Send one command and require its complete body before one exact ACK."""

    label = expected_ack.split(maxsplit=2)[1]
    deadline = time.monotonic() + response_timeout_seconds
    send_frame(connection, command, deadline)
    body: list[str] = []
    for _ in range(response_frame_limit):
        line = recv_frame(connection, deadline, label)
        if line == "END":
            raise MatrixError(f"{label} emitted unexpected stream terminal END")
        if ACK_RE.match(line):
            if line != expected_ack:
                raise MatrixError(
                    f"{label} expected terminal {expected_ack!r}, received {line!r}"
                )
            if len(body) != expected_body_frames:
                raise MatrixError(
                    f"{label} terminal arrived after {len(body)} body frames; "
                    f"expected {expected_body_frames}"
                )
            return tuple(body)
        body.append(line)
        if len(body) > expected_body_frames:
            raise MatrixError(
                f"{label} exceeded its {expected_body_frames}-frame body bound"
            )
    raise MatrixError(f"{label} exceeded the matrix response frame bound")


def validate_help(body: Sequence[str]) -> None:
    """Validate the stable QEMU help envelope."""

    if body[0] != "Commands:" or not any("netstats" in line for line in body):
        raise MatrixError("HELP body is missing the canonical command surface")
    if not any("quit" in line for line in body):
        raise MatrixError("HELP body is missing the quit command")


def validate_netstats(body: Sequence[str]) -> None:
    """Validate the isolated QEMU VirtIO NETSTATS shape."""

    if not body[0].startswith("netstats: rx_pkts="):
        raise MatrixError("NETSTATS body is missing the leading packet counters")
    expected_suffixes = ("netstatus:", "nettest:", "nettargets:")
    if tuple(line.split(maxsplit=1)[0] for line in body[-3:]) != expected_suffixes:
        raise MatrixError("NETSTATS body is missing its status/test/target suffix")


def validate_smp_activity(body: Sequence[str]) -> None:
    """Validate bounded SMP activity begin/end framing."""

    if not body[0].startswith("[smp] activity begin "):
        raise MatrixError("SMP activity body is missing its begin marker")
    if body[-1] != "[smp] activity end":
        raise MatrixError("SMP activity body is missing its end marker")


def validate_cachelog(body: Sequence[str], expected_count: int) -> None:
    """Validate one exact newest-first CACHELOG response."""

    if len(body) != expected_count:
        raise MatrixError(
            f"CACHELOG returned {len(body)} records; expected {expected_count}"
        )
    sequences: list[int] = []
    for line in body:
        match = CACHE_SEQUENCE_RE.match(line)
        if match is None:
            raise MatrixError("CACHELOG returned a noncanonical cache record")
        sequences.append(int(match.group(1), 10))
    if any(current != following + 1 for current, following in zip(sequences, sequences[1:])):
        raise MatrixError("CACHELOG records are not contiguous newest-first records")


def expect_peer_close(connection: socket.socket) -> None:
    """Require the target to close after the terminal QUIT acknowledgement."""

    deadline = time.monotonic() + QUIT_CLOSE_TIMEOUT_SECONDS
    connection.settimeout(remaining_seconds(deadline, "QUIT close"))
    try:
        trailing = connection.recv(1)
    except socket.timeout as exc:
        raise MatrixError("timeout waiting for QUIT close") from exc
    if trailing:
        raise MatrixError("received data after the terminal QUIT acknowledgement")


def run_matrix(
    host: str,
    port: int,
    token: str,
) -> list[str]:
    """Run every matrix command on one non-reconnecting TCP connection."""

    summaries: list[str] = []
    with socket.create_connection(
        (host, port), timeout=RESPONSE_TIMEOUT_SECONDS
    ) as connection:
        connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

        exchange(connection, f"AUTH {token}", "OK AUTH", 0)
        exchange(connection, "ATTACH queen", "OK ATTACH role=queen", 0)

        help_body = exchange(connection, "help", "OK HELP", HELP_BODY_FRAMES)
        validate_help(help_body)
        summaries.append("PASS HELP body_frames=11 ack=OK_HELP")

        netstats_body = exchange(
            connection,
            "netstats",
            "OK NETSTATS",
            NETSTATS_BODY_FRAMES,
        )
        validate_netstats(netstats_body)
        summaries.append("PASS NETSTATS body_frames=15 ack=OK_NETSTATS")

        smp_body = exchange(
            connection,
            "smp activity",
            "OK SMP mode=activity",
            SMP_ACTIVITY_BODY_FRAMES,
        )
        validate_smp_activity(smp_body)
        summaries.append("PASS SMP body_frames=16 ack=OK_SMP_mode=activity")

        cachelog_body = exchange(
            connection,
            f"cachelog {CACHELOG_MATRIX_COUNT}",
            "OK CACHELOG",
            CACHELOG_MATRIX_COUNT,
        )
        validate_cachelog(cachelog_body, CACHELOG_MATRIX_COUNT)
        summaries.append("PASS CACHELOG body_frames=9 ack=OK_CACHELOG")

        ping_body = exchange(
            connection,
            "ping",
            "OK PING reply=pong",
            1,
        )
        if ping_body != ("PONG",):
            raise MatrixError("PING did not return its exact PONG body")
        summaries.append("PASS PING body_frames=1 ack=OK_PING_reply=pong")

        exchange(connection, "quit", "OK QUIT", 0)
        connection.shutdown(socket.SHUT_WR)
        expect_peer_close(connection)
        summaries.append("PASS QUIT body_frames=0 ack=OK_QUIT close=EOF")

    summaries.append("PASS TCP_MATRIX same_connection=yes handshakes=2 commands=6")
    return summaries


def build_parser() -> argparse.ArgumentParser:
    """Build the target-matrix command-line parser."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--host",
        default=os.environ.get("COHSH_TCP_HOST", "127.0.0.1"),
        help="QEMU TCP console host (default: COHSH_TCP_HOST or 127.0.0.1)",
    )
    parser.add_argument(
        "--port",
        type=parse_port,
        default=os.environ.get("COHSH_TCP_PORT", "31337"),
        help="QEMU TCP console port (default: COHSH_TCP_PORT or 31337)",
    )
    parser.add_argument(
        "--mode",
        choices=("fixed",),
        default="fixed",
        help="fixed functional response matrix",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """Run the command-line target matrix."""

    args = build_parser().parse_args(argv)
    try:
        host = validate_host(args.host)
        token = load_auth_token()
        summaries = run_matrix(
            host,
            args.port,
            token,
        )
    except (MatrixError, OSError) as exc:
        print(f"FAIL TCP_MATRIX {exc}", file=sys.stderr)
        return 1
    for summary in summaries:
        print(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
