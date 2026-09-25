#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Bind selected A2A task recovery to exact-source CUDA and PEFT native results.
# Copyright 2026 Lukas Bower
"""Accept only real A2A delegation reconciled with the original target result."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import subprocess
from typing import Any
from urllib.parse import urlparse

from cohesix.auth import resolve_secret_reference
from cohesix.backends import RestBackend
from provider_m28_live import qemu_image_identity, read_artifact
from provider_matrix import require


SCHEMA = "cohesix-m28e-live-reference/v1"
EVIDENCE = {
    "cuda_task", "cuda_restart", "cuda_recover", "cuda_verified",
    "peft_task", "peft_restart", "peft_recover", "peft_deployment",
    "peft_report", "python_parity", "nat_probe", "refusals",
}


def document(path: Path, maximum: int = 131072) -> dict[str, Any]:
    """Read one bounded regular JSON record without following a symlink."""
    value = json.loads(read_artifact(path, maximum))
    require(isinstance(value, dict), f"M28e evidence object required: {path.name}")
    return value


def sha(path: Path, maximum: int = 64 * 1024 * 1024) -> str:
    """Hash a bounded local source, image or package artifact."""
    return hashlib.sha256(read_artifact(path, maximum)).hexdigest()


def task(path: Path, expected: str) -> dict[str, Any]:
    """Require the official SDK's parsed task to retain the native identity."""
    evidence = document(path)
    require(evidence["sdk"] == "a2a-sdk==0.3.26", "M28e pinned SDK")
    value = evidence["task"]
    metadata = value["metadata"]
    require(value["kind"] == "task" and value["id"] == expected
            and value["contextId"] == expected
            and metadata["admissionId"] == expected
            and metadata["effectReplayAllowed"] is False
            and metadata["providerVerified"] is False
            and value["status"]["state"] == "completed",
            "M28e A2A task identity or state")
    return value


