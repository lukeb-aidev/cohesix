#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify exact-source selected jobs and standing refusals against a real target and native host.
# Copyright 2026 Lukas Bower

"""M28 live cases retain what the target and native host actually reported."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import time
import tomllib
from typing import Any
from urllib.parse import urlsplit

from cohesix.auth import resolve_secret_reference
from cohesix.backends import RestBackend
from cohesix.errors import CohesixError
from cohesix.providers import registry

from provider_matrix import ROOT, require

SCHEMA = "cohesix-m28-live-reference/v1"
REPORT_SCHEMA = "cohesix-m28-live-report/v1"
CASES = {"m28-jobs-live", "m28-authority-live"}
MAX_FILE_BYTES = 512 * 1024 * 1024
DERIVED_PREFIXES = (
    "apps/root-task/src/generated/", "apps/cohsh/src/generated/",
    "apps/coh/src/generated/", "configs/generated/", "docs/snippets/",
)
DERIVED_FILES = {
    "apps/swarmui/src/generated.rs",
    "crates/cohesix-authority/src/provider_generated.rs",
    "tools/cohesix-py/cohesix/generated.py",
    "tools/cohesix-py/cohesix/provider_generated.py",
    "scripts/cohsh/boot_v0.coh",
}


def is_generated_derivation(path: str) -> bool:
    """Allow only compiler-owned outputs to differ for the Linux KVM profile."""
    return path in DERIVED_FILES or any(
        path.startswith(prefix) for prefix in DERIVED_PREFIXES
    )


def digest(path: Path, maximum: int = MAX_FILE_BYTES) -> str:
    """Hash one bounded regular file through a descriptor opened without symlinks."""
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode), f"missing regular artifact: {path}")
        require(metadata.st_size <= maximum, f"artifact exceeds bound: {path}")
        state = hashlib.sha256()
        for chunk in iter(lambda: stream.read(65536), b""):
            state.update(chunk)
        return state.hexdigest()


def read_artifact(path: Path, maximum: int) -> bytes:
    """Read a small regular evidence input from one bounded descriptor."""
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode), f"missing regular artifact: {path}")
        require(metadata.st_size <= maximum, f"artifact exceeds bound: {path}")
        value = stream.read(maximum + 1)
        require(len(value) <= maximum, f"artifact exceeds bound: {path}")
        return value


def load_reference(path: Path, host_profile: str) -> dict[str, Any]:
    """Reject a partial or self-widening live selection before any effect."""
    document = tomllib.loads(read_artifact(path, 65536).decode("utf-8"))
    expected = {
        "schema", "host_profile", "source_commit", "gateway_url",
        "request_auth_ref", "delegated_ticket_ref", "source_manifest",
        "target_manifest_sha256", "gateway_binary", "gateway_sha256",
        "agent_binary", "agent_sha256", "gpu_bridge_binary", "gpu_bridge_sha256",
        "cuda_helper_binary", "cuda_helper_sha256", "service_request",
        "service_request_sha256", "gpu_request", "gpu_request_sha256",
        "gpu_input_cas", "gpu_device_uuid", "nvidia_driver_version",
        "service_unit", "provider_evidence_root", "wait_seconds", "jobs_evidence",
    }
    require(set(document) == expected, "M28 reference fields")
    require(document["schema"] == SCHEMA, "M28 reference schema")
    require(document["host_profile"] == host_profile, "M28 host profile mismatch")
    require(isinstance(document["wait_seconds"], int)
            and 1 <= document["wait_seconds"] <= 300, "M28 wait bound")
    url = urlsplit(document["gateway_url"])
    require(url.scheme in {"http", "https"} and bool(url.hostname)
            and not url.username and not url.password and not url.path.strip("/")
            and not url.query and not url.fragment, "M28 gateway URL")
    require(url.scheme == "https" or url.hostname in {"127.0.0.1", "::1", "localhost"},
            "M28 credentials require TLS or loopback")
    for key in ("request_auth_ref", "delegated_ticket_ref"):
        require(isinstance(document[key], str)
                and document[key].startswith(("env:", "file:")), "M28 credential reference")
    for key in ("source_manifest", "gateway_binary", "agent_binary",
                "gpu_bridge_binary", "cuda_helper_binary", "gpu_input_cas",
                "service_request", "gpu_request", "jobs_evidence",
                "provider_evidence_root"):
        value = document[key]
        require(isinstance(value, str) and Path(value).is_absolute(), f"M28 {key} path")
    for key in ("source_commit", "target_manifest_sha256", "gateway_sha256",
                "agent_sha256", "gpu_bridge_sha256", "cuda_helper_sha256",
                "service_request_sha256", "gpu_request_sha256"):
        require(isinstance(document[key], str)
                and re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", document[key]),
                f"M28 {key} digest")
    require(len(document["source_commit"]) == 40, "M28 source commit")
    for key in ("target_manifest_sha256", "gateway_sha256", "agent_sha256",
                "gpu_bridge_sha256", "cuda_helper_sha256",
                "service_request_sha256", "gpu_request_sha256"):
        require(len(document[key]) == 64, f"M28 {key} hash")
    require(isinstance(document["gpu_device_uuid"], str)
            and re.fullmatch(r"[0-9a-f]{32}", document["gpu_device_uuid"]),
            "M28 CUDA device UUID")
    require(isinstance(document["service_unit"], str)
            and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", document["service_unit"])
            and ".." not in document["service_unit"]
            and document["service_unit"].endswith(".service"),
            "M28 service unit")
    require(isinstance(document["nvidia_driver_version"], str)
            and re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}", document["nvidia_driver_version"]),
            "M28 NVIDIA driver version")
    return document


def source_and_target(config: dict[str, Any], backend: RestBackend) -> dict[str, str]:
    """Bind source revision, binaries, generated manifest and live boot."""
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()
    require(commit == config["source_commit"], "M28 source commit changed")
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", "HEAD", "--"], cwd=ROOT
    ).decode().splitlines()
    require(all(is_generated_derivation(path) for path in changed),
            "M28 source changes outside selected generated profile")
    untracked = subprocess.check_output(
        ["git", "ls-files", "--others", "--exclude-standard"], cwd=ROOT
    )
    require(not untracked, "M28 untracked source files")
    for binary in ("gateway", "agent", "gpu_bridge", "cuda_helper"):
        require(digest(Path(config[f"{binary}_binary"])) == config[f"{binary}_sha256"],
                f"M28 {binary} binary hash changed")
    require(platform.system() == "Linux" and platform.machine() == "aarch64",
            "M28 live host must be Linux AArch64")
    try:
        query = subprocess.check_output(
            ["/usr/sbin/nvidia-smi", "--query-gpu=uuid,driver_version",
             "--format=csv,noheader"], timeout=5,
        ).decode("utf-8")
    except subprocess.SubprocessError as error:
        raise ValueError("M28 native GPU discovery unavailable") from error
    rows = [row.strip() for row in query.splitlines() if row.strip()]
    require(len(rows) == 1 and rows[0].count(",") == 1,
            "M28 one native GPU device required")
    uuid, driver = (part.strip() for part in rows[0].split(",", 1))
    normalized_uuid = uuid.removeprefix("GPU-").replace("-", "").lower()
    require(normalized_uuid == config["gpu_device_uuid"]
            and driver == config["nvidia_driver_version"],
            "M28 native GPU identity or driver changed")
    manifest = Path(config["source_manifest"])
    require(manifest == ROOT / "configs/generated/root_task_resolved.json",
            "M28 selected generated manifest path")
    manifest_bytes = read_artifact(manifest, 2 * 1024 * 1024)
    manifest_hash = hashlib.sha256(manifest_bytes).hexdigest()
    require(manifest_hash == config["target_manifest_sha256"],
            "M28 source manifest hash changed")
    boot = backend.read_file("/proc/boot", 8192).decode("utf-8")
    require(boot.splitlines().count(f"manifest.sha256={manifest_hash}") == 1,
            "M28 live boot manifest mismatch")
    bounds = backend.get_bounds()
    require(isinstance(bounds, dict) and bounds.get("manifest_sha256") == manifest_hash,
            "M28 gateway manifest mismatch")
    selected = json.loads(manifest_bytes)
    controls = selected["standing_authority"]
    require(controls["enabled"] is True
            and set(controls["actions"]) == {"gpu.workload.submit", "systemd.restart"},
            "M28 generated standing controls")
    gpu_contract = registry()["contract"]["gpu_executor"]
    require(gpu_contract["profile"] == config["host_profile"]
            and gpu_contract["schema"] == "cohesix-gpu-local/v1",
            "M28 selected GPU runtime profile")
    return {
        "source_commit": commit,
        "manifest_sha256": manifest_hash,
        "gateway_sha256": config["gateway_sha256"],
        "agent_sha256": config["agent_sha256"],
        "provider_graph_sha256": registry()["graph_sha256"],
        "gpu_device_uuid": normalized_uuid,
        "nvidia_driver_version": driver,
        "gpu_bridge_sha256": config["gpu_bridge_sha256"],
        "cuda_helper_sha256": config["cuda_helper_sha256"],
    }


def request(path: Path, action: str, graph: str, config: dict[str, Any]) -> dict[str, Any]:
    """Load a fresh immutable identity; the gateway still derives current facts."""
    request_bytes = read_artifact(path, 4096)
    selected = json.loads(request_bytes)
    prefix = "service" if action == "systemd.restart" else "gpu"
    require(hashlib.sha256(request_bytes).hexdigest()
            == config[f"{prefix}_request_sha256"],
            "M28 selected request digest changed")
    require(isinstance(selected, dict) and set(selected) == {"binding", "ticket"},
            "M28 job request shape")
    binding, ticket = selected["binding"], selected["ticket"]
    require(isinstance(binding, dict) and isinstance(ticket, dict)
            and binding.get("schema") == "cohesix-job-binding/v1"
            and binding.get("action") == ticket.get("action") == action
            and binding.get("ticket_id") == ticket.get("id")
            and binding.get("idempotency_key") == ticket.get("idempotency_key")
            and binding.get("policy_sha256") == graph
            and binding.get("units") == 1
            and "admission" not in ticket,
            "M28 exact job identity")
    if action == "systemd.restart":
        unit = config["service_unit"]
        encoded_args = json.dumps(ticket.get("args"), sort_keys=True,
                                  separators=(",", ":")).encode()
        require(ticket.get("target") == binding.get("target")
                == f"/host/systemd/{unit}/restart"
                and ticket.get("args") == {"unit": unit}
                and binding.get("input_sha256")
                == hashlib.sha256(encoded_args).hexdigest(),
                "M28 exact service request")
    else:
        cas = Path(config["gpu_input_cas"])
        cas_bytes = read_artifact(cas, 8192)
        cas_hash = hashlib.sha256(cas_bytes).hexdigest()
        args = ticket.get("args")
        require(isinstance(args, dict)
                and args.get("request_sha256") == cas_hash
                == binding.get("input_sha256")
                and cas.name == f"{cas_hash}.json"
                and binding.get("target")
                == f"/gpu/{ticket.get('subject_ref')}/workload",
                "M28 exact GPU CAS request")
        workload = json.loads(cas_bytes)
        native = workload.get("request")
        gpu_contract = registry()["contract"]["gpu_executor"]
        require(isinstance(native, dict)
                and workload.get("schema") == "cohesix-gpu-workload-input/v1"
                and workload.get("artifact_sha256") == config["cuda_helper_sha256"]
                and isinstance(workload.get("expected_output_sha256"), str)
                and re.fullmatch(r"[0-9a-f]{64}", workload["expected_output_sha256"])
                and native.get("ticket_id") == binding["ticket_id"]
                and native.get("device_uuid") == config["gpu_device_uuid"]
                and native.get("provider_graph_sha256") == graph
                and native.get("entrypoint") in gpu_contract["entrypoints"]
                and isinstance(native.get("memory_budget_bytes"), int)
                and 0 < native["memory_budget_bytes"]
                <= gpu_contract["maximum_device_allocation_bytes"]
                and isinstance(native.get("deadline_ms"), int)
                and 0 < native["deadline_ms"] <= gpu_contract["maximum_runtime_ms"],
                "M28 generated GPU resource and runtime bounds")
    return selected


def observe_terminal(
    backend: RestBackend, admission_id: str, wait_seconds: int
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Wait for the same admission and one exact target terminal; never submit again."""
    deadline = time.monotonic() + wait_seconds
    while time.monotonic() < deadline:
        record = backend.selected_job_status(admission_id)
        require(record["binding"]["admission_id"] == admission_id,
                "M28 status identity mismatch")
        if record["execution"] in {"confirmed", "uncertain", "refused_no_effect"}:
            reconciliation = backend.reconcile_selected_job(admission_id)
            require(reconciliation["effect_replay_allowed"] is False
                    and reconciliation["record"]["binding"] == record["binding"],
                    "M28 reconciliation identity mismatch")
            record = reconciliation["record"]
            rows = reconciliation["target_results"]
            hashes = reconciliation["target_result_sha256"]
            require(isinstance(rows, list) and isinstance(hashes, list)
                    and len(rows) == len(hashes) <= 1,
                    "M28 target result ambiguous")
            if (
                record["execution"] == "confirmed"
                and record["delivery"] == "acknowledged"
                and len(rows) == 1
            ):
                terminal = rows[0]
                require(terminal["state"] == "succeeded"
                        and hashes[0] == record["result_sha256"]
                        and terminal["id"] == record["binding"]["ticket_id"]
                        and terminal["idempotency_key"] == record["binding"]["idempotency_key"]
                        and terminal["admission"]["admission_id"] == admission_id,
                        "M28 target terminal mismatch")
                return record, terminal
        time.sleep(0.25)
    raise ValueError(f"M28 terminal/delivery not confirmed for {admission_id}")


