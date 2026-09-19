#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Build and inspect isolated Python distributions from the compiler-selected source inventory.
# Copyright 2026 Lukas Bower
"""Build a wheel and sdist without copying an ambient checkout into a release."""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

PREFIX = "tools/cohesix-py/"
MAX_BYTES = 64 * 1024 * 1024
MAX_FILES = 128
METADATA = {"METADATA", "WHEEL", "entry_points.txt", "top_level.txt", "RECORD"}
EGG_INFO = {"PKG-INFO", "SOURCES.txt", "dependency_links.txt", "entry_points.txt",
            "requires.txt", "top_level.txt"}


def source_files(inventory: dict) -> list[str]:
    """Select package inputs from the existing generated release contract."""
    if inventory.get("schema") != "cohesix-implementation-surface-inventory/v1":
        raise ValueError("invalid implementation inventory schema")
    rows = inventory["release"]["python_artifacts"]
    if not isinstance(rows, list) or len(rows) > MAX_FILES or len(rows) != len(set(rows)):
        raise ValueError("invalid Python source inventory")
    selected = []
    for row in rows:
        if not isinstance(row, str) or not row.startswith(PREFIX):
            raise ValueError("Python source outside registered package")
        path = row.removeprefix(PREFIX)
        safe_path(path)
        if path in {"README.md", "pyproject.toml"} or re.fullmatch(r"cohesix/[a-z_]+\.py", path):
            selected.append(path)
        elif not path.startswith("examples/"):
            raise ValueError("non-product Python source in release inventory")
    if not {"pyproject.toml", "README.md", "cohesix/__init__.py"}.issubset(selected):
        raise ValueError("Python package metadata missing")
    return sorted(selected)


def safe_path(name: str) -> None:
    """Reject traversal, aliases, control bytes and ambiguous archive names."""
    path = PurePosixPath(name)
    if (not name or path.is_absolute() or str(path) != name or ".." in path.parts
            or "\\" in name or any(ord(char) < 32 for char in name)):
        raise ValueError("invalid package path")


def read_regular(path: Path, maximum: int = MAX_BYTES) -> bytes:
    """Reject symlinks and nonregular files before reading bounded bytes."""
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= maximum:
        raise ValueError("package input must be a bounded regular file")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(descriptor, "rb") as stream:
        opened = os.fstat(stream.fileno())
        if (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino):
            raise ValueError("package input replaced while opening")
        data = stream.read(maximum + 1)
        after = os.fstat(stream.fileno())
    if (len(data) != metadata.st_size or opened.st_mtime_ns != after.st_mtime_ns
            or opened.st_size != after.st_size):
        raise ValueError("package input changed while reading")
    return data


def inspect_wheel(path: Path, expected: dict[str, str], version: str) -> dict:
    """Require the exact source set, bounded metadata and valid wheel RECORD hashes."""
    raw = read_regular(path)
    prefix = f"cohesix-{version}.dist-info/"
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        infos = archive.infolist()
        names = [item.filename for item in infos]
        allowed = set(expected) | {prefix + name for name in METADATA}
        if len(names) != len(set(names)) or set(names) != allowed or len(names) > MAX_FILES:
            raise ValueError("wheel missing or unexpected files")
        if sum(info.file_size for info in infos) > MAX_BYTES:
            raise ValueError("wheel expanded byte bound")
        contents = {}
        for info in infos:
            safe_path(info.filename)
            mode = info.external_attr >> 16
            if info.is_dir() or (stat.S_IFMT(mode) not in (0, stat.S_IFREG)):
                raise ValueError("wheel entry must be a regular file")
            contents[info.filename] = archive.read(info)
        for name, digest in expected.items():
            if hashlib.sha256(contents[name]).hexdigest() != digest:
                raise ValueError("wheel source digest mismatch")
        records = list(csv.reader(io.StringIO(contents[prefix + "RECORD"].decode())))
        if len(records) != len(names) or {row[0] for row in records if len(row) == 3} != set(names):
            raise ValueError("wheel RECORD inventory mismatch")
        for row in records:
            name, digest, size = row
            if name == prefix + "RECORD":
                if digest or size:
                    raise ValueError("wheel RECORD self hash")
            else:
                data = contents[name]
                actual = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
                if digest != "sha256=" + actual or size != str(len(data)):
                    raise ValueError("wheel RECORD content mismatch")
        from email.parser import BytesParser
        metadata = BytesParser().parsebytes(contents[prefix + "METADATA"])
        if (metadata["Name"] != "cohesix" or metadata["Version"] != version
                or metadata["Requires-Python"] != ">=3.11"):
            raise ValueError("wheel package identity mismatch")
        wheel = BytesParser().parsebytes(contents[prefix + "WHEEL"])
        if wheel["Root-Is-Purelib"] != "true" or wheel.get_all("Tag") != ["py3-none-any"]:
            raise ValueError("wheel architecture mismatch")
    return artifact(path, raw, "wheel", "any", version, names)


