# CLASSIFICATION: COMMUNITY
# Filename: test_validator_engine.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-15
"""Tests for advanced validator rule handling."""

import json
import time
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from validator import Validator


def test_invalid_rule_format(tmp_path):
    bad = tmp_path / "bad.json"
    bad.write_text(json.dumps({"sensor": "temp"}))
    v = Validator()
    try:
        v.inject_rule(bad)
    except ValueError:
        pass
    else:
        assert False, "invalid rule should raise"


def test_rule_timeout(monkeypatch, tmp_path):
    now = [0.0]
    monkeypatch.setattr(time, "time", lambda: now[0])
    rule = {
        "conditions": [{"sensor": "s", "op": ">", "threshold": 1}],
        "timeout": 5,
    }
    path = tmp_path / "r.json"
    path.write_text(json.dumps(rule))
    v = Validator()
    v.inject_rule(path)
    assert not v.evaluate("s", 2)
    now[0] = 6
    assert v.evaluate("s", 2)


def test_logic_chains(tmp_path):
    rule_and = {
        "conditions": [
            {"sensor": "a", "op": ">", "threshold": 1},
            {"sensor": "b", "op": ">", "threshold": 1},
        ],
        "logic": "AND",
    }
    path_and = tmp_path / "and.json"
    path_and.write_text(json.dumps(rule_and))
    v = Validator()
    v.inject_rule(path_and)
    assert not v.evaluate_all({"a": 2, "b": 2})
    assert v.evaluate_all({"a": 0, "b": 2})

    rule_or = {
        "conditions": [
            {"sensor": "x", "op": ">", "threshold": 1},
            {"sensor": "y", "op": ">", "threshold": 1},
        ],
        "logic": "OR",
    }
    path_or = tmp_path / "or.json"
    path_or.write_text(json.dumps(rule_or))
    v = Validator()
    v.inject_rule(path_or)
    assert not v.evaluate_all({"x": 2, "y": 0})
    assert v.evaluate_all({"x": 0, "y": 0})
