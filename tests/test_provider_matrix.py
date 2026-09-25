# Author: Lukas Bower
# Purpose: Keep matrix selection bounded and prevent host-contract results from becoming live execution claims.
# Copyright 2026 Lukas Bower
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/ci"))
import provider_matrix as matrix  # noqa: E402


def load(tmp_path: Path, change: tuple[str, str] | None = None) -> dict:
    text = (ROOT / "configs/provider_conformance.toml").read_text()
    if change:
        assert change[0] in text
        text = text.replace(*change)
    path = tmp_path / "matrix.toml"
    path.write_text(text)
    contract = json.loads(
        (ROOT / "configs/generated/provider_registry.json").read_bytes()
    )
    return matrix.load_matrix(path, contract)


def test_matrix_rejects_weakened_obligations_unknown_identity_and_unscoped_commands(
    tmp_path: Path,
) -> None:
    valid = load(tmp_path)
    assert valid["maximum_case_seconds"] == 600
    assert any(case["id"] == "signed-causal-graph-refusals" for case in valid["cases"])
    assert {case["id"] for case in valid["cases"] if case["proof_class"] == "live_target"} == {
        "m28-jobs-live", "m28-authority-live", "m28a-workloads-live",
        "m28a-recovery-live", "m28b-peft-live", "m28b-serving-live",
        "m28c1-mlx-live", "m28c1-vmlx-live", "m28d-mcp-live",
    }
    for old, new in [
        ('"discover", "preflight"', '"discover", "discover"'),
        ('"jetson-orin-nano-jp7"', '"invented-profile"'),
        (
            'providers = ["systemd", "docker", "k8s"]',
            'providers = ["invented-provider"]',
        ),
        ('proof_class = "host_contract"', 'proof_class = "live_safe"'),
        ('["cargo", "test", "--locked", "-p"', '["cargo", "test", "--workspace", "-p"'),
        ("maximum_output_bytes = 1048576", "maximum_output_bytes = 1048577"),
        ('id = "identity-exact-delegation"', 'id = "signed-causal-graph-refusals"'),
        ('runner = "provider_m28_live"', 'runner = "unregistered_runner"'),
    ]:
        with pytest.raises(ValueError):
            load(tmp_path, (old, new))


def test_validation_records_no_execution_and_exact_selected_contract(
    tmp_path: Path,
) -> None:
    policy = load(tmp_path)
    contract = json.loads(
        (ROOT / "configs/generated/provider_registry.json").read_bytes()
    )
    output = tmp_path / "output"
    assert (
        matrix.run_matrix(
            policy, contract, output, group="evidence", validate_only=True
        )
        == 0
    )
    result = json.loads((output / "summary.json").read_bytes())
    assert result["result"] == "VALIDATED"
    assert result["phase"] == "host_contracts"
    assert result["production_proven"] is False
    assert result["claiming"] is False
    expected = {
        "signed-causal-graph-refusals",
        "signed-worker-admission-contract",
        "signed-gateway-root-admission",
        "signed-worker-terminal-correlation",
    }
    assert result["selected_cases"] == len(expected)
    assert {row["id"] for row in result["results"]} == expected
    assert result["results"][0]["result"] == "NOT_RUN"
    assert result["results"][0]["id"] == "signed-causal-graph-refusals"
    assert not list(output.glob("*.log"))
    with pytest.raises(FileExistsError):
        matrix.run_matrix(
            policy, contract, output, group="evidence", validate_only=True
        )
    with pytest.raises(ValueError, match="no matching"):
        matrix.run_matrix(policy, contract, tmp_path / "unknown", provider="unknown")
    assert not (tmp_path / "unknown").exists()
    selected = tmp_path / "one"
    assert matrix.run_matrix(
        policy, contract, selected, case_id="signed-causal-graph-refusals", validate_only=True
    ) == 0
    rows = json.loads((selected / "summary.json").read_bytes())["results"]
    assert [row["id"] for row in rows] == ["signed-causal-graph-refusals"]
    with pytest.raises(ValueError, match="no matching"):
        matrix.run_matrix(
            policy, contract, tmp_path / "missing", case_id="unknown", validate_only=True
        )


def test_command_capture_refuses_empty_success_and_output_overflow() -> None:
    # These children test the runner's capture contract, not provider execution.
    code, result, output, elapsed = matrix.run_command(
        ["python3", "-c", "print('no tests')"], 10, 1024
    )
    assert (code, result, output) == (0, "NO_TESTS", b"no tests\n")
    assert elapsed > 0
    _, result, output, _ = matrix.run_command(
        ["python3", "-c", "print('x' * 4096)"], 10, 64
    )
    assert result == "OUTPUT_LIMIT"
    assert output == b"x" * 64
    code, result, _, _ = matrix.run_command(
        ["python3", "-c", "raise SystemExit(3)"], 10, 1024
    )
    assert (code, result) == (3, "FAIL")


def test_case_cli_refuses_duplicate_or_incompatible_selection(tmp_path: Path) -> None:
    script = ROOT / "scripts/ci/provider_conformance_run.py"
    env = {**os.environ, "PYTHONPATH": str(ROOT / "tools/cohesix-py")}
    state = tmp_path / "case"

    def run(*selection: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(script), "--state-dir", str(state), *selection],
            cwd=ROOT,
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

    for selection in [
        ("--case", "signed-causal-graph-refusals", "--case", "signed-causal-graph-refusals"),
        ("--case", "signed-causal-graph-refusals", "--evidence-only"),
        ("--case", "missing", "--validate-only"),
        ("--case", "m28-jobs-live"),
        ("--case", "m28-authority-live", "--validate-only"),
    ]:
        assert run(*selection).returncode != 0
        assert not state.exists()
    assert run("--case", "signed-causal-graph-refusals", "--validate-only").returncode == 0
    summary = json.loads((state / "summary.json").read_bytes())
    assert summary["selected_cases"] == 1
    assert summary["results"][0]["result"] == "NOT_RUN"
