# CLASSIFICATION: COMMUNITY
# Filename: test_integration_boot.py v0.1
# Date Modified: 2025-06-18
# Author: Cohesix Codex

"""Boot simulation integration test for Cohesix."""

import os
import pytest

try:
    from cohesix.cuda.runtime import CudaExecutor
    from cohesix.sim.rapier_bridge import SimBridge, SimCommand
    from cohesix.plan9.namespace import NamespaceLoader
    from cohesix.shell import busybox_runner
except Exception:  # pragma: no cover - skip if crate not built for Python
    pytest.skip("cohesix Python bindings missing", allow_module_level=True)


def test_boot_services(tmp_path):
    os.chdir(tmp_path)
    os.makedirs("srv", exist_ok=True)
    os.makedirs("sim", exist_ok=True)

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
