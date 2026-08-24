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
import urllib.error
from dataclasses import replace
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


class ReadinessClient:
    """Record readiness-probe order without issuing network requests."""

    def __init__(self, *, connected: bool, root_status: str = "OK") -> None:
        self.connected = connected
        self.root_status = root_status
        self.calls: list[str] = []
        self.bounds = {"manifest_sha256": "demo"}

    def get_json(self, path: str) -> dict:
        self.calls.append(path)
        if path == "/v1/meta/status":
            return {"connected": self.connected}
        if path == "/v1/meta/bounds":
            return self.bounds
        raise AssertionError(f"unexpected readiness GET {path}")

    def ls(self, path: str) -> rest_perf.GatewayResponse:
        self.calls.append(f"LS {path}")
        return rest_perf.GatewayResponse(
            status=self.root_status,
            verb="LS",
            path=path,
            end=True,
            lines=[],
            bytes=None,
            error=None,
        )


def test_normalize_rest_url_trims_slashes() -> None:
    assert rest_perf.normalize_rest_url("http://127.0.0.1:8080/") == (
        "http://127.0.0.1:8080"
    )


def test_worker_failure_context_is_relevant_recent_and_bounded() -> None:
    lines = ["unrelated audit line"]
    lines.extend(
        f"WORKER_TASK_RECEIPT role=worker-lora slot=3 sequence={index}"
        for index in range(20)
    )
    lines.append("worker-20 exact public identity")

    context = rest_perf.select_worker_failure_context(
        lines, "worker-20", "worker-lora", 3
    )

    assert "unrelated audit line" not in context
    assert "sequence=7" not in context
    assert "sequence=19" in context
    assert "worker-20 exact public identity" in context
    assert len(context) <= rest_perf.WORKER_FAILURE_CONTEXT_MAX_BYTES
    assert rest_perf.normalize_rest_url("http://127.0.0.1:8080") == (
        "http://127.0.0.1:8080"
    )


def test_rest_client_retains_target_refusal_from_http_200(monkeypatch) -> None:
    detail = (
        "ERR ECHO reason=quota detail=buffer-full "
        "path=/queen/schedule/ctl error=buffer full"
    )
    payload = json.dumps(
        {
            "status": "ERR",
            "verb": "ECHO",
            "path": "/queen/schedule/ctl",
            "end": True,
            "lines": [],
            "bytes": None,
            "error": detail,
        }
    ).encode("utf-8")

    class Response:
        def __enter__(self):
            return self

        def __exit__(self, _exc_type, _exc, _traceback) -> None:
            return None

        def read(self) -> bytes:
            return payload

    def accept_request(_request, timeout: float) -> Response:
        assert timeout == 1.0
        return Response()

    monkeypatch.setattr(rest_perf.urllib.request, "urlopen", accept_request)
    client = rest_perf.RestClient("http://127.0.0.1:8080", 1.0)

    response = client.echo("/queen/schedule/ctl", "{}")

    assert response.status == "ERR"
    assert response.error == detail
    assert rest_perf.is_buffer_full_response(response)


def test_gateway_readiness_rejects_disconnected_status_before_bounds() -> None:
    client = ReadinessClient(connected=False)

    try:
        rest_perf.probe_gateway_readiness(client)
    except rest_perf.RestError as exc:
        assert str(exc) == "Gateway not ready: backend is not connected"
    else:
        raise AssertionError("disconnected gateway must not pass readiness")

    assert client.calls == ["/v1/meta/status"]


def test_gateway_readiness_checks_status_then_bounds_then_root() -> None:
    client = ReadinessClient(connected=True)

    bounds = rest_perf.probe_gateway_readiness(client)

    assert bounds is client.bounds
    assert client.calls == [
        "/v1/meta/status",
        "/v1/meta/bounds",
        "LS /",
    ]


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


def test_telemetry_segment_receipt_avoids_duplicate_read_with_safe_fallback() -> None:
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
    assert receipt_client.cat_calls == 0

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
    assert stale_client.cat_calls == 0


def test_telemetry_append_holds_per_device_lifecycle_lock() -> None:
    class TrackingLock:
        def __init__(self) -> None:
            self.depth = 0

        def __enter__(self):
            self.depth += 1
            return self

        def __exit__(self, _exc_type, _exc, _traceback) -> None:
            self.depth -= 1

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
    lock = TrackingLock()
    state.telemetry_device_locks["bench"] = lock
    state.telemetry_segments["bench"] = "seg-000001"

    class DummyClient:
        def echo(self, path: str, _line: str) -> rest_perf.GatewayResponse:
            assert path == "/queen/telemetry/bench/seg/seg-000001"
            assert lock.depth == 1
            return rest_perf.GatewayResponse(
                status="OK",
                verb="ECHO",
                path=path,
                end=True,
                lines=[],
                bytes=1,
                error=None,
            )

    rest_perf.telemetry_append_op(DummyClient(), "worker-1", state)
    assert lock.depth == 0


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


