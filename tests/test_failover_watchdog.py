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


def _transaction(tmp_path, hook):
    a_mount, b_mount, live = (tmp_path / name for name in ("a", "b", "live"))
    a_mount.mkdir(exist_ok=True)
    b_mount.mkdir(exist_ok=True)
    live.symlink_to(a_mount)
    tx = watchdog.CutoverTransaction(tmp_path / "cutover.json", live, hook)
    return tx, a_mount, b_mount, live


def _receipt(request):
    return {**request, "status": "succeeded", "terminal": True, "verified": True}


def test_production_cutover_orders_verified_fence_before_promotion_and_routing(tmp_path):
    calls = []

    def hook(action, request):
        calls.append(action)
        assert live.resolve() == (b_mount if action == "resume" else a_mount)
        return _receipt(request)

    tx, a_mount, b_mount, live = _transaction(tmp_path, hook)
    tx.cutover("a", "b", 4, b_mount, lambda: True, "a" * 32, a_mount)
    assert calls == ["pause", "fence", "promote", "resume"]
    assert live.resolve() == b_mount
    reloaded = watchdog.CutoverTransaction(tx.path, live, hook)
    assert reloaded.recover()
    assert reloaded.state["phase"] == "complete"
    assert reloaded.state["writer_epoch"] == 4


def test_ambiguous_fence_receipt_never_promotes_and_removes_routing(tmp_path):
    calls = []

    def hook(action, request):
        calls.append(action)
        receipt = _receipt(request)
        if action == "fence":
            receipt["id"] = "b" * 32
        return receipt

    tx, a_mount, b_mount, live = _transaction(tmp_path, hook)
    import pytest
    with pytest.raises(RuntimeError, match="ambiguous"):
        tx.cutover("a", "b", 4, b_mount, lambda: True, "a" * 32, a_mount)
    assert calls == ["pause", "fence", "stop"]
    assert not live.is_symlink()
    assert tx.state["phase"] == "stopped"


def test_failed_post_cutover_health_stops_both_without_rollback(tmp_path):
    calls = []

    def hook(action, request):
        calls.append(action)
        return _receipt(request)

    tx, a_mount, b_mount, live = _transaction(tmp_path, hook)
    checks = iter([True, False])
    import pytest
    with pytest.raises(RuntimeError, match="post-cutover"):
        tx.cutover("a", "b", 4, b_mount, lambda: next(checks), "a" * 32, a_mount)
    assert calls == ["pause", "fence", "promote", "stop"]
    assert not live.is_symlink()
    assert tx.state["phase"] == "stopped"


def test_restart_at_every_durable_boundary_stops_without_repeating_promotion(tmp_path):
    import pytest

    class Crash(BaseException):
        pass

    for phase in ("prepared", "pause", "fence", "promote", "routing", "resume"):
        directory = tmp_path / phase
        directory.mkdir()
        calls = []

        def hook(action, request):
            calls.append(action)
            return _receipt(request)

        tx, a_mount, b_mount, live = _transaction(directory, hook)
        save = tx.save

        def crash_after_sync(next_phase):
            save(next_phase)
            if next_phase == phase:
                raise Crash()

        tx.save = crash_after_sync
        with pytest.raises(Crash):
            tx.cutover("a", "b", 4, b_mount, lambda: True, "a" * 32, a_mount)
        before = list(calls)
        recovered = watchdog.CutoverTransaction(tx.path, live, hook)
        assert not recovered.recover()
        assert calls == before + ["stop"]
        assert not live.is_symlink()
        assert recovered.state["phase"] == "stopped"


def test_crash_after_provider_commit_before_receipt_never_reissues_provider(tmp_path):
    import pytest

    class Crash(BaseException):
        pass

    for boundary in ("pause", "fence", "promote", "resume"):
        directory = tmp_path / boundary
        directory.mkdir()
        calls = []

        def hook(action, request):
            calls.append(action)
            if action == boundary:
                raise Crash()
            return _receipt(request)

        tx, a_mount, b_mount, live = _transaction(directory, hook)
        with pytest.raises(Crash):
            tx.cutover("a", "b", 4, b_mount, lambda: True, "a" * 32, a_mount)
        recovered = watchdog.CutoverTransaction(tx.path, live, hook)
        assert not recovered.recover()
        assert calls.count(boundary) == 1
        assert calls[-1] == "stop"


def test_stale_promotion_and_failed_stop_cannot_report_success(tmp_path):
    import pytest

    def hook(action, request):
        if action == "stop":
            raise RuntimeError("external stop unavailable")
        return _receipt(request)

    tx, a_mount, b_mount, live = _transaction(tmp_path, hook)
    with pytest.raises(RuntimeError, match="external stop unavailable"):
        tx.cutover("a", "b", 4, b_mount, lambda: False, "a" * 32, a_mount)
    assert tx.state["phase"] == "stop-unverified"
    assert not live.is_symlink()
    with pytest.raises(RuntimeError, match="invalid cutover"):
        tx.cutover("a", "b", 4, b_mount, lambda: True, "b" * 32, a_mount)


def test_storage_failure_does_not_suppress_physical_stop(tmp_path, monkeypatch):
    import pytest
    calls = []
    def hook(action, request):
        calls.append(action)
        return _receipt(request)
    tx, a_mount, b_mount, _live = _transaction(tmp_path, hook)
    tx.cutover("a", "b", 4, b_mount, lambda: True, "a" * 32, a_mount)
    calls.clear()
    def unavailable(_phase):
        raise OSError("journal volume unavailable")
    monkeypatch.setattr(tx, "save", unavailable)
    with pytest.raises(OSError, match="unavailable"):
        tx.stop()
    assert calls == ["stop"]
