# Author: Lukas Bower
# Purpose: Unit tests for the REST performance harness helpers.
# Copyright 2026 Lukas Bower

"""Tests for scripts/rest_perf_harness.py helpers."""

import argparse
import importlib.util
import json
import pathlib
import socket
import sys
from typing import Optional

MODULE_PATH = (
    pathlib.Path(__file__).resolve().parents[1]
    / "scripts"
    / "rest_perf_harness.py"
)

spec = importlib.util.spec_from_file_location("rest_perf_harness", MODULE_PATH)
rest_perf = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = rest_perf
spec.loader.exec_module(rest_perf)


def test_normalize_rest_url_trims_slashes() -> None:
    assert rest_perf.normalize_rest_url("http://127.0.0.1:8080/") == (
        "http://127.0.0.1:8080"
    )
    assert rest_perf.normalize_rest_url("http://127.0.0.1:8080") == (
        "http://127.0.0.1:8080"
    )


def test_apply_entropy_extremes() -> None:
    weights = {"a": 4.0, "b": 1.0}
    zero = rest_perf.apply_entropy(weights, 0.0)
    assert zero["a"] == 1.0
    assert zero["b"] == 0.0
    full = rest_perf.apply_entropy(weights, 1.0)
    assert abs(full["a"] - 0.5) < 1e-6
    assert abs(full["b"] - 0.5) < 1e-6


def test_normalize_weights_sum_to_one() -> None:
    weights = rest_perf.normalize_weights({"a": 2.0, "b": 3.0})
    total = sum(weights.values())
    assert abs(total - 1.0) < 1e-9


def test_clamp_int_bounds() -> None:
    assert rest_perf.clamp_int(5, 1, 10, "value") == 5
    try:
        rest_perf.clamp_int(0, 1, 10, "value")
    except Exception as exc:
        assert "value" in str(exc)
    else:
        raise AssertionError("Expected clamp_int to fail for out of bounds")


def test_clamp_target_workers_respects_cap() -> None:
    assert rest_perf.clamp_target_workers(10, None) == 10
    assert rest_perf.clamp_target_workers(10, 8) == 8
    assert rest_perf.clamp_target_workers(6, 8) == 6


def test_apply_multi_hive_defaults_sets_total_worker_target() -> None:
    args = argparse.Namespace(
        multi_hive=True,
        hives=3,
        workers_per_hive=1000,
        workers_min=rest_perf.DEFAULT_WORKERS_MIN,
        workers_max=rest_perf.DEFAULT_WORKERS_MAX,
    )
    rest_perf.apply_multi_hive_defaults(args, argv_tokens=[])
    assert args.workers_min == 3000
    assert args.workers_max == 3000


def test_apply_multi_hive_defaults_respects_explicit_bounds() -> None:
    args = argparse.Namespace(
        multi_hive=True,
        hives=4,
        workers_per_hive=900,
        workers_min=2000,
        workers_max=2400,
    )
    rest_perf.apply_multi_hive_defaults(
        args,
        argv_tokens=["--workers-min", "2000", "--workers-max", "2400"],
    )
    assert args.workers_min == 2000
    assert args.workers_max == 2400


def test_parse_bind_host_port_accepts_valid_host_port() -> None:
    host, port = rest_perf.parse_bind_host_port("127.0.0.1:8080", "gateway-bind")
    assert host == "127.0.0.1"
    assert port == 8080


def test_parse_bind_host_port_rejects_invalid_value() -> None:
    try:
        rest_perf.parse_bind_host_port("not-a-bind", "gateway-bind")
    except SystemExit as exc:
        assert "gateway-bind must be host:port" in str(exc)
    else:
        raise AssertionError("Expected parse_bind_host_port to fail for malformed bind")


def test_assert_bind_available_rejects_in_use_port() -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    host, port = sock.getsockname()
    try:
        try:
            rest_perf.assert_bind_available(host, port, "QEMU console")
        except SystemExit as exc:
            assert "already in use" in str(exc)
        else:
            raise AssertionError("Expected assert_bind_available to fail on occupied port")
    finally:
        sock.close()


def test_is_transient_error_policy_denied() -> None:
    err = Exception(
        "ERR ECHO reason=policy detail=denied path=/queen/ctl error=EPERM"
    )
    assert rest_perf.is_transient_error(err)