def test_relaxed_echo_with_policy_retry_queues_on_buffer_full() -> None:
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

    assert [call[0] for call in client.calls] == [
        "/queen/ctl",
        "/actions/queue",
        "/queen/ctl",
    ]
    assert client.queen_calls == 2
    assert response.status == "OK"


def test_strict_echo_buffer_full_attempts_once_without_approval() -> None:
    refusal = rest_perf.GatewayResponse(
        status="ERR",
        verb="ECHO",
        path="/queen/ctl",
        end=True,
        lines=[],
        bytes=None,
        error="ERR ECHO reason=quota detail=buffer-full path=/queen/ctl",
    )

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
            if path != "/queen/ctl":
                raise AssertionError(f"unexpected path {path}")
            self.queen_calls += 1
            if self.queen_calls == 1:
                return refusal
            return rest_perf.GatewayResponse(
                status="OK",
                verb="ECHO",
                path=path,
                end=True,
                lines=[],
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
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=True,
        strict_control_errors=True,
    )
    client = DummyClient()

    try:
        rest_perf.echo_with_policy_retry(client, "/queen/ctl", "{}", state)
    except rest_perf.RestError as exc:
        assert str(exc) == (
            "ECHO /queen/ctl failed: "
            "ERR ECHO reason=quota detail=buffer-full path=/queen/ctl"
        )
        assert exc.response is refusal
    else:
        raise AssertionError("strict buffer-full refusal must fail")

    assert [call[0] for call in client.calls] == ["/queen/ctl"]
    assert client.queen_calls == 1


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


def test_ramp_progress_holds_the_configured_endpoint_for_final_step() -> None:
    assert rest_perf.ramp_progress(0.0, 120.0, 8.0) == 0.0
    assert rest_perf.ramp_progress(56.0, 120.0, 8.0) == 0.5
    assert rest_perf.ramp_progress(112.0, 120.0, 8.0) == 1.0
    assert rest_perf.ramp_progress(120.0, 120.0, 8.0) == 1.0
    assert rest_perf.ramp_progress(0.0, 8.0, 8.0) == 1.0


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


def test_schedule_operation_closes_the_exact_fifo_lifecycle() -> None:
    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=0,
        policy_enabled=False,
        actions_enabled=False,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=False,
        transient_retries=False,
        strict_control_errors=True,
        run_token="schedule-test",
    )
    operation = next(
        operation
        for operation in rest_perf.build_operations({}, ["queen"], [], [], state)
        if operation.name == "schedule_write"
    )

    class ScheduleClient:
        def __init__(self) -> None:
            self.lines: list[dict[str, object]] = []

        def echo(self, path: str, line: str) -> rest_perf.GatewayResponse:
            assert path == "/queen/schedule/ctl"
            self.lines.append(json.loads(line))
            return rest_perf.GatewayResponse(
                status="OK",
                verb="ECHO",
                path=path,
                end=True,
                lines=[],
                bytes=len(line),
                error=None,
            )

    client = ScheduleClient()
    operation.func(client, "worker-1", state)

    assert len(client.lines) == 2
    request, dequeue = client.lines
    assert request["id"] == "sched-schedule-test-000001"
    assert dequeue == {"op": "dequeue", "id": request["id"]}


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
            "--strict-control-errors",
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
    assert args.strict_control_errors
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
                "slot": 0,
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
            f"role={worker['role']} slot={worker['slot']} "
            f"image_sha256={worker['image_sha256']}"
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


def test_parse_worker_runtime_state_accepts_bounded_v2_projection() -> None:
    worker_id = "opaque-instance-8"
    path = f"/shard/00/worker/{worker_id}/telemetry"
    record = {
        "schema": "worker-runtime-state/v2",
        "worker_id": worker_id,
        "role": "worker-gpu",
        "state": "ready",
        "identity": [1, 2, 3, 4],
        "sequence": [5, 6, 7, 8],
    }

    instance = rest_perf.parse_worker_runtime_state(
        [json.dumps(record)], worker_id, path
    )
    assert instance is not None
    assert instance.slot == 1
    assert instance.lease_epoch == 2
    assert instance.supervisor_generation == 3
    assert instance.cap_generation == 4
    assert instance.ready_sequence == 5
    assert instance.control_sequence == 6
    assert instance.receipt_sequence == 7
    assert instance.completion_sequence == 8

    record["sequence"] = [5, 6, 7, 1 << 32]
    try:
        rest_perf.parse_worker_runtime_state([json.dumps(record)], worker_id, path)
    except rest_perf.RestError as exc:
        assert "malformed structured Worker state" in str(exc)
    else:
        raise AssertionError("v2 counters outside the wire bound must fail")


