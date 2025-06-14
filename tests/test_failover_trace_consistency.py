#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_failover_trace_consistency.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Verify trace integrity during failover replay."""

import json
from pathlib import Path
import os
import sys

sys.path.append(str(Path(__file__).resolve().parents[1]))
from validator import trace_integrity


def test_failover_trace_consistency(tmp_path):
    traces = tmp_path / "history" / "failover" / "traces"
    traces.mkdir(parents=True)
    data = {"ts": 1, "event": "spawn", "detail": "ok"}
    for i in range(3):
        (traces / f"t{i}.log").write_text(json.dumps(data))
    for path in traces.iterdir():
        assert trace_integrity(path)