def test_is_transient_error_buffer_full() -> None:
    err = Exception(
        "ERR ECHO reason=quota detail=buffer-full path=/queen/ctl error=buffer full"
    )
    assert rest_perf.is_transient_error(err)


def test_is_transient_error_http_429() -> None:
    err = Exception("HTTP 429 Too Many Requests for http://127.0.0.1:8080/v1/fs/echo")
    assert rest_perf.is_transient_error(err)


def test_is_buffer_full_error_matches() -> None:
    err = Exception("buffer full")
    assert rest_perf.is_buffer_full_error(err)
    err = Exception("detail=buffer-full")
    assert rest_perf.is_buffer_full_error(err)
    err = Exception("invalid payload")
    assert not rest_perf.is_buffer_full_error(err)


def test_lease_tracking_helpers() -> None:
    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=True,
        strict_control_errors=False,
    )

    assert rest_perf.choose_lease_id(state) is None
    rest_perf.remember_lease_id(state, "lease-1")
    rest_perf.remember_lease_id(state, "lease-1")
    assert state.active_leases == ["lease-1"]
    rest_perf.remember_lease_id(state, "lease-2")
    assert set(state.active_leases) == {"lease-1", "lease-2"}
    chosen = rest_perf.choose_lease_id(state)
    assert chosen in {"lease-1", "lease-2"}
    rest_perf.remove_lease_id(state, "lease-1")
    assert "lease-1" not in state.active_leases


def test_is_transient_error_rejects_invalid_payload() -> None:
    err = Exception(
        "ERR ECHO reason=policy detail=invalid-payload path=/policy/ctl error=invalid payload"
    )
    assert not rest_perf.is_transient_error(err)


def test_allocate_ids_are_monotonic() -> None:
    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=True,
        strict_control_errors=False,
        run_token="abc123ef",
    )
    assert rest_perf.allocate_schedule_id(state) == "sched-abc123ef-000001"
    assert rest_perf.allocate_schedule_id(state) == "sched-abc123ef-000002"
    assert rest_perf.allocate_lease_id(state) == "lease-abc123ef-000001"
    assert rest_perf.allocate_lease_id(state) == "lease-abc123ef-000002"


def test_reconcile_worker_snapshot_keeps_bounded_tail_listing() -> None:
    current = [f"worker-{idx}" for idx in range(1, 1501)]
    actual = [f"worker-{idx}" for idx in range(1437, 1501)]

    reconciled, missing, truncated = rest_perf.reconcile_worker_snapshot(
        current,
        actual,
    )

    assert reconciled == current
    assert missing == 0
    assert truncated


def test_reconcile_worker_snapshot_trims_overpredicted_workers() -> None:
    current = [f"worker-{idx}" for idx in range(1, 1511)]
    actual = [f"worker-{idx}" for idx in range(1, 1501)]

    reconciled, missing, truncated = rest_perf.reconcile_worker_snapshot(
        current,
        actual,
    )

    assert reconciled == [f"worker-{idx}" for idx in range(1, 1501)]
    assert missing == 10
    assert not truncated


def test_expand_bounded_worker_listing_recovers_existing_target() -> None:
    tail = [f"worker-{idx}" for idx in range(1437, 1501)]

    expanded = rest_perf.expand_bounded_worker_listing(tail, 1500)

    assert expanded[0] == "worker-1"
    assert expanded[-1] == "worker-1500"
    assert len(expanded) == 1500


def test_expand_bounded_worker_listing_keeps_short_prefix() -> None:
    prefix = [f"worker-{idx}" for idx in range(1, 65)]

    expanded = rest_perf.expand_bounded_worker_listing(prefix, 1500)

    assert expanded == prefix


def test_emit_benchmark_marker_writes_queen_log_line() -> None:
    class DummyClient:
        rest_url = "http://127.0.0.1:8080"

        def __init__(self) -> None:
            self.calls: list[tuple[str, str]] = []

        def echo(self, path: str, line: str) -> rest_perf.GatewayResponse:
            self.calls.append((path, line))
            return rest_perf.GatewayResponse(
                status="ok",
                verb="ECHO",
                path=path,
                end=True,
                lines=[],
                bytes=None,
                error=None,
            )

    client = DummyClient()
    rest_perf.emit_benchmark_marker(
        client,
        None,
        mode="perf",
        phase="start",
        run_token="abc123ef",
        status="running",
    )

    assert client.calls == [
        (
            "/log/queen.log",
            "benchmark mode=perf phase=start run=abc123ef "
            "status=running rest=http://127.0.0.1:8080",
        )
    ]


