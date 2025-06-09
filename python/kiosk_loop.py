# CLASSIFICATION: COMMUNITY
# Filename: kiosk_loop.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-15

"""Simulated kiosk UI event loop for KioskInteractive role."""

from __future__ import annotations
import json
import os
import signal
import time
from contextlib import contextmanager
from pathlib import Path

BASE = os.environ.get("COH_BASE", "")
FED_PATH = Path(BASE) / "srv" / "kiosk_federation.json"
LOG_PATH = Path(BASE) / "log" / "kiosk_watchdog.log"
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
    FED_PATH.write_text(json.dumps(events))


def main() -> None:
    while not _SHUTDOWN:
        try:
            with watchdog(5):
                evt = {"timestamp": int(time.time()), "event": "heartbeat"}
                append_event(evt)
                time.sleep(5)
        except TimeoutError:
            LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
            with LOG_PATH.open("a") as f:
                f.write(f"{int(time.time())}: watchdog timeout\n")
            continue


if __name__ == "__main__":
    main()
