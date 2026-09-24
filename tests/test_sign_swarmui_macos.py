# Author: Lukas Bower
# Purpose: Refuse unsafe Mac signing selections and preserve the exact shared Keychain entitlement.
# Copyright 2026 Lukas Bower
"""Pure signing-input checks; no certificate, notarisation or external upload is used."""

import importlib.util
from datetime import datetime, timedelta, timezone
from pathlib import Path
import plistlib
from subprocess import CompletedProcess

import pytest

SPEC = importlib.util.spec_from_file_location(
    "sign_swarmui_macos",
    Path(__file__).resolve().parents[1] / "scripts/install/sign_swarmui_macos.py",
)
assert SPEC and SPEC.loader
signer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signer)


def test_shared_group_is_rendered_from_selected_team() -> None:
    app = signer.entitlements(
        signer.ROOT / "packaging/swarmui/SwarmUI.entitlements", "KB88FQXUX2"
    )
    extension = signer.entitlements(
        signer.ROOT / "apps/swarmui/native/apple/Sources/SwarmUIIntents/SwarmUIIntents.entitlements",
        "KB88FQXUX2",
    )
    group = ["KB88FQXUX2.com.cohesix.swarmui"]
    assert plistlib.loads(app)["keychain-access-groups"] == group
    assert plistlib.loads(extension)["keychain-access-groups"] == group
    assert plistlib.loads(extension)["com.apple.security.app-sandbox"] is True
    with pytest.raises(ValueError, match="Team ID"):
        signer.entitlements(
            signer.ROOT / "packaging/swarmui/SwarmUI.entitlements", "bad/team"
        )


def test_app_selection_rejects_wrong_identity_and_symlink(tmp_path: Path) -> None:
    app = tmp_path / "SwarmUI.app"
    extension = app / signer.EXTENSION
    for root, name, bundle_id, binary in (
        (app, "SwarmUI", "com.cohesix.swarmui", "swarmui"),
        (extension, "SwarmUIIntents", "com.cohesix.swarmui.intents", "SwarmUIIntents"),
    ):
        (root / "Contents/MacOS").mkdir(parents=True)
        (root / "Contents/MacOS" / binary).write_bytes(b"Mach-O fixture")
        (root / "Contents/Info.plist").write_bytes(plistlib.dumps({
            "CFBundleName": name, "CFBundleIdentifier": bundle_id,
        }))
    assert signer.select_app(app) == app
    assert signer.select_state(tmp_path / "evidence", app) == tmp_path / "evidence"
    with pytest.raises(ValueError, match="outside"):
        signer.select_state(app / "Contents/evidence", app)
    (extension / "Contents/MacOS/SwarmUIIntents").unlink()
    (extension / "Contents/MacOS/SwarmUIIntents").symlink_to(
        app / "Contents/MacOS/swarmui"
    )
    with pytest.raises(ValueError, match="symlink"):
        signer.select_app(app)


def test_json_command_uses_stdout_without_discarding_failure_diagnostics(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        signer.subprocess, "run",
        lambda *_args, **_kwargs: CompletedProcess([], 0, b'{"status":"Accepted"}', b"warning\n"),
    )
    assert signer.command(["/usr/bin/xcrun"], stdout_only=True) == '{"status":"Accepted"}'
    monkeypatch.setattr(
        signer.subprocess, "run",
        lambda *_args, **_kwargs: CompletedProcess([], 1, b"", b"reason\n"),
    )
    with pytest.raises(ValueError, match="reason"):
        signer.command(["/usr/bin/xcrun"], stdout_only=True)


def test_profile_must_authorize_exact_team_bundle_and_group(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile_path = tmp_path / "app.provisionprofile"
    profile_path.write_bytes(b"signed fixture")
    profile = {
        "TeamIdentifier": ["KB88FQXUX2"],
        "UUID": "11111111-1111-1111-1111-111111111111",
        "ExpirationDate": datetime.now(timezone.utc) + timedelta(days=1),
        "Entitlements": {
            "com.apple.application-identifier": "KB88FQXUX2.com.cohesix.swarmui",
            "keychain-access-groups": ["KB88FQXUX2.com.cohesix.swarmui"],
        },
    }
    monkeypatch.setattr(signer, "command", lambda *_args, **_kwargs:
                        plistlib.dumps(profile).decode())
    selected = signer.provisioning_profile(
        profile_path, "KB88FQXUX2", "com.cohesix.swarmui"
    )
    assert selected["bundle_id"] == "com.cohesix.swarmui"
    profile["Entitlements"]["keychain-access-groups"] = ["KB88FQXUX2.*"]
    assert signer.provisioning_profile(
        profile_path, "KB88FQXUX2", "com.cohesix.swarmui"
    )["bundle_id"] == "com.cohesix.swarmui"
    profile["Entitlements"]["keychain-access-groups"] = ["OTHERTEAM.*"]
    with pytest.raises(ValueError, match="authorize"):
        signer.provisioning_profile(profile_path, "KB88FQXUX2", "com.cohesix.swarmui")
    profile["Entitlements"]["keychain-access-groups"] = ["KB88FQXUX2.*"]
    with pytest.raises(ValueError, match="authorize"):
        signer.provisioning_profile(
            profile_path, "KB88FQXUX2", "com.cohesix.swarmui.intents"
        )
