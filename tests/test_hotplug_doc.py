#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_hotplug_doc.py v0.1
# Author: Cohesix Codex
# Date Modified: 2025-07-14
"""Ensure HOTPLUG.md is present and non-empty."""

from pathlib import Path

DOC = Path('docs/devices/HOTPLUG.md')

def test_hotplug_doc_exists():
    assert DOC.exists(), 'HOTPLUG.md must exist'
    assert DOC.read_text().strip(), 'HOTPLUG.md should not be empty'
