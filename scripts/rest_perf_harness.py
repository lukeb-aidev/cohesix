#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Benchmark and load-test the REST gateway for Cohesix.
# Copyright 2026 Lukas Bower

"""REST performance and load harness for Cohesix hive-gateway.

Modes:
  - perf: Measure sequential vs parallel latency for status/telemetry reads.
  - simulate: Launch QEMU + hive-gateway (optional) and drive REST traffic that
    mimics a live hive with varying worker counts and intensity.
"""

from __future__ import annotations

import argparse
import base64
import concurrent.futures
import csv
import errno
import hashlib
import json
import math
import os
import pathlib
import queue
import random
import signal
import socket
import stat
import subprocess
import sys
import threading
import time
import traceback
from datetime import datetime, timezone
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field, replace
from typing import Callable, Dict, Iterable, List, Optional, Sequence, Tuple, TextIO

DEFAULT_REST_URL = "http://127.0.0.1:8080"
DEFAULT_RUNS = 3
DEFAULT_TIMEOUT_SECS = 3.0
DEFAULT_SIMULATE_TIMEOUT_SECS = 10.0
DEFAULT_MAX_WORKERS = 4
MAX_WORKER_STATE_TAIL_BYTES = 4096
DEFAULT_TAIL_BYTES = MAX_WORKER_STATE_TAIL_BYTES
DEFAULT_LOG_TAIL_BYTES = 32768
WORKER_FAILURE_CONTEXT_MAX_LINES = 12
WORKER_FAILURE_CONTEXT_LINE_BYTES = 240
WORKER_FAILURE_CONTEXT_MAX_BYTES = 2048
# The target retains at most 64 complete 256-byte ticket wire records.
HOST_TICKET_LOG_TAIL_BYTES = 64 * 256
HOST_TICKET_CURRENT_MAX_BYTES = 256
HOST_TICKET_CURRENT_PREFIX = "/host/tickets/current/"
DEFAULT_QEMU_SMP = "4,cores=4,threads=1,sockets=1"
DEFAULT_WORKERS_MIN = 8
DEFAULT_WORKERS_MAX = 50
DEFAULT_INTENSITY_MIN = 1
DEFAULT_INTENSITY_MAX = 10
DEFAULT_DURATION_MINS = 5
DEFAULT_RAMP_STEP_SECS = 30
DEFAULT_BASE_RPS = 1.0
DEFAULT_MAX_INFLIGHT = 64
DEFAULT_ENTROPY = 5.0
DEFAULT_GATEWAY_BIND = "127.0.0.1:8080"
DEFAULT_TCP_HOST = "127.0.0.1"
DEFAULT_TCP_PORT = 31337
DEFAULT_AUTH_TOKEN = ""
DEFAULT_REQUEST_AUTH_TOKEN = ""
DEFAULT_ROLE = "queen"
DEFAULT_SUMMARY_MAX_ERROR_LINES = 400
DEFAULT_READY_TIMEOUT_SECS = 180
DEFAULT_TELEMETRY_REFERENCE_CHUNK_BYTES = 16 * 1024 * 1024
MAX_TELEMETRY_SEGMENT_ID_BYTES = 32
BENCHMARK_MARKER_SETTLE_SECS = 1.0
GATEWAY_STATUS_BROKER_COUNTERS = (
    "control_waiters",
    "telemetry_waiters",
    "control_waiters_high_water",
    "telemetry_waiters_high_water",
    "control_checkouts",
    "telemetry_checkouts",
    "pool_exhausted",
    "checkout_retries",
    "timeout_rejections",
    "telemetry_yields",
    "proc_cache_hits",
    "proc_cache_misses",
    "proc_cache_evictions",
    "control_write_retryable_errors",
    "control_write_retries",
    "control_write_retry_sleep_ms",
    "control_write_retry_exhaustions",
    "control_write_success_after_retry",
)
RETAINED_STATE_OPERATION_NAMES = (
    "schedule_write",
    "lease_grant",
    "lease_preempt",
    "lease_quota",
)
RAMP_BOUNDARY_FIELDS = (
    "step",
    "workers",
    "intensity",
    "rps",
    "ops",
    "ok",
    "err",
    "err_rate",
    "throughput_ops_s",
    "ok_ops_s",
    "max_inflight_observed",
    "max_inflight_configured",
)

FAST_RAMP_WORKERS_MIN = 24
FAST_RAMP_WORKERS_MAX = 120
FAST_RAMP_INTENSITY_MIN = 2
FAST_RAMP_INTENSITY_MAX = 10
FAST_RAMP_DURATION_MINS = 2
FAST_RAMP_RAMP_STEP_SECS = 8
FAST_RAMP_BASE_RPS = 0.6
FAST_RAMP_MAX_INFLIGHT = 192

TELEMETRY_SCENARIO_BYTES = {
    "telemetry-1mb": 1 * 1024 * 1024,
    "telemetry-10mb": 10 * 1024 * 1024,
    "telemetry-100mb": 100 * 1024 * 1024,
    "telemetry-1gb": 1024 * 1024 * 1024,
}
TELEMETRY_REFERENCE_SCHEMA = "coh-ref-c/v1"
POPULATION_HOST_MODEL = "host-model"
POPULATION_EXECUTABLE = "executable"
POPULATION_EXECUTABLE_LOG = "executable-log"
EXECUTABLE_WORKER_ROLES = (
    "worker-heartbeat",
    "worker-gpu",
    "worker-lora",
)


def is_live_executable_population(population_mode: str) -> bool:
    """Return whether traffic targets real compiler-admitted QEMU Workers."""
    return population_mode in (POPULATION_EXECUTABLE, POPULATION_EXECUTABLE_LOG)
WORKER_LIFECYCLE_STATES = frozenset(
    ("absent", "queued", "starting", "ready", "closing", "faulted", "terminal")
)
WORKER_RUNTIME_STATE_SCHEMA_V1 = "worker-runtime-state/v1"
WORKER_RUNTIME_STATE_SCHEMA_V2 = "worker-runtime-state/v2"
WORKER_RUNTIME_STATE_SCHEMAS = frozenset(
    (WORKER_RUNTIME_STATE_SCHEMA_V1, WORKER_RUNTIME_STATE_SCHEMA_V2)
)
WORKER_RUNTIME_STATE_V2_MAX_COUNTER = (1 << 32) - 1
MAX_DISCOVERED_SHARDS = 256
MAX_DISCOVERED_WORKERS = 256
EXECUTABLE_UART_MARKERS = (
    "WORKER_TASK_ADMISSION",
    "WORKER_TASK_READY",
    "WORKER_TASK_RECEIPT",
    "WORKER_TASK_COMPLETION",
    "WORKER_TASK_FAULT",
    "WORKER_TASK_TEARDOWN",
    "GPU_BRIDGE_FIXTURE_ADMISSION",
)
EXECUTABLE_GDB_MARKERS = (
    "M26E_QEMU_SESSION target=qemu machine=virt gic_version=3",
    "M26E_GDB_ELF role=worker-heartbeat",
    "M26E_GDB_ELF role=worker-gpu",
    "M26E_GDB_ELF role=worker-lora",
    "M26E_GDB_INJECTION",
    "phase=pre-ready",
    "phase=during-ipc",
    "phase=budget-exhaustion",
)
SHA256_HEX_LENGTH = 64
EXECUTABLE_PROC_PATHS = (
    "/proc/schedule/summary",
    "/proc/schedule/queue",
    "/proc/lease/summary",
    "/proc/lease/active",
    "/proc/lease/preemptions",
)


@dataclass(frozen=True)
class TelemetryScenario:
    """Scenario preset for large telemetry reference-manifest runs."""

    name: str
    artifact_bytes: int
    chunk_bytes: int
    reference_entries: int
    requests_per_operation: int


@dataclass(frozen=True)
class RequestSpec:
    """Describe a REST request to execute."""

    path: str
    max_bytes: int
    verb: str


@dataclass(frozen=True)
class GatewayResponse:
    """Parsed hive-gateway response."""

    status: str
    verb: str
    path: str
    end: bool
    lines: List[str]
    bytes: Optional[int]
    error: Optional[str]


@dataclass(frozen=True)
class WorkerInstance:
    """One structured Worker instance discovered through canonical `/shard`."""

    worker_id: str
    role: str
    lifecycle: str
    telemetry_path: str
    slot: int
    lease_epoch: int
    supervisor_generation: int
    cap_generation: int
    ready_sequence: int
    control_sequence: int
    receipt_sequence: int
    completion_sequence: int

    def identity_dict(self) -> Dict[str, object]:
        """Return the public five-part identity used by evidence correlation."""
        return {
            "role": self.role,
            "slot": self.slot,
            "lease_epoch": self.lease_epoch,
            "supervisor_generation": self.supervisor_generation,
            "cap_generation": self.cap_generation,
        }


@dataclass(frozen=True)
class HostTicketCurrent:
    """One bounded exact ticket/Worker projection from NineDoor."""

    state: str
    role: str
    worker_id: str
    lifecycle: str
    slot: int
    lease_epoch: int
    supervisor_generation: int
    cap_generation: int
    ready_sequence: int
    control_sequence: int
    receipt_sequence: int
    completion_sequence: int
    admission_sequence: int


@dataclass(frozen=True)
class PopulationSnapshot:
    """Bounded population observation kept separate from target proof."""

    requested: int
    discovered: int
    ready: int
    backend_class: str
    proof_class: str

    def as_dict(self) -> Dict[str, object]:
        return {
            "requested": self.requested,
            "discovered": self.discovered,
            "ready": self.ready,
            "backend_class": self.backend_class,
            "proof_class": self.proof_class,
        }


class RestError(RuntimeError):
    """REST request failed with a gateway error or HTTP exception."""

    def __init__(self, message: str, response: Optional[GatewayResponse] = None):
        super().__init__(message)
        self.response = response


class _StrictControlRefusal(RestError):
    """Typed strict-mode refusal that must bypass every harness retry loop."""


@dataclass
class RunLogger:
    """Simple run logger that writes to a timestamped file."""

    path: str
    handle: TextIO
    echo_stdout: bool = True

    def log(self, message: str) -> None:
        timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        line = f"{timestamp} {message}"
        self.handle.write(line + "\n")
        self.handle.flush()
        if self.echo_stdout:
            print(message)

    def close(self) -> None:
        self.handle.close()


def is_transient_error(error: Exception) -> bool:
    if isinstance(error, _StrictControlRefusal):
        return False
    message = str(error).lower()
    if "http 503" in message or "http 502" in message or "service unavailable" in message:
        return True
    if "http 429" in message or "too many requests" in message:
        return True
    if "reason=policy" in message and "detail=denied" in message:
        return True
    if "buffer full" in message or "buffer-full" in message:
        return True
    if "timed out" in message or "timeout" in message:
        return True
    if "connection reset" in message or "connection refused" in message:
        return True
    return False


def is_buffer_full_message(message: str) -> bool:
    """Return true when a gateway or target error is a bounded refusal."""
    message = message.lower()
    return "buffer full" in message or "buffer-full" in message


def is_buffer_full_error(error: Exception) -> bool:
    return is_buffer_full_message(str(error))


def is_buffer_full_response(response: GatewayResponse) -> bool:
    return is_buffer_full_message(response.error or "")


def is_worker_capacity_event(error: Exception) -> bool:
    message = str(error).lower()
    return (
        "buffer full" in message
        or "buffer-full" in message
        or "timed out" in message
        or "timeout" in message
    )


def retry_transient(
    fn: Callable[[], None],
    timeout_s: float,
    label: str,
    base_sleep: float = 0.5,
    max_sleep: float = 2.0,
    jitter: float = 0.25,
) -> None:
    deadline = time.monotonic() + timeout_s
    attempt = 0
    sleep_s = base_sleep
    while True:
        try:
            fn()
            return
        except Exception as exc:
            if not is_transient_error(exc):
                raise
            attempt += 1
            now = time.monotonic()
            if now >= deadline:
                raise RestError(f"{label} failed after {attempt} retries: {exc}") from exc
            remaining = deadline - now
            if remaining <= 0:
                raise RestError(f"{label} failed after {attempt} retries: {exc}") from exc
            jitter_scale = 1.0 + ((random.random() * 2.0 - 1.0) * jitter)
            wait_s = min(sleep_s * max(0.1, jitter_scale), remaining)
            time.sleep(wait_s)
            sleep_s = min(sleep_s * 1.5, max_sleep)


def run_with_retry_policy(
    fn: Callable[[], None],
    state: Optional["SimState"],
    timeout_s: float,
    label: str,
    base_sleep: float = 0.5,
    max_sleep: float = 2.0,
    jitter: float = 0.25,
) -> None:
    if state is not None and not state.transient_retries:
        fn()
        return
    retry_transient(
        fn,
        timeout_s=timeout_s,
        label=label,
        base_sleep=base_sleep,
        max_sleep=max_sleep,
        jitter=jitter,
    )


@dataclass
class Operation:
    """A single REST operation used for simulation."""

    name: str
    weight: float
    category: str
    func: Callable[["RestClient", str, "SimState"], None]


@dataclass
class OpStats:
    """Aggregate stats for a set of operations."""

    count: int = 0
    ok: int = 0
    err: int = 0
    total_s: float = 0.0
    min_s: float = 0.0
    max_s: float = 0.0
    samples: List[float] = field(default_factory=list)
    sample_limit: int = 2048
    errors: Dict[str, int] = field(default_factory=dict)

    def record(self, elapsed_s: float, ok: bool, error: Optional[str]) -> None:
        self.count += 1
        if ok:
            self.ok += 1
        else:
            self.err += 1
            if error:
                self.errors[error] = self.errors.get(error, 0) + 1
        self.total_s += elapsed_s
        if self.count == 1:
            self.min_s = elapsed_s
            self.max_s = elapsed_s
        else:
            self.min_s = min(self.min_s, elapsed_s)
            self.max_s = max(self.max_s, elapsed_s)
        if len(self.samples) < self.sample_limit:
            self.samples.append(elapsed_s)
        else:
            idx = random.randint(0, self.count - 1)
            if idx < self.sample_limit:
                self.samples[idx] = elapsed_s

    def avg(self) -> float:
        if self.count == 0:
            return 0.0
        return self.total_s / self.count

    def percentile(self, pct: float) -> float:
        if not self.samples:
            return 0.0
        ordered = sorted(self.samples)
        index = int(round((pct / 100.0) * (len(ordered) - 1)))
        return ordered[min(max(index, 0), len(ordered) - 1)]


@dataclass
class ConcurrencyStats:
    """Track REST operations that are currently in flight."""

    current: int = 0
    high_water: int = 0
    submitted: int = 0
    completed: int = 0
    lock: threading.Lock = field(default_factory=threading.Lock)

    def start(self) -> int:
        with self.lock:
            self.current += 1
            self.submitted += 1
            self.high_water = max(self.high_water, self.current)
            return self.current

    def finish(self) -> None:
        with self.lock:
            if self.current > 0:
                self.current -= 1
            self.completed += 1

    def snapshot(self, configured_max: int) -> Dict[str, object]:
        with self.lock:
            return {
                "configured_max_inflight": configured_max,
                "observed_high_water": self.high_water,
                "current_inflight": self.current,
                "submitted": self.submitted,
                "completed": self.completed,
            }


@dataclass
class SimState:
    """Shared simulation state."""

    bounds: dict
    rest_url: str
    rng: random.Random
    entropy: float
    tail_bytes: int
    policy_enabled: bool
    actions_enabled: bool
    telemetry_enabled: bool
    include_lifecycle: bool
    auto_approve: bool
    transient_retries: bool
    strict_control_errors: bool
    run_token: str = field(
        default_factory=lambda: hashlib.sha256(
            f"{os.getpid()}-{time.time_ns()}".encode("ascii")
        ).hexdigest()[:8]
    )
    worker_cap: Optional[int] = None
    next_worker_seq: int = 1
    approval_seq: int = 0
    policy_lock: threading.Lock = field(default_factory=threading.Lock)
    schedule_lock: threading.Lock = field(default_factory=threading.Lock)
    lease_lock: threading.Lock = field(default_factory=threading.Lock)
    id_lock: threading.Lock = field(default_factory=threading.Lock)
    active_leases: List[str] = field(default_factory=list)
    policy_current: Optional[str] = None
    policy_previous: Optional[str] = None
    telemetry_segments: Dict[str, str] = field(default_factory=dict)
    telemetry_lock: threading.Lock = field(default_factory=threading.Lock)
    telemetry_device_locks: Dict[str, threading.RLock] = field(default_factory=dict)
    next_schedule_seq: int = 0
    next_lease_seq: int = 0
    logger: Optional[RunLogger] = None
    telemetry_scenario: Optional[TelemetryScenario] = None
    telemetry_reference_chunk_bytes: int = DEFAULT_TELEMETRY_REFERENCE_CHUNK_BYTES
    telemetry_reference_records: Optional[List[str]] = None
    population_mode: str = POPULATION_HOST_MODEL
    maximum_live_tasks: Optional[int] = None
    worker_telemetry_paths: Dict[str, str] = field(default_factory=dict)
    population_observations: List[PopulationSnapshot] = field(default_factory=list)
    acceptance_binding: Optional[Dict[str, object]] = None
    executable_pre_state: Optional[Dict[str, object]] = None
    executable_post_state: Optional[Dict[str, object]] = None
    lifecycle_cycles: List[Dict[str, object]] = field(default_factory=list)
    receipt_operations: List[Dict[str, object]] = field(default_factory=list)
    fault_artifacts: Dict[str, Dict[str, object]] = field(default_factory=dict)
    current_workers_by_id: Dict[str, WorkerInstance] = field(default_factory=dict)
    ticket_worker_lanes: Dict[str, queue.Queue[str]] = field(default_factory=dict)
    ticket_worker_locks: Dict[str, threading.Lock] = field(default_factory=dict)
    ticket_quarantined_workers: set[str] = field(default_factory=set)
    receipt_operation_workers: Dict[str, str] = field(default_factory=dict)
    ticket_state_lock: threading.Lock = field(default_factory=threading.Lock)
    next_ticket_seq: int = 0
    receipt_gpu_subject: Optional[str] = None
    receipt_lora_subject: Optional[str] = None
    receipt_gpu_lease_ids: List[str] = field(default_factory=list)
    next_receipt_gpu_lease: int = 0

    def record_population(self, snapshot: PopulationSnapshot) -> None:
        """Retain a bounded population history for report provenance."""
        self.population_observations.append(snapshot)
        if len(self.population_observations) > 128:
            self.population_observations.pop(0)


def should_tolerate_buffer_full(response: GatewayResponse, state: SimState) -> bool:
    return (not state.strict_control_errors) and is_buffer_full_response(response)


@dataclass
class WorkerProfile:
    """Per-worker simulation profile."""

    worker_id: str
    load_factor: float
    op_weights: List[Tuple[Operation, float]]


class RestClient:
    """Minimal REST client for hive-gateway."""

    def __init__(
        self, rest_url: str, timeout: float, request_auth_token: Optional[str] = None
    ):
        self.rest_url = normalize_rest_url(rest_url)
        self.timeout = timeout
        token = (request_auth_token or "").strip()
        self.request_auth_token = token if token else None

    def get_json(self, path: str, params: Optional[Dict[str, str]] = None) -> dict:
        url = self._build_url(path, params)
        try:
            return fetch_json(url, self.timeout, self.request_auth_headers())
        except urllib.error.HTTPError as exc:
            raise RestError(f"HTTP {exc.code} {exc.reason} for {url}") from exc
        except urllib.error.URLError as exc:
            raise RestError(f"URL error for {url}: {exc}") from exc

    def post_json(self, path: str, payload: dict) -> dict:
        url = self._build_url(path, None)
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(url, data=data, method="POST")
        request.add_header("Content-Type", "application/json")
        for key, value in self.request_auth_headers().items():
            request.add_header(key, value)
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                payload = response.read()
        except urllib.error.HTTPError as exc:
            raise RestError(f"HTTP {exc.code} {exc.reason} for {url}") from exc
        except urllib.error.URLError as exc:
            raise RestError(f"URL error for {url}: {exc}") from exc
        return json.loads(payload)

    def ls(self, path: str) -> GatewayResponse:
        return parse_gateway_response(
            self.get_json("/v1/fs/ls", {"path": path})
        )

    def cat(self, path: str, max_bytes: int) -> GatewayResponse:
        return parse_gateway_response(
            self.get_json(
                "/v1/fs/cat",
                {"path": path, "max_bytes": str(max_bytes)},
            )
        )

    def tail(self, path: str, max_bytes: int) -> GatewayResponse:
        return parse_gateway_response(
            self.get_json(
                "/v1/fs/tail",
                {"path": path, "max_bytes": str(max_bytes)},
            )
        )

    def echo(self, path: str, line: str) -> GatewayResponse:
        payload = {"path": path, "line": line}
        return parse_gateway_response(self.post_json("/v1/fs/echo", payload))

    def status(self) -> dict:
        return self.get_json("/v1/meta/status")

    def request_auth_headers(self) -> Dict[str, str]:
        if self.request_auth_token is None:
            return {}
        return {
            "Authorization": f"Bearer {self.request_auth_token}",
            "x-cohesix-auth": self.request_auth_token,
        }

    def _build_url(self, path: str, params: Optional[Dict[str, str]]) -> str:
        url = f"{self.rest_url}{path}"
        if not params:
            return url
        return f"{url}?{urllib.parse.urlencode(params)}"


def select_worker_failure_context(
    lines: Sequence[str], worker_id: str, role: str, slot: int
) -> str:
    """Select a bounded, worker-relevant slice from the existing Queen log."""
    identity_markers = (
        worker_id,
        f"role={role} slot={slot}",
        "WORKER_TASK_COMPLETION_FAULT",
        "WORKER_TASK_FAULT",
    )
    selected = [
        line[:WORKER_FAILURE_CONTEXT_LINE_BYTES]
        for line in lines
        if any(marker in line for marker in identity_markers)
    ][-WORKER_FAILURE_CONTEXT_MAX_LINES:]
    return " || ".join(selected)[:WORKER_FAILURE_CONTEXT_MAX_BYTES]


def read_worker_failure_context(
    client: RestClient, worker_id: str, role: str, slot: int
) -> str:
    """Read bounded post-failure context without adding guest hot-path logging."""
    try:
        response = client.tail("/log/queen.log", DEFAULT_LOG_TAIL_BYTES)
    except Exception as exc:
        return f"qlog-unavailable={type(exc).__name__}"
    if response.status != "OK":
        return f"qlog-rejected={response.error or 'unknown'}"[
            :WORKER_FAILURE_CONTEXT_MAX_BYTES
        ]
    context = select_worker_failure_context(response.lines, worker_id, role, slot)
    return context or "qlog-no-matching-worker-event"


def fetch_json(url: str, timeout: float, headers: Optional[Dict[str, str]] = None) -> dict:
    """Fetch JSON from a URL and decode the response."""
    request = urllib.request.Request(url, method="GET")
    for key, value in (headers or {}).items():
        request.add_header(key, value)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        payload = response.read()
    return json.loads(payload)


def fetch_gateway_status_snapshot(
    client: RestClient,
    logger: Optional[RunLogger],
    label: str,
) -> Optional[Dict[str, object]]:
    """Fetch gateway broker counters without making benchmark success depend on them."""
    try:
        status = client.status()
    except Exception as exc:
        emit(logger, f"[gateway] status_{label}=unavailable error={exc}")
        return None
    broker = status.get("broker", {})
    if isinstance(broker, dict):
        counters = [
            f"{key}={broker[key]}"
            for key in GATEWAY_STATUS_BROKER_COUNTERS
            if key in broker
        ]
        emit(logger, f"[gateway] status_{label} {' '.join(counters)}")
    else:
        emit(logger, f"[gateway] status_{label}=available")
    return status


def is_json_number(value: object) -> bool:
    """Return true for JSON numeric values while excluding booleans."""
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def gateway_status_delta(
    before: Optional[Dict[str, object]],
    after: Optional[Dict[str, object]],
) -> Optional[Dict[str, Dict[str, object]]]:
    """Compute counter deltas for gateway status snapshots."""
    if before is None or after is None:
        return None
    before_broker = before.get("broker")
    after_broker = after.get("broker")
    if not isinstance(before_broker, dict) or not isinstance(after_broker, dict):
        return None
    broker_delta: Dict[str, object] = {}
    for key, after_value in after_broker.items():
        before_value = before_broker.get(key)
        if is_json_number(before_value) and is_json_number(after_value):
            diff = after_value - before_value
            broker_delta[key] = diff if diff >= 0 else 0
    if not broker_delta:
        return None
    return {"broker": broker_delta}