def test_host_ticket_current_key_and_projection_are_exact_and_bounded() -> None:
    assert rest_perf.host_ticket_correlation_digest("ticket-v2", "idem-v2") == (
        "ce114e927e7cbec302f7c7a1d07be28c79b3602e8451636f1d0104a629ae39e8"
    )
    current = rest_perf.parse_host_ticket_current(
        [
            "HOST_TICKET_CURRENT schema=host-ticket-current/v1 "
            "state=confirmed role=worker-gpu worker=worker-gpu-slot-7 "
            "lifecycle=ready identity=7,2,3,4 sequence=5,0,9,9 admission=11"
        ]
    )
    assert current == rest_perf.HostTicketCurrent(
        state="confirmed",
        role="worker-gpu",
        worker_id="worker-gpu-slot-7",
        lifecycle="ready",
        slot=7,
        lease_epoch=2,
        supervisor_generation=3,
        cap_generation=4,
        ready_sequence=5,
        control_sequence=0,
        receipt_sequence=9,
        completion_sequence=9,
        admission_sequence=11,
    )


def test_host_ticket_current_projection_rejects_ambiguous_records() -> None:
    valid = (
        "HOST_TICKET_CURRENT schema=host-ticket-current/v1 "
        "state=pending role=worker-lora worker=worker-lora-slot-1 "
        "lifecycle=ready identity=1,1,1,1 sequence=1,0,0,0 admission=1"
    )
    for lines in ([valid, valid], [valid.replace("identity=1,1,1,1", "identity=1,1")]):
        try:
            rest_perf.parse_host_ticket_current(lines)
        except rest_perf.RestError:
            pass
        else:
            raise AssertionError("ambiguous ticket-current records must fail closed")


def test_worker_projection_retains_incremental_tail_state() -> None:
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
        population_mode=rest_perf.POPULATION_EXECUTABLE_LOG,
        maximum_live_tasks=3,
    )
    cached = rest_perf.WorkerInstance(
        worker_id="worker-2",
        role="worker-gpu",
        lifecycle="ready",
        telemetry_path="/shard/00/worker/worker-2/telemetry",
        slot=0,
        lease_epoch=1,
        supervisor_generation=2,
        cap_generation=1,
        ready_sequence=1,
        control_sequence=0,
        receipt_sequence=0,
        completion_sequence=0,
    )
    state.current_workers_by_id[cached.worker_id] = cached
    advanced = replace(
        cached,
        receipt_sequence=1,
        completion_sequence=1,
    )
    assert rest_perf.merge_current_worker_instances(state, []) == [cached]
    observed = rest_perf.merge_current_worker_instances(state, [advanced])[0]
    assert observed.receipt_sequence == 1
    assert observed.completion_sequence == 1
    assert rest_perf.merge_current_worker_instances(state, []) == [observed]


def test_worker_projection_orders_control_receipt_completion() -> None:
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
        population_mode=rest_perf.POPULATION_EXECUTABLE_LOG,
        maximum_live_tasks=3,
    )
    ready = rest_perf.WorkerInstance(
        worker_id="worker-3",
        role="worker-lora",
        lifecycle="ready",
        telemetry_path="/shard/00/worker/worker-3/telemetry",
        slot=0,
        lease_epoch=1,
        supervisor_generation=3,
        cap_generation=1,
        ready_sequence=1,
        control_sequence=0,
        receipt_sequence=0,
        completion_sequence=0,
    )
    state.current_workers_by_id[ready.worker_id] = ready
    in_flight = replace(
        ready,
        control_sequence=1,
        receipt_sequence=1,
    )
    completed = replace(
        ready,
        control_sequence=0,
        receipt_sequence=1,
        completion_sequence=1,
    )
    assert rest_perf.merge_current_worker_instances(state, [in_flight]) == [in_flight]
    assert rest_perf.merge_current_worker_instances(state, []) == [in_flight]
    assert rest_perf.merge_current_worker_instances(state, [completed]) == [completed]


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
            "slot": 0,
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


def test_executable_receipt_operations_reuse_preflight_validated_subjects(
    monkeypatch,
) -> None:
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
        population_mode=rest_perf.POPULATION_EXECUTABLE_LOG,
        maximum_live_tasks=3,
        receipt_gpu_subject="GPU-0",
        receipt_lora_subject="qemu-evidence-job",
        receipt_gpu_lease_ids=["op-preflight-gpu-a", "op-preflight-gpu-b"],
    )
    observed: list[tuple[str, str, str, str | None]] = []

    def record_receipt(
        _client,
        _state,
        action: str,
        role: str,
        _args,
        subject: str,
        operation_id=None,
    ) -> None:
        observed.append((action, role, subject, operation_id))

    monkeypatch.setattr(rest_perf, "run_v2_receipt_operation", record_receipt)
    operations = {
        operation.name: operation
        for operation in rest_perf.build_operations(
            state.bounds,
            ["worker"],
            [],
            [],
            state,
        )
    }
    operations["worker_gpu_v2_receipt"].func(None, "worker-1", state)
    operations["worker_gpu_v2_receipt"].func(None, "worker-2", state)
    operations["worker_lora_v2_receipt"].func(None, "worker-2", state)

    assert observed == [
        ("gpu.lease.renew", "worker-gpu", "GPU-0", "op-preflight-gpu-a"),
        ("gpu.lease.renew", "worker-gpu", "GPU-0", "op-preflight-gpu-b"),
        ("peft.export", "worker-lora", "qemu-evidence-job", None),
    ]


