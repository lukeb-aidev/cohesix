#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_traceflow.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-23

"""End-to-end traceflow via CLI tools."""

import subprocess
import os
from pathlib import Path


def test_traceflow(tmp_path):
    env = dict(os.environ, CAP_BASE=str(tmp_path / "caps"))
    Path("/log").mkdir(parents=True, exist_ok=True)
    Path("/trace/w1").mkdir(parents=True, exist_ok=True)

    subprocess.run(["python3", "cli/cohcap.py", "grant", "camera", "--to", "w1"], env=env, check=True)
    subprocess.run(["python3", "cli/cohcli.py", "boot", "DroneWorker"], check=True)
    trace = tmp_path / "trace.json"
    trace.write_text('{"frames": []}')
    subprocess.run(["python3", "cli/cohtrace.py", "push_trace", "w1", str(trace)], check=True)
    out = Path("/trace/w1/sim.json")
    assert out.exists()
    assert out.read_text().strip() == '{"frames": []}'
