# CLASSIFICATION: COMMUNITY
# Filename: vision_overlay_test.py v0.1
# Date Modified: 2025-07-08
# Author: Cohesix Codex

"""Vision overlay CLI test."""

import subprocess
import sys
from pathlib import Path


def test_vision_overlay_cli():
    cli = Path(__file__).resolve().parents[1] / 'cli' / 'cohcli.py'
    result = subprocess.run([
        sys.executable,
        str(cli),
        'vision-overlay',
        '--agent=test'
    ], capture_output=True, text=True)
    assert 'vision overlay' in result.stdout

