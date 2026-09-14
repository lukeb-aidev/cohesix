#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Reject development authority profiles and fixture private material at release assembly.
# Copyright 2026 Lukas Bower

"""Inspect selected authority policy and streamed release files without exposing secrets."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
MAX_MANIFEST_BYTES = 4 * 1024 * 1024


def validate_policy(manifest: dict[str, Any], *, require_production: bool = True) -> None:
    """Enforce the compiler's Release A floor on retained release configuration."""
    policy = manifest.get("authority", {})
    if require_production and policy.get("production") is not True:
        raise ValueError("release requires a provisioned production authority profile")
    if policy.get("delegated_rest") is not True or policy.get("vm_verified_delegation") is not False:
        raise ValueError("delegation claim class must be gateway_enforced")
    for name, low, high in (
        ("writer_epoch", 1, 2**64 - 1), ("delegated_ticket_entries", 1, 4096),
        ("delegated_ticket_max_ttl_s", 1, 86400), ("queen_dedupe_entries", 1, 256),
        ("queen_intent_max_bytes", 256, 2048), ("gpu_frame_max_bytes", 256, 8192),
    ):
        value = policy.get(name)
        if type(value) is not int or not low <= value <= high:
            raise ValueError(f"invalid generated authority bound: {name}")
    for name in ("production_worker_ledger", "production_driver_ledger", "structured_quarantine", "host_ai", "production_failover"):
        if policy.get(name) is not False:
            raise ValueError(f"unsupported production authority claim: {name}")
    if policy.get("production") is not True:
        return
    if any(policy.get(name) is not True for name in (
        "strict_queen_intents", "writer_epoch_required", "execution_wal_required",
    )) or any(policy.get(name) is not False for name in ("legacy_queen_ctl", "debug_memory")):
        raise ValueError("production requires strict intents, fencing, WAL, and disabled legacy/debug paths")
    audit = manifest.get("ecosystem", {}).get("audit", {})
    if audit.get("enable") is not True or audit.get("replay_enable") is not True:
        raise ValueError("production audit and replay must both be enabled")
    for name, maximum in (("journal_max_bytes", 1048576), ("decisions_max_bytes", 1048576),
                          ("replay_max_entries", 4096), ("replay_ctl_max_bytes", 8192),
                          ("replay_status_max_bytes", 1048576)):
        value = audit.get(name)
        if type(value) is not int or not 1 <= value <= maximum:
            raise ValueError(f"production audit retention must be bounded: {name}")
    if manifest.get("ecosystem", {}).get("host", {}).get("federation", {}).get("enable") is not False:
        raise ValueError("Release A production federation requires separate promotion acceptance")
    tickets = manifest.get("tickets")
    if not isinstance(tickets, list) or not tickets:
        raise ValueError("production ticket sources are missing")
    for ticket in tickets:
        reference = ticket.get("secret_ref")
        if ticket.get("secret") not in (None, "") or not isinstance(reference, str):
            raise ValueError("production tickets require references and forbid secret literals")
        if reference.startswith("env:"):
            if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,127}", reference[4:]):
                raise ValueError("invalid environment secret reference")
        elif reference.startswith("file:"):
            path = Path(reference[5:])
            if (not path.is_absolute() or ".." in path.parts or len(reference) > 1029
                    or any(ord(char) < 32 for char in reference)):
                raise ValueError("invalid file secret reference")
        else:
            raise ValueError("secret reference must select exactly env: or file:")
    signing = manifest.get("cas", {}).get("signing", {})
    if signing.get("required") is not True or not signing.get("verification_key_path"):
        raise ValueError("production requires public CAS verification material")
    if "fixtures" in Path(signing["verification_key_path"]).parts:
        raise ValueError("production cannot trust fixture CAS signing material")


def read_manifest(path: Path) -> dict[str, Any]:
    """Read a bounded retained resolved manifest."""
    with path.open("rb") as stream:
        payload = stream.read(MAX_MANIFEST_BYTES + 1)
    if len(payload) > MAX_MANIFEST_BYTES:
        raise ValueError("resolved manifest exceeds byte bound")
    result = json.loads(payload)
    if not isinstance(result, dict):
        raise ValueError("resolved manifest must be an object")
    return result


def verify_release_public_key(bundle: Path) -> None:
    """Bind the installed public key to the exact compiler-retained public bytes."""
    installed = bundle / "resources/keys/cas_verification_key.hex"
    retained = bundle / "configs/generated/cas_verification_key.hex"

    def decode(path: Path) -> bytes:
        with path.open("rb") as source:
            value = source.read(66)
        if not re.fullmatch(rb"[0-9a-f]{64}\n?", value):
            raise ValueError("invalid retained release public verification key")
        return bytes.fromhex(value.decode().strip())

    public = decode(installed)
    if public != decode(retained):
        raise ValueError("release public key differs from compiler-retained key")
    fixture_lines = (ROOT / "resources/keys/cas_verification_key.hex").read_text().splitlines()
    fixture = bytes.fromhex("".join(line.strip() for line in fixture_lines if line.strip() and not line.lstrip().startswith("#")))
    if public == fixture:
        raise ValueError("release cannot trust the published fixture verification key")


def scan_bundle(bundle: Path) -> None:
    """Refuse fixture private bytes under any filename and canary-bearing files.

    The existing release inventory independently rejects extra or wildcard
    payloads. This content check also catches a private fixture renamed into
    an otherwise permitted destination. Files are streamed in bounded chunks.
    """
    fixture = (ROOT / "resources/fixtures/cas_signing_key.hex").read_text().strip()
    forbidden = (fixture.lower().encode(), bytes.fromhex(fixture), b"COHESIX_RELEASE_SECRET_CANARY_")
    overlap = max(map(len, forbidden)) - 1
    for path in bundle.rglob("*"):
        if path.is_symlink():
            raise ValueError("release payload contains a symlink")
        if not path.is_file():
            continue
        tail = b""
        with path.open("rb") as stream:
            while chunk := stream.read(65536):
                data = tail + chunk
                if any(marker in data.lower() if index == 0 else marker in data
                       for index, marker in enumerate(forbidden)):
                    raise ValueError(f"private fixture or secret canary in release file: {path.relative_to(bundle)}")
                tail = data[-overlap:]


def main() -> int:
    """Validate production releases or explicitly labelled development checks."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--allow-development", action="store_true")
    args = parser.parse_args()
    try:
        validate_policy(read_manifest(args.manifest), require_production=not args.allow_development)
        if args.bundle is not None:
            verify_release_public_key(args.bundle)
            scan_bundle(args.bundle)
    except (OSError, ValueError, TypeError, KeyError) as error:
        print(f"authority-release-gate: FAIL {error}")
        return 1
    print("authority-release-gate: PASS" + (" (development policy; no release claim)" if args.allow_development else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
