#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify repeatable cold and warm Pi 4 WiFi boot evidence for one image.
# Copyright 2026 Lukas Bower

"""Fail-closed verifier for Pi 4 CYW43 WiFi repeatability evidence.

``--staged-image`` and ``--readback-image`` supply the source bytes and the
independently read-back target bytes. Both must be structurally valid legacy
U-Boot images, have correct CRCs, share the caller-supplied raw SHA-256, and
carry a marker whose ``image-id`` independently recomputes from every complete
image byte outside the fixed self-reference and CRC fields. That exact marker
must occur in every counted serial boot slice.

The verifier consumes existing staged/read-back artifacts and serial logs. It
delegates boot slicing and gate interpretation to ``pi4_trace_normalize.py``
and never manufactures or infers missing hardware observations.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import struct
import sys
import tempfile
from collections.abc import Mapping, Sequence
from dataclasses import asdict
from datetime import datetime
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

import pi4_trace_normalize as trace_normalizer  # noqa: E402
import pi4_image_identity as image_identity  # noqa: E402


SCHEMA = "cohesix-pi4-wifi-repeatability/v3"
CAPTURE_MANIFEST_SCHEMA = "cohesix-pi4-wifi-capture-manifest/v2"
DEFAULT_REQUIRED_PASSES = 10
IMAGE_SHA256_RE = re.compile(r"[0-9a-fA-F]{64}")
BUILD_MARKER_PREFIX = "[BUILD] "
READBACK_CHUNK_BYTES = 1024 * 1024
CANONICAL_BUILD_MARKER_MAX_BYTES = 512
CANONICAL_BUILD_MARKER_RE = image_identity.BUILD_MARKER_RE
IMAGE_ID_RE = re.compile(r" image-id=(?P<image_id>[0-9a-f]{64}) ")
RUN_ID_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}")
GIT_COMMIT_RE = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})")
BUILD_ID_RE = re.compile(r"[0-9a-f]{64}")
PCAP_MAGIC = {
    b"\xd4\xc3\xb2\xa1": ("<", 1_000_000, "little-microsecond"),
    b"\xa1\xb2\xc3\xd4": (">", 1_000_000, "big-microsecond"),
    b"\x4d\x3c\xb2\xa1": ("<", 1_000_000_000, "little-nanosecond"),
    b"\xa1\xb2\x3c\x4d": (">", 1_000_000_000, "big-nanosecond"),
}


class EvidenceReadError(ValueError):
    """Raised when an evidence file cannot be read from one stable inode."""


def _read_stable_regular_file(path: Path) -> dict[str, object]:
    """Open an evidence file once and reject identity or metadata changes."""

    flags = os.O_RDONLY
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NONBLOCK", 0)
    descriptor: int | None = None
    try:
        descriptor = os.open(path, flags)
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise EvidenceReadError(f"evidence path is not a regular file: {path}")
        chunks: list[bytes] = []
        while True:
            chunk = os.read(descriptor, READBACK_CHUNK_BYTES)
            if not chunk:
                break
            chunks.append(chunk)
        after = os.fstat(descriptor)
    except EvidenceReadError:
        raise
    except (OSError, ValueError) as error:
        raise EvidenceReadError(f"cannot read evidence file {path}: {error}") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)

    before_identity = (
        before.st_dev,
        before.st_ino,
        before.st_size,
        before.st_mtime_ns,
        before.st_ctime_ns,
    )
    after_identity = (
        after.st_dev,
        after.st_ino,
        after.st_size,
        after.st_mtime_ns,
        after.st_ctime_ns,
    )
    data = b"".join(chunks)
    if before_identity != after_identity or len(data) != after.st_size:
        raise EvidenceReadError(f"evidence file changed while open: {path}")
    return {
        "data": data,
        "device": after.st_dev,
        "inode": after.st_ino,
        "size_bytes": after.st_size,
        "mtime_ns": after.st_mtime_ns,
        "ctime_ns": after.st_ctime_ns,
        "resolved_path": _resolved_path(path),
    }


def _raw_line_slices(raw: bytes, decoded_lines: Sequence[str]) -> list[bytes] | None:
    """Map normalizer line numbers back to exact captured serial bytes."""

    raw_lines = raw.splitlines(keepends=True)
    mapped: list[str] = []
    for raw_line in raw_lines:
        decoded = raw_line.decode("utf-8", errors="replace").splitlines()
        if len(decoded) != 1:
            return None
        mapped.append(decoded[0])
    if mapped != list(decoded_lines):
        return None
    return raw_lines


def _read_stable_json(path: Path, label: str) -> tuple[dict[str, object], dict[str, object]]:
    """Read one JSON object from a stable regular file descriptor."""

    snapshot = _read_stable_regular_file(path)
    raw = snapshot["data"]
    assert isinstance(raw, bytes)
    try:
        value = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvidenceReadError(f"{label} is not valid UTF-8 JSON: {error}") from error
    if not isinstance(value, dict):
        raise EvidenceReadError(f"{label} must be a JSON object")
    return value, snapshot


def _inspect_classic_pcap(data: bytes) -> dict[str, object]:
    """Validate one classic pcap and derive its packet-time interval."""

    if len(data) < 24:
        raise EvidenceReadError("pcap global header is truncated")
    format_record = PCAP_MAGIC.get(data[:4])
    if format_record is None:
        raise EvidenceReadError("capture is not a supported classic pcap")
    endian, timestamp_scale, format_label = format_record
    try:
        major, minor, _zone, _sigfigs, snaplen, linktype = struct.unpack_from(
            f"{endian}HHiIII", data, 4
        )
    except struct.error as error:
        raise EvidenceReadError("pcap global header is malformed") from error
    if (major, minor) != (2, 4):
        raise EvidenceReadError("pcap version must be 2.4")
    if snaplen == 0:
        raise EvidenceReadError("pcap snaplen must be nonzero")

    offset = 24
    packet_count = 0
    first_epoch: int | None = None
    last_epoch: int | None = None
    while offset < len(data):
        if len(data) - offset < 16:
            raise EvidenceReadError("pcap packet header is truncated")
        try:
            seconds, fraction, captured_len, original_len = struct.unpack_from(
                f"{endian}IIII", data, offset
            )
        except struct.error as error:
            raise EvidenceReadError("pcap packet header is malformed") from error
        offset += 16
        if seconds == 0 or fraction >= timestamp_scale:
            raise EvidenceReadError("pcap packet timestamp is invalid")
        if captured_len == 0 or original_len == 0:
            raise EvidenceReadError("pcap packet lengths must be nonzero")
        if captured_len > original_len or captured_len > snaplen:
            raise EvidenceReadError("pcap packet lengths exceed the capture contract")
        packet_end = offset + captured_len
        if packet_end > len(data):
            raise EvidenceReadError("pcap packet payload is truncated")
        offset = packet_end
        packet_count += 1
        first_epoch = seconds if first_epoch is None else min(first_epoch, seconds)
        last_epoch = seconds if last_epoch is None else max(last_epoch, seconds)

    if packet_count == 0 or first_epoch is None or last_epoch is None:
        raise EvidenceReadError("pcap contains no packets")
    return {
        "first_packet_epoch": first_epoch,
        "format": format_label,
        "last_packet_epoch": last_epoch,
        "linktype": linktype,
        "packet_count": packet_count,
        "snaplen": snaplen,
    }


def _build_timestamp_epoch(value: object) -> int | None:
    """Parse the canonical RFC3339 build timestamp into Unix seconds."""

    if not isinstance(value, str) or not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return int(parsed.timestamp())


def _identity_metadata_record(
    path: Path,
    expected_sha256: str,
    expected_marker: str,
    expected_git_commit: str,
    expected_build_id: str,
    expected_metadata_sha256: str,
    staged_image: Mapping[str, object],
) -> dict[str, object]:
    """Validate the required clean exact-image identity sidecar."""

    base: dict[str, object] = {
        "path": str(path),
        "resolved_path": _resolved_path(path),
        "result": "FAIL",
        "failure_reasons": [],
    }
    try:
        metadata, snapshot = _read_stable_json(path, "image identity metadata")
    except EvidenceReadError as error:
        return {
            **base,
            "failure_reasons": ["image-identity-metadata-unreadable"],
            "identity_error": str(error),
        }

    metadata_bytes = snapshot.get("data")
    assert isinstance(metadata_bytes, bytes)
    metadata_sha256 = hashlib.sha256(metadata_bytes).hexdigest()
    strict_error: str | None = None
    try:
        strict_metadata = image_identity.parse_metadata_bytes(
            metadata_bytes,
            source=str(path),
        )
    except image_identity.ImageIdentityError as error:
        strict_error = str(error)
    else:
        metadata = asdict(strict_metadata)

    failure_reasons: list[str] = []
    if BUILD_ID_RE.fullmatch(expected_metadata_sha256) is None:
        failure_reasons.append("expected-image-identity-sha256-invalid")
    elif metadata_sha256 != expected_metadata_sha256:
        failure_reasons.append("image-identity-metadata-sha256-mismatch")
    if strict_error is not None:
        failure_reasons.append("image-identity-metadata-invalid")
    if metadata.get("schema") != image_identity.SCHEMA:
        failure_reasons.append("image-identity-metadata-schema-invalid")
    git_commit = metadata.get("git_commit")
    embedded_git_commit = metadata.get("embedded_git_commit")
    build_id = metadata.get("build_id")
    if not isinstance(git_commit, str) or GIT_COMMIT_RE.fullmatch(git_commit) is None:
        failure_reasons.append("image-identity-git-commit-invalid")
    elif git_commit != expected_git_commit:
        failure_reasons.append("image-identity-git-commit-mismatch")
    if (
        not isinstance(embedded_git_commit, str)
        or len(embedded_git_commit) < 7
        or not isinstance(git_commit, str)
        or not git_commit.startswith(embedded_git_commit)
    ):
        failure_reasons.append("image-identity-embedded-commit-mismatch")
    if metadata.get("source_tree_clean") is not True:
        failure_reasons.append("image-identity-source-tree-not-clean")
    marker_parts = expected_marker.split()
    marker_commit = marker_parts[1] if len(marker_parts) > 1 else ""
    if marker_commit.endswith("-dirty"):
        failure_reasons.append("image-identity-build-marker-dirty")
    elif marker_commit != embedded_git_commit:
        failure_reasons.append("image-identity-marker-commit-mismatch")
    if not isinstance(build_id, str) or BUILD_ID_RE.fullmatch(build_id) is None:
        failure_reasons.append("image-identity-build-id-invalid")
    elif build_id != expected_build_id:
        failure_reasons.append("image-identity-build-id-mismatch")
    if metadata.get("image_sha256") != expected_sha256:
        failure_reasons.append("image-identity-sha256-mismatch")
    metadata_image_path = metadata.get("path")
    if (
        not isinstance(metadata_image_path, str)
        or _resolved_path(Path(metadata_image_path))
        != staged_image.get("resolved_path")
    ):
        failure_reasons.append("image-identity-staged-path-mismatch")
    if staged_image.get("verified") is True:
        for key in ("device", "inode", "size_bytes", "mtime_ns", "ctime_ns"):
            if metadata.get(key) != staged_image.get(key):
                failure_reasons.append(
                    f"image-identity-staged-{key.replace('_', '-')}-mismatch"
                )
    marker_match = IMAGE_ID_RE.search(expected_marker)
    expected_image_id = marker_match.group("image_id") if marker_match else None
    if metadata.get("image_id") != expected_image_id:
        failure_reasons.append("image-identity-image-id-mismatch")
    if metadata.get("build_marker") != expected_marker:
        failure_reasons.append("image-identity-build-marker-mismatch")
    build_timestamp = metadata.get("build_timestamp")
    marker_match = image_identity.BUILD_MARKER_RE.fullmatch(
        expected_marker.encode("ascii", errors="ignore")
    )
    marker_timestamp = (
        marker_match.group("build_timestamp").decode("ascii")
        if marker_match is not None
        else None
    )
    if build_timestamp != marker_timestamp:
        failure_reasons.append("image-identity-build-timestamp-mismatch")
    build_epoch = _build_timestamp_epoch(build_timestamp)
    if build_epoch is None:
        failure_reasons.append("image-identity-build-timestamp-invalid")
    if (
        isinstance(git_commit, str)
        and isinstance(build_timestamp, str)
        and isinstance(expected_image_id, str)
    ):
        try:
            derived_build_id = image_identity.canonical_build_id(
                git_commit,
                build_timestamp,
                expected_image_id,
            )
        except image_identity.ImageIdentityError:
            failure_reasons.append("image-identity-build-id-not-canonical")
        else:
            if build_id != derived_build_id or expected_build_id != derived_build_id:
                failure_reasons.append("image-identity-build-id-not-canonical")

    public_snapshot = {key: value for key, value in snapshot.items() if key != "data"}
    return {
        **base,
        **public_snapshot,
        "build_epoch": build_epoch,
        "build_id": build_id,
        "embedded_git_commit": embedded_git_commit,
        "failure_reasons": failure_reasons,
        "git_commit": git_commit,
        "image_id": metadata.get("image_id"),
        "sha256": metadata_sha256,
        "metadata": metadata,
        "identity_error": strict_error,
        "result": "PASS" if not failure_reasons else "FAIL",
    }


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
    if CANONICAL_BUILD_MARKER_RE.fullmatch(marker.encode("utf-8")) is None:
        raise argparse.ArgumentTypeError(
            "build marker must match the generated root-task marker format"
        )
    image_id_match = IMAGE_ID_RE.search(marker)
    if (
        image_id_match is None
        or image_id_match.group("image_id") == image_identity.UNSEALED_IMAGE_ID
    ):
        raise argparse.ArgumentTypeError(
            "build marker must contain a sealed content-derived image-id"
        )
    return marker


def _sha256_text(value: str) -> str:
    """Return the SHA-256 fingerprint of a diagnostic string."""

    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _resolved_path(path: Path) -> str:
    """Return a stable alias identity without trusting symlink resolution."""

    try:
        return str(path.resolve(strict=False))
    except (OSError, RuntimeError, ValueError):
        return str(path.absolute())


def _paths_alias(first: Path, second: Path) -> bool:
    """Return whether two artifact paths name the same file or inode."""

    if _resolved_path(first) == _resolved_path(second):
        return True
    try:
        return first.samefile(second)
    except (OSError, ValueError):
        return False


def _output_input_alias(
    output: Path,
    evidence_inputs: Sequence[tuple[str, Path]],
) -> tuple[str, Path] | None:
    """Return the first evidence input aliased by the requested output path."""

    for option, input_path in evidence_inputs:
        if _paths_alias(output, input_path):
            return option, input_path
    return None


def _atomic_write_report(path: Path, rendered: str) -> None:
    """Replace a report atomically after all evidence has been validated."""

    temporary_path: str | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            delete=False,
        ) as temporary:
            temporary_path = temporary.name
            temporary.write(rendered)
            temporary.flush()
            os.fsync(temporary.fileno())
        os.replace(temporary_path, path)
        temporary_path = None
    finally:
        if temporary_path is not None:
            try:
                os.unlink(temporary_path)
            except FileNotFoundError:
                pass


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
    """Validate one stable, sealed, content-bound legacy U-Boot image."""

    path = Path(path_value)
    base: dict[str, object] = {
        "conflicting_marker_count": 0,
        "ctime_ns": None,
        "distinct_marker_line_sha256": [],
        "device": None,
        "hash_match": False,
        "image_id": None,
        "image_id_match": False,
        "identity_error": None,
        "inode": None,
        "marker_occurrence_count": 0,
        "mtime_ns": None,
        "path": str(path),
        "resolved_path": _resolved_path(path),
        "sha256": None,
        "size_bytes": 0,
        "status": "missing",
        "verified": False,
    }
    if not path.is_file():
        return base
    try:
        identity = image_identity.inspect_image(path)
    except image_identity.ImageIdentityError as error:
        return {
            **base,
            "identity_error": str(error),
            "status": "image-identity-invalid",
        }

    actual_marker = identity.build_marker
    actual_sha256 = identity.image_sha256
    expected_image_id_match = IMAGE_ID_RE.search(expected_marker)
    expected_image_id = (
        expected_image_id_match.group("image_id")
        if expected_image_id_match is not None
        else None
    )
    expected_sha256_valid = IMAGE_SHA256_RE.fullmatch(expected_sha256) is not None
    hash_match = expected_sha256_valid and actual_sha256 == expected_sha256.lower()
    marker_match = actual_marker == expected_marker
    content_id_match = identity.image_id == expected_image_id
    if not expected_sha256_valid:
        status = "expected-sha256-invalid"
    elif not hash_match:
        status = "hash-mismatch"
    elif not marker_match:
        status = "marker-conflict"
    elif not content_id_match:
        status = "image-id-mismatch"
    else:
        status = "verified"
    return {
        "conflicting_marker_count": int(not marker_match),
        "ctime_ns": identity.ctime_ns,
        "device": identity.device,
        "distinct_marker_line_sha256": [identity.build_marker_sha256],
        "hash_match": hash_match,
        "image_id": identity.image_id,
        "image_id_match": content_id_match,
        "identity_error": None,
        "inode": identity.inode,
        "marker_occurrence_count": int(marker_match),
        "mtime_ns": identity.mtime_ns,
        "path": str(path),
        "resolved_path": _resolved_path(path),
        "sha256": actual_sha256,
        "size_bytes": identity.size_bytes,
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
    raw_lines: Sequence[bytes] | None,
    expected_marker: str,
) -> dict[str, object]:
    """Reduce one normalizer boot summary to repeatability proof fields."""

    source_score = str(summary.get("score", "missing"))
    kind = str(summary.get("kind", "missing"))
    gates_value = summary.get("gates")
    gates = gates_value if isinstance(gates_value, Mapping) else None
    line_start = summary.get("line_start")
    line_end = summary.get("line_end")
    raw_slice_sha256: str | None = None
    if (
        raw_lines is not None
        and isinstance(line_start, int)
        and not isinstance(line_start, bool)
        and isinstance(line_end, int)
        and not isinstance(line_end, bool)
        and line_start >= 1
        and line_end >= line_start
        and line_end <= len(raw_lines)
    ):
        raw_slice_sha256 = hashlib.sha256(
            b"".join(raw_lines[line_start - 1 : line_end])
        ).hexdigest()

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
            "raw_slice_sha256": raw_slice_sha256,
            "serial_slice_index": summary.get("boot", 0),
            "source_score": source_score,
            "supervisor_ready": "not-required",
            "supervisor_seen": "not-required",
        }

    marker = _marker_record(summary, lines, expected_marker)
    source_blockers = _stable_strings(summary.get("blockers"))
    if gates is None:
        blockers = sorted({*source_blockers, "boot-gates-missing"})
        net_active = "unknown"
        supervisor_seen = "unknown"
        supervisor_ready = "unknown"
    else:
        current_blockers = trace_normalizer.boot_evidence_blockers(gates)
        blockers = sorted({*source_blockers, *current_blockers})
        net_active = str(gates.get("NET_ACTIVE", "unknown"))
        supervisor_seen = str(
            gates.get("CYW43_BOOTSTRAP_SUPERVISOR_SEEN", "no")
        )
        supervisor_ready = str(
            gates.get("CYW43_BOOTSTRAP_SUPERVISOR_READY", "no")
        )

    failure_reasons: list[str] = []
    if kind != "cohesix-boot":
        failure_reasons.append("not-cohesix-boot")
    if source_score != "pass":
        failure_reasons.append("source-score-not-pass")
    if raw_slice_sha256 is None:
        failure_reasons.append("raw-boot-slice-range-invalid")
    if blockers:
        failure_reasons.append("boot-evidence-blockers-present")
    if net_active != "wifi":
        failure_reasons.append("net-active-not-wifi")
    if supervisor_seen != "yes":
        failure_reasons.append("bootstrap-supervisor-not-seen")
    elif supervisor_ready != "yes":
        failure_reasons.append("bootstrap-supervisor-not-ready")
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
        "raw_slice_sha256": raw_slice_sha256,
        "serial_slice_index": summary.get("boot", 0),
        "source_score": source_score,
        "supervisor_ready": supervisor_ready,
        "supervisor_seen": supervisor_seen,
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
        "ctime_ns": None,
        "device": None,
        "inode": None,
        "mtime_ns": None,
        "path": str(path),
        "resolved_path": _resolved_path(path),
        "result": "FAIL",
        "sha256": None,
        "size_bytes": 0,
    }


def assess_log(path_value: str, expected_marker: str) -> dict[str, object]:
    """Assess every boot slice in one serial log without hiding failures."""

    path = Path(path_value)
    try:
        snapshot = _read_stable_regular_file(path)
        raw_log = snapshot["data"]
        assert isinstance(raw_log, bytes)
        lines = raw_log.decode("utf-8", errors="replace").splitlines()
        log_sha256 = hashlib.sha256(raw_log).hexdigest()
    except EvidenceReadError as error:
        reason = "log-not-found" if not path.exists() else "log-unstable-or-nonregular"
        record = _empty_log_record(path, reason)
        record["identity_error"] = str(error)
        return record
    if not lines:
        return _empty_log_record(path, "boot-slices-missing")
    raw_lines = _raw_line_slices(raw_log, lines)

    summaries = trace_normalizer.summarize_boot_slices(lines)
    boots = [
        _boot_record(summary, lines, raw_lines, expected_marker)
        for summary in summaries
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
    if raw_lines is None:
        failure_reasons.append("raw-line-mapping-ambiguous")
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
        "ctime_ns": snapshot["ctime_ns"],
        "device": snapshot["device"],
        "inode": snapshot["inode"],
        "mtime_ns": snapshot["mtime_ns"],
        "path": str(path),
        "resolved_path": snapshot["resolved_path"],
        "result": "PASS" if not failure_reasons else "FAIL",
        "sha256": log_sha256,
        "size_bytes": snapshot["size_bytes"],
    }


def _evidence_input_record(
    cold: Mapping[str, object], warm: Mapping[str, object]
) -> dict[str, object]:
    """Reject aliased or byte-identical captures before counting evidence."""

    labeled_logs: list[tuple[str, Mapping[str, object]]] = []
    for classification, record in (("cold", cold), ("warm", warm)):
        logs = record.get("logs")
        if isinstance(logs, list):
            labeled_logs.extend(
                (classification, log)
                for log in logs
                if isinstance(log, Mapping)
            )

    path_owners: dict[str, list[str]] = {}
    inode_owners: dict[tuple[int, int], list[str]] = {}
    hash_owners: dict[str, list[str]] = {}
    slice_owners: dict[str, list[str]] = {}
    for classification, log in labeled_logs:
        resolved_path = log.get("resolved_path")
        if isinstance(resolved_path, str):
            path_owners.setdefault(resolved_path, []).append(classification)
        sha256 = log.get("sha256")
        if isinstance(sha256, str):
            hash_owners.setdefault(sha256, []).append(classification)
        device = log.get("device")
        inode = log.get("inode")
        if isinstance(device, int) and isinstance(inode, int):
            inode_owners.setdefault((device, inode), []).append(classification)
        boots = log.get("boots")
        if isinstance(boots, list):
            for boot in boots:
                if not isinstance(boot, Mapping) or boot.get("source_score") == "skip":
                    continue
                raw_slice_sha256 = boot.get("raw_slice_sha256")
                if isinstance(raw_slice_sha256, str):
                    slice_owners.setdefault(raw_slice_sha256, []).append(classification)

    duplicate_paths = sorted(
        path for path, owners in path_owners.items() if len(owners) > 1
    )
    duplicate_hashes = sorted(
        sha256 for sha256, owners in hash_owners.items() if len(owners) > 1
    )
    duplicate_inodes = sorted(
        f"{device}:{inode}"
        for (device, inode), owners in inode_owners.items()
        if len(owners) > 1
    )
    duplicate_slice_hashes = sorted(
        sha256 for sha256, owners in slice_owners.items() if len(owners) > 1
    )
    cross_class_paths = sorted(
        path
        for path, owners in path_owners.items()
        if {"cold", "warm"}.issubset(set(owners))
    )
    cross_class_hashes = sorted(
        sha256
        for sha256, owners in hash_owners.items()
        if {"cold", "warm"}.issubset(set(owners))
    )
    cross_class_inodes = sorted(
        f"{device}:{inode}"
        for (device, inode), owners in inode_owners.items()
        if {"cold", "warm"}.issubset(set(owners))
    )
    cross_class_slice_hashes = sorted(
        sha256
        for sha256, owners in slice_owners.items()
        if {"cold", "warm"}.issubset(set(owners))
    )

    failure_reasons: list[str] = []
    if duplicate_paths:
        failure_reasons.append("duplicate-log-paths")
    if duplicate_hashes:
        failure_reasons.append("duplicate-log-content")
    if duplicate_inodes:
        failure_reasons.append("duplicate-log-open-file-identity")
    if duplicate_slice_hashes:
        failure_reasons.append("duplicate-raw-boot-slices")
    if cross_class_paths:
        failure_reasons.append("cold-warm-log-path-overlap")
    if cross_class_hashes:
        failure_reasons.append("cold-warm-log-content-overlap")
    if cross_class_inodes:
        failure_reasons.append("cold-warm-log-open-file-overlap")
    if cross_class_slice_hashes:
        failure_reasons.append("cold-warm-raw-boot-slice-overlap")

    return {
        "cross_class_content_sha256": cross_class_hashes,
        "cross_class_open_file_identities": cross_class_inodes,
        "cross_class_paths": cross_class_paths,
        "cross_class_raw_slice_sha256": cross_class_slice_hashes,
        "duplicate_content_sha256": duplicate_hashes,
        "duplicate_open_file_identities": duplicate_inodes,
        "duplicate_paths": duplicate_paths,
        "duplicate_raw_slice_sha256": duplicate_slice_hashes,
        "failure_reasons": failure_reasons,
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


def _expected_capture_slices(
    cold: Mapping[str, object], warm: Mapping[str, object]
) -> tuple[
    dict[tuple[str, int], dict[str, object]],
    list[tuple[str, int]],
]:
    """Index every non-skip serial slice that a capture manifest must bind."""

    expected: dict[tuple[str, int], dict[str, object]] = {}
    collisions: list[tuple[str, int]] = []
    for boot_class, class_record in (("cold", cold), ("warm", warm)):
        logs = class_record.get("logs")
        if not isinstance(logs, list):
            continue
        for log in logs:
            if not isinstance(log, Mapping):
                continue
            resolved_path = log.get("resolved_path")
            serial_sha256 = log.get("sha256")
            boots = log.get("boots")
            if not isinstance(resolved_path, str) or not isinstance(boots, list):
                continue
            for boot in boots:
                if not isinstance(boot, Mapping) or boot.get("source_score") == "skip":
                    continue
                slice_index = boot.get("serial_slice_index")
                if not isinstance(slice_index, int) or isinstance(slice_index, bool):
                    continue
                key = (resolved_path, slice_index)
                if key in expected:
                    collisions.append(key)
                    continue
                expected[key] = {
                    "boot_class": boot_class,
                    "raw_slice_sha256": boot.get("raw_slice_sha256"),
                    "serial_sha256": serial_sha256,
                }
    return expected, sorted(set(collisions))


def _capture_manifest_record(
    path: Path,
    cold: Mapping[str, object],
    warm: Mapping[str, object],
    identity: Mapping[str, object],
    staged: Mapping[str, object],
    readback: Mapping[str, object],
    expected_git_commit: str,
    expected_build_id: str,
    expected_metadata_sha256: str,
) -> dict[str, object]:
    """Bind each boot slice to one unique run and one stable paired pcap."""

    try:
        manifest, manifest_snapshot = _read_stable_json(path, "capture manifest")
    except EvidenceReadError as error:
        return {
            "failure_reasons": ["capture-manifest-unreadable"],
            "identity_error": str(error),
            "path": str(path),
            "pcap_paths": [],
            "result": "FAIL",
        }

    failure_reasons: list[str] = []
    if manifest.get("schema") != CAPTURE_MANIFEST_SCHEMA:
        failure_reasons.append("capture-manifest-schema-invalid")
    if manifest.get("image_identity_metadata_sha256") != expected_metadata_sha256:
        failure_reasons.append("capture-manifest-identity-sha256-mismatch")
    if identity.get("sha256") != expected_metadata_sha256:
        failure_reasons.append("capture-manifest-identity-binding-invalid")
    runs = manifest.get("runs")
    if not isinstance(runs, list):
        runs = []
        failure_reasons.append("capture-manifest-runs-invalid")

    expected, expected_collisions = _expected_capture_slices(cold, warm)
    if expected_collisions:
        failure_reasons.append("capture-expected-slice-index-collision")
    matched: set[tuple[str, int]] = set()
    run_ids: set[str] = set()
    slice_hashes: set[str] = set()
    declared_pcap_paths: set[str] = set()
    seen_pcap_paths: set[str] = set()
    pcap_hashes: set[str] = set()
    pcap_inodes: set[tuple[int, int]] = set()
    build_epoch = identity.get("build_epoch")
    expected_image_id = identity.get("image_id")
    checked_runs: list[dict[str, object]] = []
    manifest_bytes = manifest_snapshot.get("data")
    protected_evidence: list[dict[str, object]] = [
        {
            "label": "capture-manifest",
            "resolved_path": manifest_snapshot.get("resolved_path"),
            "device": manifest_snapshot.get("device"),
            "inode": manifest_snapshot.get("inode"),
            "sha256": (
                hashlib.sha256(manifest_bytes).hexdigest()
                if isinstance(manifest_bytes, bytes)
                else None
            ),
        },
        {"label": "image-identity-metadata", **dict(identity)},
        {"label": "staged-image", **dict(staged)},
        {"label": "readback-image", **dict(readback)},
    ]
    for boot_class, class_record in (("cold", cold), ("warm", warm)):
        logs = class_record.get("logs")
        if not isinstance(logs, list):
            continue
        protected_evidence.extend(
            {"label": f"{boot_class}-serial-log", **dict(log)}
            for log in logs
            if isinstance(log, Mapping)
        )
    required_run_fields = {
        "run_id",
        "boot_class",
        "serial_path",
        "serial_sha256",
        "serial_slice_index",
        "serial_slice_sha256",
        "pcap_path",
        "pcap_sha256",
        "image_id",
        "git_commit",
        "build_id",
        "capture_epoch",
    }

    for index, raw_run in enumerate(runs):
        run_failures: list[str] = []
        if not isinstance(raw_run, Mapping):
            failure_reasons.append("capture-manifest-run-invalid")
            continue
        missing_fields = sorted(required_run_fields - set(raw_run))
        if missing_fields:
            run_failures.append("capture-run-fields-missing")
        run_id = raw_run.get("run_id")
        if not isinstance(run_id, str) or RUN_ID_RE.fullmatch(run_id) is None:
            run_failures.append("capture-run-id-invalid")
        elif run_id in run_ids:
            run_failures.append("capture-run-id-duplicate")
        else:
            run_ids.add(run_id)
        boot_class = raw_run.get("boot_class")
        if not isinstance(boot_class, str) or boot_class not in {"cold", "warm"}:
            run_failures.append("capture-run-class-invalid")
        serial_path_value = raw_run.get("serial_path")
        slice_index = raw_run.get("serial_slice_index")
        if not isinstance(serial_path_value, str) or not serial_path_value:
            run_failures.append("capture-run-serial-path-invalid")
            serial_resolved = ""
        else:
            serial_resolved = _resolved_path(Path(serial_path_value))
        if not isinstance(slice_index, int) or isinstance(slice_index, bool) or slice_index < 1:
            run_failures.append("capture-run-slice-index-invalid")
            key = (serial_resolved, -1)
        else:
            key = (serial_resolved, slice_index)
        expected_slice = expected.get(key)
        if expected_slice is None:
            run_failures.append("capture-run-orphan")
        elif key in matched:
            run_failures.append("capture-run-slice-duplicate")
        else:
            matched.add(key)
            if boot_class != expected_slice["boot_class"]:
                run_failures.append("capture-run-class-mismatch")
            if raw_run.get("serial_sha256") != expected_slice["serial_sha256"]:
                run_failures.append("capture-run-serial-sha256-mismatch")
            if raw_run.get("serial_slice_sha256") != expected_slice["raw_slice_sha256"]:
                run_failures.append("capture-run-slice-sha256-mismatch")

        slice_hash = raw_run.get("serial_slice_sha256")
        if not isinstance(slice_hash, str) or IMAGE_SHA256_RE.fullmatch(slice_hash) is None:
            run_failures.append("capture-run-slice-sha256-invalid")
        elif slice_hash in slice_hashes:
            run_failures.append("capture-run-slice-content-duplicate")
        else:
            slice_hashes.add(slice_hash)

        pcap_path_value = raw_run.get("pcap_path")
        pcap_record: dict[str, object] = {}
        if not isinstance(pcap_path_value, str) or not pcap_path_value:
            run_failures.append("capture-run-pcap-path-invalid")
        else:
            pcap_path = Path(pcap_path_value)
            pcap_declared_resolved = _resolved_path(pcap_path)
            if pcap_declared_resolved in declared_pcap_paths:
                run_failures.append("capture-run-pcap-path-duplicate")
            else:
                declared_pcap_paths.add(pcap_declared_resolved)
            try:
                pcap_snapshot = _read_stable_regular_file(pcap_path)
                pcap_data = pcap_snapshot["data"]
                assert isinstance(pcap_data, bytes)
                pcap_sha256 = hashlib.sha256(pcap_data).hexdigest()
                if raw_run.get("pcap_sha256") != pcap_sha256:
                    run_failures.append("capture-run-pcap-sha256-mismatch")
                pcap_resolved = str(pcap_snapshot["resolved_path"])
                pcap_inode = (
                    int(pcap_snapshot["device"]),
                    int(pcap_snapshot["inode"]),
                )
                if pcap_resolved in seen_pcap_paths:
                    if "capture-run-pcap-path-duplicate" not in run_failures:
                        run_failures.append("capture-run-pcap-path-duplicate")
                else:
                    seen_pcap_paths.add(pcap_resolved)
                if pcap_inode in pcap_inodes:
                    run_failures.append("capture-run-pcap-open-file-duplicate")
                else:
                    pcap_inodes.add(pcap_inode)
                if pcap_sha256 in pcap_hashes:
                    run_failures.append("capture-run-pcap-content-duplicate")
                else:
                    pcap_hashes.add(pcap_sha256)
                for evidence in protected_evidence:
                    if evidence.get("resolved_path") == pcap_resolved:
                        run_failures.append("capture-run-pcap-evidence-path-alias")
                    if (
                        evidence.get("device") == pcap_inode[0]
                        and evidence.get("inode") == pcap_inode[1]
                    ):
                        run_failures.append(
                            "capture-run-pcap-evidence-open-file-alias"
                        )
                    if evidence.get("sha256") == pcap_sha256:
                        run_failures.append("capture-run-pcap-evidence-content-alias")
                pcap_record = {
                    key: value for key, value in pcap_snapshot.items() if key != "data"
                }
                pcap_record["sha256"] = pcap_sha256
                try:
                    pcap_record.update(_inspect_classic_pcap(pcap_data))
                except EvidenceReadError as error:
                    run_failures.append("capture-run-pcap-format-invalid")
                    pcap_record["format_error"] = str(error)
                capture_epoch = raw_run.get("capture_epoch")
                if (
                    isinstance(capture_epoch, int)
                    and not isinstance(capture_epoch, bool)
                    and isinstance(pcap_record.get("first_packet_epoch"), int)
                    and pcap_record["first_packet_epoch"] != capture_epoch
                ):
                    run_failures.append("capture-run-pcap-epoch-mismatch")
            except EvidenceReadError as error:
                run_failures.append("capture-run-pcap-unreadable")
                pcap_record = {"identity_error": str(error)}

        if raw_run.get("image_id") != expected_image_id:
            run_failures.append("capture-run-image-id-mismatch")
        if raw_run.get("git_commit") != expected_git_commit:
            run_failures.append("capture-run-git-commit-mismatch")
        if raw_run.get("build_id") != expected_build_id:
            run_failures.append("capture-run-build-id-mismatch")
        capture_epoch = raw_run.get("capture_epoch")
        if not isinstance(capture_epoch, int) or isinstance(capture_epoch, bool):
            run_failures.append("capture-run-epoch-invalid")
        elif isinstance(build_epoch, int) and capture_epoch < build_epoch:
            run_failures.append("capture-run-before-build")

        run_failures = sorted(set(run_failures))
        if run_failures:
            failure_reasons.append("capture-run-invalid")
        checked_runs.append(
            {
                "index": index,
                "run_id": run_id,
                "failure_reasons": run_failures,
                "pcap": pcap_record,
                "result": "PASS" if not run_failures else "FAIL",
            }
        )

    missing = sorted(set(expected) - matched)
    if missing:
        failure_reasons.append("capture-manifest-slices-missing")
    if len(runs) != len(expected):
        failure_reasons.append("capture-manifest-run-count-mismatch")

    public_snapshot = {
        key: value for key, value in manifest_snapshot.items() if key != "data"
    }
    return {
        **public_snapshot,
        "failure_reasons": sorted(set(failure_reasons)),
        "path": str(path),
        "pcap_paths": sorted(declared_pcap_paths),
        "result": "PASS" if not failure_reasons else "FAIL",
        "runs": checked_runs,
        "slice_index_collisions": [
            {"serial_path": serial_path, "serial_slice_index": slice_index}
            for serial_path, slice_index in expected_collisions
        ],
        "slices_expected": len(expected),
        "slices_matched": len(matched),
    }


def build_report(
    *,
    cold_logs: Sequence[str],
    warm_logs: Sequence[str],
    image_sha256: str,
    build_marker: str,
    staged_image: str | Path,
    readback_image: str | Path,
    image_identity_metadata: str | Path,
    capture_manifest: str | Path,
    expected_git_commit: str,
    expected_build_id: str,
    expected_image_identity_sha256: str,
    required_cold_passes: int = DEFAULT_REQUIRED_PASSES,
    required_warm_passes: int = DEFAULT_REQUIRED_PASSES,
) -> dict[str, object]:
    """Build a deterministic repeatability report from supplied evidence."""

    normalized_sha256 = image_sha256.lower()
    image_valid = IMAGE_SHA256_RE.fullmatch(image_sha256) is not None
    build_marker_valid = _is_exact_build_marker(build_marker)
    distinct_artifact_paths = not _paths_alias(
        Path(staged_image), Path(readback_image)
    )
    staged = assess_readback_image(staged_image, normalized_sha256, build_marker)
    readback = assess_readback_image(readback_image, normalized_sha256, build_marker)
    cold = _class_record("cold", cold_logs, required_cold_passes, build_marker)
    warm = _class_record("warm", warm_logs, required_warm_passes, build_marker)
    evidence_inputs = _evidence_input_record(cold, warm)
    identity_metadata = _identity_metadata_record(
        Path(image_identity_metadata),
        normalized_sha256,
        build_marker,
        expected_git_commit,
        expected_build_id,
        expected_image_identity_sha256,
        staged,
    )
    capture = _capture_manifest_record(
        Path(capture_manifest),
        cold,
        warm,
        identity_metadata,
        staged,
        readback,
        expected_git_commit,
        expected_build_id,
        expected_image_identity_sha256,
    )
    marker_image_id_match = IMAGE_ID_RE.search(build_marker)
    marker_image_id = (
        marker_image_id_match.group("image_id")
        if marker_image_id_match is not None
        else None
    )

    failure_reasons: list[str] = []
    if not image_valid:
        failure_reasons.append("image-sha256-invalid")
    if not build_marker_valid:
        failure_reasons.append("build-marker-invalid")
    if GIT_COMMIT_RE.fullmatch(expected_git_commit) is None:
        failure_reasons.append("expected-git-commit-invalid")
    if BUILD_ID_RE.fullmatch(expected_build_id) is None:
        failure_reasons.append("expected-build-id-invalid")
    readback_status = str(readback["status"])
    readback_failure_by_status = {
        "missing": "readback-image-missing",
        "image-identity-invalid": "readback-image-identity-invalid",
        "hash-mismatch": "readback-image-hash-mismatch",
        "marker-absent": "readback-image-build-marker-absent",
        "marker-conflict": "readback-image-build-marker-conflict",
        "image-id-mismatch": "readback-image-id-mismatch",
    }
    readback_failure = readback_failure_by_status.get(readback_status)
    if readback_failure is not None:
        failure_reasons.append(readback_failure)
    if (
        int(readback.get("conflicting_marker_count", 0)) > 0
        and readback_failure != "readback-image-build-marker-conflict"
    ):
        failure_reasons.append("readback-image-build-marker-conflict")
    staged_failure_by_status = {
        "missing": "staged-image-missing",
        "image-identity-invalid": "staged-image-identity-invalid",
        "hash-mismatch": "staged-image-hash-mismatch",
        "marker-absent": "staged-image-build-marker-absent",
        "marker-conflict": "staged-image-build-marker-conflict",
        "image-id-mismatch": "staged-image-id-mismatch",
    }
    staged_failure = staged_failure_by_status.get(str(staged["status"]))
    if staged_failure is not None:
        failure_reasons.append(staged_failure)
    if (
        int(staged.get("conflicting_marker_count", 0)) > 0
        and staged_failure != "staged-image-build-marker-conflict"
    ):
        failure_reasons.append("staged-image-build-marker-conflict")
    distinct_open_files = not (
        staged.get("device") is not None
        and staged.get("device") == readback.get("device")
        and staged.get("inode") == readback.get("inode")
    )
    if not distinct_artifact_paths:
        failure_reasons.append("staged-readback-path-alias")
    if not distinct_open_files and distinct_artifact_paths:
        failure_reasons.append("staged-readback-open-file-alias")
    failure_reasons.extend(cold["failure_reasons"])  # type: ignore[arg-type]
    failure_reasons.extend(warm["failure_reasons"])  # type: ignore[arg-type]
    failure_reasons.extend(  # type: ignore[arg-type]
        evidence_inputs["failure_reasons"]
    )
    failure_reasons.extend(identity_metadata["failure_reasons"])  # type: ignore[arg-type]
    failure_reasons.extend(capture["failure_reasons"])  # type: ignore[arg-type]

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
            "identity_role": "staged-source-and-external-readback",
            "sha256": normalized_sha256,
            "valid": image_valid,
        },
        "evidence_inputs": evidence_inputs,
        "image_identity_metadata": identity_metadata,
        "capture_manifest": capture,
        "staged_image": staged,
        "readback_image": readback,
        "staged_readback_binding": {
            "distinct_paths": distinct_artifact_paths,
            "distinct_open_files": distinct_open_files,
            "sha256_match": (
                staged.get("sha256") is not None
                and staged.get("sha256") == readback.get("sha256")
            ),
            "valid": (
                distinct_artifact_paths
                and distinct_open_files
                and staged["verified"] is True
                and readback["verified"] is True
            ),
        },
        "identity_binding": {
            "scheme": image_identity.SCHEMA,
            "image_id": (
                marker_image_id
                if image_valid
                and build_marker_valid
                and staged["verified"] is True
                and readback["verified"] is True
                and distinct_artifact_paths
                and distinct_open_files
                else None
            ),
            "valid": (
                image_valid
                and build_marker_valid
                and staged["verified"] is True
                and readback["verified"] is True
                and distinct_artifact_paths
                and distinct_open_files
                and staged.get("image_id") == marker_image_id
                and readback.get("image_id") == marker_image_id
                and identity_metadata.get("result") == "PASS"
                and capture.get("result") == "PASS"
            ),
        },
        "result": "PASS" if not failure_reasons else "FAIL",
        "schema": SCHEMA,
    }


def build_parser() -> argparse.ArgumentParser:
    """Build the repeatability verifier command-line parser."""

    parser = argparse.ArgumentParser(
        description=(
            "Verify blocker-free WiFi boot slices by matching staged and "
            "external-readback image SHA-256 identities plus the exact [BUILD] "
            "marker emitted in every counted serial boot."
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
        "--staged-image",
        required=True,
        type=Path,
        help="source image artifact copied to the target medium",
    )
    parser.add_argument(
        "--readback-image",
        required=True,
        type=Path,
        help="exact image bytes read back from the staged media",
    )
    parser.add_argument(
        "--image-identity-metadata",
        required=True,
        type=Path,
        help="required clean exact-image pi4-image-identity.json sidecar",
    )
    parser.add_argument(
        "--capture-manifest",
        required=True,
        type=Path,
        help="capture manifest binding every boot slice to one paired pcap",
    )
    parser.add_argument(
        "--expected-git-commit",
        required=True,
        help="full lowercase clean Git commit expected for this evidence set",
    )
    parser.add_argument(
        "--expected-build-id",
        required=True,
        help="canonical derived build ID expected for this evidence set",
    )
    parser.add_argument(
        "--expected-image-identity-sha256",
        required=True,
        help=(
            "independently preserved SHA-256 of the exact "
            "pi4-image-identity.json produced by the clean image build"
        ),
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

    parser = build_parser()
    args = parser.parse_args(argv)
    if args.output is not None:
        evidence_inputs = [
            ("--staged-image", args.staged_image),
            ("--readback-image", args.readback_image),
            ("--image-identity-metadata", args.image_identity_metadata),
            ("--capture-manifest", args.capture_manifest),
            *(("--cold-log", Path(path)) for path in args.cold_log),
            *(("--warm-log", Path(path)) for path in args.warm_log),
        ]
        output_alias = _output_input_alias(args.output, evidence_inputs)
        if output_alias is not None:
            option, input_path = output_alias
            parser.error(
                f"--output must not alias evidence input {option}={input_path}"
            )
    report = build_report(
        cold_logs=args.cold_log,
        warm_logs=args.warm_log,
        image_sha256=args.image_sha256,
        build_marker=args.build_marker,
        staged_image=args.staged_image,
        readback_image=args.readback_image,
        image_identity_metadata=args.image_identity_metadata,
        capture_manifest=args.capture_manifest,
        expected_git_commit=args.expected_git_commit,
        expected_build_id=args.expected_build_id,
        expected_image_identity_sha256=args.expected_image_identity_sha256,
        required_cold_passes=args.required_cold_passes,
        required_warm_passes=args.required_warm_passes,
    )
    if args.output is not None:
        capture_record = report.get("capture_manifest")
        pcap_paths = (
            capture_record.get("pcap_paths", [])
            if isinstance(capture_record, Mapping)
            else []
        )
        pcap_inputs = [
            ("capture-manifest pcap", Path(path))
            for path in pcap_paths
            if isinstance(path, str)
        ]
        output_alias = _output_input_alias(args.output, pcap_inputs)
        if output_alias is not None:
            option, input_path = output_alias
            parser.error(f"--output must not alias evidence input {option}={input_path}")
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output is not None:
        _atomic_write_report(args.output, rendered)
    sys.stdout.write(rendered)
    return 0 if report["result"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
