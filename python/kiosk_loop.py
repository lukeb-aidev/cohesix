# CLASSIFICATION: COMMUNITY
# Filename: kiosk_loop.py v0.3
# Author: Lukas Bower
# Date Modified: 2025-06-09

"""Simulated kiosk UI event loop for KioskInteractive role."""

from __future__ import annotations

import json
import logging
import os
import signal
import time
from contextlib import contextmanager
from pathlib import Path

from sensors.sensor_proxy import run_once as capture_sensors, SENSOR_DIR  # type: ignore
from validator import Validator  # type: ignore


logger = logging.getLogger(__name__)

BASE = os.environ.get("COH_BASE", "")
FED_PATH = Path(BASE) / "srv" / "kiosk_federation.json"
LOG_PATH = Path(BASE) / "log" / "kiosk_watchdog.log"
TRACE_PATH = Path(BASE) / "log" / "validation" / "trace_run.json"
SUMMARY_FILE = Path(BASE) / "VALIDATION_SUMMARY.md"
validator = Validator()
_SHUTDOWN = False


def _handle_signal(signum, frame):
    global _SHUTDOWN
    _SHUTDOWN = True


signal.signal(signal.SIGINT, _handle_signal)
signal.signal(signal.SIGTERM, _handle_signal)


@contextmanager
def watchdog(timeout: int = 5):
    """Alarm-based watchdog raising TimeoutError on expiration."""

    def _timeout(signum, frame):
        raise TimeoutError

    old = signal.signal(signal.SIGALRM, _timeout)
    signal.alarm(timeout)
    try:
        yield
    finally:
        signal.alarm(0)
        signal.signal(signal.SIGALRM, old)


def append_event(event: dict) -> None:
    events = []
    if FED_PATH.exists():
        try:
            events = json.loads(FED_PATH.read_text())
        except Exception:
            events = []
    events.append(event)
    try:
        FED_PATH.write_text(json.dumps(events))
    except OSError as exc:
        logger.error("failed to write federation log %s: %s", FED_PATH, exc)


def append_summary(trace_name: str, allowed: bool) -> None:
    """Append a validation summary row."""
    row = (
        f"| {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())} | "
        f"{trace_name} | {'allow' if allowed else 'block'} | "
        f"{'PASS' if allowed else 'FAIL'} |\n"
    )
    SUMMARY_FILE.parent.mkdir(parents=True, exist_ok=True)
    try:
        with SUMMARY_FILE.open("a") as f:
            f.write(row)
    except OSError as exc:
        logger.error("failed to write summary %s: %s", SUMMARY_FILE, exc)


def main() -> None:
    while not _SHUTDOWN:
        try:
            with watchdog(5):
                capture_sensors()
                sensors = {}
                for f in SENSOR_DIR.glob("*.json"):
                    try:
                        text = f.read_text()
                        data = json.loads(text)
                        sensors[f.stem] = float(data.get("value", 0))
                    except (OSError, json.JSONDecodeError) as exc:
                        logger.error("failed to read sensor %s: %s", f, exc)
                        continue
                allowed = validator.evaluate_all(sensors)
                validator.emit_trace(sensors, allowed, TRACE_PATH)
                append_summary(TRACE_PATH.name, allowed)
                evt = {"timestamp": int(time.time()), "event": "heartbeat"}
                append_event(evt)
                time.sleep(5)
        except TimeoutError:
            LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
            try:
                with LOG_PATH.open("a") as logf:
                    logf.write(f"{int(time.time())}: watchdog timeout\n")
            except OSError as exc:
                logger.error("failed to write log %s: %s", LOG_PATH, exc)
            continue


if __name__ == "__main__":
    main()
