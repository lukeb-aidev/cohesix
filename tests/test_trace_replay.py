# CLASSIFICATION: COMMUNITY
# Filename: test_trace_replay.py v0.1
# Date Modified: 2025-06-18
# Author: Lukas Bower

"""Trace replay integration test."""

import os
from pathlib import Path

import pytest


@pytest.fixture
def tmpboot(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    os.environ["COH_ROLE"] = "DroneWorker"
    Path("srv").mkdir()
    return tmp_path


def test_trace_replay(tmpboot):
    import sys

    sys.path.append(str(Path(__file__).resolve().parents[1] / "scripts"))
    import cohtrace

    events: list[dict[str, str]] = []
    cohtrace.log_event(events, "spawn", "busybox")
    cohtrace.log_event(events, "mount", "/srv/telemetry")
    tmp = tmpboot / "trace.trc"
    cohtrace.write_trace(tmp, events)

    loaded = cohtrace.read_trace(tmp)
    assert loaded[0]["event"] == "spawn"
    assert loaded[1]["event"] == "mount"
