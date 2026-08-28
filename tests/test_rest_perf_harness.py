# Author: Lukas Bower
# Purpose: Unit tests for the REST performance harness helpers.
# Copyright 2026 Lukas Bower

"""Tests for scripts/rest_perf_harness.py helpers."""

import argparse
import ast
import copy
import hashlib
import importlib.util
import json
import os
import pathlib
import socket
import subprocess
import sys
import threading
import time
import tomllib
import urllib.error
import zlib
from dataclasses import replace
from types import SimpleNamespace
from typing import Optional

import pytest

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


def _start_tcp_auth_server(
    responses: list[str],
) -> tuple[int, threading.Thread, list[bytes], list[BaseException]]:
    """Serve one framed auth exchange on a fresh loopback listener."""

    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.settimeout(2.0)
    port = int(listener.getsockname()[1])
    requests: list[bytes] = []
    failures: list[BaseException] = []

    def recv_exact(connection: socket.socket, length: int) -> bytes:
        payload = bytearray()
        while len(payload) < length:
            part = connection.recv(length - len(payload))
            if not part:
                raise ConnectionError("auth test client closed a partial frame")
            payload.extend(part)
        return bytes(payload)

    def serve() -> None:
        try:
            with listener:
                connection, _address = listener.accept()
                with connection:
                    header = recv_exact(connection, 4)
                    total = int.from_bytes(header, "little")
                    requests.append(recv_exact(connection, total - 4))
                    for response in responses:
                        payload = response.encode("utf-8")
                        frame = (len(payload) + 4).to_bytes(4, "little") + payload
                        connection.sendall(frame)
        except BaseException as exc:  # pragma: no cover - asserted by caller
            failures.append(exc)

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    return port, thread, requests, failures


def test_validate_tcp_auth_waits_for_exact_terminal_ok() -> None:
    """An informational AUTH ACK cannot replace the terminal success ACK."""

    port, thread, requests, failures = _start_tcp_auth_server(
        ["OK AUTH detail=present-token", "OK AUTH"]
    )

    rest_perf.validate_tcp_auth("127.0.0.1", port, "correct-token", 1.0)
    thread.join(timeout=2.0)

    assert not thread.is_alive()
    assert failures == []
    assert requests == [b"AUTH correct-token"]


def test_validate_tcp_auth_rejects_terminal_err_after_information() -> None:
    """ERR AUTH must fail preflight even after a positive informational ACK."""

    port, thread, requests, failures = _start_tcp_auth_server(
        ["OK AUTH detail=present-token", "ERR AUTH detail=invalid-token"]
    )

    with pytest.raises(rest_perf.RestError, match="authentication rejected"):
        rest_perf.validate_tcp_auth("127.0.0.1", port, "wrong-token", 1.0)
    thread.join(timeout=2.0)

    assert not thread.is_alive()
    assert failures == []
    assert requests == [b"AUTH wrong-token"]


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


def pi_genet_direct_handoff_serial(*later_lines: str) -> bytes:
    """Build one minimal latest-boot direct-GENET handoff fixture."""

    lines = [
        "U-Boot 2025.10",
        (
            "[console-network] shell constructed generation=7 tcb=0x2421 "
            "state=suspended descriptor=pending-dhcp fault_registry=registered "
            "backend=bcmgenet-v5"
        ),
        (
            "CONSOLE_NETWORK_HANDOFF phase=direct-link-armed tcb=0x2421 "
            "ip=192.168.10.2/24 gateway=192.168.10.1 "
            "mac=02-00-00-00-00-02 descriptor=finalized state=suspended "
            "owner=pending-genet-command root_tcp=disabled backend=bcmgenet-v5"
        ),
        (
            "CONSOLE_NETWORK_HANDOFF phase=direct-link-complete "
            "tcb=0x2421 generation=7 ip=192.168.10.2/24 "
            "gateway=192.168.10.1 mac=02-00-00-00-00-02 state=active "
            "owner=driver-console-direct root_packet_mediation=disabled "
            "backend=bcmgenet-v5"
        ),
        (
            "netstats: generation=7 mode=dhcp policy=wired active=wired "
            "standby=wifi addr_src=dhcp-lease ip=192.168.10.2 "
            "gateway=192.168.10.1 dhcp=bound"
        ),
        *later_lines,
    ]
    return ("\n".join(lines) + "\n").encode("ascii")


def test_pi_genet_direct_handoff_accepts_one_current_terminal() -> None:
    rest_perf.validate_pi_genet_direct_handoff(pi_genet_direct_handoff_serial())


@pytest.mark.parametrize(
    "serial_raw, message",
    (
        (b"U-Boot 2025.10\n", "lacks one exact"),
        (
            pi_genet_direct_handoff_serial(
                "CONSOLE_NETWORK_HANDOFF phase=direct-link-complete "
                "tcb=0x2421 generation=8 ip=192.168.10.2/24 "
                "gateway=192.168.10.1 mac=02:00:00:00:00:02 state=active "
                "owner=driver-console-direct root_packet_mediation=disabled "
                "backend=bcmgenet-v5"
            ),
            "lacks one exact",
        ),
        (
            pi_genet_direct_handoff_serial(
                "CONSOLE_NETWORK_HANDOFF phase=direct-link-command "
                "generation=7 status=failed containment_started=true "
                "action=contain-no-fallback backend=bcmgenet-v5"
            ),
            "failed direct handoff",
        ),
        (
            pi_genet_direct_handoff_serial(
                "DRIVER_FAULT_CONTAINMENT v1 q=1 task=bcmgenet-v5 "
                "c=standard"
            ),
            "later driver fault",
        ),
        (
            pi_genet_direct_handoff_serial(
                "[console-network] generation=7 terminal-fault "
                "source=local reason=direct-genet-invalid-cursor"
            ),
            "later console fault",
        ),
        (
            pi_genet_direct_handoff_serial(
                "CONSOLE_NETWORK_TEARDOWN generation=7 state=terminal"
            ),
            "pair containment",
        ),
        (
            pi_genet_direct_handoff_serial(
                "DIRECT_GENET_CURSOR_POISON generation=7 reason=2"
            ),
            "poisoned cursor",
        ),
        (
            pi_genet_direct_handoff_serial(
                "[console-network] generation=7 fail-closed "
                "reason=direct-genet-invalid-cursor"
            ),
            "later console fault",
        ),
        (
            (
                "U-Boot 2025.10\n"
                "CONSOLE_NETWORK_HANDOFF phase=console-activate generation=7 "
                "status=failed containment_started=true "
                "action=contain-no-fallback backend=bcmgenet-v5\n"
            ).encode("ascii")
            + pi_genet_direct_handoff_serial().split(b"\n", 1)[1],
            "failed direct handoff",
        ),
        (
            (
                "U-Boot 2025.10\n"
                "DRIVER_FAULT_CONTAINMENT v1 q=1 task=bcmgenet-v5 "
                "c=standard\n"
            ).encode("ascii")
            + pi_genet_direct_handoff_serial().split(b"\n", 1)[1],
            "later driver fault",
        ),
        (
            (
                "U-Boot 2025.10\n"
                "[console-network] fault generation mismatch "
                "expected=7 observed=6\n"
            ).encode("ascii")
            + pi_genet_direct_handoff_serial().split(b"\n", 1)[1],
            "later console fault",
        ),
        (
            (
                "U-Boot 2025.10\n"
                "CONSOLE_NETWORK_TEARDOWN generation=7 state=terminal\n"
            ).encode("ascii")
            + pi_genet_direct_handoff_serial().split(b"\n", 1)[1],
            "pair containment",
        ),
    ),
)
def test_pi_genet_direct_handoff_rejects_nonterminal_evidence(
    serial_raw: bytes,
    message: str,
) -> None:
    with pytest.raises(rest_perf.RestError, match=message):
        rest_perf.validate_pi_genet_direct_handoff(serial_raw)


@pytest.mark.parametrize(
    "old, new",
    (
        ("generation=7", "generation=0"),
        ("generation=7", "generation=18446744073709551616"),
        ("tcb=0x2421", "tcb=2421"),
        ("root_packet_mediation=disabled", "root_packet_mediation=enabled"),
        ("backend=bcmgenet-v5", "backend=bcmgenet-v4"),
        ("backend=bcmgenet-v5", "backend=bcmgenet-v5 unexpected=yes"),
    ),
)
def test_pi_genet_direct_handoff_rejects_contract_drift(old: str, new: str) -> None:
    serial_raw = pi_genet_direct_handoff_serial().replace(
        old.encode("ascii"), new.encode("ascii")
    )
    with pytest.raises(rest_perf.RestError):
        rest_perf.validate_pi_genet_direct_handoff(serial_raw)


@pytest.mark.parametrize(
    "old, new",
    (
        ("phase=direct-link-armed", "phase=unknown"),
        ("descriptor=finalized", "descriptor=wrong"),
        ("owner=pending-genet-command", "owner=root"),
        ("root_tcp=disabled", "root_tcp=enabled"),
    ),
)
def test_pi_genet_direct_handoff_rejects_armed_contract_drift(
    old: str,
    new: str,
) -> None:
    serial_raw = pi_genet_direct_handoff_serial().replace(
        old.encode("ascii"), new.encode("ascii"), 1
    )
    with pytest.raises(rest_perf.RestError):
        rest_perf.validate_pi_genet_direct_handoff(serial_raw)


def test_pi_genet_direct_handoff_rejects_identity_disagreement() -> None:
    serial_raw = pi_genet_direct_handoff_serial().replace(
        b"phase=direct-link-armed tcb=0x2421 ip=192.168.10.2/24",
        b"phase=direct-link-armed tcb=0x2421 ip=192.168.10.9/24",
    )
    with pytest.raises(rest_perf.RestError, match="armed and complete identities differ"):
        rest_perf.validate_pi_genet_direct_handoff(serial_raw)


def test_pi_genet_direct_handoff_rejects_shell_after_handoff() -> None:
    lines = pi_genet_direct_handoff_serial().decode("ascii").splitlines()
    shell = lines.pop(1)
    lines.insert(3, shell)
    with pytest.raises(rest_perf.RestError, match="phases are out of order"):
        rest_perf.validate_pi_genet_direct_handoff(
            ("\n".join(lines) + "\n").encode("ascii")
        )


def test_pi_genet_network_identity_uses_direct_terminal_and_hyphen_mac() -> None:
    mac_raw, ip_raw, mac_value, ip_value = (
        rest_perf.derive_pi_serial_network_identity(
            pi_genet_direct_handoff_serial(),
            rest_perf.BENCHMARK_TRANSPORT_GENET,
        )
    )

    assert mac_raw == bytes.fromhex("020000000002")
    assert ip_raw == bytes((192, 168, 10, 2))
    assert mac_value == "02-00-00-00-00-02"
    assert ip_value == "192.168.10.2"


def test_pi_genet_direct_handoff_ignores_adverse_prior_boot() -> None:
    prior = pi_genet_direct_handoff_serial(
        "[console-network] generation=7 terminal-fault source=local reason=cursor"
    )
    latest = pi_genet_direct_handoff_serial().replace(
        b"U-Boot 2025.10", b"U-Boot 2025.11"
    )
    rest_perf.validate_pi_genet_direct_handoff(prior + latest)


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


@pytest.mark.parametrize(
    "payload",
    (
        b'{"connected":true,"connected":false}',
        b'{"connected":true,"unused":NaN}',
    ),
)
def test_live_gateway_json_rejects_ambiguous_values(
    monkeypatch,
    payload: bytes,
) -> None:
    """Acceptance-critical live responses use the strict evidence decoder."""

    class Response:
        def __enter__(self):
            return self

        def __exit__(self, _exc_type, _exc, _traceback) -> None:
            return None

        def read(self) -> bytes:
            return payload

    monkeypatch.setattr(
        rest_perf.urllib.request,
        "urlopen",
        lambda _request, timeout: Response(),
    )
    with pytest.raises(rest_perf.RestError):
        rest_perf.fetch_json("http://127.0.0.1:8080/v1/meta/status", 1.0)


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
        bounds={"control_plane": {"lease": {"active_max_entries": 2}}},
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
    try:
        rest_perf.remember_lease_id(state, "lease-3")
    except rest_perf.RestError as error:
        assert str(error) == (
            "successful lease grant exceeded the generated active lease bound"
        )
    else:
        raise AssertionError("lease tracking silently exceeded the generated bound")
    assert set(state.active_leases) == {"lease-1", "lease-2"}
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


