# Author: Lukas Bower
# Purpose: Resolve Cohesix TCP auth tokens from explicit input, manifest files, and environment.
# Copyright 2026 Lukas Bower

"""Authentication token helpers for Cohesix transports."""

from __future__ import annotations

import os
import re
import json
import stat
from pathlib import Path
from typing import Iterable, Optional, Sequence

from .defaults import DEFAULTS

_TOKEN_ENV_KEYS = ("COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN")
_MANIFEST_ENV_KEYS = ("COH_RTC_MANIFEST", "COH_MANIFEST", "COHESIX_MANIFEST")
_MANIFEST_DEFAULTS = ("configs/root_task.toml", "configs/root_task_regression.toml")
_MAX_SECRET_BYTES = 4096
_SECTION_TICKETS = re.compile(r"^\s*\[\[tickets\]\]\s*$")
_SECTION_ANY = re.compile(r"^\s*\[[^\]]+\]\s*$")
_TICKET_KV = re.compile(r'^\s*(role|secret|secret_ref)\s*=\s*"([^"]*)"\s*$')


def validate_live_secret(value: str) -> str:
    """Reject generated placeholders and protocol delimiters before connecting."""
    if len(value.encode("utf-8")) > _MAX_SECRET_BYTES:
        raise ValueError("secret exceeds byte bound")
    value = value.strip()
    if value.lower() in DEFAULTS["placeholder_credentials"]:
        raise ValueError("auth token uses insecure placeholder")
    if not value or not value.isascii() or any(char.isspace() or ord(char) < 32 or ord(char) == 127 for char in value):
        raise ValueError("secret is empty or malformed")
    return value


def resolve_secret_reference(reference: str) -> str:
    """Read exactly env:NAME or file:/absolute/path, with no failure fallback."""
    if reference.startswith("env:"):
        name = reference[4:]
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,127}", name):
            raise ValueError("invalid secret environment reference")
        value = os.environ.get(name)
        if value is None:
            raise ValueError("selected secret source is unavailable")
    elif reference.startswith("file:"):
        name = reference[5:]
        path = Path(name)
        if len(name) > 1024 or not path.is_absolute() or ".." in path.parts or any(ord(c) < 32 or ord(c) == 127 for c in name):
            raise ValueError("invalid secret file reference")
        try:
            if not path.is_file():
                raise ValueError("selected secret source is unavailable or not a regular file")
            with path.open("rb") as handle:
                if not path.is_file():
                    raise ValueError("secret source must be a regular file")
                value = handle.read(_MAX_SECRET_BYTES + 1).decode("utf-8")
        except (OSError, UnicodeError) as exc:
            raise ValueError("selected secret source is unavailable") from exc
    else:
        raise ValueError("expected env:NAME or file:/absolute/path")
    return validate_live_secret(value)


def resolve_secret(value: str) -> str:
    """Resolve an explicit reference or validate a compatibility literal."""
    if value.startswith(("env:", "file:")):
        return resolve_secret_reference(value)
    return validate_live_secret(value)


def resolve_manifest_auth_token(path: str | Path, format_name: str = "toml") -> str:
    """Resolve exactly one selected manifest Queen credential, with no fallback."""
    if not stat.S_ISREG(Path(path).lstat().st_mode):
        raise ValueError("manifest Queen ticket input must be a regular non-symlink file")
    with Path(path).open("rb") as stream:
        raw = stream.read(4 * 1024 * 1024 + 1)
    if len(raw) > 4 * 1024 * 1024:
        raise ValueError("manifest exceeds byte bound")
    if format_name == "json":
        document = json.loads(raw)
    elif format_name == "toml":
        try:
            import tomllib
        except ModuleNotFoundError:
            import tomli as tomllib
        document = tomllib.loads(raw.decode("utf-8"))
    else:
        raise ValueError("unsupported manifest format")
    tickets = document.get("tickets")
    if not isinstance(tickets, list):
        raise ValueError("manifest tickets must be a list")
    matches = [ticket for ticket in tickets if isinstance(ticket, dict) and ticket.get("role") == "queen"]
    if len(matches) != 1:
        raise ValueError("manifest must declare exactly one Queen credential")
    ticket = matches[0]
    reference, literal = ticket.get("secret_ref"), ticket.get("secret")
    if reference is not None:
        if literal:
            raise ValueError("ambiguous ticket secret sources")
        if not isinstance(reference, str):
            raise ValueError("invalid secret reference")
        return resolve_secret_reference(reference)
    if not isinstance(literal, str):
        raise ValueError("missing Queen credential")
    return validate_live_secret(literal)


