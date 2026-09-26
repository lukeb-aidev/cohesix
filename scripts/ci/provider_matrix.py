#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Execute bounded selected conformance contracts including Mac MLX release with exact source hashes and proof limits.
# Copyright 2026 Lukas Bower
"""Matrix selection is not permission to infer native execution from host tests."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import sys
import time
import tomllib
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
LIFECYCLE = {
    "discover",
    "preflight",
    "execute",
    "observe",
    "verify",
    "compensate",
    "export_evidence",
}
GROUPS = {
    "executors",
    "evidence",
    "observability",
    "packaging",
    "identity",
    "registry",
    "playbooks",
    "perf",
}
LANES = {"mock", "dry_run", "live_safe", "negative", "receipt", "recovery", "package"}


def require(condition: bool, message: str) -> None:
    """Reject malformed policy before any process or state directory is created."""
    if not condition:
        raise ValueError(message)


def bounded_read(path: Path, maximum: int) -> bytes:
    """Read only regular non-symlink inputs within the explicit byte bound."""
    require(
        path.is_file() and not path.is_symlink(), "matrix input must be a regular file"
    )
    with path.open("rb") as stream:
        data = stream.read(maximum + 1)
    require(len(data) <= maximum, "matrix input exceeds byte bound")
    return data


def load_matrix(path: Path, contract: dict[str, Any]) -> dict[str, Any]:
    """Bind all selected ids and exact profile requirements to generated truth."""
    raw = bounded_read(path, 65536)
    matrix = tomllib.loads(raw.decode("utf-8"))
    require(
        set(matrix)
        == {
            "schema",
            "registry",
            "maximum_cases",
            "maximum_output_bytes",
            "maximum_case_seconds",
            "profiles",
            "required_lifecycle",
            "proof_lanes",
            "cases",
        },
        "unknown or missing matrix field",
    )
    require(
        matrix["schema"] == "cohesix-provider-conformance-matrix/v1", "matrix schema"
    )
    require(
        matrix["registry"] == "configs/generated/provider_registry.json",
        "matrix registry owner",
    )
    for field, maximum in [
        ("maximum_cases", 64),
        ("maximum_output_bytes", 1048576),
        ("maximum_case_seconds", 600),
    ]:
        require(
            type(matrix[field]) is int and 1 <= matrix[field] <= maximum,
            "matrix bound: " + field,
        )
    for field, expected in [("required_lifecycle", LIFECYCLE), ("proof_lanes", LANES)]:
        values = matrix[field]
        require(
            isinstance(values, list)
            and len(values) == len(expected)
            and set(values) == expected,
            "matrix obligations: " + field,
        )
    profiles = {row["id"] for row in contract["contract"]["profiles"]}
    selected = matrix["profiles"]
    require(
        isinstance(selected, list)
        and bool(selected)
        and len(selected) <= 16
        and len(selected) == len(set(selected))
        and set(selected) <= profiles,
        "unknown/duplicate matrix profile",
    )
    families = {row["id"] for row in contract["contract"]["families"]}
    surfaces = {row["id"] for row in contract["contract"]["integration_surfaces"]}
    cases = matrix["cases"]
    require(
        isinstance(cases, list) and 1 <= len(cases) <= matrix["maximum_cases"],
        "matrix case count",
    )
    seen = set()
    for case in cases:
        required = {"id", "group", "providers", "proof_class", "lane"}
        live = case.get("id") in {"m28-jobs-live", "m28-authority-live",
                                    "m28a-workloads-live", "m28a-recovery-live",
                                    "m28b-peft-live", "m28b-serving-live",
                                    "m28c-platform-live", "m28c1-mlx-live",
                                    "m28c1-vmlx-live", "m28d-mcp-live",
                                    "m28e-a2a-live", "m28f-nemo-install",
                                    "m28f-nemo-live"}
        optional = {"surface", "runner"} if live else {"surface", "command"}
        require(set(case) >= required | {"runner" if live else "command"}
                and set(case) <= required | optional,
                "matrix case fields")
        identifier = case["id"]
        require(
            isinstance(identifier, str)
            and re.fullmatch(r"[a-z][a-z0-9-]{0,95}", identifier) is not None
            and identifier not in seen,
            "matrix case identity",
        )
        seen.add(identifier)
        require(
            case["group"] in GROUPS and case["lane"] in LANES, "matrix case group/lane"
        )
        require(
            case["proof_class"] == ("live_host" if identifier in {
                                    "m28c-platform-live", "m28f-nemo-install"}
                                    else "live_target" if live else "host_contract")
            and (not live or case["lane"] == "live_safe"),
            "host tests cannot claim live evidence",
        )
        require(
            isinstance(case["providers"], list)
            and set(case["providers"]) <= families
            and len(case["providers"]) == len(set(case["providers"])),
            "unregistered/duplicate provider",
        )
        require(
            "surface" not in case or case["surface"] in surfaces, "unregistered surface"
        )
        if live:
            expected_runner = ("provider_m28f_live" if identifier in {
                "m28f-nemo-install", "m28f-nemo-live"} else
                "provider_m28e_live" if identifier == "m28e-a2a-live" else
                "provider_m28d_live" if identifier == "m28d-mcp-live" else
                "provider_m28c1_live" if identifier in {
                "m28c1-mlx-live", "m28c1-vmlx-live"} else
                "provider_m28c_platform" if identifier == "m28c-platform-live" else
                "provider_m28b_live" if identifier in {
                "m28b-peft-live", "m28b-serving-live"} else
                "provider_m28a_live" if identifier in {
                "m28a-workloads-live", "m28a-recovery-live"} else "provider_m28_live")
            require(case.get("runner") == expected_runner, "unregistered live runner")
            continue
        command = case["command"]
        require(
            isinstance(command, list)
            and 4 <= len(command) <= 20
            and all(
                isinstance(arg, str) and re.fullmatch(r"[A-Za-z0-9_./:,-]{1,255}", arg)
                for arg in command
            ),
            "matrix command tokens",
        )
        rust = (
            command[:3] == ["cargo", "test", "--locked"]
            and "--workspace" not in command
        )
        python = command[:4] == ["python3", "-m", "pytest", "-q"]
        require(rust or python, "matrix permits only scoped cargo/pytest contracts")
        require(
            not any(".." in arg or arg.startswith("/") for arg in command),
            "matrix command path",
        )
        if rust:
            require(
                "-p" in command
                and any(
                    selection in command for selection in ("--lib", "--test", "--bin")
                ),
                "unscoped cargo selection",
            )
        else:
            require(
                all(
                    arg.startswith(("tests/", "tools/cohesix-py/tests/"))
                    and arg.endswith(".py")
                    for arg in command[4:]
                ),
                "unscoped pytest selection",
            )
    matrix["matrix_sha256"] = hashlib.sha256(raw).hexdigest()
    return matrix


def run_command(
    command: list[str], timeout_s: int, maximum: int
) -> tuple[int, str, bytes, int]:
    """Drain merged child output under one deadline and kill its owned group on overflow."""
    args = [sys.executable if command[0] == "python3" else command[0], *command[1:]]
    start = time.monotonic_ns()
    deadline = time.monotonic() + timeout_s
    output = bytearray()
    state = "PASS"

    def kill_group(pid: int) -> None:
        try:
            os.killpg(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    with subprocess.Popen(
        args,
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    ) as child:
        assert child.stdout is not None
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)
        try:
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    state = "TIMEOUT"
                    break
                for key, _ in selector.select(min(remaining, 1)):
                    data = os.read(key.fd, min(65536, maximum + 1 - len(output)))
                    if not data:
                        selector.unregister(key.fileobj)
                    else:
                        output.extend(data)
                if len(output) > maximum:
                    state = "OUTPUT_LIMIT"
                    break
            if state != "PASS":
                kill_group(child.pid)
            try:
                code = child.wait(timeout=max(0.001, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                kill_group(child.pid)
                code = child.wait()
                state = "TIMEOUT"
        finally:
            selector.close()
            if child.poll() is None:
                kill_group(child.pid)
                child.wait()
    if state == "PASS" and code != 0:
        state = "FAIL"
    if state == "PASS":
        marker = (
            rb"test result: ok\. [1-9][0-9]* passed"
            if command[0] == "cargo"
            else rb"[1-9][0-9]* passed"
        )
        if re.search(marker, output) is None:
            state = "NO_TESTS"
    return code, state, bytes(output[:maximum]), time.monotonic_ns() - start


def run_matrix(
    matrix: dict[str, Any],
    contract: dict[str, Any],
    state_dir: Path,
    group: str | None = None,
    provider: str | None = None,
    validate_only: bool = False,
    case_id: str | None = None,
) -> int:
    """Retain selected host results without upgrading generated availability."""
    cases = [
        case
        for case in matrix["cases"]
        if case["proof_class"] == "host_contract"
        if (case["group"] != "perf" if group is None else case["group"] == group)
        and (case_id is None or case["id"] == case_id)
        and (
            provider is None
            or provider in case["providers"]
            or case.get("surface") == provider
        )
    ]
    require(bool(cases), "no matching conformance cases")
    state_dir.mkdir(parents=True, exist_ok=False, mode=0o700)
    results = []
    for case in cases:
        result = {key: value for key, value in case.items() if key != "command"}
        result["command"] = case["command"]
        if validate_only:
            result["result"] = "NOT_RUN"
        else:
            print("Running conformance case " + case["id"], flush=True)
            code, outcome, output, duration = run_command(
                case["command"],
                matrix["maximum_case_seconds"],
                matrix["maximum_output_bytes"],
            )
            filename = case["id"] + ".log"
            (state_dir / filename).write_bytes(output)
            result.update(
                result=outcome,
                exit_code=code,
                duration_ns=duration,
                output=filename,
                output_sha256=hashlib.sha256(output).hexdigest(),
                output_bytes=len(output),
            )
            if case["group"] == "perf" and outcome == "PASS":
                reports = [
                    line.removeprefix(b"COHESIX_PROVIDER_PERF_JSON=")
                    for line in output.splitlines()
                    if line.startswith(b"COHESIX_PROVIDER_PERF_JSON=")
                ]
                require(len(reports) == 1, "performance report missing or ambiguous")
                report = json.loads(reports[0])
                require(
                    report["schema"] == "cohesix-provider-overhead/v1"
                    and report["production_proven"] is False
                    and report["claiming"] is False
                    and report["provider_graph_sha256"] == contract["graph_sha256"],
                    "performance source/compiled graph mismatch",
                )
                (state_dir / "performance.json").write_bytes(reports[0] + b"\n")
                result["performance_sha256"] = hashlib.sha256(
                    reports[0] + b"\n"
                ).hexdigest()
        results.append(result)
        if result["result"] not in ("PASS", "NOT_RUN"):
            break
    selected_profiles = [
        row
        for row in contract["contract"]["profiles"]
        if row["id"] in matrix["profiles"]
    ]
    result = (
        "VALIDATED"
        if validate_only
        else "PASS" if all(row["result"] == "PASS" for row in results) else "FAIL"
    )
    summary = {
        "schema": "cohesix-provider-conformance-run/v1",
        "phase": "host_contracts",
        "claiming": False,
        "production_proven": False,
        "result": result,
        "graph_sha256": contract["graph_sha256"],
        "matrix_sha256": matrix["matrix_sha256"],
        "profiles": selected_profiles,
        "required_lifecycle": matrix["required_lifecycle"],
        "proof_lanes": matrix["proof_lanes"],
        "results": results,
        "selected_cases": len(cases),
        "provider_availability": [
            {
                "id": f["id"],
                "availability": f["availability"],
                "required_for_release_a": f["required_for_release_a"],
            }
            for f in contract["contract"]["families"]
        ],
        "proof_limits": [
            "Host tests do not prove live execution, target, Worker or package installation.",
            "Discovery, native execution, deployment and evidence delivery retain separate artifacts.",
        ],
    }
    source = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, check=True
    )
    summary["source_commit"] = source.stdout.decode().strip()
    changed = subprocess.check_output(
        ["git", "ls-files", "-z", "--modified", "--others", "--exclude-standard"],
        cwd=ROOT,
    )
    summary["source_files"] = [
        {
            "path": p,
            "sha256": (
                hashlib.sha256((ROOT / p).read_bytes()).hexdigest()
                if (ROOT / p).is_file()
                else None
            ),
        }
        for p in sorted(set(changed.decode().strip("\0").split("\0")))
        if p
    ]
    (state_dir / "summary.json").write_text(
        json.dumps(summary, sort_keys=True, indent=2) + "\n"
    )
    print(
        json.dumps(
            {
                "result": result,
                "summary": str(state_dir / "summary.json"),
                "production_proven": False,
            }
        )
    )
    return 0 if result in ("VALIDATED", "PASS") else 1
