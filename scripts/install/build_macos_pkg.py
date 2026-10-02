#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Build and notarize an exact-release Mac package with the signed native SwarmUI app.
# Copyright 2026 Lukas Bower
"""Package selected host tools and an independently checked Developer ID app."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tempfile
from typing import Any

from native_package import load_reference, normalize_public_modes, stage_subset, write_manifest
from sign_swarmui_macos import select_app, signature

INSTALL_ROOT = Path("Library/Application Support/Cohesix")
APP_ROOT = Path("Applications/SwarmUI.app")
UNINSTALL = Path(__file__).resolve().parents[2] / "packaging/macos/cohesix-uninstall.sh"
COH_ENTITLEMENTS = Path(__file__).resolve().parents[2] / "packaging/macos/coh.entitlements"


def checked(command: list[str], timeout: int = 600) -> str:
    """Invoke a fixed Apple package tool without shell expansion."""
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise ValueError(f"Mac package tool failed: {command[0]}: {result.stderr[:2048]}")
    return result.stdout + result.stderr


def uuids(path: Path) -> set[str]:
    """Compare Mach-O source identity before and after code signing."""
    output = checked(["/usr/bin/dwarfdump", "--uuid", str(path)])
    values = set(re.findall(r"UUID: ([0-9A-F-]{36})", output))
    if not values:
        raise ValueError("Mac executable has no Mach-O UUID")
    return values


def app_records(app: Path) -> list[dict[str, object]]:
    """Record all installed signed app files without following links."""
    records: list[dict[str, object]] = []
    for source in sorted(app.rglob("*")):
        if source.is_symlink():
            raise ValueError("Mac app contains a symlink")
        if source.is_file():
            records.append({
                "path": "/" + str(APP_ROOT / source.relative_to(app)),
                "size": source.stat().st_size,
                "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            })
    if not records or len(records) > 1024:
        raise ValueError("Mac app file count bound")
    return records


def sign_host_tool(source: Path, staged: Path, team: str, identity: str) -> None:
    """Sign the copied Mach-O while retaining its release build UUID."""
    source_uuids = uuids(source)
    if uuids(staged) != source_uuids:
        raise ValueError("staged host tool differs from the selected release binary")
    signing = [
        "/usr/bin/codesign", "--force", "--sign", identity,
        "--options", "runtime", "--timestamp",
    ]
    # coh loads the separately signed macFUSE library. Confine the exception
    # to that executable and retain hardened runtime.
    if source.name == "coh":
        signing.extend(["--entitlements", str(COH_ENTITLEMENTS)])
    checked([*signing, str(staged)])
    checked(["/usr/bin/codesign", "--verify", "--strict", str(staged)])
    details = checked(["/usr/bin/codesign", "-dvv", str(staged)])
    if (
        f"TeamIdentifier={team}" not in details
        or "Authority=Developer ID Application:" not in details
        or re.search(r"^CodeDirectory .*\(.*runtime.*\)", details, re.MULTILINE) is None
        or re.search(r"^Timestamp=.+", details, re.MULTILINE) is None
        or uuids(staged) != source_uuids
    ):
        raise ValueError("signed host tool lacks selected identity or runtime")
    if source.name == "coh":
        output = checked(["/usr/bin/codesign", "-d", "--entitlements", ":-", str(staged)])
        payload = re.search(r"<plist\b.*?</plist>", output, re.DOTALL)
        expected = {"com.apple.security.cs.disable-library-validation": True}
        if payload is None or plistlib.loads(payload[0].encode()) != expected:
            raise ValueError("signed coh lacks the bounded macFUSE library exception")


def stage(reference: dict[str, Any], root: Path, app_identity: str) -> list[dict[str, object]]:
    """Stage code in OS-owned locations while leaving user state outside pkg."""
    app = select_app(Path(reference["signed_app"]))
    binary = app / "Contents/MacOS/swarmui"
    if hashlib.sha256(binary.read_bytes()).hexdigest() != reference["app_binary_sha256"]:
        raise ValueError("selected signed app executable changed")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    extension_info = plistlib.loads((
        app / "Contents/Extensions/SwarmUIIntents.appex/Contents/Info.plist"
    ).read_bytes())
    if (
        info.get("CFBundleShortVersionString") != reference["app_version"]
        or extension_info.get("CFBundleShortVersionString") != reference["app_version"]
        or not re.fullmatch(r"[1-9][0-9]*", str(info.get("CFBundleVersion", "")))
        or info.get("CFBundleVersion") != extension_info.get("CFBundleVersion")
    ):
        raise ValueError("selected signed app version changed")
    icon = app / "Contents/Resources/SwarmUI.icns"
    icon_bytes = icon.read_bytes() if icon.is_file() and not icon.is_symlink() else b""
    if (
        info.get("CFBundleIconFile") != "SwarmUI.icns"
        or len(icon_bytes) < 8
        or icon_bytes[:4] != b"icns"
        or int.from_bytes(icon_bytes[4:8], "big") != len(icon_bytes)
    ):
        raise ValueError("signed app icon is missing or invalid")
    signature(app, reference["team_id"], "Developer ID Application")
    checked(["/usr/bin/xcrun", "stapler", "validate", str(app)])
    checked(["/usr/sbin/spctl", "--assess", "--type", "execute", str(app)])
    original = Path(reference["bundle"]) / "bin/swarmui"
    if uuids(original) != uuids(binary):
        raise ValueError("signed SwarmUI differs from the selected release binary")

    destination = root / INSTALL_ROOT
    records = stage_subset(Path(reference["bundle"]), destination, "controller")
    records = [{**row, "path": "/" + str(INSTALL_ROOT / row["path"])}
               for row in records]
    tool_root = Path("/") / INSTALL_ROOT / "bin"
    for row in records:
        installed = Path(str(row["path"]))
        if installed.parent != tool_root:
            continue
        source = Path(reference["bundle"]) / "bin" / installed.name
        staged = root / installed.relative_to("/")
        sign_host_tool(source, staged, reference["team_id"], app_identity)
        row["size"] = staged.stat().st_size
        row["sha256"] = hashlib.sha256(staged.read_bytes()).hexdigest()
    installed_app = root / APP_ROOT
    shutil.copytree(app, installed_app, symlinks=False)
    signature(installed_app, reference["team_id"], "Developer ID Application")
    records.extend(app_records(installed_app))
    helper = destination / "bin/cohesix-uninstall"
    shutil.copyfile(UNINSTALL, helper)
    helper.chmod(0o755)
    records.append({
        "path": "/" + str(INSTALL_ROOT / "bin/cohesix-uninstall"),
        "size": helper.stat().st_size,
        "sha256": hashlib.sha256(helper.read_bytes()).hexdigest(),
    })
    write_manifest(
        destination / "installed-payload.json", reference=reference,
        records=records, package="cohesix-macos-host",
    )
    (destination / "INSTALLED.sha256").write_text("".join(
        f"{row['sha256']}  {row['path']}\n"
        for row in sorted(records, key=lambda item: str(item["path"]))
    ))
    normalize_public_modes(root)
    signature(installed_app, reference["team_id"], "Developer ID Application")
    return records


def build(reference_path: Path, output: Path, identity: str | None,
          notary_profile: str | None, app_identity: str | None) -> dict[str, object]:
    """Create the signed, notarized pkg and retain its exact publisher proof."""
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("Mac package requires an Apple Silicon build host")
    if (
        not identity or not re.fullmatch(r"[0-9A-Fa-f]{40}", identity)
        or not app_identity or not re.fullmatch(r"[0-9A-Fa-f]{40}", app_identity)
        or not notary_profile
        or not re.fullmatch(r"[A-Za-z0-9_.-]{1,80}", notary_profile)
    ):
        raise ValueError("select Developer ID Application and Installer plus stored notary profile")
    reference = load_reference(reference_path, "macos")
    if output.exists() or output.is_symlink():
        raise ValueError("installer output already exists")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="m28g-macos-pkg-", dir=output.parent) as temporary:
        work = Path(temporary)
        published = work / "published"
        published.mkdir()
        stage(reference, work / "payload", app_identity)
        component = work / "Cohesix-component.pkg"
        checked([
            "/usr/bin/pkgbuild", "--root", str(work / "payload"),
            "--identifier", "com.cohesix.host", "--version",
            reference["version"].split("-", 1)[0], "--install-location", "/",
            str(component),
        ])
        package = published / f"Cohesix-{reference['version']}-MacOS.pkg"
        checked([
            "/usr/bin/productbuild", "--package", str(component),
            "--sign", identity, str(package),
        ])
        assessment = checked(["/usr/sbin/pkgutil", "--check-signature", str(package)])
        if "Developer ID Installer" not in assessment:
            raise ValueError("outer package lacks Developer ID Installer signature")
        submitted = json.loads(checked([
            "/usr/bin/xcrun", "notarytool", "submit", str(package),
            "--keychain-profile", notary_profile, "--wait", "--output-format", "json",
        ], timeout=3600))
        if submitted.get("status") != "Accepted":
            raise ValueError("Apple did not accept the exact installer package")
        checked(["/usr/bin/xcrun", "stapler", "staple", str(package)])
        checked(["/usr/bin/xcrun", "stapler", "validate", str(package)])
        checked(["/usr/sbin/spctl", "--assess", "--type", "install", str(package)])
        result: dict[str, object] = {
            "schema": "cohesix-m28g-installers/v1", "host": "macos",
            "version": reference["version"], "source_commit": reference["source_commit"],
            "archive_sha256": reference["archive_sha256"],
            "package": package.name,
            "package_sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
            "notarization_id": submitted["id"], "team_id": reference["team_id"],
            "state": "built-signed-notarized-unqualified",
        }
        (published / "installers.json").write_text(
            json.dumps(result, sort_keys=True, indent=2) + "\n"
        )
        os.replace(published, output)
    return result


def main() -> None:
    """Signing identities and notary credentials remain outside the package."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference-config", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = build(
            args.reference_config, args.out,
            os.environ.get("COHESIX_INSTALLER_IDENTITY"),
            os.environ.get("COHESIX_NOTARY_PROFILE"),
            os.environ.get("COHESIX_APP_IDENTITY"),
        )
    except (ValueError, OSError, subprocess.TimeoutExpired,
            subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        parser.error(str(exc))
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
