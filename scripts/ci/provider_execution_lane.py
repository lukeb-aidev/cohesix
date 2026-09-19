# Author: Lukas Bower
# Purpose: Exercise the fixed CUDA reference under a hardened unit or pinned NVIDIA container and retain native lifecycle observations.
# Copyright 2026 Lukas Bower

"""Native deployment observations do not confer ticket or Worker authority."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import time
from typing import Any

from cohesix.native_providers import bounded_command


def docker_events(raw: bytes, identity: str) -> list[dict[str, Any]]:
    """Use the Engine Actor.ID schema and withhold unselected event attributes."""
    if len(raw) > 65_536:
        raise ValueError("Docker event byte bound exceeded")
    lines = raw.splitlines()
    if len(lines) > 128:
        raise ValueError("Docker event count bound exceeded")
    retained = []
    for line in lines:
        row = json.loads(line)
        if row.get("Type") != "container" or row.get("Actor", {}).get("ID") != identity:
            raise ValueError("Docker event native identity mismatch")
        if row.get("Action") in {"create", "start", "die", "destroy", "kill", "oom", "restart", "stop"}:
            stamp = row.get("timeNano")
            if type(stamp) is not int or stamp <= 0:
                raise ValueError("Docker event time missing")
            retained.append({"action": row["Action"], "container_id": identity, "time_nano": stamp})
    return retained


def run_lane(lane: str, bridge: Path, build: Path, state: Path, *,
             user: str, sidecar: Path | None, image: str | None) -> dict[str, Any]:
    """Launch only this repository's finite reference; no arbitrary argv or code."""
    if re.fullmatch(r"[a-z_][a-z0-9_-]{0,31}", user) is None:
        raise ValueError("invalid execution user")
    account = pwd.getpwnam(user)
    if account.pw_uid == 0:
        raise ValueError("reference workload must use an unprivileged uid")
    root = Path(__file__).resolve().parents[2]
    bridge, build, state = bridge.resolve(strict=True), build.resolve(strict=True), state.resolve()
    state.mkdir(parents=True, mode=0o700)
    if os.geteuid() == 0:
        os.chown(state, account.pw_uid, account.pw_gid)
    elif os.geteuid() != account.pw_uid:
        raise ValueError("execution uid differs from state owner")
    name = f"cohesix-conformance-m27b-{lane}-{time.time_ns()}"
    runner = root / "scripts/ci/provider_conformance_run.sh"
    arguments = [str(runner), "--provider", "gpu.workload", "--live-reference",
                 "--gpu-bridge", str(bridge), "--cuda-build", str(build),
                 "--state-dir", str(state / "run")]

    def command(args: list[str], timeout: float = 10) -> bytes:
        executable = Path("/usr/bin") / args[0]
        return bounded_command([str(executable), *args[1:]], timeout_s=timeout)

    observations: dict[str, Any] = {}
    if lane == "systemd":
        if os.geteuid() != 0 or sidecar is None:
            raise ValueError("systemd lane requires an authorized root launcher and --sidecar-bridge")
        sidecar = sidecar.resolve(strict=True)
        unit = name + ".service"
        properties = [f"User={user}", f"Group={account.pw_gid}", "Type=oneshot",
                      "RemainAfterExit=yes", "SuccessExitStatus=2", "NoNewPrivileges=yes",
                      "PrivateTmp=yes", "ProtectSystem=strict", "ProtectHome=yes",
                      "CapabilityBoundingSet=", "RestrictAddressFamilies=AF_UNIX",
                      "MemoryMax=1G", "CPUQuota=200%", "TasksMax=64",
                      "TimeoutStartSec=60", "RuntimeMaxSec=90",
                      f"ReadWritePaths={state}", "Environment=PYTHONDONTWRITEBYTECODE=1",
                      "StandardOutput=journal", "StandardError=journal", f"WorkingDirectory={root}"]
        launch = ["systemd-run", "--no-block", f"--unit={unit}"]
        launch.extend(f"--property={value}" for value in properties)
        started = int(time.time())
        try:
            (state / "dispatch.log").write_bytes(command(launch + arguments))
            deadline = time.monotonic() + 60
            while True:
                raw = bounded_command([str(sidecar), "--native-systemd-unit", unit])
                observed = json.loads(raw)
                data = observed["data"]
                (state / "unit.json").write_bytes(raw)
                if data["sub_state"] in ("exited", "failed") and data["job_id"] == 0:
                    break
                if time.monotonic() >= deadline:
                    raise ValueError("systemd reference deadline exceeded")
                time.sleep(0.1)
            if data["service_result"] != "success" or data["sub_state"] != "exited" or not data["invocation_id"]:
                raise ValueError("systemd reference terminal state failed")
            journal = command(["journalctl", f"--since=@{started}", f"--until=@{int(time.time())+1}",
                               f"_SYSTEMD_INVOCATION_ID={data['invocation_id']}", "--no-pager", "--output=json",
                               "--output-fields=_SYSTEMD_UNIT,_SYSTEMD_INVOCATION_ID,__CURSOR,__REALTIME_TIMESTAMP,MESSAGE",
                               "--lines=32"])
            rows = [json.loads(line) for line in journal.splitlines()]
            if not rows or any(row.get("_SYSTEMD_INVOCATION_ID") != data["invocation_id"] for row in rows):
                raise ValueError("missing exact invocation journal correlation")
            (state / "journal.jsonl").write_bytes(journal)
            observations = {"unit": data, "properties": properties, "journal_sha256": hashlib.sha256(journal).hexdigest(),
                            "cursor_start": rows[0]["__CURSOR"], "cursor_end": rows[-1]["__CURSOR"]}
        finally:
            command(["systemctl", "stop", unit])
    elif lane == "nvidia-container":
        if image is None or re.fullmatch(r"[a-zA-Z0-9./_-]+@sha256:[0-9a-f]{64}", image) is None:
            raise ValueError("NVIDIA image must have an immutable SHA-256 digest")
        launch = ["docker", "create", "--name", name, "--runtime=nvidia", "--network=none",
                  "--read-only", "--user", f"{account.pw_uid}:{account.pw_gid}", "--cap-drop=ALL",
                  "--security-opt=no-new-privileges", "--memory=1g", "--memory-swap=1g",
                  "--cpus=2", "--pids-limit=64", "--tmpfs", "/tmp:rw,nosuid,nodev,size=16m",
                  "-e", "NVIDIA_VISIBLE_DEVICES=all", "-e", "NVIDIA_DRIVER_CAPABILITIES=compute,utility",
                  "-e", "PYTHONDONTWRITEBYTECODE=1", "--label", f"io.cohesix.evidence={name}",
                  "-v", f"{root}:/source:ro", "-v", f"{bridge}:/bridge:ro", "-v", f"{build}:/build:ro",
                  "-v", f"{state}:/evidence:rw", "--entrypoint", "/bin/bash", image,
                  "/source/scripts/ci/provider_conformance_run.sh", "--provider", "gpu.workload", "--live-reference",
                  "--gpu-bridge", "/bridge", "--cuda-build", "/build", "--state-dir", "/evidence/run"]
        started = int(time.time())
        identity = command(launch, 30).decode().strip()
        if re.fullmatch(r"[0-9a-f]{64}", identity) is None:
            raise ValueError("invalid created container identity")
        (state / "container-identity.json").write_text(json.dumps({"container_id": identity, "image_reference": image}) + "\n")
        try:
            command(["docker", "start", identity])
            code = command(["docker", "wait", identity], 30).decode().strip()
            inspect = json.loads(command(["docker", "inspect", identity]))[0]
            if inspect["Id"] != identity or code != "2" or inspect["State"]["Running"] or inspect["State"]["OOMKilled"]:
                raise ValueError("container reference terminal state failed")
            logs = command(["docker", "logs", "--tail=32", identity])
            (state / "container.log").write_bytes(logs)
            raw_events = command(["docker", "events", "--since", str(started), "--until", str(int(time.time())+1),
                                  "--filter", f"container={identity}", "--format", "{{json .}}"])
            events = docker_events(raw_events, identity)
            if not any(row["action"] == "die" for row in events):
                raise ValueError("container terminal event missing")
            (state / "events.json").write_text(json.dumps(events, sort_keys=True) + "\n")
            observations = {"container_id": identity, "image_digest": inspect["Image"], "image_reference": image,
                            "state": inspect["State"], "log_sha256": hashlib.sha256(logs).hexdigest(), "events": events}
        finally:
            command(["docker", "rm", "-f", identity])
    else:
        raise ValueError("unsupported execution lane")
    with (state / "run/summary.json").open("rb") as stream:
        raw = stream.read(65_537)
    if len(raw) > 65_536:
        raise ValueError("reference summary bound exceeded")
    reference = json.loads(raw)
    result = {"schema": "cohesix-cuda-deployment-observation/v1", "lane": lane,
              "authoritative": False, "worker_proof": False, "production_proven": False,
              "reference_result": reference["reference_result"], "reference_sha256": hashlib.sha256(raw).hexdigest(),
              "source": reference["source"], "native": observations}
    (state / "summary.json").write_text(json.dumps(result, sort_keys=True, indent=2) + "\n")
    return result
