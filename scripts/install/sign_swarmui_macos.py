#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Sign the exact SwarmUI app and App Intents extension with shared Keychain entitlements, then retain direct-distribution notarisation evidence.
# Copyright 2026 Lukas Bower
"""Prepare and verify one Mac app for direct distribution; no release upload occurs."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import plistlib
import re
import subprocess
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
EXTENSION = Path("Contents/Extensions/SwarmUIIntents.appex")
GROUP_SUFFIX = ".com.cohesix.swarmui"
MAX_OUTPUT = 256 * 1024
MAX_APP_BYTES = 512 * 1024 * 1024


def require(condition: bool, reason: str) -> None:
    """Fail before a signing or notarisation side effect when input is ambiguous."""
    if not condition:
        raise ValueError(reason)


def select_app(value: Path) -> Path:
    """Select one already staged bundle without symlink traversal."""
    require(value.is_absolute() and value.suffix == ".app" and ".." not in value.parts,
            "absolute app selection required")
    require(value.is_dir() and all(not part.is_symlink() for part in (value, *value.parents)),
            "app directory cannot be symlinked")
    members = 0
    total = 0
    for member in value.rglob("*"):
        members += 1
        require(members <= 1024, "app member count bound")
        require(not member.is_symlink(), "app contains a symlinked member")
        if member.is_file():
            total += member.stat().st_size
            require(total <= MAX_APP_BYTES, "app byte bound")
    for name in ("Contents/Info.plist", "Contents/MacOS/swarmui",
                 str(EXTENSION / "Contents/Info.plist"),
                 str(EXTENSION / "Contents/MacOS/SwarmUIIntents")):
        require((value / name).is_file(), f"missing app member {name}")
    app_info = plistlib.loads(read_bounded(value / "Contents/Info.plist", 65_536))
    ext_info = plistlib.loads(read_bounded(value / EXTENSION / "Contents/Info.plist", 65_536))
    require(app_info.get("CFBundleIdentifier") == "com.cohesix.swarmui"
            and ext_info.get("CFBundleIdentifier") == "com.cohesix.swarmui.intents",
            "unexpected app or extension bundle identity")
    return value


def team_id(value: str) -> str:
    """Require an exact Apple Team ID, never a caller-supplied entitlement prefix."""
    require(re.fullmatch(r"[A-Z0-9]{10}", value) is not None, "invalid Apple Team ID")
    return value


def select_state(value: Path, app: Path) -> Path:
    """Keep mutable signing evidence outside the exact app being signed."""
    require(value.is_absolute() and ".." not in value.parts
            and all(not part.is_symlink() for part in value.parents),
            "absolute non-symlink state directory required")
    require(not value.is_relative_to(app),
            "signing evidence must be outside the app bundle")
    return value


def read_bounded(path: Path, maximum: int) -> bytes:
    """Read a bounded regular signing input after checking its metadata."""
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= maximum,
            "signing input bound")
    data = path.read_bytes()
    require(len(data) <= maximum, "signing input changed while reading")
    return data


def digest_file(path: Path, maximum: int = MAX_APP_BYTES) -> str:
    """Hash one bounded file without materializing an archive or executable."""
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= maximum,
            "signed file bound")
    digest = hashlib.sha256()
    total = 0
    with path.open("rb") as source:
        while block := source.read(1024 * 1024):
            total += len(block)
            require(total <= maximum, "signed file changed while reading")
            digest.update(block)
    return digest.hexdigest()


def entitlements(source: Path, team: str) -> bytes:
    """Render Xcode's team prefix macro for direct codesign on a staged app."""
    template = plistlib.loads(read_bounded(source, 65_536))
    require(template.get("keychain-access-groups") ==
            ["$(AppIdentifierPrefix)com.cohesix.swarmui"],
            "unexpected Keychain entitlement template")
    template["keychain-access-groups"] = [team_id(team) + GROUP_SUFFIX]
    return plistlib.dumps(template)


