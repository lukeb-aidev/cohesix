#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify exact-source admitted registered CUDA work and independent outputs on the selected Jetson host.
# Copyright 2026 Lukas Bower

"""One fresh M28a workload case retains its own source, target and native proof."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import tomllib
from typing import Any
from urllib.parse import urlsplit

from cohesix.auth import resolve_secret_reference
from cohesix.backends import RestBackend
from cohesix.errors import CohesixError
from cohesix.workload import batch_edges_expected, verify_batch_edges

from provider_m28_live import digest, read_artifact, source_and_target, submit_once
from provider_matrix import require

SCHEMA = "cohesix-m28a-live-reference/v1"
REPORT_SCHEMA = "cohesix-m28a-live-report/v1"
FIELDS = {
    "schema", "host_profile", "source_commit", "gateway_url",
    "request_auth_ref", "delegated_ticket_ref", "source_manifest",
    "target_qemu_pid", "target_manifest_sha256", "gateway_binary",
    "gateway_sha256", "agent_binary", "agent_sha256",
    "gpu_bridge_binary", "gpu_bridge_sha256", "cuda_helper_binary",
    "cuda_helper_sha256", "gpu_device_uuid", "nvidia_driver_version",
    "provider_evidence_root", "wait_seconds", "scenario", "lane",
    "selected_request", "selected_sha256", "input_cas",
    "registration", "registration_sha256", "package_binary",
    "package_sha256", "input_file", "executor_state_root",
}
RECOVERY_FIELDS = {
    "cancel_ticket", "cancel_ticket_sha256", "admin_ticket_ref",
}
PATHS = {
    "source_manifest", "gateway_binary", "agent_binary", "gpu_bridge_binary",
    "cuda_helper_binary", "provider_evidence_root", "selected_request",
    "input_cas", "registration", "package_binary", "input_file",
    "executor_state_root",
}
HASHES = {
    "target_manifest_sha256", "gateway_sha256", "agent_sha256",
    "gpu_bridge_sha256", "cuda_helper_sha256", "selected_sha256",
    "registration_sha256", "package_sha256",
}


def load_reference(path: Path, host_profile: str,
                   case: str = "m28a-workloads-live") -> dict[str, Any]:
    """Reject incomplete or widened operator inputs before contacting a target."""
    config = tomllib.loads(read_artifact(path, 65536).decode("utf-8"))
    required = FIELDS | (RECOVERY_FIELDS if case == "m28a-recovery-live" else set())
    require(set(config) == required and config["schema"] == SCHEMA,
            "M28a selected reference fields")
    require(config["host_profile"] == host_profile == "jetson-orin-nano-jp7",
            "M28a host profile")
    require(config["scenario"] in {"reference", "adaptation"}
            and config["lane"] in {"systemd", "docker"},
            "M28a workload and native owner")
    require(type(config["target_qemu_pid"]) is int and config["target_qemu_pid"] > 0
            and type(config["wait_seconds"]) is int
            and 1 <= config["wait_seconds"] <= 300,
            "M28a process and deadline")
    require(isinstance(config["source_commit"], str)
            and re.fullmatch(r"[0-9a-f]{40}", config["source_commit"]),
            "M28a source commit")
    require(isinstance(config["gpu_device_uuid"], str)
            and re.fullmatch(r"[0-9a-f]{32}", config["gpu_device_uuid"]),
            "M28a device UUID")
    require(isinstance(config["nvidia_driver_version"], str)
            and re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}",
                             config["nvidia_driver_version"]),
            "M28a NVIDIA driver")
    for key in HASHES:
        require(isinstance(config[key], str)
                and re.fullmatch(r"[0-9a-f]{64}", config[key]),
                f"M28a {key} digest")
    for key in PATHS:
        require(isinstance(config[key], str) and Path(config[key]).is_absolute(),
                f"M28a {key} path")
    for key in ("request_auth_ref", "delegated_ticket_ref"):
        require(isinstance(config[key], str)
                and config[key].startswith(("env:", "file:")),
                f"M28a {key} reference")
    if case == "m28a-recovery-live":
        require(isinstance(config["cancel_ticket_sha256"], str)
                and re.fullmatch(r"[0-9a-f]{64}",
                                 config["cancel_ticket_sha256"]),
                "M28a cancel request digest")
        require(isinstance(config["cancel_ticket"], str)
                and Path(config["cancel_ticket"]).is_absolute(),
                "M28a cancel request path")
        require(isinstance(config["admin_ticket_ref"], str)
                and config["admin_ticket_ref"].startswith(("env:", "file:")),
                "M28a administrative ticket reference")
    url = urlsplit(config["gateway_url"])
    require(url.scheme in {"http", "https"} and bool(url.hostname)
            and not url.username and not url.password and not url.query
            and not url.fragment and not url.path.strip("/")
            and (url.scheme == "https" or url.hostname in
                 {"127.0.0.1", "::1", "localhost"}),
            "M28a authenticated gateway URL")
    return config


def selected_workload(config: dict[str, Any], graph: str,
                      now_ms: int) -> tuple[dict[str, Any], dict[str, int]]:
    """Bind the original admission to the registered package and frozen dataset."""
    selected_path = Path(config["selected_request"])
    selected_bytes = read_artifact(selected_path, 4096)
    require(hashlib.sha256(selected_bytes).hexdigest()
            == config["selected_sha256"], "M28a selected request changed")
    selected = json.loads(selected_bytes)
    require(isinstance(selected, dict) and set(selected) == {"binding", "ticket"},
            "M28a selected job shape")
    binding, ticket = selected["binding"], selected["ticket"]
    require(isinstance(binding, dict) and isinstance(ticket, dict)
            and binding.get("schema") == "cohesix-job-binding/v1"
            and binding.get("action") == ticket.get("action")
            == "gpu.workload.submit"
            and binding.get("ticket_id") == ticket.get("id")
            and binding.get("idempotency_key") == ticket.get("idempotency_key")
            and binding.get("policy_sha256") == graph
            and binding.get("units") == 1
            and binding.get("target")
            == f"/gpu/{ticket.get('subject_ref')}/workload"
            and "admission" not in ticket,
            "M28a exact admitted job identity")
    cas_path = Path(config["input_cas"])
    cas_bytes = read_artifact(cas_path, 8192)
    cas_hash = hashlib.sha256(cas_bytes).hexdigest()
    require(cas_path.name == f"{cas_hash}.json"
            and binding.get("input_sha256") == cas_hash
            and ticket.get("args", {}).get("request_sha256") == cas_hash,
            "M28a exact input CAS")
    workload = json.loads(cas_bytes)
    request = workload.get("request")
    require(isinstance(request, dict)
            and workload.get("schema") == "cohesix-gpu-workload-input/v2"
            and workload.get("artifact_sha256") == config["cuda_helper_sha256"]
            and request.get("schema") == "cohesix-registered-cuda-request/v1"
            and request.get("ticket_id") == binding["ticket_id"]
            and request.get("device_uuid") == config["gpu_device_uuid"]
            and request.get("provider_graph_sha256") == graph
            and request.get("registration_sha256")
            == config["registration_sha256"]
            and type(request.get("inventory_observed_unix_ms")) is int
            and 0 <= now_ms - request["inventory_observed_unix_ms"] < 5000
            and type(request.get("memory_budget_bytes")) is int
            and 0 < request["memory_budget_bytes"] <= 64 * 1024 * 1024
            and type(request.get("deadline_ms")) is int
            and 0 < request["deadline_ms"] <= 30_000,
            "M28a request device, registration or freshness")
    registration_bytes = read_artifact(Path(config["registration"]), 8192)
    require(Path(config["registration"])
            == Path(config["executor_state_root"]) / "registrations"
            / f"{config['registration_sha256']}.json",
            "M28a installed registration path")
    require(hashlib.sha256(registration_bytes).hexdigest()
            == config["registration_sha256"], "M28a registration changed")
    registration = json.loads(registration_bytes)
    require(registration.get("schema") == "cohesix-cuda-registration/v1"
            and registration.get("id") == "batch-edges"
            and registration.get("device_uuid") == config["gpu_device_uuid"]
            and registration.get("package_sha256") == config["package_sha256"]
            and registration.get("package") == config["package_binary"]
            and registration.get("input_root")
            == str(Path(config["input_file"]).parent)
            and digest(Path(config["package_binary"]), 16 * 1024 * 1024)
            == config["package_sha256"],
            "M28a enrolled package")
    parameters = request.get("parameters")
    require(isinstance(parameters, dict)
            and set(parameters) == {"width", "height", "frames", "iterations"}
            and all(type(value) is int for value in parameters.values()),
            "M28a task parameters")
    data = read_artifact(Path(config["input_file"]), 262_144)
    require(hashlib.sha256(data).hexdigest() == request.get("input_sha256")
            and Path(config["input_file"]).name == request["input_sha256"],
            "M28a input data CAS")
    expected = batch_edges_expected(data, parameters["width"],
                                    parameters["height"], parameters["frames"])
    require(hashlib.sha256(expected).hexdigest()
            == workload.get("expected_output_sha256"),
            "M28a frozen independent output")
    return selected, parameters


def direct_cancel(config: dict[str, Any], original: dict[str, Any]) -> dict[str, Any]:
    """Bind a separate target-admitted control ticket to the original native job."""
    data = read_artifact(Path(config["cancel_ticket"]), 4096)
    require(hashlib.sha256(data).hexdigest()
            == config["cancel_ticket_sha256"],
            "M28a cancel request changed")
    ticket = json.loads(data)
    prior = original["ticket"]
    require(isinstance(ticket, dict)
            and ticket.get("schema") == "host-ticket/v2"
            and ticket.get("action") == "gpu.workload.cancel"
            and ticket.get("id") != prior.get("id")
            and ticket.get("idempotency_key") != prior.get("idempotency_key")
            and ticket.get("operation_id") == ticket.get("id")
            and ticket.get("receipt_mode") == "worker"
            and "target" not in ticket and prior.get("target") is None
            and all(ticket.get(key) == prior.get(key) for key in (
                "subject_ref", "receipt_worker_role", "receipt_worker_id",
                "receipt_supervisor_generation", "receipt_cap_generation",
                "writer_epoch",
            ))
            and ticket.get("args") == {"job_id": prior.get("id")}
            and type(ticket.get("expires_unix_ms")) is int
            and ticket["expires_unix_ms"] <= prior.get("expires_unix_ms", 0)
            and "admission" not in ticket
            and "resolved_worker_slot" not in ticket
            and "resolved_lease_epoch" not in ticket
            and "admission_sequence" not in ticket,
            "M28a cancel must name the original native job")
    return ticket


def direct_terminal(backend: RestBackend, ticket: dict[str, Any],
                    wait_seconds: int) -> dict[str, Any]:
    """Reconcile a raw target control ticket without resubmitting its effect."""
    deadline = time.monotonic() + wait_seconds
    while time.monotonic() < deadline:
        data = backend.read_file("/host/tickets/status", 32768)
        rows = [json.loads(line) for line in data.splitlines() if line]
        matches = [row for row in rows if row.get("id") == ticket["id"]
                   and row.get("idempotency_key") == ticket["idempotency_key"]]
        terminals = [row for row in matches
                     if row.get("state") in {"succeeded", "failed", "expired"}]
        require(len(terminals) <= 1 or all(row == terminals[0] for row in terminals),
                "M28a cancel target result ambiguous")
        if terminals:
            result = terminals[-1]
            require(result.get("action") == "gpu.workload.cancel"
                    and result.get("operation_id") == ticket["operation_id"]
                    and result.get("subject_ref") == ticket["subject_ref"]
                    and result.get("receipt_worker_id") == ticket["receipt_worker_id"],
                    "M28a cancel target identity changed")
            return result
        time.sleep(0.25)
    raise ValueError(f"M28a cancel outcome unresolved for {ticket['id']}")


def terminal_for(backend: RestBackend, binding: dict[str, Any],
                 wait_seconds: int) -> tuple[dict[str, Any], dict[str, Any]]:
    """Reconcile exactly one target terminal, including a failed original job."""
    admission = binding["admission_id"]
    deadline = time.monotonic() + wait_seconds
    while time.monotonic() < deadline:
        record = backend.selected_job_status(admission)
        require(record.get("binding") == binding,
                "M28a recovery admission changed")
        if record.get("execution") in {"confirmed", "refused_no_effect", "uncertain"}:
            reconciled = backend.reconcile_selected_job(admission)
            require(reconciled.get("effect_replay_allowed") is False
                    and reconciled.get("record", {}).get("binding") == binding,
                    "M28a recovery reconciliation changed")
            rows = reconciled.get("target_results")
            hashes = reconciled.get("target_result_sha256")
            require(isinstance(rows, list) and isinstance(hashes, list)
                    and len(rows) == len(hashes) <= 16,
                    "M28a recovery result stream bound")
            terminals = []
            for row, row_hash in zip(rows, hashes):
                require(isinstance(row, dict) and isinstance(row_hash, str)
                        and re.fullmatch(r"[0-9a-f]{64}", row_hash)
                        and row.get("id") == binding["ticket_id"]
                        and row.get("idempotency_key") == binding["idempotency_key"]
                        and row.get("admission", {}).get("admission_id") == admission,
                        "M28a recovery result identity")
                if row.get("state") in {"succeeded", "failed", "expired"}:
                    terminals.append((row, row_hash))
            require(len({row_hash for _, row_hash in terminals}) <= 1,
                    "M28a recovery terminal ambiguous")
            if (record["execution"] == "confirmed"
                    and record["delivery"] == "acknowledged" and terminals):
                terminal, row_hash = terminals[-1]
                require(record.get("result_sha256") == row_hash,
                        "M28a recovery result hash changed")
                return record, terminal
        time.sleep(0.25)
    raise ValueError(f"M28a original outcome unresolved for {admission}")


def native_for(root: Path, terminal: dict[str, Any],
               binding: dict[str, Any], graph: str) -> dict[str, Any]:
    """Resolve a signed target reference to its bounded native evidence object."""
    message = terminal.get("message")
    require(isinstance(message, str), "M28a native reference missing")
    found = re.findall(r"native_observation=sha256:([0-9a-f]{64})", message)
    require(len(found) == 1, "M28a native reference ambiguous")
    data = read_artifact(root / f"{found[0]}.json", 65536)
    require(hashlib.sha256(data).hexdigest() == found[0],
            "M28a native object digest")
    native = json.loads(data)
    require(native.get("schema") == "cohesix-native-provider-evidence/v1"
            and native.get("ticket_id") == binding["ticket_id"]
            and native.get("idempotency_key") == binding["idempotency_key"]
            and native.get("action") == binding["action"]
            and native.get("provider_graph_sha256") == graph
            and native.get("proof_class") == "native_provider_operation"
            and native.get("authoritative") is False,
            "M28a native result correlation")
    return native


def recovery(backend: RestBackend, config: dict[str, Any],
             selected: dict[str, Any], graph: str,
             state_dir: Path) -> dict[str, Any]:
    """Drop one response, cancel the active child and prove settled accounting."""
    cancel = direct_cancel(config, selected)
    original_binding = selected["binding"]
    scope_id = original_binding["scope_id"]
    with (state_dir / "attempts.jsonl").open("x", encoding="utf-8") as stream:
        stream.write(json.dumps({
            "admission_id": original_binding["admission_id"],
            "ticket_id": original_binding["ticket_id"],
            "action": original_binding["action"],
            "request_sha256": hashlib.sha256(
                json.dumps(selected, sort_keys=True).encode()).hexdigest(),
        }, sort_keys=True) + "\n")
        stream.write(json.dumps({
            "ticket_id": cancel["id"],
            "action": cancel["action"],
            "request_sha256": config["cancel_ticket_sha256"],
        }, sort_keys=True) + "\n")
        stream.flush()
        os.fsync(stream.fileno())
    directory_fd = os.open(state_dir, os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)
    admin = RestBackend(
        config["gateway_url"],
        request_auth_token=resolve_secret_reference(config["request_auth_ref"]),
        delegated_ticket=resolve_secret_reference(config["admin_ticket_ref"]),
        timeout_s=5.0, max_attempts=1,
    )
    before = admin.inspect_standing_scope(scope_id)
    initial = before["budget"]
    # A received response is deliberately discarded to reproduce a controller
    # interruption without risking a second side effect on this identity.
    try:
        backend.submit_selected_job(original_binding, selected["ticket"])
        submission = "discarded_after_send"
    except CohesixError as error:
        submission = f"uncertain_response:{type(error).__name__}"
    interrupted = RestBackend(
        config["gateway_url"],
        request_auth_token=resolve_secret_reference(config["request_auth_ref"]),
        delegated_ticket=resolve_secret_reference(config["delegated_ticket_ref"]),
        timeout_s=5.0, max_attempts=1,
    )
    require(interrupted.selected_job_status(
        original_binding["admission_id"])["binding"] == original_binding,
            "M28a lost reply could not be reconciled by identity")
    marker = (Path(config["executor_state_root"]) / original_binding["ticket_id"]
              / "execution" / "started.json")
    deadline = time.monotonic() + config["wait_seconds"]
    started = None
    while time.monotonic() < deadline:
        if marker.exists():
            try:
                started = json.loads(read_artifact(marker, 1024))
            except json.JSONDecodeError:
                started = None
            if started is not None:
                break
        time.sleep(0.05)
    require(isinstance(started, dict)
            and started.get("schema") == "cohesix-registered-cuda-started/v1"
            and started.get("device_uuid") == config["gpu_device_uuid"]
            and started.get("workload_id") == "batch-edges"
            and started.get("completed_iterations") == 1,
            "M28a cancellation did not reach the native CUDA child")
    during = admin.inspect_standing_scope(scope_id)
    require(during["budget"]["reserved_units"] == initial["reserved_units"] + 1,
            "M28a original reservation absent before cancellation")
    sentinel = subprocess.Popen(
        ["/bin/sleep", "600"], stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        cancel_submission = "target_write_ack"
        try:
            payload = json.dumps(cancel, separators=(",", ":")).encode()
            require(interrupted.write_append("/host/tickets/spec", payload)
                    == len(payload), "M28a cancel target write ACK unavailable")
        except CohesixError as error:
            cancel_submission = f"uncertain_response:{type(error).__name__}"
        cancel_terminal = direct_terminal(
            interrupted, cancel, config["wait_seconds"],
        )
        require(cancel_terminal["state"] == "succeeded"
                and sentinel.poll() is None,
                "M28a cancel terminal or unrelated process changed")
    finally:
        if sentinel.poll() is None:
            sentinel.terminate()
        sentinel.wait(timeout=5)
    cancelled_native = native_for(
        Path(config["provider_evidence_root"]),
        cancel_terminal, {"ticket_id": cancel["id"],
                          "idempotency_key": cancel["idempotency_key"],
                          "action": cancel["action"]}, graph,
    )
    cancelled_job = cancelled_native.get("observation", {})
    require(cancelled_job.get("binding", {}).get("ticket_id")
            == original_binding["ticket_id"]
            and cancelled_job.get("state") == "cancelled"
            and type(cancelled_job.get("terminal_unix_ms")) is int,
            "M28a native cancellation not reaped")
    original_record, original_terminal = terminal_for(
        interrupted, original_binding, config["wait_seconds"],
    )
    require(original_terminal["state"] in {"failed", "expired"},
            "M28a cancelled original reported success")
    original_native = native_for(
        Path(config["provider_evidence_root"]), original_terminal,
        original_binding, graph,
    )
    require(original_native.get("observation", {}).get("state") == "cancelled",
            "M28a original native result changed")
    after = admin.inspect_standing_scope(scope_id)
    require(after["budget"]["reserved_units"] == initial["reserved_units"]
            and after["budget"]["active"] == initial["active"]
            and after["budget"]["settled_units"]
            == initial["settled_units"] + 1,
            "M28a original capacity not settled after observed reaping")
    return {"lost_response": submission,
            "original_admission_id": original_binding["admission_id"],
            "started": started, "marker": str(marker),
            "cancel": {"submission": cancel_submission,
                       "target_terminal": cancel_terminal,
                       "unrelated_sentinel_alive_after_cancel": True},
            "cancel_native": cancelled_native,
            "original_record": original_record,
            "original_terminal": original_terminal,
            "original_native": original_native,
            "scope_before": before, "scope_during": during, "scope_after": after}


def run_live(case: str, reference: Path, host_profile: str,
             state_dir: Path) -> int:
    """Submit once, reconcile by identity and retain independent pixel proof."""
    require(case in {"m28a-workloads-live", "m28a-recovery-live"},
            "M28a live case")
    config = load_reference(reference, host_profile, case)
    state_dir.mkdir(mode=0o700, parents=True, exist_ok=False)
    summary: dict[str, Any] = {
        "schema": REPORT_SCHEMA, "case": case, "scenario": config["scenario"],
        "lane": config["lane"], "proof_class": "live_target",
        "result": "IN_PROGRESS", "identity": None, "proof": None,
        "limits": ["One scenario and one native owner per invocation; combine all required runs in the milestone evidence ledger.",
                   "This component check does not qualify a release or unrelated GPU."],
    }

    def retain() -> None:
        with (state_dir / "summary.json").open("w", encoding="utf-8") as stream:
            stream.write(json.dumps(summary, sort_keys=True, indent=2) + "\n")
            stream.flush()
            os.fsync(stream.fileno())

    retain()
    try:
        backend = RestBackend(
            config["gateway_url"],
            request_auth_token=resolve_secret_reference(config["request_auth_ref"]),
            delegated_ticket=resolve_secret_reference(config["delegated_ticket_ref"]),
            timeout_s=5.0, max_attempts=1,
        )
        identity = source_and_target(config, backend)
        summary["identity"] = identity
        retain()
        selected, dimensions = selected_workload(
            config, identity["provider_graph_sha256"], int(time.time() * 1000),
        )
        if case == "m28a-recovery-live":
            summary["proof"] = recovery(
                backend, config, selected, identity["provider_graph_sha256"],
                state_dir,
            )
            summary["result"] = "PASS"
            retain()
            print(json.dumps({"result": "PASS",
                              "summary": str(state_dir / "summary.json")}))
            return 0
        job = submit_once(
            backend, selected, config["wait_seconds"],
            Path(config["provider_evidence_root"]),
            identity["provider_graph_sha256"],
        )
        ticket_id = selected["binding"]["ticket_id"]
        output_path = (Path(config["executor_state_root"]) / ticket_id
                       / "execution" / "output.bin")
        report = verify_batch_edges(
            Path(config["input_file"]), output_path,
            dimensions["width"], dimensions["height"], dimensions["frames"],
        )
        observation = job["native"]["observation"]["observation"]
        require(observation.get("schema") == "cohesix-registered-cuda-observation/v1"
                and observation.get("output", {}).get("sha256")
                == report["observed_output_sha256"]
                and observation.get("registration_sha256")
                == config["registration_sha256"]
                and observation.get("package_sha256") == config["package_sha256"]
                and observation.get("native_enforcement", {}).get("owner", {}).get("kind")
                == config["lane"],
                "M28a native owner, package or output changed")
        summary["proof"] = {"job": job, "verifier": report}
        summary["result"] = "PASS"
    except (ValueError, OSError, KeyError, TypeError, CohesixError,
            subprocess.SubprocessError) as error:
        summary["result"] = "FAIL"
        summary["error"] = str(error)[:512]
    retain()
    print(json.dumps({"result": summary["result"],
                      "summary": str(state_dir / "summary.json")}))
    return 0 if summary["result"] == "PASS" else 1
