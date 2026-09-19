# Author: Lukas Bower
# Purpose: Exercise real CUDA through authenticated local GPU IPC using explicitly fixture admission, retaining cancellation and recovery evidence without Worker claims.
# Copyright 2026 Lukas Bower

"""The native IPC lane tests executor contracts; its admission is explicitly a fixture."""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import signal
from pathlib import Path
import secrets
import socket
import struct
import subprocess
import time
from typing import Any


def encoded(value: Any) -> bytes:
    """Match the declared Rust struct field order for bounded IPC inputs."""
    return json.dumps(value, separators=(",", ":"), allow_nan=False).encode()


def run_workload_ipc(bridge: Path, build: Path, state: Path) -> dict[str, Any]:
    """Prove real GPU output plus auth, replay, revoke, cancellation and restart."""
    state = state.resolve()
    state.mkdir(parents=True, exist_ok=False, mode=0o700)
    bridge = bridge.resolve(strict=True)
    helper = (build / "cohesix-cuda-reference").resolve(strict=True)
    manifest = json.loads((build / "build.json").read_bytes())
    helper_hash = hashlib.sha256(helper.read_bytes()).hexdigest()
    if helper_hash != manifest["binary_sha256"]:
        raise ValueError("helper digest mismatch")
    socket_path = state / "executor.sock"
    # Linux sockaddr_un is bounded to 108 bytes including its terminator.
    if len(str(socket_path).encode()) >= 108:
        raise ValueError("executor socket path exceeds platform bound")
    token = secrets.token_hex(32).encode()
    key_path = state / "credential.private"
    with key_path.open("xb") as stream:
        stream.write(token)
    key_path.chmod(0o600)
    common = [str(bridge), "--reference-helper", str(helper),
              "--reference-helper-sha256", helper_hash]
    sequence = 0
    server = None
    results: list[dict[str, Any]] = []
    config_path = state / "config.json"
    output = (state / "bridge.stdout").open("xb")
    errors = (state / "bridge.stderr").open("xb")

    def inventory(name: str) -> dict[str, Any]:
        destination = state / ("inventory-" + name)
        result = subprocess.run(common + ["--reference-inventory", "--reference-state", str(destination)],
                                capture_output=True, timeout=12, check=True)
        if len(result.stdout) > 16384 or len(result.stderr) > 16384:
            raise ValueError("native inventory capture bound")
        return json.loads(result.stdout)

    def start() -> subprocess.Popen:
        if socket_path.exists():
            socket_path.unlink()
        process = subprocess.Popen([str(bridge), "--workload-config", str(config_path)],
                                   stdin=subprocess.DEVNULL, stdout=output, stderr=errors)
        deadline = time.monotonic() + 5
        while not socket_path.exists():
            if process.poll() is not None or time.monotonic() >= deadline:
                raise ValueError("GPU bridge failed to start; see bridge.stderr")
            time.sleep(0.02)
        return process

    def send(command: dict[str, Any], *, bad_key: bool = False) -> dict[str, Any]:
        payload = {"schema": "cohesix-gpu-local/v1", "issued_unix_ms": time.time_ns() // 1_000_000,
                   "command": command}
        signing_key = b"incorrect-native-conformance-key" if bad_key else token
        envelope = {"payload": payload, "mac": hmac.new(signing_key, encoded(payload), hashlib.sha256).hexdigest()}
        data = encoded(envelope)
        if len(data) > 16384:
            raise ValueError("request frame limit")
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(0.5)
            connection.connect(str(socket_path))
            connection.sendall(len(data).to_bytes(4, "big") + data)
            deadline = time.monotonic() + 0.5

            def read(size: int) -> bytes:
                buffer = bytearray()
                while len(buffer) < size:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise ValueError("response deadline")
                    connection.settimeout(remaining)
                    chunk = connection.recv(size - len(buffer))
                    if not chunk:
                        raise ValueError("response truncated")
                    buffer.extend(chunk)
                return bytes(buffer)

            length = int.from_bytes(read(4), "big")
            if not 0 < length <= 16384:
                raise ValueError("response frame limit")
            signed = json.loads(read(length))
        response_bytes = json.dumps(signed["response"], sort_keys=True, separators=(",", ":")).encode()
        expected = hmac.new(token, response_bytes, hashlib.sha256).hexdigest()
        if not hmac.compare_digest(expected, signed["mac"]):
            raise ValueError("response authentication")
        result = signed["response"]
        if result["request_sha256"] != hashlib.sha256(data).hexdigest():
            raise ValueError("response request binding")
        return result

    def make_request(name: str, entry: str = "vadd", long: bool = False) -> tuple[dict, dict]:
        nonlocal sequence
        sequence += 1
        observed = inventory(name)
        now = time.time_ns() // 1_000_000
        ticket = "native-ipc-" + name
        binding = {"ticket_id": ticket, "idempotency_key": "once-" + name, "action": "gpu.workload.submit",
                   "operation_id": ticket, "gpu_id": "GPU-0", "worker_id": "fixture-worker-gpu-1",
                   "worker_slot": 0, "lease_epoch": 1, "supervisor_generation": 2, "cap_generation": 3,
                   "admission_sequence": sequence, "writer_epoch": 1, "expires_unix_ms": now + 60000,
                   "provider_graph_sha256": observed["provider_graph_sha256"]}
        dimension = 128 if long else (8 if entry == "matmul" else 64)
        values = ([dimension * (i // dimension + 1) * (i % dimension + 1)
                   for i in range(dimension * dimension)] if entry == "matmul"
                  else [3 * (i % 1024) for i in range(dimension)])
        output_bytes = b"".join(struct.pack("<f", value) for value in values)
        request = {"schema": "cohesix-cuda-reference-request/v1", "ticket_id": ticket, "entrypoint": entry,
                   "dimension": dimension, "iterations": 10000 if long else 1, "device_ordinal": 0,
                   "device_uuid": observed["native"]["device_uuid"], "inventory_observed_unix_ms": observed["observed_unix_ms"],
                   "provider_graph_sha256": observed["provider_graph_sha256"], "memory_budget_bytes": 1048576,
                   "deadline_ms": 30000}
        value = {"schema": "cohesix-gpu-workload-input/v1", "artifact_sha256": helper_hash,
                 "topology_sha256": observed["topology_sha256"],
                 "expected_output_sha256": hashlib.sha256(output_bytes).hexdigest(), "request": request}
        command = {"op": "submit", "binding": binding, "lease_id": "fixture-lease-1", "lease_sequence": 1,
                   "request_sha256": hashlib.sha256(encoded(value)).hexdigest(), "input": value}
        return binding, command

    def observe(binding: dict, *, started: bool = False, renew: bool = True, pause: bool = False) -> dict:
        deadline = time.monotonic() + 35
        serial = 0
        marker = state / "jobs" / binding["ticket_id"] / "execution" / "started.json"
        last_poll = 0.0
        while time.monotonic() < deadline:
            if started and marker.exists():
                native = json.loads(marker.read_bytes())
                if native["completed_iterations"] != 1:
                    raise ValueError("native start marker")
                if pause:
                    pause_owned_child(binding)
                return native
            if started and time.monotonic() - last_poll < 0.1:
                time.sleep(0.001)
                continue
            last_poll = time.monotonic()
            if renew:
                serial = time.time_ns() // 1_000_000
                reply = send({"op": "renew", "binding": binding, "lease_id": "fixture-lease-1",
                              "lease_sequence": 1, "serial": serial})
                if not reply["ok"] and reply["code"] != "stale_lease_heartbeat":
                    raise ValueError(reply)
            reply = send({"op": "status", "binding": binding})
            if not reply["ok"]:
                raise ValueError(reply)
            job = reply["job"]
            if job["terminal_unix_ms"] is not None:
                if started:
                    raise ValueError("workload ended before first kernel: " + job["detail"])
                return job
            time.sleep(0.001 if started else 0.05)
        raise ValueError("native workload terminal timeout")

    def pause_owned_child(binding: dict) -> None:
        """Inject a stalled consumer after a native kernel, without extending production bounds."""
        expected = state / "jobs" / binding["ticket_id"] / "execution" / "executor"
        children: set[int] = set()
        for task in list(Path(f"/proc/{server.pid}/task").iterdir())[:128]:
            try:
                children.update(int(pid) for pid in (task / "children").read_text().split())
            except FileNotFoundError:
                continue
        if not children:
            # Jetson kernels can omit /proc/PID/task/TID/children. The fallback
            # inspects only bounded same-UID process metadata, never argv/env.
            scanned = 0
            for process in Path("/proc").iterdir():
                if not process.name.isdigit():
                    continue
                scanned += 1
                if scanned > 32768:
                    raise ValueError("native process inventory exceeds bound")
                try:
                    if process.stat().st_uid != os.getuid():
                        continue
                    with (process / "status").open() as metadata:
                        status = metadata.read(8193)
                    if len(status) > 8192:
                        raise ValueError("native process metadata exceeds bound")
                    parent = next((line.split()[1] for line in status.splitlines()
                                   if line.startswith("PPid:")), None)
                    if parent == str(server.pid):
                        children.add(int(process.name))
                except (FileNotFoundError, ProcessLookupError):
                    continue
        owned = []
        for pid in children:
            try:
                if Path(f"/proc/{pid}/exe").resolve(strict=True) == expected:
                    owned.append(pid)
            except FileNotFoundError:
                continue
        if len(owned) != 1:
            raise ValueError(f"exact native executor child unavailable for stall injection: children={children}, expected={expected}")
        os.kill(owned[0], signal.SIGSTOP)

    try:
        first = inventory("config")
        config = {"schema": "cohesix-gpu-executor-config/v1", "socket": str(socket_path),
                  "state_root": str(state / "jobs"), "helper": str(helper), "helper_sha256": helper_hash,
                  "credential_ref": "file:" + str(key_path), "writer_epoch": 1, "gpu_id": "GPU-0",
                  "device_uuid": first["native"]["device_uuid"], "provider_graph_sha256": first["provider_graph_sha256"]}
        config_path.write_bytes(encoded(config))
        server = start()
        for name in ["vadd", "matmul", "output-mismatch", "cancel", "revoke", "heartbeat-expiry", "restart"]:
            binding, command = make_request(name, "matmul" if name != "vadd" else "vadd", name in {"cancel", "revoke", "heartbeat-expiry", "restart"})
            if name == "vadd":
                rejected = send(command, bad_key=True)
                if rejected.get("code") != "unauthorized_local_message":
                    raise ValueError("wrong key was not refused")
                results.append({"case": "wrong-key", "result": "PASS"})
            if name == "output-mismatch":
                command["input"]["expected_output_sha256"] = "a" * 64
                command["request_sha256"] = hashlib.sha256(encoded(command["input"])).hexdigest()
            reply = send(command)
            if not reply["ok"]:
                raise ValueError(reply)
            if name in {"cancel", "revoke", "heartbeat-expiry", "restart"}:
                observe(binding, started=True, pause=name == "heartbeat-expiry")
                if name == "cancel":
                    control = dict(binding)
                    control["ticket_id"] = "cancel-" + name
                    control["idempotency_key"] = "cancel-once"
                    control["action"] = "gpu.workload.cancel"
                    control["admission_sequence"] += 100
                    result = send({"op": "cancel", "binding": control, "job_id": binding["ticket_id"]})
                    if not result["ok"]:
                        raise ValueError(result)
                elif name == "revoke":
                    result = send({"op": "revoke", "binding": binding})
                    if not result["ok"]:
                        raise ValueError(result)
                elif name == "restart":
                    server.kill()
                    server.wait(timeout=5)
                    server = start()
                job = observe(binding, renew=False)
            else:
                job = observe(binding)
            expected_state = {"vadd": "succeeded", "matmul": "succeeded", "output-mismatch": "failed",
                              "cancel": "cancelled", "revoke": "revoked", "heartbeat-expiry": "revoked",
                              "restart": "interrupted"}[name]
            if job["state"] != expected_state:
                raise ValueError(f"{name}: expected {expected_state}, got {job['state']}: {job['detail']}")
            # Identical submit bytes return the retained terminal; they cannot run another kernel.
            duplicate = send(command)
            if not duplicate["ok"] or duplicate["job"] != job:
                raise ValueError("duplicate terminal changed")
            (state / (name + ".json")).write_bytes(encoded(job))
            results.append({"case": name, "result": "PASS", "native_state": job["state"], "detail": job["detail"],
                            "fault_injection": "owned_child_SIGSTOP_after_first_kernel" if name == "heartbeat-expiry" else "none"})
        summary = {"schema": "cohesix-gpu-ipc-conformance/v1", "reference_result": "PASS", "result": "OBSERVED",
                   "authoritative": False, "admission_source": "fixture", "worker_proof": False,
                   "proof_class": "native_provider_operation", "profile_id": "jetson-orin-nano-jp7",
                   "provider_graph_sha256": first["provider_graph_sha256"], "helper_sha256": helper_hash,
                   "bridge_sha256": hashlib.sha256(bridge.read_bytes()).hexdigest(), "results": results}
        (state / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
        return summary
    finally:
        if server is not None and server.poll() is None:
            server.terminate()
            server.wait(timeout=5)
        output.close()
        errors.close()
        key_path.unlink(missing_ok=True)
