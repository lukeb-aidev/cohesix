# Author: Lukas Bower
# Purpose: Check receiver freshness, source isolation and withdrawal at the Python snapshot boundary.
# Copyright 2026 Lukas Bower

import copy
import hashlib
import json

import pytest

from cohesix import SnapshotReader
from cohesix.backends import Backend
from cohesix.errors import CohesixError

MANIFEST = {"ecosystem": {"host": {"enable": True, "mount_at": "/host", "snapshots": {
    "enable": True, "max_bytes": 8192, "max_entries": 64, "max_value_bytes": 1024,
    "max_ttl_ms": 30000, "publishers": [{"source_id": "native-host", "providers": ["network"]}],
}}}, "authority": {"writer_epoch": 7}}
OBSERVATION = {
    "schema": "cohesix-host-snapshot/v1", "provider": "network", "source_id": "native-host",
    "epoch": 7, "sequence": 12, "observed_unix_ms": 1234567, "ttl_ms": 4000,
    "available": True, "reason": None, "entries": [{"path": "links/0", "value": '{"up":true}'}],
}


class Receiver(Backend):
    def __init__(self, value: dict, *, expired: bool = False) -> None:
        self.raw = json.dumps(value, separators=(",", ":")).encode()
        state = "available" if value["available"] else "unavailable"
        self.status = (
            f"schema=host-snapshot-status/v1 source=native-host provider=network epoch=7 "
            f"sequence=12 sha256={hashlib.sha256(self.raw).hexdigest()} state={state}"
        ).encode() if not expired else b"state=unavailable reason=expired sequence=12"

    def read_file(self, path: str, max_bytes: int) -> bytes:
        assert path.startswith("/host/snapshots/network/native-host/")
        result = self.raw if path.endswith("/snapshot") else self.status
        assert len(result) <= max_bytes
        return result


def test_receiver_digest_and_expiry_bound_point_in_time_observation() -> None:
    result = SnapshotReader(Receiver(OBSERVATION), MANIFEST).read("network", "native-host")
    assert result.available_at_read is True
    assert result.entries == (("links/0", '{"up":true}'),)
    assert result.sequence == 12
    with pytest.raises(CohesixError, match="expired"):
        SnapshotReader(Receiver(OBSERVATION, expired=True), MANIFEST).read("network", "native-host")
    backend = Receiver(OBSERVATION)
    backend.status = backend.status.replace(b"sequence=12", b"sequence=13")
    with pytest.raises(CohesixError, match="replaced"):
        SnapshotReader(backend, MANIFEST).read("network", "native-host")


def test_withdrawal_and_source_counter_entry_contracts() -> None:
    withdrawn = dict(OBSERVATION, available=False, reason="source_failed", entries=[])
    result = SnapshotReader(Receiver(withdrawn), MANIFEST).read("network", "native-host")
    assert (result.available_at_read, result.reason, result.entries) == (False, "source_failed", ())
    invalid = [dict(OBSERVATION, source_id="foreign-host"), dict(OBSERVATION, epoch=8),
               dict(OBSERVATION, sequence=True), dict(OBSERVATION, ttl_ms=30001),
               dict(OBSERVATION, available=False, reason="source_failed"),
               dict(OBSERVATION, entries=[{"path": "../escape", "value": "x"}])]
    for value in invalid:
        with pytest.raises(CohesixError):
            SnapshotReader(Receiver(value), MANIFEST).read("network", "native-host")
    manifest = copy.deepcopy(MANIFEST)
    manifest["ecosystem"]["host"]["snapshots"]["max_bytes"] = 8193
    with pytest.raises(CohesixError):
        SnapshotReader(Receiver(OBSERVATION), manifest)
