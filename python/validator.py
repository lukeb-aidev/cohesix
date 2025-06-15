# CLASSIFICATION: COMMUNITY
# Filename: validator.py v0.4
# Author: Lukas Bower
# Date Modified: 2025-06-09
"""Python-side validation helpers with live rule updates."""

import json
import logging
import operator
import time
from pathlib import Path

logger = logging.getLogger(__name__)


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


OPS = {
    ">": operator.gt,
    "<": operator.lt,
    ">=": operator.ge,
    "<=": operator.le,
    "==": operator.eq,
    "!=": operator.ne,
}


class Validator:
    """Runtime validator supporting rule chains with timeouts."""

    def __init__(self):
        self.rules: list[dict] = []

    def inject_rule(self, path: Path) -> None:
        """Load a rule from *path* and store internal metadata."""
        try:
            text = path.read_text()
            rule = json.loads(text)
        except OSError as exc:
            logger.error("failed to read rule %s: %s", path, exc)
            return
        except json.JSONDecodeError as exc:
            logger.error("invalid rule JSON %s: %s", path, exc)
            return
        if not isinstance(rule.get("conditions"), list):
            raise ValueError("invalid rule format")
        for cond in rule["conditions"]:
            if not all(k in cond for k in ("sensor", "op", "threshold")):
                raise ValueError("invalid rule format")
            if cond["op"] not in OPS:
                raise ValueError("invalid rule format")
        rule.setdefault("logic", "AND")
        rule.setdefault("duration_active", 1)
        rule.setdefault("timeout", 0)
        rule["_counter"] = 0
        rule["_injected_at"] = time.time()
        self.rules.append(rule)

    def evaluate(self, sensor: str, value: float) -> bool:
        """Evaluate a single sensor reading."""
        return self.evaluate_all({sensor: value})

    def evaluate_all(self, sensors: dict[str, float]) -> bool:
        """Evaluate a set of sensor readings."""
        allow = True
        now = time.time()
        for rule in self.rules:
            if rule["timeout"] and now - rule["_injected_at"] > rule["timeout"]:
                continue
            results = []
            for cond in rule["conditions"]:
                val = sensors.get(cond["sensor"])
                if val is None:
                    results.append(False)
                else:
                    op_func = OPS[cond["op"]]
                    results.append(op_func(val, cond["threshold"]))
            triggered = all(results) if rule.get("logic") == "AND" else any(results)
            if triggered:
                rule["_counter"] += 1
                if rule["_counter"] >= rule["duration_active"]:
                    allow = False
            else:
                rule["_counter"] = 0
        return allow

    def emit_trace(self, sensors: dict[str, float], allow: bool, path: Path) -> None:
        """Append a cohtrace-compatible event."""
        evt = {"ts": time.time(), "sensors": sensors, "allow": allow}
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            with path.open("a") as f:
                f.write(json.dumps(evt) + "\n")
        except OSError as exc:
            logger.error("failed to write trace %s: %s", path, exc)


__all__ = ["trace_integrity", "Validator"]


def main_live():
    validator = Validator()
    while True:
        inj = Path("/srv/validator/inject_rule")
        if inj.exists():
            try:
                validator.inject_rule(inj)
                inj.unlink()
            except Exception as exc:  # safety catch
                logger.error("failed to inject rule %s: %s", inj, exc)
        for f in Path("/srv/sensors").glob("*.json"):
            try:
                text = f.read_text()
                data = json.loads(text)
            except (OSError, json.JSONDecodeError) as exc:
                logger.error("failed to read sensor %s: %s", f, exc)
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