def submit_once(
    backend: RestBackend, selected: dict[str, Any], wait_seconds: int,
    evidence_root: Path, graph: str,
) -> dict[str, Any]:
    """A lost HTTP reply is resolved by identity, never by replaying POST."""
    binding = selected["binding"]
    admission_id = binding["admission_id"]
    try:
        response = backend.submit_selected_job(binding, selected["ticket"])
        require(response.get("record", {}).get("binding") == binding
                and response.get("submission") == "target_write_ack",
                "M28 target write ACK unavailable")
        submission = "target_write_ack"
    except CohesixError as error:
        submission = f"uncertain_response:{type(error).__name__}"
        # The original admission may have reached Root. Status is the only
        # permitted next operation; no second POST is made on this identity.
    record, terminal = observe_terminal(backend, admission_id, wait_seconds)
    native = native_evidence(evidence_root, terminal, binding, graph)
    return {"admission_id": admission_id, "submission": submission,
            "record": record, "target_terminal": terminal, "native": native}


def native_evidence(
    root: Path, terminal: dict[str, Any], binding: dict[str, Any], graph: str
) -> dict[str, Any]:
    """Read the retained native object named by the exact target result."""
    message = terminal.get("message")
    require(isinstance(message, str), "M28 native reference missing")
    matches = re.findall(r"native_observation=sha256:([0-9a-f]{64})", message)
    require(len(matches) == 1, "M28 native reference ambiguous")
    native_hash = matches[0]
    path = root / f"{native_hash}.json"
    native_bytes = read_artifact(path, 65536)
    require(hashlib.sha256(native_bytes).hexdigest() == native_hash,
            "M28 native object digest")
    native = json.loads(native_bytes)
    require(native.get("schema") == "cohesix-native-provider-evidence/v1"
            and native.get("ticket_id") == binding["ticket_id"]
            and native.get("idempotency_key") == binding["idempotency_key"]
            and native.get("action") == binding["action"]
            and native.get("provider_graph_sha256") == graph
            and native.get("proof_class") == "native_provider_operation"
            and native.get("authoritative") is False,
            "M28 native object correlation")
    observation = native.get("observation")
    require(isinstance(observation, dict), "M28 native observation shape")
    if binding["action"] == "systemd.restart":
        before, after = observation.get("before"), observation.get("after")
        require(isinstance(before, dict) and isinstance(after, dict)
                and observation.get("job", 0) > 0
                and after.get("active_state") == "active"
                and after.get("service_result") == "success"
                and before.get("invocation_id") != after.get("invocation_id"),
                "M28 systemd native postcondition")
    else:
        require(observation.get("state") == "succeeded"
                and isinstance(observation.get("terminal_unix_ms"), int)
                and observation["terminal_unix_ms"] > 0,
                "M28 GPU native postcondition")
    return {"sha256": native_hash, "path": str(path), "observation": native}


