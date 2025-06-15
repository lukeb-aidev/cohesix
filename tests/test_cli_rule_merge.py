# CLASSIFICATION: COMMUNITY
# Filename: test_cli_rule_merge.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-08-01
"""Test live rule merge using the cohrun CLI."""

import json
import os
import subprocess
from pathlib import Path

def test_cohrun_inject_rule(tmp_path, monkeypatch):
    rule = tmp_path / "rule.json"
    rule.write_text(json.dumps({"conditions": [{"sensor": "t", "op": ">", "threshold": 1}]}))

    script = tmp_path / "dummy_bin.py"
    script.write_text(
        "#!/usr/bin/env python3\n"
        "import sys,shutil,os\n"
        "src=sys.argv[sys.argv.index('--from')+1]\n"
        "dest=os.environ.get('VALIDATOR_DIR','/srv/validator')\n"
        "os.makedirs(dest,exist_ok=True)\n"
        "shutil.copy(src,os.path.join(dest,'inject_rule'))\n"
    )
    script.chmod(0o755)

    validator_dir = tmp_path / 'validator'
    env = dict(os.environ, COHRUN_BIN=str(script), VALIDATOR_DIR=str(validator_dir), COHESIX_LOG=str(tmp_path / 'log'))
    subprocess.run(["python3", "cli/cohrun.py", "inject-rule", "--from", str(rule)], env=env, check=True)
    assert (validator_dir / "inject_rule").exists()
    data = (validator_dir / "inject_rule").read_text()
    assert "sensor" in data
