#!/usr/bin/env python3
# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Validate SwarmUI transitive dependency policy outside the Rust test harness.
# Author: Lukas Bower

"""Check SwarmUI dependency policy for default and minimal feature sets."""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
from collections.abc import Sequence
from typing import Any


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


def cargo_metadata(
    root: pathlib.Path, host: str, feature_args: Sequence[str]
) -> dict[str, Any]:
    args = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--manifest-path",
        "apps/swarmui/Cargo.toml",
        "--filter-platform",
        host,
        *feature_args,
    ]
    return json.loads(run_command(args, root))


def swarmui_dependency_names(metadata: dict[str, Any]) -> set[str]:
    package_name_by_id = {
        package["id"]: package["name"] for package in metadata["packages"]
    }
    swarmui_id = next(
        package_id
        for package_id, name in package_name_by_id.items()
        if name == "swarmui"
    )
    node_by_id = {
        node["id"]: node for node in metadata["resolve"]["nodes"]
    }
    pending = [swarmui_id]
    seen: set[str] = set()
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        node = node_by_id[package_id]
        pending.extend(dep["pkg"] for dep in node["deps"])
    return {
        package_name_by_id[package_id]
        for package_id in seen
        if package_id in package_name_by_id
    }


def check_feature_set(
    root: pathlib.Path,
    host: str,
    label: str,
    feature_args: Sequence[str],
    allowed_banned: set[str],
) -> list[str]:
    metadata = cargo_metadata(root, host, feature_args)
    names = swarmui_dependency_names(metadata)
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
