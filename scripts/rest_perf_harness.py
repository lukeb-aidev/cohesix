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
import concurrent.futures
import json
import os
import random
import socket
import subprocess
import sys
import threading
import time
import traceback
from datetime import datetime, timezone
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from typing import Callable, Dict, Iterable, List, Optional, Sequence, Tuple, TextIO

DEFAULT_REST_URL = "http://127.0.0.1:8080"
DEFAULT_RUNS = 3
DEFAULT_TIMEOUT_SECS = 3.0
DEFAULT_MAX_WORKERS = 4
DEFAULT_TAIL_BYTES = 256
DEFAULT_LOG_TAIL_BYTES = 32768
DEFAULT_QEMU_SMP = "4,cores=4,threads=1,sockets=1"
DEFAULT_WORKERS_MIN = 5
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
DEFAULT_AUTH_TOKEN = "changeme"
DEFAULT_ROLE = "queen"


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


class RestError(RuntimeError):
    """REST request failed with a gateway error or HTTP exception."""

    def __init__(self, message: str, response: Optional[GatewayResponse] = None):
        super().__init__(message)
        self.response = response


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
    message = str(error).lower()
    if "http 503" in message or "http 502" in message or "service unavailable" in message:
        return True
    if "reason=policy" in message and "detail=denied" in message:
        return True
    if "buffer full" in message or "buffer-full" in message:
        return True
    return False


def is_buffer_full_error(error: Exception) -> bool:
    message = str(error).lower()
    return "buffer full" in message or "buffer-full" in message


def retry_transient(
    fn: Callable[[], None],
    timeout_s: float,
    label: str,
    base_sleep: float = 0.5,
) -> None:
    deadline = time.time() + timeout_s
    attempt = 0
    while True:
        try:
            fn()
            return
        except Exception as exc:
            if not is_transient_error(exc):
                raise
            attempt += 1
            if time.time() >= deadline:
                raise RestError(f"{label} failed after {attempt} retries: {exc}") from exc
            time.sleep(base_sleep)


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
    worker_cap: Optional[int] = None
    approval_seq: int = 0
    policy_lock: threading.Lock = field(default_factory=threading.Lock)
    lease_lock: threading.Lock = field(default_factory=threading.Lock)
    active_leases: List[str] = field(default_factory=list)
    policy_current: Optional[str] = None
    policy_previous: Optional[str] = None
    telemetry_segments: Dict[str, str] = field(default_factory=dict)
    telemetry_lock: Optional[threading.Lock] = None
    logger: Optional[RunLogger] = None


@dataclass
class WorkerProfile:
    """Per-worker simulation profile."""

    worker_id: str
    load_factor: float
    op_weights: List[Tuple[Operation, float]]


class RestClient:
    """Minimal REST client for hive-gateway."""

    def __init__(self, rest_url: str, timeout: float):
        self.rest_url = normalize_rest_url(rest_url)
        self.timeout = timeout

    def get_json(self, path: str, params: Optional[Dict[str, str]] = None) -> dict:
        url = self._build_url(path, params)
        try:
            return fetch_json(url, self.timeout)
        except urllib.error.HTTPError as exc:
            raise RestError(f"HTTP {exc.code} {exc.reason} for {url}") from exc
        except urllib.error.URLError as exc:
            raise RestError(f"URL error for {url}: {exc}") from exc

    def post_json(self, path: str, payload: dict) -> dict:
        url = self._build_url(path, None)
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(url, data=data, method="POST")
        request.add_header("Content-Type", "application/json")
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

    def _build_url(self, path: str, params: Optional[Dict[str, str]]) -> str:
        url = f"{self.rest_url}{path}"
        if not params:
            return url
        return f"{url}?{urllib.parse.urlencode(params)}"


def fetch_json(url: str, timeout: float) -> dict:
    """Fetch JSON from a URL and decode the response."""
    with urllib.request.urlopen(url, timeout=timeout) as response:
        payload = response.read()
    return json.loads(payload)


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
    """Fetch worker IDs from the gateway."""
    response = client.ls("/worker")
    if response.status != "OK":
        raise RestError(
            f"LS /worker failed: {response.error}",
            response,
        )
    return [line.strip() for line in response.lines if line.strip()]