def recovery(path: Path, admission: str) -> dict[str, Any]:
    """Verify target terminal and signed ledger correlation independently of A2A."""
    result = document(path)
    record = result["record"]
    binding = record["binding"]
    rows = result["target_results"]
    hashes = result["target_result_sha256"]
    require(result["effect_replay_allowed"] is False
            and binding["admission_id"] == admission
            and record["execution"] == "confirmed"
            and record["delivery"] == "acknowledged"
            and re.fullmatch(r"[0-9a-f]{64}", record["result_sha256"])
            and 1 <= len(rows) == len(hashes) <= 16
            and hashes[-1] == record["result_sha256"]
            and rows[-1]["state"] == "succeeded"
            and all(row["id"] == binding["ticket_id"]
                    and row["idempotency_key"] == binding["idempotency_key"]
                    and row["admission"]["admission_id"] == admission
                    for row in rows), "M28e native terminal mismatch")
    return result


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Recheck the retained source, running image, SDK and native outcomes."""
    require(case == "m28e-a2a-live" and host_profile == "jetson-orin-nano-jp7",
            "M28e selected host profile")
    config = document(reference)
    required = {"schema", "source_root", "source_commit", "manifest_sha256",
                "rootserver_sha256", "qemu_pid", "qemu_binary", "qemu_sha256",
                "gateway_binary", "gateway_sha256", "agent_binary", "agent_sha256",
                "coh_binary", "coh_sha256", "gateway_url", "request_auth_ref",
                "delegated_ticket_ref", "evidence"}
    require(set(config) == required and config["schema"] == SCHEMA
            and set(config["evidence"]) == EVIDENCE, "M28e reference fields")
    root = Path(config["source_root"])
    require(root.is_absolute() and root.is_dir()
            and re.fullmatch(r"[0-9a-f]{40}", config["source_commit"])
            and type(config["qemu_pid"]) is int and config["qemu_pid"] > 0,
            "M28e source or PID")
    current = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root,
                                      text=True, timeout=5).strip()
    require(current == config["source_commit"], "M28e source changed")
    paths = {name: Path(value) for name, value in config["evidence"].items()}
    private = root / "out/private/m28e-session"
    require(all(path.is_absolute() and path.is_relative_to(private)
                for name, path in paths.items() if name != "peft_deployment"),
            "M28e private evidence boundary")
    manifest = root / "configs/generated/root_task_resolved.json"
    require(sha(manifest, 2 * 1024 * 1024) == config["manifest_sha256"],
            "M28e selected manifest changed")
    selected = document(manifest, 2 * 1024 * 1024)
    require(selected["gateway"]["agent_protocols"]["enabled"] is True
            and selected["gateway"]["a2a"]["enabled"] is True
            and set(selected["standing_authority"]["actions"]) >= {
                "gpu.workload.submit", "peft.release"},
            "M28e selected A2A and provider controls")
    qemu = Path(config["qemu_binary"])
    require(qemu.is_absolute() and sha(qemu, 256 * 1024 * 1024) == config["qemu_sha256"]
            and Path(f"/proc/{config['qemu_pid']}/exe").resolve() == qemu,
            "M28e pinned KVM QEMU changed")
    image = qemu_image_identity(config["qemu_pid"], current, artifact_root=root)
    require(image["rootserver_sha256"] == config["rootserver_sha256"],
            "M28e target image changed")
    for name in ("gateway", "agent", "coh"):
        package = Path(config[f"{name}_binary"])
        require(package.is_absolute() and sha(package) == config[f"{name}_sha256"],
                f"M28e {name} package changed")
    endpoint = urlparse(config["gateway_url"])
    require(endpoint.scheme == "http" and endpoint.hostname == "127.0.0.1"
            and endpoint.port is not None and not endpoint.path
            and not endpoint.username and not endpoint.password,
            "M28e selected loopback gateway URL")
    backend = RestBackend(config["gateway_url"],
                          request_auth_token=resolve_secret_reference(config["request_auth_ref"]),
                          delegated_ticket=resolve_secret_reference(config["delegated_ticket_ref"]),
                          max_attempts=1)
    boot = backend.read_file("/proc/boot", 8192).decode()
    require(boot.splitlines().count(f"manifest.sha256={config['manifest_sha256']}") == 1,
            "M28e authenticated Queen boot mismatch")
    tasks = {}
    results = {}
    for family in ("cuda", "peft"):
        first_record = document(paths[f"{family}_task"])
        require(first_record["sdk"] == "a2a-sdk==0.3.26",
                "M28e creation client SDK")
        first = first_record["task"]
        admission = first["id"]
        require(re.fullmatch(r"[A-Za-z0-9._-]{1,96}", admission),
                "M28e admission ID")
        final = task(paths[f"{family}_restart"], admission)
        native = recovery(paths[f"{family}_recover"], admission)
        require(first["id"] == final["id"]
                and final["metadata"]["resultSha256"]
                == native["record"]["result_sha256"]
                and final["metadata"]["nativeOutcome"] == "succeeded",
                "M28e reconnect substituted the original result")
        tasks[family] = final
        results[family] = native
    cuda = document(paths["cuda_verified"])
    require(cuda["result"] == "PASS" and cuda["source_commit"] == current
            and cuda["admission_id"] == tasks["cuda"]["id"]
            and cuda["terminal"] == results["cuda"]["target_results"][-1],
            "M28e independent CUDA output evidence")
    verified = json.loads(subprocess.check_output([
        config["coh_binary"], "peft", "release", "--deployment",
        str(paths["peft_deployment"]), "verify"], cwd=root, timeout=30))
    require(verified == document(paths["peft_report"])
            and verified["operation_id"] == tasks["peft"]["id"]
            and verified["result"]["state"] == "succeeded",
            "M28e shared PEFT verification")
    parity = document(paths["python_parity"])
    require(parity["gpu"]["admission_id"] == tasks["cuda"]["id"]
            and parity["peft"]["operation_id"] == tasks["peft"]["id"]
            and parity["peft"]["verified"] is True,
            "M28e Python projection parity")
    nat = document(paths["nat_probe"])
    require(nat["nat_version"] == "1.9.0"
            and nat["sdk_version"] == "0.3.26"
            and nat["card_skill"] == "gpu.workload.submit"
            and nat["lookup_task_id"] == tasks["cuda"]["id"]
            and (nat["cancel_state"] != "canceled" or
                 nat.get("cancel_confirmed_no_effect") is True),
            "M28e NeMo native task lookup or cancellation")
    refusals = document(paths["refusals"])
    require(refusals["cross_subject_denied"] is True
            and refusals["revoked_scope_denied"] is True
            and refusals["budget_refused_no_effect"] is True,
            "M28e refusal boundaries")
    state_dir.mkdir(parents=True, exist_ok=True)
    summary = {"schema":"cohesix-m28e-a2a-live-summary/v1","result":"PASS",
               "proof_class":"live_target","source_commit":current,
               "manifest_sha256":config["manifest_sha256"],"image":image,
               "cuda_admission_id":tasks["cuda"]["id"],
               "peft_admission_id":tasks["peft"]["id"]}
    (state_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    return 0