def test_executable_receipt_lanes_bound_roles_independently() -> None:
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
        population_mode=rest_perf.POPULATION_EXECUTABLE_LOG,
        maximum_live_tasks=3,
    )
    template = rest_perf.WorkerInstance(
        worker_id="template",
        role="worker-gpu",
        lifecycle="ready",
        telemetry_path="/shard/00/worker/template/telemetry",
        slot=0,
        lease_epoch=1,
        supervisor_generation=1,
        cap_generation=1,
        ready_sequence=1,
        control_sequence=0,
        receipt_sequence=0,
        completion_sequence=0,
    )
    instances = [
        replace(template, worker_id="gpu-0", slot=0),
        replace(template, worker_id="gpu-1", slot=1),
        replace(template, worker_id="lora-0", role="worker-lora", slot=0),
        replace(template, worker_id="lora-1", role="worker-lora", slot=1),
    ]
    rest_perf.merge_current_worker_instances(state, instances)
    rest_perf.initialize_ticket_worker_lanes(state, instances)
    gpu_lanes = state.ticket_worker_lanes["worker-gpu"]
    lora_lanes = state.ticket_worker_lanes["worker-lora"]

    assert gpu_lanes.qsize() == 2
    assert lora_lanes.qsize() == 2
    assert {gpu_lanes.get_nowait(), gpu_lanes.get_nowait()} == {"gpu-0", "gpu-1"}
    assert lora_lanes.get_nowait() in {"lora-0", "lora-1"}
    assert set(state.ticket_worker_locks) == {"gpu-0", "gpu-1", "lora-0", "lora-1"}


def test_gpu_receipt_follow_up_uses_exact_lease_owner_lane() -> None:
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
        population_mode=rest_perf.POPULATION_EXECUTABLE_LOG,
        maximum_live_tasks=3,
    )
    template = rest_perf.WorkerInstance(
        worker_id="gpu-0",
        role="worker-gpu",
        lifecycle="ready",
        telemetry_path="/shard/00/worker/gpu-0/telemetry",
        slot=0,
        lease_epoch=1,
        supervisor_generation=1,
        cap_generation=1,
        ready_sequence=1,
        control_sequence=0,
        receipt_sequence=0,
        completion_sequence=0,
    )
    instances = [
        template,
        replace(
            template,
            worker_id="gpu-1",
            telemetry_path="/shard/00/worker/gpu-1/telemetry",
            slot=1,
            supervisor_generation=2,
        ),
        replace(
            template,
            worker_id="lora-0",
            role="worker-lora",
            telemetry_path="/shard/00/worker/lora-0/telemetry",
            supervisor_generation=3,
        ),
    ]
    rest_perf.merge_current_worker_instances(state, instances)
    rest_perf.initialize_ticket_worker_lanes(state, instances)
    indexed = {instance.worker_id: instance for instance in instances}

    class ImmediateReceiptClient:
        def __init__(self) -> None:
            self.payloads: list[dict[str, object]] = []

        def echo(self, path: str, line: str) -> rest_perf.GatewayResponse:
            assert path == "/host/tickets/spec"
            self.payloads.append(json.loads(line))
            return rest_perf.GatewayResponse("OK", "ECHO", path, True, [], len(line), None)

        def cat(self, path: str, max_bytes: int) -> rest_perf.GatewayResponse:
            assert max_bytes == rest_perf.HOST_TICKET_CURRENT_MAX_BYTES
            payload = self.payloads[-1]
            expected_path = (
                rest_perf.HOST_TICKET_CURRENT_PREFIX
                + rest_perf.host_ticket_correlation_digest(
                    str(payload["id"]), str(payload["idempotency_key"])
                )
            )
            assert path == expected_path
            worker = indexed[str(payload["receipt_worker_id"])]
            sequence = len(self.payloads)
            line = (
                "HOST_TICKET_CURRENT schema=host-ticket-current/v1 "
                f"state=confirmed role={worker.role} worker={worker.worker_id} "
                f"lifecycle=ready identity={worker.slot},{worker.lease_epoch},"
                f"{worker.supervisor_generation},{worker.cap_generation} "
                f"sequence={worker.ready_sequence},0,{sequence},{sequence} "
                f"admission={sequence}"
            )
            return rest_perf.GatewayResponse("OK", "CAT", path, True, [line], len(line), None)

    client = ImmediateReceiptClient()
    operation_id = rest_perf.run_v2_receipt_operation(
        client,
        state,
        "gpu.lease.grant",
        "worker-gpu",
        {"ttl_s": 30, "priority": 1},
        "GPU-0",
    )
    rest_perf.run_v2_receipt_operation(
        client,
        state,
        "gpu.lease.renew",
        "worker-gpu",
        {"ttl_s": 30, "priority": 1},
        "GPU-0",
        operation_id=operation_id,
    )

    assert [payload["receipt_worker_id"] for payload in client.payloads] == [
        "gpu-0",
        "gpu-0",
    ]
    assert state.receipt_operation_workers[operation_id] == "gpu-0"


