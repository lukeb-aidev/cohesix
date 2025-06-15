# CLASSIFICATION: COMMUNITY
# Filename: test_snapshot_writer.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Test snapshot writer outputs file."""
import subprocess
from pathlib import Path
import json
import os
import time

def test_snapshot_writer(tmp_path):
    script = Path('scripts/snapshot_writer.py').resolve()
    env = dict(WORKER_ID='test', SNAPSHOT_BASE=str(tmp_path/'history/snapshots'), **os.environ)
    # create dummy directories
    (tmp_path/'sim').mkdir()
    (tmp_path/'srv/agent_meta').mkdir(parents=True)
    (tmp_path/'sim/world.json').write_text('{"a":1}')
    (tmp_path/'srv/agent_meta/role.txt').write_text('DroneWorker')
    proc = subprocess.Popen(["python3", str(script)], cwd=tmp_path, env=env)
    time.sleep(1.5)
    proc.terminate()
    snap = tmp_path/'history/snapshots/test.json'
    assert snap.exists()
    data = json.loads(snap.read_text())
    assert data['worker_id']=='test'