def provisioning_profile(source: Path, team: str, bundle: str) -> dict[str, str]:
    """Verify one Apple-issued profile authorizes the exact signed bundle and group."""
    require(source.is_absolute() and ".." not in source.parts
            and all(not part.is_symlink() for part in source.parents),
            "absolute non-symlink provisioning profile required")
    read_bounded(source, MAX_OUTPUT)
    payload = command(["/usr/bin/security", "cms", "-D", "-i", str(source)],
                      stdout_only=True)
    profile = plistlib.loads(payload.encode())
    rights = profile.get("Entitlements", {})
    expiry = profile.get("ExpirationDate")
    if isinstance(expiry, datetime) and expiry.tzinfo is None:
        expiry = expiry.replace(tzinfo=timezone.utc)
    require(isinstance(expiry, datetime) and expiry > datetime.now(timezone.utc),
            "provisioning profile expired")
    allowed_groups = rights.get("keychain-access-groups")
    app_identifier = rights.get("com.apple.application-identifier")
    require(profile.get("TeamIdentifier") == [team]
            and isinstance(allowed_groups, list)
            and all(isinstance(group, str) for group in allowed_groups)
            and (team + GROUP_SUFFIX in allowed_groups or team + ".*" in allowed_groups)
            and app_identifier == team + "." + bundle,
            "provisioning profile does not authorize the exact bundle and Keychain group")
    identifier = profile.get("UUID")
    require(isinstance(identifier, str)
            and re.fullmatch(r"[0-9a-fA-F-]{36}", identifier) is not None,
            "provisioning profile UUID missing")
    return {"sha256": digest_file(source, MAX_OUTPUT), "uuid": identifier,
            "expires_at": expiry.isoformat(), "bundle_id": bundle}


def command(args: list[str], timeout: int = 120, stdout_only: bool = False) -> str:
    """Run a fixed native tool with bounded diagnostics and no shell."""
    result = subprocess.run(args, capture_output=True, check=False, timeout=timeout)
    require(len(result.stdout) <= MAX_OUTPUT and len(result.stderr) <= MAX_OUTPUT,
            "native output bound")
    diagnostic = (result.stdout + result.stderr).decode("utf-8", errors="replace")
    require(result.returncode == 0,
            f"native command failed: {args[0]}: {diagnostic[:2048]}")
    return (result.stdout if stdout_only else result.stdout + result.stderr).decode(
        "utf-8", errors="strict")


def signature(app: Path, team: str, authority: str) -> dict[str, Any]:
    """Observe exact nested signatures, signer class and shared Keychain rights."""
    command(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)])
    identities = {}
    for label, path in (("app", app), ("extension", app / EXTENSION)):
        details = command(["/usr/bin/codesign", "-dvv", str(path)])
        require(f"TeamIdentifier={team}" in details
                and f"Authority={authority}:" in details,
                f"{label} has the wrong signer or team")
        raw = command(["/usr/bin/codesign", "-d", "--entitlements", ":-", str(path)])
        marker = raw.find("<?xml")
        require(marker >= 0, f"{label} has no entitlements")
        rights = plistlib.loads(raw[marker:].encode())
        require(rights.get("keychain-access-groups") == [team + GROUP_SUFFIX],
                f"{label} has the wrong Keychain group")
        if label == "extension":
            require(rights.get("com.apple.security.app-sandbox") is True
                    and rights.get("com.apple.security.network.client") is True,
                    "extension sandbox or network client right missing")
        profile_path = path / "Contents/embedded.provisionprofile"
        profile = provisioning_profile(
            profile_path, team,
            "com.cohesix.swarmui" + (".intents" if label == "extension" else ""),
        )
        identities[label] = {
            "binary_sha256": digest_file(path / "Contents/MacOS" /
                ("swarmui" if label == "app" else "SwarmUIIntents")),
            "team_id": team,
            "keychain_group": team + GROUP_SUFFIX,
            "provisioning_profile": profile,
        }
    return identities