def test_gateway_status_delta_records_connection_continuity() -> None:
    before = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {"control_requests": 2},
    }
    after = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {"control_requests": 7},
    }
    assert rest_perf.gateway_status_delta(before, after) == {
        "connection": {
            "connected_before": True,
            "connected_after": True,
            "connects": 0,
            "reconnects": 0,
        },
        "broker": {"control_requests": 5},
    }
    rest_perf.validate_pi_gateway_continuity(
        before,
        after,
        SimpleNamespace(
            gateway_connects=1,
            gateway_reconnects=0,
            gateway_last_change_unix_ms=1000,
            gateway_status_endpoint="http://127.0.0.1:8080/v1/meta/status",
            gateway_target_host="192.168.50.23",
            gateway_target_port=31337,
        ),
        "http://127.0.0.1:8080",
        "192.168.50.23",
        31337,
    )


@pytest.mark.parametrize(
    ("start_connects", "start_reconnects", "end_connects", "end_reconnects"),
    ((2, 0, 2, 0), (1, 1, 1, 1), (1, 0, 2, 0), (1, 0, 1, 1)),
)
def test_pi_gateway_continuity_rejects_prior_or_during_run_reconnect(
    start_connects: int,
    start_reconnects: int,
    end_connects: int,
    end_reconnects: int,
) -> None:
    before = {
        "connected": True,
        "connects": start_connects,
        "reconnects": start_reconnects,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {},
    }
    after = {
        "connected": True,
        "connects": end_connects,
        "reconnects": end_reconnects,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {},
    }
    with pytest.raises(rest_perf.RestError, match="zero reconnects"):
        rest_perf.validate_pi_gateway_continuity(before, after)


def test_pi_gateway_continuity_rejects_different_gate_connection() -> None:
    snapshot = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 2000,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {},
    }
    with pytest.raises(rest_perf.RestError, match="gate-captured"):
        rest_perf.validate_pi_gateway_continuity(
            snapshot,
            snapshot,
            SimpleNamespace(
                gateway_connects=1,
                gateway_reconnects=0,
                gateway_last_change_unix_ms=1000,
                gateway_target_host="192.168.50.23",
                gateway_target_port=31337,
            ),
        )


def test_pi_gateway_continuity_rejects_different_rest_endpoint() -> None:
    snapshot = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {},
    }
    evidence = SimpleNamespace(
        gateway_connects=1,
        gateway_reconnects=0,
        gateway_last_change_unix_ms=1000,
        gateway_status_endpoint="http://127.0.0.1:8080/v1/meta/status",
        gateway_target_host="192.168.50.23",
        gateway_target_port=31337,
    )
    with pytest.raises(rest_perf.RestError, match="REST endpoint differs"):
        rest_perf.validate_pi_gateway_continuity(
            snapshot,
            snapshot,
            evidence,
            "http://127.0.0.1:8081",
        )


def test_pi_gateway_continuity_rejects_different_tcp_target() -> None:
    snapshot = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {},
    }
    evidence = SimpleNamespace(
        gateway_connects=1,
        gateway_reconnects=0,
        gateway_last_change_unix_ms=1000,
        gateway_status_endpoint="http://127.0.0.1:8080/v1/meta/status",
        gateway_target_host="192.168.50.23",
        gateway_target_port=31337,
    )
    with pytest.raises(rest_perf.RestError, match="TCP host differs"):
        rest_perf.validate_pi_gateway_continuity(
            snapshot,
            snapshot,
            evidence,
            "http://127.0.0.1:8080",
            "192.168.50.24",
            31337,
        )


@pytest.mark.parametrize(
    ("field", "value", "message"),
    (
        ("target_host", "192.168.50.24", "gate-captured"),
        ("target_port", 31338, "invalid gateway start continuity"),
    ),
)
def test_pi_gateway_continuity_rejects_live_target_drift(
    field: str,
    value: object,
    message: str,
) -> None:
    """The live gateway endpoint must equal the gate-sealed Pi endpoint."""

    snapshot = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "target_host": "192.168.50.23",
        "target_port": 31337,
        "broker": {},
    }
    snapshot[field] = value
    evidence = SimpleNamespace(
        gateway_connects=1,
        gateway_reconnects=0,
        gateway_last_change_unix_ms=1000,
        gateway_target_host="192.168.50.23",
        gateway_target_port=31337,
    )
    with pytest.raises(rest_perf.RestError, match=message):
        rest_perf.validate_pi_gateway_continuity(snapshot, snapshot, evidence)


def test_pi_runtime_gateway_continuity_requires_one_in_window_session() -> None:
    runtime = {
        "PI4_RUNTIME_DMA_CAPTURE_STARTED_UNIX_NS": "1000000000",
        "PI4_RUNTIME_DMA_CAPTURE_FINISHED_UNIX_NS": "3000000000",
        "PI4_RUNTIME_DMA_GATEWAY_CONTINUITY": "connected-single-session",
        "PI4_RUNTIME_DMA_GATEWAY_STATUS_ENDPOINT": (
            "http://127.0.0.1:8080/v1/meta/status"
        ),
        "PI4_RUNTIME_DMA_GATEWAY_TARGET_HOST": "192.168.50.23",
        "PI4_RUNTIME_DMA_GATEWAY_TARGET_PORT": "31337",
        "PI4_RUNTIME_DMA_GATEWAY_START_CAPTURED_UNIX_NS": "1500000000",
        "PI4_RUNTIME_DMA_GATEWAY_START_CONNECTED": "true",
        "PI4_RUNTIME_DMA_GATEWAY_START_CONNECTS": "1",
        "PI4_RUNTIME_DMA_GATEWAY_START_RECONNECTS": "0",
        "PI4_RUNTIME_DMA_GATEWAY_START_LAST_CHANGE_UNIX_MS": "1000",
        "PI4_RUNTIME_DMA_GATEWAY_START_TARGET_HOST": "192.168.50.23",
        "PI4_RUNTIME_DMA_GATEWAY_START_TARGET_PORT": "31337",
        "PI4_RUNTIME_DMA_GATEWAY_END_CAPTURED_UNIX_NS": "2500000000",
        "PI4_RUNTIME_DMA_GATEWAY_END_CONNECTED": "true",
        "PI4_RUNTIME_DMA_GATEWAY_END_CONNECTS": "1",
        "PI4_RUNTIME_DMA_GATEWAY_END_RECONNECTS": "0",
        "PI4_RUNTIME_DMA_GATEWAY_END_LAST_CHANGE_UNIX_MS": "1000",
        "PI4_RUNTIME_DMA_GATEWAY_END_TARGET_HOST": "192.168.50.23",
        "PI4_RUNTIME_DMA_GATEWAY_END_TARGET_PORT": "31337",
    }
    assert rest_perf.pi_gateway_continuity_from_runtime(runtime) == {
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "status_endpoint": "http://127.0.0.1:8080/v1/meta/status",
        "target_host": "192.168.50.23",
        "target_port": 31337,
    }
    runtime["PI4_RUNTIME_DMA_GATEWAY_END_RECONNECTS"] = "1"
    with pytest.raises(rest_perf.RestError, match="continuity window"):
        rest_perf.pi_gateway_continuity_from_runtime(runtime)

    runtime["PI4_RUNTIME_DMA_GATEWAY_END_RECONNECTS"] = "0"
    runtime["PI4_RUNTIME_DMA_GATEWAY_START_LAST_CHANGE_UNIX_MS"] = "999"
    runtime["PI4_RUNTIME_DMA_GATEWAY_END_LAST_CHANGE_UNIX_MS"] = "999"
    with pytest.raises(rest_perf.RestError, match="continuity window"):
        rest_perf.pi_gateway_continuity_from_runtime(runtime)


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


def test_qualified_artifact_output_rejects_collision_and_symlink(
    tmp_path: pathlib.Path,
) -> None:
    collision = tmp_path / "qualified.summary.json"
    collision.write_text("preserve\n", encoding="utf-8")
    with pytest.raises(FileExistsError):
        rest_perf.open_artifact_text(str(collision), exclusive=True)
    assert collision.read_text(encoding="utf-8") == "preserve\n"

    target = tmp_path / "target.json"
    target.write_text("target\n", encoding="utf-8")
    link = tmp_path / "qualified-link.summary.json"
    link.symlink_to(target)
    with pytest.raises(OSError):
        rest_perf.open_artifact_text(str(link), exclusive=True)
    assert target.read_text(encoding="utf-8") == "target\n"

    created = tmp_path / "qualified-new.summary.json"
    with rest_perf.open_artifact_text(str(created), exclusive=True) as handle:
        handle.write("sealed\n")
    assert created.read_text(encoding="utf-8") == "sealed\n"


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
            "task_abi_schema": "worker-task-abi/v2",
            "task_abi_version": 2,
            "maximum_live_tasks": maximum,
            "canonical_telemetry_template": (
                "/shard/<label>/worker/<id>/telemetry"
            ),
            "shard_bits": 6,
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
        "notifications": 0,
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


def test_executable_population_binds_manifest_to_live_bounds() -> None:
    bounds = executable_bounds(3)
    manifest = {
        "worker_runtime": {"max_workers": 3},
        "worker_resource_admission": {
            "enabled": True,
            "executable_roles": [
                {"role": role, "executable_slots": 1}
                for role in rest_perf.EXECUTABLE_WORKER_ROLES
            ],
        },
    }
    assert (
        rest_perf.executable_population_from_manifest_and_bounds(
            manifest,
            bounds,
            "f" * 64,
        )
        == 3
    )

    wrong_hash = copy.deepcopy(bounds)
    wrong_hash["manifest_sha256"] = "e" * 64
    wrong_slots = copy.deepcopy(bounds)
    wrong_slots["worker_runtime"]["roles"][1]["executable_slots"] = 2
    wrong_slots["worker_runtime"]["maximum_live_tasks"] = 4
    for candidate in (wrong_hash, wrong_slots):
        with pytest.raises(rest_perf.RestError):
            rest_perf.executable_population_from_manifest_and_bounds(
                manifest,
                candidate,
                "f" * 64,
            )

    wrong_manifest = copy.deepcopy(manifest)
    wrong_manifest["worker_runtime"]["max_workers"] = 4
    with pytest.raises(rest_perf.RestError):
        rest_perf.executable_population_from_manifest_and_bounds(
            wrong_manifest,
            bounds,
            "f" * 64,
        )


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
    bounds = executable_bounds()
    label = rest_perf.expected_worker_shard_label(
        worker_id,
        bounds["worker_runtime"]["shard_bits"],
    )
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
        bounds,
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


def test_qemu_target_evidence_rejects_fault_artifact_reread_drift(
    tmp_path: pathlib.Path,
) -> None:
    uart, gdb = write_fault_logs(tmp_path)
    acceptance = acceptance_summary()
    args = argparse.Namespace(qemu_uart_log=str(uart), qemu_gdb_log=str(gdb))
    artifacts, _markers = rest_perf.capture_fault_artifacts(args, acceptance)
    with uart.open("a", encoding="utf-8") as handle:
        handle.write("post-capture mutation\n")
    target_session = acceptance["target_session"]
    binding = {
        "target": "qemu",
        "source_sha256": "8" * 64,
        "manifest_sha256": target_session["manifest_sha256"],
        "root_image_sha256": target_session["root_image_sha256"],
    }

    with pytest.raises(rest_perf.RestError, match="changed before target-evidence seal"):
        rest_perf.build_qemu_benchmark_target_evidence(
            acceptance,
            binding,
            artifacts,
            str(uart),
            str(gdb),
            60,
            now_unix_s=time.time(),
        )


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
            "worker": f"before-{worker['role']}",
            "receipt_sequence": 10,
            "completion_sequence": 10,
        }
        after = dict(base)
        if worker["role"] == "worker-heartbeat":
            after["worker"] = "after-worker-heartbeat"
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


