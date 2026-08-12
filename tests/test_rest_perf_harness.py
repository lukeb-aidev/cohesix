# Author: Lukas Bower
# Purpose: Unit tests for the REST performance harness helpers.
# Copyright 2026 Lukas Bower

"""Tests for scripts/rest_perf_harness.py helpers."""

import argparse
import ast
import hashlib
import importlib.util
import json
import os
import pathlib
import socket
import subprocess
import sys
import tomllib
from typing import Optional

MODULE_PATH = (
    pathlib.Path(__file__).resolve().parents[1]
    / "scripts"
    / "rest_perf_harness.py"
)
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
PRESSURE_RUNNER_PATH = REPO_ROOT / "scripts" / "m26e_qemu_pressure.sh"

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
        verb: str = "ECHO",
        lines: Optional[list[str]] = None,
        error: Optional[str] = None,
    ) -> rest_perf.GatewayResponse:
        return rest_perf.GatewayResponse(
            status=status,
            verb=verb,
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
                return gateway_response("OK", path, verb="CAT", lines=[self.latest])
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


def test_telemetry_segment_receipt_validates_latest_with_safe_fallback() -> None:
    class DummyClient:
        def __init__(
            self,
            receipt: str,
            latest: str = "seg-000999",
            response_path: str | None = None,
        ) -> None:
            self.receipt = receipt
            self.latest = latest
            self.response_path = response_path
            self.cat_calls = 0

        def echo(self, path: str, _line: str) -> rest_perf.GatewayResponse:
            return rest_perf.GatewayResponse(
                status="OK",
                verb="ECHO",
                path=self.response_path or path,
                end=True,
                lines=[self.receipt],
                bytes=41,
                error=None,
            )

        def cat(self, path: str, _max_bytes: int) -> rest_perf.GatewayResponse:
            self.cat_calls += 1
            return rest_perf.GatewayResponse(
                status="OK",
                verb="CAT",
                path=path,
                end=True,
                lines=[self.latest],
                bytes=None,
                error=None,
            )

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
        transient_retries=False,
        strict_control_errors=True,
    )

    receipt_client = DummyClient("seg-000123", latest="seg-000123")
    segment = rest_perf.create_telemetry_segment(receipt_client, state, "bench")
    assert segment == "seg-000123"
    assert receipt_client.cat_calls == 1

    fallback_client = DummyClient("../invalid")
    segment = rest_perf.create_telemetry_segment(fallback_client, state, "bench")
    assert segment == "seg-000999"
    assert fallback_client.cat_calls == 1

    mismatched_client = DummyClient(
        "seg-000123", response_path="/queen/telemetry/other/ctl"
    )
    segment = rest_perf.create_telemetry_segment(mismatched_client, state, "bench")
    assert segment == "seg-000999"
    assert mismatched_client.cat_calls == 1

    stale_client = DummyClient("seg-000123", latest="seg-000124")
    segment = rest_perf.create_telemetry_segment(stale_client, state, "bench")
    assert segment == "seg-000123"
    assert stale_client.cat_calls == 1


def test_parse_telemetry_segment_id_rejects_unsafe_components() -> None:
    assert rest_perf.parse_telemetry_segment_id(["seg-000123"]) == "seg-000123"
    assert rest_perf.parse_telemetry_segment_id(["bad", "seg-000124"]) == "bad"
    for value in [
        "",
        ".",
        "..",
        "bad/segment",
        "bad segment",
        "bad\x00segment",
        "a" * (rest_perf.MAX_TELEMETRY_SEGMENT_ID_BYTES + 1),
    ]:
        assert rest_perf.parse_telemetry_segment_id([value]) is None


def test_latest_telemetry_segment_requires_matching_complete_cat() -> None:
    class DummyClient:
        def __init__(self, response: rest_perf.GatewayResponse) -> None:
            self.response = response

        def cat(self, _path: str, _max_bytes: int) -> rest_perf.GatewayResponse:
            return self.response

    path = "/queen/telemetry/bench/latest"

    def response(
        *,
        status: str = "OK",
        verb: str = "CAT",
        response_path: str = path,
        end: bool = True,
    ) -> rest_perf.GatewayResponse:
        return rest_perf.GatewayResponse(
            status=status,
            verb=verb,
            path=response_path,
            end=end,
            lines=["seg-000123"],
            bytes=None,
            error=None,
        )

    assert (
        rest_perf.read_latest_telemetry_segment(DummyClient(response()), "bench")
        == "seg-000123"
    )
    for invalid in [
        response(status="ERR"),
        response(verb="LS"),
        response(response_path="/queen/telemetry/other/latest"),
        response(end=False),
    ]:
        assert (
            rest_perf.read_latest_telemetry_segment(DummyClient(invalid), "bench")
            is None
        )


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
    response = rest_perf._echo_with_policy_retry_inner(client, "/queen/ctl", "{}", state)

    assert [call[0] for call in client.calls[:3]] == [
        "/queen/ctl",
        "/actions/queue",
        "/queen/ctl",
    ]
    assert response.status == "OK"


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
    response = rest_perf._echo_with_policy_retry_inner(client, "/queen/ctl", "{}", state)

    assert [call[0] for call in client.calls] == [
        "/queen/ctl",
        "/actions/queue",
        "/queen/ctl",
        "/actions/queue",
        "/queen/ctl",
    ]
    assert response.status == "OK"


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


def test_error_classification_preserves_all_failures() -> None:
    stats = rest_perf.OpStats()
    stats.record(0.01, False, "detail=buffer-full error=buffer full")
    stats.record(0.01, False, "invalid payload")
    stats.record(0.01, False, None)

    classification = rest_perf.error_classification(stats)

    assert classification == {
        "buffer_full_errors": 1,
        "other_errors": 2,
        "unclassified_errors": 1,
        "all_errors_buffer_full": False,
    }
    assert rest_perf.error_classification(rest_perf.OpStats())[
        "all_errors_buffer_full"
    ] is None


