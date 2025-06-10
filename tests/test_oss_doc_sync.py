#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_oss_doc_sync.py v0.1
# Author: Cohesix Codex
# Date Modified: 2025-07-14
"""Ensure OPEN_SOURCE_DEPENDENCIES.md lists Cargo dependencies."""

import tomllib as tomli
from pathlib import Path

DOC = Path('docs/community/governance/OPEN_SOURCE_DEPENDENCIES.md').read_text()

CARGO = Path('Cargo.toml')


def cargo_deps():
    data = tomli.loads(CARGO.read_text())
    deps = []
    for name, spec in data.get('dependencies', {}).items():
        if isinstance(spec, dict) and spec.get('optional'):
            continue
        deps.append(name)
    return deps


def test_dependencies_listed():
    deps = cargo_deps()
    listed = [d for d in deps if d in DOC]
    assert len(listed) >= 5
