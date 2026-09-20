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

from stage_host_package import stage


def main() -> None:
    """Keep browser companions separate from the exact signed native package membership."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--generated-root", type=Path, required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--profile", choices=["macos-desktop", "linux-aarch64-desktop"], required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    output = args.out.absolute()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    stage(args.repo.absolute(), args.generated_root.absolute(), args.bin_dir.absolute(), args.profile, output / "package-input")
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
    (output / "ui-assets.json").write_text(json.dumps({"kind": "qualification-companion", "files": records}, indent=2) + "\n")
    print(json.dumps({"state": "staged-unverified", "profile": args.profile, "assets": len(records)}))


if __name__ == "__main__":
    main()
