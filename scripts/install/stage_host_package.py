#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Stage only compiler-registered native and offline Python adoption assets before signed package construction.
# Copyright 2026 Lukas Bower
"""Stage a selected host profile from exact binaries and generated/source inputs."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import stat


def stage(
    repo: Path, generated: Path, binaries: Path, profile: str, output: Path,
    apple_extension: Path | None = None,
) -> None:
    """Copy the registered file set; coh package build owns format/signature checks."""
    registry = json.loads(
        (generated / "configs/generated/provider_registry.json").read_text()
    )
    matches = [
        row
        for row in registry["contract"]["deployment_profiles"]
        if row["id"] == profile
    ]
    if len(matches) != 1:
        raise ValueError("select one registered deployment profile")
    sources = {
        "SwarmUI.app/Contents/Info.plist": "packaging/swarmui/Info.plist",
        "config/coh_policy.toml": "configs/generated/coh_policy.toml",
        "config/cohsh_policy.toml": "configs/generated/cohsh_policy.toml",
        "config/root_task_resolved.json": "configs/generated/root_task_resolved.json",
        "contracts/provider_registry.json": "configs/generated/provider_registry.json",
        "contracts/implementation_surface_inventory.json": "configs/generated/implementation_surface_inventory.json",
    }
    extension_prefix = "SwarmUI.app/Contents/Extensions/SwarmUIIntents.appex/"
    extension_files = {
        "Contents/MacOS/SwarmUIIntents",
        "Contents/Info.plist",
        "Contents/Resources/Metadata.appintents/extract.actionsdata",
        "Contents/Resources/Metadata.appintents/version.json",
    }
    if profile == "macos-desktop" and apple_extension is None:
        raise ValueError("macOS desktop package requires a built App Intents extension")
    if apple_extension is not None and (
        profile != "macos-desktop"
        or not apple_extension.is_absolute()
        or apple_extension.is_symlink()
        or not apple_extension.is_dir()
        or apple_extension.name != "SwarmUIIntents.appex"
    ):
        raise ValueError("invalid Apple extension selection")
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    for artifact in matches[0]["artifacts"]:
        name = artifact["path"]
        if name == "package.sbom.json":
            continue
        if Path(name).is_absolute() or ".." in Path(name).parts:
            raise ValueError("unsafe registered path")
        if name.startswith(extension_prefix):
            relative = name[len(extension_prefix):]
            if apple_extension is None or relative not in extension_files:
                raise ValueError("undeclared Apple extension artifact")
            source = apple_extension / relative
        elif name.startswith("bin/") or name == "SwarmUI.app/Contents/MacOS/swarmui":
            source = binaries / Path(name).name
        elif name == "SwarmUI.app/Contents/Info.plist":
            source = repo / sources[name]
        elif name in sources:
            source = generated / sources[name]
        else:
            candidate = generated / name
            source = candidate if candidate.exists() else repo / name
        if (
            source.is_symlink()
            or not stat.S_ISREG(source.stat().st_mode)
            or any(parent.is_symlink() for parent in source.parents)
        ):
            raise ValueError("package source must be a regular non-symlink file")
        target = output / name
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        target.chmod(0o700 if artifact["executable"] else 0o600)


def main() -> None:
    """Staging grants no install, service activation or trust authority."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--generated-root", type=Path, required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--apple-extension-dir", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    stage(
        args.repo.absolute(),
        args.generated_root.absolute(),
        args.bin_dir.absolute(),
        args.profile,
        args.out.absolute(),
        args.apple_extension_dir.absolute() if args.apple_extension_dir else None,
    )
    print(json.dumps({"state": "staged-unverified", "profile": args.profile}))


if __name__ == "__main__":
    main()
