# Author: Lukas Bower
# Purpose: Verify release archive integrity, SD geometry and complete installation evidence.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import tarfile

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import release_qualify as qualify  # noqa: E402


def sd_fixture(path: Path) -> dict:
    """Encode independent MBR/BPB fields without simulating Pi execution."""
    size = 2 * 1024 * 1024
    payload = bytearray(size)
    payload[450] = 0x0C
    struct.pack_into("<II", payload, 454, 2048, 2048)
    payload[510:512] = b"\x55\xaa"
    payload[1048576 + 82 : 1048576 + 90] = b"FAT32   "
    payload[1048576 + 510 : 1048576 + 512] = b"\x55\xaa"
    path.write_bytes(payload)
    return {
        "schema": "cohesix-pi4-portable-sd-image/v2",
        "minimum_target_bytes": size,
        "image_size_bytes": size,
        "image_sha256": hashlib.sha256(payload).hexdigest(),
        "sector_size_bytes": 512,
        "partition_start_lba": 2048,
        "partition_sector_count": 2048,
    }


def test_sd_layout_rejects_capacity_and_geometry_drift(tmp_path: Path) -> None:
    image = tmp_path / "pi4.img"
    metadata = sd_fixture(image)
    qualify.verify_sd_layout(image, metadata)
    with pytest.raises(ValueError, match="capacity"):
        qualify.verify_sd_layout(
            image, {**metadata, "minimum_target_bytes": image.stat().st_size - 512}
        )
    with image.open("r+b") as handle:
        handle.seek(462)
        handle.write(b"\x01")
    metadata["image_sha256"] = qualify.digest(image)
    with pytest.raises(ValueError, match="exactly the compact"):
        qualify.verify_sd_layout(image, metadata)


def test_readback_rejects_an_ordinary_image_file(tmp_path: Path) -> None:
    image = tmp_path / "image/cohesix-pi4-sd.img"
    image.parent.mkdir()
    sd_fixture(image)
    with pytest.raises(ValueError, match="physical whole-disk"):
        qualify.media_readback(argparse.Namespace(bundle=tmp_path, device=image), {})