def normalize_rest_url(rest_url: str) -> str:
    """Normalize the REST base URL to avoid trailing slashes."""
    while rest_url.endswith("/"):
        rest_url = rest_url[:-1]
    return rest_url


def parse_gateway_response(payload: dict) -> GatewayResponse:
    """Parse a hive-gateway response into a typed container."""
    return GatewayResponse(
        status=str(payload.get("status", "ERR")),
        verb=str(payload.get("verb", "")),
        path=str(payload.get("path", "")),
        end=bool(payload.get("end", False)),
        lines=[line for line in payload.get("lines", []) if isinstance(line, str)],
        bytes=payload.get("bytes"),
        error=payload.get("error"),
    )


def build_status_specs(bounds: dict) -> List[RequestSpec]:
    """Build the /proc status request list based on gateway bounds."""
    observability = bounds.get("observability", {})
    schedule = observability.get("proc_schedule", {})
    lease = observability.get("proc_lease", {})
    schedule_summary = int(schedule.get("summary_bytes", 128))
    schedule_queue = int(schedule.get("queue_bytes", 256))
    lease_summary = int(lease.get("summary_bytes", 160))
    lease_active = int(lease.get("active_bytes", 256))
    lease_preemptions = int(lease.get("preemptions_bytes", 256))
    return [
        RequestSpec("/proc/root/reachable", 64, "cat"),
        RequestSpec("/proc/root/cut_reason", 64, "cat"),
        RequestSpec("/proc/9p/session/active", 128, "cat"),
        RequestSpec("/proc/pressure/busy", 64, "cat"),
        RequestSpec("/proc/pressure/quota", 64, "cat"),
        RequestSpec("/proc/pressure/cut", 64, "cat"),
        RequestSpec("/proc/pressure/policy", 64, "cat"),
        RequestSpec("/proc/schedule/summary", schedule_summary, "cat"),
        RequestSpec("/proc/schedule/queue", schedule_queue, "cat"),
        RequestSpec("/proc/lease/summary", lease_summary, "cat"),
        RequestSpec("/proc/lease/active", lease_active, "cat"),
        RequestSpec("/proc/lease/preemptions", lease_preemptions, "cat"),
    ]


def list_workers(client: RestClient) -> List[str]:
    """Fetch legacy host-model worker IDs from the gateway."""
    response = client.ls("/worker")
    if response.status != "OK":
        raise RestError(
            f"LS /worker failed: {response.error}",
            response,
        )
    return [line.strip() for line in response.lines if line.strip()]


def worker_runtime_bounds(bounds: dict) -> dict:
    """Return and validate the optional generated Worker runtime bounds."""
    runtime = bounds.get("worker_runtime")
    if not isinstance(runtime, dict):
        raise RestError(
            "executable population requires gateway worker_runtime bounds; "
            "absence means unknown"
        )
    maximum = runtime.get("maximum_live_tasks")
    shard_bits = runtime.get("shard_bits")
    template = runtime.get("canonical_telemetry_template")
    roles = runtime.get("roles")
    if (
        not isinstance(maximum, int)
        or isinstance(maximum, bool)
        or maximum <= 0
        or not isinstance(shard_bits, int)
        or isinstance(shard_bits, bool)
        or shard_bits < 1
        or shard_bits > 8
        or template != "/shard/<label>/worker/<id>/telemetry"
        or not isinstance(roles, list)
        or not roles
    ):
        raise RestError("gateway worker_runtime bounds are malformed")
    seen = set()
    executable_slots = 0
    for row in roles:
        if not isinstance(row, dict):
            raise RestError("gateway Worker role row is malformed")
        role = row.get("role")
        declaration = row.get("declaration")
        slots = row.get("executable_slots")
        if (
            not isinstance(role, str)
            or role in seen
            or declaration not in ("executable", "model-only")
            or not isinstance(slots, int)
            or isinstance(slots, bool)
            or slots < 0
            or (declaration == "model-only" and slots != 0)
        ):
            raise RestError("gateway Worker role matrix is inconsistent")
        seen.add(role)
        executable_slots += slots
    if executable_slots != maximum:
        raise RestError("gateway Worker maximum does not match executable slots")
    return runtime


def executable_role_slots(bounds: dict) -> Dict[str, int]:
    """Return the exact compiler-declared executable slot count per role."""
    runtime = worker_runtime_bounds(bounds)
    slots = {
        str(row["role"]): int(row["executable_slots"])
        for row in runtime["roles"]
        if row["declaration"] == "executable"
    }
    if set(slots) != set(EXECUTABLE_WORKER_ROLES) or any(
        count <= 0 for count in slots.values()
    ):
        raise RestError("gateway executable Worker role topology is incomplete")
    return slots


def expected_worker_shard_label(worker_id: str, shard_bits: int) -> str:
    """Compute the compiler-defined two-digit shard label."""
    shard = hashlib.sha256(worker_id.encode("utf-8")).digest()[0]
    if shard_bits < 8:
        shard >>= 8 - shard_bits
    return f"{shard:02x}"


def valid_worker_id(worker_id: str, max_bytes: int) -> bool:
    """Validate a public Worker id before composing a canonical path."""
    encoded = worker_id.encode("utf-8")
    return (
        bool(worker_id)
        and len(encoded) <= max_bytes
        and all(ch.isalnum() or ch in "-_." for ch in worker_id)
    )


def positive_json_int(value: object) -> Optional[int]:
    """Return a positive JSON integer while rejecting booleans."""
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        return None
    return value


def parse_worker_runtime_state(
    lines: Sequence[str],
    worker_id: str,
    telemetry_path: str,
) -> Optional[WorkerInstance]:
    """Parse the latest strict target Worker state record from telemetry."""
    latest: Optional[WorkerInstance] = None
    for line in lines:
        try:
            value = json.loads(line.strip())
        except (TypeError, ValueError):
            continue
        if not isinstance(value, dict) or value.get("schema") not in WORKER_RUNTIME_STATE_SCHEMAS:
            continue
        schema = value["schema"]
        role = value.get("role")
        lifecycle = value.get("state")
        if schema == WORKER_RUNTIME_STATE_SCHEMA_V2:
            identity_fields = value.get("identity")
            sequence_fields = value.get("sequence")
            if (
                not isinstance(identity_fields, list)
                or len(identity_fields) != 4
                or not isinstance(sequence_fields, list)
                or len(sequence_fields) != 4
            ):
                raise RestError(
                    f"malformed structured Worker state at {telemetry_path}"
                )
            slot, lease_epoch, supervisor_generation, cap_generation = identity_fields
            (
                ready_sequence,
                control_sequence,
                receipt_sequence,
                completion_sequence,
            ) = sequence_fields
            v2_values = (*identity_fields, *sequence_fields)
            if any(
                not isinstance(part, int)
                or isinstance(part, bool)
                or part < 0
                or part > WORKER_RUNTIME_STATE_V2_MAX_COUNTER
                for part in v2_values
            ):
                raise RestError(
                    f"malformed structured Worker state at {telemetry_path}"
                )
        else:
            slot = value.get("slot")
            lease_epoch = value.get("lease_epoch")
            supervisor_generation = value.get("supervisor_generation")
            cap_generation = value.get("cap_generation")
            ready_sequence = value.get("ready_sequence")
            control_sequence = value.get("control_sequence")
            receipt_sequence = value.get("receipt_sequence")
            completion_sequence = value.get("completion_sequence")
        identity = (
            positive_json_int(lease_epoch),
            positive_json_int(supervisor_generation),
            positive_json_int(cap_generation),
        )
        if (
            value.get("worker_id") != worker_id
            or role not in EXECUTABLE_WORKER_ROLES
            or lifecycle not in WORKER_LIFECYCLE_STATES
            or not isinstance(slot, int)
            or isinstance(slot, bool)
            or slot < 0
            or not isinstance(ready_sequence, int)
            or isinstance(ready_sequence, bool)
            or ready_sequence < 0
            or not isinstance(control_sequence, int)
            or isinstance(control_sequence, bool)
            or control_sequence < 0
            or not isinstance(receipt_sequence, int)
            or isinstance(receipt_sequence, bool)
            or receipt_sequence < 0
            or not isinstance(completion_sequence, int)
            or isinstance(completion_sequence, bool)
            or completion_sequence < 0
            or any(part is None for part in identity)
            or (lifecycle == "ready" and ready_sequence == 0)
        ):
            raise RestError(
                f"malformed structured Worker state at {telemetry_path}"
            )
        latest = WorkerInstance(
            worker_id=worker_id,
            role=role,
            lifecycle=lifecycle,
            telemetry_path=telemetry_path,
            slot=slot,
            lease_epoch=identity[0],
            supervisor_generation=identity[1],
            cap_generation=identity[2],
            ready_sequence=ready_sequence,
            control_sequence=control_sequence,
            receipt_sequence=receipt_sequence,
            completion_sequence=completion_sequence,
        )
    return latest


def discover_executable_workers(
    client: RestClient,
    bounds: dict,
) -> Tuple[List[WorkerInstance], int]:
    """Discover structured real Worker instances only through canonical shards."""
    runtime = worker_runtime_bounds(bounds)
    shard_bits = int(runtime["shard_bits"])
    max_id_bytes = int(bounds.get("console", {}).get("max_id_len", 32))
    shard_response = client.ls("/shard")
    if shard_response.status != "OK":
        raise RestError(f"LS /shard failed: {shard_response.error}", shard_response)
    labels = sorted({line.strip() for line in shard_response.lines if line.strip()})
    if len(labels) > MAX_DISCOVERED_SHARDS:
        raise RestError("canonical /shard listing exceeds discovery bound")
    instances: List[WorkerInstance] = []
    discovered_ids = set()
    for label in labels:
        if len(label) != 2 or any(ch not in "0123456789abcdef" for ch in label):
            raise RestError("canonical /shard listing contains an invalid label")
        worker_root = f"/shard/{label}/worker"
        response = client.ls(worker_root)
        if response.status != "OK":
            raise RestError(f"LS {worker_root} failed: {response.error}", response)
        for raw_id in response.lines:
            worker_id = raw_id.strip()
            if not valid_worker_id(worker_id, max_id_bytes):
                raise RestError(f"invalid Worker id in {worker_root}")
            if worker_id in discovered_ids:
                raise RestError(f"duplicate Worker id across canonical shards: {worker_id}")
            if expected_worker_shard_label(worker_id, shard_bits) != label:
                raise RestError(f"Worker {worker_id} is published under the wrong shard")
            discovered_ids.add(worker_id)
            if len(discovered_ids) > MAX_DISCOVERED_WORKERS:
                raise RestError("canonical Worker discovery exceeds harness bound")
            telemetry_path = f"{worker_root}/{worker_id}/telemetry"
            tail = client.tail(telemetry_path, MAX_WORKER_STATE_TAIL_BYTES)
            if tail.status != "OK":
                raise RestError(f"TAIL {telemetry_path} failed: {tail.error}", tail)
            instance = parse_worker_runtime_state(
                tail.lines,
                worker_id,
                telemetry_path,
            )
            if instance is not None:
                instances.append(instance)
    instances.sort(key=lambda item: (item.role, item.worker_id))
    return instances, len(discovered_ids)


def valid_sha256(value: object) -> bool:
    """Return true only for canonical lowercase SHA-256 hexadecimal."""
    return (
        isinstance(value, str)
        and len(value) == SHA256_HEX_LENGTH
        and all(ch in "0123456789abcdef" for ch in value)
    )


def executable_qemu_acceptance_binding(
    client: RestClient,
    bounds: dict,
) -> Dict[str, object]:
    """Validate current-session-bound QEMU component evidence before load."""
    status = client.status()
    if status.get("connected") is not True:
        raise RestError("executable population requires a connected target")
    if status.get("backend_class") != "console-projection":
        raise RestError(
            "executable population requires backend_class=console-projection"
        )
    acceptance = status.get("worker_acceptance")
    if not isinstance(acceptance, dict):
        raise RestError("executable population requires validated Worker acceptance")
    if (
        acceptance.get("schema") != "cohesix-worker-task-evidence/v1"
        or acceptance.get("record_kind") != "target-component"
        or acceptance.get("verdict") != "PASS"
        or acceptance.get("target") != "qemu"
        or acceptance.get("execution_proof") != "qemu"
        or not valid_sha256(acceptance.get("evidence_sha256"))
        or not valid_sha256(acceptance.get("topology_sha256"))
    ):
        raise RestError("executable population requires exact PASS QEMU component evidence")

    target_session = acceptance.get("target_session")
    session_fields = (
        "target_session_sha256",
        "manifest_sha256",
        "root_image_sha256",
        "worker_archive_sha256",
        "worker_image_manifest_sha256",
        "worker_abi_sha256",
    )
    if not isinstance(target_session, dict) or any(
        not valid_sha256(target_session.get(field)) for field in session_fields
    ):
        raise RestError("QEMU acceptance lacks a bounded current target-session binding")
    if target_session["manifest_sha256"] != bounds.get("manifest_sha256"):
        raise RestError("QEMU acceptance manifest does not match gateway bounds")

    expected_role_slots = executable_role_slots(bounds)
    expected_worker_count = sum(expected_role_slots.values())
    workers = acceptance.get("workers")
    if not isinstance(workers, list) or len(workers) != expected_worker_count:
        raise RestError("QEMU acceptance requires every executable Worker slot")
    required_inventory = {
        "tcbs",
        "scheduling_contexts",
        "reply_objects",
        "vspaces",
        "cnodes",
        "page_tables",
        "asids",
        "frames",
        "endpoints",
        "notifications",
        "fault_caps",
        "timeout_fault_caps",
        "cspace_slots",
        "untyped_bytes",
    }
    seen_workers = set()
    seen_role_slots = {role: set() for role in EXECUTABLE_WORKER_ROLES}
    for worker in workers:
        if not isinstance(worker, dict):
            raise RestError("QEMU acceptance contains a malformed Worker row")
        role = worker.get("role")
        expected_receipt = "none" if role == "worker-heartbeat" else "confirmed"
        if (
            role not in EXECUTABLE_WORKER_ROLES
            or worker.get("lifecycle") != "ready"
            or worker.get("artifact") != "verified"
            or worker.get("receipt") != expected_receipt
            or worker.get("execution_proof") != "qemu"
            or not valid_sha256(worker.get("image_sha256"))
        ):
            raise RestError("QEMU acceptance Worker state is incomplete")
        for field in (
            "lease_epoch",
            "supervisor_generation",
            "cap_generation",
            "ready_sequence",
            "completion_sequence",
        ):
            if positive_json_int(worker.get(field)) is None:
                raise RestError(f"QEMU acceptance Worker {field} is invalid")
        slot = worker.get("slot")
        core = worker.get("core")
        if (
            not isinstance(slot, int)
            or isinstance(slot, bool)
            or slot < 0
            or not isinstance(core, int)
            or isinstance(core, bool)
            or core < 0
            or core > 3
        ):
            raise RestError("QEMU acceptance Worker slot/core is invalid")
        worker_key = (role, slot)
        if (
            worker_key in seen_workers
            or slot >= expected_role_slots[role]
        ):
            raise RestError("QEMU acceptance Worker role/slot identity is invalid")
        seen_workers.add(worker_key)
        seen_role_slots[role].add(slot)
        scheduling_context = worker.get("scheduling_context")
        if not isinstance(scheduling_context, dict):
            raise RestError("QEMU acceptance Worker SC is absent")
        budget = positive_json_int(scheduling_context.get("budget_us"))
        period = positive_json_int(scheduling_context.get("period_us"))
        if budget is None or period is None or budget > period:
            raise RestError("QEMU acceptance Worker SC is invalid")
        inventory = worker.get("object_inventory")
        if not isinstance(inventory, dict) or set(inventory) != required_inventory:
            raise RestError("QEMU acceptance Worker object inventory is invalid")
        for field, value in inventory.items():
            if (
                not isinstance(value, int)
                or isinstance(value, bool)
                or value < 0
                or (field not in ("endpoints", "reply_objects") and value == 0)
            ):
                raise RestError("QEMU acceptance Worker object inventory is invalid")
    for role, count in expected_role_slots.items():
        if seen_role_slots[role] != set(range(count)):
            raise RestError(f"QEMU acceptance omits an executable {role} slot")
    return json.loads(json.dumps(acceptance))


def gateway_population_axes(
    client: RestClient,
    population_mode: str,
    bounds: Optional[dict] = None,
) -> Tuple[str, str]:
    """Return backend and proof axes without deriving proof from connectivity."""
    if population_mode == POPULATION_HOST_MODEL:
        try:
            status = client.status()
        except Exception as exc:
            raise RestError(
                "host-model population requires gateway backend metadata"
            ) from exc
        backend_class = status.get("backend_class")
        if status.get("connected") is not True:
            raise RestError("host-model population requires a connected backend")
        if backend_class != "host-model":
            raise RestError(
                "host-model population requires backend_class=host-model; "
                "console-projection targets require executable population"
            )
        return "host-model", "host-model"
    if bounds is None:
        raise RestError("executable population requires generated bounds")
    if population_mode == POPULATION_EXECUTABLE_LOG:
        status = client.status()
        if status.get("connected") is not True:
            raise RestError("live-log executable population requires a connected target")
        if status.get("backend_class") != "console-projection":
            raise RestError(
                "live-log executable population requires backend_class=console-projection"
            )
        return "console-projection", "qemu-live-log"
    executable_qemu_acceptance_binding(client, bounds)
    return "console-projection", "qemu"


def gateway_observation_axes(client: RestClient) -> Tuple[str, str]:
    """Describe a read-only gateway without granting target execution proof."""
    try:
        status = client.status()
    except Exception:
        return "unknown", "none"
    backend_class = status.get("backend_class", "unknown")
    if backend_class not in ("host-model", "console-projection"):
        backend_class = "unknown"
    proof_class = (
        "host-model"
        if backend_class == "host-model" and status.get("connected") is True
        else "none"
    )
    return str(backend_class), proof_class


def executable_population_snapshot(
    client: RestClient,
    bounds: dict,
    requested: int,
    population_mode: str = POPULATION_EXECUTABLE,
) -> Tuple[List[WorkerInstance], PopulationSnapshot]:
    """Select an exact READY population without spawning or synthetic expansion."""
    runtime = worker_runtime_bounds(bounds)
    maximum = int(runtime["maximum_live_tasks"])
    if requested < 1 or requested > maximum:
        raise RestError(
            f"requested executable population {requested} exceeds generated "
            f"maximum_live_tasks={maximum}"
        )
    instances, discovered = discover_executable_workers(client, bounds)
    ready = [instance for instance in instances if instance.lifecycle == "ready"]
    backend_class, proof_class = gateway_population_axes(
        client,
        population_mode,
        bounds,
    )
    snapshot = PopulationSnapshot(
        requested=requested,
        discovered=discovered,
        ready=len(ready),
        backend_class=backend_class,
        proof_class=proof_class,
    )
    if len(ready) < requested:
        raise RestError(
            "executable population is not READY: "
            f"requested={requested} discovered={discovered} ready={len(ready)}"
        )
    return ready[:requested], snapshot


def acceptance_workers_by_identity(
    binding: Dict[str, object],
) -> Dict[Tuple[str, int], dict]:
    """Index the validated acceptance projection by exact role-local slot."""
    workers = binding.get("workers")
    if not isinstance(workers, list):
        raise RestError("QEMU acceptance Worker rows are absent")
    indexed: Dict[Tuple[str, int], dict] = {}
    for worker in workers:
        if (
            not isinstance(worker, dict)
            or not isinstance(worker.get("role"), str)
            or not isinstance(worker.get("slot"), int)
            or isinstance(worker.get("slot"), bool)
        ):
            raise RestError("QEMU acceptance Worker identity is malformed")
        key = (str(worker["role"]), int(worker["slot"]))
        if key in indexed:
            raise RestError("QEMU acceptance contains a duplicate Worker identity")
        indexed[key] = worker
    if not indexed:
        raise RestError("QEMU acceptance Worker identity index is empty")
    return indexed


def merge_current_worker_instances(
    state: SimState,
    observations: Sequence[WorkerInstance],
) -> List[WorkerInstance]:
    """Merge incremental telemetry into the exact per-instance projection."""
    with state.ticket_state_lock:
        for candidate in observations:
            current = state.current_workers_by_id.get(candidate.worker_id)
            if (
                current is None
                or worker_instance_ordering_key(candidate)
                >= worker_instance_ordering_key(current)
            ):
                state.current_workers_by_id[candidate.worker_id] = candidate
        return list(state.current_workers_by_id.values())


def initialize_ticket_worker_lanes(
    state: SimState,
    instances: Sequence[WorkerInstance],
) -> None:
    """Create one bounded receipt lane per READY GPU/LoRA Worker instance."""
    lanes: Dict[str, queue.Queue[str]] = {}
    for role in ("worker-gpu", "worker-lora"):
        worker_ids = sorted(
            instance.worker_id
            for instance in instances
            if instance.role == role and instance.lifecycle == "ready"
        )
        if not worker_ids:
            raise RestError(f"receipt pressure has no READY {role} Worker lanes")
        role_lanes: queue.Queue[str] = queue.Queue(maxsize=len(worker_ids))
        for worker_id in worker_ids:
            role_lanes.put_nowait(worker_id)
        lanes[role] = role_lanes
    state.ticket_worker_lanes = lanes
    state.ticket_worker_locks = {
        instance.worker_id: threading.Lock()
        for instance in instances
        if instance.lifecycle == "ready"
    }
    state.ticket_quarantined_workers.clear()
    state.receipt_operation_workers.clear()


def capture_proc_pressure_state(client: RestClient, bounds: dict) -> Dict[str, object]:
    """Capture bounded scheduler/lease projections without interpreting them as proof."""
    observations: Dict[str, object] = {}
    for spec in build_status_specs(bounds):
        if spec.path not in EXECUTABLE_PROC_PATHS:
            continue
        response = client.cat(spec.path, spec.max_bytes)
        if response.status != "OK":
            raise RestError(f"CAT {spec.path} failed: {response.error}", response)
        encoded = "\n".join(response.lines).encode("utf-8")
        observations[spec.path] = {
            "lines": list(response.lines),
            "sha256": hashlib.sha256(encoded).hexdigest(),
            "bytes": len(encoded),
        }
    if set(observations) != set(EXECUTABLE_PROC_PATHS):
        missing = sorted(set(EXECUTABLE_PROC_PATHS) - set(observations))
        raise RestError(
            "executable pressure lacks required bounded /proc MCS observations: "
            + ",".join(missing)
        )
    return observations


def capture_executable_state(
    client: RestClient,
    state: SimState,
    *,
    require_accepted_identity: bool,
) -> Dict[str, object]:
    """Capture exact READY identities plus accepted SC/object topology."""
    if state.acceptance_binding is None:
        raise RestError("executable state capture requires acceptance binding")
    accepted = acceptance_workers_by_identity(state.acceptance_binding)
    observations, _ = discover_executable_workers(client, state.bounds)
    instances = merge_current_worker_instances(state, observations)
    ready = [instance for instance in instances if instance.lifecycle == "ready"]
    ready_by_identity: Dict[Tuple[str, int], WorkerInstance] = {}
    for instance in ready:
        key = (instance.role, instance.slot)
        if key in ready_by_identity:
            raise RestError("multiple READY Workers occupy one executable role slot")
        ready_by_identity[key] = instance
    if set(ready_by_identity) != set(accepted):
        raise RestError("executable pressure lacks the exact accepted READY Worker pool")

    workers: List[Dict[str, object]] = []
    for role, slot in sorted(accepted):
        instance = ready_by_identity[(role, slot)]
        accepted_worker = accepted[(role, slot)]
        if require_accepted_identity:
            for field in (
                "lease_epoch",
                "supervisor_generation",
                "cap_generation",
                "ready_sequence",
            ):
                if getattr(instance, field) != accepted_worker.get(field):
                    raise RestError(
                        f"live {role}/{slot} {field} differs from accepted identity"
                    )
        workers.append(
            {
                **instance.identity_dict(),
                "worker_id": instance.worker_id,
                "lifecycle": instance.lifecycle,
                "telemetry_path": instance.telemetry_path,
                "ready_sequence": instance.ready_sequence,
                "control_sequence": instance.control_sequence,
                "receipt_sequence": instance.receipt_sequence,
                "completion_sequence": instance.completion_sequence,
                "image_sha256": accepted_worker["image_sha256"],
                "core": accepted_worker["core"],
                "scheduling_context": accepted_worker["scheduling_context"],
                "object_inventory": accepted_worker["object_inventory"],
            }
        )
    state.worker_telemetry_paths = {
        instance.worker_id: instance.telemetry_path for instance in ready
    }
    return {
        "workers": workers,
        "proc": capture_proc_pressure_state(client, state.bounds),
    }