def test_parse_args_rejects_mixed_or_missing_target_evidence() -> None:
    cases = (
        (
            ["--benchmark-transport", "genet"],
            "benchmark target and transport are incompatible",
        ),
        (
            ["--benchmark-target", "pi4"],
            "benchmark target and transport are incompatible",
        ),
        (
            ["--pi-runtime-dma-proof", "pi.env"],
            "QEMU benchmark target cannot consume Pi proof inputs",
        ),
        (
            ["--pi-network-capture", "pi.pcap"],
            "QEMU benchmark target cannot consume Pi proof inputs",
        ),
        (
            ["--pi-cyw43-coexistence-record", "pi-cyw43.json"],
            "QEMU benchmark target cannot consume Pi proof inputs",
        ),
        (
            [
                "--benchmark-target",
                "pi4",
                "--benchmark-transport",
                "genet",
                "--qemu-uart-log",
                "qemu.log",
            ],
            "Pi benchmark target cannot consume QEMU run/log inputs",
        ),
        (
            [
                "--mode",
                "simulate",
                "--population-mode",
                "executable",
                "--no-qemu",
                "--no-gateway",
                "--qemu-uart-log",
                "qemu.log",
                "--qemu-gdb-log",
                "qemu.gdb",
            ],
            "qualified executable population requires --target-session",
        ),
        (
            [
                "--mode",
                "simulate",
                "--population-mode",
                "executable",
                "--no-qemu",
                "--no-gateway",
                "--qemu-uart-log",
                "qemu.log",
                "--qemu-gdb-log",
                "qemu.gdb",
                "--target-session",
                "target-session.json",
            ],
            "qualified executable population requires --error-budget-rate",
        ),
        (
            [
                "--mode",
                "simulate",
                "--population-mode",
                "executable",
                "--benchmark-target",
                "pi4",
                "--benchmark-transport",
                "genet",
                "--no-qemu",
                "--no-gateway",
                "--target-session",
                "target-session.json",
                "--error-budget-rate",
                "0.01",
                "--pi-runtime-dma-proof",
                "pi.env",
                "--pi-network-capture",
                "pi.pcap",
            ],
            "requires --pi-runtime-dma-proof, --pi-network-capture, "
            "and --pi-cyw43-coexistence-record",
        ),
        (
            [
                "--mode",
                "simulate",
                "--population-mode",
                "executable",
                "--benchmark-target",
                "pi4",
                "--benchmark-transport",
                "genet",
                "--no-qemu",
                "--no-gateway",
                "--target-session",
                "target-session.json",
                "--error-budget-rate",
                "0.01",
                "--pi-runtime-dma-proof",
                "pi.env",
                "--pi-network-capture",
                "pi.pcap",
                "--pi-cyw43-coexistence-record",
                "pi-cyw43.json",
                "--tcp-port",
                "31338",
            ],
            "qualified Pi executable population requires --tcp-port 31337",
        ),
    )
    original_argv = list(sys.argv)
    try:
        for arguments, expected in cases:
            sys.argv = ["rest_perf_harness.py", *arguments]
            with pytest.raises(SystemExit, match=expected):
                rest_perf.parse_args()
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


def test_parse_args_has_no_pi_component_performance_input(capsys) -> None:
    """Qualified Pi performance cannot be relabelled as component acceptance."""

    original_argv = list(sys.argv)
    try:
        sys.argv = [
            "rest_perf_harness.py",
            "--pi-worker-component",
            "pi-worker-component.json",
        ]
        with pytest.raises(SystemExit):
            rest_perf.parse_args()
    finally:
        sys.argv = original_argv
    assert "unrecognized arguments: --pi-worker-component" in capsys.readouterr().err


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
        'executable_population="$(resolved_executable_population)"',
        '--workers-min "$executable_population"',
        '--workers-max "$executable_population"',
        '--target-session "$TARGET_SESSION"',
        "rest.executable_population_from_manifest_and_bounds(",
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
    assert "--workers-min 3" not in source
    assert "--workers-max 3" not in source
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


def pi_acceptance_summary(manifest_sha256: str, root_sha256: str) -> dict:
    """Return a target-qualified Pi variant of the component fixture."""

    acceptance = acceptance_summary()
    acceptance["target"] = "pi4"
    acceptance["execution_proof"] = "fresh-pi"
    acceptance["target_session"]["manifest_sha256"] = manifest_sha256
    acceptance["target_session"]["root_image_sha256"] = root_sha256
    for worker in acceptance["workers"]:
        worker["execution_proof"] = "fresh-pi"
    return acceptance


PI_NORMALIZED_GATE_RAW = (
    "\n".join(
        (
            "DRIVER_TASK_ACTIVE_NET=cyw43",
            "PI4_RUNTIME_DMA_PROOF=fresh-pi",
            "PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified",
            "DRIVER_TASK_DMA_BLOCKER=none",
            "DRIVER_TASK_RING_CALL_OUTSTANDING=0",
            "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT=0",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED=0",
            "TIMER_BACKEND=arch-counter",
            "TIMER_CLOCK_HZ=54000000",
            "TIMER_EL0_COUNTER=vct",
            "DUMMY_TIMER_SEEN=no",
            "NET_ACTIVE=wifi",
            "NET_ADDR_SRC=dhcp-lease",
            "NET_DHCP=bound",
            "NET_TCP_READY=yes",
            "NETTEST_PROOF=yes",
            "COHSH_TCP_AUTH_PROOF=yes",
            "WIFI_GATE=10",
            "WIFI_BLOCKER=none",
            "WIFI_DPC_PROOF=yes",
            "DRIVER_TASK_SDIO_DEDICATED=yes",
            "DRIVER_TASK_NET_DEDICATED=yes",
            "DRIVER_TASK_OWNER_STATE_PROOF=yes",
            "CYW43_BOOTSTRAP_SUPERVISOR_READY=yes",
            "WIFI_FIRMWARE_IDENTITY_PROOF=yes",
            "WIFI_CLM_READY_PROOF=yes",
            "WIFI_FIRMWARE_VERSION_PROOF=yes",
            "WIFI_CLM_VERSION_PROOF=yes",
            "WIFI_GATE7_COMPLETE=yes",
            "SDIO_IRQ158_INBAND_PROOF=yes",
            "TCP_ACCEPTS=1",
            "TCP_AUTH_SESSIONS=1",
            "TCP_RX_BYTES=1",
        )
    )
    + "\n"
).encode("ascii")


def newc_archive_fixture(members: dict[str, bytes]) -> bytes:
    """Build the small deterministic newc subset used by Pi evidence fixtures."""

    output = bytearray()
    for name, payload in (*sorted(members.items()), ("TRAILER!!!", b"")):
        name_raw = name.encode("ascii") + b"\0"
        mode = 0 if name == "TRAILER!!!" else 0o100555
        fields = (0, mode, 0, 0, 1, 0, len(payload), 0, 0, 0, 0, len(name_raw), 0)
        output.extend(b"070701")
        output.extend(b"".join(f"{field:08x}".encode("ascii") for field in fields))
        output.extend(name_raw)
        output.extend(bytes((-len(output)) & 3))
        output.extend(payload)
        output.extend(bytes((-len(output)) & 3))
    output.extend(bytes((-len(output)) & 511))
    return bytes(output)


def uimage_fixture(payload: bytes) -> bytes:
    """Build the exact U-Boot ramdisk wrapper used by the Pi fixture."""

    header = bytearray(64)
    header[0:4] = (0x27051956).to_bytes(4, "big")
    header[8:12] = (1).to_bytes(4, "big")
    header[12:16] = len(payload).to_bytes(4, "big")
    header[24:28] = (zlib.crc32(payload) & 0xFFFFFFFF).to_bytes(4, "big")
    header[28:32] = bytes((5, 22, 3, 0))
    name = b"Cohesix Pi4 driver runtimes"
    header[32 : 32 + len(name)] = name
    header[4:8] = (zlib.crc32(header) & 0xFFFFFFFF).to_bytes(4, "big")
    return bytes(header) + payload


def ethernet_ipv4_frame_fixture(
    destination_mac: bytes,
    source_mac: bytes,
    source_ip: bytes,
    destination_ip: bytes,
    protocol: int,
    transport_payload: bytes,
) -> bytes:
    """Build one bounded Ethernet/IPv4 frame for capture-correlation tests."""

    total_length = 20 + len(transport_payload)
    ipv4 = bytearray(20)
    ipv4[0] = 0x45
    ipv4[2:4] = total_length.to_bytes(2, "big")
    ipv4[8] = 64
    ipv4[9] = protocol
    ipv4[12:16] = source_ip
    ipv4[16:20] = destination_ip
    return (
        destination_mac
        + source_mac
        + b"\x08\x00"
        + bytes(ipv4)
        + transport_payload
    )


