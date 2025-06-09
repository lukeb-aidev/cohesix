# CLASSIFICATION: COMMUNITY
# Filename: security_check.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-21
"""Query OSV for known vulnerabilities."""

from __future__ import annotations
import urllib.request
import json
import re
from pathlib import Path

OSV_URL = 'https://api.osv.dev/v1/query'


def query_osv(ecosystem: str, name: str, version: str) -> list[dict]:
    """Return list of vulnerabilities for the package."""
    body = json.dumps({'package': {'name': name, 'ecosystem': ecosystem}, 'version': version}).encode('utf-8')
    req = urllib.request.Request(OSV_URL, data=body, headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.load(resp)
    except Exception:
        return []
    return data.get('vulns', []) or []


SPDX_RE = re.compile(r"SPDX-License-Identifier:\s*(.+)")


def parse_spdx_header(path: Path) -> str | None:
    """Return SPDX identifier from the first few lines of a file."""
    try:
        with path.open("r", errors="ignore") as f:
            for _ in range(5):
                line = f.readline()
                if not line:
                    break
                m = SPDX_RE.search(line)
                if m:
                    return m.group(1).strip()
    except Exception:
        return None
    return None


def load_allowed_licenses(policy: Path) -> set[str]:
    text = policy.read_text()
    allowed = set()
    if "MIT" in text:
        allowed.add("MIT")
    if "Apache" in text:
        allowed.add("Apache-2.0")
    if "BSD" in text:
        allowed.add("BSD-2-Clause")
        allowed.add("BSD-3-Clause")
    return allowed


def validate_licenses(paths: list[str], policy_file: str) -> list[str]:
    allowed = load_allowed_licenses(Path(policy_file))
    errors = []
    for root in paths:
        for p in Path(root).rglob("*.*"):
            if p.suffix in {".rs", ".py", ".c", ".h", ".cpp", ".go"}:
                lic = parse_spdx_header(p)
                if not lic:
                    errors.append(f"{p}: missing SPDX")
                elif lic not in allowed:
                    errors.append(f"{p}: license {lic} not allowed")
    return errors


def main():
    import argparse

    parser = argparse.ArgumentParser(description="SBOM security check")
    parser.add_argument("paths", nargs="*", default=["."])
    parser.add_argument("--policy", default="docs/community/governance/OSS_REUSE.md")
    args = parser.parse_args()

    errs = validate_licenses(args.paths, args.policy)
    if errs:
        for e in errs:
            print(e)
        return 1
    print("SBOM validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
