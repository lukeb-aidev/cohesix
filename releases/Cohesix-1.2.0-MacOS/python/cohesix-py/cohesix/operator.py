# Author: Lukas Bower
# Purpose: Consume canonical operator artifacts without executing, replaying or promoting evidence.
# Copyright 2026 Lukas Bower

"""Bounded, non-authoritative readers for canonical Rust operator artifacts."""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any, Mapping

from .errors import CohesixError
from .generated import DEFAULTS


def artifact_limit() -> int:
    """Return the compiler-owned input limit shared with the Rust operator tools."""
    return int(DEFAULTS["diagnostic_artifacts"]["max_bytes"])


def read_artifact(root: Path, relative: str) -> bytes:
    """Read a bounded regular pack file without following directory or file links."""
    if (
        not relative
        or len(relative.encode("utf-8")) > DEFAULTS["console"]["max_path_len"]
        or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in relative)
    ):
        raise CohesixError("pack-path")
    path = Path(relative)
    if path.is_absolute() or any(
        part in (".", "..") for part in relative.split("/")
    ):
        raise CohesixError("pack-path-traversal")
    current = root
    for part in path.parts:
        current = current / part
        if current.is_symlink():
            raise CohesixError("pack-path-symlink")
    if not current.is_file() or current.stat().st_size > artifact_limit():
        raise CohesixError("artifact-size-or-type")
    with current.open("rb") as stream:
        payload = stream.read(artifact_limit() + 1)
    if len(payload) > artifact_limit():
        raise CohesixError("artifact-size-bound")
    return payload


def redact_value(value: Any) -> Any:
    """Match the canonical field-aware redaction, including embedded JSON strings."""
    def sensitive(key: str) -> bool:
        key = key.lower()
        return any(part in key for part in (
            "token", "authorization", "secret", "password", "signing_key",
            "api_key", "credential", "private_key",
        )) or key in ("auth_ref", "auth", "ticket")

    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            if key == "ticket":
                if isinstance(item, str):
                    if item == "none" or re.fullmatch(r"sha256:[0-9a-fA-F]{64}", item):
                        result[key] = item
                    else:
                        result[key] = "sha256:" + hashlib.sha256(item.encode()).hexdigest()
                else:
                    result[key] = "<redacted>"
            elif sensitive(key):
                result[key] = "<redacted>"
            else:
                result[key] = redact_value(item)
        return result
    if isinstance(value, list):
        return [redact_value(item) for item in value]
    if isinstance(value, str):
        try:
            embedded = json.loads(value)
        except (ValueError, RecursionError):
            if any(sensitive(part) for part in re.split(r"[\s=:\"']", value)):
                return "<redacted>"
        else:
            if isinstance(embedded, (dict, list)):
                return json.dumps(
                    redact_value(embedded), sort_keys=True,
                    ensure_ascii=False, separators=(",", ":"),
                )
    return value


def read_case(root: Path) -> Mapping[str, Any]:
    """Read an offline case review, validating source links without receipt authority."""
    raw = read_artifact(root, "case.json")
    try:
        value = json.loads(raw)
    except (ValueError, RecursionError) as exc:
        raise CohesixError("case-json") from exc
    if (
        not isinstance(value, dict)
        or value.get("schema") != "cohesix-evidence-pack/case-v1"
    ):
        raise CohesixError("case-schema")
    if value.get("evidence_class") != "offline-review" or value.get("proof") != "none":
        raise CohesixError("case-proof-promotion")
    if value.get("scenario") not in (
        "generic", "incident", "change", "maintenance", "rollout", "federation",
    ):
        raise CohesixError("case-scenario")
    timeline = read_artifact(root, "timeline.ndjson")
    if len(raw) + len(timeline) > artifact_limit():
        raise CohesixError("case-total-bound")
    lines = timeline.splitlines()
    chains = value.get("chains")
    if not isinstance(chains, list):
        raise CohesixError("case-chains")
    for chain in chains:
        if not isinstance(chain, dict) or chain.get("outcome") not in (
            "recorded-terminal", "refused", "deadlettered", "incomplete", "ambiguous",
        ):
            raise CohesixError("case-outcome")
        stages = chain.get("stages")
        if not isinstance(stages, list):
            raise CohesixError("case-stages")
        for stage in stages:
            if not isinstance(stage, dict) or stage.get("status") not in (
                "observed", "missing", "error", "unknown", "ambiguous",
            ):
                raise CohesixError("case-stage")
            sources = stage.get("sources")
            if not isinstance(sources, list):
                raise CohesixError("case-sources")
            for source in sources:
                if not isinstance(source, dict):
                    raise CohesixError("case-source")
                index = source.get("timeline_event")
                if type(index) is not int or not 0 <= index < len(lines):
                    raise CohesixError("case-event-index")
                if hashlib.sha256(lines[index]).hexdigest() != source.get(
                    "event_sha256"
                ):
                    raise CohesixError("case-event-digest")
                try:
                    event = json.loads(lines[index])
                except (ValueError, RecursionError) as exc:
                    raise CohesixError("case-event-json") from exc
                if (
                    not isinstance(event, dict)
                    or event.get("source") != source.get("path")
                ):
                    raise CohesixError("case-event-source")
    return redact_value(value)