def hash_required_fault_artifact(
    path_value: Optional[str],
    label: str,
    markers: Sequence[str],
) -> Tuple[Dict[str, object], str]:
    """Hash one bounded regular artifact and require its frozen marker index."""
    if not path_value:
        raise RestError(f"executable pressure requires --qemu-{label}-log")
    path = pathlib.Path(path_value)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as exc:
        raise RestError(f"cannot open {label} artifact safely: {exc}") from exc
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size <= 0:
            raise RestError(f"{label} artifact must be a non-empty regular file")
        if metadata.st_size > 64 * 1024 * 1024:
            raise RestError(f"{label} artifact exceeds the 64 MiB pressure bound")
        chunks: List[bytes] = []
        remaining = metadata.st_size
        while remaining:
            chunk = os.read(descriptor, min(remaining, 1024 * 1024))
            if not chunk:
                raise RestError(f"{label} artifact changed during bounded read")
            chunks.append(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise RestError(f"{label} artifact grew during bounded read")
        data = b"".join(chunks)
    finally:
        os.close(descriptor)
    text_value = data.decode("utf-8", errors="replace")
    missing = [marker for marker in markers if marker not in text_value]
    if missing:
        raise RestError(f"{label} artifact lacks required markers: {','.join(missing)}")
    return (
        {
            "sha256": hashlib.sha256(data).hexdigest(),
            "bytes": len(data),
        },
        text_value,
    )


def marker_fields(line: str, marker: str) -> Optional[Dict[str, str]]:
    """Parse one exact space-delimited target observation marker."""
    if not line.startswith(f"{marker} "):
        return None
    fields: Dict[str, str] = {}
    for token in line[len(marker) + 1 :].split():
        key, separator, value = token.partition("=")
        if not separator or not key or not value or key in fields:
            raise RestError(f"malformed {marker} field token")
        fields[key] = value
    return fields


def validate_fault_session_binding(
    uart_text: str,
    gdb_text: str,
    acceptance: Dict[str, object],
) -> None:
    """Bind immutable fault transcripts to the shared-validator status record."""
    target_session = acceptance["target_session"]
    if not isinstance(target_session, dict):
        raise RestError("fault transcript validation lacks a target session")
    expected_session = {
        "target": "qemu",
        "machine": "virt",
        "gic_version": "3",
        "root_image_sha256": str(target_session["root_image_sha256"]),
        "worker_archive_sha256": str(target_session["worker_archive_sha256"]),
        "topology_sha256": str(acceptance["topology_sha256"]),
    }
    sessions = [
        fields
        for line in gdb_text.splitlines()
        if (fields := marker_fields(line.strip(), "M26E_QEMU_SESSION")) is not None
    ]
    if sessions != [expected_session]:
        raise RestError("GDB transcript does not bind the exact QEMU target session")

    accepted_workers = acceptance_workers_by_identity(acceptance)
    for (role, slot), worker in accepted_workers.items():
        image_sha256 = str(worker["image_sha256"])
        admission_match = any(
            fields.get("role") == role
            and fields.get("slot") == str(slot)
            and fields.get("image_sha256") == image_sha256
            for line in uart_text.splitlines()
            if (fields := marker_fields(line.strip(), "WORKER_TASK_ADMISSION"))
            is not None
        )
        elf_match = any(
            fields.get("role") == role
            and fields.get("image_sha256") == image_sha256
            and valid_sha256(fields.get("elf_sha256"))
            for line in gdb_text.splitlines()
            if (fields := marker_fields(line.strip(), "M26E_GDB_ELF")) is not None
        )
        if not admission_match or not elf_match:
            raise RestError(
                f"fault transcripts do not bind accepted {role}/{slot} image identity"
            )


def capture_fault_artifacts(
    args: argparse.Namespace,
    acceptance: Dict[str, object],
) -> Tuple[Dict[str, Dict[str, object]], List[str]]:
    """Retain exact UART/GDB inputs; semantic acceptance remains collector-owned."""
    uart_artifact, uart_text = hash_required_fault_artifact(
        args.qemu_uart_log,
        "uart",
        EXECUTABLE_UART_MARKERS,
    )
    gdb_artifact, gdb_text = hash_required_fault_artifact(
        args.qemu_gdb_log,
        "gdb",
        EXECUTABLE_GDB_MARKERS,
    )
    validate_fault_session_binding(uart_text, gdb_text, acceptance)
    artifacts = {"uart": uart_artifact, "gdb": gdb_artifact}
    markers = [
        *(f"uart:{marker}" for marker in EXECUTABLE_UART_MARKERS),
        *(f"gdb:{marker}" for marker in EXECUTABLE_GDB_MARKERS),
    ]
    return artifacts, markers


def require_qemu_fixture_receipt_paths(client: RestClient) -> Tuple[str, str]:
    """Return exact fixture GPU/job subjects after validating live projections."""
    bridge = client.cat("/gpu/bridge/status", 512)
    if bridge.status != "OK" or len(bridge.lines) != 1:
        raise RestError("QEMU receipt pressure requires one GPU bridge status line", bridge)
    fields: Dict[str, str] = {}
    for token in bridge.lines[0].split():
        key, separator, value = token.partition("=")
        if separator and key and value:
            if key in fields:
                raise RestError("GPU bridge status contains a duplicate field")
            fields[key] = value
    if (
        fields.get("state") != "ok"
        or fields.get("mode") != "fixture"
        or fields.get("source") in (None, "", "none")
        or not valid_sha256(fields.get("sha256"))
    ):
        raise RestError(
            "QEMU receipt pressure requires state=ok mode=fixture GPU bridge status"
        )
    for path in (
        "/host/tickets/spec",
        "/host/tickets/spec.snapshot",
        "/host/tickets/status",
    ):
        response = client.tail(path, HOST_TICKET_LOG_TAIL_BYTES)
        if response.status != "OK":
            raise RestError(
                f"QEMU receipt pressure requires {path}: {response.error}",
                response,
            )

    gpu_root = client.ls("/gpu")
    if gpu_root.status != "OK":
        raise RestError("QEMU receipt pressure cannot list /gpu", gpu_root)
    gpu_ids = sorted(
        {
            entry.strip()
            for entry in gpu_root.lines
            if entry.strip() not in {"bridge", "models", "telemetry"}
        }
    )
    if not gpu_ids or len(gpu_ids) > 64 or any(
        not valid_worker_id(gpu_id, 64) for gpu_id in gpu_ids
    ):
        raise RestError("QEMU fixture exposes no bounded GPU subject")

    jobs = client.ls("/queen/export/lora_jobs")
    if jobs.status != "OK":
        raise RestError("QEMU receipt pressure cannot list fixture LoRA jobs", jobs)
    job_ids = sorted({entry.strip() for entry in jobs.lines if entry.strip()})
    if len(job_ids) != 1 or not valid_worker_id(job_ids[0], 64):
        raise RestError("QEMU fixture must expose exactly one bounded LoRA export job")
    job_id = job_ids[0]
    for name in ("telemetry.cbor", "base_model.ref", "policy.toml"):
        path = f"/queen/export/lora_jobs/{job_id}/{name}"
        response = client.cat(path, 8192)
        if response.status != "OK" or not response.lines:
            raise RestError(f"QEMU fixture LoRA job lacks {name}", response)
    return gpu_ids[0], job_id


def bounded_heartbeat_lifecycle_cycle(
    client: RestClient,
    state: SimState,
    timeout_s: float,
) -> List[str]:
    """Kill, observe terminal teardown, and recreate Heartbeat through `/queen/ctl`."""
    heartbeat = [
        instance
        for instance in state.current_workers_by_id.values()
        if instance.role == "worker-heartbeat" and instance.lifecycle == "ready"
    ]
    if len(heartbeat) != 1:
        raise RestError("lifecycle pressure requires one READY Heartbeat Worker")
    before = heartbeat[0]
    kill_worker(client, state, before.worker_id)
    deadline = time.time() + timeout_s
    terminal_observed = False
    while time.time() < deadline:
        observations, _ = discover_executable_workers(client, state.bounds)
        instances = merge_current_worker_instances(state, observations)
        old = next(
            (instance for instance in instances if instance.worker_id == before.worker_id),
            None,
        )
        if old is not None and old.lifecycle == "terminal":
            terminal_observed = True
            break
        time.sleep(0.1)
    if not terminal_observed:
        raise RestError("Heartbeat teardown did not reach terminal within the bound")

    spawn_line = json.dumps(
        {
            "spawn": "heartbeat",
            "ticks": 120,
            "budget": {"ttl_s": 300, "ops": 500},
        },
        separators=(",", ":"),
    )
    echo_with_policy_retry(client, "/queen/ctl", spawn_line, state)
    deadline = time.time() + timeout_s
    after: Optional[WorkerInstance] = None
    while time.time() < deadline:
        observations, _ = discover_executable_workers(client, state.bounds)
        instances = merge_current_worker_instances(state, observations)
        after = next(
            (
                instance
                for instance in instances
                if instance.role == "worker-heartbeat"
                and instance.worker_id != before.worker_id
                and instance.lifecycle == "ready"
            ),
            None,
        )
        if after is not None:
            break
        time.sleep(0.1)
    if after is None or after.supervisor_generation <= before.supervisor_generation:
        raise RestError("Heartbeat recreation did not publish a fresh READY generation")
    state.lifecycle_cycles.append(
        {
            "role": "worker-heartbeat",
            "before": before.identity_dict(),
            "after": after.identity_dict(),
            "kill_admitted": True,
            "recreate_admitted": True,
            "terminal_observed": True,
            "ready_observed": True,
        }
    )
    post = capture_executable_state(
        client,
        state,
        require_accepted_identity=False,
    )
    return [str(worker["worker_id"]) for worker in post["workers"]]


def build_executable_report_state(
    state: SimState,
    required_fault_markers: Sequence[str],
) -> Tuple[str, Dict[str, object]]:
    """Build the exact collector-facing executable pressure state."""
    if (
        state.acceptance_binding is None
        or state.executable_pre_state is None
        or state.executable_post_state is None
        or set(state.fault_artifacts) != {"uart", "gdb"}
        or not state.lifecycle_cycles
        or not state.receipt_operations
    ):
        raise RestError("executable pressure state is incomplete")
    target_session = state.acceptance_binding.get("target_session")
    if not isinstance(target_session, dict):
        raise RestError("executable pressure lacks a target-session summary")
    driven = {
        (operation.get("action"), operation.get("role"))
        for operation in state.receipt_operations
    }
    required_driven = {
        ("gpu.lease.grant", "worker-gpu"),
        ("peft.export", "worker-lora"),
    }
    if not required_driven.issubset(driven):
        raise RestError("executable pressure did not drive both GPU and LoRA receipt paths")
    target_session_sha256 = str(target_session["target_session_sha256"])
    executable_state = {
        "topology_sha256": state.acceptance_binding["topology_sha256"],
        "target_session": {
            field: target_session[field]
            for field in (
                "manifest_sha256",
                "root_image_sha256",
                "worker_archive_sha256",
                "worker_image_manifest_sha256",
                "worker_abi_sha256",
            )
        },
        "pre": state.executable_pre_state,
        "post": state.executable_post_state,
        "lifecycle_cycles": list(state.lifecycle_cycles),
        "receipt_operations": list(state.receipt_operations),
        "fault_artifacts": dict(state.fault_artifacts),
        "required_fault_markers": list(required_fault_markers),
    }
    return target_session_sha256, executable_state


def validate_executable_post_state(state: SimState) -> None:
    """Require bounded recreation and monotonic exact-pool state after pressure."""
    if state.executable_pre_state is None or state.executable_post_state is None:
        raise RestError("executable pre/post state is absent")
    pre_rows = state.executable_pre_state.get("workers")
    post_rows = state.executable_post_state.get("workers")
    if not isinstance(pre_rows, list) or not isinstance(post_rows, list):
        raise RestError("executable pre/post Worker rows are malformed")
    pre = {
        (row.get("role"), row.get("slot")): row
        for row in pre_rows
        if isinstance(row, dict)
    }
    post = {
        (row.get("role"), row.get("slot")): row
        for row in post_rows
        if isinstance(row, dict)
    }
    if set(pre) != set(post) or not pre:
        raise RestError("executable pre/post Worker slot matrix is incomplete")
    identity_fields = (
        "role",
        "slot",
        "lease_epoch",
        "supervisor_generation",
        "cap_generation",
    )
    heartbeat_keys = [key for key in pre if key[0] == "worker-heartbeat"]
    if len(heartbeat_keys) != 1:
        raise RestError("executable state requires one Heartbeat slot")
    heartbeat_key = heartbeat_keys[0]
    heartbeat_pre = pre[heartbeat_key]
    heartbeat_post = post[heartbeat_key]
    if (
        heartbeat_post["supervisor_generation"]
        <= heartbeat_pre["supervisor_generation"]
        or heartbeat_post["worker_id"] == heartbeat_pre["worker_id"]
    ):
        raise RestError("Heartbeat pressure did not retain a fresh generation")
    for role in ("worker-gpu", "worker-lora"):
        advanced = False
        role_keys = [key for key in pre if key[0] == role]
        if not role_keys:
            raise RestError(f"executable state omits {role} slots")
        for key in role_keys:
            if any(pre[key][field] != post[key][field] for field in identity_fields):
                raise RestError(f"{role}/{key[1]} identity changed during pressure")
            if (
                post[key]["receipt_sequence"] < pre[key]["receipt_sequence"]
                or post[key]["completion_sequence"]
                < pre[key]["completion_sequence"]
            ):
                raise RestError(f"{role}/{key[1]} receipt state regressed")
            advanced |= (
                post[key]["receipt_sequence"] > pre[key]["receipt_sequence"]
                and post[key]["completion_sequence"]
                > pre[key]["completion_sequence"]
            )
        if not advanced:
            raise RestError(f"{role} pool did not advance receipt/completion state")


def parse_worker_seq(worker_id: str) -> Optional[int]:
    if not worker_id.startswith("worker-"):
        return None
    suffix = worker_id[len("worker-"):]
    if not suffix.isdigit():
        return None
    value = int(suffix)
    if value <= 0:
        return None
    return value


def seed_next_worker_seq(state: SimState, worker_ids: Sequence[str]) -> None:
    max_seq = 0
    for worker_id in worker_ids:
        seq = parse_worker_seq(worker_id)
        if seq is not None and seq > max_seq:
            max_seq = seq
    if max_seq > 0:
        state.next_worker_seq = max(state.next_worker_seq, max_seq + 1)


def allocate_synthetic_worker_id(state: SimState) -> str:
    worker_id = f"worker-{state.next_worker_seq}"
    state.next_worker_seq += 1
    return worker_id


def worker_seq_max(worker_ids: Sequence[str]) -> int:
    """Return the highest monotonic worker sequence in a worker ID list."""
    max_seq = 0
    for worker_id in worker_ids:
        seq = parse_worker_seq(worker_id)
        if seq is not None and seq > max_seq:
            max_seq = seq
    return max_seq


def worker_listing_looks_truncated_tail(
    current: Sequence[str],
    actual: Sequence[str],
) -> bool:
    """Detect bounded /worker listings that expose only the tail window."""
    if len(actual) >= len(current) or not actual:
        return False
    current_set = set(current)
    actual_set = set(actual)
    if not actual_set.issubset(current_set):
        return False
    if worker_seq_max(actual) != worker_seq_max(current):
        return False
    seqs = sorted(seq for seq in (parse_worker_seq(wid) for wid in actual) if seq)
    if len(seqs) != len(actual):
        return False
    return seqs == list(range(seqs[0], seqs[-1] + 1))


def reconcile_worker_snapshot(
    current: Sequence[str],
    actual: Sequence[str],
) -> Tuple[List[str], int, bool]:
    """Merge a possibly bounded /worker listing with predicted worker IDs."""
    if worker_listing_looks_truncated_tail(current, actual):
        return list(current), 0, True
    max_actual = worker_seq_max(actual)
    max_current = worker_seq_max(current)
    if actual and max_actual < max_current:
        reconciled = [
            worker_id
            for worker_id in current
            if (parse_worker_seq(worker_id) or max_current + 1) <= max_actual
        ]
        return reconciled, len(current) - len(reconciled), False
    actual_set = set(actual)
    missing = sum(1 for worker_id in current if worker_id not in actual_set)
    return list(actual), missing, False


def expand_bounded_worker_listing(
    worker_ids: Sequence[str],
    target: int,
) -> List[str]:
    """Expand a bounded tail /worker listing when it proves the target exists."""
    if len(worker_ids) >= target:
        return list(worker_ids)
    max_seq = worker_seq_max(worker_ids)
    if max_seq < target:
        return list(worker_ids)
    seqs = sorted(seq for seq in (parse_worker_seq(wid) for wid in worker_ids) if seq)
    if len(seqs) != len(worker_ids):
        return list(worker_ids)
    if seqs != list(range(seqs[0], seqs[-1] + 1)):
        return list(worker_ids)
    return [f"worker-{idx}" for idx in range(1, target + 1)]


def build_telemetry_specs(
    workers: Sequence[str],
    max_workers: int,
    max_bytes: int,
    telemetry_paths: Optional[Dict[str, str]] = None,
) -> List[RequestSpec]:
    """Build telemetry tail request list for a subset of workers."""
    specs: List[RequestSpec] = []
    for worker_id in workers[:max_workers]:
        path = (
            telemetry_paths.get(worker_id)
            if telemetry_paths is not None
            else None
        )
        specs.append(
            RequestSpec(path or f"/worker/{worker_id}/telemetry", max_bytes, "tail")
        )
    return specs


def run_request(client: RestClient, spec: RequestSpec) -> None:
    """Execute a single REST request and validate status."""
    if spec.verb == "cat":
        response = client.cat(spec.path, spec.max_bytes)
    else:
        response = client.tail(spec.path, spec.max_bytes)
    if response.status != "OK":
        raise RestError(
            f"{spec.verb.upper()} {spec.path} failed: {response.error}",
            response,
        )


def run_sequential(
    specs: Iterable[RequestSpec],
    client: RestClient,
) -> float:
    """Run requests sequentially and return elapsed seconds."""
    start = time.perf_counter()
    for spec in specs:
        run_request(client, spec)
    return time.perf_counter() - start


def run_parallel(
    specs: Sequence[RequestSpec],
    client: RestClient,
) -> float:
    """Run requests in parallel and return elapsed seconds."""
    start = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(specs)) as executor:
        futures = [executor.submit(run_request, client, spec) for spec in specs]
        for future in futures:
            future.result()
    return time.perf_counter() - start


def measure(
    specs: Sequence[RequestSpec],
    client: RestClient,
    runs: int,
) -> Tuple[List[float], List[float]]:
    """Measure sequential and parallel timings for a spec list."""
    seq_times: List[float] = []
    par_times: List[float] = []
    for _ in range(runs):
        seq_times.append(run_sequential(specs, client))
    for _ in range(runs):
        par_times.append(run_parallel(specs, client))
    return seq_times, par_times


def average(values: Sequence[float]) -> float:
    """Compute average of a list of floats."""
    if not values:
        return 0.0
    return sum(values) / len(values)


def report(
    label: str,
    seq_times: Sequence[float],
    par_times: Sequence[float],
    assert_min_ratio: Optional[float],
    logger: Optional[RunLogger],
) -> None:
    """Print a performance report and enforce minimum ratio if configured."""
    seq_avg = average(seq_times)
    par_avg = average(par_times)
    speedup = seq_avg / par_avg if par_avg > 0 else 0.0
    seq_fmt = [f"{value:.3f}" for value in seq_times]
    par_fmt = [f"{value:.3f}" for value in par_times]
    emit(logger, f"{label} sequential: {seq_fmt} avg={seq_avg:.3f}s")
    emit(logger, f"{label} parallel:   {par_fmt} avg={par_avg:.3f}s")
    emit(logger, f"{label} speedup:    {speedup:.2f}x")
    if assert_min_ratio is not None and speedup < assert_min_ratio:
        raise SystemExit(
            f"{label} speedup {speedup:.2f}x below required {assert_min_ratio:.2f}x"
        )


def clamp_int(value: int, min_value: int, max_value: int, label: str) -> int:
    if value < min_value or value > max_value:
        raise argparse.ArgumentTypeError(
            f"{label} must be in [{min_value}, {max_value}]"
        )
    return value


def clamp_target_workers(target: int, worker_cap: Optional[int]) -> int:
    if worker_cap is not None and target > worker_cap:
        return worker_cap
    return target


def clamp_float(
    value: float, min_value: float, max_value: float, label: str
) -> float:
    if value < min_value or value > max_value:
        raise argparse.ArgumentTypeError(
            f"{label} must be in [{min_value}, {max_value}]"
        )
    return value


def resolve_telemetry_scenario(
    scenario_name: Optional[str], chunk_bytes: int
) -> Optional[TelemetryScenario]:
    if scenario_name is None:
        return None
    artifact_bytes = TELEMETRY_SCENARIO_BYTES[scenario_name]
    entries = max(1, int(math.ceil(artifact_bytes / float(chunk_bytes))))
    # Per operation: create-segment write + latest read + N manifest writes.
    requests_per_operation = entries + 2
    return TelemetryScenario(
        name=scenario_name,
        artifact_bytes=artifact_bytes,
        chunk_bytes=chunk_bytes,
        reference_entries=entries,
        requests_per_operation=requests_per_operation,
    )


def build_telemetry_reference_record(
    seq: int, offset: int, length: int, digest_b64: str
) -> str:
    envelope = {
        "schema": TELEMETRY_REFERENCE_SCHEMA,
        "seq": int(seq),
        "off": int(offset),
        "len": int(length),
        "sha256": digest_b64,
    }
    return json.dumps(envelope, separators=(",", ":"))


def build_telemetry_reference_records_for_bytes(
    total_bytes: int, chunk_bytes: int
) -> List[str]:
    if total_bytes <= 0:
        raise ValueError("total_bytes must be > 0")
    if chunk_bytes <= 0:
        raise ValueError("chunk_bytes must be > 0")
    records: List[str] = []
    offset = 0
    seq = 1
    while offset < total_bytes:
        length = min(chunk_bytes, total_bytes - offset)
        digest_seed = f"cohesix-ref:{total_bytes}:{seq}:{offset}:{length}".encode(
            "utf-8"
        )
        digest = hashlib.sha256(digest_seed).digest()
        digest_b64 = base64.urlsafe_b64encode(digest).decode("ascii").rstrip("=")
        records.append(build_telemetry_reference_record(seq, offset, length, digest_b64))
        offset += length
        seq += 1
    return records


def apply_fast_ramp_defaults(args: argparse.Namespace) -> None:
    if not args.fast_ramp:
        return
    if args.workers_min == DEFAULT_WORKERS_MIN:
        args.workers_min = FAST_RAMP_WORKERS_MIN
    if args.workers_max == DEFAULT_WORKERS_MAX:
        args.workers_max = FAST_RAMP_WORKERS_MAX
    if args.intensity_min == DEFAULT_INTENSITY_MIN:
        args.intensity_min = FAST_RAMP_INTENSITY_MIN
    if args.intensity_max == DEFAULT_INTENSITY_MAX:
        args.intensity_max = FAST_RAMP_INTENSITY_MAX
    if args.duration_mins == DEFAULT_DURATION_MINS:
        args.duration_mins = FAST_RAMP_DURATION_MINS
    if args.ramp_step_secs == DEFAULT_RAMP_STEP_SECS:
        args.ramp_step_secs = FAST_RAMP_RAMP_STEP_SECS
    if abs(args.base_rps - DEFAULT_BASE_RPS) < 1e-9:
        args.base_rps = FAST_RAMP_BASE_RPS
    if args.max_inflight == DEFAULT_MAX_INFLIGHT:
        args.max_inflight = FAST_RAMP_MAX_INFLIGHT


def ramp_progress(elapsed_s: float, duration_s: float, ramp_step_s: float) -> float:
    """Return progress that holds the configured endpoint for the final step."""
    endpoint_start_s = max(duration_s - ramp_step_s, 0.0)
    if endpoint_start_s <= 0.0:
        return 1.0
    return min(1.0, max(0.0, elapsed_s / endpoint_start_s))


