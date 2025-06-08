# CLASSIFICATION: COMMUNITY
# Filename: normalizer.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12

"""Sensor normalization utilities with smoothing and anomaly detection."""

from __future__ import annotations
import time

class SensorNormalizer:
    """Low-pass filter normalizer with anomaly flags."""

    def __init__(self, alpha: float = 0.5, threshold: float = 2.0):
        self.alpha = alpha
        self.threshold = threshold
        self.last: float | None = None
        self.timestamp = 0.0
        self.anomaly = False

    def feed(self, value: float) -> float:
        now = time.time()
        self.timestamp = now
        if self.last is None:
            self.last = value
        smoothed = self.alpha * value + (1 - self.alpha) * self.last
        self.anomaly = abs(smoothed - self.last) > self.threshold
        self.last = smoothed
        return smoothed
