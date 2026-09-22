# Author: Lukas Bower
# Purpose: Prove production release rejection and renamed private-fixture/canary detection.
# Copyright 2026 Lukas Bower

from __future__ import annotations

from copy import deepcopy
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import authority_release_gate as gate


def production_manifest():
    """Independent Release A floor fixture; contains references, never private keys."""
    return {
        "system": {"name": "test"},
        "authority": {
            "production": True, "delegated_rest": True, "vm_verified_delegation": False,
            "writer_epoch": 7, "delegated_ticket_entries": 256, "delegated_ticket_max_ttl_s": 3600,
            "queen_dedupe_entries": 64, "queen_intent_max_bytes": 2048, "gpu_frame_max_bytes": 8192,
            "strict_queen_intents": True, "writer_epoch_required": True, "execution_wal_required": True,
            "legacy_queen_ctl": False, "debug_memory": False, "production_worker_ledger": False,
            "production_driver_ledger": False, "structured_quarantine": False,
            "host_ai": False, "production_failover": False,
        },
        "ecosystem": {
            "audit": {"enable": True, "replay_enable": True, "journal_max_bytes": 8192,
                      "decisions_max_bytes": 4096, "replay_max_entries": 64,
                      "replay_ctl_max_bytes": 1024, "replay_status_max_bytes": 1024},
            "host": {"federation": {"enable": False}},
        },
        "tickets": [{"role": "queen", "secret_ref": "env:DEPLOYMENT_QUEEN_KEY"}],
        "cas": {"signing": {"required": True, "verification_key_path": "resources/keys/production.hex"}},
    }


@pytest.mark.parametrize("entries", [0, 1, 64, 512, 513, True])
def test_release_intent_capacity_matches_approved_bound(entries):
    manifest = production_manifest()
    manifest["authority"]["queen_dedupe_entries"] = entries
    if type(entries) is int and entries in (1, 64, 512):
        gate.validate_policy(manifest)
    else:
        with pytest.raises(ValueError, match="queen_dedupe_entries"):
            gate.validate_policy(manifest)


def test_production_floor_and_secret_sources_fail_closed():
    manifest = production_manifest()
    gate.validate_policy(manifest)
    for field in ("production", "delegated_rest", "writer_epoch_required", "execution_wal_required", "strict_queen_intents"):
        invalid = deepcopy(manifest)
        invalid["authority"][field] = False
        with pytest.raises(ValueError):
            gate.validate_policy(invalid)
    for field in ("enable", "replay_enable"):
        invalid = deepcopy(manifest)
        invalid["ecosystem"]["audit"][field] = False
        with pytest.raises(ValueError, match="audit and replay"):
            gate.validate_policy(invalid)
    for secret in ("bootstrap", "a-unique-but-embedded-secret"):
        invalid = deepcopy(manifest)
        invalid["tickets"][0]["secret"] = secret
        with pytest.raises(ValueError, match="forbid secret literals"):
            gate.validate_policy(invalid)
    invalid = deepcopy(manifest)
    invalid["tickets"][0]["secret_ref"] = "file:/run/../key"
    with pytest.raises(ValueError, match="file secret reference"):
        gate.validate_policy(invalid)
    invalid = deepcopy(manifest)
    invalid["cas"]["signing"]["verification_key_path"] = "resources/fixtures/key.hex"
    with pytest.raises(ValueError, match="fixture"):
        gate.validate_policy(invalid)


def test_private_fixture_and_canary_are_rejected_even_when_renamed(tmp_path):
    gate.scan_bundle(tmp_path)
    destination = tmp_path / "permitted-name.bin"
    fixture = (ROOT / "resources/fixtures/cas_signing_key.hex").read_text().strip()
    for marker in (fixture.upper().encode(), bytes.fromhex(fixture), b"COHESIX_RELEASE_SECRET_CANARY_test"):
        destination.write_bytes(b"x" * 65525 + marker + b"suffix")
        with pytest.raises(ValueError, match="private fixture or secret canary"):
            gate.scan_bundle(tmp_path)
    destination.write_bytes(b"public artifact")
    gate.scan_bundle(tmp_path)


def test_development_check_cannot_be_promoted_to_release():
    manifest = production_manifest()
    manifest["authority"]["production"] = False
    gate.validate_policy(manifest, require_production=False)
    with pytest.raises(ValueError, match="provisioned production"):
        gate.validate_policy(manifest)


def test_public_key_is_bound_to_compilation_and_cannot_trust_published_fixture(tmp_path):
    retained = tmp_path / "configs/generated/cas_verification_key.hex"
    installed = tmp_path / "resources/keys/cas_verification_key.hex"
    for path in (retained, installed):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("11" * 32 + "\n")
    gate.verify_release_public_key(tmp_path)
    installed.write_text("22" * 32 + "\n")
    with pytest.raises(ValueError, match="differs"):
        gate.verify_release_public_key(tmp_path)
    fixture = "".join(line for line in (ROOT / "resources/keys/cas_verification_key.hex").read_text().splitlines() if line and not line.startswith("#")) + "\n"
    retained.write_text(fixture)
    installed.write_text(fixture)
    with pytest.raises(ValueError, match="published fixture"):
        gate.verify_release_public_key(tmp_path)
