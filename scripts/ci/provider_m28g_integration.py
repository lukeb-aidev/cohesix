#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Recheck four live gateway modes and one original native job across MCP, A2A and REST.
# Copyright 2026 Lukas Bower
"""Qualify protocol composition only when the installed gateway reaches an exact live Queen."""

from __future__ import annotations

import csv
import hashlib
import ipaddress
import io
import json
import math
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import time
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import urlopen

from cohesix.auth import resolve_secret_reference
from cohesix.backends import RestBackend
from cohesix.errors import CohesixError

from provider_m28_live import qemu_image_identity, read_artifact
from provider_m28d_live import mcp_result
from provider_m28e_live import recovery, task
from provider_matrix import require

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from release_qualify import inspect_bundle, read_result  # noqa: E402

SCHEMA = "cohesix-m28g-integration-reference/v1"
MODES = {
    "both": ({}, True, True),
    "mcp-only": ({"HIVE_GATEWAY_A2A_ENABLED": "false"}, True, False),
    "a2a-only": ({"HIVE_GATEWAY_MCP_ENABLED": "false"}, False, True),
    "neither": ({"HIVE_GATEWAY_AGENT_PROTOCOLS_ENABLED": "false"}, False, False),
}
EVIDENCE = {"mcp", "a2a_task", "a2a_recovery", "native_cuda", "refusals",
            "queen_loss", "latency"}


def document(path: Path, maximum: int = 1024 * 1024) -> dict[str, Any]:
    """Read a bounded regular JSON record without following a symlink."""
    value = json.loads(read_artifact(path, maximum))
    require(isinstance(value, dict), "M28g integration record must be an object")
    return value


def sha(path: Path, maximum: int = 128 * 1024 * 1024) -> str:
    """Hash exact installed code or a bounded raw observation."""
    return hashlib.sha256(read_artifact(path, maximum)).hexdigest()


def current_path_for(binding: dict[str, Any]) -> str:
    """Derive Root's unambiguous v1 ticket correlation path from both identity fields."""
    ticket = binding.get("ticket_id")
    key = binding.get("idempotency_key")
    require(isinstance(ticket, str) and isinstance(key, str)
            and 1 <= len(ticket.encode()) <= 96
            and 1 <= len(key.encode()) <= 96,
            "M28g original ticket correlation fields")
    ticket_raw, key_raw = ticket.encode(), key.encode()
    material = (b"host-ticket-correlation/v1\0" + len(ticket_raw).to_bytes(2, "big")
                + ticket_raw + len(key_raw).to_bytes(2, "big") + key_raw)
    return "/host/tickets/current/" + hashlib.sha256(material).hexdigest()


def raw_evidence(rows: dict[str, Any]) -> dict[str, str]:
    """Require at least one unchanged raw attachment for an external observation."""
    require(isinstance(rows, dict) and 1 <= len(rows) <= 16,
            "M28g raw observation inventory")
    hashes = {}
    for name, record in rows.items():
        require(isinstance(name, str) and re.fullmatch(r"[a-z][a-z0-9_-]{0,63}", name)
                and isinstance(record, dict)
                and set(record) == {"path", "sha256", "size"},
                "M28g raw observation fields")
        path = Path(record["path"])
        require(path.is_absolute() and type(record["size"]) is int
                and 0 < record["size"] <= 16 * 1024 * 1024
                and isinstance(record["sha256"], str)
                and re.fullmatch(r"[0-9a-f]{64}", record["sha256"]),
                "M28g raw observation bounds")
        data = read_artifact(path, 16 * 1024 * 1024)
        require(len(data) == record["size"]
                and hashlib.sha256(data).hexdigest() == record["sha256"],
                "M28g raw observation changed")
        hashes[name] = record["sha256"]
    return hashes