def sign(app: Path, state: Path, team: str, identity: str,
         profiles: dict[str, Path], development: bool) -> dict[str, Any]:
    """Embed exact Apple profiles, then sign the extension before the parent."""
    require(re.fullmatch(r"[0-9A-Fa-f]{40}", identity) is not None,
            "select a code signing certificate SHA-1 identity")
    require(not state.exists(), "signing evidence directory already exists")
    checked = {
        label: provisioning_profile(source, team,
            "com.cohesix.swarmui" + (".intents" if label == "extension" else ""))
        for label, source in profiles.items()
    }
    require(set(checked) == {"app", "extension"}, "two selected profiles required")
    state.mkdir(parents=True, mode=0o700)
    for label, path, source in (
        ("extension", app / EXTENSION,
         ROOT / "apps/swarmui/native/apple/Sources/SwarmUIIntents/SwarmUIIntents.entitlements"),
        ("app", app, ROOT / "packaging/swarmui/SwarmUI.entitlements"),
    ):
        embedded = path / "Contents/embedded.provisionprofile"
        require(not embedded.exists(), "app already contains a provisioning profile")
        embedded.write_bytes(read_bounded(profiles[label], MAX_OUTPUT))
        require(digest_file(embedded, MAX_OUTPUT) == checked[label]["sha256"],
                "provisioning profile changed during signing")
        rights = state / f"{label}.entitlements"
        rights.write_bytes(entitlements(source, team))
        rights.chmod(0o600)
        signing = ["/usr/bin/codesign", "--force", "--sign", identity]
        if not development:
            signing += ["--options", "runtime", "--timestamp"]
        signing += ["--entitlements", str(rights), str(path)]
        command(signing)
    authority = "Apple Development" if development else "Developer ID Application"
    return {"schema": "cohesix-m28c-apple-signing/v1", "status": "signed",
            "app": str(app), "team_id": team, "authority": authority,
            "identities": signature(app, team, authority)}


def notarize(app: Path, state: Path, team: str, profile: str) -> dict[str, Any]:
    """Submit exact signed bytes, staple accepted ticket and assess installed trust."""
    require(re.fullmatch(r"[A-Za-z0-9_-]{1,64}", profile) is not None,
            "invalid notarytool Keychain profile")
    before = signature(app, team, "Developer ID Application")
    require(not state.exists(), "notarisation evidence directory already exists")
    state.mkdir(parents=True, mode=0o700)
    archive = state / (app.stem + ".zip")
    command(["/usr/bin/ditto", "-c", "-k", "--keepParent", str(app), str(archive)])
    archive_hash = digest_file(archive)
    response = json.loads(command(["/usr/bin/xcrun", "notarytool", "submit", str(archive),
                                   "--keychain-profile", profile, "--wait", "--output-format", "json"],
                                  timeout=3600, stdout_only=True))
    require(response.get("status") == "Accepted"
            and isinstance(response.get("id"), str)
            and re.fullmatch(r"[0-9a-fA-F-]{36}", response["id"]) is not None,
            "Apple notarisation did not accept the exact archive")
    log = json.loads(command(["/usr/bin/xcrun", "notarytool", "log", response["id"],
                              "--keychain-profile", profile, "--output-format", "json"],
                             stdout_only=True))
    require(log.get("status") == "Accepted" and log.get("statusCode") == 0,
            "Apple notarisation log did not confirm acceptance")
    require(digest_file(archive) == archive_hash,
            "notarisation archive changed after submission")
    command(["/usr/bin/xcrun", "stapler", "staple", str(app)])
    command(["/usr/sbin/spctl", "--assess", "--type", "execute", "--verbose", str(app)])
    after = signature(app, team, "Developer ID Application")
    require(after == before, "stapling changed signed executables or entitlements")
    return {"schema": "cohesix-m28c-apple-notarisation/v1", "status": "Accepted",
            "app": str(app), "team_id": team, "archive_sha256": archive_hash,
            "submission_id": response["id"], "identities": after}


def main() -> None:
    """Do only the selected sign or notarise step and retain its exact result."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=["sign-development", "sign", "notarize"])
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--team-id", required=True)
    parser.add_argument("--identity-sha1")
    parser.add_argument("--app-profile", type=Path)
    parser.add_argument("--extension-profile", type=Path)
    parser.add_argument("--notary-profile")
    args = parser.parse_args()
    app = select_app(args.app)
    state = select_state(args.state_dir, app)
    team = team_id(args.team_id)
    if args.phase in {"sign", "sign-development"}:
        require(args.identity_sha1 is not None and args.notary_profile is None
                and args.app_profile is not None and args.extension_profile is not None,
                "signing requires a certificate and two Apple profiles")
        report = sign(app, state, team, args.identity_sha1,
                      {"app": args.app_profile, "extension": args.extension_profile},
                      args.phase == "sign-development")
    else:
        require(args.notary_profile is not None and args.identity_sha1 is None
                and args.app_profile is None and args.extension_profile is None,
                "notarisation requires only a stored notary profile")
        report = notarize(app, state, team, args.notary_profile)
    (state / "summary.json").write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"status": report["status"], "summary": str(state / "summary.json")}))


if __name__ == "__main__":
    main()