def test_retained_state_summary_keeps_operation_ownership() -> None:
    schedule = rest_perf.OpStats()
    schedule.record(0.01, True, None)
    schedule.record(0.01, False, "detail=buffer-full")
    lease_grant = rest_perf.OpStats()
    lease_grant.record(0.01, False, "invalid payload")
    lease_preempt = rest_perf.OpStats()
    lease_preempt.record(0.01, True, None)
    unrelated = rest_perf.OpStats()
    unrelated.record(0.01, False, "detail=buffer-full")

    summary = rest_perf.retained_state_summary(
        {
            "schedule_write": schedule,
            "lease_grant": lease_grant,
            "lease_preempt": lease_preempt,
            "status": unrelated,
        }
    )

    assert summary["operation_names"] == [
        "schedule_write",
        "lease_grant",
        "lease_preempt",
        "lease_quota",
    ]
    assert summary["operations_attempted"]
    assert summary["count"] == 4
    assert summary["ok"] == 2
    assert summary["err"] == 2
    assert summary["buffer_full_errors"] == 1
    assert summary["other_errors"] == 1
    assert summary["bounded_refusal_observed"]
    assert summary["all_errors_buffer_full"] is False
    assert summary["operations"]["schedule_write"]["err"] == 1
    assert summary["operations"]["lease_grant"]["other_errors"] == 1
    assert summary["operations"]["lease_quota"]["count"] == 0


def test_capacity_boundary_summary_uses_observed_strict_crossing() -> None:
    args = argparse.Namespace(
        workers_min=8,
        workers_max=12,
        intensity_min=6,
        intensity_max=6,
        error_budget_rate=0.01,
    )
    ramp_rows = [
        {
            "step": 0,
            "workers": 8,
            "intensity": 6.0,
            "rps": 28.8,
            "ops": 100,
            "ok": 99,
            "err": 1,
            "err_rate": 0.01,
            "unexpected": "not projected",
        },
        {
            "step": 1,
            "workers": 10,
            "intensity": 6.0,
            "rps": 36.0,
            "ops": 9999,
            "ok": 9899,
            "err": 100,
            "err_rate": 0.01,
        },
    ]

    summary = rest_perf.capacity_boundary_summary(args, ramp_rows, worker_cap=10)

    assert summary["worker_shape"] == "ramped"
    assert summary["intensity_shape"] == "fixed"
    assert summary["configured_workers_max"] == 12
    assert summary["effective_workers_max"] == 10
    assert summary["observed_workers_max"] == 10
    assert summary["worker_cap_limited"]
    assert not summary["configured_endpoint_observed"]
    assert summary["effective_endpoint_observed"]
    assert summary["first_error"]["step"] == 0
    assert "unexpected" not in summary["first_error"]
    assert summary["first_error_budget_crossing"]["step"] == 1
    assert summary["first_error_budget_crossing"]["exact_err_rate"] > 0.01

    fixed_args = argparse.Namespace(
        workers_min=10,
        workers_max=10,
        intensity_min=6,
        intensity_max=6,
        error_budget_rate=None,
    )
    fixed = rest_perf.capacity_boundary_summary(
        fixed_args,
        [ramp_rows[1]],
        worker_cap=None,
    )
    assert fixed["worker_shape"] == "fixed"
    assert fixed["intensity_shape"] == "fixed"
    assert fixed["configured_endpoint_observed"]
    assert fixed["first_error_budget_crossing"] is None


def test_default_stateful_control_operation_mix_is_unchanged() -> None:
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
        strict_control_errors=True,
    )

    operations = rest_perf.build_operations({}, ["queen"], [], [], state)
    control_weights = {
        operation.name: operation.weight
        for operation in operations
        if operation.category == "control"
    }

    assert control_weights == {
        "schedule_write": 0.6,
        "lease_grant": 0.4,
        "lease_preempt": 0.3,
        "lease_quota": 0.2,
    }


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
        entropy=5.0,
        intensity_min=1,
        intensity_max=2,
        duration_mins=1,
        ramp_step_secs=30,
        base_rps=0.5,
        max_inflight=8,
        tail_bytes=256,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=True,
        strict_control_errors=False,
        timeout=10.0,
        request_auth_token="sensitive-test-token",
        role="queen",
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
    gateway_start = {
        "broker": {
            "control_waiters": 1,
            "control_waiters_high_water": 2,
            "control_checkouts": 10,
            "proc_cache_hits": 10,
            "pool_exhausted": 0,
        }
    }
    gateway_end = {
        "broker": {
            "control_waiters": 3,
            "control_waiters_high_water": 5,
            "control_checkouts": 14,
            "proc_cache_hits": 15,
            "pool_exhausted": 1,
        }
    }
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
        "broker": {
            "control_waiters": 2,
            "control_waiters_high_water": 3,
            "control_checkouts": 4,
            "proc_cache_hits": 5,
            "pool_exhausted": 1,
        }
    }
    assert payload["concurrency"]["observed_high_water"] == 1
    assert payload["overall"]["p99_s"] == 0.01
    assert payload["operations"]["status"]["error_rate"] == 0.0
    assert payload["report"]["schema"] == "cohesix-benchmark-report/v1"
    assert payload["report"]["workload"]["target_rps_max"] == 2.0
    assert payload["report"]["workload"]["seed"] == 123
    assert payload["report"]["workload"]["request_auth_enabled"]
    assert payload["control_write_outcome"] == "admitted"
    assert payload["report"]["workload"]["control_write_outcome"] == "admitted"
    assert payload["report"]["population"]["proof_class"] == "host-model"
    assert payload["report"]["capacity_boundary"] == {
        "ramp_steps": 1,
        "worker_shape": "ramped",
        "intensity_shape": "ramped",
        "configured_workers_max": 2,
        "effective_workers_max": 2,
        "observed_workers_max": 2,
        "worker_cap_limited": False,
        "configured_endpoint_observed": True,
        "effective_endpoint_observed": True,
        "first_error": None,
        "first_error_budget_crossing": None,
    }
    assert payload["report"]["retained_state"]["operations_attempted"] is False
    assert payload["report"]["reliability"]["all_errors_buffer_full"] is None
    assert payload["report"]["backpressure"]["control_waiters"] == 2
    assert payload["report"]["backpressure"]["control_waiters_high_water"] == 3
    assert payload["report"]["backpressure"]["control_checkouts"] == 4
    assert payload["report"]["backpressure"]["pool_exhausted"] == 1
    assert payload["report"]["visualization"]["recommended_charts"]
    assert "sensitive-test-token" not in pathlib.Path(
        artifacts["summary_json"]
    ).read_text()


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


