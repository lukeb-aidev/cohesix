#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify a real application request against an observed PEFT serving generation and fixed continuation.
# Copyright 2026 Lukas Bower
"""Call the selected local Transformers endpoint after promotion or recovery."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
import time
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cohesix.hf_native import Provider, Refused, encode, read_json, require, sha, write


def request(config: Path, operation: str, generation: int,
            adapter: str | None, rollback: bool = False) -> dict:
    """Bind the HTTP output, active process and accepted generation separately."""
    provider = Provider(config)
    accepted = read_json(provider.root / "accepted.json")
    runtime = read_json(provider.root / "runtime.json")
    require(accepted["generation"] == generation and accepted["adapter_sha256"] == adapter
            and runtime["adapter_sha256"] == adapter and accepted["healthy"],
            "application_generation_or_adapter_changed")
    model, _ = provider.bundle(runtime["bundle_sha256"])
    before = provider.service("show")
    require(before.get("ActiveState") == "active" and before.get("MainPID") not in {None, "0"}
            and before.get("InvocationID"), "application_runtime_not_ready")
    expected = read_json(provider.root / "operations" / operation /
                         ("baseline-behavior.json" if rollback else "candidate-behavior.json"))
    prompt = provider.profile["canary"]["prompts"][0]
    started = time.monotonic()
    payload = encode({"model": str(model), "messages": [{"role": "user", "content": prompt}],
                      "stream": True, "temperature": 0,
                      "max_tokens": provider.profile["canary"]["max_tokens"]})
    http_request = urllib.request.Request(
        f'http://127.0.0.1:{provider.config["port"]}/v1/chat/completions',
        data=payload, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(http_request, timeout=30) as response:
        require(response.status == 200, "application_http_status")
        stream = response.read(65537)
    require(len(stream) <= 65536, "application_response_bound")
    parts = []
    finished = False
    for line in stream.decode().splitlines():
        if not line.startswith("data: ") or line == "data: [DONE]":
            continue
        chunk = json.loads(line[6:])
        require(chunk.get("model") == str(model) + "@main", "application_served_model_identity")
        choices = chunk.get("choices")
        require(isinstance(choices, list) and len(choices) == 1, "application_response_shape")
        parts.append(choices[0]["delta"].get("content") or "")
        finished = finished or choices[0].get("finish_reason") is not None
    text = "".join(parts)
    latency = int((time.monotonic() - started) * 1000)
    after = provider.service("show")
    require(finished and text == expected["texts"][0]
            and latency <= provider.profile["canary"]["maximum_latency_ms"]
            and after.get("InvocationID") == before["InvocationID"],
            "application_behavior_latency_or_runtime_changed")
    return {"schema": "cohesix-peft-application-observation/v1", "operation_id": operation,
            "generation": generation, "adapter_sha256": adapter,
            "served_bundle_sha256": runtime["bundle_sha256"],
            "native_invocation": before["InvocationID"], "prompt_sha256": sha(prompt.encode()),
            "output_sha256": sha(text.encode()), "latency_ms": latency,
            "rollback": rollback, "observed_unix_ms": int(time.time() * 1000)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-config", type=Path, required=True)
    parser.add_argument("--operation", required=True)
    parser.add_argument("--generation", type=int, required=True)
    parser.add_argument("--adapter", help="Accepted adapter SHA-256; omit for the base")
    parser.add_argument("--rollback", action="store_true")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.out.exists():
        parser.error("refusing to overwrite a previous application observation")
    try:
        result = request(args.native_config, args.operation, args.generation,
                         args.adapter, args.rollback)
    except (Refused, OSError, ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    write(args.out, encode(result))
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
