# CLASSIFICATION: COMMUNITY
# Filename: validate_metadata_sync.py v0.1
# Date Modified: 2025-06-09
# Author: Lukas Bower
"""Metadata synchronization validator for Cohesix documentation."""

from __future__ import annotations
import re
import sys
from pathlib import Path

# Align with repository layout
METADATA_PATH = Path("docs/community/governance/METADATA.md")

# Regex patterns for headers
CLASS_RE = re.compile(r"CLASSIFICATION:\s*(\w+)")
FILE_RE = re.compile(r"Filename:\s*\S+\s+(v[0-9.]+)")


def parse_metadata(path: Path):
    entries = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line.startswith("|") or line.startswith("|-"):
                continue
            cols = [c.strip() for c in line.strip("|").split("|")]
            if len(cols) < 4:
                continue
            if cols[0] == "Filename" and cols[1] == "Version":
                continue
            filename, version, _date, classification = cols[:4]
            entries.append((filename, version, classification))
    return entries


def check_file(filename: str, version: str, classification: str):
    search_roots = [
        Path("."),
        Path("docs/community"),
        Path("docs/private"),
        Path("docs/man"),
        Path("docs/devices"),
        Path("scripts"),
        Path("resources"),
        Path("tests"),
    ]
    candidates = [
        p for root in search_roots for p in root.rglob(filename) if p.is_file()
    ]
    if not candidates:
        return None, [f"Missing file: {filename}"]

    for p in candidates:
        with p.open() as f:
            lines = [next(f, "") for _ in range(5)]
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
        if found_class == classification and found_version == version:
            return p, []

    # Report mismatches for first candidate if none matched exactly
    p = candidates[0]
    errors = []
    with p.open() as f:
        lines = [next(f, "") for _ in range(5)]
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
    if found_class != classification:
        errors.append(
            f"{p}: classification '{found_class}' does not match expected '{classification}'"
        )
    if found_version != version:
        errors.append(
            f"{p}: version '{found_version}' does not match expected '{version}'"
        )
    return p, errors


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
