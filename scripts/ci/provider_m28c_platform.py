#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Retain exact-source macOS App Intents discovery and Apple capability observations without claiming a Siri workflow.
# Copyright 2026 Lukas Bower
"""Run the M28c platform gate on one installed development-signed Mac app."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import plistlib
import re
import subprocess
import sys
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = "cohesix-m28c-platform-reference/v1"
FIELDS = {
    "schema", "source_commit", "app_path", "app_binary_sha256",
    "extension_binary_sha256", "team_id", "shortcuts_ax_path",
    "shortcuts_ax_sha256",
}
EXTENSION = Path("Contents/Extensions/SwarmUIIntents.appex")
ACTION = "Check Cohesix Apple Support"
MAX_INPUT = 1024 * 1024


def require(condition: bool, reason: str) -> None:
    """Refuse an incomplete observation before creating a success record."""
    if not condition:
        raise ValueError(reason)


def regular(path: Path, maximum: int = MAX_INPUT) -> bytes:
    """Read a bounded regular file through non-symlink components."""
    require(path.is_absolute() and ".." not in path.parts, "absolute input path required")
    require(not any(part.is_symlink() for part in (path, *path.parents)),
            "symlinked input path")
    require(path.is_file() and path.stat().st_size <= maximum, "input file bound")
    data = path.read_bytes()
    require(len(data) <= maximum, "input read bound")
    return data


def digest(data: bytes) -> str:
    """Bind a selected input to its predeclared SHA-256 digest."""
    return hashlib.sha256(data).hexdigest()


def reference(path: Path) -> dict[str, str]:
    """Require exact fields and identities from the private predeclared input."""
    value = json.loads(regular(path, 8192))
    require(isinstance(value, dict) and set(value) == FIELDS
            and value["schema"] == SCHEMA, "platform reference fields")
    require(re.fullmatch(r"[0-9a-f]{40}", value["source_commit"]) is not None,
            "source commit identity")
    for name in ("app_binary_sha256", "extension_binary_sha256", "shortcuts_ax_sha256"):
        require(isinstance(value[name], str)
                and re.fullmatch(r"[0-9a-f]{64}", value[name]) is not None,
                f"platform {name}")
    require(isinstance(value["team_id"], str)
            and re.fullmatch(r"[A-Z0-9]{10}", value["team_id"]) is not None,
            "development team ID")
    for name in ("app_path", "shortcuts_ax_path"):
        require(isinstance(value[name], str) and Path(value[name]).is_absolute()
                and ".." not in Path(value[name]).parts, f"platform {name}")
    return value


def command(args: list[str], maximum: int = 8192, timeout: int = 60,
            include_stderr: bool = True) -> str:
    """Run a fixed diagnostic command with bounded captured output."""
    result = subprocess.run(args, cwd=ROOT, capture_output=True, timeout=timeout,
                            check=False)
    require(len(result.stdout) <= maximum and len(result.stderr) <= maximum,
            "diagnostic output bound")
    require(result.returncode == 0, "diagnostic command failed: " + args[0])
    output = result.stdout + result.stderr if include_stderr else result.stdout
    return output.decode("utf-8", errors="strict")


def action_lines(raw: bytes) -> list[str]:
    """Keep only the actual Shortcuts action search observation, not library data."""
    text = raw.decode("utf-8", errors="strict")
    lines = text.splitlines()
    selected = [line.strip() for line in lines
                if line.startswith("Window:")
                or re.match(r"^\s+\d+ search text field \(settable\) Value: Cohesix", line)
                or re.match(r"^\s+\d+ text " + re.escape(ACTION) + r"$", line)]
    require(len(selected) == 3 and selected[0].startswith("Window:")
            and "Shortcuts" in selected[0]
            and "Value: Cohesix" in selected[1]
            and selected[2].endswith("text " + ACTION),
            "Shortcuts did not show the installed action")
    return selected


def check_entitlements(raw: str, team: str, extension: bool) -> None:
    """Require the same provisioned Keychain group on both signed components."""
    marker = raw.find("<?xml")
    require(marker >= 0, "missing app or extension entitlements")
    rights = plistlib.loads(raw[marker:].encode())
    require(rights.get("keychain-access-groups") ==
            [team + ".com.cohesix.swarmui"],
            "app and extension need the selected shared Keychain group")
    if extension:
        require(rights.get("com.apple.security.app-sandbox") is True
                and rights.get("com.apple.security.network.client") is True,
                "extension sandbox/network entitlement missing")


def identity(app: Path, selected: dict[str, str]) -> dict[str, Any]:
    """Check installed bundle, development signature, sandbox and registration."""
    require(app.is_dir() and not app.is_symlink()
            and not any(parent.is_symlink() for parent in app.parents),
            "installed app path")
    extension = app / EXTENSION
    app_info = plistlib.loads(regular(app / "Contents/Info.plist", 65536))
    ext_info = plistlib.loads(regular(extension / "Contents/Info.plist", 65536))
    require(app_info.get("CFBundleIdentifier") == "com.cohesix.swarmui"
            and ext_info.get("CFBundleIdentifier") == "com.cohesix.swarmui.intents"
            and ext_info.get("EXAppExtensionAttributes", {}).get(
                "EXExtensionPointIdentifier") == "com.apple.appintents-extension",
            "installed bundle identity")
    binaries = {
        "app_binary_sha256": app / "Contents/MacOS/swarmui",
        "extension_binary_sha256": extension / "Contents/MacOS/SwarmUIIntents",
    }
    observed_hashes = {}
    for name, path in binaries.items():
        observed_hashes[name] = digest(regular(path, 128 * 1024 * 1024))
        require(observed_hashes[name] == selected[name], f"installed {name} changed")
    command(["codesign", "--verify", "--deep", "--strict", str(app)])
    for path in (app, extension):
        details = command(["codesign", "-dvv", str(path)])
        require("TeamIdentifier=" + selected["team_id"] in details
                and "Authority=Apple Development:" in details,
                "development signer or team mismatch")
        entitlements = command(["codesign", "-d", "--entitlements", ":-", str(path)],
                               include_stderr=False)
        check_entitlements(entitlements, selected["team_id"], path == extension)
    plugins = command(["pluginkit", "-m", "-v", "-i", "com.cohesix.swarmui.intents"])
    require(str(extension) in plugins, "installed extension not registered")
    actions = json.loads(regular(
        extension / "Contents/Resources/Metadata.appintents/extract.actionsdata"))
    require(actions.get("actions", {}).get("PlatformProbeIntent", {}).get(
        "isDiscoverable") is True, "extracted action is not discoverable")
    return {"bundle_id": app_info["CFBundleIdentifier"],
            "extension_id": ext_info["CFBundleIdentifier"],
            "team_id": selected["team_id"], "hashes": observed_hashes,
            "extension_sandbox": True, "shared_keychain_group": True,
            "shortcuts_registration": True}


def run_live(case: str, reference_path: Path, host_profile: str,
             state_dir: Path) -> int:
    """Retain the platform observation at a clean exact source identity."""
    require(case == "m28c-platform-live"
            and host_profile == "macos-apple-silicon", "M28c platform selection")
    selected = reference(reference_path)
    require(sys.platform == "darwin", "macOS host required")
    require(command(["uname", "-m"]).strip() == "arm64", "Apple Silicon host required")
    version = command(["sw_vers", "-productVersion"]).strip()
    require(version.startswith("27."), "macOS 27 required")
    commit = command(["git", "rev-parse", "HEAD"]).strip()
    require(commit == selected["source_commit"], "source commit changed")
    require(not command(["git", "status", "--porcelain", "--untracked-files=all"]).strip(),
            "exact source requires a clean worktree")
    require(not state_dir.exists(), "platform evidence directory already exists")
    app = Path(selected["app_path"])
    signed = identity(app, selected)
    ax = regular(Path(selected["shortcuts_ax_path"]), 65536)
    require(digest(ax) == selected["shortcuts_ax_sha256"],
            "Shortcuts observation changed")
    discovered = action_lines(ax)
    probe = json.loads(command(["swift", "run", "--package-path",
        str(ROOT / "apps/swarmui/native/apple"), "CohesixPlatformProbe"],
        timeout=180, include_stderr=False))
    require(probe.get("schema") == "cohesix-apple-platform-probe/v1"
            and all(type(probe.get(name)) is bool for name in (
                "foundation_model_available", "foundation_model_supports_locale",
                "metal_device_available", "unified_memory")),
            "native capability probe shape")
    require(probe["metal_device_available"] and probe["unified_memory"],
            "supported unified-memory Metal device unavailable")
    state_dir.mkdir(mode=0o700, parents=True)
    record = {
        "schema": "cohesix-m28c-platform-live/v1", "result": "OBSERVED",
        "source_commit": commit, "host_profile": host_profile,
        "macos_version": version,
        "macos_build": command(["sw_vers", "-buildVersion"]).strip(),
        "xcode": command(["xcodebuild", "-version"]).strip(),
        "sdk": command(["xcrun", "--sdk", "macosx", "--show-sdk-version"]).strip(),
        "installed_app": str(app), "signed_app": signed,
        "shortcuts_action_lines": discovered,
        "shortcuts_ax_sha256": digest(ax), "native_capabilities": probe,
        "siri_invoked": False, "notarized": False,
        "proof_limit": "Installed action discovery only; no Siri, hive, MLX or distribution acceptance.",
    }
    (state_dir / "summary.json").write_text(json.dumps(record, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"result": "OBSERVED", "summary": str(state_dir / "summary.json")}))
    return 0


if __name__ == "__main__":
    raise SystemExit("use provider_conformance_run.sh --case m28c-platform-live")
