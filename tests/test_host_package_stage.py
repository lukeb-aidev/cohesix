# Author: Lukas Bower
# Purpose: Preserve exact compiler-selected staging membership and source confinement before signing.
# Copyright 2026 Lukas Bower
"""Staging tests do not assert signature, native execution or release acceptance."""

import importlib.util
import json
from pathlib import Path

import pytest

SPEC = importlib.util.spec_from_file_location(
    "stage_host_package",
    Path(__file__).resolve().parents[1] / "scripts/install/stage_host_package.py",
)
assert SPEC and SPEC.loader
stage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage)


def test_only_registered_sources_are_staged_and_symlinks_fail(tmp_path: Path) -> None:
    root = tmp_path.resolve()
    generated = root / "generated"
    (generated / "configs/generated").mkdir(parents=True)
    binaries = root / "bin"
    binaries.mkdir()
    (binaries / "coh").write_bytes(b"format validation is owned by package build")
    (root / "README.md").write_text("selected instructions")
    (root / "secret.private").write_text("must never be copied")
    registry = {
        "contract": {
            "deployment_profiles": [
                {
                    "id": "unit",
                    "artifacts": [
                        {"path": "README.md", "executable": False},
                        {"path": "bin/coh", "executable": True},
                        {"path": "package.sbom.json", "executable": False},
                    ],
                }
            ]
        }
    }
    (generated / "configs/generated/provider_registry.json").write_text(
        json.dumps(registry)
    )
    output = root / "out"
    stage.stage(root, generated, binaries, "unit", output)
    assert {
        str(path.relative_to(output)) for path in output.rglob("*") if path.is_file()
    } == {"README.md", "bin/coh"}
    assert (output / "bin/coh").stat().st_mode & 0o777 == 0o700
    with pytest.raises(FileExistsError):
        stage.stage(root, generated, binaries, "unit", output)
    (root / "README.md").unlink()
    (root / "README.md").symlink_to(root / "secret.private")
    with pytest.raises(ValueError, match="symlink"):
        stage.stage(root, generated, binaries, "unit", root / "rejected")


def test_desktop_bundle_uses_selected_binary_and_launch_metadata(tmp_path: Path) -> None:
    root = tmp_path.resolve()
    (root / "configs/generated").mkdir(parents=True)
    (root / "packaging/swarmui").mkdir(parents=True)
    (root / "bin").mkdir()
    (root / "bin/swarmui").write_bytes(b"native executable identity")
    (root / "packaging/swarmui/Info.plist").write_text("launch metadata")
    artifacts = [
        {"path": "SwarmUI.app/Contents/MacOS/swarmui", "executable": True},
        {"path": "SwarmUI.app/Contents/Info.plist", "executable": False},
    ]
    (root / "configs/generated/provider_registry.json").write_text(json.dumps({
        "contract": {"deployment_profiles": [{"id": "desktop", "artifacts": artifacts}]}
    }))
    stage.stage(root, root, root / "bin", "desktop", root / "out")
    assert (root / "out/SwarmUI.app/Contents/MacOS/swarmui").read_bytes() == b"native executable identity"
    assert (root / "out/SwarmUI.app/Contents/Info.plist").read_text() == "launch metadata"
