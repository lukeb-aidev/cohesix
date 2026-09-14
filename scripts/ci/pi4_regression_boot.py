#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Bind physical regression groups to fresh boots of one exact image.
# Copyright 2026 Lukas Bower
"""Run the operator-selected boot collector without shell evaluation."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
from typing import Any

from qemu_artifact import verify_pi4_transport_evidence


def verify_fresh_boot(prior: dict[str, Any], current: dict[str, Any]) -> None:
    """Preserve all target bindings while requiring a new physical boot ID."""
    for field in ("schema", "claim_tier", "target", "source_digest",
                  "image_identity", "target_host", "gateway_url",
                  "gateway_target_host"):
        if current.get(field) != prior.get(field):
            raise ValueError(f"fresh Pi boot changed {field}")
    if current["boot_id"] == prior["boot_id"]:
        raise ValueError("Pi regression group reused the previous boot ID")


def collector_digest(path: Path) -> str:
    """Bound the operator-supplied collector before executing or rechecking it."""
    with path.open("rb") as stream:
        payload = stream.read(4 * 1024 * 1024 + 1)
    if not payload or len(payload) > 4 * 1024 * 1024:
        raise ValueError("Pi boot collector must contain 1..4194304 bytes")
    return hashlib.sha256(payload).hexdigest()


def run_collector(command: list[str], log: Path, timeout: int = 600) -> int:
    """Bound execution and terminate all owned descendants at the boundary."""
    with log.open("xb") as output:
        process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        try:
            return process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return 124
        finally:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()


def main() -> None:
    """Keep the boot transcript, exact collector identity and validated receipt."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--collector", type=Path, required=True)
    parser.add_argument("--prior-evidence", type=Path, required=True)
    parser.add_argument("--source-digest", required=True)
    parser.add_argument("--seen-evidence", type=Path, action="append", default=[])
    parser.add_argument(
        "--group", choices=("base", "base-telemetry", "base-shard", "gated"),
        required=True,
    )
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    collector = args.collector.resolve(strict=True)
    if not collector.is_file():
        parser.error("boot collector must be a regular executable file")
    prior = verify_pi4_transport_evidence(
        args.prior_evidence, expected_source_digest=args.source_digest,
    )
    if len(args.seen_evidence) > 4:
        parser.error("at most four previous boot records are permitted")
    seen = [
        verify_pi4_transport_evidence(path, expected_source_digest=args.source_digest)
        for path in args.seen_evidence
    ]
    args.out.mkdir(parents=True, exist_ok=False)
    identity = collector_digest(collector)
    command = [str(collector), "--group", args.group, "--out", str(args.out.resolve()),
               "--prior-evidence", str(args.prior_evidence.resolve()),
               "--source-digest", args.source_digest]
    returncode = run_collector(command, args.out / "collector.log")
    (args.out / "collector.json").write_text(json.dumps({
        "command": command, "sha256": identity, "exit_code": returncode,
        "timeout_seconds": 600,
    }, indent=2) + "\n")
    if returncode:
        parser.error(f"Pi boot collector failed with exit {returncode}")
    if collector_digest(collector) != identity:
        parser.error("Pi boot collector changed during execution")
    output = args.out / "target-evidence.json"
    current = verify_pi4_transport_evidence(
        output, expected_source_digest=args.source_digest,
    )
    verify_fresh_boot(prior, current)
    for previous in seen:
        verify_fresh_boot(previous, current)
    print(output.resolve())


if __name__ == "__main__":
    main()
