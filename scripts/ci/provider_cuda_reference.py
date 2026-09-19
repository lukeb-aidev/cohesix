# Author: Lukas Bower
# Purpose: Retain non-authoritative native CUDA reference outcomes and safe negative cases through gpu-bridge-host.
# Copyright 2026 Lukas Bower

"""Diagnostic CUDA lane; production admission and Worker proof stay separate."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import time
from typing import Any


MAX_CAPTURE = 65_536


def run_reference(bridge: Path, build: Path, state: Path) -> dict[str, Any]:
    """Exercise finite native references without exhausting shared GPU/host memory."""
    state = state.resolve()
    with (build / "build.json").open("rb") as stream:
        manifest_raw = stream.read(8193)
    if len(manifest_raw) > 8192:
        raise ValueError("reference build manifest exceeds bound")
    manifest = json.loads(manifest_raw)
    if manifest.get("schema") != "cohesix-cuda-reference-build/v1":
        raise ValueError("unsupported reference build schema")
    helper = build / "cohesix-cuda-reference"
    digest = manifest["binary_sha256"]
    if helper.stat().st_size > 16 * 1024 * 1024 or hashlib.sha256(helper.read_bytes()).hexdigest() != digest:
        raise ValueError("reference helper digest mismatch")
    bridge = bridge.resolve(strict=True)
    common = [str(bridge), "--reference-helper", str(helper.resolve()),
              "--reference-helper-sha256", digest]
    state.mkdir(parents=True, mode=0o700)
    results = []

    def invoke(name: str, arguments: list[str]) -> subprocess.CompletedProcess[str]:
        # These are owned bounded binaries with finite stdout/stderr schemas. File
        # capture avoids an unbounded communicate buffer even on a broken build.
        stdout_path, stderr_path = state / f"{name}.stdout", state / f"{name}.stderr"
        with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
            result = subprocess.run(common + arguments, stdout=stdout, stderr=stderr,
                                    timeout=40, check=False)
        if stdout_path.stat().st_size > MAX_CAPTURE or stderr_path.stat().st_size > MAX_CAPTURE:
            raise ValueError("CUDA reference output exceeds bound")
        return subprocess.CompletedProcess(result.args, result.returncode,
                                           stdout_path.read_text(), stderr_path.read_text())

    def request(name: str, entrypoint: str = "vadd", dimension: int = 4096) -> dict[str, Any]:
        observed = invoke(f"inventory-{name}", ["--reference-inventory", "--reference-state", str(state / f"inventory-{name}")])
        if observed.returncode:
            raise ValueError(f"CUDA inventory unavailable; see inventory-{name}.stderr")
        inventory = json.loads(observed.stdout)
        if inventory["provider_graph_sha256"] != manifest["provider_graph_sha256"]:
            raise ValueError("compiled bridge/build provider graph mismatch")
        return {"schema": "cohesix-cuda-reference-request/v1", "ticket_id": f"m27b-reference-{name}",
                "entrypoint": entrypoint, "dimension": dimension, "iterations": 1,
                "device_ordinal": inventory["native"]["device_ordinal"],
                "device_uuid": inventory["native"]["device_uuid"],
                "inventory_observed_unix_ms": inventory["observed_unix_ms"],
                "provider_graph_sha256": inventory["provider_graph_sha256"],
                "memory_budget_bytes": 1024 * 1024, "deadline_ms": 10_000}

    cases = [("vadd", "verified"), ("matmul", "verified"), ("wrong-device", "wrong_device"),
             ("over-budget", "memory_admission"), ("stale-inventory", "freshness"),
             ("timeout", "timeout"), ("cancel", "cancelled")]
    for name, expected in cases:
        payload = request(name, "matmul" if name == "matmul" else "vadd", 64 if name == "matmul" else 4096)
        extra = []
        if name == "wrong-device":
            payload["device_uuid"] = ("0" if payload["device_uuid"][0] != "0" else "1") + payload["device_uuid"][1:]
        elif name == "over-budget":
            payload["memory_budget_bytes"] = 1
        elif name == "stale-inventory":
            payload["inventory_observed_unix_ms"] -= 5000
        elif name == "timeout":
            payload["deadline_ms"] = 1
        elif name == "cancel":
            payload["iterations"] = 10_000
            extra = ["--reference-cancel-after-ms", "100"]
        request_path = state / f"{name}.request.json"
        request_path.write_text(json.dumps(payload, sort_keys=True) + "\n")
        started = time.monotonic_ns()
        observed = invoke(name, ["--reference-request", str(request_path), "--reference-state", str(state / name)] + extra)
        elapsed_ms = (time.monotonic_ns() - started) / 1_000_000
        outcome = "PASS" if ((expected == "verified" and observed.returncode == 0 and json.loads(observed.stdout).get("outcome") == "verified")
                             or (expected != "verified" and observed.returncode != 0 and expected in observed.stderr)) else "FAIL"
        results.append({"case": name, "expected": expected, "result": outcome,
                        "exit_code": observed.returncode, "elapsed_ms": elapsed_ms,
                        "stdout_sha256": hashlib.sha256(observed.stdout.encode()).hexdigest(),
                        "stderr_sha256": hashlib.sha256(observed.stderr.encode()).hexdigest()})
        # A failure routes the next implementation change; never repeat until green.
        if outcome == "FAIL":
            break
    summary = {"schema": "cohesix-cuda-reference-conformance/v1", "claiming": False,
               "authoritative": False, "worker_proof": False, "production_proven": False,
               "reference_result": "PASS" if len(results) == len(cases) and all(row["result"] == "PASS" for row in results) else "FAIL",
               "source": manifest, "bridge_sha256": hashlib.sha256(bridge.read_bytes()).hexdigest(),
               "results": results,
               "missing_production_proofs": ["admitted_workload_transport", "physical_lease_epoch_and_revoke", "bridge_restart_recovery",
                                              "authoritative_result_graph", "worker_receipt", "nvidia_container_lane"]}
    (state / "summary.json").write_text(json.dumps(summary, sort_keys=True, indent=2) + "\n")
    return summary