def executable_bounds(maximum: int = 3) -> dict:
    return {
        "manifest_sha256": "f" * 64,
        "console": {"max_id_len": 64},
        "worker_runtime": {
            "roles": [
                {
                    "role": "worker-heartbeat",
                    "declaration": "executable",
                    "executable_slots": 1,
                },
                {
                    "role": "worker-gpu",
                    "declaration": "executable",
                    "executable_slots": 1,
                },
                {
                    "role": "worker-lora",
                    "declaration": "executable",
                    "executable_slots": 1,
                },
                {
                    "role": "worker-bus",
                    "declaration": "model-only",
                    "executable_slots": 0,
                },
            ],
            "task_abi_schema": "worker-task-abi/v1",
            "task_abi_version": 1,
            "maximum_live_tasks": maximum,
            "canonical_telemetry_template": (
                "/shard/<label>/worker/<id>/telemetry"
            ),
            "shard_bits": 8,
            "legacy_worker_alias": True,
        },
    }


def acceptance_summary() -> dict:
    inventory = {
        "tcbs": 1,
        "scheduling_contexts": 1,
        "reply_objects": 0,
        "vspaces": 1,
        "cnodes": 1,
        "page_tables": 8,
        "asids": 1,
        "frames": 16,
        "endpoints": 0,
        "notifications": 1,
        "fault_caps": 1,
        "timeout_fault_caps": 1,
        "cspace_slots": 64,
        "untyped_bytes": 1_048_576,
    }
    roles = ("worker-heartbeat", "worker-gpu", "worker-lora")
    return {
        "schema": "cohesix-worker-task-evidence/v1",
        "record_kind": "target-component",
        "evidence_sha256": "a" * 64,
        "verdict": "PASS",
        "target": "qemu",
        "execution_proof": "qemu",
        "target_session": {
            "target_session_sha256": "b" * 64,
            "manifest_sha256": "f" * 64,
            "root_image_sha256": "c" * 64,
            "worker_archive_sha256": "d" * 64,
            "worker_image_manifest_sha256": "e" * 64,
            "worker_abi_sha256": "1" * 64,
        },
        "topology_sha256": "2" * 64,
        "workers": [
            {
                "role": role,
                "lifecycle": "ready",
                "artifact": "verified",
                "receipt": "none" if role == "worker-heartbeat" else "confirmed",
                "execution_proof": "qemu",
                "slot": index,
                "lease_epoch": 10 + index,
                "supervisor_generation": 20 + index,
                "cap_generation": 30 + index,
                "image_sha256": str(index + 3) * 64,
                "ready_sequence": 40 + index,
                "completion_sequence": 50 + index,
                "core": index,
                "scheduling_context": {"budget_us": 100, "period_us": 1_000},
                "object_inventory": inventory,
            }
            for index, role in enumerate(roles)
        ],
    }


