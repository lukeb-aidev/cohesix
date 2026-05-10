#!/usr/bin/env python3
# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Validate SwarmUI transitive dependency policy outside the Rust test harness.
# Author: Lukas Bower

"""Check active SwarmUI dependency policy for default and minimal feature sets."""

from __future__ import annotations

import pathlib
import subprocess
import sys
from collections.abc import Sequence


BANNED_HTTP_DEPS = {
    "actix-web",
    "axum",
    "hyper",
    "hyper-util",
    "isahc",
    "reqwest",
    "reqwest-middleware",
    "rocket",
    "surf",
    "tower-http",
    "ureq",
    "warp",
}


def repo_root() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parents[2]


def run_command(args: Sequence[str], root: pathlib.Path) -> str:
    completed = subprocess.run(
        args,
        cwd=root,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if completed.returncode != 0:
        command = " ".join(args)
        print(f"{command} failed:\n{completed.stderr}", file=sys.stderr)
        raise SystemExit(completed.returncode)
    return completed.stdout


def host_triple(root: pathlib.Path) -> str:
    output = run_command(["rustc", "-vV"], root)
    for line in output.splitlines():
        if line.startswith("host: "):
            return line.removeprefix("host: ").strip()
    raise RuntimeError("rustc -vV did not report a host triple")


def cargo_tree_names(
    root: pathlib.Path, host: str, feature_args: Sequence[str]
) -> set[str]:
    args = [
        "cargo",
        "tree",
        "--manifest-path",
        "apps/swarmui/Cargo.toml",
        "-p",
        "swarmui",
        "--target",
        host,
        "--edges",
        "normal",
        "--prefix",
        "none",
        "--format",
        "{p}",
        *feature_args,
    ]
    names = set()
    for line in run_command(args, root).splitlines():
        line = line.strip()
        if line:
            names.add(line.split(maxsplit=1)[0])
    return names


def check_feature_set(
    root: pathlib.Path,
    host: str,
    label: str,
    feature_args: Sequence[str],
    allowed_banned: set[str],
) -> list[str]:
    names = cargo_tree_names(root, host, feature_args)
    found = sorted((BANNED_HTTP_DEPS & names) - allowed_banned)
    if found:
        return [
            f"{label}: forbidden HTTP/server dependencies detected: "
            + ", ".join(found)
        ]
    print(f"{label}: dependency policy ok")
    return []


def main() -> int:
    root = repo_root()
    host = host_triple(root)
    errors = []
    errors.extend(
        check_feature_set(
            root,
            host,
            "swarmui-default-rest",
            [],
            allowed_banned={"ureq"},
        )
    )
    errors.extend(
        check_feature_set(
            root,
            host,
            "swarmui-no-default-features",
            ["--no-default-features"],
            allowed_banned=set(),
        )
    )
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