def inspect_sdist(path: Path, expected: dict[str, str], version: str) -> dict:
    """Reject unlisted source, special tar entries and any mutable source mismatch."""
    raw = read_regular(path)
    prefix = f"cohesix-{version}/"
    generated = {"PKG-INFO", "setup.cfg"} | {"cohesix.egg-info/" + name for name in EGG_INFO}
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        found = set()
        seen = set()
        expanded = 0
        for ordinal, member in enumerate(archive, start=1):
            expanded += member.size
            if ordinal > MAX_FILES or expanded > MAX_BYTES:
                raise ValueError("sdist member or byte bound")
            safe_path(member.name)
            if member.name in seen:
                raise ValueError("sdist duplicate entry")
            seen.add(member.name)
            if not (member.name + "/").startswith(prefix):
                raise ValueError("sdist package root mismatch")
            relative = member.name.removeprefix(prefix)
            if member.isdir():
                if member.name not in {prefix.rstrip("/"), prefix + "cohesix", prefix + "cohesix.egg-info"}:
                    raise ValueError("sdist unexpected directory")
                continue
            if not member.isfile() or relative not in set(expected) | generated:
                raise ValueError("sdist missing or unexpected file type")
            found.add(relative)
            if relative in expected:
                stream = archive.extractfile(member)
                if stream is None or hashlib.sha256(stream.read()).hexdigest() != expected[relative]:
                    raise ValueError("sdist source digest mismatch")
        if found != set(expected) | generated:
            raise ValueError("sdist file inventory mismatch")
    return artifact(path, raw, "sdist", "source", version, sorted(found))


def artifact(path: Path, raw: bytes, kind: str, architecture: str, version: str, files: list[str]) -> dict:
    """Record observed distribution identity without claiming install or target proof."""
    return {"filename": path.name, "sha256": hashlib.sha256(raw).hexdigest(),
            "bytes": len(raw), "kind": kind, "architecture": architecture,
            "version": version, "files": sorted(files)}


def build(repo: Path, inventory_path: Path, out: Path) -> dict:
    """Copy only explicit product files into a private temporary build directory."""
    inventory_bytes = read_regular(inventory_path)
    selected = source_files(json.loads(inventory_bytes))
    out.mkdir(parents=True, exist_ok=False)
    digests = {}
    with tempfile.TemporaryDirectory(prefix="cohesix-python-") as temporary:
        stage = Path(temporary)
        total_bytes = 0
        for relative in selected:
            source = repo / PREFIX / relative
            if source.resolve(strict=True) != source.absolute():
                raise ValueError("Python source symlink component")
            data = read_regular(source)
            total_bytes += len(data)
            if total_bytes > MAX_BYTES:
                raise ValueError("Python source byte bound")
            digests[relative] = hashlib.sha256(data).hexdigest()
            target = stage / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        project = tomllib.loads((stage / "pyproject.toml").read_text())
        if project["project"]["name"] != "cohesix":
            raise ValueError("unexpected Python project")
        # The existing version has an alpha spelling normalized by packaging.
        from packaging.version import Version
        version = str(Version(project["project"]["version"]))
        environment = os.environ.copy()
        environment.pop("PYTHONPATH", None)
        environment["PYTHONNOUSERSITE"] = "1"
        with (out / "build.log").open("xb") as log:
            for method in ("build_sdist", "build_wheel"):
                # Each backend hook has a fresh interpreter: setuptools owns
                # sys.argv and build state inside each invocation.
                code = f"from setuptools.build_meta import {method}; import sys; {method}(sys.argv[1])"
                subprocess.run([sys.executable, "-c", code, str(out.resolve())], cwd=stage,
                               env=environment, stdin=subprocess.DEVNULL, stdout=log,
                               stderr=subprocess.STDOUT, check=True, timeout=120)
    wheel = out / f"cohesix-{version}-py3-none-any.whl"
    sdist = out / f"cohesix-{version}.tar.gz"
    modules = {name: value for name, value in digests.items() if name.startswith("cohesix/")}
    report = {"schema": "cohesix-python-distributions/v1", "authoritative": False,
              "inventory_sha256": hashlib.sha256(inventory_bytes).hexdigest(),
              "source_files": digests, "artifacts": [inspect_wheel(wheel, modules, version),
                                                       inspect_sdist(sdist, digests, version)]}
    (out / "distributions.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    return report


def main() -> int:
    """Build distributions from selected generated metadata."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument(
        "--inventory", type=Path,
        default=Path("configs/generated/implementation_surface_inventory.json"),
    )
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = build(args.repo.resolve(), args.inventory.resolve(), args.out.resolve())
    print(json.dumps({"result": "PASS", "artifacts": [row["filename"] for row in report["artifacts"]]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