def write_fault_logs(tmp_path: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    acceptance = acceptance_summary()
    uart_lines = [
        "WORKER_TASK_READY role=worker-heartbeat",
        "WORKER_TASK_RECEIPT role=worker-gpu",
        "WORKER_TASK_COMPLETION role=worker-gpu",
        "WORKER_TASK_FAULT role=worker-heartbeat",
        "WORKER_TASK_TEARDOWN role=worker-heartbeat",
        "GPU_BRIDGE_FIXTURE_ADMISSION source=qemu-fixture mode=fixture "
        "profile=qemu gate=bootstrap-trace state=admitted",
    ]
    for worker in acceptance["workers"]:
        uart_lines.append(
            "WORKER_TASK_ADMISSION "
            f"role={worker['role']} image_sha256={worker['image_sha256']}"
        )
    uart = tmp_path / "qemu.uart.log"
    uart.write_text("\n".join(uart_lines) + "\n", encoding="utf-8")

    target = acceptance["target_session"]
    gdb_lines = [
        "M26E_QEMU_SESSION target=qemu machine=virt gic_version=3 "
        f"root_image_sha256={target['root_image_sha256']} "
        f"worker_archive_sha256={target['worker_archive_sha256']} "
        f"topology_sha256={acceptance['topology_sha256']}",
    ]
    for worker in acceptance["workers"]:
        gdb_lines.append(
            "M26E_GDB_ELF "
            f"role={worker['role']} elf_sha256={'9' * 64} "
            f"image_sha256={worker['image_sha256']}"
        )
    gdb_lines.extend(
        (
            "M26E_GDB_INJECTION role=worker-heartbeat phase=pre-ready "
            "symbol=fault action=zero-x0 result=continued",
            "M26E_GDB_INJECTION role=worker-heartbeat phase=during-ipc "
            "symbol=fault action=redirect-standard-fault result=continued",
            "M26E_GDB_INJECTION role=worker-heartbeat phase=budget-exhaustion "
            "symbol=spin action=redirect-timeout-spin result=continued",
        )
    )
    gdb = tmp_path / "qemu.gdb.log"
    gdb.write_text("\n".join(gdb_lines) + "\n", encoding="utf-8")
    return uart, gdb


def test_worker_runtime_bounds_rejects_absence_and_inconsistent_slots() -> None:
    try:
        rest_perf.worker_runtime_bounds({})
    except rest_perf.RestError as exc:
        assert "absence means unknown" in str(exc)
    else:
        raise AssertionError("absent bounds must not default to model-only")

    bounds = executable_bounds()
    bounds["worker_runtime"]["maximum_live_tasks"] = 4
    try:
        rest_perf.worker_runtime_bounds(bounds)
    except rest_perf.RestError as exc:
        assert "maximum" in str(exc)
    else:
        raise AssertionError("inconsistent generated maximum must fail")


def test_parse_worker_runtime_state_requires_structured_exact_role() -> None:
    worker_id = "opaque-instance-7"
    path = f"/shard/00/worker/{worker_id}/telemetry"
    line = json.dumps(
        {
            "schema": "worker-runtime-state/v1",
            "worker_id": worker_id,
            "role": "worker-gpu",
            "state": "ready",
            "slot": 1,
            "lease_epoch": 2,
            "supervisor_generation": 3,
            "cap_generation": 4,
            "ready_sequence": 5,
            "control_sequence": 6,
            "receipt_sequence": 7,
            "completion_sequence": 8,
        }
    )
    instance = rest_perf.parse_worker_runtime_state([line], worker_id, path)
    assert instance is not None
    assert instance.role == "worker-gpu"
    assert instance.lifecycle == "ready"

    inferred = json.loads(line)
    inferred["role"] = "worker"
    try:
        rest_perf.parse_worker_runtime_state(
            [json.dumps(inferred)],
            worker_id,
            path,
        )
    except rest_perf.RestError as exc:
        assert "malformed structured Worker state" in str(exc)
    else:
        raise AssertionError("generic ids must not default to Heartbeat")


def test_executable_population_discovers_only_canonical_ready_workers() -> None:
    worker_id = "opaque-instance-7"
    label = rest_perf.expected_worker_shard_label(worker_id, 8)
    telemetry_path = f"/shard/{label}/worker/{worker_id}/telemetry"
    state_line = json.dumps(
        {
            "schema": "worker-runtime-state/v1",
            "worker_id": worker_id,
            "role": "worker-lora",
            "state": "ready",
            "slot": 2,
            "lease_epoch": 8,
            "supervisor_generation": 9,
            "cap_generation": 10,
            "ready_sequence": 11,
            "control_sequence": 12,
            "receipt_sequence": 13,
            "completion_sequence": 14,
        },
        separators=(",", ":"),
    )

    class DummyClient:
        def ls(self, path: str) -> rest_perf.GatewayResponse:
            lines = {
                "/shard": [label],
                f"/shard/{label}/worker": [worker_id],
            }[path]
            return rest_perf.GatewayResponse(
                "OK", "LS", path, True, lines, None, None
            )

        def tail(self, path: str, max_bytes: int) -> rest_perf.GatewayResponse:
            assert path == telemetry_path
            assert max_bytes == rest_perf.MAX_WORKER_STATE_TAIL_BYTES
            return rest_perf.GatewayResponse(
                "OK", "TAIL", path, True, [state_line], None, None
            )

        def status(self) -> dict:
            return {
                "connected": True,
                "backend_class": "console-projection",
                "worker_acceptance": acceptance_summary(),
            }

    instances, snapshot = rest_perf.executable_population_snapshot(
        DummyClient(),
        executable_bounds(),
        1,
    )
    assert [instance.worker_id for instance in instances] == [worker_id]
    assert instances[0].telemetry_path == telemetry_path
    assert snapshot.requested == 1
    assert snapshot.discovered == 1
    assert snapshot.ready == 1
    assert snapshot.backend_class == "console-projection"
    assert snapshot.proof_class == "qemu"


def test_executable_telemetry_operation_fails_closed_without_canonical_path() -> None:
    worker_id = "opaque-instance-7"
    state = rest_perf.SimState(
        bounds=executable_bounds(),
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=256,
        policy_enabled=False,
        actions_enabled=False,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=False,
        transient_retries=False,
        strict_control_errors=True,
        population_mode=rest_perf.POPULATION_EXECUTABLE,
        maximum_live_tasks=3,
    )
    operation = next(
        operation
        for operation in rest_perf.build_operations(
            state.bounds,
            ["worker"],
            [],
            [],
            state,
        )
        if operation.name == "tail_worker_telemetry"
    )

    class DummyClient:
        def tail(self, path: str, max_bytes: int) -> rest_perf.GatewayResponse:
            raise AssertionError(f"unexpected telemetry request {path} {max_bytes}")

    try:
        operation.func(DummyClient(), worker_id, state)
    except rest_perf.RestError as exc:
        assert "no canonical telemetry path" in str(exc)
    else:
        raise AssertionError("executable mode must not fall back to /worker")


def test_gateway_population_proof_requires_validated_acceptance_summary() -> None:
    class ConnectedOnly:
        def status(self) -> dict:
            return {"connected": True, "backend_class": "console-projection"}

    try:
        rest_perf.gateway_population_axes(
            ConnectedOnly(),
            rest_perf.POPULATION_EXECUTABLE,
            executable_bounds(),
        )
    except rest_perf.RestError as exc:
        assert "validated Worker acceptance" in str(exc)
    else:
        raise AssertionError("connectivity alone must not create QEMU proof")

    class Accepted:
        def status(self) -> dict:
            return {
                "connected": True,
                "backend_class": "console-projection",
                "worker_acceptance": acceptance_summary(),
            }

    assert rest_perf.gateway_population_axes(
        Accepted(), rest_perf.POPULATION_EXECUTABLE, executable_bounds()
    ) == ("console-projection", "qemu")


def test_executable_acceptance_rejects_backend_and_manifest_drift() -> None:
    class DummyClient:
        def __init__(self, backend: str, manifest: str):
            self.backend = backend
            self.manifest = manifest

        def status(self) -> dict:
            acceptance = acceptance_summary()
            acceptance["target_session"]["manifest_sha256"] = self.manifest
            return {
                "connected": True,
                "backend_class": self.backend,
                "worker_acceptance": acceptance,
            }

    for client, expected in (
        (DummyClient("host-model", "f" * 64), "console-projection"),
        (DummyClient("console-projection", "e" * 64), "manifest"),
    ):
        try:
            rest_perf.executable_qemu_acceptance_binding(
                client,
                executable_bounds(),
            )
        except rest_perf.RestError as exc:
            assert expected in str(exc)
        else:
            raise AssertionError("executable acceptance drift must fail closed")


def test_fault_artifacts_bind_exact_status_identities(tmp_path: pathlib.Path) -> None:
    uart, gdb = write_fault_logs(tmp_path)
    args = argparse.Namespace(qemu_uart_log=str(uart), qemu_gdb_log=str(gdb))
    artifacts, markers = rest_perf.capture_fault_artifacts(
        args,
        acceptance_summary(),
    )
    assert set(artifacts) == {"uart", "gdb"}
    assert artifacts["uart"]["bytes"] == uart.stat().st_size
    assert artifacts["gdb"]["bytes"] == gdb.stat().st_size
    assert "gdb:phase=budget-exhaustion" in markers

    changed = acceptance_summary()
    changed["target_session"]["root_image_sha256"] = "0" * 64
    try:
        rest_perf.capture_fault_artifacts(args, changed)
    except rest_perf.RestError as exc:
        assert "exact QEMU target session" in str(exc)
    else:
        raise AssertionError("fault transcript target drift must fail")


def test_qemu_fixture_receipt_paths_require_explicit_fixture_mode() -> None:
    class DummyClient:
        def __init__(self, mode: str):
            self.mode = mode

        def cat(self, path: str, max_bytes: int) -> rest_perf.GatewayResponse:
            if path.startswith("/queen/export/lora_jobs/job-fixture/"):
                assert max_bytes == 8192
                return rest_perf.GatewayResponse(
                    "OK", "CAT", path, True, ["fixture-bytes"], None, None
                )
            assert path == "/gpu/bridge/status"
            assert max_bytes == 512
            return rest_perf.GatewayResponse(
                "OK",
                "CAT",
                path,
                True,
                [
                    f"state=ok source=qemu-fixture mode={self.mode} "
                    f"bytes=10 sha256={'a' * 64}"
                ],
                None,
                None,
            )

        def ls(self, path: str) -> rest_perf.GatewayResponse:
            lines = {
                "/gpu": ["bridge", "models", "telemetry", "GPU-0"],
                "/queen/export/lora_jobs": ["job-fixture"],
            }[path]
            return rest_perf.GatewayResponse(
                "OK", "LS", path, True, lines, None, None
            )

        def tail(self, path: str, max_bytes: int) -> rest_perf.GatewayResponse:
            assert path in {
                "/host/tickets/spec",
                "/host/tickets/spec.snapshot",
                "/host/tickets/status",
            }
            assert max_bytes == 8192
            return rest_perf.GatewayResponse("OK", "TAIL", path, True, [], None, None)

    assert rest_perf.require_qemu_fixture_receipt_paths(
        DummyClient("fixture")
    ) == ("GPU-0", "job-fixture")
    try:
        rest_perf.require_qemu_fixture_receipt_paths(DummyClient("production"))
    except rest_perf.RestError as exc:
        assert "mode=fixture" in str(exc)
    else:
        raise AssertionError("production/provider mode must not replace QEMU fixture evidence")


def test_executable_report_retains_exact_pre_post_and_identity_graph() -> None:
    state = rest_perf.SimState(
        bounds=executable_bounds(),
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=256,
        policy_enabled=False,
        actions_enabled=False,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=False,
        transient_retries=False,
        strict_control_errors=True,
        population_mode=rest_perf.POPULATION_EXECUTABLE,
        maximum_live_tasks=3,
        acceptance_binding=acceptance_summary(),
    )
    pre_workers = []
    post_workers = []
    for worker in acceptance_summary()["workers"]:
        base = {
            "role": worker["role"],
            "slot": worker["slot"],
            "lease_epoch": worker["lease_epoch"],
            "supervisor_generation": worker["supervisor_generation"],
            "cap_generation": worker["cap_generation"],
            "worker_id": f"before-{worker['role']}",
            "receipt_sequence": 10,
            "completion_sequence": 10,
        }
        after = dict(base)
        if worker["role"] == "worker-heartbeat":
            after["worker_id"] = "after-worker-heartbeat"
            after["supervisor_generation"] += 1
        else:
            after["receipt_sequence"] += 1
            after["completion_sequence"] += 1
        pre_workers.append(base)
        post_workers.append(after)
    state.executable_pre_state = {"workers": pre_workers, "proc": {}}
    state.executable_post_state = {"workers": post_workers, "proc": {}}
    state.lifecycle_cycles = [{"role": "worker-heartbeat"}]
    state.receipt_operations = [
        {"action": "gpu.lease.grant", "role": "worker-gpu"},
        {"action": "peft.export", "role": "worker-lora"},
    ]
    state.fault_artifacts = {
        "uart": {"sha256": "a" * 64, "bytes": 1},
        "gdb": {"sha256": "b" * 64, "bytes": 1},
    }
    rest_perf.validate_executable_post_state(state)
    digest, report = rest_perf.build_executable_report_state(
        state,
        ["uart:WORKER_TASK_FAULT"],
    )
    assert digest == "b" * 64
    assert set(report) == {
        "topology_sha256",
        "target_session",
        "pre",
        "post",
        "lifecycle_cycles",
        "receipt_operations",
        "fault_artifacts",
        "required_fault_markers",
    }
    assert set(report["target_session"]) == {
        "manifest_sha256",
        "root_image_sha256",
        "worker_archive_sha256",
        "worker_image_manifest_sha256",
        "worker_abi_sha256",
    }


def test_build_telemetry_specs_uses_exact_canonical_paths() -> None:
    specs = rest_perf.build_telemetry_specs(
        ["opaque-instance-7"],
        1,
        512,
        {"opaque-instance-7": "/shard/ab/worker/opaque-instance-7/telemetry"},
    )
    assert specs == [
        rest_perf.RequestSpec(
            "/shard/ab/worker/opaque-instance-7/telemetry",
            512,
            "tail",
        )
    ]


def test_parse_args_executable_rejects_multi_hive_expansion() -> None:
    original_argv = list(sys.argv)
    try:
        sys.argv = [
            "rest_perf_harness.py",
            "--mode",
            "simulate",
            "--auth-token",
            "bootstrap",
            "--population-mode",
            "executable",
            "--multi-hive",
        ]
        try:
            rest_perf.parse_args()
        except SystemExit as exc:
            assert "does not permit synthetic multi-hive" in str(exc)
        else:
            raise AssertionError("executable multi-hive must fail")
    finally:
        sys.argv = original_argv


def test_parse_args_external_gateway_does_not_require_console_secret(
    monkeypatch,
) -> None:
    for name in ("COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN"):
        monkeypatch.delenv(name, raising=False)
    original_argv = list(sys.argv)
    try:
        sys.argv = [
            "rest_perf_harness.py",
            "--mode",
            "simulate",
            "--no-qemu",
            "--no-gateway",
            "--request-auth-token",
            "gateway-secret",
        ]
        args = rest_perf.parse_args()
        assert args.auth_token == ""
        assert args.request_auth_token == "gateway-secret"
    finally:
        sys.argv = original_argv


def pressure_runner_source() -> str:
    return PRESSURE_RUNNER_PATH.read_text(encoding="utf-8")


def embedded_python_blocks(source: str) -> list[str]:
    blocks: list[str] = []
    lines = source.splitlines()
    index = 0
    while index < len(lines):
        if "<<'PY'" not in lines[index]:
            index += 1
            continue
        index += 1
        block: list[str] = []
        while index < len(lines) and lines[index] != "PY":
            block.append(lines[index])
            index += 1
        assert index < len(lines), "unterminated embedded Python block"
        blocks.append("\n".join(block) + "\n")
        index += 1
    return blocks


def test_m26e_qemu_pressure_runner_has_exact_orchestration_contract() -> None:
    source = pressure_runner_source()
    assert os.access(PRESSURE_RUNNER_PATH, os.X_OK)
    assert subprocess.run(
        ["/bin/bash", "-n", str(PRESSURE_RUNNER_PATH)],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    ).returncode == 0

    for literal in (
        "umask 077",
        'preserve_input "$SEL4_SOURCE"',
        'preserve_input "$COMPILER_DIR"',
        'preserve_input "$PROFILE_VENV"',
        'preserve_input "$COMPILER_ARCHIVE"',
        'if [[ -e "$REPO_ROOT/target/CACHEDIR.TAG" ]]; then',
        'if [[ -d "$REPO_ROOT/target" ]]; then',
        'find "$REPO_ROOT/target" -depth -mindepth 1 -delete',
        'find "$REPO_ROOT/out" -depth -mindepth 1 -delete',
        'rmdir "$REPO_ROOT/target"',
        'COHESIX_QEMU_SMP_TOPO=4,cores=4,threads=1,sockets=1',
        'exact_option("-machine", "virt,gic-version=3,virtualization=on,kernel-irqchip=off")',
        '"$BUILD_RUN" --launch-existing',
        'run_pressure_boot medium 4 1 16 2604',
        'run_pressure_boot high 8 4 32 2608',
        'cohesix-qemu-frozen-collector-bindings/v1',
        'verify_frozen_collector_artifacts',
        (
            "running the canonical five-stage QEMU test plan after immutable "
            "pressure capture"
        ),
        'qemu-critical-gdb',
        'qemu-service-gdb',
        'emit-qemu-target-session',
        '--qemu-out "$OUT_ROOT"',
        '--out-dir "$RUN_DIR/session"',
        'source-inventory=source-inventory.json',
        'worker-abi-identity=worker-abi-identity.json',
        'qemu-cyw43-coexistence=qemu-cyw43-coexistence.json',
        'collect-qemu-preflight',
        'collect-qemu',
        '--preflight-service-gdb-log "$RUN_DIR/ninedoor-during-call/service.gdb.log"',
        '--preflight-service-gdb-log "$RUN_DIR/ninedoor-between-calls/service.gdb.log"',
        '--preflight-service-gdb-log "$RUN_DIR/console-standard-fault/service.gdb.log"',
        '--preflight-service-gdb-log "$RUN_DIR/console-timeout-fault/service.gdb.log"',
        '--preflight-service-uart "$RUN_DIR/ninedoor-during-call/service.uart.log"',
        '--preflight-service-uart "$RUN_DIR/ninedoor-between-calls/service.uart.log"',
        '--preflight-service-uart "$RUN_DIR/console-standard-fault/service.uart.log"',
        '--preflight-service-uart "$RUN_DIR/console-timeout-fault/service.uart.log"',
        '--auth-observation "$AUTH_OBSERVATION"',
        '--mode "$mode"',
        'ninedoor-during-call ninedoor-service during-call-standard',
        'ninedoor-between-calls ninedoor-service between-calls-revoke',
        'console-standard-fault console-network during-call-standard',
        'console-timeout-fault console-network budget-exhaustion-timeout',
        '--preflight-critical-gdb-log "$RUN_DIR/medium/critical.gdb.log"',
        '--pressure "$RUN_DIR/medium/pressure.summary.json"',
        '--pressure "$RUN_DIR/high/pressure.summary.json"',
        '--driver-archive "$DRIVER_ARCHIVE"',
        '--run-dir "$TEST_PLAN_STATE_DIR"',
        '--target-session "$FROZEN_TARGET_SESSION"',
        '--generated-inventory "$FROZEN_GENERATED_INVENTORY"',
        '--root-elf "$FROZEN_ROOT_ELF"',
        '--worker-archive "$FROZEN_WORKER_ARCHIVE"',
        '--driver-archive "$FROZEN_DRIVER_ARCHIVE"',
        '--worker-image-manifest "$FROZEN_WORKER_MANIFEST"',
        'M26E_SCAN_CONSOLE_TOKEN="$M26E_CONSOLE_AUTH_TOKEN"',
        'M26E_SCAN_REST_TOKEN="$M26E_REST_AUTH_TOKEN"',
        'M26E_CONSOLE_AUTH_TOKEN="$(queen_console_token "$SOURCE_MANIFEST" toml)"',
        'queen_console_token "$SOURCE_MANIFEST" toml >/dev/null',
        'validate_resolved_console_token',
        're.fullmatch(r"[0-9a-f]{64}", gateway)',
        'b"AUTH " + console',
        'if rest in raw:',
        'unset M26E_CONSOLE_AUTH_TOKEN M26E_REST_AUTH_TOKEN',
    ):
        assert literal in source
    assert source.count('"$BUILD_RUN" --launch-existing') == 3
    assert source.count('"$BUILD_RUN" --clean --no-run') == 1
    canonical_build = source.index(
        'log "building canonical release-qemu,bootstrap-trace artifacts"'
    )
    target_session = source.index('TARGET_SESSION="$RUN_DIR/session/target-session.json"')
    authenticated_boot = source.index(
        'log "proving one prior authenticated NineDoor operation on the exact artifacts"'
    )
    during_call_boot = source.index(
        "ninedoor-during-call ninedoor-service during-call-standard"
    )
    between_calls_boot = source.index(
        "ninedoor-between-calls ninedoor-service between-calls-revoke"
    )
    console_standard_boot = source.index(
        "console-standard-fault console-network during-call-standard"
    )
    console_timeout_boot = source.index(
        "console-timeout-fault console-network budget-exhaustion-timeout"
    )
    medium_boot = source.index("run_pressure_boot medium 4 1 16 2604")
    high_boot = source.index("run_pressure_boot high 8 4 32 2608")
    staged_plan = source.index(
        'log "running the canonical five-stage QEMU test plan after immutable '
        'pressure capture"'
    )
    final_collection = source.index(
        '"$HARNESS_PYTHON" scripts/worker_task_evidence.py collect-qemu \\\n'
    )
    assert (
        canonical_build
        < target_session
        < authenticated_boot
        < during_call_boot
        < between_calls_boot
        < console_standard_boot
        < console_timeout_boot
        < medium_boot
        < high_boot
        < staged_plan
        < final_collection
    )
    assert "rebuilding the exact pressure artifact set" not in source
    assert "qemu-gdb-services" not in source
    assert "staging/cohesix/artifacts/cohesix-driver-runtimes.cpio" not in source
    assert "--auth-token" not in source
    assert ': "${COH_AUTH_TOKEN:?' not in source
    assert "M26E_CONSOLE_AUTH_TOKEN=$COH_AUTH_TOKEN" not in source
    assert 'source_inventory = {' not in source
    assert 'find "$REPO_ROOT"' not in source
    preserve = source.index('PRESERVE_ROOT="$(mktemp -d ')
    assert source.index("COH_AUTH_TOKEN differs from the compiler-selected") < preserve
    assert source.index("gateway request secret must be 64 lowercase") < preserve


def test_m26e_qemu_pressure_freezes_collector_inputs_by_hash(
    tmp_path: pathlib.Path,
) -> None:
    blocks = embedded_python_blocks(pressure_runner_source())
    freezer = next(
        block
        for block in blocks
        if "collector source differs from its artifact binding" in block
    )
    verifier = next(
        block
        for block in blocks
        if "frozen collector binding identity is invalid" in block
    )
    filenames = {
        "target-session": "target-session.json",
        "source-inventory": "source-inventory.json",
        "worker-abi-identity": "worker-abi-identity.json",
        "qemu-cyw43-coexistence": "qemu-cyw43-coexistence.json",
        "generated-topology": "generated-topology.json",
        "worker-archive": "worker-images.cpio",
        "driver-archive": "driver-runtimes.cpio",
        "worker-manifest": "worker-image-manifest.json",
        "worker-heartbeat-elf": "worker-heart.elf",
        "worker-gpu-elf": "worker-gpu.elf",
        "worker-lora-elf": "worker-lora.elf",
        "ninedoor-elf": "nine-door-runtime.elf",
        "console-network-elf": "console-network-runtime.elf",
        "root-elf": "root-task.elf",
    }
    source_dir = tmp_path / "source"
    source_dir.mkdir()
    rows = []
    for ordinal, identifier in enumerate(filenames, start=1):
        path = source_dir / identifier
        raw = f"{identifier}-{ordinal}\n".encode()
        path.write_bytes(raw)
        rows.append(
            {
                "id": identifier,
                "path": str(path),
                "sha256": hashlib.sha256(raw).hexdigest(),
                "bytes": len(raw),
            }
        )
    source_binding = tmp_path / "artifact-bindings.json"
    source_binding.write_text(
        json.dumps(
            {
                "schema": "cohesix-qemu-artifact-bindings/v1",
                "artifacts": rows,
            }
        ),
        encoding="utf-8",
    )
    frozen_binding = tmp_path / "frozen-bindings.json"
    frozen_dir = tmp_path / "frozen"
    freeze = subprocess.run(
        [
            sys.executable,
            "-",
            str(source_binding),
            str(frozen_binding),
            str(frozen_dir),
            *(
                f"{identifier}={filename}"
                for identifier, filename in filenames.items()
            ),
        ],
        input=freezer,
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert freeze.returncode == 0, freeze.stderr

    def verify() -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                "-",
                str(source_binding),
                str(frozen_binding),
                str(frozen_dir),
            ],
            input=verifier,
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )

    assert verify().returncode == 0
    (frozen_dir / "root-task.elf").write_bytes(b"tampered\n")
    tampered = verify()
    assert tampered.returncode != 0
    assert "frozen collector artifact bytes differ: root-elf" in tampered.stderr


