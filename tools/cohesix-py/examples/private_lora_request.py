#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Prepare an immutable release request from operator-reviewed provenance and the current native baseline without creating execution evidence.
# Copyright 2026 Lukas Bower
"""Prepare an import or training request on the configured native CUDA host.

The supplied input must already contain the source attestation and exact profile
bindings. This example never signs source claims, submits tickets or emits scores.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cohesix.hf_native import Provider, encode, regular, write


def prepare(config: Path, source: Path, operation: str, model: str, entry: str) -> dict:
    """Freeze the observed baseline and predeclared reference quality bounds."""
    if not all(re.fullmatch(r"[A-Za-z0-9_-]{1,64}", value) for value in [operation, model]):
        raise ValueError("invalid operation or model identifier")
    provider = Provider(config)
    payload = regular(source, 262144)
    inputs = json.loads(payload)
    if inputs["profile_sha256"] != provider.config["profile_sha256"]:
        raise ValueError("input profile does not match the configured native profile")
    input_ref = provider.store(encode(inputs))
    current = json.loads(regular(provider.root / "accepted.json", 8192))
    # Field order matches the shared Rust request contract; the executor rejects
    # noncanonical encodings before dispatch.
    baseline = {name: current[name] for name in ["generation", "adapter_sha256",
                                                 "served_artifact_sha256", "runtime_sha256", "healthy", "rollback_verified"]}
    request = {"schema": "cohesix-peft-release/v1", "operation_id": operation, "model_id": model,
               "entry": entry, "profile_sha256": provider.config["profile_sha256"], "input_sha256": input_ref,
               "evaluation_policy": {"minimum_samples": 16, "maximum_age_ms": 3600000,
                                     "metrics": {"eval_loss": {"direction": "lower", "absolute_bound": 8.0, "maximum_regression": 0.0}}},
               "baseline": baseline}
    raw = json.dumps(request, separators=(",", ":"), allow_nan=False).encode()
    digest = provider.store(raw)
    return {"mode": "prepared-only", "request_sha256": digest, "request": request}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-config", type=Path, required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--operation", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--entry", choices=["import", "train"], required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = prepare(args.native_config, args.input,
                     args.operation, args.model, args.entry)
    if args.out.exists():
        raise ValueError("refusing to overwrite an earlier request")
    write(args.out, encode(result))
    print(json.dumps({"mode": result["mode"],
          "request_sha256": result["request_sha256"]}))


if __name__ == "__main__":
    main()
