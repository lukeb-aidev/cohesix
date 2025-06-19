# CLASSIFICATION: COMMUNITY
# Filename: test_trace_parser.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22
"""Validate trace_integrity utility."""

from pathlib import Path

from validator import trace_integrity


def test_trace_integrity(tmp_path: Path) -> None:
    trace = tmp_path / "trace.log"
    trace.write_text('{"ts":1,"event":"boot"}\n')
    assert trace_integrity(trace)
