# CLASSIFICATION: COMMUNITY
# Filename: test_cli_extra.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22
"""Additional CLI regression tests."""

import subprocess
from pathlib import Path


def _run(cli, *args):
    return subprocess.run(["python3", str(Path("cli")/cli)] + list(args), capture_output=True, text=True)


def test_cohtrace_help():
    res = _run("cohtrace.py", "--help")
    assert res.returncode == 0
    assert "list" in res.stdout


def test_cohpkg_help():
    res = _run("cohpkg.py", "--help")
    assert res.returncode == 0


def test_cohup_help():
    res = _run("cohup.py", "--help")
    assert res.returncode == 0

