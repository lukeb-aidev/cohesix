# Author: Lukas Bower
# Purpose: Unit tests for scripts/failover_watchdog.py decision and cutover helpers.
# Copyright 2026 Lukas Bower

"""Tests for scripts/failover_watchdog.py."""

import importlib.util
import pathlib
import sys

MODULE_PATH = (
    pathlib.Path(__file__).resolve().parents[1]
    / "scripts"
    / "failover_watchdog.py"
)

spec = importlib.util.spec_from_file_location("failover_watchdog", MODULE_PATH)
watchdog = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = watchdog
spec.loader.exec_module(watchdog)


def test_normalize_rest_url_trims_slash() -> None:
    assert watchdog.normalize_rest_url("http://127.0.0.1:8080/") == "http://127.0.0.1:8080"
    assert watchdog.normalize_rest_url("http://127.0.0.1:8080") == "http://127.0.0.1:8080"


def test_health_state_thresholds() -> None:
    health = watchdog.HealthState()
    health.observe(ok=False, error="timeout")
    health.observe(ok=False, error="timeout")
    assert health.failed(2)
    assert not health.healthy(1)
    health.observe(ok=True, error="")
    assert not health.failed(2)
    assert health.healthy(1)


def test_resolve_active_side_from_symlink(tmp_path: pathlib.Path) -> None:
    a_mount = tmp_path / "mnt-a"
    b_mount = tmp_path / "mnt-b"
    live = tmp_path / "live"
    a_mount.mkdir()
    b_mount.mkdir()
    live.symlink_to(a_mount)
    assert watchdog.resolve_active_side(live, a_mount, b_mount) == "a"
    live.unlink()
    live.symlink_to(b_mount)
    assert watchdog.resolve_active_side(live, a_mount, b_mount) == "b"


def test_decide_target_side_active_failed() -> None:
    a_health = watchdog.HealthState(consecutive_failures=3, consecutive_successes=0, last_error="x")
    b_health = watchdog.HealthState(consecutive_failures=0, consecutive_successes=2, last_error="")
    target, reason = watchdog.decide_target_side(
        active_side="a",
        preferred_side="a",
        a_health=a_health,
        b_health=b_health,
        failure_threshold=3,
        success_threshold=1,
        hold_down_sec=0.0,
        seconds_since_cutover=100.0,
        allow_failback=False,
    )
    assert target == "b"
    assert reason == "active-failed"


def test_decide_target_side_respects_hold_down() -> None:
    a_health = watchdog.HealthState(consecutive_failures=3, consecutive_successes=0, last_error="x")
    b_health = watchdog.HealthState(consecutive_failures=0, consecutive_successes=2, last_error="")
    target, reason = watchdog.decide_target_side(
        active_side="a",
        preferred_side="a",
        a_health=a_health,
        b_health=b_health,
        failure_threshold=3,
        success_threshold=1,
        hold_down_sec=30.0,
        seconds_since_cutover=5.0,
        allow_failback=False,
    )
    assert target is None
    assert reason == "hold-down"


def test_flip_live_link_updates_target(tmp_path: pathlib.Path) -> None:
    a_mount = tmp_path / "mnt-a"
    b_mount = tmp_path / "mnt-b"
    live = tmp_path / "live"
    a_mount.mkdir()
    b_mount.mkdir()
    live.symlink_to(a_mount)
    watchdog.flip_live_link(live_link=live, target_mount=b_mount, dry_run=False)
    assert watchdog.resolve_active_side(live, a_mount, b_mount) == "b"
