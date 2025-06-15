# CLASSIFICATION: COMMUNITY
# Filename: test_traceflow.py v0.3
# Author: Lukas Bower
# Date Modified: 2025-08-01

"""End-to-end traceflow via CLI tools."""

import subprocess
import os
import shutil
from pathlib import Path


def test_traceflow(tmp_path):
    env = dict(os.environ, CAP_BASE=str(tmp_path / "caps"), COHESIX_LOG=str(tmp_path / "log"))
    log_dir = Path(env["COHESIX_LOG"])
    log_dir.mkdir(parents=True, exist_ok=True)
    trace_root = tmp_path / "trace"
    (trace_root / "w1").mkdir(parents=True, exist_ok=True)
    if Path("/trace").exists() or Path("/trace").is_symlink():
        try:
            Path("/trace").unlink()
        except IsADirectoryError:
            shutil.rmtree("/trace")
    os.symlink(trace_root, "/trace")

    subprocess.run(["python3", str(Path("cli/cohcap.py").resolve()), "grant", "camera", "--to", "w1"], env=env, check=True)
    subprocess.run(["python3", str(Path("cli/cohcli.py").resolve()), "boot", "DroneWorker"], check=True)
    trace = tmp_path / "trace.json"
    trace.write_text('{"frames": []}')
    subprocess.run(["python3", str(Path("cli/cohtrace.py").resolve()), "push_trace", "w1", str(trace)], check=True)
    out = trace_root / "w1" / "sim.json"
    assert out.exists()
    assert out.read_text().strip() == '{"frames": []}'