def apply_multi_hive_defaults(args: argparse.Namespace, argv_tokens: Sequence[str]) -> None:
    """Derive workers-min/max from federation shape unless explicitly overridden."""
    if not args.multi_hive:
        return
    args.hives = clamp_int(args.hives, 2, 10, "hives")
    args.workers_per_hive = clamp_int(
        args.workers_per_hive, 1, 1500, "workers-per-hive"
    )
    total_workers = args.hives * args.workers_per_hive
    if "--workers-min" not in argv_tokens:
        args.workers_min = total_workers
    if "--workers-max" not in argv_tokens:
        args.workers_max = total_workers


def error_rate(entry: OpStats) -> float:
    if entry.count <= 0:
        return 0.0
    return entry.err / entry.count


def parse_args() -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode",
        choices=("perf", "simulate"),
        default="perf",
        help="Harness mode (default: %(default)s).",
    )
    parser.add_argument(
        "--rest-url",
        default=DEFAULT_REST_URL,
        help="Hive gateway base URL (default: %(default)s).",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=DEFAULT_TIMEOUT_SECS,
        help="REST request timeout in seconds.",
    )
    parser.add_argument(
        "--log-dir",
        default="logs",
        help="Directory for run logs (default: %(default)s).",
    )
    parser.add_argument(
        "--log-prefix",
        default="rest_bench",
        help="Run log filename prefix (default: %(default)s).",
    )
    parser.add_argument(
        "--no-log-stdout",
        action="store_true",
        help="Disable stdout logging (file only).",
    )
    parser.add_argument(
        "--population-mode",
        choices=(
            POPULATION_HOST_MODEL,
            POPULATION_EXECUTABLE,
            POPULATION_EXECUTABLE_LOG,
        ),
        default=POPULATION_HOST_MODEL,
        help=(
            "Population authority: synthetic host model or real structured READY "
            "Workers discovered through canonical /shard paths; executable-log "
            "runs the same live Worker workload without claiming GDB qualification."
        ),
    )

    perf = parser.add_argument_group("perf")
    perf.add_argument(
        "--suite",
        choices=("status", "telemetry", "all"),
        default="status",
        help="Which request suite to measure.",
    )
    perf.add_argument(
        "--runs",
        type=int,
        default=DEFAULT_RUNS,
        help="Number of runs per mode.",
    )
    perf.add_argument(
        "--max-workers",
        type=int,
        default=DEFAULT_MAX_WORKERS,
        help="Maximum worker count for telemetry suite.",
    )
    perf.add_argument(
        "--tail-bytes",
        type=int,
        default=DEFAULT_TAIL_BYTES,
        help=(
            "Max bytes per Worker telemetry tail request; the default admits "
            "one complete bounded structured Worker-state response."
        ),
    )
    perf.add_argument(
        "--assert-min-ratio",
        type=float,
        default=None,
        help="Fail if sequential/parallel speedup is below this ratio.",
    )

    launch = parser.add_argument_group("launch")
    launch.add_argument(
        "--bundle",
        default=None,
        help="Release bundle directory (e.g. releases/Cohesix-0.6.0-alpha-MacOS).",
    )
    launch.add_argument(
        "--version",
        default=None,
        help="Release version string to resolve from releases/.",
    )
    launch.add_argument(
        "--qemu-run",
        default=None,
        help="Path to qemu run script (overrides bundle).",
    )
    launch.add_argument(
        "--qemu-smp",
        default=DEFAULT_QEMU_SMP,
        help="QEMU SMP topology (default: %(default)s).",
    )
    launch.add_argument(
        "--qemu-log",
        default=None,
        help="Path to write QEMU log output.",
    )
    launch.add_argument(
        "--qemu-uart-log",
        default=None,
        help="Immutable live QEMU UART transcript used by executable pressure.",
    )
    launch.add_argument(
        "--qemu-gdb-log",
        default=None,
        help="Immutable qemu-gdb fault transcript used by executable pressure.",
    )
    launch.add_argument(
        "--no-qemu",
        action="store_true",
        help="Skip launching QEMU (assume already running).",
    )
    launch.add_argument(
        "--gateway-bin",
        default=None,
        help="Path to hive-gateway binary (overrides bundle).",
    )
    launch.add_argument(
        "--gateway-mock",
        action="store_true",
        default=(
            os.environ.get("HIVE_GATEWAY_MOCK", "").strip()
            not in ("", "0", "false", "off", "no")
        ),
        help=(
            "Launch hive-gateway with its in-process mock backend; requires "
            "--no-qemu and host-model population."
        ),
    )
    launch.add_argument(
        "--gateway-bind",
        default=DEFAULT_GATEWAY_BIND,
        help="Gateway bind address (default: %(default)s).",
    )
    launch.add_argument(
        "--gateway-log",
        default=None,
        help="Path to write hive-gateway log output.",
    )
    launch.add_argument(
        "--worker-acceptance-root",
        default=os.environ.get("HIVE_GATEWAY_WORKER_ACCEPTANCE_ROOT"),
        help="Gateway trust root for bounded Worker component evidence.",
    )
    launch.add_argument(
        "--worker-acceptance-evidence",
        default=os.environ.get("HIVE_GATEWAY_WORKER_ACCEPTANCE_EVIDENCE"),
        help="Validated QEMU target-component record supplied to the gateway.",
    )
    launch.add_argument(
        "--target-session",
        default=os.environ.get("HIVE_GATEWAY_TARGET_SESSION"),
        help="Exact current target-session record supplied to the gateway.",
    )
    launch.add_argument(
        "--ready-timeout-secs",
        type=int,
        default=DEFAULT_READY_TIMEOUT_SECS,
        help="Timeout budget for QEMU/gateway readiness checks.",
    )
    launch.add_argument(
        "--gateway-pool-control-sessions",
        type=int,
        default=None,
        help="Override hive-gateway pooled control sessions (optional).",
    )
    launch.add_argument(
        "--gateway-pool-telemetry-sessions",
        type=int,
        default=None,
        help="Override hive-gateway pooled telemetry sessions (optional).",
    )
    launch.add_argument(
        "--gateway-broker-control-response-timeout-ms",
        "--gateway-broker-control-timeout-ms",
        dest="gateway_broker_control_response_timeout_ms",
        type=int,
        default=None,
        help=(
            "Override hive-gateway control broker response timeout in milliseconds "
            "(optional)."
        ),
    )
    launch.add_argument(
        "--gateway-broker-telemetry-response-timeout-ms",
        "--gateway-broker-telemetry-timeout-ms",
        dest="gateway_broker_telemetry_response_timeout_ms",
        type=int,
        default=None,
        help=(
            "Override hive-gateway telemetry broker response timeout in milliseconds "
            "(optional)."
        ),
    )
    launch.add_argument(
        "--gateway-control-write-retry-window-ms",
        type=int,
        default=None,
        help=(
            "Override hive-gateway retry window for retryable control writes in "
            "milliseconds; 0 surfaces bounded VM backpressure immediately."
        ),
    )
    launch.add_argument(
        "--no-gateway",
        action="store_true",
        help="Skip launching hive-gateway (assume already running).",
    )
    launch.add_argument(
        "--tcp-host",
        default=DEFAULT_TCP_HOST,
        help="TCP console host for hive-gateway.",
    )
    launch.add_argument(
        "--tcp-port",
        type=int,
        default=DEFAULT_TCP_PORT,
        help="TCP console port for hive-gateway.",
    )
    launch.add_argument(
        "--auth-token",
        default=DEFAULT_AUTH_TOKEN,
        help="Auth token for hive-gateway.",
    )
    launch.add_argument(
        "--request-auth-token",
        default=DEFAULT_REQUEST_AUTH_TOKEN,
        help="REST request auth token for mutating gateway routes.",
    )
    launch.add_argument(
        "--role",
        default=DEFAULT_ROLE,
        help="Role for hive-gateway (default: %(default)s).",
    )

    sim = parser.add_argument_group("simulate")
    sim.add_argument(
        "--workers-min",
        type=int,
        default=DEFAULT_WORKERS_MIN,
        help="Minimum worker count (default: %(default)s).",
    )
    sim.add_argument(
        "--workers-max",
        type=int,
        default=DEFAULT_WORKERS_MAX,
        help="Maximum worker count (default: %(default)s).",
    )
    sim.add_argument(
        "--multi-hive",
        action="store_true",
        help=(
            "Enable federated multi-hive mode; this computes a total worker target from "
            "--hives * --workers-per-hive unless workers-min/max are provided explicitly."
        ),
    )
    sim.add_argument(
        "--hives",
        type=int,
        default=3,
        help="Hive count when --multi-hive is enabled (default: %(default)s).",
    )
    sim.add_argument(
        "--workers-per-hive",
        type=int,
        default=1000,
        help="Per-hive worker cap when --multi-hive is enabled (default: %(default)s).",
    )
    sim.add_argument(
        "--intensity-min",
        type=int,
        default=DEFAULT_INTENSITY_MIN,
        help="Minimum traffic intensity (1-10).",
    )
    sim.add_argument(
        "--intensity-max",
        type=int,
        default=DEFAULT_INTENSITY_MAX,
        help="Maximum traffic intensity (1-10).",
    )
    sim.add_argument(
        "--duration-mins",
        type=int,
        default=DEFAULT_DURATION_MINS,
        help="Simulation duration in minutes (1-60).",
    )
    sim.add_argument(
        "--ramp-step-secs",
        type=int,
        default=DEFAULT_RAMP_STEP_SECS,
        help="Seconds per ramp step (default: %(default)s).",
    )
    sim.add_argument(
        "--entropy",
        type=float,
        default=DEFAULT_ENTROPY,
        help="Command mix entropy (0-10, higher = more varied).",
    )
    sim.add_argument(
        "--seed",
        type=int,
        default=None,
        help="Random seed for reproducible runs.",
    )
    sim.add_argument(
        "--base-rps",
        type=float,
        default=DEFAULT_BASE_RPS,
        help="Base requests/sec per worker at intensity=1.",
    )
    sim.add_argument(
        "--max-inflight",
        type=int,
        default=DEFAULT_MAX_INFLIGHT,
        help="Maximum concurrent in-flight REST requests.",
    )
    sim.add_argument(
        "--include-lifecycle",
        action="store_true",
        help="Include lifecycle control writes in the mix.",
    )
    sim.add_argument(
        "--no-auto-approve",
        action="store_true",
        help="Disable automatic policy approvals for gated writes.",
    )
    sim.add_argument(
        "--no-cleanup",
        action="store_true",
        help="Do not kill spawned workers after the run.",
    )
    sim.add_argument(
        "--no-transient-retries",
        action="store_true",
        help="Disable transient retry loops; each operation is attempted once.",
    )
    sim.add_argument(
        "--no-retries",
        action="store_true",
        help="Alias for --no-transient-retries.",
    )
    sim.add_argument(
        "--fast-ramp",
        action="store_true",
        help=(
            "Apply the accelerated ramp preset (workers/intensity/duration/"
            "ramp/base_rps/max_inflight) unless explicitly overridden."
        ),
    )
    sim.add_argument(
        "--scenario",
        choices=tuple(sorted(TELEMETRY_SCENARIO_BYTES.keys())),
        default=None,
        help="Optional large telemetry scenario preset.",
    )
    sim.add_argument(
        "--error-budget-rate",
        type=float,
        default=None,
        help=(
            "Fail the run when overall error_rate exceeds this threshold "
            "(for example 0.01 for 1%%)."
        ),
    )
    sim.add_argument(
        "--telemetry-reference-chunk-bytes",
        type=int,
        default=DEFAULT_TELEMETRY_REFERENCE_CHUNK_BYTES,
        help="Reference-manifest chunk size used for scenario payload synthesis.",
    )
    sim.add_argument(
        "--strict-control-errors",
        action="store_true",
        help="Count control-plane buffer-full responses as errors.",
    )
    sim.add_argument(
        "--summary-max-error-lines",
        type=int,
        default=DEFAULT_SUMMARY_MAX_ERROR_LINES,
        help="Maximum distinct error lines included in summary artifacts.",
    )

    argv_tokens = sys.argv[1:]
    args = parser.parse_args()
    timeout_explicit = any(
        token == "--timeout" or token.startswith("--timeout=") for token in argv_tokens
    )

    env_tcp_token = (
        os.environ.get("COH_AUTH_TOKEN")
        or os.environ.get("COHSH_AUTH_TOKEN")
        or ""
    ).strip()
    if (not args.auth_token or args.auth_token == DEFAULT_AUTH_TOKEN) and env_tcp_token:
        args.auth_token = env_tcp_token

    env_request_token = (
        os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN")
        or os.environ.get("COHSH_REST_AUTH_TOKEN")
        or os.environ.get("COH_REST_AUTH_TOKEN")
        or ""
    ).strip()
    if (
        (not args.request_auth_token or args.request_auth_token == DEFAULT_REQUEST_AUTH_TOKEN)
        and env_request_token
    ):
        args.request_auth_token = env_request_token

    if args.mode == "simulate":
        if not timeout_explicit and args.timeout == DEFAULT_TIMEOUT_SECS:
            args.timeout = DEFAULT_SIMULATE_TIMEOUT_SECS
        if args.no_retries:
            args.no_transient_retries = True
        args.auto_approve = not args.no_auto_approve
        args.transient_retries = not args.no_transient_retries
        if args.gateway_mock:
            if not args.no_qemu:
                raise SystemExit("--gateway-mock requires --no-qemu")
            if args.no_gateway:
                raise SystemExit("--gateway-mock cannot be combined with --no-gateway")
            if args.population_mode != POPULATION_HOST_MODEL:
                raise SystemExit("--gateway-mock requires host-model population")
        if not args.auth_token.strip() and not (
            args.no_qemu and (args.no_gateway or args.gateway_mock)
        ):
            raise SystemExit(
                "simulate mode requires --auth-token (or COH_AUTH_TOKEN/COHSH_AUTH_TOKEN)"
            )
        args.telemetry_reference_chunk_bytes = clamp_int(
            args.telemetry_reference_chunk_bytes,
            1024,
            128 * 1024 * 1024,
            "telemetry-reference-chunk-bytes",
        )
        apply_fast_ramp_defaults(args)
        apply_multi_hive_defaults(args, argv_tokens)
        if is_live_executable_population(args.population_mode) and args.multi_hive:
            raise SystemExit(
                "executable population mode does not permit synthetic multi-hive expansion"
            )
        if is_live_executable_population(args.population_mode):
            args.qemu_uart_log = args.qemu_uart_log or args.qemu_log
            if not args.qemu_uart_log:
                raise SystemExit(
                    "live executable population requires --qemu-uart-log"
                )
            if (
                args.population_mode == POPULATION_EXECUTABLE
                and not args.qemu_gdb_log
            ):
                raise SystemExit("qualified executable population requires --qemu-gdb-log")
            if args.population_mode == POPULATION_EXECUTABLE and not args.no_gateway and not all(
                (
                    args.worker_acceptance_root,
                    args.worker_acceptance_evidence,
                    args.target_session,
                )
            ):
                raise SystemExit(
                    "executable gateway launch requires --worker-acceptance-root, "
                    "--worker-acceptance-evidence, and --target-session"
                )
        args.duration_mins = clamp_int(args.duration_mins, 1, 60, "duration-mins")
        workers_upper_bound = 1500 if not args.multi_hive else 15000
        args.workers_min = clamp_int(
            args.workers_min, 1, workers_upper_bound, "workers-min"
        )
        args.workers_max = clamp_int(
            args.workers_max, 1, workers_upper_bound, "workers-max"
        )
        if args.workers_min > args.workers_max:
            raise SystemExit("workers-min must be <= workers-max")
        args.intensity_min = clamp_int(args.intensity_min, 1, 10, "intensity-min")
        args.intensity_max = clamp_int(args.intensity_max, 1, 10, "intensity-max")
        if args.intensity_min > args.intensity_max:
            raise SystemExit("intensity-min must be <= intensity-max")
        args.entropy = clamp_float(args.entropy, 0.0, 10.0, "entropy")
        args.base_rps = clamp_float(args.base_rps, 0.1, 1000.0, "base-rps")
        args.max_inflight = clamp_int(args.max_inflight, 1, 4096, "max-inflight")
        if args.error_budget_rate is not None:
            args.error_budget_rate = clamp_float(
                args.error_budget_rate, 0.0, 1.0, "error-budget-rate"
            )
        args.summary_max_error_lines = clamp_int(
            args.summary_max_error_lines,
            32,
            2000,
            "summary-max-error-lines",
        )
        args.ready_timeout_secs = clamp_int(
            args.ready_timeout_secs,
            30,
            1200,
            "ready-timeout-secs",
        )
        if args.gateway_pool_control_sessions is not None:
            args.gateway_pool_control_sessions = clamp_int(
                args.gateway_pool_control_sessions,
                1,
                256,
                "gateway-pool-control-sessions",
            )
        if args.gateway_pool_telemetry_sessions is not None:
            args.gateway_pool_telemetry_sessions = clamp_int(
                args.gateway_pool_telemetry_sessions,
                1,
                512,
                "gateway-pool-telemetry-sessions",
            )
        if args.gateway_broker_control_response_timeout_ms is not None:
            args.gateway_broker_control_response_timeout_ms = clamp_int(
                args.gateway_broker_control_response_timeout_ms,
                5000,
                1_200_000,
                "gateway-broker-control-response-timeout-ms",
            )
        if args.gateway_broker_telemetry_response_timeout_ms is not None:
            args.gateway_broker_telemetry_response_timeout_ms = clamp_int(
                args.gateway_broker_telemetry_response_timeout_ms,
                5000,
                1_200_000,
                "gateway-broker-telemetry-response-timeout-ms",
            )
        if args.gateway_control_write_retry_window_ms is not None:
            args.gateway_control_write_retry_window_ms = clamp_int(
                args.gateway_control_write_retry_window_ms,
                0,
                60_000,
                "gateway-control-write-retry-window-ms",
            )

    return args


def parse_bind_host_port(bind: str, label: str) -> Tuple[str, int]:
    """Parse a host:port bind argument and fail fast on malformed values."""
    parsed = urllib.parse.urlparse(f"tcp://{bind}")
    host = parsed.hostname
    port = parsed.port
    if host is None or port is None:
        raise SystemExit(f"{label} must be host:port (got {bind!r})")
    return host, port


def assert_bind_available(host: str, port: int, label: str) -> None:
    """Fail fast when a required bind port is already occupied."""
    try:
        candidates = socket.getaddrinfo(
            host,
            port,
            type=socket.SOCK_STREAM,
            flags=socket.AI_PASSIVE,
        )
    except socket.gaierror as exc:
        raise SystemExit(f"{label} host {host!r} is not resolvable: {exc}") from exc

    last_error: Optional[OSError] = None
    for family, socktype, proto, _canonname, sockaddr in candidates:
        with socket.socket(family, socktype, proto) as sock:
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            if family == socket.AF_INET6:
                try:
                    sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                except OSError:
                    pass
            try:
                sock.bind(sockaddr)
                return
            except OSError as exc:
                last_error = exc
                if exc.errno == errno.EADDRINUSE:
                    raise SystemExit(
                        f"{label} port {host}:{port} is already in use. "
                        f"Stop the existing process or choose a different port."
                    ) from exc
                continue

    if last_error is not None:
        raise SystemExit(
            f"{label} port {host}:{port} is unavailable: {last_error}"
        ) from last_error
    raise SystemExit(f"{label} port {host}:{port} is unavailable")


def wait_for_port(host: str, port: int, timeout_s: float) -> None:
    """Wait for a TCP port to become available."""
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=1.0):
                return
        except OSError:
            time.sleep(0.5)
    raise TimeoutError(f"Timeout waiting for {host}:{port}")


def validate_tcp_auth(host: str, port: int, token: str, timeout_s: float) -> None:
    """Validate raw TCP auth handshake against the VM console."""
    token = token.strip()
    if not token:
        raise TimeoutError("TCP auth token is required for handshake preflight")
    payload = f"AUTH {token}".encode("utf-8")
    frame = (len(payload) + 4).to_bytes(4, "little") + payload
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=1.0) as sock:
                sock.settimeout(1.5)
                sock.sendall(frame)
                header = sock.recv(4)
                if len(header) != 4:
                    raise TimeoutError("short auth frame header")
                total = int.from_bytes(header, "little")
                if total < 4 or total > 8192:
                    raise TimeoutError(f"invalid auth frame size {total}")
                remaining = total - 4
                payload_bytes = b""
                while len(payload_bytes) < remaining:
                    part = sock.recv(remaining - len(payload_bytes))
                    if not part:
                        break
                    payload_bytes += part
                text = payload_bytes.decode("utf-8", errors="ignore")
                if "OK AUTH" in text or "ERR AUTH" in text:
                    return
        except OSError:
            pass
        time.sleep(0.4)
    raise TimeoutError(f"TCP auth handshake did not succeed for {host}:{port}")


def probe_gateway_readiness(client: RestClient) -> dict:
    """Require a connected backend before probing bounds and the root namespace."""
    status = client.get_json("/v1/meta/status")
    if status.get("connected") is not True:
        raise RestError("Gateway not ready: backend is not connected")
    bounds = client.get_json("/v1/meta/bounds")
    root = client.ls("/")
    if root.status != "OK":
        raise RestError(f"Gateway not ready: LS / returned {root.status}")
    return bounds


def wait_for_gateway(client: RestClient, timeout_s: float) -> dict:
    """Wait for the gateway backend and root namespace to become ready."""
    deadline = time.time() + timeout_s
    last_error: Optional[Exception] = None
    while time.time() < deadline:
        try:
            return probe_gateway_readiness(client)
        except Exception as exc:
            last_error = exc
            time.sleep(0.5)
    raise TimeoutError(f"Gateway did not become ready: {last_error}")


def init_logger(args: argparse.Namespace) -> RunLogger:
    log_dir = args.log_dir
    os.makedirs(log_dir, exist_ok=True)
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    filename = f"{args.log_prefix}_{timestamp}.log"
    path = os.path.join(log_dir, filename)
    handle = open(path, "w", encoding="utf-8")
    logger = RunLogger(path=path, handle=handle, echo_stdout=not args.no_log_stdout)
    logger.log(f"[log] started path={path}")
    return logger


def emit(logger: Optional[RunLogger], message: str) -> None:
    if logger is None:
        print(message)
    else:
        logger.log(message)


def emit_benchmark_marker(
    client: RestClient,
    logger: Optional[RunLogger],
    *,
    mode: str,
    phase: str,
    run_token: str,
    status: str,
) -> bool:
    """Best-effort queen log marker outside measured benchmark loops."""
    line = (
        f"benchmark mode={mode} phase={phase} run={run_token} "
        f"status={status} rest={client.rest_url}"
    )
    try:
        response = client.echo("/log/queen.log", line)
        if response.status != "OK":
            raise RestError(response.error or "gateway rejected benchmark marker", response)
        emit(logger, f"[queen-log] marker phase={phase} status={status} run={run_token}")
        if phase == "end":
            time.sleep(BENCHMARK_MARKER_SETTLE_SECS)
            emit(
                logger,
                "[queen-log] marker visibility settled "
                f"phase={phase} seconds={BENCHMARK_MARKER_SETTLE_SECS:.1f}",
            )
        return True
    except Exception as exc:
        emit(logger, f"[queen-log] marker skipped phase={phase} run={run_token}: {exc}")
        return False


def resolve_bundle_path(version: Optional[str], bundle: Optional[str]) -> Optional[str]:
    """Resolve release bundle directory from version or explicit path."""
    if bundle:
        return bundle
    if version:
        candidate = os.path.join("releases", f"Cohesix-{version}-MacOS")
        if os.path.isdir(candidate):
            return candidate
        candidate = os.path.join("releases", f"Cohesix-{version}-linux")
        if os.path.isdir(candidate):
            return candidate
        raise FileNotFoundError(f"No release bundle for version {version}")
    return None


def infer_bundle_binary(bundle: str, relative_path: str) -> Optional[str]:
    candidate = os.path.join(bundle, relative_path)
    if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
        return candidate
    if os.path.isfile(candidate):
        return candidate
    return None