def correlated_pcap_fixture(
    packet_unix_ns: int,
    station_mac: bytes,
    station_ip: bytes,
) -> bytes:
    """Build DHCP and TCP-console frames for one serial-selected Pi lane."""

    host_mac = bytes.fromhex("020000000001")
    host_ip = bytes((192, 168, station_ip[2], 1))
    dhcp_payload = (
        (68).to_bytes(2, "big")
        + (67).to_bytes(2, "big")
        + (9).to_bytes(2, "big")
        + b"\0\0"
        + b"D"
    )
    dhcp_frame = ethernet_ipv4_frame_fixture(
        b"\xff" * 6,
        station_mac,
        b"\0" * 4,
        b"\xff" * 4,
        17,
        dhcp_payload,
    )
    tcp_header = bytearray(20)
    tcp_header[0:2] = (50_000).to_bytes(2, "big")
    tcp_header[2:4] = rest_perf.PI_CONSOLE_TCP_PORT.to_bytes(2, "big")
    tcp_header[12] = 5 << 4
    tcp_header[13] = 0x18
    tcp_frame = ethernet_ipv4_frame_fixture(
        station_mac,
        host_mac,
        host_ip,
        station_ip,
        6,
        bytes(tcp_header) + b"AUTH",
    )
    header = bytes.fromhex("d4c3b2a1020004000000000000000000ffff000001000000")
    records = bytearray(header)
    for index, frame in enumerate((dhcp_frame, tcp_frame)):
        seconds, nanoseconds = divmod(
            packet_unix_ns + index * 1_000_000,
            1_000_000_000,
        )
        records.extend(seconds.to_bytes(4, "little"))
        records.extend((nanoseconds // 1_000).to_bytes(4, "little"))
        records.extend(len(frame).to_bytes(4, "little") * 2)
        records.extend(frame)
    return bytes(records)


def rewrite_env_fixture(path: pathlib.Path, updates: dict[str, str]) -> None:
    """Rewrite selected fields in a deterministic test-only env fixture."""

    observed = set()
    lines = []
    for line in path.read_text(encoding="utf-8").splitlines():
        key, separator, _value = line.partition("=")
        assert separator == "="
        if key in updates:
            line = f"{key}={updates[key]}"
            observed.add(key)
        lines.append(line)
    assert observed == set(updates)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_pi_benchmark_proof_chain(
    tmp_path: pathlib.Path,
) -> tuple[pathlib.Path, pathlib.Path, dict, dict]:
    """Write retained build/runtime/log bytes for Pi evidence tests."""

    manifest = tmp_path / "cohesix-root-task-resolved.json"
    manifest.write_text('{"schema":"test"}\n', encoding="utf-8")
    manifest_sha256 = hashlib.sha256(manifest.read_bytes()).hexdigest()
    role_inventory = {
        "tcbs": 1,
        "scheduling_contexts": 1,
        "reply_objects": 1,
        "vspaces": 1,
        "cnodes": 1,
        "page_tables": 8,
        "asids": 1,
        "frames": 16,
        "endpoints": 1,
        "notifications": 0,
        "fault_caps": 1,
        "timeout_fault_caps": 1,
        "cspace_slots": 64,
        "untyped_bytes": 1_048_576,
    }
    role_cores = (
        ("worker-heartbeat", 3),
        ("worker-gpu", 2),
        ("worker-lora", 3),
    )
    topology_payload = {
        "profile": {"name": "pi4-uboot-aarch64"},
        "worker_resource_admission": {
            "executable_roles": [
                {
                    "role": role,
                    "task_prefix": f"{role}-slot-",
                    "executable_slots": 1,
                    "core": core,
                    "per_slot": role_inventory,
                }
                for role, core in role_cores
            ]
        },
        "temporal_authority": {
            "tasks": [
                {
                    "id": f"{role}-slot-0",
                    "kind": "worker",
                    "execution": "passive",
                    "core": core,
                    "budget_us": 0,
                    "period_us": 0,
                }
                for role, core in role_cores
            ]
        },
    }
    topology_sha256 = rest_perf.canonical_json_sha256(topology_payload)
    topology = tmp_path / "cohesix-root-task-topology.json"
    topology.write_text(
        json.dumps(
            {
                "schema": rest_perf.GENERATED_TOPOLOGY_SCHEMA,
                "profile": "pi4-uboot-aarch64",
                "manifest_sha256": manifest_sha256,
                "topology_sha256": topology_sha256,
                "topology": topology_payload,
                "inventory": {},
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    driver_archive = tmp_path / "cohesix-driver-runtimes.cpio"
    driver_archive.write_bytes(b"exact-driver-runtime-cpio")
    driver_manifest = tmp_path / "cohesix-driver-runtime-manifest.json"
    driver_manifest.write_text(
        json.dumps(
            {
                "schema": "cohesix-driver-runtime-manifest/v1",
                "archive": {
                    "bytes": len(driver_archive.read_bytes()),
                    "sha256": hashlib.sha256(
                        driver_archive.read_bytes()
                    ).hexdigest(),
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    runtime_uimage = tmp_path / "cohesix-driver-runtimes.cpio.uimg"
    runtime_uimage.write_bytes(uimage_fixture(driver_archive.read_bytes()))
    worker_archive = tmp_path / "cohesix-worker-images.cpio"
    worker_archive.write_bytes(b"exact-worker-archive")
    worker_manifest = tmp_path / "cohesix-worker-image-manifest.json"
    worker_manifest.write_text(
        json.dumps(
            {
                "schema": "cohesix-worker-image-manifest/v1",
                "target": "aarch64-unknown-none",
                "archive": {
                    "bytes": len(worker_archive.read_bytes()),
                    "sha256": hashlib.sha256(
                        worker_archive.read_bytes()
                    ).hexdigest(),
                },
                "images": [
                    {
                        "role": role,
                        "image_sha256": hashlib.sha256(
                            f"{role}-image".encode("ascii")
                        ).hexdigest(),
                    }
                    for role, _core in role_cores
                ],
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    source_manifest_raw = b"exact-pi-source-manifest"
    source_inventory = tmp_path / "source-inventory.json"
    source_inventory.write_text(
        json.dumps(
            {
                "schema": "cohesix-source-inventory/v1",
                "algorithm": "git-visible-paths-sha256",
                "entries": [
                    {
                        "path": "configs/root_task_pi4_uboot_aarch64.toml",
                        "kind": "file",
                        "mode": 0o644,
                        "sha256": hashlib.sha256(
                            source_manifest_raw
                        ).hexdigest(),
                        "bytes": len(source_manifest_raw),
                    }
                ],
            },
            sort_keys=True,
            separators=(",", ":"),
        )
        + "\n",
        encoding="utf-8",
    )
    worker_abi = tmp_path / "worker-abi-identity.json"
    worker_abi.write_text('{"abi":"exact"}\n', encoding="utf-8")
    kernel = tmp_path / "kernel.elf"
    kernel.write_bytes(b"exact-kernel-elf")
    root = tmp_path / "rootserver"
    root.write_bytes(b"exact-root-elf")
    root_cpio = tmp_path / "archive.archive.o.cpio"
    root_cpio.write_bytes(
        newc_archive_fixture(
            {
                "kernel.elf": kernel.read_bytes(),
                "rootserver": root.read_bytes(),
            }
        )
    )
    image = tmp_path / "cohesix-image-arm-bcm2711"
    image.write_bytes(b"sealed-pi-image")
    capture_started_ns = time.time_ns() - 1_000_000_000
    packet_ns = time.time_ns()
    capture = tmp_path / "pi4-cyw43-network.pcap"
    wifi_mac = bytes.fromhex("02434f485832")
    wifi_ip = bytes((192, 168, 50, 23))
    capture.write_bytes(
        correlated_pcap_fixture(packet_ns, wifi_mac, wifi_ip)
    )
    serial = tmp_path / "pi4-cyw43-serial.log"
    image_sha256 = hashlib.sha256(image.read_bytes()).hexdigest()
    root_sha256 = hashlib.sha256(root.read_bytes()).hexdigest()
    root_cpio_sha256 = hashlib.sha256(root_cpio.read_bytes()).hexdigest()
    git_commit = "6" * 40
    build_timestamp = "2026-08-27T00:00:00Z"
    build_id = "9" * 64
    image_id = "5" * 64
    build_marker = (
        f"[BUILD] {git_commit[:12]} {build_timestamp} image-id={image_id} "
        "features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 "
        "net-console:1 qemu-driver-task-smoke:0]"
    )
    serial.write_text(
        "U-Boot 2026.01\n"
        "[cohesix:root-task] Cohesix boot: root-task online\n"
        f"{build_marker}\n"
        "[net-console] ready ip=192.168.50.23 port=31337 "
        "mac=02:43:4f:48:58:32\n"
        "netstats: generation=1 mode=dhcp policy=wifi active=wifi "
        "standby=wired addr_src=dhcp-lease ip=192.168.50.23 "
        "gateway=192.168.50.1 dhcp=bound\n",
        encoding="utf-8",
    )
    capture_finished_ns = time.time_ns()
    metadata = tmp_path / "pi4-image-identity.json"
    metadata.write_text(
        json.dumps(
            {
                "schema": "cohesix-pi4-image-identity/v2",
                "git_commit": git_commit,
                "embedded_git_commit": git_commit[:12],
                "source_tree_clean": True,
                "build_timestamp": build_timestamp,
                "build_id": build_id,
                "image_id": image_id,
                "build_marker": build_marker,
                "build_marker_sha256": hashlib.sha256(
                    build_marker.encode("ascii")
                ).hexdigest(),
                "image_sha256": image_sha256,
                "size_bytes": len(image.read_bytes()),
                "rootserver_sha256": root_sha256,
                "rootserver_cpio_sha256": root_cpio_sha256,
            }
        )
        + "\n",
        encoding="utf-8",
    )
    profile_stamp = tmp_path / "cohesix-sel4-profile-build-inputs.json"
    profile_stamp.write_text(
        '{"profile":"pi4_diagnostic","schema":"fixture/v1"}\n',
        encoding="utf-8",
    )
    profile_state = tmp_path / "cohesix-sel4-profile-tree-state.sha256"
    profile_state.write_text("2" * 64 + "\n", encoding="ascii")
    composition_record = tmp_path / "cohesix-composition-profile-build-inputs.json"
    composition_record.write_text(
        '{"profile":"pi4_diagnostic","schema":"fixture/v1"}\n',
        encoding="utf-8",
    )
    composition_cache = tmp_path / "cohesix-composition-CMakeCache.txt"
    composition_cache.write_text("KernelIsMCS:BOOL=ON\n", encoding="utf-8")
    composition_timer = tmp_path / "cohesix-composition-platform_gen.h"
    composition_timer.write_text(
        "#define TIMER_CLOCK_HZ 54000000ULL\n",
        encoding="utf-8",
    )
    provenance = tmp_path / "cohesix-image-arm-bcm2711.provenance.json"
    provenance.write_text(
        json.dumps(
            {
                "schema": rest_perf.PI_WRAPPER_PROVENANCE_SCHEMA,
                "git_commit": git_commit,
                "source_tree_clean": True,
                "build_timestamp": build_timestamp,
                "root_task_features": "kernel,bootstrap-trace,serial-console,net",
                "source_manifest_sha256": hashlib.sha256(
                    source_manifest_raw
                ).hexdigest(),
                "resolved_manifest_sha256": manifest_sha256,
                "topology_sha256": hashlib.sha256(topology.read_bytes()).hexdigest(),
                "source_inventory_sha256": hashlib.sha256(
                    source_inventory.read_bytes()
                ).hexdigest(),
                "worker_abi_identity_sha256": hashlib.sha256(
                    worker_abi.read_bytes()
                ).hexdigest(),
                "canonical_profile_stamp_sha256": hashlib.sha256(
                    profile_stamp.read_bytes()
                ).hexdigest(),
                "canonical_profile_state_sha256": profile_state.read_text(
                    encoding="ascii"
                ).strip(),
                "composition_record_sha256": hashlib.sha256(
                    composition_record.read_bytes()
                ).hexdigest(),
                "composition_cmake_cache_sha256": hashlib.sha256(
                    composition_cache.read_bytes()
                ).hexdigest(),
                "composition_timer_header_sha256": hashlib.sha256(
                    composition_timer.read_bytes()
                ).hexdigest(),
                "wrapper_sha256": image_sha256,
                "kernel_elf_sha256": hashlib.sha256(kernel.read_bytes()).hexdigest(),
                "rootserver_sha256": root_sha256,
                "rootserver_cpio_sha256": root_cpio_sha256,
                "driver_runtime_cpio_sha256": hashlib.sha256(
                    driver_archive.read_bytes()
                ).hexdigest(),
                "driver_runtime_manifest_sha256": hashlib.sha256(
                    driver_manifest.read_bytes()
                ).hexdigest(),
                "worker_image_archive_sha256": hashlib.sha256(
                    worker_archive.read_bytes()
                ).hexdigest(),
                "worker_image_manifest_sha256": hashlib.sha256(
                    worker_manifest.read_bytes()
                ).hexdigest(),
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    staged_artifacts = (
        ("PI4_RUNTIME_DMA_MANIFEST", manifest),
        ("PI4_RUNTIME_DMA_TOPOLOGY", topology),
        ("PI4_RUNTIME_DMA_RUNTIME_CPIO", driver_archive),
        ("PI4_RUNTIME_DMA_RUNTIME_UIMAGE", runtime_uimage),
        ("PI4_RUNTIME_DMA_STAGED_IMAGE", image),
        ("PI4_IMAGE_IDENTITY_METADATA", metadata),
        ("PI4_IMAGE_IDENTITY_WRAPPER_PROVENANCE", provenance),
        ("PI4_RUNTIME_DMA_CANONICAL_PROFILE_STAMP", profile_stamp),
        ("PI4_RUNTIME_DMA_CANONICAL_PROFILE_STATE", profile_state),
        ("PI4_RUNTIME_DMA_COMPOSITION_RECORD", composition_record),
        ("PI4_RUNTIME_DMA_COMPOSITION_CMAKE_CACHE", composition_cache),
        ("PI4_RUNTIME_DMA_COMPOSITION_TIMER_HEADER", composition_timer),
        ("PI4_IMAGE_IDENTITY_KERNEL_ELF", kernel),
        ("PI4_IMAGE_IDENTITY_ROOT_ELF", root),
        ("PI4_IMAGE_IDENTITY_ROOT_CPIO", root_cpio),
        ("PI4_IMAGE_IDENTITY_DRIVER_MANIFEST", driver_manifest),
        ("PI4_IMAGE_IDENTITY_WORKER_ARCHIVE", worker_archive),
        ("PI4_IMAGE_IDENTITY_WORKER_MANIFEST", worker_manifest),
        ("PI4_IMAGE_IDENTITY_SOURCE_INVENTORY", source_inventory),
        ("PI4_IMAGE_IDENTITY_WORKER_ABI", worker_abi),
    )
    stage_fields = [
        "PI4_RUNTIME_DMA_PROOF_ARTIFACT_VERSION=2",
        "PI4_RUNTIME_DMA_PROOF=target-build",
        "PI4_RUNTIME_DMA_PROFILE=bounded-no-iommu",
        "PI4_IMAGE_IDENTITY_SCHEME=cohesix-pi4-image-identity/v2",
        f"PI4_IMAGE_IDENTITY_GIT_COMMIT={git_commit}",
        f"PI4_IMAGE_IDENTITY_BUILD_TIMESTAMP={build_timestamp}",
        f"PI4_IMAGE_IDENTITY_BUILD_ID={build_id}",
        "PI4_IMAGE_IDENTITY_SOURCE_TREE_CLEAN=yes",
    ]
    for field, path in staged_artifacts:
        raw = path.read_bytes()
        stage_fields.extend(
            (
                f"{field}={path}",
                f"{field}_SHA256={hashlib.sha256(raw).hexdigest()}",
                f"{field}_BYTES={len(raw)}",
            )
        )
    stage = tmp_path / "stage.env"
    stage.write_text("\n".join(stage_fields) + "\n", encoding="utf-8")
    capture_id = "a" * 32
    capture_interface = "en0"
    capture_started_utc = time.strftime(
        "%Y-%m-%dT%H:%M:%SZ",
        time.gmtime(capture_started_ns // 1_000_000_000),
    )
    capture_finished_utc = time.strftime(
        "%Y-%m-%dT%H:%M:%SZ",
        time.gmtime(capture_finished_ns // 1_000_000_000),
    )
    gateway_change_ms = capture_started_ns // 1_000_000
    runtime = tmp_path / "pi4-cyw43-runtime-proof.env"
    runtime.write_text(
        "\n".join(
            (
                "PI4_RUNTIME_DMA_PROOF_ARTIFACT_VERSION=1",
                f"PI4_RUNTIME_DMA_SERIAL_LOG={serial}",
                f"PI4_RUNTIME_DMA_STAGE_BUILD_PROOF={stage}",
                "PI4_RUNTIME_DMA_STAGE_BUILD_PROOF_SHA256="
                + hashlib.sha256(stage.read_bytes()).hexdigest(),
                "PI4_RUNTIME_DMA_PROOF=fresh-pi",
                "PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified",
                "DRIVER_TASK_ACTIVE_NET=cyw43",
                "DRIVER_TASK_DMA_BLOCKER=none",
                "DRIVER_TASK_RING_CALL_OUTSTANDING=0",
                "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT=0",
                "DRIVER_TASK_BOOTSTRAP_DEFERRED=0",
                "TIMER_BACKEND=arch-counter",
                "TIMER_CLOCK_HZ=54000000",
                "TIMER_EL0_COUNTER=vct",
                "DUMMY_TIMER_SEEN=no",
                "PI4_RUNTIME_DMA_CAPTURE_PAIRING=controlled-concurrent",
                f"PI4_RUNTIME_DMA_CAPTURE_ID={capture_id}",
                f"PI4_RUNTIME_DMA_NETWORK_INTERFACE={capture_interface}",
                f"PI4_RUNTIME_DMA_NETWORK_CAPTURE={capture}",
                "PI4_RUNTIME_DMA_SERIAL_LOG_SHA256="
                + hashlib.sha256(serial.read_bytes()).hexdigest(),
                f"PI4_RUNTIME_DMA_SERIAL_LOG_BYTES={len(serial.read_bytes())}",
                "PI4_RUNTIME_DMA_NETWORK_CAPTURE_SHA256="
                + hashlib.sha256(capture.read_bytes()).hexdigest(),
                f"PI4_RUNTIME_DMA_NETWORK_CAPTURE_BYTES={len(capture.read_bytes())}",
                f"PI4_RUNTIME_DMA_CAPTURE_STARTED_AT_UTC={capture_started_utc}",
                f"PI4_RUNTIME_DMA_CAPTURE_FINISHED_AT_UTC={capture_finished_utc}",
                f"PI4_RUNTIME_DMA_CAPTURE_STARTED_UNIX_NS={capture_started_ns}",
                f"PI4_RUNTIME_DMA_CAPTURE_FINISHED_UNIX_NS={capture_finished_ns}",
                "PI4_RUNTIME_DMA_GATEWAY_CONTINUITY=connected-single-session",
                "PI4_RUNTIME_DMA_GATEWAY_STATUS_ENDPOINT="
                "http://127.0.0.1:8080/v1/meta/status",
                "PI4_RUNTIME_DMA_GATEWAY_TARGET_HOST=192.168.50.23",
                "PI4_RUNTIME_DMA_GATEWAY_TARGET_PORT=31337",
                "PI4_RUNTIME_DMA_GATEWAY_START_CAPTURED_UNIX_NS="
                f"{capture_started_ns + 1}",
                "PI4_RUNTIME_DMA_GATEWAY_START_CONNECTED=true",
                "PI4_RUNTIME_DMA_GATEWAY_START_CONNECTS=1",
                "PI4_RUNTIME_DMA_GATEWAY_START_RECONNECTS=0",
                "PI4_RUNTIME_DMA_GATEWAY_START_LAST_CHANGE_UNIX_MS="
                f"{gateway_change_ms}",
                "PI4_RUNTIME_DMA_GATEWAY_START_TARGET_HOST=192.168.50.23",
                "PI4_RUNTIME_DMA_GATEWAY_START_TARGET_PORT=31337",
                "PI4_RUNTIME_DMA_GATEWAY_END_CAPTURED_UNIX_NS="
                f"{capture_finished_ns - 1}",
                "PI4_RUNTIME_DMA_GATEWAY_END_CONNECTED=true",
                "PI4_RUNTIME_DMA_GATEWAY_END_CONNECTS=1",
                "PI4_RUNTIME_DMA_GATEWAY_END_RECONNECTS=0",
                "PI4_RUNTIME_DMA_GATEWAY_END_LAST_CHANGE_UNIX_MS="
                f"{gateway_change_ms}",
                "PI4_RUNTIME_DMA_GATEWAY_END_TARGET_HOST=192.168.50.23",
                "PI4_RUNTIME_DMA_GATEWAY_END_TARGET_PORT=31337",
            )
        )
        + "\n",
        encoding="utf-8",
    )
    bounds = executable_bounds()
    bounds["manifest_sha256"] = manifest_sha256
    acceptance = pi_acceptance_summary(manifest_sha256, root_sha256)
    acceptance["topology_sha256"] = topology_sha256
    acceptance["target_session"]["worker_archive_sha256"] = hashlib.sha256(
        worker_archive.read_bytes()
    ).hexdigest()
    acceptance["target_session"]["worker_image_manifest_sha256"] = hashlib.sha256(
        worker_manifest.read_bytes()
    ).hexdigest()
    acceptance["target_session"]["worker_abi_sha256"] = hashlib.sha256(
        worker_abi.read_bytes()
    ).hexdigest()
    session_projection = {
        "target": "pi4",
        "source_sha256": hashlib.sha256(source_inventory.read_bytes()).hexdigest(),
        "manifest_sha256": manifest_sha256,
        "kernel_sha256": hashlib.sha256(kernel.read_bytes()).hexdigest(),
        "root_image_sha256": root_sha256,
        "driver_archive_sha256": hashlib.sha256(
            driver_archive.read_bytes()
        ).hexdigest(),
        "driver_manifest_sha256": hashlib.sha256(
            driver_manifest.read_bytes()
        ).hexdigest(),
        "worker_archive_sha256": hashlib.sha256(
            worker_archive.read_bytes()
        ).hexdigest(),
        "worker_image_manifest_sha256": hashlib.sha256(
            worker_manifest.read_bytes()
        ).hexdigest(),
        "worker_abi_sha256": hashlib.sha256(worker_abi.read_bytes()).hexdigest(),
    }
    cyw43_coexistence = tmp_path / "pi4-cyw43-coexistence.json"
    cyw43_coexistence.write_text(
        json.dumps(
            {
                "schema": rest_perf.PI_CYW43_COEXISTENCE_SCHEMA,
                "producer": "pi4_gate_proof/v1",
                "target": "pi4",
                "transport": "wifi",
                "capture_id": capture_id,
                "captured_unix_s": capture_finished_ns // 1_000_000_000,
                "selected": True,
                "classification": "positive-exact-image-live-closure",
                "session_projection": session_projection,
                "topology_sha256": topology_sha256,
                "image_identity": {
                    "image_sha256": image_sha256,
                    "image_id": image_id,
                    "git_commit": git_commit,
                    "build_timestamp": build_timestamp,
                    "build_marker": build_marker,
                    "build_marker_sha256": hashlib.sha256(
                        build_marker.encode("ascii")
                    ).hexdigest(),
                },
                "runtime": {
                    "runtime_evidence_sha256": hashlib.sha256(
                        runtime.read_bytes()
                    ).hexdigest(),
                    "serial_sha256": hashlib.sha256(
                        serial.read_bytes()
                    ).hexdigest(),
                    "serial_bytes": len(serial.read_bytes()),
                    "latest_boot_offset": 0,
                    "normalized_gate_sha256": hashlib.sha256(
                        PI_NORMALIZED_GATE_RAW
                    ).hexdigest(),
                },
                "network_capture": {
                    "sha256": hashlib.sha256(capture.read_bytes()).hexdigest(),
                    "bytes": len(capture.read_bytes()),
                    "format": "pcap-us",
                    "link_type": 1,
                    "interface": capture_interface,
                    "capture_started_unix_ns": capture_started_ns,
                    "capture_finished_unix_ns": capture_finished_ns,
                },
                "outcomes": {
                    **rest_perf.PI_CYW43_REQUIRED_OUTCOMES,
                    "tcp_accepts": 1,
                    "tcp_auth_sessions": 1,
                    "tcp_rx_bytes": 1,
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    target_session = {
        **session_projection,
        "cyw43_coexistence_record_sha256": hashlib.sha256(
            cyw43_coexistence.read_bytes()
        ).hexdigest(),
    }
    target_session_path = tmp_path / "target-session.json"
    target_session_path.write_text(
        json.dumps(target_session, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    acceptance["target_session"]["target_session_sha256"] = hashlib.sha256(
        target_session_path.read_bytes()
    ).hexdigest()
    worker_component = tmp_path / "pi4-worker-component.json"
    worker_component.write_text(
        json.dumps(
            {
                "schema": "cohesix-worker-task-evidence/v1",
                "record_kind": "target-component",
                "target": "pi4",
                "verdict": "PASS",
                "target_session": target_session,
                "topology_sha256": topology_sha256,
                "raw_evidence": [
                    {
                        "id": identifier,
                        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                        "bytes": len(path.read_bytes()),
                    }
                    for identifier, path in (
                        ("pi4-network-capture", capture),
                        ("pi4-runtime-dma-proof", runtime),
                        ("pi4-serial-boot", serial),
                    )
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    acceptance["evidence_sha256"] = hashlib.sha256(
        worker_component.read_bytes()
    ).hexdigest()
    return runtime, target_session_path, bounds, acceptance


def write_pi_genet_current_proof(
    tmp_path: pathlib.Path,
    acceptance: dict,
) -> tuple[pathlib.Path, pathlib.Path, pathlib.Path, dict]:
    """Write a separate current GENET boot beside retained WiFi siblings."""

    started_ns = time.time_ns() - 1_000_000_000
    serial = tmp_path / "pi4-genet-serial.log"
    serial_text = (
        (tmp_path / "pi4-cyw43-serial.log")
        .read_text(encoding="utf-8")
        .replace("192.168.50.23", "192.168.10.50")
        .replace("192.168.50.1", "192.168.10.1")
        .replace("02:43:4f:48:58:32", "02:43:4f:48:58:31")
        .replace("policy=wifi active=wifi", "policy=wired active=wired")
        .replace("standby=wired", "standby=wifi")
    )
    serial_text = serial_text.replace(
        "[net-console] ready ip=192.168.10.50 port=31337 "
        "mac=02:43:4f:48:58:31\n",
        (
            "[console-network] shell constructed generation=7 tcb=0x2421 "
            "state=suspended descriptor=pending-dhcp fault_registry=registered "
            "backend=bcmgenet-v5\n"
            "CONSOLE_NETWORK_HANDOFF phase=direct-link-armed tcb=0x2421 "
            "ip=192.168.10.50/24 gateway=192.168.10.1 "
            "mac=02-43-4f-48-58-31 descriptor=finalized state=suspended "
            "owner=pending-genet-command root_tcp=disabled backend=bcmgenet-v5\n"
            "CONSOLE_NETWORK_HANDOFF phase=direct-link-complete tcb=0x2421 "
            "generation=7 ip=192.168.10.50/24 gateway=192.168.10.1 "
            "mac=02-43-4f-48-58-31 state=active "
            "owner=driver-console-direct root_packet_mediation=disabled "
            "backend=bcmgenet-v5\n"
        ),
    )
    serial.write_text(serial_text, encoding="utf-8")
    capture = tmp_path / "pi4-genet-network.pcap"
    capture.write_bytes(
        correlated_pcap_fixture(
            time.time_ns(),
            bytes.fromhex("02434f485831"),
            bytes((192, 168, 10, 50)),
        )
    )
    finished_ns = time.time_ns()
    runtime = tmp_path / "pi4-genet-runtime-proof.env"
    runtime.write_bytes((tmp_path / "pi4-cyw43-runtime-proof.env").read_bytes())
    rewrite_env_fixture(
        runtime,
        {
            "PI4_RUNTIME_DMA_SERIAL_LOG": str(serial),
            "DRIVER_TASK_ACTIVE_NET": "genet",
            "PI4_RUNTIME_DMA_CAPTURE_ID": "b" * 32,
            "PI4_RUNTIME_DMA_NETWORK_INTERFACE": "en8",
            "PI4_RUNTIME_DMA_NETWORK_CAPTURE": str(capture),
            "PI4_RUNTIME_DMA_SERIAL_LOG_SHA256": hashlib.sha256(
                serial.read_bytes()
            ).hexdigest(),
            "PI4_RUNTIME_DMA_SERIAL_LOG_BYTES": str(len(serial.read_bytes())),
            "PI4_RUNTIME_DMA_NETWORK_CAPTURE_SHA256": hashlib.sha256(
                capture.read_bytes()
            ).hexdigest(),
            "PI4_RUNTIME_DMA_NETWORK_CAPTURE_BYTES": str(len(capture.read_bytes())),
            "PI4_RUNTIME_DMA_GATEWAY_TARGET_HOST": "192.168.10.50",
            "PI4_RUNTIME_DMA_GATEWAY_START_TARGET_HOST": "192.168.10.50",
            "PI4_RUNTIME_DMA_GATEWAY_END_TARGET_HOST": "192.168.10.50",
            "PI4_RUNTIME_DMA_CAPTURE_STARTED_AT_UTC": time.strftime(
                "%Y-%m-%dT%H:%M:%SZ",
                time.gmtime(started_ns // 1_000_000_000),
            ),
            "PI4_RUNTIME_DMA_CAPTURE_FINISHED_AT_UTC": time.strftime(
                "%Y-%m-%dT%H:%M:%SZ",
                time.gmtime(finished_ns // 1_000_000_000),
            ),
            "PI4_RUNTIME_DMA_CAPTURE_STARTED_UNIX_NS": str(started_ns),
            "PI4_RUNTIME_DMA_CAPTURE_FINISHED_UNIX_NS": str(finished_ns),
            "PI4_RUNTIME_DMA_GATEWAY_START_CAPTURED_UNIX_NS": str(
                started_ns + 1
            ),
            "PI4_RUNTIME_DMA_GATEWAY_START_LAST_CHANGE_UNIX_MS": str(
                started_ns // 1_000_000
            ),
            "PI4_RUNTIME_DMA_GATEWAY_END_CAPTURED_UNIX_NS": str(
                finished_ns - 1
            ),
            "PI4_RUNTIME_DMA_GATEWAY_END_LAST_CHANGE_UNIX_MS": str(
                started_ns // 1_000_000
            ),
        },
    )
    session = json.loads(
        (tmp_path / "target-session.json").read_text(encoding="utf-8")
    )
    component = json.loads(
        (tmp_path / "pi4-worker-component.json").read_text(encoding="utf-8")
    )
    component["target_session"] = session
    component["raw_evidence"] = [
        {
            "id": identifier,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "bytes": len(path.read_bytes()),
        }
        for identifier, path in (
            ("pi4-network-capture", capture),
            ("pi4-runtime-dma-proof", runtime),
            ("pi4-serial-boot", serial),
        )
    ]
    component_path = tmp_path / "pi4-genet-worker-component.json"
    component_path.write_text(
        json.dumps(component, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    updated_acceptance = copy.deepcopy(acceptance)
    updated_acceptance["evidence_sha256"] = hashlib.sha256(
        component_path.read_bytes()
    ).hexdigest()
    return runtime, capture, component_path, updated_acceptance


def stub_pi_canonical_validators(monkeypatch) -> None:
    """Keep focused evidence tests independent of separately tested CLIs."""

    monkeypatch.setattr(
        rest_perf,
        "validate_pi_archive_manifests",
        lambda *_args: None,
    )
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_image_identity",
        lambda *_args: None,
    )


def test_pi_live_log_is_explicitly_nonclaiming() -> None:
    class PiClient:
        def status(self) -> dict:
            return {"connected": True, "backend_class": "console-projection"}

    assert rest_perf.gateway_population_axes(
        PiClient(),
        rest_perf.POPULATION_EXECUTABLE_LOG,
        executable_bounds(),
        rest_perf.BENCHMARK_TARGET_PI4,
    ) == ("console-projection", "none")
    assert rest_perf.gateway_population_axes(
        PiClient(),
        rest_perf.POPULATION_EXECUTABLE_LOG,
        executable_bounds(),
        rest_perf.BENCHMARK_TARGET_QEMU,
    ) == ("console-projection", "qemu-live-log")


def test_pi_target_evidence_is_derived_from_exact_artifacts(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session, session_raw = rest_perf.load_target_session_binding_snapshot(
        str(session_path), "pi4", bounds, None
    )
    observed: list[tuple[bytes, str]] = []
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda path, transport: (
            observed.append((path, transport)) or PI_NORMALIZED_GATE_RAW
        ),
    )

    proof = rest_perf.load_pi_benchmark_target_evidence(
        str(runtime),
        str(tmp_path / "pi4-cyw43-network.pcap"),
        str(tmp_path / "pi4-cyw43-coexistence.json"),
        str(session_path),
        rest_perf.BENCHMARK_TRANSPORT_WIFI,
        bounds,
        session,
        60,
        now_unix_s=time.time(),
    )

    assert proof.proof_class == "fresh-pi"
    assert proof.component_acceptance_sha256 is None
    assert proof.manifest_sha256 == bounds["manifest_sha256"]
    assert proof.image_sha256 == hashlib.sha256(b"sealed-pi-image").hexdigest()
    assert proof.network_capture_sha256 == hashlib.sha256(
        (tmp_path / "pi4-cyw43-network.pcap").read_bytes()
    ).hexdigest()
    assert proof.network_evidence_sha256 == rest_perf.pi_network_evidence_sha256(
        (tmp_path / "pi4-cyw43-serial.log").read_bytes(),
        (tmp_path / "pi4-cyw43-network.pcap").read_bytes(),
        (tmp_path / "pi4-cyw43-coexistence.json").read_bytes(),
    )
    assert "source_sha256" not in acceptance["target_session"]
    execution_binding = rest_perf.pi_performance_execution_binding(
        session,
        session_raw,
        proof,
    )
    assert execution_binding["record_kind"] == "performance-execution-binding"
    assert execution_binding["performance_qualification_sha256"] == (
        proof.evidence_sha256
    )
    assert len(execution_binding["workers"]) == 3

    class PiClient:
        def status(self) -> dict:
            return {"connected": True, "backend_class": "console-projection"}

    assert rest_perf.gateway_population_axes(
        PiClient(),
        rest_perf.POPULATION_EXECUTABLE,
        bounds,
        rest_perf.BENCHMARK_TARGET_PI4,
        proof,
    ) == ("console-projection", "fresh-pi")
    assert len(observed) == 1
    assert observed[0][0] == (tmp_path / "pi4-cyw43-serial.log").read_bytes()
    assert observed[0][1] == rest_perf.BENCHMARK_TRANSPORT_WIFI


def test_genet_target_uses_separate_current_boot_and_retained_wifi_closure(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    _wifi_runtime, session_path, bounds, acceptance = (
        write_pi_benchmark_proof_chain(tmp_path)
    )
    runtime, capture, component, acceptance = write_pi_genet_current_proof(
        tmp_path,
        acceptance,
    )
    stub_pi_canonical_validators(monkeypatch)
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )
    session = rest_perf.load_target_session_binding(
        str(session_path),
        "pi4",
        bounds,
        acceptance,
    )

    proof = rest_perf.load_pi_benchmark_target_evidence(
        str(runtime),
        str(capture),
        str(tmp_path / "pi4-cyw43-coexistence.json"),
        str(session_path),
        rest_perf.BENCHMARK_TRANSPORT_GENET,
        bounds,
        session,
        60,
        now_unix_s=time.time(),
    )

    assert proof.transport == rest_perf.BENCHMARK_TRANSPORT_GENET
    assert proof.runtime_evidence_sha256 == hashlib.sha256(
        runtime.read_bytes()
    ).hexdigest()
    assert proof.cyw43_coexistence_sha256 == session[
        "cyw43_coexistence_record_sha256"
    ]


def test_retained_wifi_capture_uses_hashes_not_original_paths_or_copy_mtimes(
    tmp_path: pathlib.Path,
) -> None:
    runtime_path, _session_path, _bounds, _acceptance = (
        write_pi_benchmark_proof_chain(tmp_path)
    )
    runtime = rest_perf.parse_exact_env(
        runtime_path.read_bytes(),
        "retained runtime",
    )
    runtime["PI4_RUNTIME_DMA_SERIAL_LOG"] = "/removed/live/pi4-serial.log"
    runtime["PI4_RUNTIME_DMA_NETWORK_CAPTURE"] = "/removed/live/pi4.pcap"
    old_timestamp = time.time() - 10_000
    os.utime(tmp_path / "pi4-cyw43-serial.log", (old_timestamp, old_timestamp))
    os.utime(tmp_path / "pi4-cyw43-network.pcap", (old_timestamp, old_timestamp))

    capture_id, interface, started_ns, finished_ns = (
        rest_perf.validate_retained_pi_capture(
            runtime,
            (tmp_path / "pi4-cyw43-serial.log").read_bytes(),
            (tmp_path / "pi4-cyw43-network.pcap").read_bytes(),
            60,
            time.time(),
        )
    )

    assert capture_id == "a" * 32
    assert interface == "en0"
    assert started_ns < finished_ns


def test_pi_capture_requires_serial_selected_lane_dhcp_and_console_flow(
    tmp_path: pathlib.Path,
) -> None:
    """A fresh but unrelated pcap cannot qualify the serial-selected Pi lane."""

    _runtime, _session, _bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    serial_raw = (tmp_path / "pi4-cyw43-serial.log").read_bytes()
    capture_raw = (tmp_path / "pi4-cyw43-network.pcap").read_bytes()
    identity = rest_perf.validate_pi_correlated_network_capture(
        capture_raw,
        serial_raw,
        rest_perf.BENCHMARK_TRANSPORT_WIFI,
    )
    assert identity == {
        "transport": "wifi",
        "station_mac": "02:43:4f:48:58:32",
        "ipv4": "192.168.50.23",
        "dhcp_client_frames": 1,
        "console_payload_frames": 1,
    }

    unrelated = correlated_pcap_fixture(
        time.time_ns(),
        bytes.fromhex("02434f485899"),
        bytes((192, 168, 50, 99)),
    )
    with pytest.raises(rest_perf.RestError, match="selected-lane DHCP"):
        rest_perf.validate_pi_correlated_network_capture(
            unrelated,
            serial_raw,
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
        )
    _runtime, _capture, _component, _acceptance = write_pi_genet_current_proof(
        tmp_path,
        acceptance,
    )
    genet_serial_raw = (tmp_path / "pi4-genet-serial.log").read_bytes()
    with pytest.raises(rest_perf.RestError, match="selected-lane DHCP"):
        rest_perf.validate_pi_correlated_network_capture(
            capture_raw,
            genet_serial_raw,
            rest_perf.BENCHMARK_TRANSPORT_GENET,
        )


def test_cyw43_v2_rejects_semantic_outcome_drift(
    tmp_path: pathlib.Path,
) -> None:
    runtime_path, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    session = rest_perf.load_target_session_binding(
        str(session_path),
        "pi4",
        bounds,
        acceptance,
    )
    record_path = tmp_path / "pi4-cyw43-coexistence.json"
    record = json.loads(record_path.read_text(encoding="utf-8"))
    record["outcomes"]["nettest"] = False
    runtime_raw = runtime_path.read_bytes()
    serial_raw = (tmp_path / "pi4-cyw43-serial.log").read_bytes()
    capture_raw = (tmp_path / "pi4-cyw43-network.pcap").read_bytes()
    capture_format, link_type, first_ns, last_ns = (
        rest_perf.validate_pi_network_capture(capture_raw)
    )
    capture_id, interface, started_ns, finished_ns = (
        rest_perf.validate_retained_pi_capture(
            rest_perf.parse_exact_env(runtime_raw, "retained runtime"),
            serial_raw,
            capture_raw,
            60,
            time.time(),
        )
    )
    metadata = json.loads(
        (tmp_path / "pi4-image-identity.json").read_text(encoding="utf-8")
    )

    with pytest.raises(rest_perf.RestError, match="not live exact-image boot proof"):
        rest_perf.validate_pi_cyw43_coexistence_record(
            (json.dumps(record, sort_keys=True) + "\n").encode("utf-8"),
            session,
            acceptance["topology_sha256"],
            metadata,
            hashlib.sha256(
                (tmp_path / "cohesix-image-arm-bcm2711").read_bytes()
            ).hexdigest(),
            runtime_raw,
            serial_raw,
            0,
            PI_NORMALIZED_GATE_RAW,
            capture_raw,
            capture_format,
            link_type,
            first_ns,
            last_ns,
            capture_id,
            interface,
            started_ns,
            finished_ns,
        )


def test_cyw43_normalized_gate_rejects_deferred_driver_bootstrap() -> None:
    """Qualified WiFi cannot retain deferred linked-driver bootstrap work."""

    deferred = PI_NORMALIZED_GATE_RAW.replace(
        b"DRIVER_TASK_BOOTSTRAP_DEFERRED=0",
        b"DRIVER_TASK_BOOTSTRAP_DEFERRED=1",
    )

    with pytest.raises(rest_perf.RestError, match="exact positive outcomes"):
        rest_perf.pi_cyw43_outcomes_from_normalized_gate(deferred)


def test_separate_pi_component_acceptance_rejects_wrong_same_boot_raw_row(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    component_path = tmp_path / "pi4-worker-component.json"
    component = json.loads(component_path.read_text(encoding="utf-8"))
    component["raw_evidence"][0]["sha256"] = "0" * 64
    component_path.write_text(
        json.dumps(component, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    acceptance["evidence_sha256"] = hashlib.sha256(
        component_path.read_bytes()
    ).hexdigest()
    with pytest.raises(rest_perf.RestError, match="exact same-boot"):
        rest_perf.validate_pi_worker_component(
            component_path.read_bytes(),
            acceptance["evidence_sha256"],
            component["target_session"],
            acceptance["topology_sha256"],
            (tmp_path / "pi4-cyw43-serial.log").read_bytes(),
            (tmp_path / "pi4-cyw43-network.pcap").read_bytes(),
            runtime.read_bytes(),
        )


def test_target_session_binding_rejects_bytes_not_accepted_by_gateway(
    tmp_path: pathlib.Path,
) -> None:
    _runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    session = json.loads(session_path.read_text(encoding="utf-8"))
    session["source_sha256"] = "0" * 64
    session_path.write_text(
        json.dumps(session, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    with pytest.raises(rest_perf.RestError, match="accepted component evidence"):
        rest_perf.load_target_session_binding(
            str(session_path),
            "pi4",
            bounds,
            acceptance,
        )


def test_target_session_revalidation_rejects_changed_exact_bytes_and_binding(
    tmp_path: pathlib.Path,
) -> None:
    _runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    session, raw = rest_perf.load_target_session_binding_snapshot(
        str(session_path), "pi4", bounds, acceptance
    )
    changed_session = dict(session)
    changed_session["source_sha256"] = "0" * 64
    session_path.write_text(
        json.dumps(changed_session, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    refreshed_acceptance = copy.deepcopy(acceptance)
    refreshed_acceptance["target_session"]["target_session_sha256"] = (
        hashlib.sha256(session_path.read_bytes()).hexdigest()
    )

    with pytest.raises(rest_perf.RestError, match="changed during benchmark"):
        rest_perf.revalidate_target_session_binding(
            str(session_path),
            "pi4",
            bounds,
            refreshed_acceptance,
            session,
            raw,
        )


def test_frozen_artifact_rejects_same_size_in_place_mutation(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    artifact = tmp_path / "evidence.bin"
    artifact.write_bytes(b"before")
    initial = artifact.stat()
    real_fstat = os.fstat
    changed = False

    def changing_fstat(descriptor: int):
        nonlocal changed
        metadata = real_fstat(descriptor)
        if metadata.st_ino == initial.st_ino and not changed:
            changed = True
            with artifact.open("r+b") as handle:
                handle.write(b"after!")
                handle.flush()
                os.fsync(handle.fileno())
            os.utime(
                artifact,
                ns=(initial.st_atime_ns, initial.st_mtime_ns + 1_000_000_000),
            )
        return metadata

    monkeypatch.setattr(rest_perf.os, "fstat", changing_fstat)
    with pytest.raises(rest_perf.RestError, match="changed during bounded read"):
        rest_perf.read_frozen_artifact(str(artifact), "evidence", 1024)


def test_frozen_artifact_rejects_symlinked_ancestor(
    tmp_path: pathlib.Path,
) -> None:
    real_directory = tmp_path / "real"
    real_directory.mkdir()
    (real_directory / "evidence.bin").write_bytes(b"evidence")
    linked_directory = tmp_path / "linked"
    linked_directory.symlink_to(real_directory, target_is_directory=True)

    with pytest.raises(rest_perf.RestError, match="cannot open evidence safely"):
        rest_perf.read_frozen_artifact(
            str(linked_directory / "evidence.bin"),
            "evidence",
            1024,
        )


def test_pi_runtime_uimage_requires_exact_payload_and_crcs() -> None:
    payload = b"exact-runtime-cpio"
    wrapped = uimage_fixture(payload)
    rest_perf.validate_pi_runtime_uimage(wrapped, payload)

    damaged_header = bytearray(wrapped)
    damaged_header[8] ^= 1
    with pytest.raises(rest_perf.RestError, match="header CRC"):
        rest_perf.validate_pi_runtime_uimage(bytes(damaged_header), payload)

    damaged_payload = bytearray(wrapped)
    damaged_payload[-1] ^= 1
    with pytest.raises(rest_perf.RestError, match="exact raw CPIO"):
        rest_perf.validate_pi_runtime_uimage(bytes(damaged_payload), payload)


def test_pi_root_cpio_requires_only_exact_kernel_and_root_members() -> None:
    kernel = b"kernel"
    root = b"root"
    exact = newc_archive_fixture({"kernel.elf": kernel, "rootserver": root})
    rest_perf.validate_pi_root_cpio(exact, kernel, root)

    with pytest.raises(rest_perf.RestError, match="exact kernel/root members"):
        rest_perf.validate_pi_root_cpio(
            newc_archive_fixture(
                {
                    "extra": b"unexpected",
                    "kernel.elf": kernel,
                    "rootserver": root,
                }
            ),
            kernel,
            root,
        )
    with pytest.raises(rest_perf.RestError, match="exact kernel/root members"):
        rest_perf.validate_pi_root_cpio(exact, b"other-kernel", root)


def test_pi_source_inventory_binds_one_canonical_manifest_row() -> None:
    manifest_sha256 = "1" * 64
    inventory = {
        "schema": "cohesix-source-inventory/v1",
        "algorithm": "git-visible-paths-sha256",
        "entries": [
            {
                "path": "configs/root_task_pi4_uboot_aarch64.toml",
                "kind": "file",
                "mode": 0o644,
                "sha256": manifest_sha256,
                "bytes": 10,
            }
        ],
    }
    raw = json.dumps(inventory, separators=(",", ":")).encode("utf-8")
    assert rest_perf.pi_source_manifest_sha256(raw) == manifest_sha256

    inventory["entries"].append(copy.deepcopy(inventory["entries"][0]))
    duplicate = json.dumps(inventory, separators=(",", ":")).encode("utf-8")
    with pytest.raises(rest_perf.RestError, match="invalid entry"):
        rest_perf.pi_source_manifest_sha256(duplicate)


def test_pi_archive_validator_rejects_validation_time_mutation(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    paths = [tmp_path / name for name in ("driver.cpio", "driver.json", "worker.cpio", "worker.json")]
    for index, path in enumerate(paths):
        path.write_bytes(f"artifact-{index}".encode("ascii"))
    calls = []

    def validate(command, **_kwargs):
        calls.append(command)
        if len(calls) == 2:
            paths[0].write_bytes(b"mutated-driver")
        return subprocess.CompletedProcess(command, 0, b"", b"")

    monkeypatch.setattr(rest_perf.subprocess, "run", validate)
    with pytest.raises(rest_perf.RestError, match="changed during canonical validation"):
        rest_perf.validate_pi_archive_manifests(
            str(paths[0]),
            b"artifact-0",
            str(paths[1]),
            b"artifact-1",
            str(paths[2]),
            b"artifact-2",
            str(paths[3]),
            b"artifact-3",
        )
    assert len(calls) == 2


def write_pi_image_validator_fixture(
    tmp_path: pathlib.Path,
) -> tuple[
    tuple[pathlib.Path, pathlib.Path, pathlib.Path, pathlib.Path],
    bytes,
]:
    """Write exact files and v2 stat fields for a mocked canonical verifier."""

    paths = (
        tmp_path / "image",
        tmp_path / "metadata.json",
        tmp_path / "rootserver",
        tmp_path / "root.cpio",
    )
    for index, path in enumerate(paths):
        if index != 1:
            path.write_bytes(f"artifact-{index}".encode("ascii"))
    image_metadata = paths[0].stat()
    metadata_raw = (
        json.dumps(
            {
                "schema": "cohesix-pi4-image-identity/v2",
                "device": image_metadata.st_dev,
                "inode": image_metadata.st_ino,
                "size_bytes": image_metadata.st_size,
                "mtime_ns": image_metadata.st_mtime_ns,
                "ctime_ns": image_metadata.st_ctime_ns,
            },
            sort_keys=True,
        )
        + "\n"
    ).encode("utf-8")
    paths[1].write_bytes(metadata_raw)
    return paths, metadata_raw


def test_pi_image_validator_rejects_validation_time_mutation(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    paths, metadata_raw = write_pi_image_validator_fixture(tmp_path)

    def validate(command, **_kwargs):
        paths[0].write_bytes(b"mutated-image")
        return subprocess.CompletedProcess(command, 0, b"", b"")

    monkeypatch.setattr(rest_perf.subprocess, "run", validate)
    with pytest.raises(rest_perf.RestError, match="changed during canonical validation"):
        rest_perf.validate_pi_image_identity(
            str(paths[0]),
            b"artifact-0",
            str(paths[1]),
            metadata_raw,
            str(paths[2]),
            b"artifact-2",
            str(paths[3]),
            b"artifact-3",
            "1" * 40,
            "2" * 64,
        )


def test_pi_image_validator_uses_original_exact_paths(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    """The v2 verifier must inspect the inode-bound staged files, not copies."""

    paths, metadata_raw = write_pi_image_validator_fixture(tmp_path)
    observed: list[tuple[str, ...]] = []

    def validate(command, **_kwargs):
        observed.append(command)
        return subprocess.CompletedProcess(command, 0, b"", b"")

    monkeypatch.setattr(rest_perf.subprocess, "run", validate)
    rest_perf.validate_pi_image_identity(
        str(paths[0]),
        b"artifact-0",
        str(paths[1]),
        metadata_raw,
        str(paths[2]),
        b"artifact-2",
        str(paths[3]),
        b"artifact-3",
        "1" * 40,
        "2" * 64,
    )

    assert len(observed) == 1
    command = observed[0]
    assert command[command.index("--image") + 1] == str(paths[0])
    assert command[command.index("--metadata") + 1] == str(paths[1])
    assert command[command.index("--expected-root-elf") + 1] == str(paths[2])
    assert command[command.index("--expected-root-cpio") + 1] == str(paths[3])


def test_pi_image_validator_rejects_prevalidation_input_drift(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    """Previously captured bytes must still match before v2 verification."""

    paths, metadata_raw = write_pi_image_validator_fixture(tmp_path)
    paths[0].write_bytes(b"mutated-before-validation")

    def unexpected_validate(*_args, **_kwargs):
        pytest.fail("drifted input must be rejected before the verifier runs")

    monkeypatch.setattr(rest_perf.subprocess, "run", unexpected_validate)
    with pytest.raises(rest_perf.RestError, match="changed during evidence validation"):
        rest_perf.validate_pi_image_identity(
            str(paths[0]),
            b"artifact-0",
            str(paths[1]),
            metadata_raw,
            str(paths[2]),
            b"artifact-2",
            str(paths[3]),
            b"artifact-3",
            "1" * 40,
            "2" * 64,
        )


def test_pi_image_validator_rejects_same_bytes_on_a_new_inode(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    """An atomic same-byte replacement cannot retain staged image identity."""

    paths, metadata_raw = write_pi_image_validator_fixture(tmp_path)

    def replace_with_same_bytes(command, **_kwargs):
        replacement = tmp_path / "replacement-image"
        replacement.write_bytes(paths[0].read_bytes())
        os.replace(replacement, paths[0])
        return subprocess.CompletedProcess(command, 0, b"", b"")

    monkeypatch.setattr(rest_perf.subprocess, "run", replace_with_same_bytes)
    with pytest.raises(rest_perf.RestError, match="stat identity changed"):
        rest_perf.validate_pi_image_identity(
            str(paths[0]),
            b"artifact-0",
            str(paths[1]),
            metadata_raw,
            str(paths[2]),
            b"artifact-2",
            str(paths[3]),
            b"artifact-3",
            "1" * 40,
            "2" * 64,
        )


def test_pi_target_evidence_revalidation_rejects_changed_serial_bytes(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )
    initial = rest_perf.load_pi_benchmark_target_evidence(
        str(runtime),
        str(tmp_path / "pi4-cyw43-network.pcap"),
        str(tmp_path / "pi4-cyw43-coexistence.json"),
        str(session_path),
        rest_perf.BENCHMARK_TRANSPORT_WIFI,
        bounds,
        session,
        60,
        now_unix_s=time.time(),
    )
    serial_path = tmp_path / "pi4-cyw43-serial.log"
    with serial_path.open("a", encoding="utf-8") as handle:
        handle.write("benchmark progress on the same boot\n")
    with pytest.raises(rest_perf.RestError, match="capture binding differs from bytes"):
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
            previous_evidence=initial,
        )


def test_pi_target_evidence_rejects_changed_packet_capture(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )
    capture_path = tmp_path / "pi4-cyw43-network.pcap"
    initial = rest_perf.load_pi_benchmark_target_evidence(
        str(runtime),
        str(capture_path),
        str(tmp_path / "pi4-cyw43-coexistence.json"),
        str(session_path),
        rest_perf.BENCHMARK_TRANSPORT_WIFI,
        bounds,
        session,
        60,
        now_unix_s=time.time(),
    )
    changed = bytearray(capture_path.read_bytes())
    changed[-1] ^= 1
    capture_path.write_bytes(changed)

    with pytest.raises(rest_perf.RestError, match="capture binding differs from bytes"):
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(capture_path),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
            previous_evidence=initial,
        )


def test_pi_target_evidence_binds_driver_archive_to_full_target_session(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    session["driver_archive_sha256"] = "0" * 64
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )

    with pytest.raises(rest_perf.RestError, match="target-session bytes"):
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
        )

    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    (tmp_path / "cohesix-driver-runtimes.cpio").write_bytes(b"tampered-driver")
    with pytest.raises(rest_perf.RestError, match="runtime CPIO hash"):
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
        )


@pytest.mark.parametrize(
    "field",
    sorted(rest_perf.TARGET_SESSION_KEYS - {"target"}),
)
def test_pi_target_evidence_binds_every_full_session_artifact_hash(
    tmp_path: pathlib.Path,
    monkeypatch,
    field: str,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    session[field] = "0" * 64
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )

    with pytest.raises(rest_perf.RestError, match="target-session bytes"):
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
        )


@pytest.mark.parametrize(
    "field",
    (
        "source_manifest_sha256",
        "canonical_profile_stamp_sha256",
        "canonical_profile_state_sha256",
        "composition_record_sha256",
        "composition_cmake_cache_sha256",
        "composition_timer_header_sha256",
        "wrapper_sha256",
    ),
)
def test_pi_target_evidence_rejects_v5_provenance_graph_drift(
    tmp_path: pathlib.Path,
    monkeypatch,
    field: str,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    provenance_path = tmp_path / "cohesix-image-arm-bcm2711.provenance.json"
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    provenance[field] = "0" * 64
    provenance_path.write_text(
        json.dumps(provenance, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    stage_path = tmp_path / "stage.env"
    provenance_raw = provenance_path.read_bytes()
    rewrite_env_fixture(
        stage_path,
        {
            "PI4_IMAGE_IDENTITY_WRAPPER_PROVENANCE_SHA256": hashlib.sha256(
                provenance_raw
            ).hexdigest(),
            "PI4_IMAGE_IDENTITY_WRAPPER_PROVENANCE_BYTES": str(
                len(provenance_raw)
            ),
        },
    )
    rewrite_env_fixture(
        runtime,
        {
            "PI4_RUNTIME_DMA_STAGE_BUILD_PROOF_SHA256": hashlib.sha256(
                stage_path.read_bytes()
            ).hexdigest()
        },
    )
    stub_pi_canonical_validators(monkeypatch)
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )

    with pytest.raises(rest_perf.RestError, match="metadata differs"):
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
        )

def test_pi_target_evidence_rejects_image_tampering_and_stale_runtime(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )
    (tmp_path / "cohesix-image-arm-bcm2711").write_bytes(b"tampered")
    try:
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
        )
    except rest_perf.RestError as exc:
        assert "staged image hash" in str(exc)
    else:
        raise AssertionError("tampered Pi image must fail closed")

    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    stale = time.time() - 120
    os.utime(runtime, (stale, stale))
    try:
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
        )
    except rest_perf.RestError as exc:
        assert "stale" in str(exc)
    else:
        raise AssertionError("stale Pi runtime proof must fail closed")


def test_pi_target_evidence_rejects_metadata_and_latest_boot_marker_tampering(
    tmp_path: pathlib.Path,
    monkeypatch,
) -> None:
    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    stub_pi_canonical_validators(monkeypatch)
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    monkeypatch.setattr(
        rest_perf,
        "validate_pi_network_log",
        lambda *_args: PI_NORMALIZED_GATE_RAW,
    )
    metadata_path = tmp_path / "pi4-image-identity.json"
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    metadata["image_sha256"] = "f" * 64
    metadata_path.write_text(json.dumps(metadata) + "\n", encoding="utf-8")
    try:
        rest_perf.load_pi_benchmark_target_evidence(
            str(runtime),
            str(tmp_path / "pi4-cyw43-network.pcap"),
            str(tmp_path / "pi4-cyw43-coexistence.json"),
            str(session_path),
            rest_perf.BENCHMARK_TRANSPORT_WIFI,
            bounds,
            session,
            60,
            now_unix_s=time.time(),
        )
    except rest_perf.RestError as exc:
        assert "metadata hash" in str(exc)
    else:
        raise AssertionError("tampered image metadata must fail closed")

    runtime, session_path, bounds, acceptance = write_pi_benchmark_proof_chain(
        tmp_path
    )
    session = rest_perf.load_target_session_binding(
        str(session_path), "pi4", bounds, acceptance
    )
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    correct_marker = metadata["build_marker"]
    wrong_marker = correct_marker.replace(
        f"image-id={metadata['image_id']}", f"image-id={'4' * 64}"
    )
    tampered_serial = (
        "U-Boot 2026.01\n"
        "[cohesix:root-task] Cohesix boot: root-task online\n"
        f"{correct_marker}\n"
        "U-Boot 2026.01\n"
        "[cohesix:root-task] Cohesix boot: root-task online\n"
        f"{wrong_marker}\n"
    ).encode("utf-8")
    with pytest.raises(rest_perf.RestError, match="latest Pi serial boot"):
        rest_perf.validate_serial_image_identity(
            tampered_serial,
            metadata,
        )


def test_acceptance_allows_passive_zero_sc_but_rejects_mixed_zero() -> None:
    class Accepted:
        def __init__(self, period_us: int) -> None:
            self.period_us = period_us

        def status(self) -> dict:
            acceptance = acceptance_summary()
            for worker in acceptance["workers"]:
                worker["scheduling_context"] = {
                    "budget_us": 0,
                    "period_us": self.period_us,
                }
                worker["object_inventory"]["scheduling_contexts"] = 0
            return {
                "connected": True,
                "backend_class": "console-projection",
                "worker_acceptance": acceptance,
            }

    assert rest_perf.executable_qemu_acceptance_binding(
        Accepted(0), executable_bounds()
    )["workers"]
    try:
        rest_perf.executable_qemu_acceptance_binding(
            Accepted(1000), executable_bounds()
        )
    except rest_perf.RestError as exc:
        assert "SC is invalid" in str(exc)
    else:
        raise AssertionError("mixed-zero scheduling context must fail closed")


def test_executable_state_serializes_three_exemplars_after_full_census(
    monkeypatch,
) -> None:
    bounds = executable_bounds(maximum=5)
    bounds["worker_runtime"]["roles"][1]["executable_slots"] = 2
    bounds["worker_runtime"]["roles"][2]["executable_slots"] = 2
    acceptance = acceptance_summary()
    instances = []
    sequence = 1
    for role, count in (
        ("worker-heartbeat", 1),
        ("worker-gpu", 2),
        ("worker-lora", 2),
    ):
        for slot in range(count):
            accepted = next(
                row for row in acceptance["workers"] if row["role"] == role
            )
            instances.append(
                rest_perf.WorkerInstance(
                    worker_id=f"{role}-{slot}",
                    role=role,
                    lifecycle="ready",
                    telemetry_path=f"/shard/00/worker/{role}-{slot}/telemetry",
                    slot=slot,
                    lease_epoch=accepted["lease_epoch"] if slot == 0 else 100 + slot,
                    supervisor_generation=(
                        accepted["supervisor_generation"]
                        if slot == 0
                        else 200 + slot
                    ),
                    cap_generation=(
                        accepted["cap_generation"] if slot == 0 else 300 + slot
                    ),
                    ready_sequence=(
                        accepted["ready_sequence"] if slot == 0 else sequence
                    ),
                    control_sequence=0,
                    receipt_sequence=0,
                    completion_sequence=0,
                )
            )
            sequence += 1
    monkeypatch.setattr(
        rest_perf,
        "discover_executable_workers",
        lambda *_args: (instances, len(instances)),
    )
    monkeypatch.setattr(
        rest_perf,
        "capture_proc_pressure_state",
        lambda *_args: {path: {} for path in rest_perf.EXECUTABLE_PROC_PATHS},
    )
    state = rest_perf.SimState(
        bounds=bounds,
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
        maximum_live_tasks=5,
        acceptance_binding=acceptance,
    )

    snapshot = rest_perf.capture_executable_state(
        object(), state, require_accepted_identity=True
    )

    assert len(snapshot["workers"]) == 3
    assert [row["role"] for row in snapshot["workers"]] == list(
        rest_perf.EXECUTABLE_WORKER_ROLES
    )
    assert all("worker" in row and "telemetry_path" not in row for row in snapshot["workers"])
    assert all(row["artifact"] == "verified" for row in snapshot["workers"])
    assert all(row["execution_proof"] == "qemu" for row in snapshot["workers"])
    assert snapshot["ready_census"]["ready"] == 5
    assert snapshot["ready_census"]["maximum_live_tasks"] == 5

    state.current_workers_by_id = {
        instance.worker_id: instance for instance in instances
    }
    monkeypatch.setattr(
        rest_perf,
        "discover_executable_workers",
        lambda *_args: (instances[:-1], len(instances) - 1),
    )
    with pytest.raises(
        rest_perf.RestError,
        match="exact generated READY Worker population",
    ):
        rest_perf.capture_executable_state(
            object(), state, require_accepted_identity=True
        )