def test_telemetry_append_rotates_segment_on_quota() -> None:
    def gateway_response(
        status: str,
        path: str,
        *,
        lines: Optional[list[str]] = None,
        error: Optional[str] = None,
    ) -> rest_perf.GatewayResponse:
        return rest_perf.GatewayResponse(
            status=status,
            verb="ECHO",
            path=path,
            end=True,
            lines=[] if lines is None else lines,
            bytes=None,
            error=error,
        )

    class DummyClient:
        def __init__(self) -> None:
            self.ctl_calls = 0
            self.latest = ""
            self.append_paths: list[str] = []

        def echo(self, path: str, line: str) -> rest_perf.GatewayResponse:
            if path == "/queen/telemetry/bench/ctl":
                self.ctl_calls += 1
                self.latest = f"seg-{self.ctl_calls:06d}"
                return gateway_response("OK", path)
            if path.startswith("/queen/telemetry/bench/seg/"):
                self.append_paths.append(path)
                if path.endswith("seg-000001"):
                    return gateway_response(
                        "ERR",
                        path,
                        error=(
                            "ERR ECHO reason=quota detail=buffer-full "
                            f"path={path} error=buffer full"
                        ),
                    )
                return gateway_response("OK", path)
            raise AssertionError(f"unexpected echo path: {path}")

        def cat(self, path: str, _max_bytes: int) -> rest_perf.GatewayResponse:
            if path == "/queen/telemetry/bench/latest":
                return gateway_response("OK", path, lines=[self.latest])
            raise AssertionError(f"unexpected cat path: {path}")

    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=True,
        include_lifecycle=False,
        auto_approve=False,
        transient_retries=True,
        strict_control_errors=False,
    )
    client = DummyClient()
    rest_perf.telemetry_append_op(client, "worker-1", state)
    assert client.ctl_calls == 2
    assert client.append_paths == [
        "/queen/telemetry/bench/seg/seg-000001",
        "/queen/telemetry/bench/seg/seg-000002",
    ]
    assert state.telemetry_segments["bench"] == "seg-000002"


def test_echo_with_policy_retry_queues_on_buffer_full() -> None:
    class DummyClient:
        def __init__(self) -> None:
            self.calls = []
            self.queen_calls = 0

        def echo(self, path: str, line: str) -> rest_perf.GatewayResponse:
            self.calls.append((path, line))
            if path == "/actions/queue":
                return rest_perf.GatewayResponse(
                    status="OK",
                    verb="ECHO",
                    path=path,
                    end=True,
                    lines=[],
                    bytes=None,
                    error=None,
                )
            if path == "/queen/ctl":
                self.queen_calls += 1
                if self.queen_calls == 1:
                    return rest_perf.GatewayResponse(
                        status="ERR",
                        verb="ECHO",
                        path=path,
                        end=True,
                        lines=[],
                        bytes=None,
                        error="buffer full",
                    )
                return rest_perf.GatewayResponse(
                    status="OK",
                    verb="ECHO",
                    path=path,
                    end=True,
                    lines=[],
                    bytes=None,
                    error=None,
                )
            return rest_perf.GatewayResponse(
                status="ERR",
                verb="ECHO",
                path=path,
                end=True,
                lines=[],
                bytes=None,
                error="bad path",
            )

    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=True,
        strict_control_errors=False,
    )
    client = DummyClient()
    rest_perf._echo_with_policy_retry_inner(client, "/queen/ctl", "{}", state)

    assert [call[0] for call in client.calls[:3]] == [
        "/queen/ctl",
        "/actions/queue",
        "/queen/ctl",
    ]


