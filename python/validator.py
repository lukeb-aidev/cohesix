# CLASSIFICATION: COMMUNITY
# Filename: validator.py v0.7
# Author: Lukas Bower
# Date Modified: 2025-12-10
"""Python-side validation helpers with live rule updates."""

import json
import logging
import operator
import sys
import time
from pathlib import Path
from jsonschema import ValidationError, validate

try:
    import tomllib as tomllib  # Python 3.11+
except ModuleNotFoundError:  # pragma: no cover - fallback for older Python
    import tomli as tomllib  # type: ignore

logger = logging.getLogger(__name__)

TRACE_EVENT_SCHEMA = {
    "type": "object",
    "required": ["ts", "event"],
    "properties": {
        "ts": {"type": "number"},
        "event": {"type": "string"},
        "detail": {},
    },
}

TRACE_SCHEMA = {"type": "array", "items": TRACE_EVENT_SCHEMA}


def trace_integrity(path: Path) -> bool:
    """Return True if trace file contains valid events."""
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return False
    for ln in lines:
        try:
            ev = json.loads(ln)
            validate(ev, TRACE_EVENT_SCHEMA)
        except (json.JSONDecodeError, ValidationError):
            return False
    return True


def load_trace_file(path: Path, fmt: str = "json") -> list[dict]:
    """Load a trace file and validate its schema."""
    try:
        if fmt == "jsonl":
            events = [json.loads(line) for line in path.read_text().splitlines()]
        else:
            events = json.loads(path.read_text())
    except Exception as exc:
        raise RuntimeError(f"failed to read trace {path}: {exc}") from exc
    try:
        validate(events, TRACE_SCHEMA)
    except ValidationError as exc:
        raise ValueError(f"trace schema error: {exc.message}") from exc
    return events


OPS = {
    ">": operator.gt,
    "<": operator.lt,
    ">=": operator.ge,
    "<=": operator.le,
    "==": operator.eq,
    "!=": operator.ne,
}


def _load_rule(path: Path) -> dict:
    """Load and validate a rule file from *path*."""
    try:
        text = path.read_text()
    except OSError as exc:  # pragma: no cover - I/O errors
        raise RuntimeError(f"failed to read rule {path}: {exc}") from exc

    try:
        if path.suffix.lower() in {".toml", ".tml"}:
            data = tomllib.loads(text)
        else:
            data = json.loads(text)
    except Exception as exc:
        raise ValueError(f"invalid rule format in {path}: {exc}") from exc

    allowed_top = {"conditions", "logic", "duration_active", "timeout"}
    unknown_top = set(data) - allowed_top
    if unknown_top:
        raise ValueError(f"unknown rule fields: {', '.join(sorted(unknown_top))}")

    if not isinstance(data.get("conditions"), list):
        raise ValueError("invalid rule format: conditions must be a list")

    for cond in data["conditions"]:
        if not isinstance(cond, dict):
            raise ValueError("invalid rule format: condition not dict")
        allowed_cond = {"sensor", "op", "threshold"}
        unknown_cond = set(cond) - allowed_cond
        if unknown_cond:
            raise ValueError(
                f"unknown condition fields: {', '.join(sorted(unknown_cond))}"
            )
        if cond.get("op") not in OPS:
            raise ValueError(f"invalid op {cond.get('op')}")

    return data


class Validator:
    """Runtime validator supporting rule chains with timeouts."""

    def __init__(self) -> None:
        self.rules: list[dict] = []

    def inject_rule(self, path: Path) -> None:
        """Load a rule from *path* and store internal metadata."""
        rule = _load_rule(path)
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


__all__ = ["trace_integrity", "Validator", "load_trace_file"]


def main_live(result_path: Path | None = None) -> None:
    validator = Validator()
    results: list[dict] = []
    if result_path:
        result_path.parent.mkdir(parents=True, exist_ok=True)
        tmp = result_path.with_suffix(".tmp")
        tmp.write_text("[]")
        tmp.replace(result_path)
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
            allowed = validator.evaluate(f.stem, float(data.get("value", 0)))
            if not allowed:
                logger.warning("violation %s", f.stem)
            results.append({"ts": time.time(), "sensor": f.stem, "allow": allowed})
            if result_path:
                result_path.parent.mkdir(parents=True, exist_ok=True)
                tmp = result_path.with_suffix(".tmp")
                tmp.write_text(json.dumps(results))
                tmp.replace(result_path)
        time.sleep(0.1)


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser()
    ap.add_argument("--live", action="store_true", help="run live validator loop")
    ap.add_argument("--input", help="validate trace file and exit")
    ap.add_argument(
        "--format", choices=["json", "jsonl"], default="json", help="trace file format"
    )
    ap.add_argument("--output", help="write validator results to file")
    ap.add_argument("--log", default="info", help="logging level")
    args = ap.parse_args()

    logging.basicConfig(level=getattr(logging, args.log.upper(), logging.INFO))

    try:
        if args.live:
            out = Path(args.output) if args.output else None
            main_live(out)
        elif args.input:
            load_trace_file(Path(args.input), args.format)
            print("trace valid")
            sys.exit(0)
        else:
            ap.print_help()
            sys.exit(1)
    except Exception as exc:  # safety catch to ensure non-zero exit
        logger.error("validator failed: %s", exc)
        sys.exit(1)
