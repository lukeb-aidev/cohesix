#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Stage registered desktop package inputs and identity-bound offline browser assets for focused native qualification.
# Copyright 2026 Lukas Bower
"""Stage an unpublished desktop candidate; the canonical package builder owns signing."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from stage_host_package import stage


def stage_macos_icon(source: Path, app: Path) -> dict[str, str]:
    """Render the selected square vector icon before the app is code-signed."""
    if source.is_symlink() or not source.is_file() or source.stat().st_size > 64 * 1024:
        raise ValueError("selected Mac icon source is invalid")
    destination = app / "Contents/Resources/SwarmUI.icns"
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="cohesix-icon-") as directory:
        iconset = Path(directory) / "SwarmUI.iconset"
        iconset.mkdir()
        for pixels, name in (
            (16, "icon_16x16.png"), (32, "icon_16x16@2x.png"),
            (32, "icon_32x32.png"), (64, "icon_32x32@2x.png"),
            (128, "icon_128x128.png"), (256, "icon_128x128@2x.png"),
            (256, "icon_256x256.png"), (512, "icon_256x256@2x.png"),
            (512, "icon_512x512.png"), (1024, "icon_512x512@2x.png"),
        ):
            subprocess.run(
                ["/usr/bin/sips", "-s", "format", "png", "-z", str(pixels),
                 str(pixels), str(source), "--out", str(iconset / name)],
                capture_output=True, check=True, timeout=20,
            )
        subprocess.run(
            ["/usr/bin/iconutil", "-c", "icns", str(iconset),
             "-o", str(destination)],
            capture_output=True, check=True, timeout=30,
        )
    data = destination.read_bytes()
    if len(data) < 8 or data[:4] != b"icns" or int.from_bytes(data[4:8], "big") != len(data):
        raise ValueError("generated Mac icon is invalid")
    destination.chmod(0o600)
    return {
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "icns_sha256": hashlib.sha256(data).hexdigest(),
    }


def main() -> None:
    """Keep browser companions separate from the exact signed native package membership."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--generated-root", type=Path, required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--profile", choices=["macos-desktop", "linux-aarch64-desktop"], required=True)
    parser.add_argument("--apple-extension-dir", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    output = args.out.absolute()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    stage(
        args.repo.absolute(), args.generated_root.absolute(), args.bin_dir.absolute(),
        args.profile, output / "package-input",
        args.apple_extension_dir.absolute() if args.apple_extension_dir else None,
    )
    icon = None
    if args.profile == "macos-desktop":
        icon = stage_macos_icon(
            args.repo.absolute() / "apps/swarmui/frontend/assets/icons/cohesix-icon.svg",
            output / "package-input/SwarmUI.app",
        )
    source = args.repo.absolute() / "apps/swarmui/frontend"
    entries = sorted(source.rglob("*"))
    if any(path.is_symlink() for path in entries):
        raise ValueError("desktop assets cannot contain symlinks")
    files = [path for path in entries if path.is_file()]
    if len(files) > 128 or sum(path.stat().st_size for path in files) > 32 * 1024 * 1024:
        raise ValueError("desktop asset bounds")
    records = []
    for path in files:
        relative = path.relative_to(source)
        destination = output / "ui/swarmui" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        records.append({"path": str(relative), "sha256": hashlib.sha256(destination.read_bytes()).hexdigest()})
    (output / "ui-assets.json").write_text(json.dumps(
        {"kind": "qualification-companion", "files": records, "app_icon": icon},
        indent=2,
    ) + "\n")
    print(json.dumps({"state": "staged-unverified", "profile": args.profile, "assets": len(records)}))


if __name__ == "__main__":
    main()