def launch_process(
    argv: List[str],
    env: Dict[str, str],
    log_path: Optional[str],
) -> subprocess.Popen:
    """Launch a subprocess and stream output to a log file."""
    log_file = None
    if log_path:
        log_dir = os.path.dirname(log_path)
        if log_dir:
            os.makedirs(log_dir, exist_ok=True)
        log_file = open(log_path, "w", encoding="utf-8")
    else:
        log_file = open(os.devnull, "w", encoding="utf-8")
    return subprocess.Popen(
        argv,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        env=env,
        start_new_session=True,
    )


def terminate_process(proc: Optional[subprocess.Popen], label: str) -> None:
    """Terminate a subprocess if it is running."""
    if not proc:
        return
    if proc.poll() is not None:
        return
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        print(f"[{label}] force killing", file=sys.stderr)
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            return
        proc.wait(timeout=5)


def discover_root_entries(client: RestClient) -> List[str]:
    try:
        response = client.ls("/")
    except RestError:
        return []
    if response.status != "OK":
        return []
    return response.lines


def discover_host_paths(client: RestClient) -> List[str]:
    paths: List[str] = []
    try:
        root = client.ls("/host")
    except RestError:
        return paths
    if root.status != "OK":
        return paths
    if "systemd" in root.lines:
        if path_exists(client, "/host/systemd/status"):
            paths.append("/host/systemd/status")
        else:
            for entry in list_dir(client, "/host/systemd")[:3]:
                paths.append(f"/host/systemd/{entry}/status")
    if "docker" in root.lines and path_exists(client, "/host/docker/status"):
        paths.append("/host/docker/status")
    if "k8s" in root.lines:
        for entry in list_dir(client, "/host/k8s/node")[:2]:
            paths.append(f"/host/k8s/node/{entry}/status")
    if "nvidia" in root.lines:
        for entry in list_dir(client, "/host/nvidia/gpu")[:2]:
            paths.append(f"/host/nvidia/gpu/{entry}/status")
    return paths


def discover_gpu_paths(client: RestClient) -> List[str]:
    paths: List[str] = []
    try:
        root = client.ls("/gpu")
    except RestError:
        return paths
    if root.status != "OK":
        return paths
    if "telemetry" in root.lines and path_exists(
        client, "/gpu/telemetry/schema.json"
    ):
        paths.append("/gpu/telemetry/schema.json")
    for entry in root.lines:
        if entry.upper().startswith("GPU"):
            info_path = f"/gpu/{entry}/info"
            if path_exists(client, info_path):
                paths.append(info_path)
    if path_exists(client, "/gpu/bridge/status"):
        paths.append("/gpu/bridge/status")
    return paths


def list_dir(client: RestClient, path: str) -> List[str]:
    try:
        response = client.ls(path)
    except RestError:
        return []
    if response.status != "OK":
        return []
    return response.lines


def path_exists(client: RestClient, path: str) -> bool:
    try:
        response = client.cat(path, 64)
    except RestError:
        return False
    return response.status == "OK"


def policy_enabled(client: RestClient) -> bool:
    try:
        response = client.cat("/policy/rules", 2048)
    except RestError:
        return False
    return response.status == "OK" and bool(response.lines)


def actions_enabled(client: RestClient) -> bool:
    try:
        response = client.ls("/actions")
    except RestError:
        return False
    return response.status == "OK"


def telemetry_ingest_enabled(client: RestClient) -> bool:
    try:
        response = client.ls("/queen/telemetry")
    except RestError:
        return False
    return response.status == "OK"


def queue_approval(client: RestClient, target: str, state: SimState) -> None:
    state.approval_seq += 1
    approval_id = f"approve-{state.approval_seq:06d}"
    line = json.dumps(
        {"id": approval_id, "target": target, "decision": "approve"},
        separators=(",", ":"),
    )
    response = client.echo("/actions/queue", line)
    if response.status != "OK":
        raise RestError(
            f"Approval for {target} failed: {response.error}",
            response,
        )


def remember_lease_id(state: SimState, lease_id: str) -> None:
    with state.lease_lock:
        if lease_id in state.active_leases:
            return
        lease_config = state.bounds.get("control_plane", {}).get("lease", {})
        active_max_entries = int(lease_config.get("active_max_entries", 256))
        if active_max_entries <= 0:
            raise RestError("generated active lease bound must be positive")
        if len(state.active_leases) >= active_max_entries:
            raise RestError(
                "successful lease grant exceeded the generated active lease bound"
            )
        state.active_leases.append(lease_id)


def choose_lease_id(state: SimState) -> Optional[str]:
    with state.lease_lock:
        if not state.active_leases:
            return None
        return state.rng.choice(state.active_leases)


def remove_lease_id(state: SimState, lease_id: str) -> None:
    with state.lease_lock:
        if lease_id in state.active_leases:
            state.active_leases.remove(lease_id)


def allocate_schedule_id(state: SimState) -> str:
    with state.id_lock:
        state.next_schedule_seq += 1
        return f"sched-{state.run_token}-{state.next_schedule_seq:06d}"


def allocate_lease_id(state: SimState) -> str:
    with state.id_lock:
        state.next_lease_seq += 1
        return f"lease-{state.run_token}-{state.next_lease_seq:06d}"


def echo_with_policy_retry(
    client: RestClient,
    path: str,
    line: str,
    state: SimState,
) -> GatewayResponse:
    if path == "/queen/ctl" and state.auto_approve:
        with state.policy_lock:
            return _echo_with_policy_retry_inner(client, path, line, state)
    return _echo_with_policy_retry_inner(client, path, line, state)


def _echo_with_policy_retry_inner(
    client: RestClient,
    path: str,
    line: str,
    state: SimState,
) -> GatewayResponse:
    def is_policy_retryable(error: Optional[str]) -> bool:
        if not error:
            return False
        err_lower = error.lower()
        return "policy" in err_lower or (
            not state.strict_control_errors
            and ("buffer full" in err_lower or "buffer-full" in err_lower)
        )

    admitted_response: Optional[GatewayResponse] = None

    def echo_once() -> GatewayResponse:
        try:
            return client.echo(path, line)
        except RestError as exc:
            if (
                state.strict_control_errors
                and exc.response is not None
                and is_buffer_full_response(exc.response)
            ):
                raise _StrictControlRefusal(str(exc), exc.response) from exc
            raise

    def attempt() -> None:
        nonlocal admitted_response
        response = echo_once()
        if response.status == "OK":
            admitted_response = response
            return
        if state.strict_control_errors and is_buffer_full_response(response):
            raise _StrictControlRefusal(
                f"ECHO {path} failed: {response.error}",
                response,
            )
        if (
            path == "/queen/ctl"
            and state.auto_approve
            and response.error
        ):
            if is_policy_retryable(response.error):
                # Auto-approval must tolerate concurrent /queen/ctl writes that can
                # consume the queued approval before this request retries.
                retry_sleep_s = 0.02
                max_rounds = 24
                for round_idx in range(max_rounds):
                    queue_approval(client, path, state)
                    response_retry = echo_once()
                    if response_retry.status == "OK":
                        admitted_response = response_retry
                        return
                    if (
                        state.strict_control_errors
                        and is_buffer_full_response(response_retry)
                    ):
                        raise _StrictControlRefusal(
                            f"ECHO {path} failed: {response_retry.error}",
                            response_retry,
                        )
                    response = response_retry
                    if not is_policy_retryable(response_retry.error):
                        break
                    if round_idx + 1 < max_rounds:
                        time.sleep(retry_sleep_s)
                        retry_sleep_s = min(retry_sleep_s * 1.5, 0.25)
        raise RestError(
            f"ECHO {path} failed: {response.error}",
            response,
        )

    run_with_retry_policy(
        attempt,
        state,
        timeout_s=10.0,
        label=f"echo {path}",
    )
    if admitted_response is None:
        raise RestError(f"ECHO {path} completed without an admitted response")
    return admitted_response


def spawn_worker(
    client: RestClient,
    state: SimState,
    known_workers: Sequence[str],
) -> str:
    payload = {
        "spawn": "heartbeat",
        "ticks": state.rng.randint(50, 200),
        "budget": {"ttl_s": 300, "ops": 500},
    }
    before = set(known_workers)
    seed_next_worker_seq(state, known_workers)
    predicted_worker = allocate_synthetic_worker_id(state)
    line = json.dumps(payload, separators=(",", ":"))
    run_with_retry_policy(
        lambda: echo_with_policy_retry(client, "/queen/ctl", line, state),
        state,
        timeout_s=15.0,
        label="spawn worker",
    )
    # At high worker counts, repeatedly listing /worker for every spawn turns into
    # a benchmark-side bottleneck. The root-task worker ids are monotonic, so once
    # spawn is acknowledged we can advance deterministically.
    if len(before) < 32:
        try:
            current_workers = list_workers(client)
        except RestError as exc:
            if not is_buffer_full_error(exc):
                raise
        else:
            seed_next_worker_seq(state, current_workers)
            after = set(current_workers)
            new_ids = [worker_id for worker_id in after - before]
            if new_ids:
                new_ids.sort(key=lambda worker_id: parse_worker_seq(worker_id) or 0)
                return new_ids[-1]
    return predicted_worker


def reconcile_worker_ids(
    client: RestClient,
    state: SimState,
    current: List[str],
    spawned: List[str],
    reason: str,
) -> Tuple[List[str], List[str]]:
    """Replace synthetic high-count worker IDs with the gateway's real listing."""
    try:
        actual = list_workers(client)
    except RestError as exc:
        if is_buffer_full_error(exc):
            if state.logger:
                state.logger.log(
                    f"[workers] reconciliation skipped reason={reason} error={exc}"
                )
            return current, spawned
        raise

    seed_next_worker_seq(state, actual)
    reconciled, missing, truncated_tail = reconcile_worker_snapshot(current, actual)
    reconciled_set = set(reconciled)
    reconciled_spawned = [
        worker_id for worker_id in spawned if worker_id in reconciled_set
    ]
    if len(reconciled) != len(current) or missing or truncated_tail:
        if state.logger:
            action = "kept-truncated-tail" if truncated_tail else "reconciled"
            state.logger.log(
                f"[workers] {action} "
                f"reason={reason} before={len(current)} actual={len(actual)} "
                f"after={len(reconciled)} "
                f"removed_synthetic={missing}"
            )
    return reconciled, reconciled_spawned


def kill_worker(client: RestClient, state: SimState, worker_id: str) -> None:
    line = json.dumps({"kill": worker_id}, separators=(",", ":"))
    echo_with_policy_retry(client, "/queen/ctl", line, state)


def current_ready_worker_instance(state: SimState, worker_id: str) -> WorkerInstance:
    """Return one exact cached READY Worker selected from the bounded lane pool."""
    with state.ticket_state_lock:
        current = state.current_workers_by_id.get(worker_id)
    if current is None or current.lifecycle != "ready":
        detail = "absent" if current is None else (
            f"role={current.role} lifecycle={current.lifecycle} "
            f"ready={current.ready_sequence} control={current.control_sequence} "
            f"receipt={current.receipt_sequence} "
            f"completion={current.completion_sequence}"
        )
        raise RestError(f"receipt Worker lane {worker_id} is not READY: {detail}")
    return current


def worker_instance_ordering_key(instance: WorkerInstance) -> Tuple[int, ...]:
    """Order incremental Worker projections without regressing at completion.

    The in-flight control field returns to zero when a completion commits, so
    raw tuple ordering would incorrectly prefer the older in-flight record.
    First order by the maximum operation sequence, then by its durable phase.
    """
    operation_sequence = max(
        instance.control_sequence,
        instance.receipt_sequence,
        instance.completion_sequence,
    )
    operation_phase = 0
    if operation_sequence != 0:
        if instance.completion_sequence == operation_sequence:
            operation_phase = 3
        elif instance.receipt_sequence == operation_sequence:
            operation_phase = 2
        elif instance.control_sequence == operation_sequence:
            operation_phase = 1
    lifecycle_order = {
        "absent": 0,
        "queued": 1,
        "starting": 2,
        "ready": 3,
        "closing": 4,
        "faulted": 5,
        "terminal": 6,
    }
    return (
        instance.supervisor_generation,
        instance.cap_generation,
        instance.lease_epoch,
        instance.ready_sequence,
        operation_sequence,
        operation_phase,
        lifecycle_order[instance.lifecycle],
    )


def terminal_ticket_state(lines: Sequence[str], ticket_id: str) -> Optional[str]:
    """Return the terminal v2 result state for one exact ticket id."""
    terminal = {"succeeded", "failed", "expired"}
    found: Optional[str] = None
    for line in lines:
        try:
            value = json.loads(line)
        except (TypeError, ValueError):
            continue
        if (
            isinstance(value, dict)
            and value.get("schema") == "host-ticket-result/v2"
            and value.get("id") == ticket_id
            and value.get("state") in terminal
        ):
            found = str(value["state"])
    return found


def host_ticket_correlation_digest(ticket_id: str, idempotency_key: str) -> str:
    """Return NineDoor's fixed-width exact-current lookup key."""
    ticket_bytes = ticket_id.encode("utf-8")
    key_bytes = idempotency_key.encode("utf-8")
    if len(ticket_bytes) > 0xFFFF or len(key_bytes) > 0xFFFF:
        raise RestError("v2 ticket correlation input exceeds the u16 wire bound")
    digest = hashlib.sha256()
    digest.update(b"host-ticket-correlation/v1\0")
    digest.update(len(ticket_bytes).to_bytes(2, "big"))
    digest.update(ticket_bytes)
    digest.update(len(key_bytes).to_bytes(2, "big"))
    digest.update(key_bytes)
    return digest.hexdigest()


def parse_host_ticket_current(lines: Sequence[str]) -> HostTicketCurrent:
    """Parse one strict, bounded `/host/tickets/current` record."""
    if len(lines) != 1:
        raise RestError("ticket-current projection must contain exactly one record")
    line = lines[0]
    if len(line.encode("utf-8")) > HOST_TICKET_CURRENT_MAX_BYTES:
        raise RestError("ticket-current projection exceeds its bounded record size")
    tokens = line.split()
    if not tokens or tokens[0] != "HOST_TICKET_CURRENT":
        raise RestError("ticket-current projection has an invalid marker")
    fields: Dict[str, str] = {}
    for token in tokens[1:]:
        key, separator, value = token.partition("=")
        if not separator or not key or not value or key in fields:
            raise RestError("ticket-current projection has malformed fields")
        fields[key] = value
    expected = {
        "schema",
        "state",
        "role",
        "worker",
        "lifecycle",
        "identity",
        "sequence",
        "admission",
    }
    if set(fields) != expected or fields["schema"] != "host-ticket-current/v1":
        raise RestError("ticket-current projection has an invalid schema")
    if fields["state"] not in {"pending", "confirmed", "rejected", "stale"}:
        raise RestError("ticket-current projection has an invalid state")
    if fields["role"] not in EXECUTABLE_WORKER_ROLES:
        raise RestError("ticket-current projection has an invalid Worker role")
    if fields["lifecycle"] not in WORKER_LIFECYCLE_STATES | {"absent"}:
        raise RestError("ticket-current projection has an invalid Worker lifecycle")

    def parse_vector(name: str, width: int) -> List[int]:
        raw = fields[name].split(",")
        if len(raw) != width:
            raise RestError(f"ticket-current {name} vector has invalid width")
        try:
            values = [int(value, 10) for value in raw]
        except ValueError as exc:
            raise RestError(f"ticket-current {name} vector is not numeric") from exc
        if any(value < 0 or value > 0xFFFFFFFFFFFFFFFF for value in values):
            raise RestError(f"ticket-current {name} vector exceeds the u64 wire bound")
        return values

    identity = parse_vector("identity", 4)
    sequence = parse_vector("sequence", 4)
    try:
        admission = int(fields["admission"], 10)
    except ValueError as exc:
        raise RestError("ticket-current admission sequence is not numeric") from exc
    if (
        identity[0] > 0xFFFFFFFF
        or admission <= 0
        or admission > 0xFFFFFFFFFFFFFFFF
    ):
        raise RestError("ticket-current identity or admission is outside its wire bound")
    return HostTicketCurrent(
        state=fields["state"],
        role=fields["role"],
        worker_id=fields["worker"],
        lifecycle=fields["lifecycle"],
        slot=identity[0],
        lease_epoch=identity[1],
        supervisor_generation=identity[2],
        cap_generation=identity[3],
        ready_sequence=sequence[0],
        control_sequence=sequence[1],
        receipt_sequence=sequence[2],
        completion_sequence=sequence[3],
        admission_sequence=admission,
    )


def read_host_ticket_current(
    client: RestClient,
    ticket_id: str,
    idempotency_key: str,
) -> HostTicketCurrent:
    """Read one exact admission without scanning retained ticket logs."""
    digest = host_ticket_correlation_digest(ticket_id, idempotency_key)
    path = f"{HOST_TICKET_CURRENT_PREFIX}{digest}"
    response = client.cat(path, HOST_TICKET_CURRENT_MAX_BYTES)
    if response.status != "OK":
        raise RestError(f"CAT {path} failed: {response.error}", response)
    return parse_host_ticket_current(response.lines)


def run_v2_receipt_operation(
    client: RestClient,
    state: SimState,
    action: str,
    role: str,
    args_value: Dict[str, object],
    subject_ref: str,
    operation_id: Optional[str] = None,
) -> str:
    """Submit and observe one root-admitted v2 Worker receipt operation."""
    operation_started = time.monotonic()
    role_lanes = state.ticket_worker_lanes.get(role)
    if role_lanes is None:
        raise RestError(f"v2 receipt pressure has no bounded Worker pool for {role}")
    preferred_worker: Optional[str] = None
    if operation_id is not None:
        with state.ticket_state_lock:
            preferred_worker = state.receipt_operation_workers.get(operation_id)
            if preferred_worker in state.ticket_quarantined_workers:
                raise RestError(
                    f"v2 receipt operation {operation_id} owns a quarantined Worker lane"
                )
    return_to_pool = preferred_worker is None
    worker_id: Optional[str] = preferred_worker
    lane_deadline = time.monotonic() + 15.0
    while worker_id is None:
        remaining = lane_deadline - time.monotonic()
        if remaining <= 0.0:
            raise RestError(f"v2 receipt pressure exhausted all {role} Worker lanes")
        try:
            candidate = role_lanes.get(timeout=remaining)
        except queue.Empty as exc:
            raise RestError(
                f"v2 receipt pressure exhausted all {role} Worker lanes"
            ) from exc
        with state.ticket_state_lock:
            if candidate in state.ticket_quarantined_workers:
                continue
        worker_id = candidate
    lane_lock = state.ticket_worker_locks.get(worker_id)
    if lane_lock is None:
        raise RestError(f"v2 receipt Worker lane {worker_id} lacks its bounded lock")
    remaining = max(0.0, lane_deadline - time.monotonic())
    if not lane_lock.acquire(timeout=remaining):
        if return_to_pool:
            role_lanes.put_nowait(worker_id)
        raise RestError(f"v2 receipt Worker lane {worker_id} remained busy")
    lane_reusable = False
    try:
        lane_acquired = time.monotonic()
        before = current_ready_worker_instance(state, worker_id)
        if before.role != role:
            raise RestError(f"receipt lane {worker_id} does not belong to {role}")
        with state.ticket_state_lock:
            state.next_ticket_seq += 1
            sequence = state.next_ticket_seq
        ticket_id = f"bench-{state.run_token}-{sequence:06d}"
        idempotency_key = f"idem-{state.run_token}-{sequence:06d}"
        resolved_operation_id = operation_id or f"op-{state.run_token}-{sequence:06d}"
        payload = {
            "schema": "host-ticket/v2",
            "id": ticket_id,
            "idempotency_key": idempotency_key,
            "action": action,
            "args": args_value,
            "receipt_mode": "worker",
            "operation_id": resolved_operation_id,
            "subject_ref": subject_ref,
            "receipt_worker_role": role,
            "receipt_worker_id": before.worker_id,
            "receipt_supervisor_generation": before.supervisor_generation,
            "receipt_cap_generation": before.cap_generation,
        }
        response = client.echo(
            "/host/tickets/spec",
            json.dumps(payload, separators=(",", ":")),
        )
        if response.status != "OK":
            raise RestError(f"v2 ticket admission failed: {response.error}", response)

        admitted_at = time.monotonic()
        deadline = admitted_at + 15.0
        after: Optional[WorkerInstance] = None
        terminal_state: Optional[str] = None
        last_current: Optional[HostTicketCurrent] = None
        terminal_context: Optional[str] = None
        current_reads = 0
        while time.monotonic() < deadline:
            current = read_host_ticket_current(client, ticket_id, idempotency_key)
            last_current = current
            current_reads += 1
            if current.state == "confirmed":
                terminal_state = "succeeded"
            elif current.state == "rejected":
                terminal_state = "failed"
            elif current.state == "stale":
                terminal_state = "expired"
            same_worker_identity = (
                current.role == before.role
                and current.worker_id == before.worker_id
                and current.slot == before.slot
                and current.lease_epoch == before.lease_epoch
                and current.supervisor_generation == before.supervisor_generation
                and current.cap_generation == before.cap_generation
            )
            if (
                terminal_state is not None
                and same_worker_identity
                and current.lifecycle == "ready"
                and current.receipt_sequence > before.receipt_sequence
                and current.completion_sequence > before.completion_sequence
            ):
                after = replace(
                    before,
                    lifecycle=current.lifecycle,
                    ready_sequence=current.ready_sequence,
                    control_sequence=current.control_sequence,
                    receipt_sequence=current.receipt_sequence,
                    completion_sequence=current.completion_sequence,
                )
                break
            if (
                terminal_state is not None
                and same_worker_identity
                and current.lifecycle in {"faulted", "terminal"}
            ):
                terminal_context = read_worker_failure_context(
                    client,
                    before.worker_id,
                    before.role,
                    before.slot,
                )
                if state.logger is not None:
                    state.logger.log(
                        "[worker-failure] "
                        f"ticket={ticket_id} worker={before.worker_id} "
                        f"context={terminal_context}"
                    )
                break
            time.sleep(0.1)
        completed_at = time.monotonic()
        if terminal_state is None or after is None:
            observed = (
                "state=unobserved lifecycle=unobserved sequence=unobserved"
                if last_current is None
                else (
                    f"state={last_current.state} lifecycle={last_current.lifecycle} "
                    "sequence="
                    f"{last_current.control_sequence},"
                    f"{last_current.receipt_sequence},"
                    f"{last_current.completion_sequence}"
                )
            )
            raise RestError(
                f"v2 {action} lacked a correlated terminal Worker receipt "
                f"ticket={ticket_id} worker={before.worker_id} "
                f"before={before.control_sequence},"
                f"{before.receipt_sequence},"
                f"{before.completion_sequence} "
                f"{observed} reads={current_reads} "
                f"diag={terminal_context or 'not-captured'}"
            )
        if terminal_state != "succeeded":
            raise RestError(
                f"v2 {action} pressure operation ended {terminal_state}, not succeeded"
            )
        with state.ticket_state_lock:
            owner = state.receipt_operation_workers.get(resolved_operation_id)
            if owner is not None and owner != before.worker_id:
                raise RestError(
                    f"v2 receipt operation {resolved_operation_id} changed Worker owner"
                )
            state.receipt_operation_workers[resolved_operation_id] = before.worker_id
            state.current_workers_by_id[before.worker_id] = after
            state.receipt_operations.append(
                {
                    "action": action,
                    "role": role,
                    "worker_id": before.worker_id,
                    "sequence_before": {
                        "receipt": before.receipt_sequence,
                        "completion": before.completion_sequence,
                    },
                    "sequence_after": {
                        "receipt": after.receipt_sequence,
                        "completion": after.completion_sequence,
                    },
                    "status": terminal_state,
                    "timing_s": {
                        "lane_wait": lane_acquired - operation_started,
                        "admission": admitted_at - lane_acquired,
                        "completion_wait": completed_at - admitted_at,
                        "total": completed_at - operation_started,
                    },
                    "current_reads": current_reads,
                }
            )
            if len(state.receipt_operations) > 256:
                state.receipt_operations.pop(0)
        lane_reusable = True
        return resolved_operation_id
    finally:
        lane_lock.release()
        if lane_reusable and return_to_pool:
            try:
                role_lanes.put_nowait(worker_id)
            except queue.Full as exc:
                raise RestError(
                    f"v2 receipt lane pool duplicated {worker_id}"
                ) from exc
        elif not lane_reusable:
            with state.ticket_state_lock:
                state.ticket_quarantined_workers.add(worker_id)
            if state.logger is not None:
                state.logger.log(
                    f"[worker-lane] quarantined role={role} worker={worker_id}"
                )


