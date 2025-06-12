#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_trace_parser.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22

"""Unit test for trace_integrity helper."""

from pathlib import Path
import json
import os
import sys

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")))
import importlib.util

validator_path = Path(__file__).resolve().parents[2] / "python" / "validator.py"
spec = importlib.util.spec_from_file_location("validator", validator_path)
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)
trace_integrity = validator.trace_integrity


def test_trace_integrity_valid(tmp_path: Path) -> None:
    trace = tmp_path / "valid.trc"
    trace.write_text(json.dumps({"ts": 1, "event": "start"}) + "\n")
    assert trace_integrity(trace)
