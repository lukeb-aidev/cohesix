# CLASSIFICATION: COMMUNITY
# Filename: cohpkg.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-15
"""cohpkg – minimal package manager for Cohesix."""

import argparse
import json
import os
from pathlib import Path
import tarfile
import shlex
import subprocess
from datetime import datetime
from typing import List
import traceback

LOG_DIR = Path("/log")
LOG_DIR.mkdir(parents=True, exist_ok=True)


def cohlog(msg: str) -> None:
    with (LOG_DIR / "cli_tool.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {msg}\n")
    print(msg)


def safe_run(cmd: List[str]) -> int:
    quoted = [shlex.quote(c) for c in cmd]
    with (LOG_DIR / "cli_exec.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {' '.join(quoted)}\n")
    result = subprocess.run(cmd)
    return result.returncode

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
        cohlog(f"{pkg['name']} {pkg['version']}")


def install(pkg_name: str):
    data = load_manifest()
    for pkg in data.get("packages", []):
        if pkg["name"] == pkg_name:
            tarball = UPDATE_DIR / pkg["file"]
            if not tarball.exists():
                cohlog(f"Package file missing: {tarball}")
                return
            dest = INSTALL_DIR / pkg_name
            dest.mkdir(parents=True, exist_ok=True)
            with tarfile.open(tarball) as tf:
                tf.extractall(dest)
            cohlog(f"Installed {pkg_name}")
            return
    cohlog(f"Package {pkg_name} not found")


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
    try:
        main()
    except Exception:
        with (LOG_DIR / "cli_error.log").open("a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {traceback.format_exc()}\n")
        cohlog("Unhandled error, see cli_error.log")
        sys.exit(1)
