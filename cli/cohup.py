#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: cohup.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-12

"""cohup – live patching utility."""

import argparse
import hashlib
from pathlib import Path


def parse_args():
    parser = argparse.ArgumentParser(description="cohup patch tool")
    parser.add_argument("--man", action="store_true", help="Show man page")
    sub = parser.add_subparsers(dest="command")
    patch_cmd = sub.add_parser("patch", help="Apply a live patch")
    patch_cmd.add_argument("target")
    patch_cmd.add_argument("binary")
    return parser.parse_args()


def main():
    args = parse_args()
    if getattr(args, "man", False):
        man = os.path.join(os.path.dirname(__file__), "../bin/man")
        page = os.path.join(os.path.dirname(__file__), "../docs/man/cohup.1")
        os.execv(man, [man, page])
    if args.command == "patch":
        apply_patch(args.target, args.binary)
    else:
        parser = argparse.ArgumentParser()
        parser.print_help()


def apply_patch(target: str, binary_path: str):
    data = Path(binary_path).read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    log = Path("/srv/updates/patch.log")
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("a") as f:
        f.write(f"patch {target} {digest}\n")
    Path(target).write_bytes(data)
    print(f"Patched {target} with hash {digest}")


if __name__ == "__main__":
    main()
