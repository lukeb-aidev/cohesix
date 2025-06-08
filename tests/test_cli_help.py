#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_cli_help.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-13
"""Regression tests for CLI tools."""

import subprocess
from pathlib import Path
import os


def test_cohrun_help():
    cli = Path('cli/cohrun.py')
    result = subprocess.run(['python3', str(cli), '--help'], capture_output=True, text=True)
    assert result.returncode == 0
    assert 'Run Cohesix demo scenarios' in result.stdout


def test_cohcap_grant_list(tmp_path):
    env = dict(os.environ, CAP_BASE=str(tmp_path))
    cli = Path('cli/cohcap.py')
    subprocess.run(['python3', str(cli), 'grant', 'camera', '--to', 'w1'], env=env, check=True)
    out = subprocess.run(['python3', str(cli), 'list', '--worker', 'w1'], env=env, capture_output=True, text=True)
    assert 'camera' in out.stdout
