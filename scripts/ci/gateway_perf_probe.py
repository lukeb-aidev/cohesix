#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Measure gateway authority latency, exact refusals, queue/cache state and audit emission cost.
# Copyright 2026 Lukas Bower

"""Host-only gateway microbenchmark; it cannot provide target throughput proof."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import secrets
import socket
import subprocess
import time
from typing import Any
import urllib.error
import urllib.request

MAX_RESPONSE_BYTES = 1024 * 1024


def writer_epochs(status: dict[str, Any]) -> tuple[int, int]:
    """Require a real older writer to measure the stale-fence path."""
    epoch = status.get("authority", {}).get("writer_epoch")
    if type(epoch) is not int or not 2 <= epoch <= 2**64 - 1:
        raise ValueError("stale-writer probe requires a gateway compiled with writer_epoch >= 2")
    return epoch, epoch - 1


def summarize(samples: list[dict[str, Any]]) -> dict[str, Any]:
    """Use nearest-rank percentiles and retain every unexpected status."""
    values = sorted(row["elapsed_ms"] for row in samples)
    if not values or any(not math.isfinite(value) or value < 0 for value in values):
        raise ValueError("latency samples must be finite and nonnegative")
    return {
        "requests": len(samples),
        "p50_ms": values[math.ceil(len(values) * .50) - 1],
        "p95_ms": values[math.ceil(len(values) * .95) - 1],
        "errors": sum(row["status"] != row["expected_status"] or not row.get("protocol_ok", True) for row in samples),
        "refusals": sum(row["status"] >= 400 for row in samples),
        "backpressure": sum(row["status"] == 429 for row in samples),
    }


def request(url: str, token: str, ticket: str | None, body: dict | None = None) -> tuple[int, dict]:
    """Perform one bounded HTTP operation, including refusal bodies, without retry."""
    headers = {"Authorization": f"Bearer {token}"}
    data = None
    if ticket is not None:
        headers["x-cohesix-ticket"] = ticket
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body, separators=(",", ":")).encode()
    req = urllib.request.Request(url, data=data, headers=headers)
    try:
        response = urllib.request.urlopen(req, timeout=5)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        payload = response.read(MAX_RESPONSE_BYTES + 1)
        if len(payload) > MAX_RESPONSE_BYTES:
            raise ValueError("gateway response exceeded byte bound")
        decoded = json.loads(payload)
        if not isinstance(decoded, dict):
            raise ValueError("gateway response must be an object")
        return response.code, decoded


def run_lane(binary: Path, shell: Path, out: Path, count: int, delegated: bool) -> dict:
    """Start one isolated mock gateway and record raw samples beside its audit log."""
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    token, issuer = secrets.token_hex(24), secrets.token_hex(32)
    env = os.environ.copy()
    env.update({"HIVE_GATEWAY_REQUEST_AUTH_TOKEN": token, "M27A_BENCH_ISSUER": issuer, "RUST_LOG": "info"})
    # Inherited production or operator credentials cannot influence this fixture.
    for key in ("HIVE_GATEWAY_TICKET", "COH_REST_TICKET", "HIVE_GATEWAY_DELEGATION_KEY_REF"):
        env.pop(key, None)
    args = [str(binary), "--mock", "--bind", f"127.0.0.1:{port}"]
    ticket = None
    if delegated:
        args.extend(["--delegation-key-ref", "env:M27A_BENCH_ISSUER"])
        minted = subprocess.run([str(shell), "--mint-ticket", "--role", "queen", "--ticket-subject", "probe", "--ticket-secret", "env:M27A_BENCH_ISSUER", "--ticket-write-scope", "/queen", "--ticket-ttl-s", "300", "--ticket-ops", str(count * 8)], env=env, capture_output=True, text=True, check=True)
        ticket = minted.stdout.strip()
    out.mkdir(parents=True, exist_ok=True)
    samples: dict[str, list[dict]] = {}
    base = f"http://127.0.0.1:{port}"
    with (out / "gateway.log").open("wb") as log:
        process = subprocess.Popen(args, env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 20
            while True:
                if process.poll() is not None:
                    raise RuntimeError(f"gateway exited during startup; see {out / 'gateway.log'}")
                try:
                    _, state = request(base + "/v1/meta/status", token, None)
                    if state.get("connected"):
                        break
                except (OSError, urllib.error.URLError):
                    pass
                if time.monotonic() >= deadline:
                    raise RuntimeError("gateway did not become connected before readiness deadline")
                time.sleep(.05)  # Readiness only; measured operations never retry.
            initial = state
            current_epoch, stale_epoch = writer_epochs(state) if delegated else (None, None)

            def measure(name: str, path: str, body: dict | None, expected: int, credential: str | None = ticket) -> dict:
                start = time.perf_counter_ns()
                code, result = request(base + path, token, credential, body)
                row = {"elapsed_ms": (time.perf_counter_ns() - start) / 1_000_000, "status": code, "expected_status": expected}
                if body is not None:
                    row["protocol_ok"] = result.get("status") == ("OK" if expected == 200 else "ERR") and result.get("end") is True
                if result.get("error"):
                    row["refusal"] = result["error"]
                samples.setdefault(name, []).append(row)
                return result

            for index in range(count):
                measure("status", "/v1/meta/status", None, 200)
                command = json.dumps({"spawn": "heartbeat", "ticks": 3}, separators=(",", ":"))
                if not delegated:
                    measure("legacy_write", "/v1/fs/echo", {"path": "/queen/ctl", "line": command}, 200)
                    continue
                envelope = {"schema": "queen-intent/v1", "id": f"intent-{index}", "idempotency_key": f"key-{index}", "issued_unix_ms": index + 1, "writer_epoch": current_epoch, "cmd": command}
                body = {"path": "/queen/intents/ctl", "line": json.dumps(envelope, separators=(",", ":"))}
                measure("delegated_write", "/v1/fs/echo", body, 200)
                measure("duplicate_ack", "/v1/fs/echo", body, 200)
                envelope["cmd"] = '{"spawn":"heartbeat","ticks":4}'
                body["line"] = json.dumps(envelope, separators=(",", ":"))
                measure("idempotency_conflict", "/v1/fs/echo", body, 403)
                envelope["writer_epoch"] = stale_epoch
                body["line"] = json.dumps(envelope, separators=(",", ":"))
                measure("stale_writer", "/v1/fs/echo", body, 403)
                measure("missing_delegation", "/v1/fs/echo", body, 403, None)
            _, final = request(base + "/v1/meta/status", token, None)
            result = {"schema": "gateway-authority-lane/v1", "backend": "host-model", "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "client_retries": 0, "initial_status": initial, "final_status": final, "scenarios": {name: summarize(rows) for name, rows in samples.items()}, "samples": samples}
            (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
            return result
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", choices=["delegated-rest-authority"], required=True)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--gateway", type=Path, default=Path("target/debug/hive-gateway"))
    parser.add_argument("--cohsh", type=Path, default=Path("target/debug/cohsh"))
    parser.add_argument("--baseline-gateway", type=Path)
    parser.add_argument("--iterations", type=int, default=32)
    args = parser.parse_args()
    if not 1 <= args.iterations <= 64:
        parser.error("iterations must be 1..64 to fit the default strict intent table")
    state_dir = args.state_dir.resolve()
    if state_dir.exists() and any(state_dir.iterdir()):
        parser.error("state-dir must be new or empty; retain prior evidence")
    state_dir.mkdir(parents=True, exist_ok=True)
    report: dict[str, Any] = {"schema": "gateway-authority-benchmark/v1", "scenario": args.scenario, "claim": "host-gateway-microbenchmark-only", "target_proof": "none", "rolling_26d_comparison": "requires separately retained equivalent status-read evidence"}
    try:
        current = run_lane(args.gateway.resolve(strict=True), args.cohsh.resolve(strict=True), state_dir / "current", args.iterations, True)
        report["current"] = current
        if args.baseline_gateway is not None:
            baseline = run_lane(args.baseline_gateway.resolve(strict=True), args.cohsh.resolve(strict=True), state_dir / "pre-27a", args.iterations, False)
            report["pre_27a"] = baseline
            report["comparison"] = {"status_p95_delta_ms": current["scenarios"]["status"]["p95_ms"] - baseline["scenarios"]["status"]["p95_ms"], "write_p95_delta_ms": current["scenarios"]["delegated_write"]["p95_ms"] - baseline["scenarios"]["legacy_write"]["p95_ms"]}
        report["verdict"] = "PASS" if all(row["errors"] == 0 for lane in (report.get("current", {}), report.get("pre_27a", {})) for row in lane.get("scenarios", {}).values()) else "FAIL"
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        report.update(verdict="FAIL", error=str(error))
    finally:
        (state_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"gateway authority {report['verdict']}: {state_dir / 'report.json'}")
    return 0 if report["verdict"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
