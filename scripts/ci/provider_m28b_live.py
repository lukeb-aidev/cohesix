#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Retain exact-source admitted PEFT lifecycle, serving and rollback observations from a selected Jetson host.
# Copyright 2026 Lukas Bower
"""Run one pre-enrolled M28b release without replaying uncertain side effects."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import threading
import time
import tomllib
from typing import Any

from cohesix.auth import resolve_secret_reference
from provider_m28_live import is_generated_derivation, qemu_image_identity, read_artifact
from provider_matrix import require

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "cohesix-m28b-live-reference/v1"
FIELDS = {"schema", "host_profile", "scenario", "source_commit", "source_manifest",
          "target_qemu_pid", "target_manifest_sha256", "coh_binary", "coh_sha256",
          "agent_binary", "agent_sha256", "native_python", "native_config",
          "native_config_sha256", "hf_helper", "hf_helper_sha256", "deployment",
          "deployment_sha256", "application_client", "application_client_sha256",
          "gateway_url", "request_auth_ref", "delegated_ticket_ref", "wait_seconds",
          "expected_state", "expected_generation", "expected_adapter_sha256"}
POSITIVE = {"train", "import", "resume", "promote"}
NEGATIVE = {"reject", "rollback"}


def digest(path: Path, maximum: int = 64 * 1024 * 1024) -> str:
    """Hash a selected regular input before any admission or native effect."""
    return hashlib.sha256(read_artifact(path, maximum)).hexdigest()


def load_reference(path: Path, host_profile: str, case: str) -> dict[str, Any]:
    """Reject missing identities and scenario/category mismatches before dispatch."""
    value = tomllib.loads(read_artifact(path, 65536).decode())
    require(set(value) == FIELDS and value["schema"] == SCHEMA, "M28b reference fields")
    require(value["host_profile"] == host_profile == "jetson-orin-nano-jp7",
            "M28b host profile")
    scenarios = {"train", "import", "resume"} if case == "m28b-peft-live" else {"promote", "reject", "rollback"}
    require(value["scenario"] in scenarios, "M28b selected case scenario")
    require(value["expected_state"] == ("succeeded" if value["scenario"] in POSITIVE else
                                        "failed" if value["scenario"] == "reject" else "recovered_failure"),
            "M28b predeclared outcome")
    require(type(value["target_qemu_pid"]) is int and value["target_qemu_pid"] > 0
            and type(value["wait_seconds"]) is int and 1 <= value["wait_seconds"] <= 3600,
            "M28b process or deadline bound")
    require(type(value["expected_generation"]) is int and 0 <= value["expected_generation"] < 2**32,
            "M28b generation bound")
    require(value["expected_adapter_sha256"] == "base" or
            value["expected_adapter_sha256"] == "observed" and
            value["scenario"] in {"train", "resume"} or
            isinstance(value["expected_adapter_sha256"], str) and
            re.fullmatch(r"[0-9a-f]{64}", value["expected_adapter_sha256"]) is not None,
            "M28b expected adapter")
    for name in ("source_commit",):
        require(isinstance(value[name], str) and re.fullmatch(r"[0-9a-f]{40}", value[name]),
                f"M28b {name}")
    for name in ("target_manifest_sha256", "coh_sha256", "agent_sha256",
                 "native_config_sha256", "hf_helper_sha256", "deployment_sha256",
                 "application_client_sha256"):
        require(isinstance(value[name], str) and re.fullmatch(r"[0-9a-f]{64}", value[name]),
                f"M28b {name}")
    for name in ("source_manifest", "coh_binary", "agent_binary", "native_python",
                 "native_config", "hf_helper", "deployment", "application_client"):
        require(isinstance(value[name], str) and Path(value[name]).is_absolute(), f"M28b {name} path")
    for name in ("request_auth_ref", "delegated_ticket_ref"):
        require(isinstance(value[name], str) and value[name].startswith(("file:", "env:")),
                f"M28b {name} reference")
    require(re.fullmatch(r"https?://[A-Za-z0-9.:_-]+/?", value["gateway_url"]) is not None
            and (value["gateway_url"].startswith("https://") or
                 value["gateway_url"].startswith("http://127.0.0.1:") or
                 value["gateway_url"].startswith("http://localhost:")),
            "M28b authenticated gateway endpoint")
    return value


def command(coh: Path, mode: str, deployment: Path, timeout: int,
            token: str, ticket_ref: str, gateway: str) -> subprocess.CompletedProcess[str]:
    """Run the installed controller without a shell or plaintext command argument."""
    argv = [str(coh), "--ticket-ref", ticket_ref, "peft", "release", mode,
            "--deployment", str(deployment)]
    if mode == "apply":
        argv += ["--rest-url", gateway]
    environment = dict(os.environ)
    environment["COH_REST_AUTH_TOKEN"] = token
    result = subprocess.run(argv, env=environment, text=True, capture_output=True,
                            timeout=timeout, check=False)
    require(len(result.stdout) <= 1048576 and len(result.stderr) <= 65536,
            "M28b controller output bound")
    return result


def result_report(result: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    require(result.returncode == 0, "M28b controller report unavailable")
    report = json.loads(result.stdout)
    require(report["schema"] == "cohesix-peft-report/v1", "M28b controller report schema")
    return report


def refresh_verifier_clock(path: Path, original: dict[str, Any]) -> None:
    """Verify current records at observation time without changing enrolled trust."""
    current = json.loads(read_artifact(path, 65536))
    require({key: value for key, value in current.items() if key != "verification_unix_ms"}
            == {key: value for key, value in original.items() if key != "verification_unix_ms"},
            "M28b verifier trust changed")
    current["verification_unix_ms"] = int(time.time() * 1000)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.{time.time_ns()}")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(json.dumps(current, separators=(",", ":"), allow_nan=False).encode())
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def interrupt_after_load(root: Path, operation: str, unit: str,
                         deadline: float, done: threading.Event, record: dict[str, Any]) -> None:
    """Stop only the selected owned serving unit after Load has durable completion."""
    load = root / "operations" / operation / "load.json"
    while not done.is_set() and time.monotonic() < deadline:
        if load.is_file():
            observed = json.loads(read_artifact(load, 16384))
            if observed.get("phase") == "load" and observed.get("succeeded") is True:
                stopped = subprocess.run(["systemctl", "--user", "stop", unit],
                                         capture_output=True, text=True, timeout=30, check=False)
                record.update({"load_observation_sha256": digest(load, 16384),
                               "stop_exit": stopped.returncode,
                               "stopped_unix_ms": int(time.time() * 1000)})
                return
        done.wait(0.02)


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Execute one fresh admission, then reconcile signed and native outcomes."""
    require(case in {"m28b-peft-live", "m28b-serving-live"}, "M28b case")
    selected = load_reference(reference, host_profile, case)
    require(not state_dir.exists(), "M28b evidence directory already exists")
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", "HEAD", "--"], cwd=ROOT, text=True).splitlines()
    untracked = subprocess.check_output(
        ["git", "ls-files", "--others", "--exclude-standard"], cwd=ROOT)
    require(commit == selected["source_commit"]
            and all(is_generated_derivation(path) for path in changed)
            and not untracked, "M28b exact source with only selected generated derivatives required")
    require(digest(Path(selected["source_manifest"]), 8 * 1024 * 1024) ==
            selected["target_manifest_sha256"], "M28b manifest changed")
    qemu = qemu_image_identity(selected["target_qemu_pid"], commit, artifact_root=ROOT)
    for name, maximum in [("coh", 64 * 1024 * 1024), ("agent", 64 * 1024 * 1024),
                          ("native_config", 8192), ("hf_helper", 262144),
                          ("deployment", 262144), ("application_client", 262144)]:
        path = Path(selected[name + ("_binary" if name in {"coh", "agent"} else "")])
        require(digest(path, maximum) == selected[name + "_sha256"], f"M28b {name} changed")
    native = json.loads(read_artifact(Path(selected["native_config"]), 8192))
    deployment = json.loads(read_artifact(Path(selected["deployment"]), 262144))
    trust_path = Path(deployment["execution"]["trust"])
    original_trust = json.loads(read_artifact(trust_path, 65536))
    operation = deployment["request"]["operation_id"]
    require(isinstance(operation, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", operation),
            "M28b operation identity")
    root = Path(native["root"])
    require(root.is_absolute() and root.stat().st_mode & 0o077 == 0
            and not (root / "operations" / operation).exists(),
            "M28b fresh private operation required")
    token = resolve_secret_reference(selected["request_auth_ref"])
    state_dir.mkdir(mode=0o700, parents=True)
    coh = Path(selected["coh_binary"])
    deployment_path = Path(selected["deployment"])
    ticket_ref = selected["delegated_ticket_ref"]
    gateway = selected["gateway_url"]
    plan = result_report(command(coh, "plan", deployment_path, 60, token, ticket_ref, gateway))
    require(plan["operation_id"] == operation and not plan["submitted"], "M28b plan identity")
    stopped: dict[str, Any] = {}
    done = threading.Event()
    watcher = None
    if selected["scenario"] == "rollback":
        watcher = threading.Thread(target=interrupt_after_load,
                                   args=(root, operation, native["service"],
                                         time.monotonic() + selected["wait_seconds"], done, stopped),
                                   daemon=True)
        watcher.start()
    started = time.monotonic()
    applied = command(coh, "apply", deployment_path, 90, token, ticket_ref, gateway)
    (state_dir / "apply.stdout.json").write_text(applied.stdout)
    (state_dir / "apply.stderr.txt").write_text(applied.stderr)
    report = None
    while time.monotonic() - started < selected["wait_seconds"]:
        refresh_verifier_clock(trust_path, original_trust)
        watched = command(coh, "watch", deployment_path, 60, token, ticket_ref, gateway)
        if watched.returncode != 0 and "EPERM evidence-stale-or-chronology" in watched.stderr:
            time.sleep(0.1)
            continue
        report = result_report(watched)
        if report["result"] is not None:
            break
        time.sleep(1)
    done.set()
    if watcher is not None:
        watcher.join(timeout=5)
    require(report is not None and report["result"] is not None, "M28b original outcome unresolved")
    native_journal = report["result"]["native"]
    require(report["operation_id"] == operation and report["submitted"]
            and report["result"]["state"] == selected["expected_state"]
            and native_journal["operation_id"] == operation,
            "M28b signed native outcome mismatch")
    phases = {row["phase"]: row["result"] for row in native_journal["phases"]}
    require(phases["validate"]["succeeded"], "M28b native input validation")
    if selected["scenario"] in {"train", "resume"}:
        detail = phases["train"]["detail"]
        require(phases["train"]["succeeded"] and detail["checkpoint"]["native_resume"]
                and detail["checkpoint"]["retained"]["checkpoint_sha256"],
                "M28b full-state checkpoint evidence")
        if selected["scenario"] == "resume":
            require(detail["checkpoint"]["source"]["step"] > 0
                    and detail["checkpoint"]["source"]["source_operation"] != operation,
                    "M28b original checkpoint restoration")
    if selected["scenario"] == "import":
        require("train" not in phases and phases["validate"]["detail"]["training_provenance"] == "unknown",
                "M28b independent import provenance")
    if selected["scenario"] == "reject":
        require("load" not in phases and "promote" not in phases,
                "M28b rejected candidate reached serving")
    if selected["scenario"] == "rollback":
        require(stopped.get("stop_exit") == 0 and phases["rollback"]["succeeded"]
                and phases["rollback"]["detail"]["candidate_release"] == "failed",
                "M28b interrupted promotion restoration")
    expected = selected["expected_adapter_sha256"]
    if expected == "observed":
        expected = phases["train"]["detail"]["adapter_sha256"]
        require(re.fullmatch(r"[0-9a-f]{64}", expected) is not None,
                "M28b observed trained adapter identity")
    expected_adapter = None if expected == "base" else expected
    accepted = json.loads(read_artifact(root / "accepted.json", 8192))
    require(accepted["generation"] == selected["expected_generation"]
            and accepted["adapter_sha256"] == expected_adapter,
            "M28b accepted generation or adapter changed")
    refresh_verifier_clock(trust_path, original_trust)
    verified = command(coh, "verify", deployment_path, 60, token, ticket_ref, gateway)
    require((verified.returncode == 0) == (selected["scenario"] in POSITIVE),
            "M28b candidate verification outcome")
    application = None
    if case == "m28b-serving-live" and selected["scenario"] != "reject":
        output = state_dir / "application.json"
        client = subprocess.run([selected["native_python"], "-I", selected["application_client"],
                                 "--native-config", selected["native_config"], "--operation", operation,
                                 "--generation", str(selected["expected_generation"]), "--out", str(output)] +
                                (["--adapter", expected] if expected_adapter else []) +
                                (["--rollback"] if selected["scenario"] == "rollback" else []),
                                capture_output=True, text=True, timeout=60, check=False)
        require(client.returncode == 0, "M28b application request failed")
        application = json.loads(read_artifact(output, 16384))
    summary = {"schema": "cohesix-m28b-live-report/v1", "case": case,
               "scenario": selected["scenario"], "source_commit": commit,
               "source_manifest_sha256": selected["target_manifest_sha256"],
               "qemu": qemu, "operation_id": operation,
               "signed_graph_sha256": report["result"]["graph_sha256"],
               "native_state": native_journal["state"], "phases": list(phases),
               "accepted_generation": accepted["generation"],
               "accepted_adapter_sha256": accepted["adapter_sha256"],
               "interruption": stopped or None, "application": application}
    (state_dir / "summary.json").write_text(json.dumps(summary, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"result": "PASS", "summary_sha256": digest(state_dir / "summary.json", 16384),
                      "scenario": selected["scenario"]}, sort_keys=True))
    return 0