def test_echo_with_policy_retry_waits_for_policy_consumption() -> None:
    class DummyClient:
        def __init__(self) -> None:
            self.calls = []
            self.queen_calls = 0

        def echo(self, path: str, line: str) -> rest_perf.GatewayResponse:
            self.calls.append((path, line))
            if path == "/actions/queue":
                return rest_perf.GatewayResponse(
                    status="OK",
                    verb="ECHO",
                    path=path,
                    end=True,
                    lines=[],
                    bytes=None,
                    error=None,
                )
            if path == "/queen/ctl":
                self.queen_calls += 1
                if self.queen_calls <= 2:
                    return rest_perf.GatewayResponse(
                        status="ERR",
                        verb="ECHO",
                        path=path,
                        end=True,
                        lines=[],
                        bytes=None,
                        error="ERR ECHO reason=policy detail=denied path=/queen/ctl error=EPERM",
                    )
                return rest_perf.GatewayResponse(
                    status="OK",
                    verb="ECHO",
                    path=path,
                    end=True,
                    lines=[],
                    bytes=None,
                    error=None,
                )
            return rest_perf.GatewayResponse(
                status="ERR",
                verb="ECHO",
                path=path,
                end=True,
                lines=[],
                bytes=None,
                error="bad path",
            )

    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=True,
        strict_control_errors=False,
    )
    client = DummyClient()
    rest_perf._echo_with_policy_retry_inner(client, "/queen/ctl", "{}", state)

    assert [call[0] for call in client.calls] == [
        "/queen/ctl",
        "/actions/queue",
        "/queen/ctl",
        "/actions/queue",
        "/queen/ctl",
    ]


def test_run_with_retry_policy_honors_no_retry_mode() -> None:
    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=False,
        strict_control_errors=False,
    )
    attempts = 0

    def op() -> None:
        nonlocal attempts
        attempts += 1
        raise Exception("HTTP 429 Too Many Requests")

    try:
        rest_perf.run_with_retry_policy(op, state, timeout_s=2.0, label="no-retry")
    except Exception as exc:
        assert "429" in str(exc)
    else:
        raise AssertionError("Expected no-retry mode to surface the first failure")
    assert attempts == 1


def test_run_with_retry_policy_retries_when_enabled() -> None:
    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=True,
        strict_control_errors=False,
    )
    attempts = 0

    def op() -> None:
        nonlocal attempts
        attempts += 1
        if attempts == 1:
            raise Exception("HTTP 429 Too Many Requests")

    rest_perf.run_with_retry_policy(op, state, timeout_s=2.0, label="retry")
    assert attempts == 2


def test_should_tolerate_buffer_full_respects_strict_mode() -> None:
    response = rest_perf.GatewayResponse(
        status="ERR",
        verb="ECHO",
        path="/queen/lease/ctl",
        end=True,
        lines=[],
        bytes=None,
        error="buffer full",
    )
    relaxed = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=False,
        strict_control_errors=False,
    )
    strict = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=True,
        actions_enabled=True,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=False,
        strict_control_errors=True,
    )
    assert rest_perf.should_tolerate_buffer_full(response, relaxed)
    assert not rest_perf.should_tolerate_buffer_full(response, strict)


def test_resolve_telemetry_scenario_1gb_defaults() -> None:
    scenario = rest_perf.resolve_telemetry_scenario(
        "telemetry-1gb", 16 * 1024 * 1024
    )
    assert scenario is not None
    assert scenario.artifact_bytes == 1024 * 1024 * 1024
    assert scenario.reference_entries == 64
    assert scenario.requests_per_operation == 66


def test_build_telemetry_reference_records_cover_size() -> None:
    records = rest_perf.build_telemetry_reference_records_for_bytes(
        1 * 1024 * 1024, 256 * 1024
    )
    assert len(records) == 4
    total = 0
    for index, record in enumerate(records, start=1):
        payload = json.loads(record)
        assert payload["schema"] == rest_perf.TELEMETRY_REFERENCE_SCHEMA
        assert payload["seq"] == index
        assert payload["off"] == total
        assert payload["len"] > 0
        assert isinstance(payload["sha256"], str) and payload["sha256"]
        total += payload["len"]
    assert total == 1 * 1024 * 1024