def build_operations(
    bounds: dict,
    root_entries: List[str],
    host_paths: List[str],
    gpu_paths: List[str],
    state: SimState,
) -> List[Operation]:
    ops: List[Operation] = []
    status_specs = build_status_specs(bounds)

    def op_ls(path: str) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, _state: SimState) -> None:
            def attempt() -> None:
                response = client.ls(path)
                if response.status != "OK":
                    raise RestError(f"LS {path} failed: {response.error}", response)

            run_with_retry_policy(
                attempt,
                state,
                timeout_s=5.0,
                label=f"ls {path}",
                base_sleep=0.2,
            )

        return _run

    def op_cat(path: str, max_bytes: int) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, _state: SimState) -> None:
            def attempt() -> None:
                response = client.cat(path, max_bytes)
                if response.status != "OK":
                    raise RestError(f"CAT {path} failed: {response.error}", response)

            run_with_retry_policy(
                attempt,
                state,
                timeout_s=5.0,
                label=f"cat {path}",
                base_sleep=0.2,
            )

        return _run

    def op_tail(path_builder: Callable[[str], str], max_bytes: int) -> Callable[
        [RestClient, str, SimState], None
    ]:
        def _run(client: RestClient, worker: str, sim_state: SimState) -> None:
            if is_live_executable_population(sim_state.population_mode):
                path = sim_state.worker_telemetry_paths.get(worker)
                if path is None:
                    raise RestError(
                        f"executable Worker {worker} has no canonical telemetry path"
                    )
            else:
                path = path_builder(worker)

            def attempt() -> None:
                response = client.tail(path, max_bytes)
                if response.status != "OK":
                    raise RestError(f"TAIL {path} failed: {response.error}", response)

            run_with_retry_policy(
                attempt,
                state,
                timeout_s=5.0,
                label=f"tail {path}",
                base_sleep=0.2,
            )

        return _run

    def op_tail_log(path: str, max_bytes: int) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, _state: SimState) -> None:
            def attempt() -> None:
                attempt_bytes = max_bytes
                for _ in range(3):
                    response = client.tail(path, attempt_bytes)
                    if response.status == "OK":
                        return
                    if response.error and "tail exceeded max_bytes" in response.error:
                        attempt_bytes = min(attempt_bytes * 2, 65536)
                        continue
                    raise RestError(f"TAIL {path} failed: {response.error}", response)
                raise RestError(f"TAIL {path} failed after max_bytes retries")

            run_with_retry_policy(
                attempt,
                state,
                timeout_s=5.0,
                label=f"tail log {path}",
                base_sleep=0.2,
            )

        return _run

    def op_meta_bounds() -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, _state: SimState) -> None:
            run_with_retry_policy(
                lambda: client.get_json("/v1/meta/bounds"),
                state,
                timeout_s=5.0,
                label="meta bounds",
                base_sleep=0.2,
            )

        return _run

    def op_echo(path: str, line_builder: Callable[[str, SimState], str]) -> Callable[
        [RestClient, str, SimState], None
    ]:
        def _run(client: RestClient, worker: str, sim_state: SimState) -> None:
            line = line_builder(worker, sim_state)
            echo_with_policy_retry(client, path, line, sim_state)

        return _run

    def op_echo_best_effort(
        path: str,
        line_builder: Callable[[str, SimState], str],
    ) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, worker: str, sim_state: SimState) -> None:
            line = line_builder(worker, sim_state)
            def attempt() -> None:
                response = client.echo(path, line)
                if response.status == "OK":
                    return
                if should_tolerate_buffer_full(response, sim_state):
                    return
                raise RestError(
                    f"ECHO {path} failed: {response.error}",
                    response,
                )

            run_with_retry_policy(
                attempt,
                sim_state,
                timeout_s=5.0,
                label=f"echo best-effort {path}",
            )

        return _run

    def op_schedule_lifecycle(path: str) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, sim_state: SimState) -> None:
            # One Queen owns the FIFO consumer edge. Keep enqueue and exact-head
            # dequeue together so concurrent producers cannot manufacture an
            # out-of-order completion or leave benchmark-only retained state.
            with sim_state.schedule_lock:
                schedule_id = allocate_schedule_id(sim_state)
                request = json.dumps(
                    {
                        "id": schedule_id,
                        "role": "worker-gpu",
                        "priority": sim_state.rng.randint(1, 5),
                        "ticks": sim_state.rng.randint(1, 5),
                        "budget_ms": sim_state.rng.randint(50, 200),
                    },
                    separators=(",", ":"),
                )
                response = client.echo(path, request)
                if response.status != "OK":
                    if should_tolerate_buffer_full(response, sim_state):
                        return
                    raise RestError(
                        f"ECHO {path} failed: {response.error}",
                        response,
                    )

                dequeue = json.dumps(
                    {"op": "dequeue", "id": schedule_id},
                    separators=(",", ":"),
                )
                response = client.echo(path, dequeue)
                if response.status != "OK":
                    raise RestError(
                        f"ECHO {path} dequeue failed: {response.error}",
                        response,
                    )

        return _run

    def op_lease_grant(path: str) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, sim_state: SimState) -> None:
            lease_id = allocate_lease_id(sim_state)
            line = json.dumps(
                {
                    "op": "grant",
                    "id": lease_id,
                    "subject": "queen",
                    "resource": "gpu0",
                    "ttl_s": sim_state.rng.randint(120, 600),
                    "priority": sim_state.rng.randint(1, 8),
                },
                separators=(",", ":"),
            )
            def attempt() -> None:
                response = client.echo(path, line)
                if response.status == "OK":
                    remember_lease_id(sim_state, lease_id)
                    return
                if should_tolerate_buffer_full(response, sim_state):
                    return
                raise RestError(
                    f"ECHO {path} failed: {response.error}",
                    response,
                )

            run_with_retry_policy(
                attempt,
                sim_state,
                timeout_s=5.0,
                label=f"lease grant {lease_id}",
            )

        return _run

    def op_lease_preempt(path: str) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, sim_state: SimState) -> None:
            lease_id = choose_lease_id(sim_state)
            if lease_id is None:
                return
            line = json.dumps(
                {"op": "preempt", "id": lease_id, "reason": "benchmark"},
                separators=(",", ":"),
            )
            def attempt() -> None:
                response = client.echo(path, line)
                if response.status == "OK":
                    remove_lease_id(sim_state, lease_id)
                    return
                if should_tolerate_buffer_full(response, sim_state):
                    return
                err_lower = (response.error or "").lower()
                # Another thread may have preempted the same lease first.
                if "invalid payload" in err_lower or "invalid-payload" in err_lower:
                    remove_lease_id(sim_state, lease_id)
                    return
                raise RestError(
                    f"ECHO {path} failed: {response.error}",
                    response,
                )

            run_with_retry_policy(
                attempt,
                sim_state,
                timeout_s=5.0,
                label=f"lease preempt {lease_id}",
            )

        return _run

    def op_policy_apply(path: str) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, sim_state: SimState) -> None:
            policy_id = (
                f"rev-{sim_state.rng.randint(2020, 2030)}-"
                f"{sim_state.rng.randint(1, 12):02d}-"
                f"{sim_state.rng.randint(1, 28):02d}"
            )
            line = json.dumps(
                {
                    "op": "apply",
                    "id": policy_id,
                    "sha256": "".join(
                        sim_state.rng.choice("0123456789abcdef") for _ in range(64)
                    ),
                },
                separators=(",", ":"),
            )
            with sim_state.policy_lock:
                echo_with_policy_retry(client, path, line, sim_state)
                sim_state.policy_previous = sim_state.policy_current
                sim_state.policy_current = policy_id

        return _run

    def op_policy_rollback(path: str) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, sim_state: SimState) -> None:
            with sim_state.policy_lock:
                policy_id = sim_state.policy_current
                if policy_id is None:
                    return
                previous = sim_state.policy_previous
                line = json.dumps(
                    {"op": "rollback", "id": policy_id},
                    separators=(",", ":"),
                )
                echo_with_policy_retry(client, path, line, sim_state)
                sim_state.policy_current = previous
                sim_state.policy_previous = None

        return _run

    ops.append(Operation("meta_bounds", 0.4, "meta", op_meta_bounds()))

    for entry in ("/", "/proc", "/queen", "/shard", "/worker", "/gpu", "/host"):
        if entry == "/" or entry.strip("/") in root_entries:
            ops.append(Operation(f"ls_{entry.strip('/') or 'root'}", 1.0, "ls", op_ls(entry)))

    for spec in status_specs:
        ops.append(Operation(f"cat_{spec.path}", 1.0, "status", op_cat(spec.path, spec.max_bytes)))

    if "log" in root_entries:
        log_bytes = max(state.tail_bytes, DEFAULT_LOG_TAIL_BYTES)
        ops.append(
            Operation(
                "tail_queen_log",
                0.6,
                "telemetry",
                op_tail_log("/log/queen.log", log_bytes),
            )
        )

    if "worker" in root_entries or "shard" in root_entries:
        ops.append(
            Operation(
                "tail_worker_telemetry",
                1.2,
                "telemetry",
                op_tail(lambda wid: f"/worker/{wid}/telemetry", state.tail_bytes),
            )
        )

    for path in host_paths:
        ops.append(Operation(f"cat_{path}", 0.5, "host", op_cat(path, 256)))

    for path in gpu_paths:
        ops.append(Operation(f"cat_{path}", 0.5, "gpu", op_cat(path, 2048)))

    if is_live_executable_population(state.population_mode):
        ops.append(
            Operation(
                "worker_gpu_v2_receipt",
                0.15,
                "worker-receipt",
                lambda client, _worker, sim_state: run_v2_receipt_operation(
                    client,
                    sim_state,
                    "gpu.lease.renew",
                    "worker-gpu",
                    {"ttl_s": 30, "priority": 1},
                    require_receipt_subject(
                        sim_state.receipt_gpu_subject,
                        "GPU",
                    ),
                    operation_id=select_receipt_gpu_lease(sim_state),
                ),
            )
        )
        ops.append(
            Operation(
                "worker_lora_v2_receipt",
                0.15,
                "worker-receipt",
                lambda client, _worker, sim_state: run_v2_receipt_operation(
                    client,
                    sim_state,
                    "peft.export",
                    "worker-lora",
                    {},
                    require_receipt_subject(
                        sim_state.receipt_lora_subject,
                        "LoRA",
                    ),
                ),
            )
        )

    if "queen" in root_entries:
        ops.append(
            Operation(
                "schedule_write",
                0.6,
                "control",
                op_schedule_lifecycle("/queen/schedule/ctl"),
            )
        )
        ops.append(
            Operation(
                "lease_grant",
                0.4,
                "control",
                op_lease_grant("/queen/lease/ctl"),
            )
        )
        ops.append(
            Operation(
                "lease_preempt",
                0.3,
                "control",
                op_lease_preempt("/queen/lease/ctl"),
            )
        )
        ops.append(
            Operation(
                "lease_quota",
                0.2,
                "control",
                op_echo(
                    "/queen/lease/ctl",
                    lambda _w, st: json.dumps(
                        {
                            "op": "quota",
                            "subject": "queen",
                            "resource": "gpu0",
                            "max_active": st.rng.randint(1, 4),
                            "max_preemptions": st.rng.randint(2, 8),
                        },
                        separators=(",", ":"),
                    ),
                ),
            )
        )

    if state.policy_enabled:
        ops.append(
            Operation(
                "policy_apply",
                0.3,
                "policy",
                op_policy_apply("/policy/ctl"),
            )
        )
        ops.append(
            Operation(
                "policy_rollback",
                0.2,
                "policy",
                op_policy_rollback("/policy/ctl"),
            )
        )

    if state.telemetry_enabled:
        if state.telemetry_scenario is not None:
            ops.append(
                Operation(
                    "telemetry_reference_manifest",
                    1.2,
                    "telemetry",
                    telemetry_reference_manifest_op,
                )
            )
        else:
            ops.append(
                Operation(
                    "telemetry_segment",
                    0.3,
                    "telemetry",
                    telemetry_segment_op,
                )
            )
            ops.append(
                Operation(
                    "telemetry_append",
                    0.4,
                    "telemetry",
                    telemetry_append_op,
                )
            )

    if state.include_lifecycle and "queen" in root_entries:
        for token in ("cordon", "resume", "reset"):
            ops.append(
                Operation(
                    f"lifecycle_{token}",
                    0.1,
                    "lifecycle",
                    op_echo(
                        "/queen/lifecycle/ctl",
                        lambda _w, _st, t=token: t,
                    ),
                )
            )

    return ops


def telemetry_append_op(client: RestClient, _worker: str, state: SimState) -> None:
    device_id = "bench"
    payload = f"telemetry seq={state.rng.randint(1, 100000)}"

    def attempt() -> None:
        with telemetry_device_lock(state, device_id):
            segment = ensure_telemetry_segment(client, state, device_id)
            last_response: Optional[GatewayResponse] = None
            last_path = f"/queen/telemetry/{device_id}/seg/{segment}"
            for _ in range(3):
                path = f"/queen/telemetry/{device_id}/seg/{segment}"
                last_path = path
                response = client.echo(path, payload)
                if response.status == "OK":
                    return
                last_response = response
                if is_buffer_full_response(response):
                    segment = ensure_telemetry_segment(
                        client,
                        state,
                        device_id,
                        force_new=True,
                    )
                    continue
                if is_telemetry_segment_missing_response(response):
                    segment = refresh_telemetry_segment(client, state, device_id)
                    continue
                raise RestError(f"ECHO {path} failed: {response.error}", response)
            if last_response is not None:
                raise RestError(
                    f"ECHO {last_path} failed: {last_response.error}",
                    last_response,
                )
            raise RestError(f"ECHO {last_path} failed")

    run_with_retry_policy(
        attempt,
        state,
        timeout_s=10.0,
        label=f"telemetry append {device_id}",
        base_sleep=0.2,
    )


def telemetry_reference_records_for_state(state: SimState) -> List[str]:
    if state.telemetry_scenario is None:
        raise RestError("telemetry scenario is not configured")
    if state.telemetry_reference_records is None:
        records = build_telemetry_reference_records_for_bytes(
            state.telemetry_scenario.artifact_bytes,
            state.telemetry_scenario.chunk_bytes,
        )
        if len(records) != state.telemetry_scenario.reference_entries:
            raise RestError(
                "telemetry scenario record synthesis mismatch "
                f"expected={state.telemetry_scenario.reference_entries} "
                f"actual={len(records)}"
            )
        state.telemetry_reference_records = records
    return state.telemetry_reference_records


def telemetry_reference_manifest_op(
    client: RestClient,
    worker: str,
    state: SimState,
) -> None:
    scenario = state.telemetry_scenario
    if scenario is None:
        raise RestError("telemetry reference scenario is not enabled")
    records = telemetry_reference_records_for_state(state)
    device_id = f"bench-{worker}"

    def attempt() -> None:
        with telemetry_device_lock(state, device_id):
            segment = ensure_telemetry_segment(client, state, device_id, force_new=True)
            path = f"/queen/telemetry/{device_id}/seg/{segment}"
            for line in records:
                response = client.echo(path, line)
                if response.status != "OK":
                    raise RestError(
                        f"ECHO {path} failed: {response.error}",
                        response,
                    )

    timeout_s = max(10.0, float(len(records)) * 0.25)
    run_with_retry_policy(
        attempt,
        state,
        timeout_s=timeout_s,
        label=f"telemetry reference {scenario.name}",
        base_sleep=0.2,
    )


def telemetry_segment_op(client: RestClient, _worker: str, state: SimState) -> None:
    run_with_retry_policy(
        lambda: ensure_telemetry_segment(
            client,
            state,
            "bench",
            force_new=True,
        ),
        state,
        timeout_s=10.0,
        label="telemetry segment",
        base_sleep=0.2,
    )


def ensure_telemetry_segment(
    client: RestClient,
    state: SimState,
    device_id: str,
    force_new: bool = False,
) -> str:
    with telemetry_device_lock(state, device_id):
        existing = state.telemetry_segments.get(device_id)
        if existing and not force_new:
            return existing
        return create_telemetry_segment(client, state, device_id)


def refresh_telemetry_segment(client: RestClient, state: SimState, device_id: str) -> str:
    with telemetry_device_lock(state, device_id):
        latest = read_latest_telemetry_segment(client, device_id)
        if latest is not None:
            state.telemetry_segments[device_id] = latest
            return latest
        return create_telemetry_segment(client, state, device_id)


def telemetry_device_lock(state: SimState, device_id: str) -> threading.RLock:
    """Return the bounded lifecycle lock for one telemetry device."""
    with state.telemetry_lock:
        lock = state.telemetry_device_locks.get(device_id)
        if lock is None:
            lock = threading.RLock()
            state.telemetry_device_locks[device_id] = lock
        return lock


def create_telemetry_segment(client: RestClient, state: SimState, device_id: str) -> str:
    ctl_path = f"/queen/telemetry/{device_id}/ctl"
    line = json.dumps(
        {"new": "segment", "mime": "text/plain"}, separators=(",", ":")
    )
    response = echo_with_policy_retry(client, ctl_path, line, state)
    receipt_lines = (
        response.lines
        if response.verb == "ECHO" and response.path == ctl_path and response.end
        else ()
    )
    receipt = parse_telemetry_segment_id(receipt_lines)
    if receipt is not None:
        state.telemetry_segments[device_id] = receipt
        return receipt
    latest = read_latest_telemetry_segment(client, device_id)
    if latest is None:
        raise RestError(
            f"Failed to read latest segment for {device_id}: latest unavailable"
        )
    state.telemetry_segments[device_id] = latest
    return latest


def read_latest_telemetry_segment(client: RestClient, device_id: str) -> Optional[str]:
    path = f"/queen/telemetry/{device_id}/latest"
    response = client.cat(path, 64)
    if (
        response.status != "OK"
        or response.verb != "CAT"
        or response.path != path
        or not response.end
        or not response.lines
    ):
        return None
    return parse_telemetry_segment_id(response.lines)


def parse_telemetry_segment_id(lines: Sequence[str]) -> Optional[str]:
    """Return the first bounded single-component telemetry segment ID."""
    for line in lines:
        segment = line.strip()
        if not segment or len(segment.encode("utf-8")) > MAX_TELEMETRY_SEGMENT_ID_BYTES:
            continue
        if segment in {".", ".."}:
            continue
        if not all(
            character.isascii() and (character.isalnum() or character in "-_")
            for character in segment
        ):
            continue
        return segment
    return None


def is_telemetry_segment_missing_response(response: GatewayResponse) -> bool:
    message = (response.error or "").lower()
    return (
        "segment not found" in message
        or "segment missing" in message
        or "not found" in message
        or "invalid path" in message
    )


def pick_weighted(rng: random.Random, choices: List[Tuple[Operation, float]]) -> Operation:
    total = sum(weight for _, weight in choices)
    if total <= 0:
        return choices[0][0]
    target = rng.random() * total
    acc = 0.0
    for op, weight in choices:
        acc += weight
        if acc >= target:
            return op
    return choices[-1][0]


def require_receipt_subject(value: Optional[str], label: str) -> str:
    """Return one preflight-validated fixture subject for receipt pressure."""
    if value is None:
        raise RestError(f"executable receipt pressure lacks a validated {label} subject")
    return value


def select_receipt_gpu_lease(state: SimState) -> str:
    """Round-robin over exact per-Worker GPU leases without changing ownership."""
    with state.ticket_state_lock:
        if not state.receipt_gpu_lease_ids:
            raise RestError(
                "executable receipt pressure lacks preflight GPU Worker leases"
            )
        index = state.next_receipt_gpu_lease % len(state.receipt_gpu_lease_ids)
        state.next_receipt_gpu_lease = (index + 1) % len(state.receipt_gpu_lease_ids)
        return state.receipt_gpu_lease_ids[index]


def normalize_weights(weights: Dict[str, float]) -> Dict[str, float]:
    total = sum(weights.values())
    if total <= 0:
        return {key: 1.0 / len(weights) for key in weights}
    return {key: value / total for key, value in weights.items()}


def apply_entropy(weights: Dict[str, float], entropy: float) -> Dict[str, float]:
    entropy = max(0.0, min(entropy, 1.0))
    if entropy <= 0.0:
        max_key = max(weights, key=weights.get)
        return {key: (1.0 if key == max_key else 0.0) for key in weights}
    uniform = 1.0 / len(weights)
    base = normalize_weights(weights)
    return {key: (1.0 - entropy) * base[key] + entropy * uniform for key in weights}


def build_worker_profiles(
    operations: List[Operation],
    worker_ids: List[str],
    entropy: float,
    rng: random.Random,
) -> List[WorkerProfile]:
    op_base = {op.name: op.weight for op in operations}
    mixed = apply_entropy(op_base, entropy)
    profiles: List[WorkerProfile] = []
    for worker_id in worker_ids:
        jittered: Dict[str, float] = {}
        for op in operations:
            jitter = rng.lognormvariate(0.0, 0.3 + entropy * 0.2)
            jittered[op.name] = mixed[op.name] * jitter
        normalized = normalize_weights(jittered)
        op_weights = [(op, normalized[op.name]) for op in operations]
        load_factor = rng.uniform(0.6, 1.4)
        profiles.append(WorkerProfile(worker_id, load_factor, op_weights))
    return profiles


def ensure_workers(
    client: RestClient,
    state: SimState,
    target: int,
) -> Tuple[List[str], List[str]]:
    if is_live_executable_population(state.population_mode):
        instances, snapshot = executable_population_snapshot(
            client,
            state.bounds,
            target,
            state.population_mode,
        )
        state.record_population(snapshot)
        state.worker_cap = state.maximum_live_tasks
        state.worker_telemetry_paths = {
            instance.worker_id: instance.telemetry_path for instance in instances
        }
        merge_current_worker_instances(state, instances)
        initialize_ticket_worker_lanes(state, instances)
        return [instance.worker_id for instance in instances], []

    backend_class, proof_class = gateway_population_axes(
        client,
        POPULATION_HOST_MODEL,
    )
    current = expand_bounded_worker_listing(list_workers(client), target)
    seed_next_worker_seq(state, current)
    spawned: List[str] = []
    capacity_limited = False
    while len(current) < target:
        try:
            new_worker = spawn_worker(client, state, current)
        except Exception as exc:
            if is_worker_capacity_event(exc):
                if state.worker_cap is None or len(current) < state.worker_cap:
                    state.worker_cap = len(current)
                if state.logger:
                    state.logger.log(
                        f"worker capacity reached at {len(current)} ({exc})"
                    )
                capacity_limited = True
                break
            raise
        else:
            if new_worker not in current:
                spawned.append(new_worker)
                current.append(new_worker)
    if spawned and (capacity_limited or len(current) >= 32):
        current, spawned = reconcile_worker_ids(
            client,
            state,
            current,
            spawned,
            "ensure",
        )
    state.record_population(
        PopulationSnapshot(
            requested=target,
            discovered=len(current),
            ready=0,
            backend_class=backend_class,
            proof_class=proof_class,
        )
    )
    return current, spawned


def adjust_workers(
    client: RestClient,
    state: SimState,
    worker_ids: List[str],
    spawned: List[str],
    target: int,
) -> Tuple[List[str], List[str]]:
    if is_live_executable_population(state.population_mode):
        if target == len(worker_ids):
            return list(worker_ids), []
        instances, snapshot = executable_population_snapshot(
            client,
            state.bounds,
            target,
            state.population_mode,
        )
        state.record_population(snapshot)
        state.worker_telemetry_paths = {
            instance.worker_id: instance.telemetry_path for instance in instances
        }
        merge_current_worker_instances(state, instances)
        return [instance.worker_id for instance in instances], []

    backend_class, proof_class = gateway_population_axes(
        client,
        POPULATION_HOST_MODEL,
    )
    current = list(worker_ids)
    capacity_limited = False
    if len(current) < target:
        while len(current) < target:
            try:
                new_worker = spawn_worker(client, state, current)
            except Exception as exc:
                if is_worker_capacity_event(exc):
                    if state.worker_cap is None or len(current) < state.worker_cap:
                        state.worker_cap = len(current)
                    if state.logger:
                        state.logger.log(
                            f"worker capacity reached at {len(current)} ({exc})"
                        )
                    capacity_limited = True
                    break
                raise
            else:
                if new_worker not in current:
                    spawned.append(new_worker)
                    current.append(new_worker)
        if spawned and (capacity_limited or len(current) >= 32):
            current, spawned = reconcile_worker_ids(
                client,
                state,
                current,
                spawned,
                "adjust",
            )
    elif len(current) > target:
        while len(current) > target and spawned:
            victim = spawned.pop()
            kill_worker(client, state, victim)
            if victim in current:
                current.remove(victim)
    state.record_population(
        PopulationSnapshot(
            requested=target,
            discovered=len(current),
            ready=0,
            backend_class=backend_class,
            proof_class=proof_class,
        )
    )
    return current, spawned