def stale_refusal(backend: RestBackend, selected: dict[str, Any]) -> dict[str, str]:
    """Mutate only a fresh identity and state generation; no effect is attempted."""
    clone = json.loads(json.dumps(selected))
    binding, ticket = clone["binding"], clone["ticket"]
    suffix = hashlib.sha256(os.urandom(16)).hexdigest()[:16]
    for key, value in (("admission_id", "stale-" + suffix),
                       ("ticket_id", "stale-ticket-" + suffix),
                       ("idempotency_key", "stale-once-" + suffix)):
        binding[key] = value
    ticket["id"] = binding["ticket_id"]
    ticket["idempotency_key"] = binding["idempotency_key"]
    binding["state_epoch"] += 1
    try:
        backend.submit_selected_job(binding, ticket)
    except CohesixError as error:
        refusal = str(error)
        require("standing facts changed" in refusal or "standing-facts-stale" in refusal,
                "M28 stale request refusal class")
    else:
        raise ValueError("M28 stale job was admitted")
    try:
        backend.selected_job_status(binding["admission_id"])
    except CohesixError:
        pass
    else:
        raise ValueError("M28 stale job acquired a durable allocation")
    return {"admission_id": binding["admission_id"], "refusal": refusal}


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Execute exactly one selected live case into a new private evidence root."""
    require(case in CASES, "M28 live case id")
    config = load_reference(reference, host_profile)
    state_dir.mkdir(mode=0o700, parents=True, exist_ok=False)
    proof: dict[str, Any] = {"jobs": []} if case == "m28-jobs-live" else {}
    summary = {"schema": REPORT_SCHEMA, "case": case, "proof_class": "live_target",
               "result": "IN_PROGRESS", "identity": None, "proof": proof,
               "limits": ["Native evidence signatures require the separate causal verifier.",
                          "This case does not qualify a release or unrelated host/target profiles."]}

    def retain() -> None:
        output = state_dir / "summary.json"
        with output.open("w", encoding="utf-8") as stream:
            stream.write(json.dumps(summary, sort_keys=True, indent=2) + "\n")
            stream.flush()
            os.fsync(stream.fileno())

    retain()
    try:
        auth = resolve_secret_reference(config["request_auth_ref"])
        ticket = resolve_secret_reference(config["delegated_ticket_ref"])
        backend = RestBackend(
            config["gateway_url"], request_auth_token=auth,
            delegated_ticket=ticket, timeout_s=5.0, max_attempts=1,
        )
        identity = source_and_target(config, backend)
        summary["identity"] = identity
        retain()
        service = request(Path(config["service_request"]), "systemd.restart",
                          identity["provider_graph_sha256"], config)
        gpu = request(Path(config["gpu_request"]), "gpu.workload.submit",
                      identity["provider_graph_sha256"], config)
        require(service["binding"]["admission_id"] != gpu["binding"]["admission_id"],
                "M28 job identity collision")
        if case == "m28-jobs-live":
            for selected in (service, gpu):
                admission_id = selected["binding"]["admission_id"]
                with (state_dir / "attempts.jsonl").open("a", encoding="utf-8") as stream:
                    stream.write(json.dumps({"admission_id": admission_id,
                                             "request_sha256": hashlib.sha256(
                                                 json.dumps(selected, sort_keys=True).encode()
                                             ).hexdigest()}) + "\n")
                    stream.flush()
                    os.fsync(stream.fileno())
                directory_fd = os.open(state_dir, os.O_RDONLY)
                try:
                    os.fsync(directory_fd)
                finally:
                    os.close(directory_fd)
                proof["jobs"].append(submit_once(
                    backend, selected, config["wait_seconds"],
                    Path(config["provider_evidence_root"]),
                    identity["provider_graph_sha256"],
                ))
                retain()
        else:
            prior_bytes = read_artifact(Path(config["jobs_evidence"]), 65536)
            prior = json.loads(prior_bytes)
            require(prior.get("schema") == REPORT_SCHEMA
                    and prior.get("case") == "m28-jobs-live"
                    and prior.get("result") == "PASS"
                    and prior.get("identity") == identity,
                    "M28 prior jobs evidence mismatch")
            for job in prior["proof"]["jobs"]:
                current = backend.selected_job_status(job["admission_id"])
                require(current == job["record"]
                        and current["execution"] == "confirmed"
                        and current["delivery"] == "acknowledged",
                        "M28 prior job outcome changed")
            proof["jobs_evidence_sha256"] = hashlib.sha256(prior_bytes).hexdigest()
            proof["stale_refusals"] = []
            for selected in (service, gpu):
                proof["stale_refusals"].append(stale_refusal(backend, selected))
                retain()
        summary["result"] = "PASS"
    except (ValueError, OSError, KeyError, TypeError, CohesixError,
            subprocess.SubprocessError) as exc:
        summary["result"] = "FAIL"
        summary["error"] = str(exc)[:512]
        retain()
        print(json.dumps({"result": "FAIL", "summary": str(state_dir / "summary.json")}))
        return 1
    retain()
    print(json.dumps({"result": "PASS", "summary": str(state_dir / "summary.json")}))
    return 0
