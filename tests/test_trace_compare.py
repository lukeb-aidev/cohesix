# CLASSIFICATION: COMMUNITY
# Filename: test_trace_compare.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-23
"""Tests for cohtrace trace verification and comparison."""

import subprocess
from pathlib import Path


CLI = Path("cli/cohtrace.py").resolve()


def run_cli(*args):
    return subprocess.run(
        ["python3", str(CLI)] + list(args), capture_output=True, text=True
    )


def test_verify_trace_ok(tmp_path):
    p = tmp_path / "boot_trace.json"
    p.write_text('[{"event":"boot_success"}, {"event":"namespace_mount"}]')
    res = run_cli("--verify-trace", str(p))
    assert res.returncode == 0
    assert "trace OK" in res.stdout


def test_verify_trace_missing(tmp_path):
    p = tmp_path / "boot_trace.json"
    p.write_text('[{"event":"boot_success"}]')
    res = run_cli("--verify-trace", str(p))
    assert res.returncode != 0
    assert "missing events" in res.stdout


def test_compare_trace_diff(tmp_path):
    exp = tmp_path / "exp.json"
    act = tmp_path / "act.json"
    exp.write_text('[{"event":"boot_success"}, {"event":"namespace_mount"}]')
    act.write_text('[{"event":"boot_success"}]')
    res = run_cli("compare", "--expected", str(exp), "--actual", str(act))
    assert res.returncode != 0
    assert "missing events" in res.stdout
