# Author: Lukas Bower
# Purpose: Verify native installer staging rejects altered release bytes and preserves the selected payload boundary.
# Copyright 2026 Lukas Bower
"""Focused checks for exact native installer payload selection."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/install"))
import native_package  # noqa: E402
from native_package import classify, normalize_public_modes, stage_subset, write_manifest  # noqa: E402


def fixture_bundle(root: Path) -> Path:
    """Write a minimal bounded bundle manifest with distinct install subsets."""
    bundle = root / "bundle"
    files = {
        "bin/coh": b"controller",
        "bin/swarmui": b"desktop",
        "ui/swarmui/index.html": b"<html></html>",
        "nemo/dist/cohesix_nemo_kit-0.1.0-py3-none-any.whl": b"kit",
        "qemu/run.sh": b"guest",
    }
    lines = []
    for relative, contents in files.items():
        path = bundle / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(contents)
        if relative.startswith("bin/") or relative == "qemu/run.sh":
            path.chmod(0o755)
        lines.append(f"{hashlib.sha256(contents).hexdigest()}  {relative}")
    (bundle / "MANIFEST.sha256").write_text("\n".join(lines) + "\n")
    return bundle


def test_stage_separates_headless_desktop_and_optional_guest(tmp_path: Path) -> None:
    bundle = fixture_bundle(tmp_path)
    controller = stage_subset(bundle, tmp_path / "controller", "controller")
    desktop = stage_subset(bundle, tmp_path / "desktop", "desktop")
    assert [row["path"] for row in controller] == [
        "bin/coh", "nemo/dist/cohesix_nemo_kit-0.1.0-py3-none-any.whl",
    ]
    assert {row["path"] for row in desktop} == {
        "bin/swarmui", "ui/swarmui/index.html"
    }
    assert not (tmp_path / "controller/bin/swarmui").exists()
    assert not (tmp_path / "controller/qemu").exists()
    assert (tmp_path / "controller/bin/coh").stat().st_mode & 0o111
    assert classify("qemu/run.sh") == "optional-guest"
    reference = {
        "host": "linux", "version": "1.2.0-beta",
        "source_commit": "a" * 40, "archive_sha256": "b" * 64,
    }
    manifest = tmp_path / "controller/payload.json"
    write_manifest(manifest, reference=reference,
                   records=controller, package="cohesix-controller")
    payload = json.loads(manifest.read_text())
    assert payload["files"] == controller
    assert payload["source_commit"] == reference["source_commit"]


def test_stage_refuses_changed_bytes_and_symlink(tmp_path: Path) -> None:
    bundle = fixture_bundle(tmp_path)
    (bundle / "bin/coh").write_bytes(b"changed")
    with pytest.raises(ValueError, match="differs from release manifest"):
        stage_subset(bundle, tmp_path / "changed", "controller")
    (bundle / "bin/coh").unlink()
    (bundle / "bin/coh").symlink_to("swarmui")
    with pytest.raises(ValueError, match="unsafe installer payload path"):
        stage_subset(bundle, tmp_path / "linked", "controller")


def test_stage_refuses_ambiguous_destination_and_subset(tmp_path: Path) -> None:
    bundle = fixture_bundle(tmp_path)
    destination = tmp_path / "existing"
    destination.mkdir()
    with pytest.raises(ValueError, match="already exists"):
        stage_subset(bundle, destination, "controller")
    with pytest.raises(ValueError, match="unknown installer payload subset"):
        stage_subset(bundle, tmp_path / "unknown", "other")


def test_public_modes_reject_links_and_preserve_executable_intent(tmp_path: Path) -> None:
    root = tmp_path / "root"
    binary = root / "bin/coh"
    binary.parent.mkdir(parents=True, mode=0o700)
    binary.write_bytes(b"tool")
    binary.chmod(0o700)
    note = root / "README.md"
    note.write_text("guide")
    note.chmod(0o600)
    normalize_public_modes(root)
    assert binary.parent.stat().st_mode & 0o777 == 0o755
    assert binary.stat().st_mode & 0o777 == 0o755
    assert note.stat().st_mode & 0o777 == 0o644
    (root / "linked").symlink_to(note)
    with pytest.raises(ValueError, match="symlinks"):
        normalize_public_modes(root)


@pytest.mark.parametrize("relative", ["../escape", "bin/./coh", "bin//coh", "bin\\coh"])
def test_stage_refuses_noncanonical_manifest_paths(tmp_path: Path, relative: str) -> None:
    bundle = fixture_bundle(tmp_path)
    (bundle / "MANIFEST.sha256").write_text(f"{'a' * 64}  {relative}\n")
    with pytest.raises(ValueError, match="unsafe installer payload path"):
        stage_subset(bundle, tmp_path / "bad", "controller")


def test_reference_requires_version_aligned_wheel(
    tmp_path: Path, monkeypatch,
) -> None:
    bundle = tmp_path / "bundle"
    wheel_dir = bundle / "python/dist"
    wheel_dir.mkdir(parents=True)
    archive = tmp_path / "release.tar.gz"
    archive.write_bytes(b"archive")
    archive_hash = hashlib.sha256(b"archive").hexdigest()
    value = {
        "schema": native_package.REFERENCE_SCHEMA,
        "host": "linux", "bundle": str(bundle), "archive": str(archive),
        "archive_sha256": archive_hash, "source_commit": "a" * 40,
        "version": "1.2.0-beta",
    }
    reference = tmp_path / "reference.json"
    reference.write_text(json.dumps(value))
    monkeypatch.setattr(native_package, "digest", lambda *_: archive_hash)
    monkeypatch.setattr(native_package, "inspect_bundle", lambda *_: {
        "source_commit": "a" * 40, "version": "1.2.0-beta",
        "archive": {"sha256": archive_hash}, "bundle": "Cohesix-1.2.0-beta-linux",
    })
    old = wheel_dir / "cohesix-0.2.0a2-py3-none-any.whl"
    old.write_bytes(b"old")
    with pytest.raises(ValueError, match="version-aligned"):
        native_package.load_reference(reference, "linux")
    old.rename(wheel_dir / "cohesix-1.2.0b0-py3-none-any.whl")
    assert native_package.load_reference(reference, "linux") == value
    value["version"] = 12
    reference.write_text(json.dumps(value))
    with pytest.raises(ValueError, match="identity is invalid"):
        native_package.load_reference(reference, "linux")
