# CLASSIFICATION: COMMUNITY
# Filename: validator.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Python-side validation helpers with live rule updates."""

import json
import time
from pathlib import Path


def trace_integrity(path: Path) -> bool:
    """Return True if trace file contains valid events."""
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return False
    for ln in lines:
        try:
            ev = json.loads(ln)
        except json.JSONDecodeError:
            return False
        if "ts" not in ev or "event" not in ev:
            return False
    return True


class Validator:
    """Runtime validator supporting live rule injection."""

    def __init__(self):
        self.rules: list[dict] = []

    def inject_rule(self, path: Path) -> None:
        rule = json.loads(path.read_text())
        rule.setdefault("duration_active", 1)
        rule["_counter"] = 0
        self.rules.append(rule)

    def evaluate(self, sensor: str, value: float) -> bool:
        allow = True
        for rule in self.rules:
            if rule.get("sensor") == sensor and value > rule.get("threshold", float("inf")):
                rule["_counter"] += 1
                if rule["_counter"] >= rule.get("duration_active", 1):
                    allow = False
            else:
                rule["_counter"] = 0
        return allow


__all__ = ["trace_integrity", "Validator"]


def main_live():
    validator = Validator()
    while True:
        inj = Path("/srv/validator/inject_rule")
        if inj.exists():
            validator.inject_rule(inj)
            inj.unlink()
        for f in Path("/srv/sensors").glob("*.json"):
            try:
                data = json.loads(f.read_text())
            except Exception:
                continue
            if not validator.evaluate(f.stem, float(data.get("value", 0))):
                print(f"violation {f.stem}")
        time.sleep(0.1)


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser()
    ap.add_argument("--live", action="store_true", help="run live validator loop")
    args = ap.parse_args()

    if args.live:
        main_live()
