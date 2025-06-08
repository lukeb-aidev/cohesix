#!/usr/bin/env python3
// CLASSIFICATION: COMMUNITY
// Filename: cohpkg.py v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-13
"""cohpkg – minimal package manager for Cohesix."""

import argparse
import json
import os
from pathlib import Path
import tarfile

MANIFEST = Path("/srv/updates/manifest.json")
UPDATE_DIR = Path("/srv/updates")
INSTALL_DIR = Path("/opt")


def load_manifest():
    if MANIFEST.exists():
        return json.loads(MANIFEST.read_text())
    return {"packages": []}


def list_packages():
    data = load_manifest()
    for pkg in data.get("packages", []):
        print(f"{pkg['name']} {pkg['version']}")


def install(pkg_name: str):
    data = load_manifest()
    for pkg in data.get("packages", []):
        if pkg["name"] == pkg_name:
            tarball = UPDATE_DIR / pkg["file"]
            if not tarball.exists():
                print(f"Package file missing: {tarball}")
                return
            dest = INSTALL_DIR / pkg_name
            dest.mkdir(parents=True, exist_ok=True)
            with tarfile.open(tarball) as tf:
                tf.extractall(dest)
            print(f"Installed {pkg_name}")
            return
    print(f"Package {pkg_name} not found")


def main():
    p = argparse.ArgumentParser(description="cohpkg package manager")
    sub = p.add_subparsers(dest="cmd")
    sub.add_parser("list", help="List available packages")
    inst = sub.add_parser("install", help="Install a package")
    inst.add_argument("name")
    upd = sub.add_parser("update", help="Update a package")
    upd.add_argument("name")

    args = p.parse_args()
    if args.cmd == "list":
        list_packages()
    elif args.cmd in ("install", "update"):
        install(args.name)
    else:
        p.print_help()

if __name__ == "__main__":
    main()