def test_apply_fast_ramp_defaults_updates_default_inputs() -> None:
    args = argparse.Namespace(
        fast_ramp=True,
        workers_min=rest_perf.DEFAULT_WORKERS_MIN,
        workers_max=rest_perf.DEFAULT_WORKERS_MAX,
        intensity_min=rest_perf.DEFAULT_INTENSITY_MIN,
        intensity_max=rest_perf.DEFAULT_INTENSITY_MAX,
        duration_mins=rest_perf.DEFAULT_DURATION_MINS,
        ramp_step_secs=rest_perf.DEFAULT_RAMP_STEP_SECS,
        base_rps=rest_perf.DEFAULT_BASE_RPS,
        max_inflight=rest_perf.DEFAULT_MAX_INFLIGHT,
    )
    rest_perf.apply_fast_ramp_defaults(args)
    assert args.workers_min == rest_perf.FAST_RAMP_WORKERS_MIN
    assert args.workers_max == rest_perf.FAST_RAMP_WORKERS_MAX
    assert args.intensity_min == rest_perf.FAST_RAMP_INTENSITY_MIN
    assert args.intensity_max == rest_perf.FAST_RAMP_INTENSITY_MAX
    assert args.duration_mins == rest_perf.FAST_RAMP_DURATION_MINS
    assert args.ramp_step_secs == rest_perf.FAST_RAMP_RAMP_STEP_SECS
    assert abs(args.base_rps - rest_perf.FAST_RAMP_BASE_RPS) < 1e-9
    assert args.max_inflight == rest_perf.FAST_RAMP_MAX_INFLIGHT


def test_apply_fast_ramp_defaults_preserves_explicit_inputs() -> None:
    args = argparse.Namespace(
        fast_ramp=True,
        workers_min=99,
        workers_max=199,
        intensity_min=3,
        intensity_max=9,
        duration_mins=7,
        ramp_step_secs=11,
        base_rps=3.5,
        max_inflight=77,
    )
    rest_perf.apply_fast_ramp_defaults(args)
    assert args.workers_min == 99
    assert args.workers_max == 199
    assert args.intensity_min == 3
    assert args.intensity_max == 9
    assert args.duration_mins == 7
    assert args.ramp_step_secs == 11
    assert args.base_rps == 3.5
    assert args.max_inflight == 77


def test_error_rate_helper() -> None:
    stats = rest_perf.OpStats()
    assert rest_perf.error_rate(stats) == 0.0
    stats.record(0.05, True, None)
    stats.record(0.05, False, "boom")
    assert abs(rest_perf.error_rate(stats) - 0.5) < 1e-9


def test_operation_summary_includes_report_quantiles() -> None:
    stats = rest_perf.OpStats()
    stats.record(0.01, True, None)
    stats.record(0.02, True, None)
    stats.record(0.03, False, "quota")

    summary = rest_perf.operation_summary(stats, 4)

    assert summary["count"] == 3
    assert summary["err"] == 1
    assert abs(summary["error_rate"] - (1 / 3)) < 1e-9
    assert summary["p50_s"] >= 0.01
    assert summary["p90_s"] >= summary["p50_s"]
    assert summary["p99_s"] >= summary["p95_s"]


def test_gateway_status_delta_saturates_missing_fields() -> None:
    before = {
        "broker": {
            "control_waiters": 5,
            "telemetry_waiters": 9,
            "pool_exhausted": 2,
            "connected": True,
        }
    }
    after = {
        "broker": {
            "control_waiters": 8,
            "telemetry_waiters": 7,
            "pool_exhausted": 2,
            "checkout_retries": 4,
            "connected": False,
        }
    }
    assert rest_perf.gateway_status_delta(before, after) == {
        "broker": {
            "control_waiters": 3,
            "telemetry_waiters": 0,
            "pool_exhausted": 0,
        }
    }
    assert rest_perf.gateway_status_delta(None, after) is None


