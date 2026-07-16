#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify repeatable cold and warm Pi 4 WiFi boot evidence for one image.
# Copyright 2026 Lukas Bower

"""Fail-closed verifier for Pi 4 CYW43 WiFi repeatability evidence.

The evidence identity has two parts. ``--readback-image`` supplies the exact
bytes read back from the imaged media; their streamed digest must equal
``--image-sha256``. ``--build-marker`` must occur exactly once in those bytes
and in every counted serial boot slice. Both caller-supplied identities are
preserved in the report; neither is inferred from the captured logs.

The verifier consumes existing serial logs only. It delegates boot slicing and
gate interpretation to ``pi4_trace_normalize.py`` and never manufactures or
infers missing hardware observations.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import pi4_trace_normalize as trace_normalizer  # noqa: E402


SCHEMA = "cohesix-pi4-wifi-repeatability/v1"
DEFAULT_REQUIRED_PASSES = 10
IMAGE_SHA256_RE = re.compile(r"[0-9a-fA-F]{64}")
BUILD_MARKER_PREFIX = "[BUILD] "
READBACK_CHUNK_BYTES = 1024 * 1024


def positive_int(value: str) -> int:
    """Parse a strictly positive command-line integer."""

    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("value must be greater than zero")
    return parsed


def exact_build_marker(value: str) -> str:
    """Normalize a marker payload or validate an exact ``[BUILD]`` line."""

    if not value or value != value.strip():
        raise argparse.ArgumentTypeError(
            "build marker must be nonempty without surrounding whitespace"
        )
    if any(not character.isprintable() for character in value):
        raise argparse.ArgumentTypeError("build marker must be one printable line")
    if value.startswith("[BUILD]"):
        if not value.startswith(BUILD_MARKER_PREFIX):
            raise argparse.ArgumentTypeError(
                "full build marker must start with '[BUILD] '"
            )
        marker = value
    else:
        if "[BUILD]" in value:
            raise argparse.ArgumentTypeError(
                "build marker payload must not contain a '[BUILD]' token"
            )
        marker = f"{BUILD_MARKER_PREFIX}{value}"
    if not marker[len(BUILD_MARKER_PREFIX) :]:
        raise argparse.ArgumentTypeError("build marker payload must be nonempty")
    if marker.count("[BUILD]") != 1:
        raise argparse.ArgumentTypeError(
            "build marker must contain exactly one '[BUILD]' token"
        )
    return marker


def _sha256_text(value: str) -> str:
    """Return the SHA-256 fingerprint of a diagnostic string."""

    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _is_exact_build_marker(value: str) -> bool:
    """Return whether a report API value is already one canonical marker line."""

    try:
        return exact_build_marker(value) == value
    except argparse.ArgumentTypeError:
        return False


def _stable_strings(values: object) -> list[str]:
    """Return a sorted, de-duplicated list of string evidence labels."""

    if not isinstance(values, list):
        return []
    return sorted({str(value) for value in values})


def assess_readback_image(
    path_value: str | Path,
    expected_sha256: str,
    expected_marker: str,
) -> dict[str, object]:
    """Stream and validate the externally read-back image artifact."""

    path = Path(path_value)
    base: dict[str, object] = {
        "hash_match": False,
        "marker_occurrence_count": 0,
        "path": str(path),
        "sha256": None,
        "size_bytes": 0,
        "status": "missing",
        "verified": False,
    }
    if not path.is_file():
        return base

    digest = hashlib.sha256()
    size_bytes = 0
    marker_bytes = expected_marker.encode("utf-8")
    marker_occurrence_count = 0
    carry = b""
    carry_size = max(0, len(marker_bytes) - 1)
    try:
        with path.open("rb") as readback:
            while True:
                chunk = readback.read(READBACK_CHUNK_BYTES)
                if not chunk:
                    break
                digest.update(chunk)
                size_bytes += len(chunk)
                if marker_bytes:
                    data = carry + chunk
                    process_limit = len(data) - carry_size
                    search_start = 0
                    while process_limit > 0:
                        match_at = data.find(marker_bytes, search_start)
                        if match_at < 0 or match_at >= process_limit:
                            break
                        marker_occurrence_count += 1
                        search_start = match_at + 1
                    carry = data[-carry_size:] if carry_size else b""
    except OSError:
        return {**base, "status": "unreadable"}

    actual_sha256 = digest.hexdigest()
    expected_sha256_valid = IMAGE_SHA256_RE.fullmatch(expected_sha256) is not None
    hash_match = (
        expected_sha256_valid
        and actual_sha256 == expected_sha256.lower()
    )
    if not expected_sha256_valid:
        status = "expected-sha256-invalid"
    elif not hash_match:
        status = "hash-mismatch"
    elif marker_occurrence_count == 0:
        status = "marker-absent"
    elif marker_occurrence_count != 1:
        status = "marker-ambiguous"
    else:
        status = "verified"
    return {
        "hash_match": hash_match,
        "marker_occurrence_count": marker_occurrence_count,
        "path": str(path),
        "sha256": actual_sha256,
        "size_bytes": size_bytes,
        "status": status,
        "verified": status == "verified",
    }


def _marker_record(
    summary: Mapping[str, object],
    lines: Sequence[str],
    expected_marker: str,
) -> dict[str, object]:
    """Score the exact build marker within one normalizer-provided line range."""

    line_start = summary.get("line_start")
    line_end = summary.get("line_end")
    if (
        not isinstance(line_start, int)
        or isinstance(line_start, bool)
        or not isinstance(line_end, int)
        or isinstance(line_end, bool)
        or line_start < 1
        or line_end < line_start
        or line_end > len(lines)
    ):
        return {
            "conflicting_count": 0,
            "expected_match_count": 0,
            "observed_count": 0,
            "observed_line_sha256": [],
            "status": "range-invalid",
        }

    boot_lines = lines[line_start - 1 : line_end]
    observed = [line for line in boot_lines if line.startswith("[BUILD]")]
    expected_match_count = sum(line == expected_marker for line in observed)
    conflicting_count = len(observed) - expected_match_count
    if expected_match_count == 0:
        status = "missing" if not observed else "mismatch"
    elif conflicting_count:
        status = "conflict"
    else:
        status = "match"
    return {
        "conflicting_count": conflicting_count,
        "expected_match_count": expected_match_count,
        "observed_count": len(observed),
        "observed_line_sha256": sorted({_sha256_text(line) for line in observed}),
        "status": status,
    }


def _boot_record(
    summary: Mapping[str, object],
    lines: Sequence[str],
    expected_marker: str,
) -> dict[str, object]:
    """Reduce one normalizer boot summary to repeatability proof fields."""

    source_score = str(summary.get("score", "missing"))
    kind = str(summary.get("kind", "missing"))
    gates_value = summary.get("gates")
    gates = gates_value if isinstance(gates_value, Mapping) else None

    if source_score == "skip":
        return {
            "blockers": [],
            "boot": summary.get("boot", 0),
            "build_marker": {
                "expected_match_count": 0,
                "observed_count": 0,
                "observed_line_sha256": [],
                "status": "not-required",
            },
            "counted": False,
            "failure_reasons": [],
            "kind": kind,
            "line_end": summary.get("line_end", 0),
            "line_start": summary.get("line_start", 0),
            "net_active": "unknown",
            "source_score": source_score,
        }

    marker = _marker_record(summary, lines, expected_marker)
    source_blockers = _stable_strings(summary.get("blockers"))
    if gates is None:
        blockers = sorted({*source_blockers, "boot-gates-missing"})
        net_active = "unknown"
    else:
        current_blockers = trace_normalizer.boot_evidence_blockers(gates)
        blockers = sorted({*source_blockers, *current_blockers})
        net_active = str(gates.get("NET_ACTIVE", "unknown"))

    failure_reasons: list[str] = []
    if kind != "cohesix-boot":
        failure_reasons.append("not-cohesix-boot")
    if source_score != "pass":
        failure_reasons.append("source-score-not-pass")
    if blockers:
        failure_reasons.append("boot-evidence-blockers-present")
    if net_active != "wifi":
        failure_reasons.append("net-active-not-wifi")
    marker_status = marker["status"]
    if marker_status == "missing":
        failure_reasons.append("build-marker-missing")
    elif marker_status == "mismatch":
        failure_reasons.append("build-marker-mismatch")
    elif marker_status == "conflict":
        failure_reasons.append("build-marker-conflict")
    elif marker_status != "match":
        failure_reasons.append("build-marker-range-invalid")

    return {
        "blockers": blockers,
        "boot": summary.get("boot", 0),
        "build_marker": marker,
        "counted": not failure_reasons,
        "failure_reasons": failure_reasons,
        "kind": kind,
        "line_end": summary.get("line_end", 0),
        "line_start": summary.get("line_start", 0),
        "net_active": net_active,
        "source_score": source_score,
    }


def _empty_log_record(path: Path, reason: str) -> dict[str, object]:
    """Return a deterministic failed record for unavailable log evidence."""

    return {
        "boots": [],
        "counts": {
            "boot_slices": 0,
            "cohesix_slices": 0,
            "failed_slices": 0,
            "marker_failed_slices": 0,
            "marker_matched_slices": 0,
            "passing_wifi_slices": 0,
            "skipped_slices": 0,
        },
        "failure_reasons": [reason],
        "path": str(path),
        "result": "FAIL",
    }


def assess_log(path_value: str, expected_marker: str) -> dict[str, object]:
    """Assess every boot slice in one serial log without hiding failures."""

    path = Path(path_value)
    if not path.is_file():
        return _empty_log_record(path, "log-not-found")

    try:
        lines = trace_normalizer.read_input(str(path))
    except (OSError, UnicodeError):
        return _empty_log_record(path, "log-unreadable")
    if not lines:
        return _empty_log_record(path, "boot-slices-missing")

    summaries = trace_normalizer.summarize_boot_slices(lines)
    boots = [
        _boot_record(summary, lines, expected_marker) for summary in summaries
    ]
    skipped = sum(boot["source_score"] == "skip" for boot in boots)
    cohesix = sum(boot["kind"] == "cohesix-boot" for boot in boots)
    passing = sum(boot["counted"] is True for boot in boots)
    failed = sum(
        boot["source_score"] != "skip" and boot["counted"] is not True
        for boot in boots
    )
    marker_matched = sum(
        boot["build_marker"]["status"] == "match"  # type: ignore[index]
        for boot in boots
    )
    marker_failed = sum(
        boot["source_score"] != "skip"
        and boot["build_marker"]["status"] != "match"  # type: ignore[index]
        for boot in boots
    )

    failure_reasons: list[str] = []
    if not boots:
        failure_reasons.append("boot-slices-missing")
    elif cohesix == 0:
        failure_reasons.append("skip-only" if skipped else "cohesix-slices-missing")
    if failed:
        failure_reasons.append("failed-slices-present")

    return {
        "boots": boots,
        "counts": {
            "boot_slices": len(boots),
            "cohesix_slices": cohesix,
            "failed_slices": failed,
            "marker_failed_slices": marker_failed,
            "marker_matched_slices": marker_matched,
            "passing_wifi_slices": passing,
            "skipped_slices": skipped,
        },
        "failure_reasons": failure_reasons,
        "path": str(path),
        "result": "PASS" if not failure_reasons else "FAIL",
    }


def _class_record(
    classification: str,
    paths: Sequence[str],
    required_passes: int,
    expected_marker: str,
) -> dict[str, object]:
    """Aggregate one boot class while retaining each source-log record."""

    logs = [assess_log(path, expected_marker) for path in paths]
    passing_wifi_slices = sum(
        int(log["counts"]["passing_wifi_slices"])  # type: ignore[index]
        for log in logs
    )
    failed_slices = sum(
        int(log["counts"]["failed_slices"])  # type: ignore[index]
        for log in logs
    )
    skipped_slices = sum(
        int(log["counts"]["skipped_slices"])  # type: ignore[index]
        for log in logs
    )
    boot_slices = sum(
        int(log["counts"]["boot_slices"])  # type: ignore[index]
        for log in logs
    )
    failing_logs = sum(log["result"] != "PASS" for log in logs)
    marker_matched_slices = sum(
        int(log["counts"]["marker_matched_slices"])  # type: ignore[index]
        for log in logs
    )
    marker_failed_slices = sum(
        int(log["counts"]["marker_failed_slices"])  # type: ignore[index]
        for log in logs
    )

    failure_reasons: list[str] = []
    if not logs:
        failure_reasons.append(f"{classification}-logs-missing")
    if failing_logs:
        failure_reasons.append(f"{classification}-log-failures-present")
    if passing_wifi_slices < required_passes:
        failure_reasons.append(f"{classification}-passing-slices-insufficient")

    return {
        "counts": {
            "boot_slices": boot_slices,
            "failed_slices": failed_slices,
            "failing_logs": failing_logs,
            "logs": len(logs),
            "marker_failed_slices": marker_failed_slices,
            "marker_matched_slices": marker_matched_slices,
            "passing_logs": len(logs) - failing_logs,
            "passing_wifi_slices": passing_wifi_slices,
            "skipped_slices": skipped_slices,
        },
        "failure_reasons": failure_reasons,
        "logs": logs,
        "required_passing_wifi_slices": required_passes,
        "result": "PASS" if not failure_reasons else "FAIL",
    }


def build_report(
    *,
    cold_logs: Sequence[str],
    warm_logs: Sequence[str],
    image_sha256: str,
    build_marker: str,
    readback_image: str | Path,
    required_cold_passes: int = DEFAULT_REQUIRED_PASSES,
    required_warm_passes: int = DEFAULT_REQUIRED_PASSES,
) -> dict[str, object]:
    """Build a deterministic repeatability report from supplied evidence."""

    normalized_sha256 = image_sha256.lower()
    image_valid = IMAGE_SHA256_RE.fullmatch(image_sha256) is not None
    build_marker_valid = _is_exact_build_marker(build_marker)
    readback = assess_readback_image(
        readback_image, normalized_sha256, build_marker
    )
    cold = _class_record("cold", cold_logs, required_cold_passes, build_marker)
    warm = _class_record("warm", warm_logs, required_warm_passes, build_marker)
    identity_binding = _sha256_text(f"{normalized_sha256}\n{build_marker}")

    failure_reasons: list[str] = []
    if not image_valid:
        failure_reasons.append("image-sha256-invalid")
    if not build_marker_valid:
        failure_reasons.append("build-marker-invalid")
    readback_status = str(readback["status"])
    readback_failure_by_status = {
        "missing": "readback-image-missing",
        "unreadable": "readback-image-unreadable",
        "hash-mismatch": "readback-image-hash-mismatch",
        "marker-absent": "readback-image-build-marker-absent",
        "marker-ambiguous": "readback-image-build-marker-ambiguous",
    }
    readback_failure = readback_failure_by_status.get(readback_status)
    if readback_failure is not None:
        failure_reasons.append(readback_failure)
    failure_reasons.extend(cold["failure_reasons"])  # type: ignore[arg-type]
    failure_reasons.extend(warm["failure_reasons"])  # type: ignore[arg-type]

    return {
        "classes": {
            "cold": cold,
            "warm": warm,
        },
        "failure_reasons": failure_reasons,
        "build_marker": {
            "expected_line_sha256": _sha256_text(build_marker),
            "identity_role": "serial-boot-binding",
            "line": build_marker,
            "valid": build_marker_valid,
        },
        "image": {
            "identity_role": "external-readback",
            "sha256": normalized_sha256,
            "valid": image_valid,
        },
        "readback_image": readback,
        "identity_binding": {
            "scheme": (
                "sha256(verified-readback-sha256 + LF + "
                "exact-build-marker-line)"
            ),
            "sha256": (
                identity_binding
                if image_valid
                and build_marker_valid
                and readback["verified"] is True
                else None
            ),
            "valid": (
                image_valid
                and build_marker_valid
                and readback["verified"] is True
            ),
        },
        "result": "PASS" if not failure_reasons else "FAIL",
        "schema": SCHEMA,
    }


def build_parser() -> argparse.ArgumentParser:
    """Build the repeatability verifier command-line parser."""

    parser = argparse.ArgumentParser(
        description=(
            "Verify blocker-free WiFi boot slices using a two-part identity: "
            "the external readback SHA-256 of the imaged media and the exact "
            "[BUILD] marker emitted in every counted serial boot."
        )
    )
    parser.add_argument(
        "--cold-log",
        action="append",
        required=True,
        help="serial log containing cold-boot slices; may be repeated",
    )
    parser.add_argument(
        "--warm-log",
        action="append",
        required=True,
        help="serial log containing warm-reboot slices; may be repeated",
    )
    parser.add_argument(
        "--image-sha256",
        required=True,
        help=(
            "external readback-proven SHA-256 identity of the exact imaged media"
        ),
    )
    parser.add_argument(
        "--readback-image",
        required=True,
        type=Path,
        help="exact image bytes read back from the staged media",
    )
    parser.add_argument(
        "--build-marker",
        required=True,
        type=exact_build_marker,
        help=(
            "exact '[BUILD] ...' line (preferred), or an unambiguous nonempty "
            "payload, required in every counted boot slice"
        ),
    )
    parser.add_argument(
        "--required-cold-passes",
        type=positive_int,
        default=DEFAULT_REQUIRED_PASSES,
        help=f"required passing cold boots (default: {DEFAULT_REQUIRED_PASSES})",
    )
    parser.add_argument(
        "--required-warm-passes",
        type=positive_int,
        default=DEFAULT_REQUIRED_PASSES,
        help=f"required passing warm boots (default: {DEFAULT_REQUIRED_PASSES})",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="also write the deterministic JSON report to this path",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    """Run the repeatability verifier and return zero only for full proof."""

    args = build_parser().parse_args(argv)
    report = build_report(
        cold_logs=args.cold_log,
        warm_logs=args.warm_log,
        image_sha256=args.image_sha256,
        build_marker=args.build_marker,
        readback_image=args.readback_image,
        required_cold_passes=args.required_cold_passes,
        required_warm_passes=args.required_warm_passes,
    )
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output is not None:
        args.output.write_text(rendered, encoding="utf-8")
    sys.stdout.write(rendered)
    return 0 if report["result"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