def read_attestation_result(
    root: Path, relative: str = "attestation.json",
) -> Mapping[str, Any]:
    """Project the canonical record structurally; this does not verify any signature."""
    try:
        value = json.loads(read_artifact(root, relative))
    except (ValueError, RecursionError) as exc:
        raise CohesixError("attestation-result-json") from exc
    if (
        not isinstance(value, dict)
        or value.get("schema") != "cohesix-attestation-result/v1"
    ):
        raise CohesixError("attestation-result-schema")
    verdict = value.get("verdict")
    if verdict not in ("PASS", "FAIL", "UNAVAILABLE"):
        raise CohesixError("attestation-verdict")
    if value.get("source_class") not in (
        "offline-pack", "live-console", "host-projection", "mock",
    ):
        raise CohesixError("attestation-source-class")
    scope = value.get("proof_scope", "none")
    if verdict == "PASS":
        verified = value.get("verified")
        if not isinstance(verified, dict) or scope not in (
            "offline-signature", "live-signature",
        ):
            raise CohesixError("attestation-verification-metadata")
        evidence_class = value.get("evidence_class")
        if evidence_class not in ("offline-fixture", "qemu-virtual", "pi4-device"):
            raise CohesixError("attestation-evidence-class")
        if verified.get("evidence_class") != evidence_class:
            raise CohesixError("attestation-evidence-class")
        if scope == "live-signature" and (
            evidence_class == "offline-fixture"
            or value["source_class"] not in ("live-console", "host-projection")
        ):
            raise CohesixError("attestation-proof-scope")
        if scope == "offline-signature" and value["source_class"] != "offline-pack":
            raise CohesixError("attestation-proof-scope")
        expected_reason = (
            "valid-recorded-signature" if scope == "offline-signature"
            else "valid-fresh-signature"
        )
        if value.get("reason") != expected_reason:
            raise CohesixError("attestation-reason")
        for field in ("evidence_sha256", "trust_policy_sha256", "nonce_sha256"):
            if not isinstance(verified.get(field), str) or not re.fullmatch(
                r"[0-9a-f]{64}", verified[field],
            ):
                raise CohesixError("attestation-verification-digest")
        for field, maximum in (("clock", 2**64 - 1), ("reset_count", 2**32 - 1),
                               ("restart_count", 2**32 - 1)):
            if type(verified.get(field)) is not int or not 0 <= verified[field] <= maximum:
                raise CohesixError("attestation-verification-counter")
    else:
        if scope != "none" or value.get("verified") is not None:
            raise CohesixError("attestation-proof-scope")
        if value.get("evidence_class") not in ("measurement-only", "unavailable"):
            raise CohesixError("attestation-evidence-class")
        if not isinstance(value.get("reason"), str) or not re.fullmatch(
            r"[a-z][a-z0-9-]{0,63}", value["reason"],
        ):
            raise CohesixError("attestation-reason")
    return redact_value(value)


def trace_reference(
    root: Path, relative: str, *, sha256: str, size: int,
) -> Mapping[str, Any]:
    """Check opaque trace identity and length without parsing or replaying its payload."""
    if (
        not isinstance(sha256, str)
        or not re.fullmatch(r"[0-9a-f]{64}", sha256)
        or type(size) is not int or size < 0
    ):
        raise CohesixError("trace-reference-identity")
    payload = read_artifact(root, relative)
    if len(payload) != size or hashlib.sha256(payload).hexdigest() != sha256:
        raise CohesixError("trace-reference-mismatch")
    return {
        "path": relative, "bytes": size, "sha256": sha256,
        "source_class": "offline-trace", "proof": "none",
    }