@pytest.fixture
def bundle_fixture(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Create an independently specified minimal host packaging contract."""
    bundle = tmp_path / "Cohesix-1.0.0-beta-MacOS"
    bundle.mkdir()
    (bundle / "VERSION.txt").write_text("1.0.0-beta\n")
    (bundle / "BUILD_PROVENANCE.json").write_text(
        json.dumps(
            {
                "schema": "cohesix-release-build-provenance/v1",
                "host": "macos",
                "source_commit": "c" * 40,
                "files": {},
            }
        )
    )
    names = ["VERSION.txt", "BUILD_PROVENANCE.json"]
    (bundle / "MANIFEST.sha256").write_text(
        "".join(f"{qualify.digest(bundle / name)}  {name}\n" for name in names)
    )
    generated = tmp_path / "configs/generated"
    generated.mkdir(parents=True)
    (generated / "implementation_surface_inventory.json").write_text(
        json.dumps(
            {
                "release": {
                    "version": "1.0.0-beta",
                    "expected_bundle_files": names + ["MANIFEST.sha256"],
                    "host_tools": [],
                    "target_images": [],
                }
            }
        )
    )
    monkeypatch.setattr(qualify, "ROOT", tmp_path)
    archive = tmp_path / f"{bundle.name}.tar.gz"
    with tarfile.open(archive, "w:gz") as handle:
        handle.add(bundle, arcname=bundle.name)
    return bundle, archive


def test_extracted_bundle_and_archive_must_match(bundle_fixture) -> None:
    bundle, archive = bundle_fixture
    record = qualify.inspect_bundle(bundle, archive)
    assert record["version"] == "1.0.0-beta"
    (bundle / "VERSION.txt").write_text("0.1.0-alpha1\n")
    with pytest.raises(ValueError, match="VERSION"):
        qualify.inspect_bundle(bundle, archive)


def test_archive_links_are_rejected_even_with_matching_file_names(
    bundle_fixture,
) -> None:
    bundle, archive = bundle_fixture
    with tarfile.open(archive, "w:gz") as handle:
        member = tarfile.TarInfo(f"{bundle.name}/VERSION.txt")
        member.type = tarfile.SYMTYPE
        member.linkname = "/etc/passwd"
        handle.addfile(member)
    with pytest.raises(ValueError, match="link"):
        qualify.inspect_bundle(bundle, archive)


def test_archive_payload_drift_is_rejected(bundle_fixture) -> None:
    bundle, archive = bundle_fixture
    with tarfile.open(archive, "w:gz") as handle:
        member = tarfile.TarInfo(f"{bundle.name}/VERSION.txt")
        member.size = 5
        handle.addfile(member, io.BytesIO(b"wrong"))
    with pytest.raises(ValueError, match="archive differs"):
        qualify.inspect_bundle(bundle, archive)


def test_qualification_logs_are_content_bound(tmp_path: Path) -> None:
    log = tmp_path / "tcp.log"
    log.write_text("fixture authenticated operation\n")
    record = {
        "schema": qualify.SCHEMA,
        "kind": "macos",
        "status": "pass",
        "logs": [qualify.file_record(log)],
    }
    record["result_sha256"] = hashlib.sha256(
        qualify.evidence.canonical_bytes(record)
    ).hexdigest()
    output = tmp_path / "result.json"
    output.write_text(json.dumps(record))
    assert qualify.read_result(output, "macos")["kind"] == "macos"
    with pytest.raises(ValueError, match="passing pi4"):
        qualify.read_result(output, "pi4")
    log.write_text("changed")
    with pytest.raises(ValueError, match="attachment changed"):
        qualify.read_result(output, "macos")


def test_failed_check_cannot_emit_a_passing_command_log(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="check failed"):
        qualify.run(
            [sys.executable, "-c", "raise SystemExit(7)"],
            tmp_path,
            tmp_path / "command.log",
            {},
            timeout=5,
        )


def test_installation_checks_cannot_change_qualified_payload(bundle_fixture) -> None:
    bundle, _ = bundle_fixture
    manifest_hash = qualify.digest(bundle / "MANIFEST.sha256")
    qualify.verify_tested_bytes(bundle, manifest_hash)
    (bundle / "VERSION.txt").write_text("different bytes\n")
    with pytest.raises(ValueError, match="payload changed"):
        qualify.verify_tested_bytes(bundle, manifest_hash)


@pytest.fixture
def release_results(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Specify the required three archives and independent passing receipts."""
    version, commit = "1.0.0-beta", "b" * 40
    generated = tmp_path / "configs/generated"
    generated.mkdir(parents=True)
    (generated / "implementation_surface_inventory.json").write_text(
        json.dumps({"release": {"version": version}})
    )
    monkeypatch.setattr(qualify, "ROOT", tmp_path)
    monkeypatch.setattr(qualify.subprocess, "check_output", lambda *a, **k: commit)
    releases = tmp_path / "releases"
    releases.mkdir()
    paths = {}
    for kind, suffix in (("macos", "MacOS"), ("linux", "linux"), ("pi4", "Pi4")):
        archive = releases / f"Cohesix-{version}-{suffix}.tar.gz"
        archive.write_bytes(f"independent {kind} archive bytes".encode())
        result = {
            "schema": qualify.SCHEMA,
            "kind": kind,
            "status": "pass",
            "checks": qualify.PI_CHECKS if kind == "pi4" else qualify.HOST_CHECKS,
            "version": version,
            "source_commit": commit,
            "archive": qualify.file_record(archive),
            "logs": [],
        }
        result["result_sha256"] = hashlib.sha256(
            qualify.evidence.canonical_bytes(result)
        ).hexdigest()
        path = tmp_path / f"{kind}.json"
        path.write_text(json.dumps(result))
        paths[f"{kind}_result"] = path
    return argparse.Namespace(releases_dir=releases, **paths)


def test_final_gate_requires_and_binds_all_three_results(release_results) -> None:
    record = qualify.verify_release(release_results)
    assert record["checks"] == ["macos", "linux", "pi4"]
    assert len(record["qualifications"]) == 3
    assert record["qualifications"][2]["sha256"] == qualify.digest(
        release_results.pi4_result
    )
    release_results.pi4_result.unlink()
    with pytest.raises(qualify.evidence.EvidenceError):
        qualify.verify_release(release_results)


def test_final_gate_rejects_archives_changed_after_testing(release_results) -> None:
    archive = next(release_results.releases_dir.glob("*-linux.tar.gz"))
    archive.write_bytes(b"rebuilt after qualification")
    with pytest.raises(ValueError, match="archive changed"):
        qualify.verify_release(release_results)


def test_final_gate_rejects_stale_source(release_results, monkeypatch) -> None:
    monkeypatch.setattr(qualify.subprocess, "check_output", lambda *a, **k: "f" * 40)
    with pytest.raises(ValueError, match="current selected source"):
        qualify.verify_release(release_results)
