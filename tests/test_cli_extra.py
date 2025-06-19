# CLASSIFICATION: COMMUNITY
# Filename: test_cli_extra.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-08-01
"""Additional CLI regression tests."""

import subprocess
from pathlib import Path
import os


def _run(cli, *args, log_dir=None):
    env = dict(os.environ)
    if log_dir:
        env["COHESIX_LOG"] = str(log_dir)
    return subprocess.run(
        ["python3", str(Path("cli") / cli)] + list(args),
        capture_output=True,
        text=True,
        env=env,
    )


def test_cohtrace_help(tmp_path):
    res = _run("cohtrace.py", "--help", log_dir=tmp_path)
    assert res.returncode == 0
    assert "list" in res.stdout


def test_cohpkg_help(tmp_path):
    res = _run("cohpkg.py", "--help", log_dir=tmp_path)
    assert res.returncode == 0


def test_cohup_help(tmp_path):
    res = _run("cohup.py", "--help", log_dir=tmp_path)
    assert res.returncode == 0
