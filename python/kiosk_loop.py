# CLASSIFICATION: COMMUNITY
# Filename: kiosk_loop.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12

"""Simulated kiosk UI event loop for KioskInteractive role."""

from __future__ import annotations
import json
import os
import time
from pathlib import Path

BASE = os.environ.get("COH_BASE", "")
FED_PATH = Path(BASE) / "srv" / "kiosk_federation.json"


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
    while True:
        evt = {"timestamp": int(time.time()), "event": "heartbeat"}
        append_event(evt)
        time.sleep(5)


if __name__ == "__main__":
    main()
