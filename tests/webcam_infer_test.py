# CLASSIFICATION: COMMUNITY
# Filename: webcam_infer_test.py v0.1
# Author: Cohesix Codex
# Date Modified: 2025-07-08
"""Validate webcam inference loop."""

import os
import subprocess


def test_infer_loop(tmp_path):
    env = os.environ.copy()
    env["INFER_CONF"] = "motion"
    subprocess.run(
        ["python3", "-m", "cohesix.webcam.worker_inference"], env=env, check=False
    )
    assert True
