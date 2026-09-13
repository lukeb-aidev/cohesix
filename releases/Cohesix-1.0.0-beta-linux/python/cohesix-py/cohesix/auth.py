# Author: Lukas Bower
# Purpose: Resolve Cohesix TCP auth tokens from explicit input, manifest files, and environment.
# Copyright 2026 Lukas Bower

"""Authentication token helpers for Cohesix transports."""

from __future__ import annotations

import os
import re
from pathlib import Path
from typing import Iterable, Optional, Sequence

_TOKEN_ENV_KEYS = ("COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN")
_MANIFEST_ENV_KEYS = ("COH_RTC_MANIFEST", "COH_MANIFEST", "COHESIX_MANIFEST")
_MANIFEST_DEFAULTS = ("configs/root_task.toml", "configs/root_task_regression.toml")
_INSECURE_PLACEHOLDER_TOKEN = "changeme"
_SECTION_TICKETS = re.compile(r"^\s*\[\[tickets\]\]\s*$")
_SECTION_ANY = re.compile(r"^\s*\[[^\]]+\]\s*$")
_TICKET_KV = re.compile(r'^\s*(role|secret)\s*=\s*"([^"]*)"\s*$')


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
        elif key == "secret":
            secret = _normalize_token(value)
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

    explicit = _normalize_token(value)
    if explicit:
        if explicit == _INSECURE_PLACEHOLDER_TOKEN:
            raise ValueError("TCP auth token uses insecure placeholder token 'changeme'")
        return explicit
    saw_placeholder = False
    for manifest_path in _candidate_manifest_paths(manifest_paths):
        manifest_token = _manifest_queen_secret(manifest_path)
        if manifest_token:
            if manifest_token == _INSECURE_PLACEHOLDER_TOKEN:
                saw_placeholder = True
                continue
            return manifest_token
    for env_name in _TOKEN_ENV_KEYS:
        token = _normalize_token(os.environ.get(env_name))
        if token:
            if token == _INSECURE_PLACEHOLDER_TOKEN:
                saw_placeholder = True
                continue
            return token
    if saw_placeholder:
        raise ValueError("TCP auth token uses insecure placeholder token 'changeme'")
    raise ValueError(
        "TCP auth token is required; set --auth-token or COH_AUTH_TOKEN/COHSH_AUTH_TOKEN, "
        "or provide a manifest with a queen ticket secret"
    )
