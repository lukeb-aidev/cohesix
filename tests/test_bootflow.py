# CLASSIFICATION: COMMUNITY
# Filename: test_bootflow.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-06-17

import os
import tempfile
import subprocess

from pathlib import Path

ROLES = [
    "QueenPrimary",
    "DroneWorker",
    "KioskInteractive",
    "SensorRelay",
    "SimulatorTest",
]


def test_bootflow_roles():
    with tempfile.TemporaryDirectory() as tmp:
        srv = Path(tmp) / "srv"
        os.environ["SRV_DIR"] = str(srv)
        for role in ROLES:
            subprocess.run(["python3", "scripts/boottrace.py", "role_detected"], check=True)
            srv.mkdir(exist_ok=True)
            (srv / "cohrole").write_text(role)
            assert (srv / "cohrole").read_text() == role
        log = (srv / "boottrace.log").read_text()
        assert "role_detected" in log
