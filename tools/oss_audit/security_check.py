# CLASSIFICATION: COMMUNITY
# Filename: security_check.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Query OSV for known vulnerabilities."""

from __future__ import annotations
import urllib.request
import json

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
