#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check installed-wheel public APIs and selected authority without claiming target execution.
# Copyright 2026 Lukas Bower

import hashlib
import json
import os
import platform
import subprocess
import sys
from importlib import metadata
from pathlib import Path

contract_path = Path(sys.argv[1])
expected_target = sys.argv[2]
output = Path(sys.argv[3])
mock_root = Path(sys.argv[4])
import cohesix
from cohesix import CohesixClient, CohesixError, MockBackend, load_profile_contract
from cohesix.authority import QueenIntent
from cohesix.generated import DEFAULTS
from cohesix.integrations import probe_peft_runtime

required_exports = {
    "CohesixClient",
    "MockBackend",
    "TargetProfileContract",
    "WorkerClient",
    "WorkerIdentity",
    "WorkerObservation",
    "WorkerReceipt",
    "load_profile_contract",
    "parse_receipt",
}
if not required_exports.issubset(set(cohesix.__all__)):
    raise SystemExit("python-compat: installed wheel public API is incomplete")
if DEFAULTS.get("manifest_sha256") is not None or DEFAULTS.get("execution_proof") != "none":
    raise SystemExit("python-compat: installed wheel defaults are not target-neutral")
contract = load_profile_contract(contract_path, expected_target=expected_target)
strict = not contract.authority["legacy_queen_ctl"]
for role, worker_id in (("heartbeat", "smoke-heart"), ("gpu", "smoke-gpu"), ("lora", "smoke-lora")):
    worker_root = mock_root / worker_id
    client = CohesixClient(MockBackend(str(worker_root)), profile_contract=contract)
    if strict:
        try:
            client.worker_spawn(role, worker_id)
        except CohesixError as error:
            if str(error) != "strict Queen intent identity is required by selected profile":
                raise
        else:
            raise SystemExit("python-compat: production spawn omitted its strict identity")
        command = {"spawn": role, "worker_id": worker_id, "slot": 0}
        intent = QueenIntent(
            worker_id, worker_id + "-retry", 1000, command,
            writer_epoch=contract.authority["writer_epoch"],
        )
        admitted = client.workers.spawn(role, worker_id, intent=intent)
        retained = (worker_root / "queen/intents/ctl").read_bytes()
        envelope = json.loads(retained)
        if (
            not admitted.request_admitted or admitted.lifecycle != "queued"
            or admitted.bytes_written != len(retained)
            or envelope["schema"] != "queen-intent/v1"
            or envelope["id"] != worker_id
            or envelope["idempotency_key"] != worker_id + "-retry"
            or envelope["issued_unix_ms"] != 1000
            or envelope["writer_epoch"] != contract.authority["writer_epoch"]
            or json.loads(envelope["cmd"]) != command
            or (worker_root / "queen/ctl").exists()
        ):
            raise SystemExit("python-compat: strict request serialization drift")
        # MockBackend retains strict bytes but does not execute Queen intents.
        # Readiness, teardown and target execution require their live lanes.
        continue
    if not client.worker_spawn(role, worker_id).request_admitted:
        raise SystemExit("python-compat: mock spawn admission failed")
    observation = client.worker_wait_ready(role, worker_id, timeout_s=0.2)
    if observation.state.execution_proof != "host-model":
        raise SystemExit("python-compat: mock observation proof class widened")
    client.worker_teardown(role, worker_id)
try:
    client.worker_spawn("bus", "smoke-bus")
except CohesixError:
    pass
else:
    raise SystemExit("python-compat: WorkerBus spawn was not refused")
probe = probe_peft_runtime()
if probe.status == "ok" and any(value is None for value in probe.data["versions"].values()):
    raise SystemExit("python-compat: missing optional runtime reported ready")
entry = subprocess.run(
    [sys.executable, "-m", "cohesix.playbook_cli", "--list"],
    check=False,
    capture_output=True,
    text=True,
)
if entry.returncode != 0 or "mac-release-factory" not in entry.stdout:
    raise SystemExit("python-compat: public playbook entry point smoke failed")
record = {
    "schema": "cohesix-python-smoke/v1",
    "implementation": platform.python_implementation(),
    "python_version": platform.python_version(),
    "platform": platform.platform(),
    "package_version": metadata.version("cohesix"),
    "profile_contract_sha256": contract.contract_sha256,
    "target": expected_target,
    "target_defaults": "neutral",
    "optional_peft_status": probe.status,
    "worker_roles": ["worker-gpu", "worker-heartbeat", "worker-lora"],
    "worker_control_proof": (
        "strict-request-serialization" if strict else "compatibility-host-model-lifecycle"
    ),
    "worker_bus": "model-only-refused",
    "result": "PASS",
}
tmp = output.with_suffix(output.suffix + ".partial")
tmp.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
os.replace(tmp, output)
