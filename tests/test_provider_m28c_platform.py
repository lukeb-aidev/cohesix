# Author: Lukas Bower
# Purpose: Refuse substituted Apple platform references and incomplete Shortcuts discovery observations.
# Copyright 2026 Lukas Bower
"""Pure validation checks for the live macOS platform evidence collector."""

import importlib.util
import json
from pathlib import Path
import plistlib

import pytest

SPEC = importlib.util.spec_from_file_location(
    "provider_m28c_platform",
    Path(__file__).resolve().parents[1] / "scripts/ci/provider_m28c_platform.py",
)
assert SPEC and SPEC.loader
platform = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(platform)


def reference_record(tmp_path: Path) -> dict[str, str]:
    """Give every required identity a syntactically valid independent value."""
    return {
        "schema": platform.SCHEMA,
        "source_commit": "a" * 40,
        "app_path": str(tmp_path / "SwarmUI.app"),
        "app_binary_sha256": "b" * 64,
        "extension_binary_sha256": "c" * 64,
        "team_id": "KB88FQXUX2",
        "shortcuts_ax_path": str(tmp_path / "shortcuts.ax.txt"),
        "shortcuts_ax_sha256": "d" * 64,
    }


def test_reference_refuses_missing_or_changed_identity(tmp_path: Path) -> None:
    path = tmp_path / "reference.json"
    valid = reference_record(tmp_path)
    path.write_text(json.dumps(valid))
    assert platform.reference(path) == valid
    for changed in ({**valid, "extra": True},
                    {**valid, "extension_binary_sha256": "unknown"},
                    {**valid, "app_path": "relative.app"}):
        path.write_text(json.dumps(changed))
        with pytest.raises(ValueError):
            platform.reference(path)


def test_shortcuts_requires_search_and_exact_action() -> None:
    observed = b"""Window: \"New Shortcut 4\", App: Shortcuts.
  3 text Check Cohesix Apple Support
  6 search text field (settable) Value: Cohesix, Placeholder: Search
  9 table
    14 text Check Cohesix Apple Support
  25 toolbar
The focused UI element is 6 search text field (settable) Value: Cohesix, Placeholder: Search
"""
    assert platform.action_lines(observed)[2].endswith(platform.ACTION)
    with pytest.raises(ValueError, match="Shortcuts"):
        platform.action_lines(observed.replace(b"Check Cohesix", b"Check Other"))
    with pytest.raises(ValueError, match="Shortcuts"):
        platform.action_lines(observed.replace(b"Value: Cohesix", b"Value: Other"))
    with pytest.raises(ValueError, match="Shortcuts"):
        platform.action_lines(observed.replace(b"    14 text Check Cohesix Apple Support\n", b""))


def test_both_signatures_need_matching_keychain_group() -> None:
    group = ["KB88FQXUX2.com.cohesix.swarmui"]
    app = plistlib.dumps({"keychain-access-groups": group}).decode()
    extension = plistlib.dumps({
        "keychain-access-groups": group,
        "com.apple.security.app-sandbox": True,
        "com.apple.security.network.client": True,
    }).decode()
    platform.check_entitlements(app, "KB88FQXUX2", False)
    platform.check_entitlements(extension, "KB88FQXUX2", True)
    with pytest.raises(ValueError, match="Keychain"):
        platform.check_entitlements(app, "AAAAAAAAAA", False)
    with pytest.raises(ValueError, match="sandbox"):
        platform.check_entitlements(app, "KB88FQXUX2", True)
