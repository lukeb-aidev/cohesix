#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Collect and validate complete bounded Worker records from authenticated qlog exports.
# Copyright 2026 Lukas Bower

"""Reassemble target-issued Worker fragments without inventing missing proof."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from typing import Callable, Mapping, Sequence
import urllib.parse
import urllib.request

FRAGMENT = re.compile(
    r"WORKER_LOG id=([0-9]+) part=([0-9]+) last=([01]) data=(.*)$"
)
RECORD_PREFIXES = (
    "WORKER_TASK_",
    "GPU_BRIDGE_FIXTURE_ADMISSION ",
    "LORA_EXPORT_FIXTURE_ADMISSION ",
)
MAX_RETAINED_BYTES = 64 * 1024 * 1024


def records(text: str, *, complete: bool = True) -> str:
    """Deduplicate identical exports and reject conflicting or incomplete records."""
    grouped: dict[int, dict[int, tuple[bool, str]]] = {}
    for line in text.splitlines():
        if line.startswith("WORKER_LOG_ERROR "):
            raise ValueError("target could not retain a complete Worker record")
        match = FRAGMENT.fullmatch(line)
        if match is None:
            if line.startswith("WORKER_LOG "):
                raise ValueError("malformed Worker fragment envelope")
            continue
        identity, part = int(match[1]), int(match[2])
        if identity > 2**64 - 1 or part > 5 or len(match[4].encode()) > 176:
            raise ValueError("Worker fragment exceeds target bounds")
        value = (match[3] == "1", match[4])
        fragments = grouped.setdefault(identity, {})
        previous = fragments.setdefault(part, value)
        if previous != value:
            raise ValueError("conflicting Worker fragment for the same record")
    output = []
    for identity, fragments in sorted(grouped.items()):
        last = [part for part, (terminal, _) in fragments.items() if terminal]
        if len(last) > 1:
            raise ValueError("Worker record has multiple terminal fragments")
        if not last or set(fragments) != set(range(last[0] + 1)):
            if complete:
                raise ValueError(f"Worker record {identity} is incomplete")
            continue
        record = "".join(fragments[part][1] for part in range(last[0] + 1))
        if len(record.encode()) > 1024 or not record.startswith(RECORD_PREFIXES):
            raise ValueError("Worker record violates the target record contract")
        output.append(record)
    return "\n".join(output) + ("\n" if output else "")


def append_export(path: Path, text: str) -> None:
    """Retain exported data lines; decoding never rewrites the source artifact."""
    with path.open("a", encoding="utf-8") as output:
        output.write(text)
        if not text.endswith("\n"):
            output.write("\n")


class ExportMonitor:
    """One bounded reader through the existing gateway; failures invalidate proof."""

    def __init__(self, path: Path, read: Callable[[], str]) -> None:
        self.path = path
        self.read = read
        self.stop_event = threading.Event()
        self.error: Exception | None = None
        self.export_lock = threading.Lock()
        raw = b""
        if path.exists():
            with path.open("rb") as retained:
                raw = retained.read(MAX_RETAINED_BYTES + 1)
        self.retained_bytes = len(raw)
        if self.retained_bytes > MAX_RETAINED_BYTES:
            raise ValueError("Worker log exceeds its evidence byte bound")
        existing = raw.decode("utf-8")
        self.fragments = {
            line for line in existing.splitlines()
            if line.startswith(("WORKER_LOG ", "WORKER_LOG_ERROR "))
        }
        self.observed = set(records(existing, complete=False).splitlines())
        self.thread = threading.Thread(
            target=self._run, name="worker-log-export", daemon=True,
        )

    def _run(self) -> None:
        try:
            while not self.stop_event.is_set():
                self.checkpoint(opportunistic=True)
                self.stop_event.wait(5)
        except Exception as error:
            self.error = error

    def start(self) -> None:
        self.thread.start()

    def checkpoint(
        self,
        required: Sequence[Mapping[str, object]] = (),
        *,
        after_generation: int = 0,
        timeout_s: float = 15.0,
        opportunistic: bool = False,
    ) -> None:
        """Retain original fragments before dependent traffic can evict proof."""
        deadline = time.monotonic() + timeout_s
        while True:
            if opportunistic:
                if not self.export_lock.acquire(blocking=False):
                    return
            elif not self.export_lock.acquire(timeout=max(0.0, deadline - time.monotonic())):
                raise ValueError("Worker log checkpoint exceeded its capture deadline")
            try:
                if self.error is not None:
                    raise ValueError("Worker log export failed") from self.error
                # One export may satisfy several concurrently completed Workers.
                # Their exact identities and sequences remain the proof keys.
                if required and self._observed(required, after_generation):
                    return
                exported = self.read()
                additions = []
                for line in dict.fromkeys(exported.splitlines()):
                    if line.startswith(("WORKER_LOG ", "WORKER_LOG_ERROR ")):
                        if line not in self.fragments:
                            additions.append(line)
                if additions:
                    payload = "\n".join(additions) + "\n"
                    if self.retained_bytes + len(payload.encode()) > MAX_RETAINED_BYTES:
                        self.error = ValueError("Worker log exceeds its evidence byte bound")
                        raise self.error
                    append_export(self.path, payload)
                    self.retained_bytes += len(payload.encode())
                    self.fragments.update(additions)
                self.observed.update(records(exported, complete=False).splitlines())
                if self._observed(required, after_generation):
                    return
            finally:
                self.export_lock.release()
            if time.monotonic() >= deadline:
                raise ValueError("Worker log checkpoint lacks required target records")
            self.stop_event.wait(0.1)

    def _observed(
        self, required: Sequence[Mapping[str, object]], after_generation: int,
    ) -> bool:
        return all(
            any(record_matches(line, expected, after_generation) for line in self.observed)
            for expected in required
        )

    def stop(self) -> None:
        self.stop_event.set()
        self.thread.join(timeout=35)
        if self.thread.is_alive():
            raise ValueError("Worker log export did not stop within its read deadline")
        if self.error is not None:
            raise ValueError("Worker log export failed") from self.error

    def finish(self) -> None:
        self.stop()
        self.checkpoint()
        records(self.path.read_text())


def record_matches(
    line: str, expected: Mapping[str, object], after_generation: int = 0,
) -> bool:
    """Compare exact target fields; equal per-Worker sequences are not identity."""
    tokens = line.split()
    if not tokens:
        return False
    fields = dict(token.split("=", 1) for token in tokens[1:] if "=" in token)
    fields["marker"] = tokens[0]
    generation = fields.get("supervisor_generation", "")
    return (
        generation.isdecimal() and int(generation) > after_generation
        and all(fields.get(key) == str(value) for key, value in expected.items())
    )


def capture(args: argparse.Namespace) -> None:
    """Use the current sole console owner and wait only on complete target records."""
    deadline = time.monotonic() + args.timeout
    while True:
        if args.rest_url:
            query = urllib.parse.urlencode({
                "path": "/log/queen.log", "max_bytes": 524288,
            })
            request = urllib.request.Request(f"{args.rest_url}/v1/fs/cat?{query}")
            token = os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN", "")
            if token:
                request.add_header("Authorization", f"Bearer {token}")
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read(2 * 1024 * 1024 + 1)
            if len(raw) > 2 * 1024 * 1024:
                raise ValueError("qlog export exceeds its response bound")
            payload = json.loads(raw)
            if not isinstance(payload, dict):
                raise ValueError("qlog export response is not an object")
            lines = payload.get("lines", [])
            if (
                payload.get("status") != "OK"
                or payload.get("end") is not True
                or payload.get("path") != "/log/queen.log"
                or payload.get("verb") != "CAT"
                or not isinstance(lines, list)
                or not all(isinstance(line, str) for line in lines)
            ):
                raise ValueError("qlog export is not one complete CAT response")
            text = "\n".join(lines) + "\n"
        else:
            with tempfile.TemporaryDirectory(prefix="cohesix-worker-log-") as temp:
                script = Path(temp) / "log.coh"
                script.write_text(
                    "attach queen\nEXPECT OK\n"
                    "cat /log/queen.log\nEXPECT OK\nquit\n"
                )
                result = subprocess.run(
                    [
                        args.cohsh, "--transport", "tcp", "--tcp-host", args.host,
                        "--tcp-port", str(args.port), "--script", str(script),
                    ],
                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                    check=False, timeout=30,
                )
            text = result.stdout.decode("utf-8")
            append_export(args.out, text)
            if result.returncode:
                raise ValueError(
                    f"authenticated qlog export failed ({result.returncode})"
                )
        if args.rest_url:
            append_export(args.out, text)
        if not args.wait_marker:
            return
        observed = records(args.out.read_text(), complete=False)
        if sum(args.wait_marker in line for line in observed.splitlines()) >= args.count:
            return
        if time.monotonic() >= deadline:
            raise ValueError(f"timed out waiting for complete {args.wait_marker}")
        time.sleep(0.1)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cohsh")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=31337)
    parser.add_argument("--rest-url")
    parser.add_argument("--wait-marker")
    parser.add_argument("--count", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    if bool(args.cohsh) == bool(args.rest_url):
        parser.error("select exactly one current console owner: cohsh or REST")
    if (
        not math.isfinite(args.timeout) or not 0 < args.timeout <= 600
        or not 1 <= args.port <= 65535 or args.count < 1
    ):
        parser.error("invalid timeout, TCP port or positive marker count")
    capture(args)


if __name__ == "__main__":
    main()
