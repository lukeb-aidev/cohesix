#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Build and inspect the exact source-selected offline NeMo client wheel for Release B.
# Copyright 2026 Lukas Bower
"""Bind the NeMo wheel to compiler-selected source bytes without native claims."""

from __future__ import annotations

import argparse
import ast
import base64
import csv
from email.parser import BytesParser
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import tomllib
import zipfile

from packaging.version import Version

from build_python_package import read_regular, safe_path

PREFIX = "integrations/nemo-agent-toolkit/"
DIST_INFO = {"METADATA", "WHEEL", "entry_points.txt", "top_level.txt", "RECORD"}
MAX_FILES = 32
MAX_BYTES = 32 * 1024 * 1024


def selected_sources(inventory: dict) -> list[str]:
    """Select only the registered kit files, including its dependency lock."""
    if inventory.get("schema") != "cohesix-implementation-surface-inventory/v1":
        raise ValueError("invalid implementation inventory")
    paths = [
        path.removeprefix(PREFIX)
        for path in inventory["release"]["support_files"]
        if isinstance(path, str) and path.startswith(PREFIX)
    ]
    expected = {
        "README.md", "pyproject.toml", "requirements-linux-aarch64.lock",
        "src/cohesix_nemo_kit/__init__.py",
        "src/cohesix_nemo_kit/cli.py",
        "src/cohesix_nemo_kit/native.py",
        "src/cohesix_nemo_kit/nat_plugin.py",
        "src/cohesix_nemo_kit/configs/a2a-agent.yaml",
        "src/cohesix_nemo_kit/configs/direct-agent.yaml",
        "src/cohesix_nemo_kit/configs/mcp-agent.yaml",
    }
    if len(paths) != len(set(paths)) or set(paths) != expected:
        raise ValueError("release NeMo source selection is incomplete")
    for path in paths:
        safe_path(path)
    return sorted(paths)


def inspect_wheel(path: Path, sources: dict[str, str], version: str) -> dict:
    """Check each installed module, metadata and RECORD against selected bytes."""
    raw = read_regular(path, MAX_BYTES)
    prefix = f"cohesix_nemo_kit-{version}.dist-info/"
    expected = {
        name.removeprefix("src/"): digest
        for name, digest in sources.items() if name.startswith("src/")
    }
    allowed = set(expected) | {prefix + name for name in DIST_INFO}
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        infos = archive.infolist()
        names = [item.filename for item in infos]
        if len(names) != len(set(names)) or set(names) != allowed or len(names) > MAX_FILES:
            raise ValueError("NeMo wheel file inventory differs from selected source")
        if sum(item.file_size for item in infos) > MAX_BYTES:
            raise ValueError("NeMo wheel expanded byte bound")
        contents: dict[str, bytes] = {}
        for item in infos:
            safe_path(item.filename)
            if item.is_dir() or stat.S_IFMT(item.external_attr >> 16) not in (0, stat.S_IFREG):
                raise ValueError("NeMo wheel entry is not a regular file")
            contents[item.filename] = archive.read(item)
    for name, digest in expected.items():
        if hashlib.sha256(contents[name]).hexdigest() != digest:
            raise ValueError("NeMo wheel source digest mismatch")
    module = ast.parse(contents["cohesix_nemo_kit/__init__.py"].decode("utf-8"))
    declared_versions = [
        statement.value.value
        for statement in module.body
        if isinstance(statement, ast.Assign)
        and len(statement.targets) == 1
        and isinstance(statement.targets[0], ast.Name)
        and statement.targets[0].id == "__version__"
        and isinstance(statement.value, ast.Constant)
        and isinstance(statement.value.value, str)
    ]
    if declared_versions != [version]:
        raise ValueError("NeMo module version differs from selected release")
    records = list(csv.reader(io.StringIO(contents[prefix + "RECORD"].decode())))
    if (len(records) != len(names) or any(len(row) != 3 for row in records)
            or {row[0] for row in records} != set(names)):
        raise ValueError("NeMo wheel RECORD inventory mismatch")
    for name, digest, size in records:
        if name == prefix + "RECORD":
            if digest or size:
                raise ValueError("NeMo wheel RECORD self hash")
        else:
            data = contents[name]
            actual = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
            if digest != "sha256=" + actual or size != str(len(data)):
                raise ValueError("NeMo wheel RECORD content mismatch")
    metadata = BytesParser().parsebytes(contents[prefix + "METADATA"])
    wheel = BytesParser().parsebytes(contents[prefix + "WHEEL"])
    python_requirement = metadata["Requires-Python"]
    if (metadata["Name"] != "cohesix-nemo-kit"
            or metadata["Version"] != version
            or not isinstance(python_requirement, str)
            or set(python_requirement.split(",")) != {">=3.12", "<3.14"}
            or wheel["Root-Is-Purelib"] != "true"
            or wheel.get_all("Tag") != ["py3-none-any"]):
        raise ValueError("NeMo wheel package identity differs from selected source")
    return {"filename": path.name, "sha256": hashlib.sha256(raw).hexdigest(),
            "size": len(raw), "version": version}


