#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Prepare retained strict Queen requests without submitting or minting authority.
# Copyright 2026 Lukas Bower

"""Write separate approval, intent and observation scripts for one reviewed request."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import time
import uuid
from pathlib import Path
from typing import Any

HEADER = (
    "# Author: Lukas Bower\n"
    "# Purpose: Retain one reviewed 1.1.0-beta Queen operation and its identity.\n"
    "# Copyright 2026 Lukas Bower\n"
)


def identifier(value: str) -> str:
    """Accept a bounded authority identifier, never a path or script fragment."""
    if (
        not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]{0,63}", value)
        or ".." in value
    ):
        raise argparse.ArgumentTypeError("expected a 1..64 byte authority identifier")
    return value


def unsigned(value: str) -> int:
    """Parse the unsigned 64-bit writer epoch read from the selected Queen."""
    if not value.isascii() or not value.isdecimal():
        raise argparse.ArgumentTypeError("writer epoch must be an unsigned integer")
    epoch = int(value)
    if epoch > (1 << 64) - 1:
        raise argparse.ArgumentTypeError("writer epoch exceeds u64")
    return epoch


def prepare(
    output: Path,
    command: dict[str, Any],
    epoch: int,
    operation_id: str,
    issued_ms: int,
) -> None:
    """Create a private directory exclusively so an uncertain request cannot be replaced."""
    identifier(operation_id)
    if type(epoch) is not int or not 0 <= epoch <= (1 << 64) - 1:
        raise ValueError("writer epoch must fit u64")
    if type(issued_ms) is not int or not 0 < issued_ms <= (1 << 64) - 1:
        raise ValueError("issue time must be a positive u64")
    # Only the fixed CLI command shapes are admitted by this demo helper.
    if command not in (
        {"spawn": "heartbeat", "ticks": 10, "budget": {"ttl_s": 60, "ops": 100}},
        {"spawn": "lora"},
    ):
        if set(command) != {"kill"} or not isinstance(command["kill"], str):
            raise ValueError("unsupported demo command")
        identifier(command["kill"])
    intent = {
        "schema": "queen-intent/v1",
        "id": operation_id,
        "idempotency_key": operation_id,
        "issued_unix_ms": issued_ms,
        "writer_epoch": epoch,
        "cmd": json.dumps(command, separators=(",", ":")),
    }
    payload = json.dumps(intent, separators=(",", ":"))
    if len(payload.encode("utf-8")) > 2048:
        raise ValueError("intent exceeds the console echo payload bound")
    approval = json.dumps(
        {"id": operation_id, "target": "/queen/ctl", "decision": "approve"},
        separators=(",", ":"),
    )
    output.mkdir(mode=0o700, parents=False, exist_ok=False)
    scripts = {
        "approve.coh": (
            "# Run once only if the selected policy requires this approval.\n"
            "attach queen\nEXPECT OK\n"
            f"echo '{approval}' > /actions/queue\nEXPECT OK\n"
            f"cat /actions/{operation_id}/status\nEXPECT OK\nquit\n"
        ),
        "intent.coh": (
            "# This exact file is the retry identity. Never regenerate an ambiguous request.\n"
            "attach queen\nEXPECT OK\n"
            f"echo {payload} > /queen/intents/ctl\nEXPECT OK\n"
            "cat /proc/queen/dedupe\nEXPECT OK\nls /worker\nEXPECT OK\nquit\n"
        ),
        "observe.coh": (
            "# Read published state; admission alone is not Worker readiness.\n"
            "attach queen\nEXPECT OK\ncat /proc/authority\nEXPECT OK\n"
            "cat /proc/queen/dedupe\nEXPECT OK\nls /worker\nEXPECT OK\n"
            "ls /shard\nEXPECT OK\ntail /log/queen.log 16\nEXPECT OK\nquit\n"
        ),
    }
    for name, body in scripts.items():
        path = output / name
        with path.open("x", encoding="utf-8") as stream:
            stream.write(HEADER + body)
        path.chmod(0o600)
    digest = hashlib.sha256((output / "intent.coh").read_bytes()).hexdigest()
    print(f"Prepared {output}; operation={operation_id}; intent_sha256={digest}")
    print("mode=prepared proof=none; review before execution; no request was submitted")


def main() -> None:
    """Select a small bounded command, retaining fresh time and operation identity."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--writer-epoch", type=unsigned, required=True)
    parser.add_argument("--out", type=Path, required=True)
    commands = parser.add_subparsers(dest="action", required=True)
    commands.add_parser("heartbeat", help="request 10 ticks with a 60-second/100-op budget")
    commands.add_parser("lora", help="request a LoRA receipt Worker, without training")
    kill = commands.add_parser("kill", help="terminate only an explicitly observed Worker")
    kill.add_argument("--worker-id", type=identifier, required=True)
    args = parser.parse_args()
    if args.action == "heartbeat":
        command = {"spawn": "heartbeat", "ticks": 10, "budget": {"ttl_s": 60, "ops": 100}}
    elif args.action == "lora":
        command = {"spawn": "lora"}
    else:
        command = {"kill": args.worker_id}
    try:
        prepare(
            args.out, command, args.writer_epoch,
            "demo-" + uuid.uuid4().hex, time.time_ns() // 1_000_000,
        )
    except (OSError, ValueError) as error:
        parser.exit(2, f"prepare_worker: {error}\n")


if __name__ == "__main__":
    main()