def test_m26e_qemu_pressure_derives_exact_manifest_queen_token(
    tmp_path: pathlib.Path,
) -> None:
    token_parser = next(
        block
        for block in embedded_python_blocks(pressure_runner_source())
        if "manifest Queen ticket input" in block
    )
    toml_manifest = tmp_path / "root_task.toml"
    toml_manifest.write_text(
        '[[tickets]]\nrole = "queen"\nsecret = "bootstrap"\n',
        encoding="utf-8",
    )
    json_manifest = tmp_path / "root_task_resolved.json"
    json_manifest.write_text(
        json.dumps({"tickets": [{"role": "queen", "secret": "bootstrap"}]}),
        encoding="utf-8",
    )

    for manifest, format_name in (
        (toml_manifest, "toml"),
        (json_manifest, "json"),
    ):
        completed = subprocess.run(
            [sys.executable, "-", str(manifest), format_name],
            input=token_parser,
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert completed.returncode == 0, completed.stderr
        assert completed.stdout == "bootstrap\n"

    toml_manifest.write_text(
        '[[tickets]]\nrole = "queen"\nsecret = "one"\n'
        '[[tickets]]\nrole = "queen"\nsecret = "two"\n',
        encoding="utf-8",
    )
    duplicate = subprocess.run(
        [sys.executable, "-", str(toml_manifest), "toml"],
        input=token_parser,
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert duplicate.returncode != 0
    assert "exactly one Queen ticket secret" in duplicate.stderr


def test_m26e_qemu_pressure_secret_scan_is_context_aware(
    tmp_path: pathlib.Path,
) -> None:
    leak_scanner = next(
        block
        for block in embedded_python_blocks(pressure_runner_source())
        if "console_credential_forms" in block
    )
    console = "bootstrap"
    rest = "a" * 64

    def scan(root: pathlib.Path) -> subprocess.CompletedProcess[str]:
        env = dict(os.environ)
        env["M26E_SCAN_CONSOLE_TOKEN"] = console
        env["M26E_SCAN_REST_TOKEN"] = rest
        return subprocess.run(
            [sys.executable, "-", str(root)],
            input=leak_scanner,
            env=env,
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )

    benign = tmp_path / "benign"
    benign.mkdir()
    (benign / "source-inventory.json").write_text(
        "apps/root-task/src/bootstrap release-qemu,bootstrap-trace\n",
        encoding="utf-8",
    )
    assert scan(benign).returncode == 0

    console_leak = tmp_path / "console-leak"
    console_leak.mkdir()
    (console_leak / "uart.log").write_text(
        f"AUTH {console}\n",
        encoding="utf-8",
    )
    console_result = scan(console_leak)
    assert console_result.returncode != 0
    assert "console credential form" in console_result.stderr

    rest_leak = tmp_path / "rest-leak"
    rest_leak.mkdir()
    (rest_leak / "gateway.log").write_text(rest, encoding="utf-8")
    rest_result = scan(rest_leak)
    assert rest_result.returncode != 0
    assert "REST bearer bytes" in rest_result.stderr


def test_m26e_qemu_pressure_quiescence_uses_actual_output_writers() -> None:
    source = pressure_runner_source()
    writer_guard = source[
        source.index("require_no_repo_output_writers() {") : source.index(
            "\nrequire_quiescent_host() {"
        )
    ]
    quiescence = source[
        source.index("require_quiescent_host() {") : source.index(
            "\nvalidate_clean_ownership() {"
        )
    ]

    assert '("lsof", "-nP", "-w", "-F", "pcan")' in writer_guard
    assert 'access in {b"w", b"u"}' in writer_guard
    assert 'value.startswith(root + b"/")' in writer_guard
    assert "completed.returncode != 0" in writer_guard
    assert "require_no_repo_output_writers" in quiescence
    assert "host-bootpd" not in quiescence
    assert "bootpd" not in quiescence


def test_m26e_qemu_pressure_writer_guard_parses_lsof_access(
    tmp_path: pathlib.Path,
) -> None:
    writer_guard = next(
        block
        for block in embedded_python_blocks(pressure_runner_source())
        if "repository out/target has active writable descriptors" in block
    )
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    fake_lsof = fake_bin / "lsof"
    fake_lsof.write_text(
        "#!/bin/sh\n"
        "printf 'p4242\\ncwriter\\nf7\\na%s\\nn%s\\n' "
        '"${M26E_TEST_ACCESS}" "${M26E_TEST_PATH}"\n',
        encoding="utf-8",
    )
    fake_lsof.chmod(0o755)
    out_dir = (tmp_path / "out").resolve()
    target_dir = (tmp_path / "target").resolve()
    out_dir.mkdir()
    target_dir.mkdir()

    def inspect(access: str, path: pathlib.Path) -> subprocess.CompletedProcess[str]:
        env = dict(os.environ)
        env["PATH"] = str(fake_bin)
        env["M26E_TEST_ACCESS"] = access
        env["M26E_TEST_PATH"] = str(path)
        return subprocess.run(
            [sys.executable, "-", str(out_dir), str(target_dir)],
            input=writer_guard,
            cwd=REPO_ROOT,
            env=env,
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )

    writable = inspect("w", out_dir / "held.log")
    assert writable.returncode != 0
    assert "pid=4242 command=writer fd=7 access=w" in writable.stderr
    assert str(out_dir / "held.log") in writable.stderr
    assert inspect("u", target_dir / "held.rw").returncode != 0
    assert inspect("r", out_dir / "reader.log").returncode == 0
    assert inspect("w", tmp_path / "out-other" / "sibling.log").returncode == 0


def test_m26e_qemu_pressure_embedded_python_is_api_aligned() -> None:
    blocks = embedded_python_blocks(pressure_runner_source())
    assert blocks
    for index, block in enumerate(blocks):
        tree = ast.parse(block, filename=f"m26e_qemu_pressure.py[{index}]")
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            if isinstance(node.func, ast.Attribute):
                assert node.func.attr != "bounds"
                if node.func.attr == "RestClient":
                    assert len(node.args) <= 3
            elif isinstance(node.func, ast.Name) and node.func.id == "RestClient":
                assert len(node.args) <= 3


def test_m26e_qemu_pressure_rejects_hostile_path_overrides() -> None:
    cases = (
        (["--run-dir", "out/../escape"], "may not contain '..'"),
        (["--run-dir", "out/toolchain/sel4-profile-venv/evidence"], "direct child"),
        (["--sel4-source", "/"], "outside its required root"),
        (["--profile-python", "/bin/python"], "canonical repository virtualenv"),
    )
    clean_env = dict(os.environ)
    for name in (
        "COH_AUTH_TOKEN",
        "COHSH_AUTH_TOKEN",
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
    ):
        clean_env.pop(name, None)
    for arguments, expected in cases:
        completed = subprocess.run(
            [str(PRESSURE_RUNNER_PATH), "--check-only", *arguments],
            cwd=REPO_ROOT,
            env=clean_env,
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
        )
        assert completed.returncode != 0
        assert expected in completed.stderr


def test_m26e_qemu_pressure_has_explicit_implementation_surface() -> None:
    source = tomllib.loads(
        (REPO_ROOT / "configs" / "implementation_surfaces.toml").read_text(
            encoding="utf-8"
        )
    )
    rows = [
        row
        for row in source["surfaces"]
        if row["path"] == "scripts/m26e_qemu_pressure.sh"
    ]
    assert len(rows) == 1
    assert rows[0]["id"] == "tool:m26e-qemu-pressure"
    assert rows[0]["class"] == "diagnostic"
    assert rows[0]["production_reachable"] is False
