#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Host-side watchdog to detect queen/gateway failures and auto-cutover the live mount.
# Copyright 2026 Lukas Bower

"""Cohesix failover watchdog for active/standby mount cutover.

This is ops automation for 0.9.0-beta active/standby deployments. It does not
introduce in-VM leader election or self-promotion. The watchdog probes both
REST gateways, decides whether the active side is failed, and atomically flips
the live symlink to the healthy standby mount when required.
"""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from typing import Any


@dataclass
class HealthState:
    """Tracks consecutive probe outcomes for threshold-based decisions."""

    consecutive_failures: int = 0
    consecutive_successes: int = 0
    last_error: str = ""

    def observe(self, ok: bool, error: str) -> None:
        """Record the latest probe outcome."""
        if ok:
            self.consecutive_successes += 1
            self.consecutive_failures = 0
            self.last_error = ""
        else:
            self.consecutive_failures += 1
            self.consecutive_successes = 0
            self.last_error = error

    def failed(self, threshold: int) -> bool:
        """Return True once failures meet or exceed threshold."""
        return self.consecutive_failures >= threshold

    def healthy(self, threshold: int) -> bool:
        """Return True once successes meet or exceed threshold."""
        return self.consecutive_successes >= threshold


@dataclass
class Endpoint:
    """Represents one failover side (A or B)."""

    name: str
    rest_url: str
    mount_path: pathlib.Path
    health: HealthState = field(default_factory=HealthState)


@dataclass
class ProbeResult:
    """Result of a REST health probe."""

    ok: bool
    reason: str


def normalize_rest_url(url: str) -> str:
    """Normalize REST URL by trimming trailing slashes."""
    return url.rstrip("/")


def now_utc_iso() -> str:
    """Return UTC timestamp formatted for logs."""
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def emit(event: str, **fields: Any) -> None:
    """Emit a single line JSON event to stdout."""
    payload: dict[str, Any] = {"ts": now_utc_iso(), "event": event}
    payload.update(fields)
    print(json.dumps(payload, sort_keys=True), flush=True)


def auth_headers(token: str | None) -> dict[str, str]:
    """Build standard auth headers for gateway requests."""
    if token:
        return {"Authorization": f"Bearer {token}"}
    return {}