def run_simulation(args: argparse.Namespace) -> int:
    rng = random.Random(args.seed)
    entropy = args.entropy / 10.0

    bundle = resolve_bundle_path(args.version, args.bundle)
    qemu_run = args.qemu_run
    gateway_bin = args.gateway_bin
    if bundle:
        qemu_run = qemu_run or infer_bundle_binary(bundle, "qemu/run.sh")
        gateway_bin = gateway_bin or infer_bundle_binary(bundle, "bin/hive-gateway")

    rest_url = args.rest_url
    if args.gateway_bind and args.rest_url == DEFAULT_REST_URL:
        rest_url = f"http://{args.gateway_bind}"

    qemu_proc: Optional[subprocess.Popen] = None
    gateway_proc: Optional[subprocess.Popen] = None

    try:
        if not args.no_qemu:
            assert_bind_available(args.tcp_host, args.tcp_port, "QEMU console")
            if not qemu_run:
                raise SystemExit("QEMU launch requested but no run script found.")
            env = os.environ.copy()
            env["COHESIX_QEMU_SMP_TOPO"] = args.qemu_smp
            qemu_proc = launch_process([qemu_run], env, args.qemu_log)
            wait_for_port(args.tcp_host, args.tcp_port, args.ready_timeout_secs)
            validate_tcp_auth(
                args.tcp_host,
                args.tcp_port,
                args.auth_token,
                args.ready_timeout_secs,
            )
        else:
            if args.no_gateway:
                emit(
                    args.logger,
                    "using external gateway; skipping direct TCP auth preflight",
                )
            elif args.gateway_mock:
                emit(
                    args.logger,
                    "using managed host-model gateway; skipping target TCP auth preflight",
                )
            else:
                wait_for_port(args.tcp_host, args.tcp_port, args.ready_timeout_secs)
                validate_tcp_auth(
                    args.tcp_host,
                    args.tcp_port,
                    args.auth_token,
                    args.ready_timeout_secs,
                )

        if not args.no_gateway:
            gateway_host, gateway_port = parse_bind_host_port(
                args.gateway_bind,
                "gateway-bind",
            )
            assert_bind_available(gateway_host, gateway_port, "Gateway bind")
            if not gateway_bin:
                raise SystemExit("Gateway launch requested but no binary found.")
            if not args.request_auth_token.strip():
                raise SystemExit(
                    "simulate mode requires --request-auth-token (or HIVE_GATEWAY_REQUEST_AUTH_TOKEN)"
                )
            env = os.environ.copy()
            env["COH_TCP_HOST"] = args.tcp_host
            env["COH_TCP_PORT"] = str(args.tcp_port)
            env["COH_AUTH_TOKEN"] = args.auth_token
            env["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"] = args.request_auth_token
            env["COH_ROLE"] = args.role
            env["HIVE_GATEWAY_BIND"] = args.gateway_bind
            gateway_cmd = [gateway_bin, "--bind", args.gateway_bind]
            if args.gateway_mock:
                gateway_cmd.append("--mock")
            if args.worker_acceptance_root is not None:
                gateway_cmd.extend(
                    ["--worker-acceptance-root", args.worker_acceptance_root]
                )
            if args.worker_acceptance_evidence is not None:
                gateway_cmd.extend(
                    [
                        "--worker-acceptance-evidence",
                        args.worker_acceptance_evidence,
                    ]
                )
            if args.target_session is not None:
                gateway_cmd.extend(["--target-session", args.target_session])
            if args.gateway_pool_control_sessions is not None:
                gateway_cmd.extend(
                    [
                        "--pool-control-sessions",
                        str(args.gateway_pool_control_sessions),
                    ]
                )
            if args.gateway_pool_telemetry_sessions is not None:
                gateway_cmd.extend(
                    [
                        "--pool-telemetry-sessions",
                        str(args.gateway_pool_telemetry_sessions),
                    ]
                )
            if args.gateway_broker_control_response_timeout_ms is not None:
                env["HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS"] = str(
                    args.gateway_broker_control_response_timeout_ms
                )
                gateway_cmd.extend(
                    [
                        "--broker-control-response-timeout-ms",
                        str(args.gateway_broker_control_response_timeout_ms),
                    ]
                )
            if args.gateway_broker_telemetry_response_timeout_ms is not None:
                env["HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS"] = str(
                    args.gateway_broker_telemetry_response_timeout_ms
                )
                gateway_cmd.extend(
                    [
                        "--broker-telemetry-response-timeout-ms",
                        str(args.gateway_broker_telemetry_response_timeout_ms),
                    ]
                )
            if args.gateway_control_write_retry_window_ms is not None:
                env["HIVE_GATEWAY_CONTROL_WRITE_RETRY_WINDOW_MS"] = str(
                    args.gateway_control_write_retry_window_ms
                )
                gateway_cmd.extend(
                    [
                        "--control-write-retry-window-ms",
                        str(args.gateway_control_write_retry_window_ms),
                    ]
                )
            gateway_proc = launch_process(
                gateway_cmd,
                env,
                args.gateway_log,
            )

        client = RestClient(rest_url, args.timeout, args.request_auth_token)
        bounds = wait_for_gateway(client, args.ready_timeout_secs)
        if args.population_mode == POPULATION_HOST_MODEL:
            # A target-backed console projection owns only compiler-admitted
            # executable slots. Synthetic 24..120 Worker populations belong to
            # the explicit host-model backend and must fail before any mutation.
            gateway_population_axes(client, POPULATION_HOST_MODEL)
        maximum_live_tasks: Optional[int] = None
        acceptance_binding: Optional[Dict[str, object]] = None
        fixture_gpu_id: Optional[str] = None
        fixture_lora_job: Optional[str] = None
        if is_live_executable_population(args.population_mode):
            runtime = worker_runtime_bounds(bounds)
            maximum_live_tasks = int(runtime["maximum_live_tasks"])
            if args.workers_max > maximum_live_tasks:
                raise SystemExit(
                    "executable workers-max exceeds generated maximum_live_tasks: "
                    f"{args.workers_max} > {maximum_live_tasks}"
                )
            if args.population_mode == POPULATION_EXECUTABLE:
                acceptance_binding = executable_qemu_acceptance_binding(client, bounds)
            fixture_gpu_id, fixture_lora_job = require_qemu_fixture_receipt_paths(
                client
            )
            # Validate the complete marker and identity binding before issuing
            # any benchmark traffic. The exact hashes are captured again after
            # the workload for the retained report.
            if acceptance_binding is not None:
                capture_fault_artifacts(args, acceptance_binding)

        root_entries = discover_root_entries(client)
        host_paths = discover_host_paths(client)
        gpu_paths = discover_gpu_paths(client)
        policy_on = policy_enabled(client)
        actions_on = actions_enabled(client)
        telemetry_on = telemetry_ingest_enabled(client)
        scenario = resolve_telemetry_scenario(
            args.scenario, args.telemetry_reference_chunk_bytes
        )
        if scenario is not None and not telemetry_on:
            raise SystemExit(
                "telemetry scenario requested but /queen/telemetry is unavailable"
            )

        state = SimState(
            bounds=bounds,
            rest_url=rest_url,
            rng=rng,
            entropy=entropy,
            tail_bytes=args.tail_bytes,
            policy_enabled=policy_on,
            actions_enabled=actions_on,
            telemetry_enabled=telemetry_on,
            include_lifecycle=args.include_lifecycle,
            auto_approve=args.auto_approve,
            transient_retries=args.transient_retries,
            strict_control_errors=args.strict_control_errors,
            logger=args.logger,
            telemetry_scenario=scenario,
            telemetry_reference_chunk_bytes=args.telemetry_reference_chunk_bytes,
            population_mode=args.population_mode,
            maximum_live_tasks=maximum_live_tasks,
            acceptance_binding=acceptance_binding,
            receipt_gpu_subject=fixture_gpu_id,
            receipt_lora_subject=fixture_lora_job,
        )
        emit_benchmark_marker(
            client,
            args.logger,
            mode="simulate",
            phase="start",
            run_token=state.run_token,
            status="running",
        )

        worker_ids, spawned = ensure_workers(client, state, args.workers_min)
        if args.population_mode == POPULATION_EXECUTABLE:
            state.executable_pre_state = capture_executable_state(
                client,
                state,
                require_accepted_identity=True,
            )
            worker_ids = bounded_heartbeat_lifecycle_cycle(
                client,
                state,
                float(args.ready_timeout_secs),
            )
        if is_live_executable_population(args.population_mode):
            gpu_lanes = state.ticket_worker_lanes.get("worker-gpu")
            if gpu_lanes is None or gpu_lanes.maxsize <= 0:
                raise RestError("executable pressure lacks READY GPU Worker lanes")
            state.receipt_gpu_lease_ids = [
                run_v2_receipt_operation(
                    client,
                    state,
                    "gpu.lease.grant",
                    "worker-gpu",
                    {"ttl_s": 30, "priority": 1},
                    fixture_gpu_id,
                )
                for _ in range(gpu_lanes.maxsize)
            ]
            run_v2_receipt_operation(
                client,
                state,
                "peft.export",
                "worker-lora",
                {},
                fixture_lora_job,
            )

        operations = build_operations(
            bounds,
            root_entries,
            host_paths,
            gpu_paths,
            state,
        )
        if not operations:
            raise SystemExit("No operations available to run.")
        gateway_status_start = fetch_gateway_status_snapshot(
            client,
            args.logger,
            "start",
        )

        stats: Dict[str, OpStats] = {}
        stats_lock = threading.Lock()
        overall = OpStats()
        concurrency = ConcurrencyStats()
        ramp_rows: List[Dict[str, object]] = []
        run_error: Optional[Exception] = None

        duration_s = args.duration_mins * 60
        ramp_step = max(1, args.ramp_step_secs)
        start_time = time.time()
        end_time = start_time + duration_s
        gateway_control_retry_window = (
            args.gateway_control_write_retry_window_ms
            if args.gateway_control_write_retry_window_ms is not None
            else "default"
        )

        args.logger.log(
            f"[simulate] duration={args.duration_mins}m "
            f"workers={args.workers_min}-{args.workers_max} "
            f"population_mode={args.population_mode} "
            f"multi_hive={'on' if args.multi_hive else 'off'} "
            f"hives={args.hives if args.multi_hive else 1} "
            f"workers_per_hive={args.workers_per_hive if args.multi_hive else args.workers_max} "
            f"intensity={args.intensity_min}-{args.intensity_max} "
            f"rest={rest_url} "
            f"transient_retries={'on' if args.transient_retries else 'off'} "
            f"strict_control_errors={'on' if args.strict_control_errors else 'off'} "
            f"gateway_pool_control={args.gateway_pool_control_sessions or 'default'} "
            f"gateway_pool_telemetry={args.gateway_pool_telemetry_sessions or 'default'} "
            f"gateway_broker_control_response_timeout_ms="
            f"{args.gateway_broker_control_response_timeout_ms or 'default'} "
            f"gateway_broker_telemetry_response_timeout_ms="
            f"{args.gateway_broker_telemetry_response_timeout_ms or 'default'} "
            f"gateway_control_write_retry_window_ms="
            f"{gateway_control_retry_window} "
            f"scenario={scenario.name if scenario else 'mixed'} "
            f"error_budget_rate={args.error_budget_rate if args.error_budget_rate is not None else 'none'}"
        )
        if scenario is not None:
            args.logger.log(
                f"[simulate] scenario={scenario.name} artifact_bytes={scenario.artifact_bytes} "
                f"chunk_bytes={scenario.chunk_bytes} reference_entries={scenario.reference_entries} "
                f"requests_per_operation={scenario.requests_per_operation}"
            )

        executor = concurrent.futures.ThreadPoolExecutor(
            max_workers=args.max_inflight
        )
        semaphore = threading.BoundedSemaphore(args.max_inflight)

        try:
            try:
                step_index = 0
                while time.time() < end_time:
                    now = time.time()
                    progress = ramp_progress(
                        now - start_time,
                        duration_s,
                        ramp_step,
                    )
                    target_workers = int(
                        round(
                            args.workers_min
                            + (args.workers_max - args.workers_min) * progress
                        )
                    )
                    target_workers = clamp_target_workers(target_workers, state.worker_cap)
                    target_intensity = args.intensity_min + (
                        args.intensity_max - args.intensity_min
                    ) * progress

                    worker_ids, spawned = adjust_workers(
                        client, state, worker_ids, spawned, target_workers
                    )
                    profiles = build_worker_profiles(
                        operations, worker_ids, entropy, rng
                    )
                    load_weights = normalize_weights(
                        {profile.worker_id: profile.load_factor for profile in profiles}
                    )
                    worker_lookup = {profile.worker_id: profile for profile in profiles}

                    step_end = min(end_time, now + ramp_step)
                    rps = args.base_rps * target_intensity * max(len(worker_ids), 1)
                    if (
                        scenario is not None
                        and scenario.requests_per_operation > 0
                    ):
                        rps = rps / scenario.requests_per_operation
                    if rps <= 0:
                        time.sleep(step_end - time.time())
                        continue
                    interval = 1.0 / rps

                    args.logger.log(
                        f"[simulate] workers={len(worker_ids)} "
                        f"intensity={target_intensity:.1f} rps={rps:.1f}"
                    )
                    with stats_lock:
                        step_start_count = overall.count
                        step_start_ok = overall.ok
                        step_start_err = overall.err
                    step_started_at = time.time()
                    step_max_inflight = 0

                    while time.time() < step_end:
                        remaining_s = step_end - time.time()
                        if remaining_s <= 0:
                            break
                        worker_id = pick_worker(rng, load_weights)
                        profile = worker_lookup[worker_id]
                        op = pick_weighted(rng, profile.op_weights)
                        if not semaphore.acquire(timeout=remaining_s):
                            break
                        current_inflight = concurrency.start()
                        step_max_inflight = max(step_max_inflight, current_inflight)
                        try:
                            executor.submit(
                                execute_operation,
                                client,
                                op,
                                worker_id,
                                state,
                                stats,
                                stats_lock,
                                overall,
                                semaphore,
                                concurrency,
                            )
                        except Exception:
                            concurrency.finish()
                            semaphore.release()
                            raise
                        sleep_s = min(interval, max(0.0, step_end - time.time()))
                        if sleep_s > 0:
                            time.sleep(sleep_s)

                    with stats_lock:
                        step_ops = overall.count - step_start_count
                        step_ok = overall.ok - step_start_ok
                        step_err = overall.err - step_start_err
                        cumulative_avg_s = overall.avg()
                        cumulative_p95_s = overall.percentile(95)
                        cumulative_p99_s = overall.percentile(99)
                    err_rate = 0.0 if step_ops == 0 else step_err / step_ops
                    step_elapsed_s = max(time.time() - step_started_at, 1e-9)
                    ramp_rows.append(
                        {
                            "step": step_index,
                            "workers": len(worker_ids),
                            "intensity": round(target_intensity, 3),
                            "rps": round(rps, 3),
                            "ops": step_ops,
                            "ok": step_ok,
                            "err": step_err,
                            "err_rate": round(err_rate, 6),
                            "throughput_ops_s": round(step_ops / step_elapsed_s, 3),
                            "ok_ops_s": round(step_ok / step_elapsed_s, 3),
                            "max_inflight_observed": step_max_inflight,
                            "max_inflight_configured": args.max_inflight,
                            "cumulative_avg_s": round(cumulative_avg_s, 6),
                            "cumulative_p95_s": round(cumulative_p95_s, 6),
                            "cumulative_p99_s": round(cumulative_p99_s, 6),
                        }
                    )
                    step_index += 1
            except Exception as exc:
                run_error = exc
        finally:
            executor.shutdown(wait=True)

        args.logger.log("[simulate] summary")
        report_stats(overall, stats, args.logger)
        overall_err_rate = error_rate(overall)
        error_budget_pass = (
            args.error_budget_rate is None
            or overall_err_rate <= args.error_budget_rate
        )
        args.logger.log(
            f"[simulate] reliability error_rate={overall_err_rate:.6f} "
            f"budget={args.error_budget_rate if args.error_budget_rate is not None else 'none'} "
            f"pass={'yes' if error_budget_pass else 'no'}"
        )
        target_session_sha256: Optional[str] = None
        executable_state: Optional[Dict[str, object]] = None
        required_fault_markers: List[str] = []
        if args.population_mode == POPULATION_EXECUTABLE:
            try:
                state.executable_post_state = capture_executable_state(
                    client,
                    state,
                    require_accepted_identity=False,
                )
                validate_executable_post_state(state)
            except Exception as exc:
                if run_error is None:
                    run_error = exc

        final_status = "ok"
        if run_error is not None:
            final_status = "error"
        elif not error_budget_pass:
            final_status = "error-budget"
        emit_benchmark_marker(
            client,
            args.logger,
            mode="simulate",
            phase="end",
            run_token=state.run_token,
            status=final_status,
        )

        if args.population_mode == POPULATION_EXECUTABLE and run_error is None:
            try:
                assert state.acceptance_binding is not None
                state.fault_artifacts, required_fault_markers = capture_fault_artifacts(
                    args,
                    state.acceptance_binding,
                )
                target_session_sha256, executable_state = build_executable_report_state(
                    state,
                    required_fault_markers,
                )
            except Exception as exc:
                run_error = exc

        gateway_status_end = fetch_gateway_status_snapshot(client, args.logger, "end")
        gateway_status_diff = gateway_status_delta(
            gateway_status_start,
            gateway_status_end,
        )
        if gateway_status_diff is not None:
            broker_delta = gateway_status_diff.get("broker", {})
            if broker_delta:
                args.logger.log(
                    "[gateway] status_delta "
                    + " ".join(
                        f"{key}={value}" for key, value in sorted(broker_delta.items())
                    )
                )
        latest_population = (
            (
                state.population_observations[0]
                if is_live_executable_population(args.population_mode)
                else state.population_observations[-1]
            )
            if state.population_observations
            else PopulationSnapshot(
                requested=0,
                discovered=0,
                ready=0,
                backend_class="unknown",
                proof_class=(
                    "host-model"
                    if args.population_mode == POPULATION_HOST_MODEL
                    else "none"
                ),
            )
        )
        population_report = {
            "mode": args.population_mode,
            "maximum_live_tasks": state.maximum_live_tasks,
            **latest_population.as_dict(),
            "observations": [
                snapshot.as_dict()
                for snapshot in state.population_observations
            ],
        }
        args.logger.log(
            "[population] "
            f"mode={args.population_mode} "
            f"requested={latest_population.requested} "
            f"discovered={latest_population.discovered} "
            f"ready={latest_population.ready} "
            f"backend_class={latest_population.backend_class} "
            f"proof_class={latest_population.proof_class}"
        )
        artifacts = write_simulation_artifacts(
            args,
            args.logger,
            overall,
            stats,
            ramp_rows,
            state.worker_cap,
            overall_err_rate,
            error_budget_pass,
            gateway_status_start,
            gateway_status_end,
            gateway_status_diff,
            concurrency.snapshot(args.max_inflight),
            population_report,
            target_session_sha256,
            executable_state,
        )
        for label, path in artifacts.items():
            args.logger.log(f"[artifact] {label}={path}")

        if not args.no_cleanup and args.no_qemu:
            for worker_id in list(spawned):
                try:
                    kill_worker(client, state, worker_id)
                except Exception as exc:
                    args.logger.log(f"[cleanup] failed to kill {worker_id}: {exc}")
        elif not args.no_cleanup:
            args.logger.log("[cleanup] skipping per-worker kill; QEMU teardown will reset state")
        if run_error is not None:
            args.logger.log(f"[simulate] failed: {run_error}")
            return 1
        if not error_budget_pass:
            args.logger.log(
                "[simulate] failed: error budget exceeded "
                f"(error_rate={overall_err_rate:.6f} budget={args.error_budget_rate})"
            )
            return 1
        return 0
    finally:
        terminate_process(gateway_proc, "gateway")
        terminate_process(qemu_proc, "qemu")


def pick_worker(rng: random.Random, weights: Dict[str, float]) -> str:
    target = rng.random()
    acc = 0.0
    for worker_id, weight in weights.items():
        acc += weight
        if acc >= target:
            return worker_id
    return next(iter(weights))


def execute_operation(
    client: RestClient,
    op: Operation,
    worker_id: str,
    state: SimState,
    stats: Dict[str, OpStats],
    stats_lock: threading.Lock,
    overall: OpStats,
    semaphore: threading.BoundedSemaphore,
    concurrency: ConcurrencyStats,
) -> None:
    start = time.perf_counter()
    ok = True
    error = None
    try:
        op.func(client, worker_id, state)
    except Exception as exc:  # pragma: no cover - runtime errors
        ok = False
        error = str(exc)
    finally:
        elapsed = time.perf_counter() - start
        try:
            with stats_lock:
                if op.name not in stats:
                    stats[op.name] = OpStats()
                stats[op.name].record(elapsed, ok, error)
                overall.record(elapsed, ok, error)
            if not ok and state.logger is not None:
                state.logger.log(f"[op] {op.name} worker={worker_id} error={error}")
        finally:
            concurrency.finish()
            semaphore.release()


def report_stats(overall: OpStats, stats: Dict[str, OpStats], logger: RunLogger) -> None:
    logger.log(
        f"overall ops={overall.count} ok={overall.ok} err={overall.err} "
        f"avg={overall.avg():.3f}s p95={overall.percentile(95):.3f}s"
    )
    for name in sorted(stats):
        entry = stats[name]
        logger.log(
            f"{name}: ops={entry.count} ok={entry.ok} err={entry.err} "
            f"avg={entry.avg():.3f}s p95={entry.percentile(95):.3f}s"
        )


def summarize_errors(errors: Dict[str, int], max_lines: int) -> List[Dict[str, object]]:
    ordered = sorted(errors.items(), key=lambda item: item[1], reverse=True)
    return [
        {"error": message, "count": count}
        for message, count in ordered[:max_lines]
    ]


def latency_summary(entry: OpStats) -> Dict[str, float]:
    return {
        "avg_s": entry.avg(),
        "min_s": entry.min_s,
        "max_s": entry.max_s,
        "p50_s": entry.percentile(50),
        "p90_s": entry.percentile(90),
        "p95_s": entry.percentile(95),
        "p99_s": entry.percentile(99),
    }


def operation_summary(entry: OpStats, max_error_lines: int) -> Dict[str, object]:
    summary: Dict[str, object] = {
        "count": entry.count,
        "ok": entry.ok,
        "err": entry.err,
        "error_rate": error_rate(entry),
        "errors": summarize_errors(entry.errors, max_error_lines),
    }
    summary.update(latency_summary(entry))
    return summary


def error_classification(entry: OpStats) -> Dict[str, object]:
    """Classify recorded failures without changing reliability accounting."""
    recorded_errors = sum(entry.errors.values())
    buffer_full_errors = sum(
        count
        for message, count in entry.errors.items()
        if is_buffer_full_message(message)
    )
    unclassified_errors = max(entry.err - recorded_errors, 0)
    other_errors = max(entry.err - buffer_full_errors, 0)
    return {
        "buffer_full_errors": buffer_full_errors,
        "other_errors": other_errors,
        "unclassified_errors": unclassified_errors,
        "all_errors_buffer_full": (
            None if entry.err == 0 else other_errors == 0
        ),
    }


def retained_state_summary(stats: Dict[str, OpStats]) -> Dict[str, object]:
    """Project stateful control results used to identify bounded refusals."""
    operations: Dict[str, Dict[str, object]] = {}
    total_count = 0
    total_ok = 0
    total_err = 0
    total_buffer_full = 0
    total_unclassified = 0

    for name in RETAINED_STATE_OPERATION_NAMES:
        entry = stats.get(name, OpStats())
        classification = error_classification(entry)
        operations[name] = {
            "count": entry.count,
            "ok": entry.ok,
            "err": entry.err,
            "error_rate": error_rate(entry),
            **classification,
        }
        total_count += entry.count
        total_ok += entry.ok
        total_err += entry.err
        total_buffer_full += int(classification["buffer_full_errors"])
        total_unclassified += int(classification["unclassified_errors"])

    total_other = max(total_err - total_buffer_full, 0)
    return {
        "operation_names": list(RETAINED_STATE_OPERATION_NAMES),
        "operations_attempted": total_count > 0,
        "count": total_count,
        "ok": total_ok,
        "err": total_err,
        "error_rate": 0.0 if total_count == 0 else total_err / total_count,
        "buffer_full_errors": total_buffer_full,
        "other_errors": total_other,
        "unclassified_errors": total_unclassified,
        "bounded_refusal_observed": total_buffer_full > 0,
        "all_errors_buffer_full": (
            None if total_err == 0 else total_other == 0
        ),
        "operations": operations,
    }