def validate_latency_samples(latency: dict[str, Any]) -> None:
    """Recompute each nearest-rank p95 from the retained bounded call samples."""
    sample_record = latency["raw_evidence"].get("samples")
    require(isinstance(sample_record, dict), "M28g latency sample CSV missing")
    raw = read_artifact(Path(sample_record["path"]), 16 * 1024 * 1024)
    reader = csv.DictReader(io.StringIO(raw.decode("utf-8")))
    require(reader.fieldnames == ["mode", "protocol", "sample", "elapsed_ms", "status"],
            "M28g latency sample columns")
    observed: dict[tuple[str, str], dict[int, float]] = {}
    sample_total = 0
    for row in reader:
        require(set(row) == set(reader.fieldnames)
                and row["mode"] in MODES
                and row["protocol"] in {"rest", "mcp", "a2a"}
                and row["status"] == "ok", "M28g latency sample identity or refusal")
        try:
            index = int(row["sample"])
            elapsed = float(row["elapsed_ms"])
        except (ValueError, TypeError) as error:
            raise ValueError("M28g malformed latency sample") from error
        require(1 <= index <= 4096 and math.isfinite(elapsed)
                and 0 <= elapsed <= 600_000,
                "M28g latency sample bound")
        key = (row["mode"], row["protocol"])
        samples = observed.setdefault(key, {})
        require(index not in samples, "M28g duplicate latency sample")
        samples[index] = elapsed
        sample_total += 1
        require(sample_total <= 4 * 3 * 4096,
                "M28g latency sample count")
    expected = {(mode, "rest") for mode in MODES}
    expected.update((mode, protocol) for mode, (_, mcp, a2a) in MODES.items()
                    for protocol, enabled in (("mcp", mcp), ("a2a", a2a)) if enabled)
    require(set(observed) == expected, "M28g missing or disabled protocol samples")
    for (mode, protocol), samples in observed.items():
        count = latency["modes"][mode]["samples"]
        require(len(samples) == count and set(samples) == set(range(1, count + 1)),
                "M28g incomplete latency window")
        values = sorted(samples.values())
        percentile = round(values[math.ceil(0.95 * count) - 1], 3)
        claimed = latency["modes"][mode][f"{protocol}_p95_ms"]
        require(abs(percentile - claimed) <= 0.001,
                "M28g reported p95 differs from raw samples")


