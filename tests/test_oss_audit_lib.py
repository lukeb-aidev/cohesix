#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: test_oss_audit_lib.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-22
"""Unit tests for OSS audit helpers."""

import json
from pathlib import Path
import types
import subprocess

import pytest

tomli = pytest.importorskip('tomli', reason='tomli not installed')
from tools.oss_audit import security_check, license_fetch, scan


def test_parse_spdx_header(tmp_path):
    f = tmp_path / "file.rs"
    f.write_text("// SPDX-License-Identifier: MIT\n")
    assert security_check.parse_spdx_header(f) == "MIT"


def test_load_allowed(tmp_path):
    policy = tmp_path / "POLICY.md"
    policy.write_text("MIT\nApache")
    allowed = security_check.load_allowed_licenses(policy)
    assert "MIT" in allowed and "Apache-2.0" in allowed


def test_validate_licenses(tmp_path):
    policy = tmp_path / "POLICY.md"
    policy.write_text("MIT")
    src = tmp_path / "src"
    src.mkdir()
    (src / "lib.rs").write_text("// SPDX-License-Identifier: MIT\n")
    errs = security_check.validate_licenses([str(src)], str(policy))
    assert errs == []


def test_query_osv(monkeypatch):
    def fake_urlopen(req):
        class R:
            def __enter__(self): return self
            def __exit__(self, *a): pass
            def read(self): return b'{"vulns": []}'
        return R()
    monkeypatch.setattr(security_check.urllib.request, "urlopen", fake_urlopen)
    vulns = security_check.query_osv("crates.io", "foo", "0.1")
    assert vulns == []


def test_fetch_license_text(monkeypatch, tmp_path):
    monkeypatch.setattr(license_fetch, "_fetch_url", lambda u: "MIT text")
    text = license_fetch.fetch_license_text("foo", "0.1", "MIT")
    assert "MIT" in text


def test_parse_cargo_toml(tmp_path):
    toml = tmp_path / "Cargo.toml"
    toml.write_text("[dependencies]\nfoo = \"0.1\"")
    deps = scan.parse_cargo_toml(toml)
    assert deps == [("foo", "0.1", "https://crates.io/crates/foo")]


def test_scan_paths(tmp_path):
    d = tmp_path / "pkg"
    d.mkdir()
    (d / "Cargo.toml").write_text("[dependencies]\nfoo=\"0.1\"")
    res = scan.scan_paths([str(tmp_path)])
    assert res[0]["name"] == "foo"


def test_generate_outputs(monkeypatch, tmp_path):
    deps = [{"name": "foo", "version": "0.1", "source": "url"}]
    monkeypatch.setattr(scan, "fetch_license_text", lambda *a, **k: "T")
    monkeypatch.setattr(scan, "query_osv", lambda *a, **k: [])
    scan.generate_outputs(deps, tmp_path)
    assert (tmp_path / "OPEN_SOURCE_DEPENDENCIES.md").exists()


def test_run_audit(monkeypatch, tmp_path):
    monkeypatch.setattr(scan, "scan_paths", lambda p: [{"name": "foo", "version": "0.1", "source": "u"}])
    monkeypatch.setattr(scan, "generate_outputs", lambda deps, out: out.mkdir(exist_ok=True))
    scan.run_audit(["."], str(tmp_path))
    assert tmp_path.exists()

