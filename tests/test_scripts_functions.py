#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_scripts_functions.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22
"""Unit tests for Python helper scripts."""

import sys
import types
import subprocess
from pathlib import Path
import builtins
import json

import pytest

import scripts.boottrace as boottrace
import scripts.cohtrace as cohtrace
import scripts.snapshot_writer as snapshot_writer
import scripts.autorun_tests as autorun_tests
import scripts.full_trace_audit as full_trace_audit
try:
    import scripts.worker_inference as worker_inference
except ModuleNotFoundError:  # cv2 missing
    worker_inference = None


def test_boottrace_log_event(tmp_path):
    srv = tmp_path / "srv"
    boottrace.SRV_DIR = str(srv)
    boottrace.LOG_FILE = str(srv / "boottrace.log")
    boottrace.log_event("start")
    log_file = srv / "boottrace.log"
    assert log_file.exists()
    assert "start" in log_file.read_text()


def test_cohtrace_roundtrip(tmp_path):
    p = tmp_path / "trace.json"
    trace = []
    cohtrace.log_event(trace, "e", "d")
    cohtrace.write_trace(p, trace)
    loaded = cohtrace.read_trace(p)
    assert loaded[0]["event"] == "e"
    assert loaded[0]["detail"] == "d"


def test_snapshot_collect(tmp_path):
    world = tmp_path / "sim/world.json"
    world.parent.mkdir(parents=True)
    world.write_text("{}")
    meta = tmp_path / "srv/agent_meta"
    meta.mkdir(parents=True)
    (meta / "m1.txt").write_text("ok")
    snap = snapshot_writer.collect_snapshot("w1")
    assert snap["worker_id"] == "w1"


def test_write_snapshot(tmp_path):
    base = tmp_path / "out"
    p = snapshot_writer.write_snapshot(base, "w1")
    assert p.exists()


def test_autorun_snapshot(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    Path("a.txt").write_text("1")
    res = autorun_tests.snapshot()
    assert "a.txt" in res


def test_autorun_run_tests(monkeypatch):
    calls = []
    def fake_run(cmd, check=False):
        calls.append(tuple(cmd))
    monkeypatch.setattr(subprocess, "run", fake_run)
    autorun_tests.run_tests()
    assert any(cmd[0] == "cargo" for cmd in calls)


def test_full_trace_audit(tmp_path, capsys, monkeypatch):
    f = tmp_path / "srv/trace/sim.log"
    f.parent.mkdir(parents=True)
    f.write_text("a\n")
    monkeypatch.setattr(full_trace_audit, "TRACE_PATH", f)
    full_trace_audit.main()
    out = capsys.readouterr().out
    assert "trace lines" in out


@pytest.mark.skipif(worker_inference is None, reason="worker_inference unavailable")
def test_worker_inference_run(monkeypatch, tmp_path):
    class DummyCap:
        def read(self):
            return False, None
    cap = DummyCap()
    dummy_cv2 = types.SimpleNamespace(
        VideoCapture=lambda p: cap,
        CascadeClassifier=lambda p: types.SimpleNamespace(detectMultiScale=lambda *a, **kw: [])
    )
    monkeypatch.setattr(worker_inference, "cv2", dummy_cv2)
    out_file = tmp_path / "out.txt"
    def fake_open(path, mode="r"):
        assert path == "/srv/infer/out"
        return open(out_file, "w")
    monkeypatch.setattr(worker_inference, "open", fake_open)
    worker_inference.run()
    assert out_file.exists()

