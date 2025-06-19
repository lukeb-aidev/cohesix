# CLASSIFICATION: COMMUNITY
# Filename: test_scan_smoke.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Smoke test for OSS audit scanner."""

import subprocess
import json
import sys


def test_scan_demo(tmp_path):
    out = tmp_path / "out"
    subprocess.check_call([sys.executable, "-m", "pip", "install", "tomli"])
    subprocess.check_call(
        ["bash", "scripts/run_oss_audit.sh", "--demo", "--output", str(out)]
    )
    assert (out / "OPEN_SOURCE_DEPENDENCIES.md").exists()
    data = (out / "sbom_spdx_2.3.json").read_text()
    assert "packages" in json.loads(data)