def test_host_model_telemetry_operation_uses_complete_worker_state_bound() -> None:
    worker_id = "worker-3"
    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=rest_perf.DEFAULT_TAIL_BYTES,
        policy_enabled=False,
        actions_enabled=False,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=False,
        transient_retries=False,
        strict_control_errors=True,
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
    assert (operation.name, operation.weight, operation.category) == (
        "tail_worker_telemetry",
        1.2,
        "telemetry",
    )
    observation = json.dumps(
        {
            "schema": "cohesix-worker-observation/v1",
            "public_instance_id": worker_id,
            "identity": {
                "role": "worker-heartbeat",
                "slot": 0,
                "lease_epoch": 1,
                "supervisor_generation": 1,
                "cap_generation": 1,
            },
            "state": {
                "declaration": "executable",
                "lifecycle": "ready",
                "artifact": "missing",
                "receipt": "none",
                "execution_proof": "host-model",
            },
            "request_admitted": True,
            "provider_completed": False,
            "receipt_sequence": 0,
        },
        separators=(",", ":"),
    )
    assert len(observation) == 381

    class DummyClient:
        def __init__(self) -> None:
            self.calls: list[tuple[str, int]] = []

        def tail(self, path: str, max_bytes: int) -> rest_perf.GatewayResponse:
            self.calls.append((path, max_bytes))
            return rest_perf.GatewayResponse(
                "OK",
                "TAIL",
                path,
                True,
                [observation],
                len(observation),
                None,
            )

    client = DummyClient()
    operation.func(client, worker_id, state)
    assert client.calls == [
        (
            f"/worker/{worker_id}/telemetry",
            rest_perf.MAX_WORKER_STATE_TAIL_BYTES,
        )
    ]


def test_executable_telemetry_operation_uses_canonical_path_once() -> None:
    worker_id = "opaque-instance-7"
    telemetry_path = f"/shard/ab/worker/{worker_id}/telemetry"
    state = rest_perf.SimState(
        bounds=executable_bounds(),
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=rest_perf.DEFAULT_TAIL_BYTES,
        policy_enabled=False,
        actions_enabled=False,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=False,
        transient_retries=False,
        strict_control_errors=True,
        population_mode=rest_perf.POPULATION_EXECUTABLE,
        maximum_live_tasks=3,
        worker_telemetry_paths={worker_id: telemetry_path},
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
        def __init__(self) -> None:
            self.calls: list[tuple[str, int]] = []

        def tail(self, path: str, max_bytes: int) -> rest_perf.GatewayResponse:
            self.calls.append((path, max_bytes))
            return rest_perf.GatewayResponse(
                "OK", "TAIL", path, True, ["state"], 5, None
            )

    client = DummyClient()
    operation.func(client, worker_id, state)
    assert client.calls == [
        (telemetry_path, rest_perf.MAX_WORKER_STATE_TAIL_BYTES)
    ]


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


def test_host_model_population_requires_exact_host_model_backend() -> None:
    class BackendClient:
        def __init__(self, connected: bool, backend: str | None) -> None:
            self.connected = connected
            self.backend = backend

        def status(self) -> dict:
            return {
                "connected": self.connected,
                "backend_class": self.backend,
            }

    assert rest_perf.gateway_population_axes(
        BackendClient(True, "host-model"),
        rest_perf.POPULATION_HOST_MODEL,
    ) == ("host-model", "host-model")

    for client, expected in (
        (BackendClient(True, "console-projection"), "executable population"),
        (BackendClient(True, None), "backend_class=host-model"),
        (BackendClient(False, "host-model"), "connected backend"),
    ):
        try:
            rest_perf.gateway_population_axes(
                client,
                rest_perf.POPULATION_HOST_MODEL,
            )
        except rest_perf.RestError as exc:
            assert expected in str(exc)
        else:
            raise AssertionError("invalid host-model backend must fail closed")


def test_gateway_observation_axes_do_not_require_synthetic_population() -> None:
    class BackendClient:
        def __init__(self, connected: bool, backend: str) -> None:
            self.connected = connected
            self.backend = backend

        def status(self) -> dict:
            return {
                "connected": self.connected,
                "backend_class": self.backend,
            }

    assert rest_perf.gateway_observation_axes(
        BackendClient(True, "console-projection")
    ) == ("console-projection", "none")
    assert rest_perf.gateway_observation_axes(
        BackendClient(True, "host-model")
    ) == ("host-model", "host-model")
    assert rest_perf.gateway_observation_axes(
        BackendClient(False, "host-model")
    ) == ("host-model", "none")


def test_read_only_perf_accepts_console_projection_without_population_admission(
    monkeypatch,
    tmp_path: pathlib.Path,
) -> None:
    class Logger:
        def __init__(self, path: pathlib.Path) -> None:
            self.path = str(path)
            self.lines: list[str] = []

        def log(self, message: str) -> None:
            self.lines.append(message)

    class ConsoleProjectionClient:
        def request_auth_headers(self) -> dict[str, str]:
            return {}

        def status(self) -> dict:
            return {
                "connected": True,
                "backend_class": "console-projection",
            }

    client = ConsoleProjectionClient()
    markers: list[str] = []
    monkeypatch.setattr(rest_perf, "RestClient", lambda *_args: client)
    monkeypatch.setattr(rest_perf, "fetch_json", lambda *_args: {})
    monkeypatch.setattr(rest_perf, "build_status_specs", lambda _bounds: [])
    monkeypatch.setattr(rest_perf, "measure", lambda *_args: ([0.01], [0.01]))
    monkeypatch.setattr(rest_perf, "report", lambda *_args: None)
    monkeypatch.setattr(
        rest_perf,
        "fetch_gateway_status_snapshot",
        lambda *_args: {},
    )
    monkeypatch.setattr(rest_perf, "gateway_status_delta", lambda *_args: {})
    monkeypatch.setattr(
        rest_perf,
        "emit_benchmark_marker",
        lambda *_args, **kwargs: markers.append(str(kwargs["phase"])),
    )
    logger = Logger(tmp_path / "perf.log")
    args = argparse.Namespace(
        rest_url="http://127.0.0.1:8080",
        timeout=1.0,
        request_auth_token="gateway-secret",
        population_mode=rest_perf.POPULATION_HOST_MODEL,
        max_workers=8,
        suite="status",
        runs=1,
        assert_min_ratio=None,
        logger=logger,
    )

    assert rest_perf.run_perf(args) == 0
    assert markers == ["start", "end"]
    summary_path = tmp_path / "perf.perf-summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    assert summary["population"] == {
        "backend_class": "console-projection",
        "discovered": 0,
        "maximum_live_tasks": None,
        "mode": "host-model",
        "proof_class": "none",
        "ready": 0,
        "requested": 8,
    }


