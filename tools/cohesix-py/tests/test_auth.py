# Author: Lukas Bower
# Purpose: Validate deterministic and secure TCP auth token resolution for Cohesix Python tooling.
# Copyright 2026 Lukas Bower

"""Tests for `cohesix.auth`."""

from __future__ import annotations

import os
import tempfile
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.auth import resolve_tcp_auth_token  # noqa: E402


def _write_manifest(path: Path, queen_secret: str) -> None:
    path.write_text(
        "\n".join(
            [
                '[[tickets]]',
                'role = "queen"',
                f'secret = "{queen_secret}"',
                "",
                '[[tickets]]',
                'role = "worker-heart"',
                'secret = "worker"',
                "",
            ]
        ),
        encoding="utf-8",
    )


def test_resolve_tcp_auth_token_prefers_explicit_value() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        manifest = Path(tmp) / "root_task.toml"
        _write_manifest(manifest, "bootstrap")
        original = dict(os.environ)
        try:
            os.environ["COH_AUTH_TOKEN"] = "env-token"
            assert (
                resolve_tcp_auth_token("explicit-token", manifest_paths=[manifest])
                == "explicit-token"
            )
        finally:
            os.environ.clear()
            os.environ.update(original)


def test_resolve_tcp_auth_token_prefers_manifest_over_env() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        manifest = Path(tmp) / "root_task.toml"
        _write_manifest(manifest, "bootstrap")
        original = dict(os.environ)
        try:
            os.environ["COH_AUTH_TOKEN"] = "bootstrap-token"
            assert resolve_tcp_auth_token(None, manifest_paths=[manifest]) == "bootstrap"
        finally:
            os.environ.clear()
            os.environ.update(original)


def test_resolve_tcp_auth_token_uses_env_when_manifest_missing() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        missing = Path(tmp) / "missing.toml"
        original = dict(os.environ)
        try:
            os.environ["COHSH_AUTH_TOKEN"] = "env-fallback"
            assert (
                resolve_tcp_auth_token(None, manifest_paths=[missing]) == "env-fallback"
            )
        finally:
            os.environ.clear()
            os.environ.update(original)


def test_resolve_tcp_auth_token_rejects_insecure_placeholder_explicit() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        manifest = Path(tmp) / "root_task.toml"
        _write_manifest(manifest, "bootstrap")
        try:
            resolve_tcp_auth_token("changeme", manifest_paths=[manifest])
        except ValueError as exc:
            assert "insecure placeholder" in str(exc)
        else:  # pragma: no cover
            raise AssertionError("expected placeholder token rejection")


def test_resolve_tcp_auth_token_rejects_placeholder_when_only_option() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        missing = Path(tmp) / "missing.toml"
        original = dict(os.environ)
        try:
            os.environ["COH_AUTH_TOKEN"] = "changeme"
            try:
                resolve_tcp_auth_token(None, manifest_paths=[missing])
            except ValueError as exc:
                assert "insecure placeholder" in str(exc)
            else:  # pragma: no cover
                raise AssertionError("expected placeholder token rejection")
        finally:
            os.environ.clear()
            os.environ.update(original)
