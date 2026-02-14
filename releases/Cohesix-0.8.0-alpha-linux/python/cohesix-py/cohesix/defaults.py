"""Default policy values derived from coh-rtc outputs."""

from __future__ import annotations

from typing import Any, Dict

try:
    from .generated import DEFAULTS as GENERATED_DEFAULTS
except Exception:  # pragma: no cover - used only if generated is missing
    GENERATED_DEFAULTS = {
        "manifest_sha256": "unknown",
        "secure9p": {"msize": 8192, "walk_depth": 8},
        "console": {
            "max_line_len": 256,
            "max_path_len": 96,
            "max_json_len": 192,
            "max_id_len": 32,
            "max_echo_len": 128,
            "max_ticket_len": 224,
        },
        "paths": {
            "queen_ctl": "/queen/ctl",
            "queen_lifecycle_ctl": "/queen/lifecycle/ctl",
            "log": "/log/queen.log",
        },
        "telemetry_ingest": {
            "max_segments_per_device": 4,
            "max_bytes_per_segment": 32768,
            "max_total_bytes_per_device": 131072,
            "eviction_policy": "evict-oldest",
        },
        "telemetry_push": {"schema": "cohsh-telemetry-push/v1", "max_record_bytes": 4096},
        "coh": {},
        "retry": {"max_attempts": 3, "backoff_ms": 200, "ceiling_ms": 2000, "timeout_ms": 5000},
        "examples": {},
    }

DEFAULTS: Dict[str, Any] = GENERATED_DEFAULTS


def manifest_hash() -> str:
    return DEFAULTS.get("manifest_sha256", "unknown")
