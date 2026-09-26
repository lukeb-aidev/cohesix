#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check the pinned NeMo installation and source-bound native Toolkit workflow evidence.
# Copyright 2026 Lukas Bower
"""Accept only an installed artifact and independently verified live outcomes."""

from __future__ import annotations

import csv
import hashlib
import json
from pathlib import Path
import re
import subprocess
from typing import Any
from urllib.parse import urlsplit
from urllib.request import urlopen

from provider_m28_live import qemu_image_identity, read_artifact
from provider_matrix import require


SCHEMA = "cohesix-m28f-nemo-reference/v1"
VERSIONS = {"cohesix-nemo-kit": "0.1.0", "nvidia-nat": "1.9.0",
            "nvidia-nat-mcp": "1.9.0", "nvidia-nat-a2a": "1.9.0",
            "mcp": "1.29.1", "a2a-sdk": "0.3.26"}
EVIDENCE = {"doctor", "mcp_discovery", "a2a_discovery", "cuda_mcp",
            "cuda_verified", "peft_a2a", "peft_deployment", "peft_report",
            "mcp_peft_recovery", "a2a_cuda_recovery", "rejected_a2a",
            "rejected_deployment", "rejected_report", "negative_action",
            "cross_subject", "budget_denial", "budget_recovery", "agent_mcp",
            "agent_a2a", "eval_direct", "eval_a2a", "accepted_state"}


def document(path: Path, maximum: int = 131072) -> dict[str, Any]:
    """Read one bounded non-symlink JSON object."""
    value = json.loads(read_artifact(path, maximum))
    require(isinstance(value, dict), f"M28f evidence object: {path.name}")
    return value


def digest(path: Path, maximum: int) -> str:
    """Hash an exact bounded package or target artifact."""
    return hashlib.sha256(read_artifact(path, maximum)).hexdigest()


def installed(venv: Path) -> dict[str, str]:
    """Inspect distributions inside the fresh wheel installation itself."""
    python = venv / "bin/python"
    require(python.exists(), "M28f installed Python missing")
    script = ("import importlib.metadata as m,json; "
              "names=['cohesix-nemo-kit','nvidia-nat','nvidia-nat-mcp',"
              "'nvidia-nat-a2a','mcp','a2a-sdk']; "
              "d=m.distribution('cohesix-nemo-kit'); "
              "print(json.dumps({'versions':{n:m.version(n) for n in names},"
              "'direct_url':d.read_text('direct_url.json')}))")
    result = json.loads(subprocess.check_output([str(python), "-c", script],
                                                 text=True, timeout=20))
    require(result["versions"] == VERSIONS
            and '"editable": true' not in (result["direct_url"] or ""),
            "M28f wrong or editable installed artifact")
    subprocess.run([str(python), "-m", "pip", "check"], capture_output=True,
                   text=True, timeout=30, check=True)
    return result["versions"]


