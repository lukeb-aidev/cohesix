# CLASSIFICATION: COMMUNITY
# Filename: test_integration_boot.py v0.3
# Date Modified: 2026-10-28
# Author: Cohesix Codex

"""Boot simulation integration test for Cohesix."""

import os
import pytest

try:
    from cohesix.plan9.namespace import NamespaceLoader
    from cohesix.shell import busybox_runner
except Exception:  # pragma: no cover - skip if crate not built for Python
    pytest.skip("cohesix Python bindings missing", allow_module_level=True)


def test_boot_services(tmp_path):
    os.chdir(tmp_path)
    os.makedirs("srv", exist_ok=True)
    os.makedirs("sim", exist_ok=True)
    os.makedirs("dev", exist_ok=True)
    console = open("dev/console", "w+")
    console.write("exit\n")
    console.flush()

    # simulate service registration via namespace loader
    ns = NamespaceLoader.parse("srv /srv/test")
    NamespaceLoader.apply(ns)

    assert os.path.exists("/srv/test")

    # role exposure
    with open("/srv/cohrole", "w") as f:
        f.write("DroneWorker")
    with open("/srv/cohrole") as f:
        assert f.read() == "DroneWorker"

    # shell output stub
    busybox_runner.spawn_shell()
    log_dir = tmp_path / "log"
    log_dir.mkdir(parents=True, exist_ok=True)
    env_log = str(log_dir)
    with open(log_dir / "session.log", "a") as log:
        log.write("role startup success\n")
    assert "role startup success" in (log_dir / "session.log").read_text()

    import subprocess

    for role in (
        "QueenPrimary",
        "RegionalQueen",
        "BareMetalQueen",
        "DroneWorker",
        "KioskInteractive",
        "InteractiveAiBooth",
    ):
        env = dict(os.environ, COH_ROLE=role, COHESIX_LOG=env_log)
        res = subprocess.run(
            ["python3", "cli/cohcli.py", "status"],
            env=env,
            capture_output=True,
            text=True,
        )
        assert role in res.stdout
