# Author: Lukas Bower
# Purpose: Check Mac native release identity, baseline generation, and launchd observation parsing without claiming live admission.
# Copyright 2026 Lukas Bower
"""Focused pure contract checks for the native Mac release adapter."""

from __future__ import annotations

import json
from pathlib import Path
import socket
import subprocess
import sys

import pytest

from cohesix.hf_native import encode, sha
from cohesix import mlx_release
from cohesix.mlx_native import tree_digest


def _provider(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> mlx_release.Provider:
    root = tmp_path / "native"
    root.mkdir(mode=0o700)
    (root / "objects").mkdir(mode=0o700)
    operation = root / "operations" / "mlx-release-1"
    operation.mkdir(parents=True, mode=0o700)
    context = {"runtime_sha256": "b" * 64}
    profile = {
        "schema": "cohesix-mlx-profile/v1",
        "model_directory": str(tmp_path / "model"),
        "model_sha256": "a" * 64,
        "data_directory": str(tmp_path / "data"),
        "data_sha256": "c" * 64,
        "memory_limit_bytes": 1_073_741_824,
        "settings": {"steps": 2, "rank": 8, "seed": 1},
        "canary": {"prompts": ["Explain the gate."], "max_tokens": 4,
                   "maximum_latency_ms": 30_000},
        "quality": {"expected_output_sha256": ["e" * 64],
                    "maximum_peak_memory_bytes": 1_073_741_824},
        "evaluation_policy": {"minimum_samples": 4,
                              "maximum_age_ms": 3_600_000,
                              "metrics": {"eval_loss": {"direction": "lower",
                                                        "absolute_bound": 8.0,
                                                        "maximum_regression": 1.0}}},
        "context": context,
        "source_sha256": "d" * 64,
        "license_refs": ["local-license"],
    }
    profile_bytes = encode(profile)
    profile_sha = sha(profile_bytes)
    (root / "objects" / profile_sha).write_bytes(profile_bytes)
    input_bytes = encode({"profile_sha256": profile_sha,
                          "source_sha256": "d" * 64,
                          "adapter_directory": None, "adapter_sha256": None})
    input_sha = sha(input_bytes)
    (root / "objects" / input_sha).write_bytes(input_bytes)
    baseline = {"generation": 0, "adapter_sha256": None,
                "served_artifact_sha256": "a" * 64,
                "runtime_sha256": "b" * 64,
                "healthy": True, "rollback_verified": True}
    request_path = root / "objects" / "request"
    request_path.write_bytes(encode({
        "schema": "cohesix-peft-release/v1", "operation_id": "mlx-release-1",
        "profile_sha256": profile_sha, "input_sha256": input_sha,
        "entry": "train", "evaluation_policy": profile["evaluation_policy"],
        "baseline": baseline, "model_id": "local-model"}))
    config_path = root / "config.json"
    config_path.write_bytes(encode({
        "schema": "cohesix-mlx-native/v1", "root": str(root),
        "profile_sha256": profile_sha,
        "service_label": "cohesix-mlx-serve-test", "port": 48621,
        "python": sys.executable}))
    (root / "accepted.json").write_bytes(encode(baseline))
    (root / "runtime.json").write_bytes(encode({
        "generation": 0, "adapter_directory": None, "adapter_sha256": None}))
    monkeypatch.setattr(mlx_release.MlxSelection, "validate", lambda self: None)
    monkeypatch.setattr(mlx_release.Provider, "context", lambda self: context)
    monkeypatch.setattr(mlx_release, "observed_metal",
                        lambda _selection: {"device_name": "Test Metal"})
    monkeypatch.setattr(mlx_release.Provider, "_start_service",
                        lambda self, _runtime: {"PID": "1234"})
    monkeypatch.setattr(mlx_release.Provider, "_behavior",
                        lambda self, _runtime: {"texts": ["sample"]})
    monkeypatch.setattr(mlx_release.Provider, "canary",
                        lambda self, rollback=False: {"healthy": True})
    return mlx_release.Provider(config_path, request_path,
                                Path(mlx_release.__file__))


def test_validate_binds_frozen_generation_before_native_phase(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    provider = _provider(tmp_path, monkeypatch)
    assert provider.validate()["source_sha256"] == "d" * 64
    accepted = provider.root / "accepted.json"
    changed = json.loads(accepted.read_bytes())
    changed["generation"] = 1
    accepted.write_bytes(encode(changed))
    with pytest.raises(ValueError, match="baseline_generation_changed"):
        provider.validate()


def test_launchd_state_reads_actual_pid_and_exit_status(
        monkeypatch: pytest.MonkeyPatch) -> None:
    output = b'{\n\t"LastExitStatus" = 0;\n\t"PID" = 1234;\n};\n'
    monkeypatch.setattr(mlx_release.subprocess, "run",
                        lambda *_args, **_kwargs: subprocess.CompletedProcess(
                            [], 0, output, b""))
    assert mlx_release._service_state("cohesix-mlx-serve-test") == {
        "LastExitStatus": "0", "PID": "1234"}


def test_foreign_launchd_label_is_never_removed(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(mlx_release, "_service_state",
                        lambda _label: {"Label": "cohesix-mlx-serve-test",
                                        "Program": "/bin/echo"})
    calls: list[object] = []
    monkeypatch.setattr(mlx_release.subprocess, "run",
                        lambda *_args, **_kwargs: calls.append(_args))
    with pytest.raises(ValueError, match="foreign_launchd_service_label"):
        mlx_release._stop_service("cohesix-mlx-serve-test", tmp_path)
    assert not calls


def test_launchd_stop_waits_for_owned_process_to_exit(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    label = "cohesix-mlx-serve-test"
    logs = tmp_path / "service-logs"
    states = iter([{"Label": label, "Program": "/usr/bin/env",
                    "StandardOutPath": str(logs / "stdout"),
                    "StandardErrorPath": str(logs / "stderr"), "PID": "1234"},
                   None, None])
    monkeypatch.setattr(mlx_release, "_service_state", lambda _label: next(states))
    monkeypatch.setattr(mlx_release.subprocess, "run",
                        lambda *_args, **_kwargs: subprocess.CompletedProcess(
                            [], 0, b"", b""))
    checks: list[int] = []

    def process_check(pid: int, _signal: int) -> None:
        checks.append(pid)
        if len(checks) == 2:
            raise ProcessLookupError(pid)

    monkeypatch.setattr(mlx_release.os, "kill", process_check)
    monkeypatch.setattr(mlx_release.time, "sleep", lambda _seconds: None)
    mlx_release._stop_service(label, tmp_path)
    assert checks == [1234, 1234]


def test_service_restart_refuses_occupied_loopback_port() -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        port = listener.getsockname()[1]
        with pytest.raises(ValueError, match="ambiguous_service_port_occupied"):
            mlx_release._await_port_available(port, timeout_s=0.05)
    mlx_release._await_port_available(port, timeout_s=1.0)


def test_import_of_accepted_staged_adapter_reuses_only_exact_bytes(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    provider = _provider(tmp_path, monkeypatch)
    staged = provider.root / "staged"
    staged.mkdir(mode=0o700)
    source = tmp_path / "adapter"
    source.mkdir(mode=0o700)
    (source / "adapter_config.json").write_text('{"rank":8}')
    adapter_sha = tree_digest(source, mlx_release.MAX_ADAPTER_BYTES, 4)
    destination = staged / adapter_sha
    source.rename(destination)
    monkeypatch.setattr(provider, "candidate", lambda: {
        "adapter_directory": str(destination), "adapter_sha256": adapter_sha,
        "entry": "import"})
    staged_result = provider.stage()
    assert staged_result["adapter_directory"] == str(destination)
    assert staged_result["generation"] == 1

    changed = tmp_path / "different-input"
    changed.mkdir(mode=0o700)
    (changed / "adapter_config.json").write_text('{"rank":8}')
    monkeypatch.setattr(provider, "candidate", lambda: {
        "adapter_directory": str(changed), "adapter_sha256": adapter_sha,
        "entry": "import"})
    with pytest.raises(ValueError, match="ambiguous_stage_already_started"):
        provider.stage()