def build_telemetry_specs(
    workers: Sequence[str],
    max_workers: int,
    max_bytes: int,
) -> List[RequestSpec]:
    """Build telemetry tail request list for a subset of workers."""
    specs: List[RequestSpec] = []
    for worker_id in workers[:max_workers]:
        specs.append(RequestSpec(f"/worker/{worker_id}/telemetry", max_bytes, "tail"))
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
        help="Max bytes per telemetry tail request.",
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

    args = parser.parse_args()

    if args.mode == "simulate":
        args.auto_approve = not args.no_auto_approve
        args.duration_mins = clamp_int(args.duration_mins, 1, 60, "duration-mins")
        args.workers_min = clamp_int(args.workers_min, 1, 512, "workers-min")
        args.workers_max = clamp_int(args.workers_max, 1, 512, "workers-max")
        if args.workers_min > args.workers_max:
            raise SystemExit("workers-min must be <= workers-max")
        args.intensity_min = clamp_int(args.intensity_min, 1, 10, "intensity-min")
        args.intensity_max = clamp_int(args.intensity_max, 1, 10, "intensity-max")
        if args.intensity_min > args.intensity_max:
            raise SystemExit("intensity-min must be <= intensity-max")
        args.entropy = clamp_float(args.entropy, 0.0, 10.0, "entropy")
        args.base_rps = clamp_float(args.base_rps, 0.1, 1000.0, "base-rps")
        args.max_inflight = clamp_int(args.max_inflight, 1, 4096, "max-inflight")

    return args


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


def wait_for_gateway(client: RestClient, timeout_s: float) -> dict:
    """Wait for the gateway to respond to /v1/meta/bounds."""
    deadline = time.time() + timeout_s
    last_error: Optional[Exception] = None
    while time.time() < deadline:
        try:
            bounds = client.get_json("/v1/meta/bounds")
            root = client.ls("/")
            if root.status == "OK":
                return bounds
            last_error = RestError(
                f"Gateway not ready: LS / returned {root.status}"
            )
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
    )


def terminate_process(proc: Optional[subprocess.Popen], label: str) -> None:
    """Terminate a subprocess if it is running."""
    if not proc:
        return
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        print(f"[{label}] force killing", file=sys.stderr)
        proc.kill()
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
        state.active_leases.append(lease_id)
        if len(state.active_leases) > 128:
            state.active_leases.pop(0)


def choose_lease_id(state: SimState) -> Optional[str]:
    with state.lease_lock:
        if not state.active_leases:
            return None
        return state.rng.choice(state.active_leases)


def remove_lease_id(state: SimState, lease_id: str) -> None:
    with state.lease_lock:
        if lease_id in state.active_leases:
            state.active_leases.remove(lease_id)


def echo_with_policy_retry(
    client: RestClient,
    path: str,
    line: str,
    state: SimState,
) -> None:
    if path == "/queen/ctl" and state.auto_approve:
        with state.policy_lock:
            _echo_with_policy_retry_inner(client, path, line, state)
        return
    _echo_with_policy_retry_inner(client, path, line, state)


def _echo_with_policy_retry_inner(
    client: RestClient,
    path: str,
    line: str,
    state: SimState,
) -> None:
    def attempt() -> None:
        response = client.echo(path, line)
        if response.status == "OK":
            return
        if (
            path == "/queen/ctl"
            and state.auto_approve
            and response.error
        ):
            err_lower = response.error.lower()
            if "policy" in err_lower or "buffer full" in err_lower or "buffer-full" in err_lower:
                queue_approval(client, path, state)
                response_retry = client.echo(path, line)
                if response_retry.status == "OK":
                    return
                response = response_retry
        raise RestError(
            f"ECHO {path} failed: {response.error}",
            response,
        )

    retry_transient(attempt, timeout_s=10.0, label=f"echo {path}")


