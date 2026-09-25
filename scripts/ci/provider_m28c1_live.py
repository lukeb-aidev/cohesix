#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Retain exact-source admitted Mac MLX release, rollback and generation-bound vMLX observations from selected hosts.
# Copyright 2026 Lukas Bower
"""Run selected M28c1 cases without replaying an uncertain native phase."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import threading
import time
import tomllib
from typing import Any

from cohesix.auth import resolve_secret_reference
from cohesix.vmlx_governed import GovernedSelection, GovernedVmlxSession
from provider_m28b_live import command, refresh_verifier_clock, result_report
from provider_m28_live import is_generated_derivation, read_artifact
from provider_matrix import require


ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "cohesix-m28c1-live-reference/v1"
COMMON = {"schema", "host_profile", "scenario", "source_commit",
          "source_manifest", "target_manifest_sha256", "target_host",
          "target_source", "target_qemu_pid", "coh_binary", "coh_sha256",
          "gateway_url", "request_auth_ref", "delegated_ticket_ref",
          "wait_seconds", "native_config", "native_config_sha256"}
MLX = COMMON | {"agent_binary", "agent_sha256", "helper", "helper_sha256",
                "deployment", "deployment_sha256", "expected_generation",
                "expected_adapter_sha256"}
VMLX = COMMON | {"verified_release_report", "verified_release_report_sha256",
                 "release_deployment", "release_deployment_sha256",
                 "governed_profile", "governed_profile_sha256",
                 "fused_provenance", "fused_provenance_sha256",
                 "prompts_path", "prompts_sha256", "rollback_report",
                 "rollback_report_sha256", "rollback_deployment",
                 "rollback_deployment_sha256"}


def digest(path: Path, maximum: int = 64 * 1024 * 1024) -> str:
    return hashlib.sha256(read_artifact(path, maximum)).hexdigest()


def _selected(path: Path, host_profile: str, case: str) -> dict[str, Any]:
    value = tomllib.loads(read_artifact(path, 65536).decode())
    expected = MLX if case == "m28c1-mlx-live" else VMLX
    if value.get("target_host") == "local":
        expected = expected | {"qemu_binary", "qemu_sha256"}
    require(set(value) == expected and value["schema"] == SCHEMA,
            "M28c1 selected reference fields")
    require(value["host_profile"] == host_profile == "mac-apple-m4-macos27",
            "M28c1 selected Mac profile")
    require(value["scenario"] in ({"train", "rollback"}
                                  if case == "m28c1-mlx-live" else {"vmlx"}),
            "M28c1 selected scenario")
    require(re.fullmatch(r"[0-9a-f]{40}", value["source_commit"]) is not None
            and re.fullmatch(r"[A-Za-z0-9_.@-]+", value["target_host"]) is not None
            and type(value["target_qemu_pid"]) is int
            and value["target_qemu_pid"] > 0
            and type(value["wait_seconds"]) is int
            and 1 <= value["wait_seconds"] <= 3600,
            "M28c1 source or process identity")
    path_fields = ("source_manifest", "target_source", "coh_binary",
                   "native_config")
    if value["target_host"] == "local":
        path_fields += ("qemu_binary",)
    for key in path_fields:
        require(isinstance(value[key], str) and Path(value[key]).is_absolute(),
                f"M28c1 {key} path")
    for key in expected:
        if key.endswith("_sha256") and key != "expected_adapter_sha256":
            require(isinstance(value[key], str)
                    and re.fullmatch(r"[0-9a-f]{64}", value[key]) is not None,
                    f"M28c1 {key}")
        if key in {"agent_binary", "helper", "deployment", "verified_release_report",
                   "release_deployment", "governed_profile", "fused_provenance",
                   "prompts_path",
                   "rollback_report", "rollback_deployment"}:
            require(isinstance(value[key], str) and Path(value[key]).is_absolute(),
                    f"M28c1 {key} path")
    require(value["gateway_url"].startswith(("http://127.0.0.1:",
                                              "http://localhost:", "https://")),
            "M28c1 authenticated gateway")
    require(value["request_auth_ref"].startswith(("file:", "env:"))
            and value["delegated_ticket_ref"].startswith(("file:", "env:")),
            "M28c1 scoped credential references")
    if case == "m28c1-mlx-live":
        require(type(value["expected_generation"]) is int
                and 0 <= value["expected_generation"] < 2**32
                and (value["expected_adapter_sha256"] == "observed" or
                     re.fullmatch(r"[0-9a-f]{64}",
                                  value["expected_adapter_sha256"]) is not None),
                "M28c1 expected deployment")
    return value


def _exact_source(selected: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"],
                                     cwd=ROOT, text=True).strip()
    changed = subprocess.check_output(["git", "diff", "--name-only", "HEAD", "--"],
                                      cwd=ROOT, text=True).splitlines()
    untracked = subprocess.check_output(
        ["git", "ls-files", "--others", "--exclude-standard"], cwd=ROOT)
    require(commit == selected["source_commit"]
            and all(is_generated_derivation(path) for path in changed)
            and not untracked, "M28c1 exact source required")
    require(digest(Path(selected["source_manifest"]), 8 * 1024 * 1024) ==
            selected["target_manifest_sha256"], "M28c1 manifest changed")
    for name, maximum in (("coh_binary", 64 * 1024 * 1024),
                          ("native_config", 8192)):
        require(digest(Path(selected[name]), maximum) ==
                selected[name.replace("_binary", "") + "_sha256"],
                f"M28c1 {name} changed")
    remote = selected["target_source"]
    require(re.fullmatch(r"[A-Za-z0-9_./-]+", remote) is not None,
            "M28c1 remote source path")
    if selected["target_host"] == "local":
        require(Path(remote) == ROOT, "M28c1 local selected source path")
        require(digest(Path(selected["qemu_binary"]), 128 * 1024 * 1024) ==
                selected["qemu_sha256"], "M28c1 pinned QEMU changed")
        return commit, _mac_qemu_image_identity(selected, commit)
    script = ("import json,sys; from pathlib import Path; "
              "sys.path.insert(0,sys.argv[1]+'/scripts/ci'); "
              "from provider_m28_live import qemu_image_identity; "
              "print(json.dumps(qemu_image_identity(int(sys.argv[2]),sys.argv[3],"
              "artifact_root=Path(sys.argv[1]))))")
    command_line = " ".join(shlex.quote(part) for part in
                            ("python3", "-c", script, remote,
                             str(selected["target_qemu_pid"]), commit))
    checked = subprocess.run(["ssh", selected["target_host"], command_line],
                             capture_output=True, text=True, check=False,
                             timeout=30)
    require(checked.returncode == 0 and len(checked.stdout) <= 8192,
            "M28c1 remote exact QEMU identity")
    return commit, json.loads(checked.stdout)


def _mac_qemu_image_identity(selected: dict[str, Any], commit: str) -> dict[str, str]:
    """Bind the live pinned HVF process to this source and built image."""
    observed = subprocess.run(
        ["ps", "-ww", "-p", str(selected["target_qemu_pid"]), "-o", "command="],
        capture_output=True, text=True, check=False, timeout=10,
    )
    require(observed.returncode == 0 and 0 < len(observed.stdout) <= 16384,
            "M28c1 live Mac QEMU process")
    args = shlex.split(observed.stdout.strip())
    require(args and Path(args[0]).resolve() == Path(selected["qemu_binary"]).resolve()
            and Path(args[0]).name == "qemu-system-aarch64"
            and any(args[index:index + 2] == ["-accel", "hvf"]
                    for index in range(len(args) - 1)),
            "M28c1 pinned HVF process identity")

    def option(name: str) -> Path:
        positions = [index for index, arg in enumerate(args[:-1]) if arg == name]
        require(len(positions) == 1, f"M28c1 QEMU {name} identity")
        path = Path(args[positions[0] + 1])
        require(path.is_absolute() and path.is_relative_to(ROOT / "out"),
                f"M28c1 QEMU {name} path")
        return path

    elfloader = option("-kernel")
    cpio = option("-initrd")
    loaders = [arg for index, arg in enumerate(args)
               if index > 0 and args[index - 1] == "-device"
               and arg.startswith("loader,file=") and ",addr=0x80000000," in arg]
    require(len(loaders) == 1, "M28c1 QEMU rootserver loader")
    rootserver = Path(loaders[0].split(",", 2)[1].removeprefix("file="))
    require(rootserver.is_absolute() and rootserver.is_relative_to(ROOT / "out")
            and rootserver.name == "rootserver", "M28c1 QEMU rootserver path")
    image = read_artifact(rootserver, 32 * 1024 * 1024)
    markers = re.findall(rb"\[BUILD\] ([0-9a-f]{12})(?:-dirty)? ", image)
    require(markers == [commit[:12].encode()], "M28c1 live rootserver source")
    return {"qemu_pid": str(selected["target_qemu_pid"]),
            "qemu_sha256": selected["qemu_sha256"],
            "rootserver_sha256": hashlib.sha256(image).hexdigest(),
            "elfloader_sha256": digest(elfloader), "cpio_sha256": digest(cpio)}


def _run_release(selected: dict[str, Any], state_dir: Path,
                 commit: str, qemu: dict[str, Any]) -> dict[str, Any]:
    for name, maximum in (("agent_binary", 64 * 1024 * 1024),
                          ("helper", 262144), ("deployment", 262144)):
        require(digest(Path(selected[name]), maximum) ==
                selected[name.replace("_binary", "") + "_sha256"],
                f"M28c1 {name} changed")
    native = json.loads(read_artifact(Path(selected["native_config"]), 8192))
    require(native["schema"] == "cohesix-mlx-native/v1",
            "M28c1 native Mac profile")
    deployment = json.loads(read_artifact(Path(selected["deployment"]), 262144))
    operation = deployment["request"]["operation_id"]
    require(re.fullmatch(r"[A-Za-z0-9_-]{1,64}", operation) is not None,
            "M28c1 operation identity")
    root = Path(native["root"])
    require(root.is_absolute() and root.stat().st_mode & 0o077 == 0
            and not (root / "operations" / operation).exists(),
            "M28c1 fresh private operation")
    trust_path = Path(deployment["execution"]["trust"])
    trust = json.loads(read_artifact(trust_path, 65536))
    token = resolve_secret_reference(selected["request_auth_ref"])
    coh = Path(selected["coh_binary"])
    deployment_path = Path(selected["deployment"])
    ticket = selected["delegated_ticket_ref"]
    gateway = selected["gateway_url"]
    plan = result_report(command(coh, "plan", deployment_path, 60,
                                 token, ticket, gateway))
    require(plan["operation_id"] == operation and not plan["submitted"],
            "M28c1 release plan identity")
    stopped: dict[str, Any] = {}
    done = threading.Event()
    watcher = None
    if selected["scenario"] == "rollback":
        label = native["service_label"]
        def interrupt() -> None:
            load = root / "operations" / operation / "load.json"
            deadline = time.monotonic() + selected["wait_seconds"]
            while not done.is_set() and time.monotonic() < deadline:
                if load.exists():
                    observation = json.loads(read_artifact(load, 16384))
                    if observation.get("succeeded") is True:
                        removed = subprocess.run(["/bin/launchctl", "remove", label],
                                                 capture_output=True, check=False,
                                                 timeout=15)
                        stopped.update({"load_sha256": digest(load, 16384),
                                        "remove_exit": removed.returncode,
                                        "observed_unix_ms": int(time.time() * 1000)})
                        return
                done.wait(0.02)
        watcher = threading.Thread(target=interrupt, daemon=True)
        watcher.start()
    applied = command(coh, "apply", deployment_path, 90, token, ticket, gateway)
    (state_dir / "apply.stdout.json").write_text(applied.stdout)
    (state_dir / "apply.stderr.txt").write_text(applied.stderr)
    started = time.monotonic()
    report = None
    while time.monotonic() - started < selected["wait_seconds"]:
        refresh_verifier_clock(trust_path, trust)
        watched = command(coh, "watch", deployment_path, 60, token, ticket, gateway)
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
    require(report is not None and report["result"] is not None,
            "M28c1 original outcome unresolved")
    native_journal = report["result"]["native"]
    expected_state = "succeeded" if selected["scenario"] == "train" else "recovered_failure"
    require(report["operation_id"] == operation
            and report["result"]["state"] == expected_state
            and native_journal["operation_id"] == operation,
            "M28c1 signed native outcome")
    phases = {row["phase"]: row["result"] for row in native_journal["phases"]}
    require(phases["validate"]["succeeded"]
            and phases["evaluate"]["succeeded"]
            and phases["scan"]["succeeded"]
            and phases["load"]["succeeded"],
            "M28c1 native release phases")
    if selected["scenario"] == "train":
        require(phases["train"]["succeeded"]
                and phases["canary"]["succeeded"]
                and phases["promote"]["succeeded"],
                "M28c1 trained candidate promotion")
    else:
        require(stopped.get("remove_exit") == 0
                and phases["rollback"]["succeeded"]
                and phases["rollback"]["detail"]["candidate_release"] == "failed",
                "M28c1 governed rollback")
    expected = selected["expected_adapter_sha256"]
    if expected == "observed":
        expected = phases["scan"]["detail"]["adapter_sha256"]
    accepted = json.loads(read_artifact(root / "accepted.json", 8192))
    require(accepted["generation"] == selected["expected_generation"]
            and accepted["adapter_sha256"] == expected,
            "M28c1 accepted generation")
    refresh_verifier_clock(trust_path, trust)
    verified = command(coh, "verify", deployment_path, 60, token, ticket, gateway)
    require((verified.returncode == 0) == (selected["scenario"] == "train"),
            "M28c1 shared verifier result")
    summary = {"schema": "cohesix-m28c1-live-report/v1",
               "case": "m28c1-mlx-live", "scenario": selected["scenario"],
               "source_commit": commit, "qemu": qemu,
               "operation_id": operation,
               "signed_graph_sha256": report["result"]["graph_sha256"],
               "native_state": native_journal["state"],
               "phases": list(phases),
               "accepted_generation": accepted["generation"],
               "accepted_adapter_sha256": accepted["adapter_sha256"],
               "interruption": stopped or None}
    (state_dir / "verified-report.json").write_text(
        json.dumps(report, sort_keys=True, indent=2) + "\n")
    return summary


def _run_vmlx(selected: dict[str, Any], state_dir: Path,
              commit: str, qemu: dict[str, Any]) -> dict[str, Any]:
    for name in ("verified_release_report", "governed_profile",
                 "fused_provenance",
                 "prompts_path", "rollback_report", "release_deployment",
                 "rollback_deployment"):
        require(digest(Path(selected[name]), 262144) == selected[name + "_sha256"],
                f"M28c1 {name} changed")
    release = json.loads(read_artifact(Path(selected["verified_release_report"]), 262144))
    rollback = json.loads(read_artifact(Path(selected["rollback_report"]), 262144))
    require(release["result"]["state"] == "succeeded"
            and rollback["result"]["state"] == "recovered_failure"
            and release["result"]["graph_sha256"] !=
            rollback["result"]["graph_sha256"],
            "M28c1 separate verified release and rollback")
    coh = Path(selected["coh_binary"])
    token = resolve_secret_reference(selected["request_auth_ref"])
    for report, path in ((release, selected["release_deployment"]),
                         (rollback, selected["rollback_deployment"])):
        deployment = json.loads(read_artifact(Path(path), 262144))
        trust_path = Path(deployment["execution"]["trust"])
        trust = json.loads(read_artifact(trust_path, 65536))
        refresh_verifier_clock(trust_path, trust)
        observed = result_report(command(
            coh, "watch", Path(path), 60, token,
            selected["delegated_ticket_ref"], selected["gateway_url"]))
        require(observed["operation_id"] == report["operation_id"]
                and observed["result"] == report["result"],
                "M28c1 retained release report differs from shared verifier")
    selection = GovernedSelection.from_profile(Path(selected["governed_profile"]))
    fusion = json.loads(read_artifact(Path(selected["fused_provenance"]), 262144))
    released = {row["phase"]: row["result"]
                for row in release["result"]["native"]["phases"]}
    restored = {row["phase"]: row["result"]
                for row in rollback["result"]["native"]["phases"]}
    accepted = json.loads(read_artifact(selection.accepted_path, 8192))
    require(fusion["source_model_sha256"] ==
            released["validate"]["detail"]["context"]["base_sha256"]
            and fusion["source_adapter_sha256"] ==
            released["scan"]["detail"]["adapter_sha256"]
            and fusion["fused_model_sha256"] == selection.source_sha256,
            "M28c1 fused model provenance")
    require(selection.release_graph_sha256 == release["result"]["graph_sha256"]
            and released["promote"]["succeeded"]
            and restored["rollback"]["succeeded"]
            and restored["rollback"]["detail"]["candidate_release"] == "failed"
            and released["promote"]["detail"]["accepted"] == accepted
            and restored["rollback"]["detail"]["accepted"] == accepted
            and accepted["generation"] == selection.generation
            and accepted["adapter_sha256"] == selection.adapter_sha256
            and accepted["served_artifact_sha256"] ==
            selection.served_artifact_sha256
            and restored["rollback"]["detail"]["canary"]["healthy"] is True,
            "M28c1 vMLX signed incumbent and rollback binding")
    prompts = json.loads(read_artifact(Path(selected["prompts_path"]), 8192))
    require(isinstance(prompts, list)
            and len(prompts) == len(selection.expected_output_sha256)
            and all(isinstance(text, str) and 1 <= len(text.encode()) <= 256
                    for text in prompts), "M28c1 vMLX frozen prompts")
    observed = []
    session = GovernedVmlxSession(selection)
    try:
        session.start()
        for index, prompt in enumerate(prompts):
            observed.append(session.generate(prompt, 64, index))
        changed = selection.__class__(**{
            **selection.__dict__, "generation": selection.generation + 1})
        try:
            changed.validate()
        except ValueError as error:
            require("accepted_generation_changed" in str(error),
                    "M28c1 changed generation refusal")
        else:
            raise ValueError("M28c1 changed generation accepted")
    finally:
        session.close()
    return {"schema": "cohesix-m28c1-live-report/v1",
            "case": "m28c1-vmlx-live", "source_commit": commit,
            "qemu": qemu, "release_graph_sha256": selection.release_graph_sha256,
            "rollback_graph_sha256": rollback["result"]["graph_sha256"],
            "accepted_generation": selection.generation,
            "source_sha256": selection.source_sha256,
            "responses": [row["reply"] for row in observed],
            "quality_resources": [{"elapsed_ms": row["elapsed_ms"],
                                   "rss_bytes": row["peak_observed_rss_bytes"]}
                                  for row in observed],
            "custody_pid": observed[-1]["custody"]["native"]["pid"],
            "changed_generation_refused": True,
            "rollback_incumbent_observed":
            restored["rollback"]["detail"]["canary"]["healthy"]}


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Run one fresh selected live case and retain only bounded source-bound facts."""
    require(case in {"m28c1-mlx-live", "m28c1-vmlx-live"}, "M28c1 case")
    selected = _selected(reference, host_profile, case)
    require(not state_dir.exists(), "M28c1 evidence directory already exists")
    commit, qemu = _exact_source(selected)
    state_dir.mkdir(mode=0o700, parents=True)
    summary = (_run_release(selected, state_dir, commit, qemu)
               if case == "m28c1-mlx-live" else
               _run_vmlx(selected, state_dir, commit, qemu))
    (state_dir / "summary.json").write_text(
        json.dumps(summary, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"result": "PASS",
                      "summary_sha256": digest(state_dir / "summary.json", 16384),
                      "case": case}, sort_keys=True))
    return 0
