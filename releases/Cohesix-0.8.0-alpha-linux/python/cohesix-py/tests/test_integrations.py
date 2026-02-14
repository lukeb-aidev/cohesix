# Author: Lukas Bower
# Purpose: Validate host integration adapters and fallback behavior for the Cohesix Python SDK.
# Copyright 2026 Lukas Bower

"""Tests for `cohesix.integrations`."""

from __future__ import annotations

import builtins
import subprocess
from pathlib import Path
from unittest import mock

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.integrations import (  # noqa: E402
    HostSnapshot,
    ProbeResult,
    probe_docker,
    probe_peft_runtime,
    probe_systemd,
    snapshot_to_ndjson,
)


def test_probe_systemd_skips_without_systemctl() -> None:
    with mock.patch("cohesix.integrations.shutil.which", return_value=None):
        result = probe_systemd(["cohesix-agent.service"])
    assert result.provider == "systemd"
    assert result.status == "skipped"
    assert "unavailable" in (result.error or "")


def test_probe_docker_cli_fallback_parses_rows() -> None:
    original_import = builtins.__import__

    def fake_import(name, *args, **kwargs):
        if name == "docker":
            raise ModuleNotFoundError("docker")
        return original_import(name, *args, **kwargs)

    completed = subprocess.CompletedProcess(
        args=["docker", "ps"],
        returncode=0,
        stdout='{"ID":"abc123","Names":"coh","Image":"cohesix:dev","Status":"Up 1m"}\n',
        stderr="",
    )
    with mock.patch("builtins.__import__", side_effect=fake_import):
        with mock.patch("cohesix.integrations.shutil.which", return_value="/usr/bin/docker"):
            with mock.patch("cohesix.integrations._safe_command", return_value=completed):
                result = probe_docker()

    assert result.provider == "docker"
    assert result.status == "ok"
    containers = result.data.get("containers", [])
    assert isinstance(containers, list)
    assert len(containers) == 1
    assert containers[0]["name"] == "coh"


def test_probe_peft_runtime_returns_versions() -> None:
    result = probe_peft_runtime()
    assert result.provider == "peft"
    versions = result.data.get("versions", {})
    assert isinstance(versions, dict)
    assert "torch" in versions
    assert "peft" in versions


def test_snapshot_to_ndjson_renders_one_line_per_probe() -> None:
    snapshot = HostSnapshot(
        captured_at_utc="2026-02-14T00:00:00+00:00",
        results={
            "systemd": ProbeResult(
                provider="systemd",
                status="ok",
                data={"services": {"cohesix-agent.service": {"active": "active"}}},
            ),
            "docker": ProbeResult(
                provider="docker",
                status="degraded",
                data={"containers": []},
                error="daemon unavailable",
            ),
        },
    )
    payload = snapshot_to_ndjson(snapshot)
    lines = [line for line in payload.splitlines() if line.strip()]
    assert len(lines) == 2
    assert "\"provider\":\"docker\"" in payload
