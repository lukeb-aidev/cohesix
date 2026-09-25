#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check exact-source M28d MCP CUDA and PEFT evidence, recovery, and shared refusal boundaries.
# Copyright 2026 Lukas Bower
"""Inspect retained real effects and reverify the shared PEFT graph on the selected host."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import subprocess
from typing import Any

from cohesix.auth import resolve_secret_reference
from cohesix.backends import RestBackend
from provider_m28_live import qemu_image_identity, read_artifact
from provider_matrix import require


SCHEMA = "cohesix-m28d-live-reference/v1"
EVIDENCE_KEYS = {
    "cuda_recover", "cuda_recover_restart", "cuda_recover_final",
    "cuda_verified", "peft_recover", "peft_recover_restart",
    "peft_recover_final", "peft_deployment", "peft_report",
    "python_parity", "nat_probe", "auth_refusals", "cancel_recover",
    "budget_refusal", "revoke_preflight", "revocation_summary", "ledger_cancel",
    "ledger_budget_revoke", "native_accepted",
}


def document(path: Path, maximum: int = 65536) -> dict[str, Any]:
    """Read one bounded regular JSON artifact without following a file symlink."""
    value = json.loads(read_artifact(path, maximum))
    require(isinstance(value, dict), f"M28d object required: {path.name}")
    return value


def sha(path: Path, maximum: int = 64 * 1024 * 1024) -> str:
    """Hash an exact local artifact within its declared bound."""
    return hashlib.sha256(read_artifact(path, maximum)).hexdigest()


def mcp_result(path: Path, terminal: str) -> dict[str, Any]:
    """Require a named SDK recovery of the original terminal with no effect replay."""
    response = document(path)["response"]
    require(response["isError"] is False, "M28d MCP recovery error")
    result = response["structuredContent"]
    record = result["record"]
    rows = result["target_results"]
    hashes = result["target_result_sha256"]
    binding = record["binding"]
    require(result["effect_replay_allowed"] is False
            and record["execution"] == "confirmed"
            and record["delivery"] == "acknowledged"
            and isinstance(record["result_sha256"], str)
            and re.fullmatch(r"[0-9a-f]{64}", record["result_sha256"])
            and 1 <= len(rows) == len(hashes) <= 16
            and hashes[-1] == record["result_sha256"]
            and rows[-1]["state"] == terminal
            and all(row["id"] == binding["ticket_id"]
                    and row["idempotency_key"] == binding["idempotency_key"]
                    and row["admission"]["admission_id"] == binding["admission_id"]
                    for row in rows), "M28d original target result mismatch")
    return result


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Bind the already executed selected case to live Queen and native evidence."""
    require(case == "m28d-mcp-live" and host_profile == "jetson-orin-nano-jp7",
            "M28d selected host case")
    config = document(reference)
    required = {"schema", "source_root", "source_commit", "manifest_sha256",
                "rootserver_sha256", "qemu_pid", "qemu_binary", "qemu_sha256",
                "gateway_binary", "gateway_sha256", "agent_binary",
                "agent_sha256", "coh_binary", "coh_sha256", "request_auth_ref",
                "delegated_ticket_ref", "evidence"}
    require(set(config) == required and config["schema"] == SCHEMA,
            "M28d selected reference fields")
    require(set(config["evidence"]) == EVIDENCE_KEYS,
            "M28d required evidence paths")
    root = Path(config["source_root"])
    require(root.is_absolute() and root.is_dir()
            and re.fullmatch(r"[0-9a-f]{40}", config["source_commit"])
            and type(config["qemu_pid"]) is int and config["qemu_pid"] > 0,
            "M28d source or process identity")
    paths = {key: Path(value) for key, value in config["evidence"].items()}
    private = root / "out/private/m28d-session"
    require(all(path.is_absolute() and path.is_relative_to(private)
                for key, path in paths.items() if key != "native_accepted")
            and paths["native_accepted"].is_absolute(),
            "M28d private evidence boundary")
    source = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, timeout=5, text=True,
    ).strip()
    require(source == config["source_commit"], "M28d selected source changed")
    qemu = Path(config["qemu_binary"])
    require(qemu.is_absolute() and sha(qemu, 256 * 1024 * 1024) == config["qemu_sha256"]
            and Path(f"/proc/{config['qemu_pid']}/exe").resolve() == qemu,
            "M28d pinned KVM QEMU changed")
    image = qemu_image_identity(config["qemu_pid"], source, artifact_root=root)
    require(image["rootserver_sha256"] == config["rootserver_sha256"],
            "M28d live image changed")
    manifest = root / "configs/generated/root_task_resolved.json"
    require(sha(manifest, 2 * 1024 * 1024) == config["manifest_sha256"],
            "M28d selected manifest changed")
    for name in ("gateway", "agent"):
        path = Path(config[f"{name}_binary"])
        require(path.is_absolute() and sha(path) == config[f"{name}_sha256"],
                f"M28d {name} package changed")
    backend = RestBackend(
        "http://127.0.0.1:8385",
        request_auth_token=resolve_secret_reference(config["request_auth_ref"]),
        delegated_ticket=resolve_secret_reference(config["delegated_ticket_ref"]),
        max_attempts=1,
    )
    boot = backend.read_file("/proc/boot", 8192).decode()
    require(boot.splitlines().count(
        f"manifest.sha256={config['manifest_sha256']}") == 1,
        "M28d authenticated Queen boot mismatch")
    catalogue = document(root / "configs/generated/mcp_catalogue.json")
    controls = document(manifest, 2 * 1024 * 1024)["standing_authority"]
    require(catalogue["enabled"] is True
            and catalogue["revision"] == "2025-11-25"
            and catalogue["transports"] == ["streamable-http", "stdio"]
            and set(controls["actions"])
            == {"gpu.workload.submit", "peft.release", "systemd.restart"},
            "M28d selected catalogue changed; requalify advertised actions")
    cuda = [mcp_result(paths[name], "succeeded") for name in
            ("cuda_recover", "cuda_recover_restart", "cuda_recover_final")]
    peft = [mcp_result(paths[name], "succeeded") for name in
            ("peft_recover", "peft_recover_restart", "peft_recover_final")]
    for sequence in (cuda, peft):
        require(all(row["record"]["binding"] == sequence[0]["record"]["binding"]
                    and row["record"]["result_sha256"]
                    == sequence[0]["record"]["result_sha256"]
                    for row in sequence), "M28d restart substituted original job")
    cuda_verified = document(paths["cuda_verified"], 131072)
    require(cuda_verified["result"] == "PASS"
            and cuda_verified["source_commit"] == source
            and cuda_verified["admission_id"]
            == cuda[0]["record"]["binding"]["admission_id"]
            and cuda_verified["image"] == image
            and cuda_verified["terminal"] == cuda[0]["target_results"][-1],
            "M28d independent CUDA output evidence")
    coh = Path(config["coh_binary"])
    require(coh.is_absolute() and sha(coh) == config["coh_sha256"],
            "M28d shared verifier package changed")
    verified = json.loads(subprocess.check_output(
        [str(coh), "peft", "release", "--deployment",
         str(paths["peft_deployment"]), "verify"],
        cwd=root, timeout=30,
    ))
    retained = document(paths["peft_report"], 131072)
    require(verified == retained
            and verified["result"]["state"] == "succeeded"
            and verified["operation_id"]
            == peft[0]["record"]["binding"]["admission_id"],
            "M28d shared PEFT verifier mismatch")
    native = verified["result"]["native"]
    accepted = document(paths["native_accepted"])
    require(native["state"] == "succeeded"
            and native["comparison"]["baseline"]["generation"] == 6
            and accepted["generation"] == 7
            and accepted["adapter_sha256"]
            == native["comparison"]["candidate_sha256"]
            and native["phases"][-1]["phase"] == "promote",
            "M28d native comparison or serving generation")
    parity = document(paths["python_parity"])
    require(parity["wheel_version"] == "1.1.0b1"
            and parity["gpu"]["admission_id"]
            == cuda[0]["record"]["binding"]["admission_id"]
            and parity["gpu"]["result_sha256"]
            == cuda[0]["record"]["result_sha256"]
            and parity["peft"]["operation_id"] == verified["operation_id"]
            and parity["peft"]["graph_sha256"]
            == verified["result"]["graph_sha256"]
            and parity["peft"]["verified"] is True,
            "M28d installed Python result parity")
    nat = document(paths["nat_probe"])
    require(nat["nat_version"] == "1.9.0"
            and nat["mcp_sdk_version"] == "1.29.1"
            and nat["transport"] == "streamable-http"
            and nat["terminal"] == "succeeded"
            and "cohesix.recover_job" in nat["tools"],
            "M28d native NeMo MCP transport")
    auth = document(paths["auth_refusals"])
    require(auth == {"missing_auth": 401, "bad_origin": 403,
                     "bad_revision_http": 400, "other_tool_count": 2,
                     "invalid_preflight_is_error": True},
            "M28d auth or scoped discovery refusal")
    cancelled = document(paths["ledger_cancel"])["jobs"]
    cancelled = cancelled["m28d-reference-systemd-08"]
    cancel_result = document(paths["cancel_recover"])["response"]["structuredContent"]
    require(cancelled["cancel_requested"] is True
            and cancelled["execution"] == "refused_no_effect"
            and cancelled["dispatched_unix_ms"] is None
            and cancelled["result_sha256"] is None
            and cancel_result["record"] == cancelled,
            "M28d pre-dispatch cancellation effect")
    budget = document(paths["ledger_budget_revoke"])
    budget_job = budget["jobs"]["m28d-reference-systemd-09"]
    refused = document(paths["budget_refusal"])["response"]
    require(budget_job["execution"] == "confirmed"
            and nat["admission_id"] == budget_job["binding"]["admission_id"]
            and budget["scopes"]["m28d-gpu-budget"]["scope"]["max_total_units"] == 1
            and refused["isError"] is True
            and refused["structuredContent"]["error"]
            == "EPERM selected scope unavailable"
            and "m28d-reference-systemd-10" not in budget["jobs"],
            "M28d standing unit budget exhaustion")
    revoke_preflight = document(paths["revoke_preflight"])["response"]
    revoke = document(paths["revocation_summary"])
    require(revoke_preflight["isError"] is False
            and budget["scopes"]["m28d-gpu-revoke"]["revoked"] is True
            and revoke["rest_error"] == "EPERM standing-scope-revoked"
            and revoke["mcp_error_code"] == -32601
            and "m28d-reference-systemd-11" not in budget["jobs"],
            "M28d revocation before effect")
    summary = {"schema": "cohesix-m28d-mcp-live-summary/v1",
               "proof_class": "live_target", "source_commit": source,
               "manifest_sha256": config["manifest_sha256"], "image": image,
               "qemu_sha256": config["qemu_sha256"],
               "cuda_admission_id": cuda[0]["record"]["binding"]["admission_id"],
               "cuda_output_sha256": cuda_verified["independent_verifier"]["observed_output_sha256"],
               "peft_operation_id": verified["operation_id"],
               "peft_graph_sha256": verified["result"]["graph_sha256"],
               "accepted_generation": accepted["generation"],
               "nat_version": nat["nat_version"],
               "mcp_sdk_version": nat["mcp_sdk_version"],
               "cancelled_no_effect": True, "budget_refused": True,
               "revoked_no_effect": True,
               "mixed_or_weight_transfer_advertised": False,
               "vmlx_client_claimed": False, "result": "PASS"}
    state_dir.mkdir(parents=True, exist_ok=False)
    (state_dir / "summary.json").write_text(json.dumps(summary, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"case": case, "result": "PASS", "summary": str(state_dir / "summary.json")}))
    return 0