def profiler(directory: Path) -> tuple[float, float | None, set[str], list[str]]:
    """Read native NeMo profiler rows and the actual model response."""
    csv_path = directory / ".tmp/nat/examples/default/standardized_data_all.csv"
    raw = read_artifact(csv_path, 1024 * 1024).decode("utf-8")
    rows = list(csv.DictReader(raw.splitlines()))
    require(rows and {"event_type", "event_timestamp", "tool_name", "llm_text_output"}
            <= set(rows[0]), "M28f native profiler columns")
    times = {row["event_type"]: float(row["event_timestamp"])
             for row in rows if row["event_type"] in {"WORKFLOW_START", "WORKFLOW_END"}}
    require(set(times) == {"WORKFLOW_START", "WORKFLOW_END"}
            and times["WORKFLOW_END"] > times["WORKFLOW_START"],
            "M28f profiler completion")
    tools = {row["tool_name"] for row in rows if row["event_type"] == "TOOL_START"}
    tool_starts = [float(row["event_timestamp"]) for row in rows
                   if row["event_type"] == "TOOL_START"]
    tool_ends = [float(row["event_timestamp"]) for row in rows
                 if row["event_type"] == "TOOL_END"]
    require(len(tool_starts) == len(tool_ends) <= 1
            and (not tool_starts or tool_ends[0] >= tool_starts[0]),
            "M28f bounded native tool span")
    tool_seconds = round(tool_ends[0] - tool_starts[0], 3) if tool_starts else None
    outputs = [row["llm_text_output"] for row in rows if row["llm_text_output"]]
    return round(times["WORKFLOW_END"] - times["WORKFLOW_START"], 3), tool_seconds, tools, outputs


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Recheck the selected installed kit and exact native job evidence."""
    require(case in {"m28f-nemo-install", "m28f-nemo-live"}
            and host_profile == "jetson-orin-nano-jp7", "M28f selected host")
    config = document(reference)
    required = {"schema", "wheel", "wheel_sha256", "lock", "lock_sha256", "venv",
                "target_source_root", "target_source_commit", "manifest_sha256",
                "qemu_pid", "rootserver_sha256", "coh_binary", "model_url",
                "model_name", "model_revision", "evidence"}
    require(set(config) == required and config["schema"] == SCHEMA
            and set(config["evidence"]) == EVIDENCE, "M28f reference fields")
    paths = {name: Path(value) for name, value in config["evidence"].items()}
    require(all(path.is_absolute() for path in paths.values()),
            "M28f absolute evidence paths")
    wheel, lock, venv = (Path(config[name]) for name in ("wheel", "lock", "venv"))
    require(all(path.is_absolute() for path in (wheel, lock, venv))
            and digest(wheel, 32 * 1024 * 1024) == config["wheel_sha256"]
            and digest(lock, 1024 * 1024) == config["lock_sha256"],
            "M28f sealed wheel or dependency lock")
    versions = installed(venv)
    doctor = document(paths["doctor"])
    mcp = document(paths["mcp_discovery"])
    a2a = document(paths["a2a_discovery"])
    require(doctor["state"] == "ready" and doctor["native_versions"]
            == {k: v for k, v in VERSIONS.items() if k != "cohesix-nemo-kit"}
            and mcp["state"] == a2a["state"] == "discovered"
            and mcp["scope_count"] >= 2
            and {"gpu.workload.submit", "peft.release"} <= set(a2a["skills"]),
            "M28f installed native client discovery")
    summary: dict[str, Any] = {"schema": "cohesix-m28f-nemo-summary/v1",
                               "case": case, "result": "PASS", "versions": versions,
                               "wheel_sha256": config["wheel_sha256"],
                               "lock_sha256": config["lock_sha256"]}
    if case == "m28f-nemo-live":
        root = Path(config["target_source_root"])
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root,
                                         text=True, timeout=5).strip()
        require(commit == config["target_source_commit"]
                and re.fullmatch(r"[0-9a-f]{40}", commit), "M28f target source")
        manifest = root / "configs/generated/root_task_resolved.json"
        require(digest(manifest, 2 * 1024 * 1024) == config["manifest_sha256"],
                "M28f selected manifest")
        image = qemu_image_identity(config["qemu_pid"], commit, artifact_root=root)
        require(image["rootserver_sha256"] == config["rootserver_sha256"],
                "M28f running KVM image")
        model_url = urlsplit(config["model_url"])
        require(model_url.scheme == "http" and model_url.hostname == "127.0.0.1"
                and model_url.path.rstrip("/") == "/v1"
                and config["model_revision"]
                == "50d427756c6b1b2fe0c0a10f67fbda1fc8e82c1b",
                "M28f pinned local model")
        with urlopen(config["model_url"].rstrip("/") + "/models", timeout=10) as reply:
            catalogue = json.loads(reply.read(65537))
        require(config["model_name"] in {row.get("id") for row in catalogue["data"]},
                "M28f loaded model endpoint")
        cuda = document(paths["cuda_mcp"])
        cuda_proof = document(paths["cuda_verified"])
        peft = document(paths["peft_a2a"])
        cross_mcp = document(paths["mcp_peft_recovery"])
        cross_a2a = document(paths["a2a_cuda_recovery"])
        require(cuda["state"] == "confirmed" and cuda["admission_id"]
                == "m28f-reference-systemd-02" and cuda["effect_replay_allowed"] is False
                and cuda_proof["result"] == "PASS"
                and cuda_proof["admission_id"] == cuda["admission_id"]
                and peft["state"] == "completed"
                and peft["task_id"] == "m28f-a2a-peft-01"
                and peft["provider_verified"] is False
                and cross_mcp["admission_id"] == peft["task_id"]
                and cross_mcp["state"] == "confirmed"
                and cross_a2a["task_id"] == cuda["admission_id"]
                and cross_a2a["state"] == "completed", "M28f original cross-protocol jobs")
        coh = Path(config["coh_binary"])
        verified = json.loads(subprocess.check_output([
            str(coh), "peft", "release", "--deployment",
            str(paths["peft_deployment"]), "verify"], cwd=root, timeout=30))
        require(verified == document(paths["peft_report"], 1024 * 1024)
                and verified["result"]["state"] == "succeeded",
                "M28f shared PEFT verifier")
        refused = subprocess.run([str(coh), "peft", "release", "--deployment",
                                  str(paths["rejected_deployment"]), "verify"],
                                 cwd=root, capture_output=True, text=True, timeout=30)
        failed = document(paths["rejected_report"], 1024 * 1024)
        rejected = document(paths["rejected_a2a"])
        accepted = document(paths["accepted_state"])
        require(refused.returncode != 0 and "unverified native-release" in refused.stderr
                and failed["result"]["state"] == "failed"
                and rejected["state"] == "failed"
                and accepted["generation"] == 9 and accepted["healthy"] is True,
                "M28f rejected candidate did not promote")
        negative = document(paths["negative_action"])
        subject = document(paths["cross_subject"])
        budget = document(paths["budget_denial"])
        uncertain = document(paths["budget_recovery"])
        require(negative["is_error"] is True
                and subject["cross_subject_denied"] is True
                and budget["denied"] is True
                and uncertain["record"]["execution"] == "reserved"
                and uncertain["effect_replay_allowed"] is False
                and uncertain["target_results"] == [],
                "M28f refusal, isolation or uncertain-budget boundary")
        agent_mcp = document(paths["agent_mcp"])
        agent_a2a = document(paths["agent_a2a"])
        direct = document(paths["eval_direct"])
        governed = document(paths["eval_a2a"])
        require(all(row["exit_code"] == 0 for row in
                    (agent_mcp, agent_a2a, direct, governed)),
                "M28f installed model-backed agents and eval")
        direct_seconds, direct_tool_seconds, direct_tools, direct_outputs = profiler(
            Path(direct["output_directory"]))
        governed_seconds, governed_tool_seconds, governed_tools, governed_outputs = profiler(
            Path(governed["output_directory"]))
        require(not direct_tools and "cohesix_tasks__get_task" in governed_tools
                and direct_tool_seconds is None
                and governed_tool_seconds is not None
                and governed_seconds <= 30 and governed_tool_seconds <= 1
                and any(output.strip().lower() == "unknown" for output in direct_outputs)
                and any("is completed" in output.lower()
                        for output in governed_outputs),
                "M28f same-input native profiler comparison")
        summary.update({"proof_class": "live_target", "target_commit": commit,
                        "image": image, "model_name": config["model_name"],
                        "model_revision": config["model_revision"],
                        "cuda_admission_id": cuda["admission_id"],
                        "peft_operation_id": peft["task_id"],
                        "peft_graph_sha256": verified["result"]["graph_sha256"],
                        "rejected_graph_sha256": failed["result"]["graph_sha256"],
                        "direct_seconds": direct_seconds,
                        "governed_seconds": governed_seconds,
                        "governed_tool_seconds": governed_tool_seconds,
                        "budget_unknown": True})
    else:
        summary["proof_class"] = "live_host"
    hash_names = EVIDENCE if case == "m28f-nemo-live" else {
        "doctor", "mcp_discovery", "a2a_discovery"}
    summary["evidence_sha256"] = {name: digest(paths[name], 4 * 1024 * 1024)
                                  for name in sorted(hash_names)}
    state_dir.mkdir(parents=True, exist_ok=True)
    (state_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    return 0