def test_host_model_backend_metadata_error_preserves_cause() -> None:
    class BrokenStatusClient:
        def status(self) -> dict:
            raise ValueError("status unavailable")

    try:
        rest_perf.gateway_population_axes(
            BrokenStatusClient(),
            rest_perf.POPULATION_HOST_MODEL,
        )
    except rest_perf.RestError as exc:
        assert str(exc) == "host-model population requires gateway backend metadata"
        assert isinstance(exc.__cause__, ValueError)
    else:
        raise AssertionError("missing backend metadata must fail closed")


def test_host_model_backend_mismatch_fails_before_population_mutation() -> None:
    class ConsoleProjectionClient:
        def __init__(self) -> None:
            self.calls: list[str] = []

        def status(self) -> dict:
            self.calls.append("status")
            return {
                "connected": True,
                "backend_class": "console-projection",
            }

        def ls(self, path: str) -> rest_perf.GatewayResponse:
            self.calls.append(f"LS {path}")
            raise AssertionError("host-model mismatch must not list Workers")

        def echo(self, path: str, line: str) -> rest_perf.GatewayResponse:
            self.calls.append(f"ECHO {path} {line}")
            raise AssertionError("host-model mismatch must not mutate the target")

    client = ConsoleProjectionClient()
    state = rest_perf.SimState(
        bounds={},
        rest_url="http://127.0.0.1:8080",
        rng=rest_perf.random.Random(0),
        entropy=0.0,
        tail_bytes=256,
        policy_enabled=False,
        actions_enabled=False,
        telemetry_enabled=False,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=False,
        strict_control_errors=False,
    )

    try:
        rest_perf.ensure_workers(client, state, 24)
    except rest_perf.RestError as exc:
        assert "console-projection targets require executable population" in str(exc)
    else:
        raise AssertionError("target-backed host-model population must fail closed")

    assert client.calls == ["status"]
    assert state.approval_seq == 0
    assert state.population_observations == []


def test_run_simulation_rejects_backend_before_marker_or_discovery(
    monkeypatch,
) -> None:
    class Logger:
        def log(self, _message: str) -> None:
            pass

    class ConsoleProjectionClient:
        def status(self) -> dict:
            return {
                "connected": True,
                "backend_class": "console-projection",
            }

    client = ConsoleProjectionClient()
    monkeypatch.setattr(rest_perf, "RestClient", lambda *_args: client)
    monkeypatch.setattr(rest_perf, "wait_for_gateway", lambda *_args: {})
    monkeypatch.setattr(
        rest_perf,
        "emit_benchmark_marker",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(
            AssertionError("backend mismatch must precede benchmark marker")
        ),
    )
    monkeypatch.setattr(
        rest_perf,
        "discover_root_entries",
        lambda *_args: (_ for _ in ()).throw(
            AssertionError("backend mismatch must precede discovery")
        ),
    )
    args = argparse.Namespace(
        seed=26,
        entropy=0,
        version=None,
        bundle=None,
        qemu_run=None,
        gateway_bin=None,
        gateway_mock=False,
        rest_url="http://127.0.0.1:8080",
        gateway_bind=None,
        no_qemu=True,
        no_gateway=True,
        tcp_host="127.0.0.1",
        tcp_port=31337,
        ready_timeout_secs=1.0,
        qemu_smp="clusters=1,cores=4,threads=1",
        qemu_log="unused-qemu.log",
        auth_token="",
        request_auth_token="gateway-secret",
        timeout=1.0,
        population_mode=rest_perf.POPULATION_HOST_MODEL,
        logger=Logger(),
    )

    try:
        rest_perf.run_simulation(args)
    except rest_perf.RestError as exc:
        assert "console-projection targets require executable population" in str(exc)
    else:
        raise AssertionError("target-backed host-model run must fail closed")


