# Author: Lukas Bower
# Purpose: Exchange external credentials through the enrolled gateway without accepting client-selected identity authority.
# Copyright 2026 Lukas Bower
"""Bounded host identity helpers; a mapping or issued ticket is not execution proof."""

from __future__ import annotations

import hashlib
import ipaddress
import json
import math
import os
import re
import stat
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .auth import resolve_secret_reference
from .errors import CohesixError
from .ticket import decode_ticket_claims

_MAX_CREDENTIAL_BYTES = 8192
_MAX_RESPONSE_BYTES = 4096
_DIGEST = re.compile(r"[0-9a-f]{64}\Z")
_ID = re.compile(r"[A-Za-z0-9_.][A-Za-z0-9_.-]{0,127}\Z")


@dataclass(frozen=True)
class IdentityTicket:
    """A gateway-only credential. Retention, refresh and storage are explicit."""

    mapping_id: str
    provider_graph_sha256: str
    credential_sha256: str
    expires_unix_s: int
    ticket: str = field(repr=False)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # Never forward the credential or request-auth header to another URL.
        raise CohesixError("EPERM identity redirect")


def _endpoint(base_url: str) -> str:
    if len(base_url) > 2048 or any(ord(c) <= 32 or ord(c) == 127 for c in base_url):
        raise CohesixError("EPERM identity gateway URL")
    try:
        parsed = urllib.parse.urlsplit(base_url)
        local = parsed.hostname is not None and ipaddress.ip_address(
            parsed.hostname
        ).is_loopback
    except ValueError:
        local = False
    try:
        parsed = urllib.parse.urlsplit(base_url)
        _ = parsed.port
    except ValueError as exc:
        raise CohesixError("EPERM identity gateway URL") from exc
    if (
        not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or parsed.path not in ("", "/")
        or (parsed.scheme != "https" and not (parsed.scheme == "http" and local))
    ):
        raise CohesixError("EPERM identity requires HTTPS or loopback HTTP")
    return base_url.rstrip("/") + "/v1/identity/exchange"


def _credential(reference: str) -> str:
    if reference.startswith("env:"):
        name = reference[4:]
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,127}", name):
            raise CohesixError("EPERM identity credential reference")
        value = os.environ.get(name, "")
    elif reference.startswith("file:"):
        path = Path(reference[5:])
        if not path.is_absolute() or ".." in path.parts:
            raise CohesixError("EPERM identity credential reference")
        try:
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(fd, "rb") as stream:
                if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                    raise CohesixError("EPERM identity credential file")
                value = stream.read(_MAX_CREDENTIAL_BYTES + 2).decode("ascii")
        except (OSError, UnicodeError) as exc:
            raise CohesixError("EPERM identity credential unavailable") from exc
        value = value.removesuffix("\n")
    else:
        raise CohesixError("EPERM identity expects env:NAME or file:/absolute/path")
    if not (1 <= len(value) <= _MAX_CREDENTIAL_BYTES) or not re.fullmatch(
        r"[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+", value
    ):
        raise CohesixError("EPERM identity credential shape")
    return value


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value: dict[str, Any] = {}
    for key, item in pairs:
        if key in value:
            raise CohesixError("EPERM identity response duplicate field")
        value[key] = item
    return value


def exchange_identity(
    base_url: str,
    mapping_id: str,
    credential_ref: str,
    request_auth_ref: str,
    expected_provider_graph_sha256: str,
    *,
    timeout_s: float = 5.0,
) -> IdentityTicket:
    """Perform one authenticated exchange without retries, redirects or token logs.

    The operator supplies an independently enrolled provider graph digest. The
    gateway verifies JWT signatures and current generated policy; this helper
    validates the bounded response and never verifies a ticket MAC locally.
    """
    endpoint = _endpoint(base_url)
    if (
        not _ID.fullmatch(mapping_id)
        or ".." in mapping_id
        or not _DIGEST.fullmatch(expected_provider_graph_sha256)
        or not math.isfinite(timeout_s)
        or not 0 < timeout_s <= 30
    ):
        raise CohesixError("EPERM identity request bounds")
    credential = _credential(credential_ref)
    try:
        request_auth = resolve_secret_reference(request_auth_ref)
    except ValueError as exc:
        raise CohesixError("EPERM identity request authentication source") from exc
    request = urllib.request.Request(
        endpoint,
        data=json.dumps({"mapping_id": mapping_id, "credential": credential}).encode(),
        method="POST",
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            "x-cohesix-auth": request_auth,
        },
    )
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect)
    try:
        with opener.open(request, timeout=timeout_s) as response:
            raw = response.read(_MAX_RESPONSE_BYTES + 1)
    except (urllib.error.HTTPError, urllib.error.URLError, OSError):
        # Exception representations can contain URLs or response data. Expose
        # a deterministic refusal and leave credential contents out of errors.
        raise CohesixError("EPERM identity exchange refused or unavailable") from None
    if len(raw) > _MAX_RESPONSE_BYTES:
        raise CohesixError("ELIMIT identity response")
    try:
        value = json.loads(raw, object_pairs_hook=_unique_object)
    except (ValueError, UnicodeError) as exc:
        raise CohesixError("EPERM identity response JSON") from exc
    expected_keys = {
        "schema", "status", "authoritative", "identity_class", "mapping_id",
        "provider_graph_sha256", "credential_sha256", "expires_unix_s", "ticket",
    }
    digest = hashlib.sha256(credential.encode()).hexdigest()
    if (
        not isinstance(value, dict)
        or value.keys() != expected_keys
        or value["schema"] != "cohesix-identity-exchange/v1"
        or value["status"] != "OK"
        or value["authoritative"] is not False
        or value["identity_class"] != "gateway_enforced"
        or value["mapping_id"] != mapping_id
        or value["provider_graph_sha256"] != expected_provider_graph_sha256
        or value["credential_sha256"] != digest
        or type(value["expires_unix_s"]) is not int
        or not 0 < value["expires_unix_s"] <= (2**64 - 1) // 1000
        or not isinstance(value["ticket"], str)
    ):
        raise CohesixError("EPERM identity response binding")
    claims = decode_ticket_claims(value["ticket"])
    if not claims.subject or not claims.subject.startswith("mi-"):
        raise CohesixError("EPERM identity ticket class")
    return IdentityTicket(
        mapping_id, expected_provider_graph_sha256, digest,
        value["expires_unix_s"], value["ticket"],
    )


def local_subject() -> str:
    """Read this process's kernel euid; this is not a remote identity assertion.

    An enrolled operator uses ``coh identity --local`` to apply generated policy
    and optionally issue with a separately enrolled gateway key. Sending this
    uid to the HTTP exchange endpoint never authenticates a local caller.
    """
    return str(os.geteuid())
