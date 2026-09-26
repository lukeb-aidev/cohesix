#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Build signed, exact-release Ubuntu ARM64 controller and optional desktop packages without service activation.
# Copyright 2026 Lukas Bower
"""Create native packages from an independently pinned release archive."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile

from native_package import load_reference, normalize_public_modes, stage_subset, write_manifest

ROOT = Path(__file__).resolve().parents[2]
DESKTOP_ENTRY = ROOT / "packaging/debian/com.cohesix.swarmui.desktop"
ICON = ROOT / "apps/swarmui/frontend/assets/icons/cohesix-icon.svg"


def debian_version(version: str) -> str:
    """Sort beta versions before their corresponding stable versions."""
    return version.replace("-beta", "~beta")


def checked(command: list[str]) -> None:
    """Run fixed package tools without shell interpolation or logged secrets."""
    subprocess.run(command, check=True, timeout=120)


def add_file(source: Path, target: Path, installed: str) -> dict[str, object]:
    """Copy one repository-owned desktop asset into the package root."""
    if source.is_symlink() or not source.is_file():
        raise ValueError("desktop asset must be a regular file")
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    target.chmod(0o644)
    return {
        "path": installed, "size": target.stat().st_size,
        "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
    }


def make_package(reference: dict[str, str], root: Path, subset: str,
                 output: Path, maintainer: str) -> dict[str, object]:
    """Build one package with declared ownership and no maintainer side effects."""
    package = "cohesix-controller" if subset == "controller" else "cohesix-swarmui"
    install_root = root / "usr/lib/cohesix"
    records = stage_subset(Path(reference["bundle"]), install_root, subset)
    records = [{**row, "path": "/usr/lib/cohesix/" + row["path"]} for row in records]
    if subset == "desktop":
        records.extend([
            add_file(
                DESKTOP_ENTRY,
                root / "usr/share/applications/com.cohesix.swarmui.desktop",
                "/usr/share/applications/com.cohesix.swarmui.desktop",
            ),
            add_file(
                ICON,
                root / "usr/share/icons/hicolor/scalable/apps/com.cohesix.swarmui.svg",
                "/usr/share/icons/hicolor/scalable/apps/com.cohesix.swarmui.svg",
            ),
        ])
    write_manifest(
        root / f"usr/share/doc/{package}/payload.json",
        reference=reference, records=records, package=package,
    )
    if subset == "controller":
        depends = "libc6 (>= 2.35)"
        description = "Cohesix host controller and offline Python client assets"
    else:
        depends = (
            f"cohesix-controller (= {debian_version(reference['version'])}), "
            "libc6 (>= 2.35), libgtk-3-0t64 | libgtk-3-0, "
            "libwebkit2gtk-4.1-0"
        )
        description = "Cohesix SwarmUI GNOME desktop client"
    control = root / "DEBIAN/control"
    control.parent.mkdir(parents=True, exist_ok=True)
    control.write_text(
        f"Package: {package}\n"
        f"Version: {debian_version(reference['version'])}\n"
        "Section: admin\nPriority: optional\nArchitecture: arm64\n"
        f"Maintainer: {maintainer}\n"
        f"Depends: {depends}\nDescription: {description}\n"
    )
    control.chmod(0o644)
    normalize_public_modes(root)
    destination = output / f"{package}_{debian_version(reference['version'])}_arm64.deb"
    checked(["dpkg-deb", "--build", "--root-owner-group", str(root), str(destination)])
    return {
        "package": package, "path": destination.name,
        "size": destination.stat().st_size,
        "sha256": hashlib.sha256(destination.read_bytes()).hexdigest(),
    }


def build(reference_path: Path, output: Path, key: str | None,
          maintainer: str | None) -> dict[str, object]:
    """Bind both packages to the same archive and external publisher key."""
    if platform.system() != "Linux" or platform.machine() != "aarch64":
        raise ValueError("Ubuntu ARM64 packages must be built on native Linux AArch64")
    if not shutil.which("dpkg-deb") or not shutil.which("gpg"):
        raise ValueError("dpkg-deb and gpg are required for native publication")
    if not key or not key.isascii() or len(key) > 128 or key.startswith("-"):
        raise ValueError("select a publisher signing key outside the payload")
    if not maintainer or not re.fullmatch(
        r"[A-Za-z][A-Za-z .'-]{0,79} <[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}>",
        maintainer,
    ):
        raise ValueError("select a public Debian maintainer name and email")
    reference = load_reference(reference_path, "linux")
    if output.exists() or output.is_symlink():
        raise ValueError("installer output already exists")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="m28g-deb-", dir=output.parent) as temporary:
        work = Path(temporary)
        published = work / "published"
        published.mkdir()
        packages = [
            make_package(reference, work / "controller", "controller", published, maintainer),
            make_package(reference, work / "desktop", "desktop", published, maintainer),
        ]
        manifest = {
            "schema": "cohesix-m28g-installers/v1",
            "host": "linux", "version": reference["version"],
            "source_commit": reference["source_commit"],
            "archive_sha256": reference["archive_sha256"],
            "packages": packages,
            "publisher_key": key,
            "state": "built-signed-unqualified",
        }
        manifest_path = published / "installers.json"
        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        checked([
            "gpg", "--batch", "--yes", "--armor", "--detach-sign",
            "--local-user", key, "--output", str(published / "installers.json.asc"),
            str(manifest_path),
        ])
        os.replace(published, output)
    return manifest


def main() -> None:
    """Use an existing keyring; never import publisher trust from a package."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference-config", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = build(
            args.reference_config, args.out,
            os.environ.get("COHESIX_DEB_SIGNING_KEY"),
            os.environ.get("COHESIX_DEB_MAINTAINER"),
        )
    except (ValueError, OSError, subprocess.TimeoutExpired,
            subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        parser.error(str(exc))
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
