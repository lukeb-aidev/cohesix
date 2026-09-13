# Author: Lukas Bower
# Purpose: Check shared operator fixtures without creating a second evidence or trace authority.
# Copyright 2026 Lukas Bower

"""Cross-language redaction, source-link and non-attestation contracts."""

import hashlib
import json
import shutil
from pathlib import Path

import pytest

from cohesix.errors import CohesixError
from cohesix.operator import (
    read_artifact,
    read_attestation_result,
    read_case,
    redact_value,
    trace_reference,
)

FIXTURES = Path(__file__).resolve().parents[3] / "tests/fixtures/operator"


def test_canonical_redaction_fixture():
    contract = json.loads((FIXTURES / "contracts.json").read_text())
    assert redact_value(contract["redaction"]["input"]) == contract["redaction"]["expected"]


def test_canonical_case_links_and_unknown_additive_fields(tmp_path):
    shutil.copytree(FIXTURES / "case", tmp_path / "case")
    root = tmp_path / "case"
    case = read_case(root)
    assert case["proof"] == "none"
    assert case["chains"][0]["outcome"] == "recorded-terminal"
    value = json.loads((root / "case.json").read_text())
    value["future"] = {"auth_token": "CANARY"}
    (root / "case.json").write_text(json.dumps(value))
    assert read_case(root)["future"] == {"auth_token": "<redacted>"}
    value["chains"][0]["stages"][0]["sources"][0]["event_sha256"] = "0" * 64
    (root / "case.json").write_text(json.dumps(value))
    with pytest.raises(CohesixError, match="case-event-digest"):
        read_case(root)


def test_attestation_records_cannot_be_upgraded_by_python(tmp_path):
    contract = json.loads((FIXTURES / "contracts.json").read_text())
    for record in contract["attestation"]:
        (tmp_path / "attestation.json").write_text(json.dumps(record))
        assert read_attestation_result(tmp_path) == record
        record["verdict"] = "PASS"
        (tmp_path / "attestation.json").write_text(json.dumps(record))
        with pytest.raises(CohesixError, match="attestation-verification-metadata"):
            read_attestation_result(tmp_path)


def test_signed_result_projects_the_same_fixture_without_live_authority(tmp_path):
    record = json.loads((FIXTURES / "attestation/result.json").read_text())
    path = tmp_path / "attestation.json"
    path.write_text(json.dumps(record))
    assert read_attestation_result(tmp_path) == record
    assert record["proof_scope"] == "offline-signature"
    record["proof_scope"] = "live-signature"
    path.write_text(json.dumps(record))
    with pytest.raises(CohesixError, match="attestation-proof-scope"):
        read_attestation_result(tmp_path)
    record["proof_scope"] = "offline-signature"
    record["evidence_class"] = "pi4-device"
    path.write_text(json.dumps(record))
    with pytest.raises(CohesixError, match="attestation-evidence-class"):
        read_attestation_result(tmp_path)


def test_trace_references_are_opaque_bounded_and_confined(tmp_path):
    payload = b"opaque canonical trace reference fixture"
    path = tmp_path / "capture.trace"
    path.write_bytes(payload)
    digest = hashlib.sha256(payload).hexdigest()
    assert trace_reference(tmp_path, path.name, sha256=digest, size=len(payload))["proof"] == "none"
    path.write_bytes(payload[:-1])
    with pytest.raises(CohesixError, match="trace-reference-mismatch"):
        trace_reference(tmp_path, path.name, sha256=digest, size=len(payload))
    with pytest.raises(CohesixError, match="pack-path-traversal"):
        trace_reference(tmp_path, "../capture.trace", sha256=digest, size=len(payload))


@pytest.mark.parametrize("relative", ["../secret", "x" * 97, "bad\x7fpath"])
def test_artifact_paths_fail_closed(tmp_path, relative):
    with pytest.raises(CohesixError, match="pack-path"):
        read_artifact(tmp_path, relative)