def ramp_row_projection(row: Dict[str, object]) -> Dict[str, object]:
    """Return the bounded ramp fields needed to review a reliability boundary."""
    projection = {
        field: row[field] for field in RAMP_BOUNDARY_FIELDS if field in row
    }
    projection["exact_err_rate"] = ramp_error_rate(row)
    return projection


def ramp_error_rate(row: Dict[str, object]) -> float:
    """Return an exact interval error rate, preferring integer count fields."""
    ops = row.get("ops")
    errors = row.get("err")
    if is_json_number(ops) and is_json_number(errors) and float(ops) > 0:
        return float(errors) / float(ops)
    stored_rate = row.get("err_rate")
    return float(stored_rate) if is_json_number(stored_rate) else 0.0


def capacity_boundary_summary(
    args: argparse.Namespace,
    ramp_rows: List[Dict[str, object]],
    worker_cap: Optional[int],
) -> Dict[str, object]:
    """Describe the observed endpoint and first interval reliability failures."""
    observed_workers_max = max(
        (
            int(row["workers"])
            for row in ramp_rows
            if is_json_number(row.get("workers"))
        ),
        default=0,
    )
    effective_workers_max = (
        args.workers_max
        if worker_cap is None
        else min(args.workers_max, worker_cap)
    )
    first_error = next(
        (
            ramp_row_projection(row)
            for row in ramp_rows
            if is_json_number(row.get("err")) and float(row["err"]) > 0
        ),
        None,
    )
    first_budget_crossing = None
    if args.error_budget_rate is not None:
        first_budget_crossing = next(
            (
                ramp_row_projection(row)
                for row in ramp_rows
                if ramp_error_rate(row) > args.error_budget_rate
            ),
            None,
        )

    return {
        "ramp_steps": len(ramp_rows),
        "worker_shape": (
            "fixed" if args.workers_min == args.workers_max else "ramped"
        ),
        "intensity_shape": (
            "fixed"
            if args.intensity_min == args.intensity_max
            else "ramped"
        ),
        "configured_workers_max": args.workers_max,
        "effective_workers_max": effective_workers_max,
        "observed_workers_max": observed_workers_max,
        "worker_cap_limited": effective_workers_max < args.workers_max,
        "configured_endpoint_observed": observed_workers_max >= args.workers_max,
        "effective_endpoint_observed": observed_workers_max >= effective_workers_max,
        "first_error": first_error,
        "first_error_budget_crossing": first_budget_crossing,
    }


def numeric_broker_delta(
    gateway_status_diff: Optional[Dict[str, Dict[str, object]]],
    key: str,
) -> float:
    if gateway_status_diff is None:
        return 0.0
    broker = gateway_status_diff.get("broker", {})
    value = broker.get(key, 0)
    if is_json_number(value):
        return float(value)
    return 0.0


def benchmark_backpressure_summary(
    gateway_status_diff: Optional[Dict[str, Dict[str, object]]],
) -> Dict[str, object]:
    cache_hits = numeric_broker_delta(gateway_status_diff, "proc_cache_hits")
    cache_misses = numeric_broker_delta(gateway_status_diff, "proc_cache_misses")
    cache_total = cache_hits + cache_misses
    return {
        "source": "gateway_status_delta",
        "control_waiters": int(
            numeric_broker_delta(gateway_status_diff, "control_waiters")
        ),
        "telemetry_waiters": int(
            numeric_broker_delta(gateway_status_diff, "telemetry_waiters")
        ),
        "control_waiters_high_water": int(
            numeric_broker_delta(
                gateway_status_diff,
                "control_waiters_high_water",
            )
        ),
        "telemetry_waiters_high_water": int(
            numeric_broker_delta(
                gateway_status_diff,
                "telemetry_waiters_high_water",
            )
        ),
        "control_checkouts": int(
            numeric_broker_delta(gateway_status_diff, "control_checkouts")
        ),
        "telemetry_checkouts": int(
            numeric_broker_delta(gateway_status_diff, "telemetry_checkouts")
        ),
        "pool_exhausted": int(numeric_broker_delta(gateway_status_diff, "pool_exhausted")),
        "checkout_retries": int(
            numeric_broker_delta(gateway_status_diff, "checkout_retries")
        ),
        "timeout_rejections": int(
            numeric_broker_delta(gateway_status_diff, "timeout_rejections")
        ),
        "control_write_retryable_errors": int(
            numeric_broker_delta(gateway_status_diff, "control_write_retryable_errors")
        ),
        "control_write_retries": int(
            numeric_broker_delta(gateway_status_diff, "control_write_retries")
        ),
        "control_write_retry_exhaustions": int(
            numeric_broker_delta(gateway_status_diff, "control_write_retry_exhaustions")
        ),
        "control_write_success_after_retry": int(
            numeric_broker_delta(gateway_status_diff, "control_write_success_after_retry")
        ),
        "control_write_retry_sleep_ms": int(
            numeric_broker_delta(gateway_status_diff, "control_write_retry_sleep_ms")
        ),
        "proc_cache_hits": int(cache_hits),
        "proc_cache_misses": int(cache_misses),
        "proc_cache_hit_rate": 0.0 if cache_total <= 0 else cache_hits / cache_total,
    }


def top_operations_by(
    stats: Dict[str, OpStats],
    metric: str,
    limit: int = 10,
) -> List[Dict[str, object]]:
    rows = []
    for name, entry in stats.items():
        if metric == "p95_s":
            value = entry.percentile(95)
        elif metric == "error_rate":
            value = error_rate(entry)
        elif metric == "count":
            value = float(entry.count)
        else:
            raise ValueError(f"unsupported operation metric: {metric}")
        rows.append(
            {
                "operation": name,
                "metric": metric,
                "value": value,
                "count": entry.count,
                "ok": entry.ok,
                "err": entry.err,
            }
        )
    rows.sort(key=lambda row: (float(row["value"]), int(row["count"])), reverse=True)
    return rows[:limit]


def target_rps_bounds(args: argparse.Namespace) -> Tuple[float, float]:
    min_rps = args.base_rps * args.intensity_min * max(args.workers_min, 1)
    max_rps = args.base_rps * args.intensity_max * max(args.workers_max, 1)
    scenario = resolve_telemetry_scenario(
        args.scenario,
        args.telemetry_reference_chunk_bytes,
    )
    if scenario is not None and scenario.requests_per_operation > 0:
        min_rps /= scenario.requests_per_operation
        max_rps /= scenario.requests_per_operation
    return min_rps, max_rps


def benchmark_report_payload(
    args: argparse.Namespace,
    overall: OpStats,
    stats: Dict[str, OpStats],
    ramp_rows: List[Dict[str, object]],
    worker_cap: Optional[int],
    overall_error_rate: float,
    error_budget_pass: bool,
    gateway_status_diff: Optional[Dict[str, Dict[str, object]]],
    concurrency: Dict[str, object],
    population: Optional[Dict[str, object]] = None,
    executable_state: Optional[Dict[str, object]] = None,
) -> Dict[str, object]:
    duration_s = max(float(args.duration_mins) * 60.0, 1e-9)
    population_mode = getattr(args, "population_mode", POPULATION_HOST_MODEL)
    target_rps_min, target_rps_max = target_rps_bounds(args)
    latency = latency_summary(overall)
    payload: Dict[str, object] = {
        "schema": "cohesix-benchmark-report/v1",
        "workload": {
            "mode": "simulate",
            "population_mode": population_mode,
            "control_write_outcome": "admitted",
            "scenario": args.scenario or "mixed",
            "seed": args.seed,
            "entropy": args.entropy,
            "workers_min": args.workers_min,
            "workers_max": args.workers_max,
            "worker_cap": worker_cap,
            "multi_hive": args.multi_hive,
            "hives": args.hives if args.multi_hive else 1,
            "workers_per_hive": (
                args.workers_per_hive if args.multi_hive else args.workers_max
            ),
            "intensity_min": args.intensity_min,
            "intensity_max": args.intensity_max,
            "base_rps": args.base_rps,
            "target_rps_min": target_rps_min,
            "target_rps_max": target_rps_max,
            "duration_s": duration_s,
            "ramp_step_secs": args.ramp_step_secs,
            "max_inflight_configured": args.max_inflight,
            "tail_bytes": args.tail_bytes,
            "telemetry_reference_chunk_bytes": (
                args.telemetry_reference_chunk_bytes
            ),
            "include_lifecycle": args.include_lifecycle,
            "auto_approve": args.auto_approve,
            "transient_retries": args.transient_retries,
            "strict_control_errors": args.strict_control_errors,
            "request_timeout_s": args.timeout,
            "request_auth_enabled": bool(args.request_auth_token.strip()),
            "role": args.role,
        },
        "throughput": {
            "ops_per_s": overall.count / duration_s,
            "ok_ops_per_s": overall.ok / duration_s,
            "err_ops_per_s": overall.err / duration_s,
        },
        "latency": latency,
        "reliability": {
            "error_rate": overall_error_rate,
            "error_budget_rate": args.error_budget_rate,
            "error_budget_pass": error_budget_pass,
            "ok": overall.ok,
            "err": overall.err,
            "count": overall.count,
            **error_classification(overall),
        },
        "capacity_boundary": capacity_boundary_summary(
            args,
            ramp_rows,
            worker_cap,
        ),
        "retained_state": retained_state_summary(stats),
        "concurrency": concurrency,
        "backpressure": benchmark_backpressure_summary(gateway_status_diff),
        "population": population or {
            "mode": population_mode,
            "requested": 0,
            "discovered": 0,
            "ready": 0,
            "backend_class": "unknown",
            "proof_class": "host-model" if population_mode == POPULATION_HOST_MODEL else "none",
        },
        "top_operations_by_p95": top_operations_by(stats, "p95_s"),
        "top_operations_by_error_rate": top_operations_by(stats, "error_rate"),
        "visualization": {
            "primary_series": [
                "ramp.workers",
                "ramp.throughput_ops_s",
                "ramp.err_rate",
                "ramp.cumulative_p95_s",
                "ramp.max_inflight_observed",
                "report.backpressure.control_write_retryable_errors",
            ],
            "recommended_charts": [
                "workers_vs_error_rate",
                "target_rps_vs_observed_throughput",
                "latency_quantiles_by_operation",
                "gateway_backpressure_delta",
                "inflight_high_water_by_ramp_step",
            ],
        },
    }
    if executable_state is not None:
        payload["executable_state"] = executable_state
    return payload


def write_ramp_svg(ramp_rows: List[Dict[str, object]], svg_path: str) -> None:
    if not ramp_rows:
        return
    width = 900
    height = 320
    padding = 40
    x_span = max(len(ramp_rows) - 1, 1)
    max_workers = max(int(row.get("workers", 0)) for row in ramp_rows)
    max_err_rate = max(float(row.get("err_rate", 0.0)) for row in ramp_rows)
    if max_workers <= 0:
        max_workers = 1
    if max_err_rate <= 0:
        max_err_rate = 1.0

    worker_points: List[str] = []
    err_points: List[str] = []
    for index, row in enumerate(ramp_rows):
        x = padding + ((width - 2 * padding) * index / x_span)
        workers = float(row.get("workers", 0.0))
        err_rate = float(row.get("err_rate", 0.0))
        y_workers = height - padding - ((height - 2 * padding) * workers / max_workers)
        y_err = height - padding - ((height - 2 * padding) * err_rate / max_err_rate)
        worker_points.append(f"{x:.1f},{y_workers:.1f}")
        err_points.append(f"{x:.1f},{y_err:.1f}")

    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">
  <rect x="0" y="0" width="{width}" height="{height}" fill="#ffffff"/>
  <line x1="{padding}" y1="{height - padding}" x2="{width - padding}" y2="{height - padding}" stroke="#333" stroke-width="1"/>
  <line x1="{padding}" y1="{padding}" x2="{padding}" y2="{height - padding}" stroke="#333" stroke-width="1"/>
  <polyline fill="none" stroke="#1f77b4" stroke-width="2" points="{' '.join(worker_points)}"/>
  <polyline fill="none" stroke="#d62728" stroke-width="2" points="{' '.join(err_points)}"/>
  <text x="{padding}" y="20" font-size="14" fill="#111">Ramp workers (blue) vs error rate (red)</text>
</svg>
"""
    with open(svg_path, "w", encoding="utf-8") as handle:
        handle.write(svg)


def write_simulation_artifacts(
    args: argparse.Namespace,
    logger: RunLogger,
    overall: OpStats,
    stats: Dict[str, OpStats],
    ramp_rows: List[Dict[str, object]],
    worker_cap: Optional[int],
    overall_error_rate: float,
    error_budget_pass: bool,
    gateway_status_start: Optional[Dict[str, object]] = None,
    gateway_status_end: Optional[Dict[str, object]] = None,
    gateway_status_diff: Optional[Dict[str, Dict[str, object]]] = None,
    concurrency: Optional[Dict[str, object]] = None,
    population: Optional[Dict[str, object]] = None,
    target_session_sha256: Optional[str] = None,
    executable_state: Optional[Dict[str, object]] = None,
) -> Dict[str, str]:
    base_path = logger.path.rsplit(".", 1)[0]
    summary_json = f"{base_path}.summary.json"
    ops_csv = f"{base_path}.ops.csv"
    ramp_csv = f"{base_path}.ramp.csv"
    ramp_svg = f"{base_path}.ramp.svg"
    concurrency_summary = concurrency or {
        "configured_max_inflight": args.max_inflight,
        "observed_high_water": 0,
        "current_inflight": 0,
        "submitted": 0,
        "completed": 0,
    }
    population_mode = getattr(args, "population_mode", POPULATION_HOST_MODEL)

    summary_payload = {
        "mode": "simulate",
        "population_mode": population_mode,
        "control_write_outcome": "admitted",
        "seed": args.seed,
        "rest_url": args.rest_url,
        "workers_min": args.workers_min,
        "workers_max": args.workers_max,
        "multi_hive": args.multi_hive,
        "hives": args.hives if args.multi_hive else 1,
        "workers_per_hive": args.workers_per_hive if args.multi_hive else args.workers_max,
        "federated_worker_target": (
            args.hives * args.workers_per_hive if args.multi_hive else args.workers_max
        ),
        "worker_cap": worker_cap,
        "intensity_min": args.intensity_min,
        "intensity_max": args.intensity_max,
        "duration_mins": args.duration_mins,
        "transient_retries": args.transient_retries,
        "no_retries": not args.transient_retries,
        "strict_control_errors": args.strict_control_errors,
        "fast_ramp": args.fast_ramp,
        "scenario": args.scenario,
        "telemetry_reference_chunk_bytes": args.telemetry_reference_chunk_bytes,
        "error_budget_rate": args.error_budget_rate,
        "error_rate": overall_error_rate,
        "error_budget_pass": error_budget_pass,
        "gateway_pool_control_sessions": args.gateway_pool_control_sessions,
        "gateway_pool_telemetry_sessions": args.gateway_pool_telemetry_sessions,
        "gateway_broker_control_response_timeout_ms": (
            args.gateway_broker_control_response_timeout_ms
        ),
        "gateway_broker_telemetry_response_timeout_ms": (
            args.gateway_broker_telemetry_response_timeout_ms
        ),
        "gateway_control_write_retry_window_ms": (
            args.gateway_control_write_retry_window_ms
        ),
        "gateway_status_start": gateway_status_start,
        "gateway_status_end": gateway_status_end,
        "gateway_status_delta": gateway_status_diff,
        "concurrency": concurrency_summary,
        "population": population,
        "report": benchmark_report_payload(
            args,
            overall,
            stats,
            ramp_rows,
            worker_cap,
            overall_error_rate,
            error_budget_pass,
            gateway_status_diff,
            concurrency_summary,
            population,
            executable_state,
        ),
        "overall": operation_summary(overall, args.summary_max_error_lines),
        "operations": {
            name: operation_summary(entry, args.summary_max_error_lines)
            for name, entry in sorted(stats.items())
        },
        "ramp": ramp_rows,
    }
    if target_session_sha256 is not None:
        summary_payload["target_session_sha256"] = target_session_sha256
    with open(summary_json, "w", encoding="utf-8") as handle:
        json.dump(summary_payload, handle, indent=2, sort_keys=True)

    with open(ops_csv, "w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(
            [
                "operation",
                "count",
                "ok",
                "err",
                "error_rate",
                "avg_s",
                "p50_s",
                "p90_s",
                "p95_s",
                "p99_s",
                "min_s",
                "max_s",
            ]
        )
        for name, entry in sorted(stats.items()):
            writer.writerow(
                [
                    name,
                    entry.count,
                    entry.ok,
                    entry.err,
                    f"{error_rate(entry):.6f}",
                    f"{entry.avg():.6f}",
                    f"{entry.percentile(50):.6f}",
                    f"{entry.percentile(90):.6f}",
                    f"{entry.percentile(95):.6f}",
                    f"{entry.percentile(99):.6f}",
                    f"{entry.min_s:.6f}",
                    f"{entry.max_s:.6f}",
                ]
            )

    with open(ramp_csv, "w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=[
                "step",
                "workers",
                "intensity",
                "rps",
                "ops",
                "ok",
                "err",
                "err_rate",
                "throughput_ops_s",
                "ok_ops_s",
                "max_inflight_observed",
                "max_inflight_configured",
                "cumulative_avg_s",
                "cumulative_p95_s",
                "cumulative_p99_s",
            ],
        )
        writer.writeheader()
        for row in ramp_rows:
            writer.writerow(row)

    write_ramp_svg(ramp_rows, ramp_svg)

    return {
        "summary_json": summary_json,
        "ops_csv": ops_csv,
        "ramp_csv": ramp_csv,
        "ramp_svg": ramp_svg,
    }


def run_perf(args: argparse.Namespace) -> int:
    client = RestClient(args.rest_url, args.timeout, args.request_auth_token)
    bounds_url = f"{normalize_rest_url(args.rest_url)}/v1/meta/bounds"
    perf_summary: Dict[str, Dict[str, object]] = {}
    run_token = hashlib.sha256(
        f"perf-{os.getpid()}-{time.time_ns()}".encode("ascii")
    ).hexdigest()[:8]
    try:
        bounds = fetch_json(bounds_url, args.timeout, client.request_auth_headers())
    except Exception as exc:  # pragma: no cover - CLI error reporting
        args.logger.log(f"Failed to fetch bounds: {exc}")
        return 1
    perf_population: Dict[str, object]
    executable_workers: Optional[List[WorkerInstance]] = None
    if is_live_executable_population(args.population_mode):
        try:
            runtime = worker_runtime_bounds(bounds)
            requested = min(args.max_workers, int(runtime["maximum_live_tasks"]))
            executable_workers, population = executable_population_snapshot(
                client,
                bounds,
                requested,
                args.population_mode,
            )
        except Exception as exc:
            args.logger.log(f"Executable Worker discovery failed: {exc}")
            return 1
        perf_population = {
            "mode": args.population_mode,
            "maximum_live_tasks": int(runtime["maximum_live_tasks"]),
            **population.as_dict(),
        }
    else:
        backend_class, proof_class = gateway_observation_axes(client)
        perf_population = {
            "mode": POPULATION_HOST_MODEL,
            "maximum_live_tasks": None,
            "requested": args.max_workers,
            "discovered": 0,
            "ready": 0,
            "backend_class": backend_class,
            "proof_class": proof_class,
        }
    emit_benchmark_marker(
        client,
        args.logger,
        mode="perf",
        phase="start",
        run_token=run_token,
        status="running",
    )
    status_specs = build_status_specs(bounds)
    gateway_status_start = fetch_gateway_status_snapshot(
        client,
        args.logger,
        "start",
    )
    if args.suite in ("status", "all"):
        seq_times, par_times = measure(status_specs, client, args.runs)
        report("status", seq_times, par_times, args.assert_min_ratio, args.logger)
        args.logger.log("status suite complete")
        seq_avg = average(seq_times)
        par_avg = average(par_times)
        perf_summary["status"] = {
            "sequential_s": seq_times,
            "parallel_s": par_times,
            "avg_sequential_s": seq_avg,
            "avg_parallel_s": par_avg,
            "speedup": seq_avg / par_avg if par_avg > 0 else 0.0,
        }

    if args.suite in ("telemetry", "all"):
        telemetry_paths: Optional[Dict[str, str]] = None
        if executable_workers is not None:
            workers = [instance.worker_id for instance in executable_workers]
            telemetry_paths = {
                instance.worker_id: instance.telemetry_path
                for instance in executable_workers
            }
        else:
            try:
                workers = list_workers(client)
            except Exception as exc:  # pragma: no cover - CLI error reporting
                args.logger.log(f"Failed to list workers: {exc}")
                emit_benchmark_marker(
                    client,
                    args.logger,
                    mode="perf",
                    phase="end",
                    run_token=run_token,
                    status="error",
                )
                return 1
            perf_population["discovered"] = len(workers)
        if not workers:
            args.logger.log("telemetry: no workers found; skipping telemetry suite.")
            emit_benchmark_marker(
                client,
                args.logger,
                mode="perf",
                phase="end",
                run_token=run_token,
                status="skipped",
            )
            return 0
        telemetry_specs = build_telemetry_specs(
            workers,
            args.max_workers,
            args.tail_bytes,
            telemetry_paths,
        )
        if not telemetry_specs:
            args.logger.log("telemetry: no telemetry specs; skipping telemetry suite.")
            emit_benchmark_marker(
                client,
                args.logger,
                mode="perf",
                phase="end",
                run_token=run_token,
                status="skipped",
            )
            return 0
        seq_times, par_times = measure(telemetry_specs, client, args.runs)
        report(
            "telemetry",
            seq_times,
            par_times,
            args.assert_min_ratio,
            args.logger,
        )
        args.logger.log("telemetry suite complete")
        seq_avg = average(seq_times)
        par_avg = average(par_times)
        perf_summary["telemetry"] = {
            "sequential_s": seq_times,
            "parallel_s": par_times,
            "avg_sequential_s": seq_avg,
            "avg_parallel_s": par_avg,
            "speedup": seq_avg / par_avg if par_avg > 0 else 0.0,
        }

    if perf_summary:
        gateway_status_end = fetch_gateway_status_snapshot(client, args.logger, "end")
        gateway_status_diff = gateway_status_delta(
            gateway_status_start,
            gateway_status_end,
        )
        if gateway_status_diff is not None:
            broker_delta = gateway_status_diff.get("broker", {})
            if broker_delta:
                args.logger.log(
                    "[gateway] status_delta "
                    + " ".join(
                        f"{key}={value}" for key, value in sorted(broker_delta.items())
                    )
                )
        base_path = args.logger.path.rsplit(".", 1)[0]
        summary_path = f"{base_path}.perf-summary.json"
        with open(summary_path, "w", encoding="utf-8") as handle:
            json.dump(
                {
                    "mode": "perf",
                    "population_mode": args.population_mode,
                    "population": perf_population,
                    "rest_url": args.rest_url,
                    "runs": args.runs,
                    "suite": args.suite,
                    "gateway_status_start": gateway_status_start,
                    "gateway_status_end": gateway_status_end,
                    "gateway_status_delta": gateway_status_diff,
                    "results": perf_summary,
                },
                handle,
                indent=2,
                sort_keys=True,
            )
        args.logger.log(f"[artifact] perf_summary_json={summary_path}")

    emit_benchmark_marker(
        client,
        args.logger,
        mode="perf",
        phase="end",
        run_token=run_token,
        status="ok",
    )
    return 0


def main() -> int:
    """Entry point."""
    args = parse_args()
    args.logger = init_logger(args)
    try:
        if args.mode == "simulate":
            return run_simulation(args)
        return run_perf(args)
    except Exception:  # pragma: no cover - top-level logging
        args.logger.log("Unhandled exception:")
        args.logger.log(traceback.format_exc())
        raise
    finally:
        args.logger.log("[log] finished")
        args.logger.close()


if __name__ == "__main__":
    raise SystemExit(main())
