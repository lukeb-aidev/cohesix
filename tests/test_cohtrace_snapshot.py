# CLASSIFICATION: COMMUNITY
# Filename: test_cohtrace_snapshot.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Test cohtrace view_snapshot command."""
import subprocess
from pathlib import Path
import os


def test_view_snapshot(tmp_path):
    snap_dir = tmp_path / "history/snapshots"
    snap_dir.mkdir(parents=True)
    (snap_dir / "w1.json").write_text('{"ok":1}')
    cli = Path("cli/cohtrace.py").resolve()
    env = dict(SNAPSHOT_BASE=str(snap_dir), **os.environ)
    result = subprocess.run(
        ["python3", str(cli), "view_snapshot", "w1"],
        cwd=tmp_path,
        env=env,
        capture_output=True,
        text=True,
    )
    assert "ok" in result.stdout