def build(repo: Path, inventory_path: Path, output: Path) -> dict:
    """Stage selected files into an isolated build root and retain exact hashes."""
    inventory_bytes = read_regular(inventory_path)
    inventory = json.loads(inventory_bytes)
    selected = selected_sources(inventory)
    release_version = str(Version(inventory["release"]["version"]))
    output.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix="cohesix-nemo-") as temporary:
        stage = Path(temporary)
        digests = {}
        for relative in selected:
            source = repo / PREFIX / relative
            if source.resolve(strict=True) != source.absolute():
                raise ValueError("NeMo source has a symlinked path component")
            data = read_regular(source)
            digests[relative] = hashlib.sha256(data).hexdigest()
            destination = stage / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        project = tomllib.loads((stage / "pyproject.toml").read_text())
        if project["project"]["name"] != "cohesix-nemo-kit":
            raise ValueError("unexpected NeMo project")
        version = str(Version(project["project"]["version"]))
        if version != release_version:
            raise ValueError("NeMo kit version differs from selected release contract")
        environment = os.environ.copy()
        environment.pop("PYTHONPATH", None)
        environment.update(PYTHONNOUSERSITE="1", PYTHONHASHSEED="0",
                           SOURCE_DATE_EPOCH="315532800")
        with (output / "build.log").open("xb") as log:
            code = "from setuptools.build_meta import build_wheel; import sys; build_wheel(sys.argv[1])"
            subprocess.run([sys.executable, "-c", code, str(output.resolve())],
                           cwd=stage, env=environment, stdin=subprocess.DEVNULL,
                           stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
    wheel = output / f"cohesix_nemo_kit-{version}-py3-none-any.whl"
    artifact = inspect_wheel(wheel, digests, version)
    report = {
        "schema": "cohesix-nemo-distribution/v1",
        "authoritative": False,
        "inventory_sha256": hashlib.sha256(inventory_bytes).hexdigest(),
        "source_files": digests,
        "wheel": artifact,
        "proof_boundary": {"kit_install_is_target_proof": False,
                           "toolkit_output_is_native_outcome": False},
    }
    (output / "nemo-distribution.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n"
    )
    return report


def verify(repo: Path, inventory_path: Path, directory: Path) -> dict:
    """Recheck an offline wheel and report before either native archive copies it."""
    inventory_bytes = read_regular(inventory_path)
    inventory = json.loads(inventory_bytes)
    selected = selected_sources(inventory)
    version = str(Version(inventory["release"]["version"]))
    report = json.loads(read_regular(directory / "nemo-distribution.json", 1024 * 1024))
    source_digests = {
        name: hashlib.sha256(read_regular(repo / PREFIX / name)).hexdigest()
        for name in selected
    }
    wheel = directory / f"cohesix_nemo_kit-{version}-py3-none-any.whl"
    observed = inspect_wheel(wheel, source_digests, version)
    if (
        report.get("schema") != "cohesix-nemo-distribution/v1"
        or report.get("authoritative") is not False
        or report.get("inventory_sha256") != hashlib.sha256(inventory_bytes).hexdigest()
        or report.get("source_files") != source_digests
        or report.get("wheel") != observed
        or report.get("proof_boundary") != {
            "kit_install_is_target_proof": False,
            "toolkit_output_is_native_outcome": False,
        }
    ):
        raise ValueError("NeMo distribution differs from selected release source")
    return report


def main() -> None:
    """Create an unpublished offline kit candidate from the selected inventory."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--inventory", type=Path,
                        default=Path("configs/generated/implementation_surface_inventory.json"))
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--out", type=Path)
    action.add_argument("--verify-dir", type=Path)
    args = parser.parse_args()
    result = (build(args.repo.resolve(), args.inventory.resolve(), args.out.resolve())
              if args.out else verify(args.repo.resolve(), args.inventory.resolve(),
                                      args.verify_dir.resolve()))
    print(json.dumps({"result": "PASS", "wheel": result["wheel"]["filename"]}))


if __name__ == "__main__":
    main()
