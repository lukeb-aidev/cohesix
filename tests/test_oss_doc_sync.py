# CLASSIFICATION: COMMUNITY
# Filename: test_oss_doc_sync.py v0.1
# Author: Cohesix Codex
# Date Modified: 2025-07-14
"""Ensure OPEN_SOURCE_DEPENDENCIES.md lists Cargo dependencies."""
import types

try:
    import tomllib

    toml_parser: types.ModuleType = tomllib
except ModuleNotFoundError:
    import tomli  # type: ignore[import-not-found]

    toml_parser = tomli
from pathlib import Path

DOC_PATH = Path("docs/community/OPEN_SOURCE_DEPENDENCIES.md")
DOC = DOC_PATH.read_text() if DOC_PATH.exists() else None

CARGO = Path("Cargo.toml")


def cargo_deps():
    data = toml_parser.loads(CARGO.read_text())
    deps = []
    for name, spec in data.get("dependencies", {}).items():
        if isinstance(spec, dict) and spec.get("optional"):
            continue
        deps.append(name)
    return deps


def test_dependencies_listed():
    if DOC is None:
        import pytest

        pytest.skip("dependency document missing")
    deps = cargo_deps()
    listed = [d for d in deps if d in DOC]
    assert len(listed) >= 5
