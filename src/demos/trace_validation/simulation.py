# CLASSIFICATION: COMMUNITY
# Filename: simulation.py v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
"""Simulation logic for trace_validation demo."""

from __future__ import annotations
import logging
import time
from demos.common import ensure_dirs

def run() -> None:
    logging.basicConfig(level=logging.INFO)
    ensure_dirs()
    for idx in range(3):
        logging.info("%s step %d", "trace_validation", idx)
        time.sleep(0.02)
