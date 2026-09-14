# Author: Lukas Bower
# Purpose: Verify exact gateway benchmark accounting and nearest-rank latency summaries.
# Copyright 2026 Lukas Bower

import importlib.util
from pathlib import Path

import pytest

SOURCE = Path(__file__).resolve().parents[1] / "scripts/ci/gateway_perf_probe.py"
SPEC = importlib.util.spec_from_file_location("gateway_perf_probe", SOURCE)
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


def test_protocol_refusals_and_http_failures_are_accounted_independently():
    samples = [
        {"elapsed_ms": 1.0, "status": 200, "expected_status": 200, "protocol_ok": False},
        {"elapsed_ms": 2.0, "status": 403, "expected_status": 403, "protocol_ok": True},
        {"elapsed_ms": 10.0, "status": 429, "expected_status": 200, "protocol_ok": True},
    ]
    assert probe.summarize(samples) == {
        "requests": 3, "p50_ms": 2.0, "p95_ms": 10.0, "errors": 2,
        "refusals": 2, "backpressure": 1,
    }


@pytest.mark.parametrize("elapsed", [float("nan"), float("inf"), -1.0])
def test_invalid_timing_cannot_produce_a_benchmark_pass(elapsed):
    with pytest.raises(ValueError, match="finite and nonnegative"):
        probe.summarize([{"elapsed_ms": elapsed, "status": 200, "expected_status": 200}])
    with pytest.raises(ValueError):
        probe.summarize([])


def test_stale_scenario_uses_a_valid_older_writer_epoch():
    assert probe.writer_epochs({"authority": {"writer_epoch": 7}}) == (7, 6)
    for value in (None, True, 0, 1, "7", 2**64):
        with pytest.raises(ValueError, match="writer_epoch"):
            probe.writer_epochs({"authority": {"writer_epoch": value}})
