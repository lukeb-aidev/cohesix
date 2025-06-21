# CLASSIFICATION: COMMUNITY
# Filename: common.py v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
"""Common helper functions for demo scenarios."""

from __future__ import annotations

import json
import logging
import os
import random
import time
from pathlib import Path

from validator import Validator

TRACE_DIR = Path("/log/trace")
RULE_DIR = Path("/mnt/data/rules")
GESTURE_DIR = Path("/mnt/data/gesture_map")


def ensure_dirs() -> None:
    """Ensure runtime directories exist."""
    TRACE_DIR.mkdir(parents=True, exist_ok=True)
    RULE_DIR.mkdir(parents=True, exist_ok=True)
    GESTURE_DIR.mkdir(parents=True, exist_ok=True)


def simple_sensor_sequence(n: int = 5) -> list[float]:
    """Generate a sequence of mock sensor values."""
    return [random.random() for _ in range(n)]


def run_demo(name: str) -> None:
    """Run a simple validator loop for *name* demo."""
    ensure_dirs()
    log_path = TRACE_DIR / f"{name}.log"
    v = Validator()
    rule_path = RULE_DIR / f"{name}_default.json"
    if rule_path.exists():
        v.inject_rule(rule_path)
    for value in simple_sensor_sequence():
        allow = v.evaluate("sim", value)
        v.emit_trace({"sim": value}, allow, log_path)
        time.sleep(0.02)

