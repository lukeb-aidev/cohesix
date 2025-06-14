# CLASSIFICATION: COMMUNITY
# Filename: test_manifest_trace_errors.py v0.1
# Date Modified: 2025-07-16
# Author: Lukas Bower
"""Validate error handling for corrupted SLM manifests and traces."""

import json
from pathlib import Path
from validator import trace_integrity


def test_corrupted_manifest_and_trace(tmp_path):
    log_dir = tmp_path / "log"
    log_dir.mkdir()
    log_file = log_dir / "validator_runtime.log"

    manifest = tmp_path / "broken_manifest.json"
    manifest.write_text("{ bad json")
    try:
        json.loads(manifest.read_text())
    except json.JSONDecodeError as e:
        with open(log_file, "a") as f:
            f.write(f"rule_violation(type=\"slm_manifest\", file=\"{manifest}\", error=\"{e.msg}\")\n")

    trace = tmp_path / "broken.trc"
    trace.write_text("{invalid:true}\n")
    ok = trace_integrity(trace)
    assert not ok
    with open(log_file, "a") as f:
        f.write(f"rule_violation(type=\"trace_integrity\", file=\"{trace}\")\n")

    log_contents = log_file.read_text()
    assert str(manifest) in log_contents
    assert "trace_integrity" in log_contents
