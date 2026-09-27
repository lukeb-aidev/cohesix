# Author: Lukas Bower
# Purpose: Verify Mac installer staging binds the signed app to its selected release and leaves user state outside the package.
# Copyright 2026 Lukas Bower
"""Focused staging checks; real signatures and pkg lifecycle require macOS."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import platform
import plistlib
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/install"))
import build_macos_pkg as macos  # noqa: E402
from stage_swarmui import stage_macos_icon  # noqa: E402


@pytest.mark.skipif(platform.system() != "Darwin", reason="uses macOS iconutil")
def test_selected_vector_icon_stages_as_native_app_icon(tmp_path: Path) -> None:
    source = Path(__file__).resolve().parents[1] / (
        "apps/swarmui/frontend/assets/icons/cohesix-icon.svg"
    )
    report = stage_macos_icon(source, tmp_path / "SwarmUI.app")
    icon = tmp_path / "SwarmUI.app/Contents/Resources/SwarmUI.icns"
    assert report["source_sha256"] == hashlib.sha256(source.read_bytes()).hexdigest()
    assert report["icns_sha256"] == hashlib.sha256(icon.read_bytes()).hexdigest()


def make_inputs(tmp_path: Path) -> dict[str, str]:
    """Make minimal package inputs with distinct code and user-state paths."""
    bundle = tmp_path / "bundle"
    for relative, data in (("bin/coh", b"controller"),
                           ("bin/swarmui", b"desktop")):
        path = bundle / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(0o700)
    (bundle / "MANIFEST.sha256").write_text("\n".join(
        f"{hashlib.sha256((bundle / relative).read_bytes()).hexdigest()}  {relative}"
        for relative in ("bin/coh", "bin/swarmui")
    ) + "\n")
    app = tmp_path / "SwarmUI.app"
    extension = app / "Contents/Extensions/SwarmUIIntents.appex"
    for path, data in (
        (app / "Contents/MacOS/swarmui", b"desktop"),
        (extension / "Contents/MacOS/SwarmUIIntents", b"extension"),
    ):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(0o700)
    for path, bundle_id in (
        (app / "Contents/Info.plist", "com.cohesix.swarmui"),
        (extension / "Contents/Info.plist", "com.cohesix.swarmui.intents"),
    ):
        path.write_bytes(plistlib.dumps({
            "CFBundleIdentifier": bundle_id,
            "CFBundleShortVersionString": "1.2.0",
            "CFBundleVersion": "3",
            **({"CFBundleIconFile": "SwarmUI.icns"}
               if bundle_id == "com.cohesix.swarmui" else {}),
        }))
    icon = app / "Contents/Resources/SwarmUI.icns"
    icon.parent.mkdir(parents=True)
    icon.write_bytes(b"icns" + (8).to_bytes(4, "big"))
    return {
        "signed_app": str(app), "app_binary_sha256": hashlib.sha256(b"desktop").hexdigest(),
        "app_version": "1.2.0", "team_id": "ABCDEFGHIJ",
        "bundle": str(bundle), "host": "macos", "version": "1.2.0",
        "source_commit": "a" * 40, "archive_sha256": "b" * 64,
    }


def test_stage_installs_code_and_app_without_user_state(
    tmp_path: Path, monkeypatch,
) -> None:
    reference = make_inputs(tmp_path)
    monkeypatch.setattr(macos, "signature", lambda *_: {})
    monkeypatch.setattr(macos, "checked", lambda *_: "")
    monkeypatch.setattr(macos, "uuids", lambda *_: {"same-build-uuid"})
    root = tmp_path / "root"
    macos.stage(reference, root)
    assert (root / "Library/Application Support/Cohesix/bin/coh").read_bytes() == b"controller"
    assert (root / "Applications/SwarmUI.app/Contents/MacOS/swarmui").read_bytes() == b"desktop"
    assert (root / "Applications/SwarmUI.app/Contents").stat().st_mode & 0o777 == 0o755
    assert (root / "Applications/SwarmUI.app/Contents/MacOS/swarmui").stat().st_mode & 0o777 == 0o755
    assert not (root / "Users").exists()
    assert not (root / "Library/Application Support/Cohesix/qemu").exists()
    manifest = json.loads((root / "Library/Application Support/Cohesix/installed-payload.json").read_text())
    assert {row["path"] for row in manifest["files"]} >= {
        "/Library/Application Support/Cohesix/bin/coh",
        "/Library/Application Support/Cohesix/bin/cohesix-uninstall",
        "/Applications/SwarmUI.app/Contents/MacOS/swarmui",
    }
    assert (root / "Library/Application Support/Cohesix/INSTALLED.sha256").is_file()


def test_stage_refuses_changed_app_binary(tmp_path: Path) -> None:
    reference = make_inputs(tmp_path)
    (Path(reference["signed_app"]) / "Contents/MacOS/swarmui").write_bytes(b"changed")
    with pytest.raises(ValueError, match="signed app executable changed"):
        macos.stage(reference, tmp_path / "root")


def test_stage_refuses_extension_version_drift(tmp_path: Path) -> None:
    reference = make_inputs(tmp_path)
    extension = Path(reference["signed_app"]) / (
        "Contents/Extensions/SwarmUIIntents.appex/Contents/Info.plist"
    )
    info = plistlib.loads(extension.read_bytes())
    info["CFBundleShortVersionString"] = "0.1.0"
    extension.write_bytes(plistlib.dumps(info))
    with pytest.raises(ValueError, match="selected signed app version changed"):
        macos.stage(reference, tmp_path / "root")


def test_release_b_app_and_extension_versions_match_selected_candidate() -> None:
    root = Path(__file__).resolve().parents[1]
    expected = "1.2.0"
    app = plistlib.loads((root / "packaging/swarmui/Info.plist").read_bytes())
    extension = plistlib.loads((root / (
        "apps/swarmui/native/apple/Sources/SwarmUIIntents/Info.plist"
    )).read_bytes())
    tauri = json.loads((root / "apps/swarmui/tauri.conf.json").read_text())
    assert app["CFBundleShortVersionString"] == expected
    assert extension["CFBundleShortVersionString"] == expected
    assert tauri["version"] == expected
    assert int(app["CFBundleVersion"]) == int(extension["CFBundleVersion"])


def test_notary_failure_leaves_no_publishable_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(macos.platform, "system", lambda: "Darwin")
    monkeypatch.setattr(macos.platform, "machine", lambda: "arm64")
    monkeypatch.setattr(macos, "load_reference", lambda *_: {
        "version": "1.2.0", "source_commit": "a" * 40,
        "archive_sha256": "b" * 64, "team_id": "ABCDEFGHIJ",
    })
    monkeypatch.setattr(macos, "stage", lambda *_: [])

    def fake_checked(command: list[str], timeout: int = 600) -> str:
        if command[0] in {"/usr/bin/pkgbuild", "/usr/bin/productbuild"}:
            Path(command[-1]).write_bytes(b"package")
        if command[0] == "/usr/sbin/pkgutil":
            return "Developer ID Installer: Cohesix (ABCDEFGHIJ)"
        if command[0] == "/usr/bin/xcrun" and command[1] == "notarytool":
            raise ValueError("notary submission failed")
        return ""

    monkeypatch.setattr(macos, "checked", fake_checked)
    output = tmp_path / "installers"
    with pytest.raises(ValueError, match="notary submission failed"):
        macos.build(tmp_path / "reference.json", output, "a" * 40, "profile")
    assert not output.exists()
