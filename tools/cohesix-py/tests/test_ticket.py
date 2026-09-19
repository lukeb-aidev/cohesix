# Author: Lukas Bower
# Purpose: Preserve independent v1/v2 ticket wire vectors and canonical u64 parsing limits.
# Copyright 2026 Lukas Bower
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cohesix.ticket import PayloadCursor, TicketError, decode_ticket_claims

# Same independent wire vector as the Rust ticket test. Zero MAC is deliberately
# unsigned: SDK parsing cannot establish authority or replace gateway verification.
COMPACT = "02001e01c80107006d692d74657374c0843d000000000112002f686f73742f7469636b6574732f737065630100000000"


def token(payload: str) -> str:
    return "cohesix-ticket-" + payload + "." + "00" * 32


def test_versioned_claim_vectors_preserve_legacy_and_compact_subject() -> None:
    claims = decode_ticket_claims(token(COMPACT))
    assert (claims.role, claims.subject) == ("queen", "mi-test")
    legacy = decode_ticket_claims(token("010000" + "00" * 12))
    assert (legacy.role, legacy.subject) == ("queen", None)
    for payload in (
        "020080",  # reserved flag
        COMPACT.replace("02001e01", "02001e8100", 1),  # redundant integer zero
        COMPACT + "00",  # trailing byte
        "020010000000000000",  # explicit empty scopes
        "020020" + "00" * 21,  # explicit empty quotas
        COMPACT[:-10] + "0300000000",  # unknown scope verb
    ):
        with pytest.raises(TicketError):
            decode_ticket_claims(token(payload))
    with pytest.raises(TicketError):
        decode_ticket_claims(token(COMPACT.replace("0200", "02 00", 1)))


def test_compact_u64_independent_vectors_and_overflow_boundaries() -> None:
    for raw, expected in (
        (b"\x00", 0), (b"\x7f", 127), (b"\x80\x01", 128),
        (b"\x80\x80\x01", 16384), (b"\xff" * 9 + b"\x01", 2**64 - 1),
    ):
        cursor = PayloadCursor(raw)
        assert cursor.read_integer(2) == expected
        cursor.ensure_empty()
    for raw in (
        b"\x80", b"\x80\x00", b"\x81\x00", b"\xff" * 9 + b"\x02",
        b"\x80" * 10, b"\x80" * 9 + b"\x00",
    ):
        with pytest.raises(TicketError):
            PayloadCursor(raw).read_integer(2)