def test_managed_gateway_mock_skips_target_tcp_preflight(monkeypatch) -> None:
    class StopAfterBackend(RuntimeError):
        pass

    class Logger:
        def log(self, _message: str) -> None:
            pass

    class HostModelClient:
        def status(self) -> dict:
            return {"connected": True, "backend_class": "host-model"}

    launched: list[list[str]] = []
    monkeypatch.setattr(rest_perf, "assert_bind_available", lambda *_args: None)
    monkeypatch.setattr(
        rest_perf,
        "wait_for_port",
        lambda *_args: (_ for _ in ()).throw(
            AssertionError("managed gateway mock must not probe target TCP")
        ),
    )
    monkeypatch.setattr(
        rest_perf,
        "validate_tcp_auth",
        lambda *_args: (_ for _ in ()).throw(
            AssertionError("managed gateway mock must not authenticate target TCP")
        ),
    )
    monkeypatch.setattr(
        rest_perf,
        "launch_process",
        lambda command, _env, _log: launched.append(list(command)) or object(),
    )
    monkeypatch.setattr(rest_perf, "terminate_process", lambda *_args: None)
    monkeypatch.setattr(rest_perf, "RestClient", lambda *_args: HostModelClient())
    monkeypatch.setattr(rest_perf, "wait_for_gateway", lambda *_args: {})
    monkeypatch.setattr(
        rest_perf,
        "discover_root_entries",
        lambda *_args: (_ for _ in ()).throw(StopAfterBackend()),
    )
    args = argparse.Namespace(
        seed=26,
        entropy=0,
        version=None,
        bundle=None,
        qemu_run=None,
        gateway_bin="hive-gateway",
        gateway_mock=True,
        rest_url="http://127.0.0.1:8080",
        gateway_bind="127.0.0.1:8080",
        no_qemu=True,
        no_gateway=False,
        tcp_host="127.0.0.1",
        tcp_port=31337,
        ready_timeout_secs=1.0,
        qemu_smp="clusters=1,cores=4,threads=1",
        qemu_log="unused-qemu.log",
        gateway_log="mock-gateway.log",
        auth_token="",
        request_auth_token="gateway-secret",
        timeout=1.0,
        population_mode=rest_perf.POPULATION_HOST_MODEL,
        role="queen",
        worker_acceptance_root=None,
        worker_acceptance_evidence=None,
        target_session=None,
        gateway_pool_control_sessions=None,
        gateway_pool_telemetry_sessions=None,
        gateway_broker_control_response_timeout_ms=None,
        gateway_broker_telemetry_response_timeout_ms=None,
        gateway_control_write_retry_window_ms=None,
        logger=Logger(),
    )

    try:
        rest_perf.run_simulation(args)
    except StopAfterBackend:
        pass
    else:
        raise AssertionError("test sentinel was not reached")

    assert launched == [["hive-gateway", "--bind", "127.0.0.1:8080", "--mock"]]


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
            assert max_bytes == rest_perf.HOST_TICKET_LOG_TAIL_BYTES
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


def test_parse_args_managed_gateway_mock_needs_no_console_secret(
    monkeypatch,
) -> None:
    monkeypatch.delenv("HIVE_GATEWAY_MOCK", raising=False)
    for name in ("COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN"):
        monkeypatch.delenv(name, raising=False)
    original_argv = list(sys.argv)
    try:
        sys.argv = [
            "rest_perf_harness.py",
            "--mode",
            "simulate",
            "--no-qemu",
            "--gateway-mock",
            "--request-auth-token",
            "gateway-secret",
        ]
        args = rest_perf.parse_args()
        assert args.gateway_mock is True
        assert args.auth_token == ""
        assert args.tail_bytes == 4096
    finally:
        sys.argv = original_argv


