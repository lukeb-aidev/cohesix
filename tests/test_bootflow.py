# CLASSIFICATION: COMMUNITY
# Filename: test_bootflow.py v0.3
# Author: Lukas Bower
# Date Modified: 2026-10-28

import os
import tempfile
import subprocess

from pathlib import Path

ROLES = [
    "QueenPrimary",
    "RegionalQueen",
    "BareMetalQueen",
    "DroneWorker",
    "KioskInteractive",
    "InteractiveAiBooth",
    "SensorRelay",
    "SimulatorTest",
]


def test_bootflow_roles():
    with tempfile.TemporaryDirectory() as tmp:
        srv = Path(tmp) / "srv"
        srv.mkdir()
        os.environ["SRV_DIR"] = str(srv)
        for role in ROLES:
            os.environ["COHROLE"] = role
            subprocess.run(
                ["python3", "scripts/boottrace.py", "COHESIX_BOOT_OK"], check=True
            )
            (srv / "shell_out").write_text(role)
            log = (srv / "boottrace.log").read_text()
            assert "COHESIX_BOOT_OK" in log
            assert (srv / "shell_out").read_text() == role
            (srv / "boottrace.log").unlink()
