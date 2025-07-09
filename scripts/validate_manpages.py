# CLASSIFICATION: COMMUNITY
# Filename: validate_manpages.py v0.1
# Author: Lukas Bower
# Date Modified: 2027-12-07
#!/usr/bin/env python3  # noqa: E265
"""Validate that all staged binaries have matching man pages.

This script parses `cohesix_fetch_build.sh` to determine which binaries are
staged for the Plan9 environment. It then checks that each binary has a
corresponding man page in `workspace/docs/man` and verifies the mandoc
implementation and `cohman` wrapper exist.
"""

from __future__ import annotations

import os
import re
from pathlib import Path


def parse_binaries(script: Path) -> list[str]:
    """Return binary names staged by `cohesix_fetch_build.sh`."""
    text = script.read_text()
    bins: list[str] = []
    match = re.search(r"for bin in (.+?); do", text, re.S)
    if match:
        bins.extend(match.group(1).split())
    match_scripts = re.search(r"for script in (.+?); do", text, re.S)
    if match_scripts:
        bins.extend(match_scripts.group(1).split())
    # remove duplicates while preserving order
    seen = set()
    unique: list[str] = []
    for b in bins:
        if b not in seen:
            seen.add(b)
            unique.append(b)
    return unique


def check_man_pages(binaries: list[str], man_dir: Path) -> dict[str, bool]:
    results = {}
    for name in binaries:
        page1 = man_dir / f"{name}.1"
        page8 = man_dir / f"{name}.8"
        results[name] = page1.exists() or page8.exists()
    return results


def check_mandoc() -> dict[str, bool]:
    arch = os.uname().machine
    prebuilt = Path(f"prebuilt/mandoc/mandoc.{arch}")
    bin_script = Path("bin/mandoc")
    cohman_script = Path("bin/cohman.sh")
    cohman_bin = Path("/mnt/data/bin/cohman")
    return {
        "prebuilt": prebuilt.is_file(),
        "mandoc_script": bin_script.is_file() and os.access(bin_script, os.X_OK),
        "cohman_script": cohman_script.is_file() and os.access(cohman_script, os.X_OK),
        "cohman_bin": cohman_bin.is_file(),
    }


def main() -> None:
    binaries = parse_binaries(Path("cohesix_fetch_build.sh"))
    man_results = check_man_pages(binaries, Path("workspace/docs/man"))
    mandoc_results = check_mandoc()

    print("== Staged Binaries ==")
    for name in binaries:
        status = "OK" if man_results.get(name) else "MISSING"
        print(f"{name}: {status}")

    print("\n== Mandoc Status ==")
    for key, val in mandoc_results.items():
        print(f"{key}: {'present' if val else 'absent'}")


if __name__ == "__main__":
    main()
