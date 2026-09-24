# Author: Lukas Bower
# Purpose: Prepare digest-pinned user batch-edge work and verify its native output independently.
# Copyright 2026 Lukas Bower

"""Prepare a useful CUDA image batch without changing Cohesix source."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

from cohesix.workload import (batch_edges_expected, read_batch_artifact,
                              verify_batch_edges)


def digest(data: bytes) -> str:
    """Return a lower-case content address."""
    return hashlib.sha256(data).hexdigest()


def write_new(path: Path, data: bytes) -> None:
    """Create immutable operator input without replacing an existing record."""
    with path.open("xb") as stream:
        stream.write(data)


def prepare(args: argparse.Namespace) -> None:
    """Freeze package, dataset, bounds and expected output before admission."""
    for value, length in ((args.device_uuid, 32),
                          (args.cuda_helper_sha256, 64),
                          (args.topology_sha256, 64),
                          (args.provider_graph_sha256, 64)):
        if len(value) != length or not re.fullmatch(r"[0-9a-f]+", value):
            raise ValueError("invalid native device or generated digest")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", args.ticket_id):
        raise ValueError("invalid ticket identity")
    data = read_batch_artifact(args.input)
    expected = batch_edges_expected(data, args.width, args.height, args.frames)
    package = args.package.resolve(strict=True)
    package_hash = digest(read_batch_artifact(package, 16 * 1024 * 1024))
    input_root = args.input_root.resolve(strict=True)
    if not input_root.is_dir():
        raise ValueError("input CAS root must be a directory")
    input_hash = digest(data)
    input_cas = input_root / input_hash
    if input_cas.exists():
        if read_batch_artifact(input_cas) != data:
            raise ValueError("input CAS conflict")
    else:
        write_new(input_cas, data)
    registration = {
        "schema": "cohesix-cuda-registration/v1",
        "id": "batch-edges",
        "package": str(package),
        "package_sha256": package_hash,
        "entrypoint": "run",
        "input_root": str(input_root),
        "max_input_bytes": 262_144,
        "max_output_bytes": 262_144,
        "max_memory_bytes": 2 * 1024 * 1024,
        "max_disk_bytes": 32 * 1024 * 1024,
        "max_deadline_ms": 30_000,
        "device_uuid": args.device_uuid,
        "parameters": {
            "frames": {"kind": "integer", "minimum": 1, "maximum": 4},
            "height": {"kind": "integer", "minimum": 3, "maximum": 256},
            "iterations": {"kind": "integer", "minimum": 1, "maximum": 100000},
            "width": {"kind": "integer", "minimum": 3, "maximum": 256},
        },
        "secret_refs": {},
    }
    registration_bytes = json.dumps(registration, indent=2, allow_nan=False).encode() + b"\n"
    write_new(args.registration_out, registration_bytes)
    request = {
        "schema": "cohesix-gpu-workload-input/v2",
        "artifact_sha256": args.cuda_helper_sha256,
        "topology_sha256": args.topology_sha256,
        "expected_output_sha256": digest(expected),
        "request": {
            "schema": "cohesix-registered-cuda-request/v1",
            "ticket_id": args.ticket_id,
            "registration_sha256": digest(registration_bytes),
            "input_sha256": input_hash,
            "parameters": {
                "frames": args.frames,
                "height": args.height,
                "iterations": args.iterations,
                "width": args.width,
            },
            "device_ordinal": 0,
            "device_uuid": args.device_uuid,
            "inventory_observed_unix_ms": args.inventory_observed_unix_ms,
            "provider_graph_sha256": args.provider_graph_sha256,
            "memory_budget_bytes": 2 * 1024 * 1024,
            "deadline_ms": 30_000,
        },
    }
    request_bytes = json.dumps(request, separators=(",", ":"), allow_nan=False).encode()
    write_new(args.request_out, request_bytes)
    print(json.dumps({
        "schema": "cohesix-batch-edges-preparation/v1",
        "registration_sha256": digest(registration_bytes),
        "request_sha256": digest(request_bytes),
        "input_sha256": input_hash,
        "expected_output_sha256": digest(expected),
        "pixels": len(expected),
        "authoritative": False,
        "execution": "not_started",
    }, indent=2))


def verify(args: argparse.Namespace) -> None:
    """Recompute every output byte and check the frozen expected digest."""
    report = verify_batch_edges(args.input, args.output, args.width,
                                args.height, args.frames)
    request = json.loads(args.request.read_text(encoding="utf-8"))
    if report["observed_output_sha256"] != request["expected_output_sha256"]:
        raise ValueError("output differs from the frozen request")
    print(json.dumps(report, indent=2))


def main() -> None:
    """Expose preparation and verification as separate reviewable steps."""
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    preparation = subcommands.add_parser("prepare")
    preparation.add_argument("--package", type=Path, required=True)
    preparation.add_argument("--input", type=Path, required=True)
    preparation.add_argument("--input-root", type=Path, required=True)
    preparation.add_argument("--registration-out", type=Path, required=True)
    preparation.add_argument("--request-out", type=Path, required=True)
    preparation.add_argument("--ticket-id", required=True)
    preparation.add_argument("--device-uuid", required=True)
    preparation.add_argument("--cuda-helper-sha256", required=True)
    preparation.add_argument("--topology-sha256", required=True)
    preparation.add_argument("--provider-graph-sha256", required=True)
    preparation.add_argument("--inventory-observed-unix-ms", type=int, required=True)
    preparation.add_argument("--iterations", type=int, default=1)
    for argument in (preparation, subcommands.add_parser("verify")):
        argument.add_argument("--width", type=int, required=True)
        argument.add_argument("--height", type=int, required=True)
        argument.add_argument("--frames", type=int, required=True)
    verification = subcommands.choices["verify"]
    verification.add_argument("--input", type=Path, required=True)
    verification.add_argument("--output", type=Path, required=True)
    verification.add_argument("--request", type=Path, required=True)
    args = parser.parse_args()
    (prepare if args.command == "prepare" else verify)(args)


if __name__ == "__main__":
    main()
