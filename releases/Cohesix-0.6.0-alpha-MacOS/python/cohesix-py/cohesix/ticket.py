"""Ticket parsing and validation mirroring cohsh-core semantics."""

from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional

from .errors import CohesixError

TICKET_PREFIX = "cohesix-ticket-"
MAX_TICKET_LEN = 224
MAX_MOUNT_FIELD_LEN = 255
MAX_SCOPE_COUNT = 16
CLAIMS_VERSION = 1

FLAG_TICKS = 0b0000_0001
FLAG_OPS = 0b0000_0010
FLAG_TTL = 0b0000_0100
FLAG_SUBJECT = 0b0000_1000
FLAG_SCOPES = 0b0001_0000
FLAG_QUOTAS = 0b0010_0000

ROLE_MAP = {
    0: "queen",
    1: "worker-heartbeat",
    2: "worker-gpu",
    3: "worker-bus",
    4: "worker-lora",
}
ROLE_ALIAS = {
    "queen": "queen",
    "worker": "worker-heartbeat",
    "worker-heartbeat": "worker-heartbeat",
    "worker-gpu": "worker-gpu",
    "worker-bus": "worker-bus",
    "worker-lora": "worker-lora",
}


class TicketError(CohesixError):
    """Raised when a ticket fails validation."""


@dataclass
class TicketClaims:
    role: str
    subject: Optional[str]


class PayloadCursor:
    def __init__(self, data: bytes) -> None:
        self._data = data
        self._pos = 0

    def read_exact(self, size: int) -> bytes:
        end = self._pos + size
        if end > len(self._data):
            raise TicketError("ticket payload truncated")
        chunk = self._data[self._pos:end]
        self._pos = end
        return chunk

    def read_u8(self) -> int:
        return self.read_exact(1)[0]

    def read_u32(self) -> int:
        return int.from_bytes(self.read_exact(4), "little", signed=False)

    def read_u64(self) -> int:
        return int.from_bytes(self.read_exact(8), "little", signed=False)

    def read_string(self) -> str:
        length = int.from_bytes(self.read_exact(2), "little", signed=False)
        if length > MAX_MOUNT_FIELD_LEN:
            raise TicketError("ticket string field too large")
        raw = self.read_exact(length)
        try:
            return raw.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise TicketError("ticket string field not UTF-8") from exc

    def ensure_empty(self) -> None:
        if self._pos != len(self._data):
            raise TicketError("ticket payload has trailing data")


def _decode_scopes(cursor: PayloadCursor) -> None:
    count = cursor.read_u8()
    if count > MAX_SCOPE_COUNT:
        raise TicketError("ticket scope count exceeds max")
    for _ in range(count):
        _ = cursor.read_string()
        _ = cursor.read_u8()  # verb
        _ = cursor.read_u32()  # rate_per_s


def _decode_quotas(cursor: PayloadCursor) -> None:
    _ = cursor.read_u64()  # bandwidth_bytes
    _ = cursor.read_u32()  # cursor_resumes
    _ = cursor.read_u32()  # cursor_advances


def decode_ticket_claims(token: str) -> TicketClaims:
    if not token.startswith(TICKET_PREFIX):
        raise TicketError("ticket missing cohesix-ticket prefix")
    payload = token[len(TICKET_PREFIX) :]
    if "." not in payload:
        raise TicketError("ticket missing mac separator")
    payload_hex, mac_hex = payload.split(".", 1)
    try:
        payload_bytes = bytes.fromhex(payload_hex)
        mac_bytes = bytes.fromhex(mac_hex)
    except ValueError as exc:
        raise TicketError("ticket hex decode failed") from exc
    if len(mac_bytes) != 32:
        raise TicketError("ticket mac length invalid")

    cursor = PayloadCursor(payload_bytes)
    version = cursor.read_u8()
    if version != CLAIMS_VERSION:
        raise TicketError("ticket version unsupported")
    role_code = cursor.read_u8()
    role = ROLE_MAP.get(role_code)
    if role is None:
        raise TicketError("ticket role unsupported")
    flags = cursor.read_u8()
    if flags & FLAG_TICKS:
        cursor.read_u64()
    if flags & FLAG_OPS:
        cursor.read_u64()
    if flags & FLAG_TTL:
        cursor.read_u64()
    subject = None
    if flags & FLAG_SUBJECT:
        subject = cursor.read_string()
    cursor.read_u64()  # issued_at_ms
    cursor.read_string()  # mounts.service
    cursor.read_string()  # mounts.at
    if flags & FLAG_SCOPES:
        _decode_scopes(cursor)
    if flags & FLAG_QUOTAS:
        _decode_quotas(cursor)
    cursor.ensure_empty()
    return TicketClaims(role=role, subject=subject)


def normalize_role(role: str) -> str:
    canonical = ROLE_ALIAS.get(role.lower().strip())
    if canonical is None:
        raise TicketError(f"unknown role '{role}'")
    return canonical


def normalize_ticket(role: str, ticket: Optional[str], queen_validate: bool = True) -> Optional[str]:
    canonical_role = normalize_role(role)
    trimmed = ticket.strip() if ticket is not None else ""
    trimmed = trimmed if trimmed else None

    if trimmed is not None and len(trimmed) > MAX_TICKET_LEN:
        raise TicketError(f"ticket payload exceeds {MAX_TICKET_LEN} bytes")

    if canonical_role == "queen":
        if trimmed is None:
            return None
        if queen_validate:
            _ = decode_ticket_claims(trimmed)
        return trimmed

    if trimmed is None:
        raise TicketError("ticket payload is required")

    claims = decode_ticket_claims(trimmed)
    if claims.role != canonical_role:
        raise TicketError(
            f"ticket role {claims.role} does not match requested role {canonical_role}"
        )
    if not claims.subject:
        raise TicketError("ticket missing required subject identity")
    return trimmed
