# CLASSIFICATION: COMMUNITY
# Filename: test_invalid_cli.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-23

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
def test_rejects_bad_flag(cli):
    res = subprocess.run(['python3', cli, '--badflag'], capture_output=True, text=True)
    assert res.returncode != 0
    combined = (res.stderr + res.stdout).lower()
    assert 'usage' in combined or 'unrecognized' in combined


def test_cohpkg_bad_json(tmp_path):
    base = Path('/srv/updates')
    base.mkdir(parents=True, exist_ok=True)
    (base / 'manifest.json').write_text('{broken')
    res = subprocess.run(['python3', 'cli/cohpkg.py', 'list'], capture_output=True, text=True)
    assert res.returncode != 0
