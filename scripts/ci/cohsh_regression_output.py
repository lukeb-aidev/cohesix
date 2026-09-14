#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify regression data streams independently of ACK previews.
# Copyright 2026 Lukas Bower
"""Validate the append fixture's exact records in its successful CAT stream."""

from __future__ import annotations

import argparse
from pathlib import Path


def verify_append_stream(text: str) -> None:
    """Require all three exact records once and in order in one CAT response.

    ACK previews are metadata. Only standalone returned data records count;
    unrelated boot/audit records may appear between application appends.
    The invoking runner separately requires successful script completion,
    including overflow refusal and the transport's terminal framing.
    """
    started = False
    finished = False
    records: list[str] = []
    for line in text.splitlines():
        if line.startswith("[console] OK CAT path=/log/queen.log "):
            if started:
                raise ValueError("multiple append CAT responses")
            started = True
            continue
        if started and line.startswith("[console] "):
            finished = True
            break
        if started and line in {"batch-1", "batch-2", "batch-3"}:
            records.append(line)
    if not started or not finished:
        raise ValueError("append CAT response is missing or unterminated")
    if records != ["batch-1", "batch-2", "batch-3"]:
        raise ValueError("append CAT records are missing, duplicated or reordered")


def main() -> None:
    """Apply only the named canonical fixture's independent output oracle."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--script", required=True)
    parser.add_argument("--log", type=Path, required=True)
    args = parser.parse_args()
    if args.script != "9p_batch.coh":
        return
    with args.log.open("rb") as stream:
        payload = stream.read(8 * 1024 * 1024 + 1)
    if len(payload) > 8 * 1024 * 1024:
        parser.error("regression log exceeds 8 MiB")
    try:
        verify_append_stream(payload.decode("utf-8"))
    except (UnicodeError, ValueError) as error:
        parser.error(str(error))
    print("PASS append stream: batch-1, batch-2, batch-3 in exact order")


if __name__ == "__main__":
    main()
