# Author: Lukas Bower
# Purpose: Reject stale, foreign-profile and untested payloads at the release boundary.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import release_inputs as release  # noqa: E402


def fixture_module():
    """Use the existing immutable-artifact fixture writer."""
    spec = importlib.util.spec_from_file_location(
        "release_artifact_fixtures",
        ROOT / "scripts/ci/test_qemu_artifact.py",
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture
def accepted(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Create a hash-bound TCP result and its separate native artifact."""
    support = fixture_module()
    helper = release.evidence
    monkeypatch.setattr(helper.platform, "system", lambda: "Darwin")
    inputs = support.create_artifact_inputs(tmp_path)
    artifact = tmp_path / "artifact.json"
    support.record_artifact(helper, inputs, artifact)
    log = tmp_path / "tcp.log"
    log.write_text("fixture: authenticated boot smoke passed\n")
    result = tmp_path / "result.json"
    source = "sha256:" + "a" * 64
    assert (
        helper.main(
            [
                "result",
                "--output",
                str(result),
                "--action-id",
                "qemu.tcp-regression",
                "--catalog-action-digest",
                support.CATALOG_DIGEST,
                "--claim-tier",
                "qemu-integration",
                "--target",
                "qemu",
                "--source-digest",
                source,
                "--evidence-root",
                str(tmp_path),
                "--artifact-manifest",
                str(artifact),
                "--artifact-action-id",
                "stage-03-qemu-tcp",
                "--artifact-catalog-action-digest",
                support.CATALOG_DIGEST,
                "--boot-id",
                "fixture-boot",
                "--group",
                "base",
                "--status",
                "pass",
                "--script",
                "boot_v0.coh",
                "--log",
                str(log),
            ]
        )
        == 0
    )
    return artifact, result, source, inputs


def test_archival_inspection_preserves_local_launch_checks(accepted, monkeypatch):
    artifact, result, source, inputs = accepted
    # A release assembler may inspect retained bytes on another host without
    # treating that host as capable of launching the recorded native runtime.
    inputs["qemu"].unlink()
    monkeypatch.setattr(release.evidence.platform, "system", lambda: "Linux")
    record = release.verified_inputs(artifact, result, source, "macos")
    assert record["sel4"]["timer_clock_hz"] == 24_000_000
    with pytest.raises(release.evidence.EvidenceError, match="verifying host"):
        release.evidence.verify_artifact_document(artifact)
    with pytest.raises(
        release.evidence.EvidenceError, match="wrong production profile"
    ):
        release.verified_inputs(artifact, result, source, "linux")


@pytest.mark.parametrize(
    "relative",
    ["host-tools/coh", "staging/rootserver", "staging/cohesix/manifest.json"],
)
def test_stale_host_or_guest_bytes_are_rejected(accepted, relative):
    artifact, result, source, inputs = accepted
    (inputs["artifact"] / relative).write_bytes(b"stale release artifact")
    with pytest.raises(release.evidence.EvidenceError, match="mismatch"):
        release.verified_inputs(artifact, result, source, "macos")


def test_wrong_source_and_changed_tcp_evidence_are_rejected(accepted):
    artifact, result, source, _ = accepted
    with pytest.raises(release.evidence.EvidenceError, match="source digest"):
        release.verified_inputs(artifact, result, "sha256:" + "b" * 64, "macos")
    (result.parent / "tcp.log").write_text("changed")
    with pytest.raises(release.evidence.EvidenceError, match="log size mismatch"):
        release.verified_inputs(artifact, result, source, "macos")


def test_regression_manifest_cannot_replace_default_release_manifest(accepted):
    artifact, result, source, _ = accepted
    with pytest.raises(
        release.evidence.EvidenceError, match="selected default manifest"
    ):
        release.verified_inputs(
            artifact,
            result,
            source,
            "macos",
            "sha256:" + "f" * 64,
        )


def test_packaged_payload_cannot_be_rebuilt_after_acceptance(accepted, tmp_path):
    artifact, result, source, inputs = accepted
    record = release.verified_inputs(artifact, result, source, "macos")
    records = release.payload_records(record)
    bundle = tmp_path / "bundle"
    for destination in records:
        origin = release.IMAGE_PATHS.get(
            destination, f"host-tools/{Path(destination).name}"
        )
        path = bundle / destination
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes((inputs["artifact"] / origin).read_bytes())
    release.verify_payload(bundle, records)
    (bundle / "bin/coh").write_bytes(b"another build")
    with pytest.raises(release.evidence.EvidenceError, match="mismatch"):
        release.verify_payload(bundle, records)


def test_another_valid_artifact_does_not_satisfy_the_tcp_binding(accepted):
    artifact, result, source, _ = accepted
    document = json.loads(artifact.read_text())
    document["action_id"] = "another-build"
    document["artifact_id"] = release.evidence.sha256_bytes(
        release.evidence.canonical_bytes(
            release.evidence.artifact_identity_material(document)
        ),
    )
    other = artifact.with_name("other.json")
    other.write_text(json.dumps(document))
    with pytest.raises(
        release.evidence.EvidenceError, match="differs from the artifact"
    ):
        release.verified_inputs(other, result, source, "macos")


@pytest.mark.parametrize(
    ("field", "value"),
    [("group", "diagnostic"), ("scripts", ["status.coh"])],
)
def test_release_requires_the_default_boot_regression(accepted, field, value):
    artifact, result, source, _ = accepted
    document = json.loads(result.read_text())
    document[field] = value
    document["result_id"] = release.evidence.sha256_bytes(
        release.evidence.canonical_bytes(
            release.evidence.result_identity_material(document)
        )
    )
    result.write_text(json.dumps(document))
    with pytest.raises(release.evidence.EvidenceError, match="default base TCP"):
        release.verified_inputs(artifact, result, source, "macos")
