# CLASSIFICATION: COMMUNITY
# Filename: test_validator_live_rule_merge.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Test live rule injection merge."""

from pathlib import Path
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from python.validator import Validator
import json


def test_validator_live_rule_merge(tmp_path):
    rule_path = tmp_path / "rule.json"
    rule = {
        "conditions": [{"sensor": "accelerometer", "op": ">", "threshold": 1.2}],
        "duration_active": 3,
    }
    rule_path.write_text(json.dumps(rule))
    v = Validator()
    v.inject_rule(rule_path)
    assert v.rules[0]["duration_active"] == 3
    for _ in range(2):
        assert v.evaluate("accelerometer", 1.3)
    assert not v.evaluate("accelerometer", 1.3)