def test_parse_args_gateway_mock_rejects_incompatible_topologies(
    monkeypatch,
) -> None:
    monkeypatch.delenv("HIVE_GATEWAY_MOCK", raising=False)
    cases = (
        (["--gateway-mock"], "requires --no-qemu"),
        (
            ["--gateway-mock", "--no-qemu", "--no-gateway"],
            "cannot be combined with --no-gateway",
        ),
        (
            [
                "--gateway-mock",
                "--no-qemu",
                "--population-mode",
                "executable",
            ],
            "requires host-model population",
        ),
    )
    original_argv = list(sys.argv)
    try:
        for extra, expected in cases:
            sys.argv = [
                "rest_perf_harness.py",
                "--mode",
                "simulate",
                "--request-auth-token",
                "gateway-secret",
                *extra,
            ]
            try:
                rest_perf.parse_args()
            except SystemExit as exc:
                assert expected in str(exc)
            else:
                raise AssertionError("incompatible gateway mock topology must fail")
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
    assert "SYSTEM_CPIO_BYTES=\"$(stat -f" not in source
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
        'mkdir -m 0700 "$RUN_DIR"',
        "export QEMU_BIN",
        "selected QEMU binary lacks the required host accelerator",
        'export COHESIX_QEMU_ACCEL=hvf',
        'export COHESIX_QEMU_VIRT=off',
        'COHESIX_QEMU_SMP_TOPO=4,cores=4,threads=1,sockets=1',
        'exact_option("-accel", accelerator)',
        'exact_option("-machine", machine)',
        'exact_option("-cpu", cpu)',
        'wait_for_marker_count "$boot_dir/uart.live.log" "Cohesix console ready" 1 180',
        "rest.wait_for_gateway(client, 60.0)",
        'die "direct cohsh command attempted while hive-gateway owns the console"',
        'kill -INT "$pid"',
        'die "hive-gateway did not release the console gracefully"',
        'GATEWAY_OPERATOR OK KILL role=worker-heartbeat',
        'cp "$boot_dir/qemu-command.txt" "$boot_dir/preflight.qemu-command.txt"',
        'wait_for_gateway_acceptance',
        'diagnostic.get("code") != "read-failed"',
        (
            'drive_worker_fault_plan "$boot_dir" worker-lora 300\n\n'
            '    mkdir -p "$boot_dir/preflight"\n'
            '    start_gateway \\\n'
            '        "$boot_dir" \\\n'
            '        "$boot_dir/preflight/worker-task-evidence.json" \\\n'
            '        "$boot_dir/target-session.json"\n'
            '    assert_gateway_unproven\n'
            '    publish_gpu_fixture "$boot_dir"\n'
            '    drive_receipt_matrix "$boot_dir"'
        ),
        '--reuse-artifacts',
        'export COHESIX_QEMU_ACCEL=kvm',
        'export COHESIX_QEMU_CROSS_HOST_REPLAY=1',
        'verify-artifacts',
        'source-host-launch-record.json',
        'cohesix-qemu-host-replay/v1',
        'scripts/ci/test_plan_target_canary.sh --target qemu',
        'TEST_PLAN_CONVERGENCE_QEMU_BIN="$QEMU_BIN"',
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
        '--preflight-service-uart "$RUN_DIR/ninedoor-during-call/service.uart.log"',
        '--preflight-service-uart "$RUN_DIR/ninedoor-between-calls/service.uart.log"',
        '--preflight-service-uart "$RUN_DIR/console-standard-fault/service.uart.log"',
        '--auth-observation "$AUTH_OBSERVATION"',
        '--mode "$mode"',
        'ninedoor-during-call ninedoor-service during-call-standard',
        'ninedoor-between-calls ninedoor-service between-calls-revoke',
        'console-standard-fault console-network during-call-standard',
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
    assert source.count(
        'wait_for_marker_count "$boot_dir/uart.live.log" '
        '"Cohesix console ready" 1 180'
    ) == 2
    canonical_build = source.index(
        'log "building canonical release-qemu,bootstrap-trace artifacts"'
    )
    run_dir_creation = source.index('mkdir -m 0700 "$RUN_DIR"')
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
        run_dir_creation
        < canonical_build
        < target_session
        < authenticated_boot
        < during_call_boot
        < between_calls_boot
        < console_standard_boot
        < medium_boot
        < high_boot
        < staged_plan
        < final_collection
    )
    assert "rebuilding the exact pressure artifact set" not in source
    assert "pressure QEMU pidfile is malformed" not in source
    assert "export COHESIX_QEMU_ACCEL=tcg" not in source
    assert "export COHESIX_QEMU_VIRT=on" not in source
    assert "canonical TCG accelerator" not in source
    assert "console-timeout-fault" not in source
    assert "budget-exhaustion-timeout" not in source
    assert "qemu-gdb-services" not in source
    assert "staging/cohesix/artifacts/cohesix-driver-runtimes.cpio" not in source
    assert "--auth-token" not in source
    assert ': "${COH_AUTH_TOKEN:?' not in source
    assert "M26E_CONSOLE_AUTH_TOKEN=$COH_AUTH_TOKEN" not in source
    assert 'source_inventory = {' not in source
    assert 'find "$REPO_ROOT"' not in source
    preserve = source.index('PRESERVE_ROOT="$(mktemp -d ')
    canonical_run_dir = source.index(
        'RUN_DIR="$(canonical_future_dir "$RUN_DIR" "$OUT_DIR")"'
    )
    canonical_gdb = source.index(
        'GDB_BIN="$(canonical_existing_file "$GDB_BIN" "$COMPILER_DIR" yes)"'
    )
    branch_guard = source.index(
        '[[ "$(git branch --show-current)" == "main" ]]'
    )
    assert canonical_run_dir < canonical_gdb < branch_guard < preserve
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