def _normalize_token(token: Optional[str]) -> Optional[str]:
    if token is None:
        return None
    trimmed = token.strip()
    if not trimmed:
        return None
    return trimmed


def _candidate_manifest_paths(
    manifest_paths: Optional[Sequence[str | Path]] = None,
) -> Iterable[Path]:
    seen: set[Path] = set()
    candidates: list[Path] = []
    for env_name in _MANIFEST_ENV_KEYS:
        value = os.environ.get(env_name)
        if value and value.strip():
            candidates.append(Path(value.strip()).expanduser())
    if manifest_paths:
        for path in manifest_paths:
            candidates.append(Path(path).expanduser())
    if manifest_paths is None:
        cwd = Path.cwd()
        for rel in _MANIFEST_DEFAULTS:
            candidates.append(cwd / rel)
        repo_root = Path(__file__).resolve().parents[3]
        for rel in _MANIFEST_DEFAULTS:
            candidates.append(repo_root / rel)
    for candidate in candidates:
        resolved = candidate.resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        yield resolved


def _parse_queen_secret_with_tomllib(text: str) -> Optional[str]:
    try:
        import tomllib  # type: ignore[attr-defined]
    except ModuleNotFoundError:
        try:
            import tomli as tomllib  # type: ignore[import-not-found]
        except ModuleNotFoundError:
            return None
    try:
        data = tomllib.loads(text)
    except Exception:
        return None
    tickets = data.get("tickets")
    if not isinstance(tickets, list):
        return None
    for ticket in tickets:
        if not isinstance(ticket, dict):
            continue
        role = str(ticket.get("role", "")).strip().lower()
        if role != "queen":
            continue
        if ticket.get("secret_ref"):
            if ticket.get("secret"):
                raise ValueError("ambiguous ticket secret sources")
            return resolve_secret_reference(str(ticket["secret_ref"]))
        secret = _normalize_token(str(ticket.get("secret", "")))
        if secret:
            return secret
    return None


def _parse_queen_secret_fallback(text: str) -> Optional[str]:
    in_ticket = False
    role: Optional[str] = None
    secret: Optional[str] = None
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if _SECTION_TICKETS.match(line):
            if role == "queen" and secret:
                return secret
            in_ticket = True
            role = None
            secret = None
            continue
        if _SECTION_ANY.match(line):
            if in_ticket and role == "queen" and secret:
                return secret
            in_ticket = False
            continue
        if not in_ticket:
            continue
        match = _TICKET_KV.match(line)
        if match is None:
            continue
        key, value = match.groups()
        if key == "role":
            role = value.strip().lower()
        elif key in {"secret", "secret_ref"}:
            if secret is not None:
                raise ValueError("ambiguous ticket secret sources")
            secret = resolve_secret_reference(value) if key == "secret_ref" else _normalize_token(value)
    if role == "queen" and secret:
        return secret
    return None


def _manifest_queen_secret(path: Path) -> Optional[str]:
    if not path.is_file():
        return None
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return None
    parsed = _parse_queen_secret_with_tomllib(text)
    if parsed:
        return parsed
    return _parse_queen_secret_fallback(text)


def resolve_tcp_auth_token(
    value: Optional[str] = None,
    *,
    manifest_paths: Optional[Sequence[str | Path]] = None,
) -> str:
    """Resolve a TCP auth token without relying on insecure placeholder defaults."""

    if value is not None:
        return resolve_secret(value)
    for env_name in ("COH_AUTH_TOKEN_REF", *_TOKEN_ENV_KEYS):
        token = os.environ.get(env_name)
        if token is not None:
            return resolve_secret_reference(token) if env_name.endswith("_REF") else resolve_secret(token)
    for manifest_path in _candidate_manifest_paths(manifest_paths):
        token = _manifest_queen_secret(manifest_path)
        if token is not None:
            return resolve_secret(token)
    raise ValueError("TCP auth token is required; configure an explicit env/file reference or token")