def test_write_simulation_artifacts_includes_gateway_status(tmp_path: pathlib.Path) -> None:
    log_path = tmp_path / "bench.log"
    args = argparse.Namespace(
        seed=123,
        rest_url="http://127.0.0.1:8080",
        workers_min=1,
        workers_max=2,
        multi_hive=False,
        hives=1,
        workers_per_hive=2,
        intensity_min=1,
        intensity_max=2,
        duration_mins=1,
        base_rps=0.5,
        max_inflight=8,
        transient_retries=True,
        strict_control_errors=False,
        fast_ramp=False,
        scenario=None,
        telemetry_reference_chunk_bytes=rest_perf.DEFAULT_TELEMETRY_REFERENCE_CHUNK_BYTES,
        error_budget_rate=None,
        gateway_pool_control_sessions=None,
        gateway_pool_telemetry_sessions=None,
        gateway_broker_control_response_timeout_ms=None,
        gateway_broker_telemetry_response_timeout_ms=None,
        gateway_control_write_retry_window_ms=None,
        summary_max_error_lines=rest_perf.DEFAULT_SUMMARY_MAX_ERROR_LINES,
    )
    overall = rest_perf.OpStats()
    overall.record(0.01, True, None)
    operation = rest_perf.OpStats()
    operation.record(0.01, True, None)
    ramp_rows = [
        {
            "step": 0,
            "workers": 2,
            "intensity": 2.0,
            "rps": 2.0,
            "ops": 1,
            "ok": 1,
            "err": 0,
            "err_rate": 0.0,
            "throughput_ops_s": 1.0,
            "ok_ops_s": 1.0,
            "max_inflight_observed": 1,
            "max_inflight_configured": 8,
            "cumulative_avg_s": 0.01,
            "cumulative_p95_s": 0.01,
            "cumulative_p99_s": 0.01,
        }
    ]
    gateway_start = {"broker": {"proc_cache_hits": 10, "pool_exhausted": 0}}
    gateway_end = {"broker": {"proc_cache_hits": 15, "pool_exhausted": 1}}
    gateway_diff = rest_perf.gateway_status_delta(gateway_start, gateway_end)
    concurrency = {
        "configured_max_inflight": 8,
        "observed_high_water": 1,
        "current_inflight": 0,
        "submitted": 1,
        "completed": 1,
    }

    with log_path.open("w", encoding="utf-8") as handle:
        logger = rest_perf.RunLogger(str(log_path), handle, echo_stdout=False)
        artifacts = rest_perf.write_simulation_artifacts(
            args,
            logger,
            overall,
            {"status": operation},
            ramp_rows,
            None,
            0.0,
            True,
            gateway_start,
            gateway_end,
            gateway_diff,
            concurrency,
        )

    payload = json.loads(pathlib.Path(artifacts["summary_json"]).read_text())
    assert payload["gateway_status_start"] == gateway_start
    assert payload["gateway_status_end"] == gateway_end
    assert payload["gateway_status_delta"] == {
        "broker": {"proc_cache_hits": 5, "pool_exhausted": 1}
    }
    assert payload["concurrency"]["observed_high_water"] == 1
    assert payload["overall"]["p99_s"] == 0.01
    assert payload["operations"]["status"]["error_rate"] == 0.0
    assert payload["report"]["schema"] == "cohesix-benchmark-report/v1"
    assert payload["report"]["workload"]["target_rps_max"] == 2.0
    assert payload["report"]["backpressure"]["pool_exhausted"] == 1
    assert payload["report"]["visualization"]["recommended_charts"]


def test_parse_args_no_retries_alias_disables_transient_retries() -> None:
    original_argv = list(sys.argv)
    try:
        sys.argv = [
            "rest_perf_harness.py",
            "--mode",
            "simulate",
            "--auth-token",
            "changeme",
            "--no-retries",
            "--scenario",
            "telemetry-1mb",
            "--error-budget-rate",
            "0.01",
        ]
        args = rest_perf.parse_args()
    finally:
        sys.argv = original_argv
    assert args.no_transient_retries
    assert not args.transient_retries
    assert args.scenario == "telemetry-1mb"
    assert abs(args.error_budget_rate - 0.01) < 1e-9
    assert args.timeout == rest_perf.DEFAULT_SIMULATE_TIMEOUT_SECS


def test_parse_args_simulate_timeout_override_is_preserved() -> None:
    original_argv = list(sys.argv)
    try:
        sys.argv = [
            "rest_perf_harness.py",
            "--mode",
            "simulate",
            "--auth-token",
            "bootstrap",
            "--timeout",
            "4.5",
        ]
        args = rest_perf.parse_args()
    finally:
        sys.argv = original_argv
    assert args.timeout == 4.5


def test_parse_args_accepts_gateway_broker_timeout_overrides() -> None:
    original_argv = list(sys.argv)
    try:
        sys.argv = [
            "rest_perf_harness.py",
            "--mode",
            "simulate",
            "--auth-token",
            "bootstrap",
            "--gateway-broker-control-timeout-ms",
            "120000",
            "--gateway-broker-telemetry-response-timeout-ms",
            "180000",
        ]
        args = rest_perf.parse_args()
    finally:
        sys.argv = original_argv
    assert args.gateway_broker_control_response_timeout_ms == 120000
    assert args.gateway_broker_telemetry_response_timeout_ms == 180000