def validate_external(config: dict[str, Any], source: str,
                      admission: str, result_sha256: str) -> dict[str, Any]:
    """Recheck SDK identity, native verifier, refusals and fixed latency limits."""
    require(isinstance(config.get("evidence"), dict),
            "M28g evidence map required")
    paths = {name: Path(value) for name, value in config["evidence"].items()}
    require(set(paths) == EVIDENCE and all(path.is_absolute() for path in paths.values()),
            "M28g required live evidence paths")
    mcp = mcp_result(paths["mcp"], "succeeded")
    require(mcp["record"]["binding"]["admission_id"] == admission
            and mcp["record"]["result_sha256"] == result_sha256
            and current_path_for(mcp["record"]["binding"]) == config["current_path"],
            "M28g MCP original native result")
    a2a = task(paths["a2a_task"], admission)
    recovered = recovery(paths["a2a_recovery"], admission)
    require(a2a["metadata"]["resultSha256"] == result_sha256
            and recovered["record"]["binding"] == mcp["record"]["binding"]
            and recovered["record"]["result_sha256"] == result_sha256,
            "M28g A2A shared result or identity")
    native = document(paths["native_cuda"])
    require(native.get("result") == "PASS"
            and native.get("source_commit") == source
            and native.get("admission_id") == admission
            and native.get("terminal") == mcp["target_results"][-1],
            "M28g independent native CUDA output")
    refusals = document(paths["refusals"])
    require(refusals.get("schema") == "cohesix-m28g-refusals/v1"
            and refusals.get("source_commit") == source
            and refusals.get("admission_id") == admission
            and refusals.get("cross_subject_denied") is True
            and refusals.get("revoked_scope_denied") is True
            and refusals.get("budget_refused_no_effect") is True,
            "M28g subject, revocation or budget refusal")
    refused_raw = raw_evidence(refusals.get("raw_evidence"))
    queen = document(paths["queen_loss"])
    require(queen.get("schema") == "cohesix-m28g-queen-loss/v1"
            and queen.get("source_commit") == source
            and queen.get("admission_id") == admission
            and queen.get("refused_new_effect") is True
            and queen.get("recovered_original_id") is True
            and queen.get("no_vm_persistence_claim") is True,
            "M28g Queen loss boundary")
    queen_raw = raw_evidence(queen.get("raw_evidence"))
    latency = document(paths["latency"])
    require(latency.get("schema") == "cohesix-m28g-protocol-latency/v1"
            and latency.get("source_commit") == source
            and set(latency.get("modes", {})) == set(MODES),
            "M28g frozen latency windows")
    require(isinstance(latency.get("raw_evidence"), dict)
            and {"samples", "client"} <= set(latency["raw_evidence"]),
            "M28g latency samples and client log required")
    for mode, (_, mcp_on, a2a_on) in MODES.items():
        row = latency["modes"][mode]
        require(isinstance(row, dict)
                and set(row) == {"samples", "rest_p95_ms", "mcp_p95_ms", "a2a_p95_ms"}
                and type(row["samples"]) is int and row["samples"] >= 30
                and type(row["rest_p95_ms"]) in {int, float}
                and math.isfinite(row["rest_p95_ms"])
                and row["rest_p95_ms"] >= 0,
                "M28g latency sample or direct REST baseline")
        limit = row["rest_p95_ms"] + max(100, 0.25 * row["rest_p95_ms"])
        for protocol, enabled in (("mcp", mcp_on), ("a2a", a2a_on)):
            measured = row[f"{protocol}_p95_ms"]
            require((type(measured) in {int, float}
                     and math.isfinite(measured) and 0 <= measured <= limit)
                    if enabled else measured is None,
                    f"M28g {mode} {protocol} control latency")
    latency_raw = raw_evidence(latency.get("raw_evidence"))
    validate_latency_samples(latency)
    return {"native_result_sha256": result_sha256,
            "refusal_raw_sha256": refused_raw, "queen_loss_raw_sha256": queen_raw,
            "latency_raw_sha256": latency_raw,
            "evidence_sha256": {name: sha(path, 1024 * 1024)
                                for name, path in paths.items()}}


def http_code(port: int, path: str) -> int | None:
    """Read only a loopback route's response status under a bounded deadline."""
    try:
        with urlopen(f"http://127.0.0.1:{port}{path}", timeout=2) as response:
            response.read(256)
            return response.status
    except HTTPError as error:
        error.read(256)
        return error.code
    except URLError:
        return None


def free_port() -> int:
    """Ask the OS for a short-lived loopback port for the next gateway process."""
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def private_target(value: str) -> bool:
    """Keep test credentials on a literal private or loopback Queen address."""
    try:
        address = ipaddress.ip_address(value)
    except ValueError:
        return False
    return address.is_private or address.is_loopback


def target_binding(config: dict[str, Any], source: str,
                   host_profile: str) -> dict[str, Any]:
    """Keep physical media proof distinct from a source-bound live KVM loader."""
    if config["target_kind"] == "qemu":
        require(host_profile == "jetson-orin-nano-jp7"
                and config["target_profile"] == "qemu-smp-production"
                and type(config["target_qemu_pid"]) is int
                and config["target_qemu_pid"] > 0
                and config["target_proof"] is None,
                "M28g selected Jetson KVM identity")
        image = qemu_image_identity(config["target_qemu_pid"], source,
                                    artifact_root=Path(config["source_root"]))
        require(image["rootserver_sha256"] == config["target_image_sha256"],
                "M28g live KVM image changed")
        return {"kind": "qemu", **image}
    require(config["target_kind"] == "pi4"
            and config["target_profile"] == "pi4-production"
            and config["target_qemu_pid"] is None
            and isinstance(config["target_proof"], str)
            and Path(config["target_proof"]).is_absolute(),
            "M28g physical Pi proof path")
    proof = read_result(Path(config["target_proof"]), "pi4")
    require(proof["source_commit"] == source
            and proof["media_image_sha256"] == config["target_image_sha256"]
            and proof["provisioning_verified"] is True,
            "M28g selected SD image qualification")
    return {"kind": "pi4", "media_image_sha256": proof["media_image_sha256"],
            "qualification_sha256": sha(Path(config["target_proof"]), 1024 * 1024)}


