# CLASSIFICATION: COMMUNITY
# Filename: sensor_proxy.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12

"""Proxy process normalizing live sensor input and emitting JSON events."""

from __future__ import annotations
import json
import os
import random
import time
from pathlib import Path
from .normalizer import SensorNormalizer

BASE = os.environ.get("COH_BASE", "")
SENSOR_DIR = Path(BASE) / "srv" / "sensors"
HISTORY_PATH = Path(BASE) / "history" / "sensor_log.json"


def read_raw_temperature() -> float:
    candidates = ["/sys/class/thermal/thermal_zone0/temp"]
    for path in candidates:
        try:
            return float(Path(path).read_text().strip()) / 1000.0
        except Exception:
            continue
    return random.uniform(20.0, 30.0)


def read_raw_accel() -> float:
    candidates = ["/sys/bus/iio/devices/iio:device0/in_accel_x_raw"]
    for path in candidates:
        try:
            return float(Path(path).read_text().strip())
        except Exception:
            continue
    return random.uniform(-1.0, 1.0)


def emit(sensor: str, raw: float, norm: float, anomaly: bool):
    SENSOR_DIR.mkdir(parents=True, exist_ok=True)
    HISTORY_PATH.parent.mkdir(parents=True, exist_ok=True)
    data = {"ts": time.time(), "raw": raw, "value": norm, "anomaly": anomaly}
    (SENSOR_DIR / f"{sensor}.json").write_text(json.dumps(data))
    with HISTORY_PATH.open("a") as f:
        f.write(json.dumps({"sensor": sensor, **data}) + "\n")


def run_once():
    t_norm = SensorNormalizer()
    a_norm = SensorNormalizer()
    temp = read_raw_temperature()
    accel = read_raw_accel()
    emit("temperature", temp, t_norm.feed(temp), t_norm.anomaly)
    emit("accelerometer", accel, a_norm.feed(accel), a_norm.anomaly)


def main():
    run_once()


if __name__ == "__main__":
    main()
