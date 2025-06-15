# CLASSIFICATION: COMMUNITY
# Filename: test_invalid_cli.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-08-01

"""Check CLI tools reject invalid arguments."""

import subprocess
from pathlib import Path
import os
import pytest

CLIS = [
    'cli/cohcap.py',
    'cli/cohcli.py',
    'cli/cohrun.py',
    'cli/cohtrace.py',
    'cli/cohpkg.py',
    'cli/cohup.py',
]

@pytest.mark.parametrize('cli', CLIS)
def test_rejects_bad_flag(cli, tmp_path):
    env = dict(os.environ, COHESIX_LOG=str(tmp_path / 'log'))
    res = subprocess.run(['python3', cli, '--badflag'], capture_output=True, text=True, env=env)
    assert res.returncode != 0
    combined = (res.stderr + res.stdout).lower()
    assert 'usage' in combined or 'unrecognized' in combined


def test_cohpkg_bad_json(tmp_path):
    base = tmp_path / 'updates'
    base.mkdir(parents=True, exist_ok=True)
    (base / 'manifest.json').write_text('{broken')
    env = dict(os.environ, COHESIX_LOG=str(tmp_path / 'log'), COHPKG_DIR=str(base))
    res = subprocess.run(['python3', 'cli/cohpkg.py', 'list'], capture_output=True, text=True, env=env)
    assert res.returncode != 0