def http_json(
    method: str,
    url: str,
    token: str | None,
    timeout_sec: float,
    body: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Issue an HTTP request and parse JSON body."""
    data: bytes | None = None
    headers = auth_headers(token)
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url=url, method=method, data=data, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout_sec) as resp:
            raw = resp.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"HTTP {exc.code} {url}: {detail}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"URL error {url}: {exc.reason}") from exc
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Non-JSON response from {url}: {raw[:256]}") from exc
    if not isinstance(parsed, dict):
        raise RuntimeError(f"Unexpected JSON shape from {url}: {type(parsed)!r}")
    return parsed


def probe_endpoint(
    endpoint: Endpoint,
    token: str | None,
    timeout_sec: float,
    check_root_reachable: bool,
) -> ProbeResult:
    """Probe endpoint health via gateway status and root reachability."""
    status_url = f"{endpoint.rest_url}/v1/meta/status"
    try:
        status = http_json("GET", status_url, token, timeout_sec)
    except RuntimeError as exc:
        return ProbeResult(ok=False, reason=str(exc))
    connected = bool(status.get("connected", False))
    if not connected:
        last_error = status.get("last_error", "gateway not connected")
        return ProbeResult(ok=False, reason=str(last_error))

    if not check_root_reachable:
        return ProbeResult(ok=True, reason="ok")

    qs = urllib.parse.urlencode(
        {"path": "/proc/root/reachable", "max_bytes": "64"},
        quote_via=urllib.parse.quote,
    )
    cat_url = f"{endpoint.rest_url}/v1/fs/cat?{qs}"
    try:
        reachable = http_json("GET", cat_url, token, timeout_sec)
    except RuntimeError as exc:
        return ProbeResult(ok=False, reason=f"reachable-check failed: {exc}")
    if reachable.get("status") != "OK":
        return ProbeResult(ok=False, reason=f"reachable-check status={reachable.get('status')}")
    lines = reachable.get("lines", [])
    if not isinstance(lines, list):
        return ProbeResult(ok=False, reason="reachable-check invalid lines")
    if not any(isinstance(line, str) and "reachable=yes" in line for line in lines):
        return ProbeResult(ok=False, reason=f"reachable-check lines={lines!r}")
    return ProbeResult(ok=True, reason="ok")


def canonical_path(path: pathlib.Path) -> pathlib.Path:
    """Return canonicalized path without requiring existence."""
    return path.resolve(strict=False)


def resolve_active_side(
    live_link: pathlib.Path,
    a_mount: pathlib.Path,
    b_mount: pathlib.Path,
) -> str | None:
    """Resolve which side (a or b) the live link currently points to."""
    if not live_link.exists() and not live_link.is_symlink():
        return None
    try:
        current = canonical_path(live_link)
    except OSError:
        return None
    if current == canonical_path(a_mount):
        return "a"
    if current == canonical_path(b_mount):
        return "b"
    return None


def flip_live_link(live_link: pathlib.Path, target_mount: pathlib.Path, dry_run: bool) -> None:
    """Atomically flip live symlink to the target mount path."""
    if dry_run:
        emit(
            "cutover-dry-run",
            live_link=str(live_link),
            target=str(target_mount),
        )
        return
    live_link.parent.mkdir(parents=True, exist_ok=True)
    tmp = live_link.with_name(
        f".{live_link.name}.tmp-{os.getpid()}-{int(time.time() * 1000)}"
    )
    try:
        if tmp.exists() or tmp.is_symlink():
            tmp.unlink()
        os.symlink(str(target_mount), str(tmp))
        os.replace(str(tmp), str(live_link))
    finally:
        if tmp.exists() or tmp.is_symlink():
            tmp.unlink()


def run_hook(
    hook_name: str,
    command_template: str | None,
    src_side: str,
    dst_side: str,
    dry_run: bool,
) -> None:
    """Execute optional hook command with side placeholders."""
    if not command_template:
        return
    command = command_template.format(src=src_side, dst=dst_side)
    emit("hook", name=hook_name, command=command, dry_run=dry_run)
    if dry_run:
        return
    subprocess.run(command, shell=True, check=True)


def other_side(side: str) -> str:
    """Return opposite side label."""
    return "b" if side == "a" else "a"


def decide_target_side(
    active_side: str | None,
    preferred_side: str,
    a_health: HealthState,
    b_health: HealthState,
    failure_threshold: int,
    success_threshold: int,
    hold_down_sec: float,
    seconds_since_cutover: float,
    allow_failback: bool,
) -> tuple[str | None, str]:
    """Return desired target side and reason."""
    if seconds_since_cutover < hold_down_sec:
        return None, "hold-down"

    health_map = {"a": a_health, "b": b_health}
    if active_side is None:
        preferred_health = health_map[preferred_side]
        if preferred_health.healthy(success_threshold):
            return preferred_side, "bootstrap"
        return None, "active-unknown"

    active_health = health_map[active_side]
    standby = other_side(active_side)
    standby_health = health_map[standby]

    if active_health.failed(failure_threshold) and standby_health.healthy(success_threshold):
        return standby, "active-failed"

    if (
        allow_failback
        and active_side != preferred_side
        and health_map[preferred_side].healthy(success_threshold)
    ):
        return preferred_side, "failback"

    return None, "stable"


def positive_int(value: str) -> int:
    """Parse positive integer CLI values."""
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be >= 1")
    return parsed


def non_negative_float(value: str) -> float:
    """Parse non-negative float CLI values."""
    parsed = float(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be >= 0")
    return parsed


def parse_args() -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--a-rest-url", required=True, help="REST URL for side A gateway.")
    parser.add_argument("--b-rest-url", required=True, help="REST URL for side B gateway.")
    parser.add_argument("--a-mount", required=True, help="Mount path for side A.")
    parser.add_argument("--b-mount", required=True, help="Mount path for side B.")
    parser.add_argument("--live-link", required=True, help="Stable live symlink path.")
    parser.add_argument(
        "--preferred-side",
        choices=["a", "b"],
        default="a",
        help="Preferred steady-state side (default: a).",
    )
    parser.add_argument(
        "--rest-auth-token",
        default=os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN", ""),
        help=(
            "REST request token; defaults to HIVE_GATEWAY_REQUEST_AUTH_TOKEN. "
            "Set empty only for gateways that do not require request-auth."
        ),
    )
    parser.add_argument(
        "--interval-sec",
        type=non_negative_float,
        default=1.0,
        help="Probe interval in seconds (default: 1.0).",
    )
    parser.add_argument(
        "--request-timeout-sec",
        type=non_negative_float,
        default=1.5,
        help="Per-request timeout in seconds (default: 1.5).",
    )
    parser.add_argument(
        "--failure-threshold",
        type=positive_int,
        default=3,
        help="Consecutive failures to declare active failed (default: 3).",
    )
    parser.add_argument(
        "--success-threshold",
        type=positive_int,
        default=1,
        help="Consecutive successes to treat side healthy (default: 1).",
    )
    parser.add_argument(
        "--hold-down-sec",
        type=non_negative_float,
        default=15.0,
        help="Minimum seconds between cutovers (default: 15).",
    )
    parser.add_argument(
        "--lock-file",
        default="/tmp/cohesix-failover-watchdog.lock",
        help="Exclusive watchdog lock file path.",
    )
    parser.add_argument(
        "--fence-cmd",
        default="",
        help="Optional shell command before cutover. Supports {src} and {dst}.",
    )
    parser.add_argument(
        "--post-cutover-cmd",
        default="",
        help="Optional shell command after cutover. Supports {src} and {dst}.",
    )
    parser.add_argument(
        "--relay-pause-cmd",
        default="",
        help=(
            "Optional shell command to pause federation relay before fencing/cutover. "
            "Supports {src} and {dst}."
        ),
    )
    parser.add_argument(
        "--relay-resume-cmd",
        default="",
        help=(
            "Optional shell command to resume federation relay after successful cutover. "
            "Supports {src} and {dst}."
        ),
    )
    parser.add_argument(
        "--allow-failback",
        action="store_true",
        help="Allow automatic failback to preferred side once healthy.",
    )
    parser.add_argument(
        "--skip-root-reachable-check",
        action="store_true",
        help="Skip /proc/root/reachable probe and use gateway connected status only.",
    )
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run one probe/evaluation cycle and exit.",
    )
    parser.add_argument(
        "--max-loops",
        type=positive_int,
        default=0,
        help="Maximum loops before exit (0 means run forever).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Do not change symlink or run hooks; log planned actions only.",
    )
    return parser.parse_args()


def acquire_lock(lock_file: pathlib.Path) -> int:
    """Acquire process-exclusive watchdog lock file."""
    fd = os.open(lock_file, os.O_RDWR | os.O_CREAT, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        os.close(fd)
        raise RuntimeError(f"lock already held: {lock_file}") from None
    return fd


def main() -> int:
    """Program entry point."""
    args = parse_args()
    token = args.rest_auth_token if args.rest_auth_token else None

    endpoint_a = Endpoint(
        name="a",
        rest_url=normalize_rest_url(args.a_rest_url),
        mount_path=pathlib.Path(args.a_mount),
    )
    endpoint_b = Endpoint(
        name="b",
        rest_url=normalize_rest_url(args.b_rest_url),
        mount_path=pathlib.Path(args.b_mount),
    )
    live_link = pathlib.Path(args.live_link)

    try:
        lock_fd = acquire_lock(pathlib.Path(args.lock_file))
    except RuntimeError as exc:
        emit("fatal", error=str(exc))
        return 2

    last_cutover_monotonic = -1e9
    loop = 0
    emit(
        "start",
        a_rest_url=endpoint_a.rest_url,
        b_rest_url=endpoint_b.rest_url,
        a_mount=str(endpoint_a.mount_path),
        b_mount=str(endpoint_b.mount_path),
        live_link=str(live_link),
        preferred_side=args.preferred_side,
        dry_run=args.dry_run,
    )

    try:
        while True:
            loop += 1
            probe_a = probe_endpoint(
                endpoint=endpoint_a,
                token=token,
                timeout_sec=args.request_timeout_sec,
                check_root_reachable=not args.skip_root_reachable_check,
            )
            endpoint_a.health.observe(probe_a.ok, probe_a.reason)

            probe_b = probe_endpoint(
                endpoint=endpoint_b,
                token=token,
                timeout_sec=args.request_timeout_sec,
                check_root_reachable=not args.skip_root_reachable_check,
            )
            endpoint_b.health.observe(probe_b.ok, probe_b.reason)

            active_side = resolve_active_side(
                live_link=live_link,
                a_mount=endpoint_a.mount_path,
                b_mount=endpoint_b.mount_path,
            )
            seconds_since_cutover = time.monotonic() - last_cutover_monotonic
            target_side, reason = decide_target_side(
                active_side=active_side,
                preferred_side=args.preferred_side,
                a_health=endpoint_a.health,
                b_health=endpoint_b.health,
                failure_threshold=args.failure_threshold,
                success_threshold=args.success_threshold,
                hold_down_sec=args.hold_down_sec,
                seconds_since_cutover=seconds_since_cutover,
                allow_failback=args.allow_failback,
            )

            emit(
                "probe",
                loop=loop,
                active_side=active_side,
                target_side=target_side,
                decision_reason=reason,
                a_ok=probe_a.ok,
                a_error=probe_a.reason if not probe_a.ok else "",
                a_failures=endpoint_a.health.consecutive_failures,
                a_successes=endpoint_a.health.consecutive_successes,
                b_ok=probe_b.ok,
                b_error=probe_b.reason if not probe_b.ok else "",
                b_failures=endpoint_b.health.consecutive_failures,
                b_successes=endpoint_b.health.consecutive_successes,
            )

            if target_side and target_side != active_side:
                src_side = active_side or "none"
                target_mount = (
                    endpoint_a.mount_path if target_side == "a" else endpoint_b.mount_path
                )
                if not target_mount.exists():
                    emit(
                        "cutover-blocked",
                        reason="target-mount-missing",
                        target_side=target_side,
                        target_mount=str(target_mount),
                    )
                else:
                    try:
                        run_hook(
                            hook_name="relay-pause",
                            command_template=args.relay_pause_cmd,
                            src_side=src_side,
                            dst_side=target_side,
                            dry_run=args.dry_run,
                        )
                        run_hook(
                            hook_name="fence",
                            command_template=args.fence_cmd,
                            src_side=src_side,
                            dst_side=target_side,
                            dry_run=args.dry_run,
                        )
                        flip_live_link(
                            live_link=live_link,
                            target_mount=target_mount,
                            dry_run=args.dry_run,
                        )
                        run_hook(
                            hook_name="post-cutover",
                            command_template=args.post_cutover_cmd,
                            src_side=src_side,
                            dst_side=target_side,
                            dry_run=args.dry_run,
                        )
                        run_hook(
                            hook_name="relay-resume",
                            command_template=args.relay_resume_cmd,
                            src_side=src_side,
                            dst_side=target_side,
                            dry_run=args.dry_run,
                        )
                        last_cutover_monotonic = time.monotonic()
                        emit(
                            "cutover",
                            src_side=src_side,
                            dst_side=target_side,
                            target_mount=str(target_mount),
                            reason=reason,
                            dry_run=args.dry_run,
                        )
                    except Exception as exc:  # noqa: BLE001
                        emit(
                            "cutover-error",
                            src_side=src_side,
                            dst_side=target_side,
                            error=str(exc),
                        )

            if args.once:
                break
            if args.max_loops and loop >= args.max_loops:
                break
            time.sleep(args.interval_sec)
    except KeyboardInterrupt:
        emit("stop", reason="keyboard-interrupt")
    finally:
        os.close(lock_fd)
    return 0


if __name__ == "__main__":
    sys.exit(main())
