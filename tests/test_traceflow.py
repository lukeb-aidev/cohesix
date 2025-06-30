# CLASSIFICATION: COMMUNITY
# Filename: test_traceflow.py v0.4
# Author: Lukas Bower
# Date Modified: 2025-12-20

"""End-to-end traceflow via CLI tools."""

import os
import shutil
import subprocess
from pathlib import Path

import pytest
import yaml


def _validate_config() -> None:
    """Ensure config.yaml contains required keys."""
    cfg_path = Path("setup/config.yaml")
    try:
        data = yaml.safe_load(cfg_path.read_text())
    except Exception as exc:  # pragma: no cover - config read errors
        pytest.skip(f"failed to load config: {exc}")
    required = {"network", "logging"}
    if not isinstance(data, dict):
        pytest.skip("config.yaml not a mapping")
    if "role" not in data and "default_role" not in data:
        pytest.skip("config.yaml missing required keys")
    if not required.issubset(data):
        pytest.skip("config.yaml missing required keys")


def test_traceflow(tmp_path: Path) -> None:
    env = dict(
        os.environ, CAP_BASE=str(tmp_path / "caps"), COHESIX_LOG=str(tmp_path / "log")
    )
    log_dir = Path(env["COHESIX_LOG"])
    log_dir.mkdir(parents=True, exist_ok=True)
    trace_root = tmp_path / "trace"
    (trace_root / "w1").mkdir(parents=True, exist_ok=True)
    _validate_config()
    trace_link = Path("/srv/trace")
    if trace_link.exists() or trace_link.is_symlink():
        if os.access(trace_link, os.W_OK):
            try:
                trace_link.unlink()
            except IsADirectoryError:
                try:
                    shutil.rmtree(trace_link)
                except PermissionError:
                    pytest.skip("insufficient permissions to modify /srv/trace")
        else:
            pytest.skip("insufficient permissions to modify /srv/trace")
    try:
        os.symlink(trace_root, trace_link)
    except PermissionError:
        pytest.skip("insufficient permissions to modify /srv/trace")

    subprocess.run(
        [
            "python3",
            str(Path("cli/cohcap.py").resolve()),
            "grant",
            "camera",
            "--to",
            "w1",
        ],
        env=env,
        check=True,
    )
    subprocess.run(
        ["python3", str(Path("cli/cohcli.py").resolve()), "boot", "DroneWorker"],
        check=True,
    )
    trace = tmp_path / "trace.json"
    trace.write_text('{"frames": []}')
    subprocess.run(
        [
            "python3",
            str(Path("cli/cohtrace.py").resolve()),
            "push_trace",
            "w1",
            str(trace),
        ],
        check=True,
    )
    out = trace_root / "w1" / "sim.json"
    assert out.exists(), "sim.json missing"
    assert out.read_text().strip() == '{"frames": []}', "sim.json wrong contents"