def spawn_worker(client: RestClient, state: SimState) -> str:
    payload = {
        "spawn": "heartbeat",
        "ticks": state.rng.randint(50, 200),
        "budget": {"ttl_s": 300, "ops": 500},
    }
    before = set(list_workers(client))
    line = json.dumps(payload, separators=(",", ":"))
    retry_transient(
        lambda: echo_with_policy_retry(client, "/queen/ctl", line, state),
        timeout_s=15.0,
        label="spawn worker",
    )
    deadline = time.time() + 10
    while time.time() < deadline:
        time.sleep(0.5)
        after = set(list_workers(client))
        new_ids = list(after - before)
        if new_ids:
            return new_ids[0]
    raise RestError("spawn did not yield a new worker")


def kill_worker(client: RestClient, state: SimState, worker_id: str) -> None:
    line = json.dumps({"kill": worker_id}, separators=(",", ":"))
    echo_with_policy_retry(client, "/queen/ctl", line, state)


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
            response = client.ls(path)
            if response.status != "OK":
                raise RestError(f"LS {path} failed: {response.error}", response)

        return _run

    def op_cat(path: str, max_bytes: int) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, _state: SimState) -> None:
            response = client.cat(path, max_bytes)
            if response.status != "OK":
                raise RestError(f"CAT {path} failed: {response.error}", response)

        return _run

    def op_tail(path_builder: Callable[[str], str], max_bytes: int) -> Callable[
        [RestClient, str, SimState], None
    ]:
        def _run(client: RestClient, worker: str, _state: SimState) -> None:
            path = path_builder(worker)
            response = client.tail(path, max_bytes)
            if response.status != "OK":
                raise RestError(f"TAIL {path} failed: {response.error}", response)

        return _run

    def op_tail_log(path: str, max_bytes: int) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, _state: SimState) -> None:
            attempt_bytes = max_bytes
            for _ in range(3):
                response = client.tail(path, attempt_bytes)
                if response.status == "OK":
                    return
                if response.error and "tail exceeded max_bytes" in response.error:
                    attempt_bytes = min(attempt_bytes * 2, 65536)
                    continue
                raise RestError(f"TAIL {path} failed: {response.error}", response)

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
            try:
                echo_with_policy_retry(client, path, line, sim_state)
            except RestError as exc:
                if is_buffer_full_error(exc):
                    return
                raise

        return _run

    def op_lease_grant(path: str) -> Callable[[RestClient, str, SimState], None]:
        def _run(client: RestClient, _worker: str, sim_state: SimState) -> None:
            lease_id = f"lease-{sim_state.rng.randint(1000, 9999)}"
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
            try:
                echo_with_policy_retry(client, path, line, sim_state)
            except RestError as exc:
                if is_buffer_full_error(exc):
                    return
                raise
            else:
                remember_lease_id(sim_state, lease_id)

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
            echo_with_policy_retry(client, path, line, sim_state)
            remove_lease_id(sim_state, lease_id)

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

    ops.append(Operation("meta_bounds", 0.4, "meta", lambda c, w, s: c.get_json("/v1/meta/bounds")))

    for entry in ("/", "/proc", "/queen", "/worker", "/gpu", "/host"):
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

    if "worker" in root_entries:
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

    if "queen" in root_entries:
        ops.append(
            Operation(
                "schedule_write",
                0.6,
                "control",
                op_echo_best_effort(
                    "/queen/schedule/ctl",
                    lambda _w, st: json.dumps(
                        {
                            "id": f"sched-{st.rng.randint(1000, 9999)}",
                            "role": "worker-gpu",
                            "priority": st.rng.randint(1, 5),
                            "ticks": st.rng.randint(1, 5),
                            "budget_ms": st.rng.randint(50, 200),
                        },
                        separators=(",", ":"),
                    ),
                ),
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
    segment = ensure_telemetry_segment(client, state, device_id)
    payload = f"telemetry seq={state.rng.randint(1, 100000)}"
    path = f"/queen/telemetry/{device_id}/seg/{segment}"
    def attempt() -> None:
        response = client.echo(path, payload)
        if response.status != "OK":
            raise RestError(f"ECHO {path} failed: {response.error}", response)

    retry_transient(attempt, timeout_s=10.0, label=f"telemetry append {device_id}")


def telemetry_segment_op(client: RestClient, _worker: str, state: SimState) -> None:
    retry_transient(
        lambda: ensure_telemetry_segment(client, state, "bench"),
        timeout_s=10.0,
        label="telemetry segment",
    )


def ensure_telemetry_segment(
    client: RestClient, state: SimState, device_id: str
) -> str:
    if state.telemetry_lock is None:
        state.telemetry_lock = threading.Lock()
    with state.telemetry_lock:
        existing = state.telemetry_segments.get(device_id)
        if existing:
            return existing
        line = json.dumps(
            {"new": "segment", "mime": "text/plain"}, separators=(",", ":")
        )
        echo_with_policy_retry(
            client, f"/queen/telemetry/{device_id}/ctl", line, state
        )
        response = client.cat(f"/queen/telemetry/{device_id}/latest", 64)
        if response.status != "OK" or not response.lines:
            raise RestError(
                f"Failed to read latest segment for {device_id}: {response.error}",
                response,
            )
        segment = response.lines[0].strip()
        if not segment:
            raise RestError(f"Empty segment id for {device_id}")
        state.telemetry_segments[device_id] = segment
        return segment


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
    current = list_workers(client)
    spawned: List[str] = []
    while len(current) < target:
        try:
            new_worker = spawn_worker(client, state)
        except RestError as exc:
            if "buffer full" in str(exc).lower():
                if state.worker_cap is None or len(current) < state.worker_cap:
                    state.worker_cap = len(current)
                if state.logger:
                    state.logger.log(
                        f"worker capacity reached at {len(current)} (buffer full)"
                    )
                break
            raise
        else:
            spawned.append(new_worker)
            current.append(new_worker)
    return current, spawned


def adjust_workers(
    client: RestClient,
    state: SimState,
    worker_ids: List[str],
    spawned: List[str],
    target: int,
) -> Tuple[List[str], List[str]]:
    current = list(worker_ids)
    if len(current) < target:
        while len(current) < target:
            try:
                new_worker = spawn_worker(client, state)
            except RestError as exc:
                if "buffer full" in str(exc).lower():
                    if state.worker_cap is None or len(current) < state.worker_cap:
                        state.worker_cap = len(current)
                    if state.logger:
                        state.logger.log(
                            f"worker capacity reached at {len(current)} (buffer full)"
                        )
                    break
                raise
            else:
                spawned.append(new_worker)
                current.append(new_worker)
    elif len(current) > target:
        while len(current) > target and spawned:
            victim = spawned.pop()
            kill_worker(client, state, victim)
            if victim in current:
                current.remove(victim)
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
            if not qemu_run:
                raise SystemExit("QEMU launch requested but no run script found.")
            env = os.environ.copy()
            env["COHESIX_QEMU_SMP_TOPO"] = args.qemu_smp
            qemu_proc = launch_process([qemu_run], env, args.qemu_log)
            wait_for_port(args.tcp_host, args.tcp_port, 120)

        if not args.no_gateway:
            if not gateway_bin:
                raise SystemExit("Gateway launch requested but no binary found.")
            env = os.environ.copy()
            env["COH_TCP_HOST"] = args.tcp_host
            env["COH_TCP_PORT"] = str(args.tcp_port)
            env["COH_AUTH_TOKEN"] = args.auth_token
            env["COH_ROLE"] = args.role
            env["HIVE_GATEWAY_BIND"] = args.gateway_bind
            gateway_proc = launch_process(
                [gateway_bin, "--bind", args.gateway_bind],
                env,
                args.gateway_log,
            )

        client = RestClient(rest_url, args.timeout)
        bounds = wait_for_gateway(client, 60)

        root_entries = discover_root_entries(client)
        host_paths = discover_host_paths(client)
        gpu_paths = discover_gpu_paths(client)
        policy_on = policy_enabled(client)
        actions_on = actions_enabled(client)
        telemetry_on = telemetry_ingest_enabled(client)

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
            logger=args.logger,
        )

        worker_ids, spawned = ensure_workers(client, state, args.workers_min)

        operations = build_operations(
            bounds,
            root_entries,
            host_paths,
            gpu_paths,
            state,
        )
        if not operations:
            raise SystemExit("No operations available to run.")

        stats: Dict[str, OpStats] = {}
        stats_lock = threading.Lock()
        overall = OpStats()

        duration_s = args.duration_mins * 60
        ramp_step = max(1, args.ramp_step_secs)
        end_time = time.time() + duration_s

        args.logger.log(
            f"[simulate] duration={args.duration_mins}m "
            f"workers={args.workers_min}-{args.workers_max} "
            f"intensity={args.intensity_min}-{args.intensity_max} "
            f"rest={rest_url}"
        )

        executor = concurrent.futures.ThreadPoolExecutor(
            max_workers=args.max_inflight
        )
        semaphore = threading.BoundedSemaphore(args.max_inflight)

        try:
            while time.time() < end_time:
                now = time.time()
                progress = min(1.0, max(0.0, 1.0 - (end_time - now) / duration_s))
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
                if rps <= 0:
                    time.sleep(step_end - time.time())
                    continue
                interval = 1.0 / rps

                args.logger.log(
                    f"[simulate] workers={len(worker_ids)} "
                    f"intensity={target_intensity:.1f} rps={rps:.1f}"
                )

                while time.time() < step_end:
                    worker_id = pick_worker(rng, load_weights)
                    profile = worker_lookup[worker_id]
                    op = pick_weighted(rng, profile.op_weights)
                    semaphore.acquire()
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
                    )
                    time.sleep(interval)
        finally:
            executor.shutdown(wait=True)

        args.logger.log("[simulate] summary")
        report_stats(overall, stats, args.logger)

        if not args.no_cleanup:
            for worker_id in list(spawned):
                try:
                    kill_worker(client, state, worker_id)
                except RestError as exc:
                    args.logger.log(f"[cleanup] failed to kill {worker_id}: {exc}")

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
) -> None:
    start = time.perf_counter()
    ok = True
    error = None
    try:
        op.func(client, worker_id, state)
    except Exception as exc:  # pragma: no cover - runtime errors
        ok = False
        error = str(exc)
    elapsed = time.perf_counter() - start
    with stats_lock:
        if op.name not in stats:
            stats[op.name] = OpStats()
        stats[op.name].record(elapsed, ok, error)
        overall.record(elapsed, ok, error)
    if not ok and state.logger is not None:
        state.logger.log(f"[op] {op.name} worker={worker_id} error={error}")
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


def run_perf(args: argparse.Namespace) -> int:
    client = RestClient(args.rest_url, args.timeout)
    bounds_url = f"{normalize_rest_url(args.rest_url)}/v1/meta/bounds"
    try:
        bounds = fetch_json(bounds_url, args.timeout)
    except Exception as exc:  # pragma: no cover - CLI error reporting
        args.logger.log(f"Failed to fetch bounds: {exc}")
        return 1
    status_specs = build_status_specs(bounds)
    if args.suite in ("status", "all"):
        seq_times, par_times = measure(status_specs, client, args.runs)
        report("status", seq_times, par_times, args.assert_min_ratio, args.logger)
        args.logger.log("status suite complete")

    if args.suite in ("telemetry", "all"):
        try:
            workers = list_workers(client)
        except Exception as exc:  # pragma: no cover - CLI error reporting
            args.logger.log(f"Failed to list workers: {exc}")
            return 1
        if not workers:
            args.logger.log("telemetry: no workers found; skipping telemetry suite.")
            return 0
        telemetry_specs = build_telemetry_specs(
            workers, args.max_workers, args.tail_bytes
        )
        if not telemetry_specs:
            args.logger.log("telemetry: no telemetry specs; skipping telemetry suite.")
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