def probe_modes(config: dict[str, Any], state_dir: Path) -> list[dict[str, Any]]:
    """Restart the exact installed gateway four times against one live Queen."""
    auth = resolve_secret_reference(config["request_auth_ref"])
    delegated = resolve_secret_reference(config["delegated_ticket_ref"])
    gateway = Path(config["gateway_binary"])
    observations = []
    for mode, (disabled, mcp_on, a2a_on) in MODES.items():
        port = free_port()
        command = [str(gateway), "--bind", f"127.0.0.1:{port}",
                   "--tcp-host", config["target_host"],
                   "--tcp-port", str(config["target_port"]),
                   "--worker-runtime-profile", config["target_profile"],
                   "--delegation-key-ref", config["delegation_key_ref"],
                   "--standing-ledger", config["standing_ledger"],
                   "--standing-scopes", config["standing_scopes"]]
        environment = {key: os.environ[key] for key in
                       ("PATH", "HOME", "TMPDIR", "LANG") if key in os.environ}
        environment.update({"COH_AUTH_TOKEN": config["queen_auth_ref"],
                            "COH_TICKET": config["gateway_ticket_ref"],
                            "HIVE_GATEWAY_REQUEST_AUTH_TOKEN": config["request_auth_ref"],
                            **disabled})
        log = state_dir / f"gateway-{mode}.log"
        with log.open("xb") as stream:
            process = subprocess.Popen(command, env=environment, cwd=config["source_root"],
                                       stdin=subprocess.DEVNULL, stdout=stream,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            try:
                deadline = time.monotonic() + 20
                while http_code(port, "/docs") != 200:
                    require(process.poll() is None and time.monotonic() < deadline,
                            f"M28g {mode} gateway failed to start")
                    time.sleep(0.1)
                codes = {"mcp": http_code(port, "/mcp"),
                         "a2a_card": http_code(port, "/.well-known/agent-card.json"),
                         "a2a": http_code(port, "/a2a")}
                require(codes == {"mcp": 401 if mcp_on else 404,
                                  "a2a_card": 401 if a2a_on else 404,
                                  "a2a": 405 if a2a_on else 404},
                        f"M28g {mode} endpoint selection")
                backend = RestBackend(f"http://127.0.0.1:{port}", timeout_s=5,
                                      request_auth_token=auth,
                                      delegated_ticket=delegated, max_attempts=1)
                while True:
                    try:
                        boot = backend.read_file("/proc/boot", 8192).decode()
                        break
                    except (CohesixError, OSError):
                        require(process.poll() is None and time.monotonic() < deadline,
                                f"M28g {mode} authenticated Queen unavailable")
                        time.sleep(0.2)
                require(boot.splitlines().count(
                    f"manifest.sha256={config['target_manifest_sha256']}") == 1,
                    f"M28g {mode} target manifest mismatch")
                result = backend.read_file(config["current_path"], 8192)
                require(result.decode().startswith(
                    "HOST_TICKET_CURRENT schema=host-ticket-current/v1 state=confirmed ")
                    and len(result.splitlines()) == 1
                    and hashlib.sha256(result).hexdigest() == config["current_sha256"],
                        f"M28g {mode} original terminal changed")
                observations.append({"mode": mode, "http_codes": codes,
                                     "boot_sha256": hashlib.sha256(boot.encode()).hexdigest(),
                                     "current_sha256": config["current_sha256"]})
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
        if observations and observations[-1]["mode"] == mode:
            observations[-1]["gateway_log_sha256"] = sha(log, 4 * 1024 * 1024)
    return observations


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Join exact installed code, live four-mode operation and native client proof."""
    require(case == "m28g-integration-live"
            and host_profile in {"mac-apple-m4-macos27", "jetson-orin-nano-jp7"},
            "M28g selected integration host")
    config = document(reference, 65536)
    required = {"schema", "source_root", "source_commit", "host_bundle",
                "gateway_binary", "gateway_sha256", "target_host", "target_port",
                "target_profile", "target_kind", "target_qemu_pid", "target_proof",
                "target_image_sha256", "target_manifest_sha256", "queen_auth_ref",
                "request_auth_ref", "delegated_ticket_ref", "gateway_ticket_ref",
                "delegation_key_ref", "standing_ledger", "standing_scopes",
                "current_path", "current_sha256", "admission_id", "result_sha256",
                "evidence"}
    require(set(config) == required and config["schema"] == SCHEMA
            and re.fullmatch(r"[0-9a-f]{40}", config["source_commit"])
            and config["target_profile"] in {"qemu-smp-production", "pi4-production"}
            and config["target_kind"] in {"qemu", "pi4"}
            and type(config["target_port"]) is int
            and 1 <= config["target_port"] <= 65535
            and isinstance(config["target_host"], str)
            and private_target(config["target_host"])
            and re.fullmatch(r"/host/tickets/current/[0-9a-f]{64}", config["current_path"])
            and re.fullmatch(r"[A-Za-z0-9._-]{1,96}", config["admission_id"])
            and all(re.fullmatch(r"[0-9a-f]{64}", config[name])
                    for name in ("gateway_sha256", "target_image_sha256",
                                 "target_manifest_sha256",
                                 "current_sha256", "result_sha256")),
            "M28g integration reference fields")
    root = Path(config["source_root"])
    bundle = Path(config["host_bundle"])
    gateway = Path(config["gateway_binary"])
    require(all(path.is_absolute() for path in (root, bundle, gateway))
            and all(Path(config[name]).is_absolute()
                    for name in ("standing_ledger", "standing_scopes")),
            "M28g installed source and code paths")
    for key in ("queen_auth_ref", "request_auth_ref", "delegated_ticket_ref",
                "gateway_ticket_ref", "delegation_key_ref"):
        require(isinstance(config[key], str)
                and config[key].startswith(("env:", "file:")),
                f"M28g {key} must be a private reference")
    current = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"],
                                      text=True, timeout=5).strip()
    require(current == config["source_commit"], "M28g source changed")
    suffix = "MacOS" if host_profile == "mac-apple-m4-macos27" else "linux"
    require(bundle.name == f"Cohesix-1.2.0-{suffix}", "M28g selected host bundle")
    bundle_record = inspect_bundle(bundle, bundle.with_name(bundle.name + ".tar.gz"))
    require(bundle_record["source_commit"] == current
            and sha(gateway) == config["gateway_sha256"]
            == sha(bundle / "bin/hive-gateway"),
            "M28g installed gateway differs from qualified bundle")
    require(all(Path(config[name]).is_file() and not Path(config[name]).is_symlink()
                for name in ("standing_ledger", "standing_scopes")),
            "M28g private standing inputs")
    target = target_binding(config, current, host_profile)
    external = validate_external(config, current, config["admission_id"],
                                 config["result_sha256"])
    state_dir.mkdir(parents=True, exist_ok=False, mode=0o700)
    summary: dict[str, Any] = {"schema": "cohesix-m28g-integration-live-summary/v1",
                               "case": case, "result": "IN_PROGRESS",
                               "proof_class": "live_target", "source_commit": current,
                               "host_profile": host_profile,
                               "gateway_sha256": config["gateway_sha256"]}
    (state_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    modes = probe_modes(config, state_dir)
    require(len({row["boot_sha256"] for row in modes}) == 1,
            "M28g target changed across protocol modes")
    summary.update({"result": "PASS", "target_manifest_sha256":
                    config["target_manifest_sha256"],
                    "admission_id": config["admission_id"], "modes": modes,
                    "target": target,
                    "host_bundle_sha256": bundle_record["archive"]["sha256"],
                    **external})
    (state_dir / "summary.json").write_text(json.dumps(summary, sort_keys=True, indent=2) + "\n")
    return 0
