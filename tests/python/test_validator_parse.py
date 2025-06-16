# CLASSIFICATION: COMMUNITY
# Filename: test_validator_parse.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-08-18
"""Tests for validator rule parsing and validation."""

import json
from pathlib import Path
import pytest

from validator import Validator


def test_unknown_rule_field(tmp_path: Path) -> None:
    p = tmp_path / "bad.json"
    p.write_text(json.dumps({"conditions": [], "extra": 1}))
    v = Validator()
    with pytest.raises(ValueError):
        v.inject_rule(p)


def test_parse_toml(tmp_path: Path) -> None:
    p = tmp_path / "rule.toml"
    p.write_text("""[[conditions]]\nsensor='s'\nop='>'\nthreshold=1\n""")
    v = Validator()
    v.inject_rule(p)
    assert v.evaluate('s', 2) is False
    assert v.evaluate('s', 0) is True
