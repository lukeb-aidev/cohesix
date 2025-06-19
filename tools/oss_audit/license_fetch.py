# CLASSIFICATION: COMMUNITY
# Filename: license_fetch.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Fetch license texts for dependencies with caching."""

from __future__ import annotations
import urllib.request
import json
from pathlib import Path

CACHE_DIR = Path(".cache/licenses")
CACHE_DIR.mkdir(parents=True, exist_ok=True)

SPDX_BASE = "https://raw.githubusercontent.com/spdx/license-list-data/master/text"


def _fetch_url(url: str) -> str | None:
    try:
        with urllib.request.urlopen(url) as resp:
            return resp.read().decode("utf-8")
    except Exception:
        return None


def fetch_license_text(
    name: str, version: str, license_id: str | None, source_url: str | None = None
) -> str:
    """Return license text for given dependency, cached by name and version."""
    cache_file = CACHE_DIR / f"{name}-{version}.txt"
    if cache_file.exists():
        return cache_file.read_text()

    text = None
    if license_id:
        lic_id = license_id.split()[0].split("OR")[0].strip()
        text = _fetch_url(f"{SPDX_BASE}/{lic_id}.txt")

    if not text and source_url:
        # try GitHub API license endpoint
        if "github.com" in source_url:
            api_url = (
                source_url.rstrip("/").replace("github.com", "api.github.com/repos")
                + "/license"
            )
            data = _fetch_url(api_url)
            if data:
                try:
                    text = json.loads(data).get("content")
                    if text:
                        import base64

                        text = base64.b64decode(text).decode("utf-8")
                except Exception:
                    text = None

    if not text:
        text = "UNKNOWN"

    cache_file.write_text(text)
    return text
