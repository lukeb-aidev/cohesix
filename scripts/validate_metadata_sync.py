#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: validate_metadata_sync.py v0.1
# Date Modified: 2025-06-09
# Author: Lukas Bower
"""Metadata synchronization validator for Cohesix documentation."""

from __future__ import annotations
import re
import sys
from pathlib import Path

METADATA_PATH = Path('docs/community/METADATA.md')

# Regex patterns for headers
CLASS_RE = re.compile(r"CLASSIFICATION:\s*(\w+)")
FILE_RE = re.compile(r"Filename:\s*\S+\s+(v[0-9.]+)")


def parse_metadata(path: Path):
    entries = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line.startswith('|') or line.startswith('|-'):
                continue
            cols = [c.strip() for c in line.strip('|').split('|')]
            if len(cols) < 4:
                continue
            filename, version, _date, classification = cols[:4]
            entries.append((filename, version, classification))
    return entries


def check_file(filename: str, version: str, classification: str):
    possible_paths = [Path('docs/community') / filename, Path('docs/private') / filename]
    for p in possible_paths:
        if p.exists():
            with p.open() as f:
                lines = [next(f, '') for _ in range(5)]
            found_class = None
            found_version = None
            for ln in lines:
                if found_class is None:
                    m = CLASS_RE.search(ln)
                    if m:
                        found_class = m.group(1)
                if found_version is None:
                    m = FILE_RE.search(ln)
                    if m:
                        found_version = m.group(1)
            errors = []
            if found_class != classification:
                errors.append(
                    f"{p}: classification '{found_class}' does not match expected '{classification}'"
                )
            if found_version != version:
                errors.append(
                    f"{p}: version '{found_version}' does not match expected '{version}'"
                )
            return p, errors
    return None, [f"Missing file: {filename}"]


def main():
    entries = parse_metadata(METADATA_PATH)
    all_errors = []
    for filename, version, classification in entries:
        _path, errs = check_file(filename, version, classification)
        all_errors.extend(errs)
    if all_errors:
        for err in all_errors:
            print(err)
        sys.exit(1)
    print("All metadata headers match.")
    sys.exit(0)


if __name__ == "__main__":
    main()
