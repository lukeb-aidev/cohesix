# CLASSIFICATION: COMMUNITY
# Filename: test_cli_rule_merge.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22
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
        "os.makedirs('/srv/validator',exist_ok=True)\n"
        "import shutil; shutil.copy(src,'/srv/validator/inject_rule')\n"
    )
    script.chmod(0o755)

    env = dict(os.environ, COHRUN_BIN=str(script))
    subprocess.run(["python3", "cli/cohrun.py", "inject-rule", "--from", str(rule)], env=env, check=True)
    assert Path("/srv/validator/inject_rule").exists()
    data = Path("/srv/validator/inject_rule").read_text()
    assert "sensor" in data
