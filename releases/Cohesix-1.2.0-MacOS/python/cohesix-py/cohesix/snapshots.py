# Author: Lukas Bower
# Purpose: Read source-scoped host snapshots with exact target enrollment and receiver freshness checks without inventing execution evidence.
# Copyright 2026 Lukas Bower

"""Point-in-time native observations. Retained Python objects are not live health."""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from typing import Any, Mapping

from .backends import Backend
from .errors import CohesixError

_FIELDS = (
    "schema", "provider", "source_id", "epoch", "sequence", "observed_unix_ms",
    "ttl_ms", "available", "reason", "entries",
)
_REASONS = {"not_supported", "not_enabled", "source_failed", "timed_out"}
_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")


def _id(value: Any) -> bool:
    return isinstance(value, str) and _ID.fullmatch(value) is not None and ".." not in value


def _unique(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CohesixError("invalid snapshot duplicate key")
        result[key] = value
    return result


@dataclass(frozen=True)
class HostObservation:
    """Receiver-checked observation at read time; never an execution receipt."""

    provider: str
    source_id: str
    sequence: int
    observed_unix_ms: int
    available_at_read: bool
    reason: str | None
    entries: tuple[tuple[str, str], ...]
    sha256: str


class SnapshotReader:
    """Bind reads to an explicitly selected resolved target manifest.

    The caller enrolls this manifest through its deployment/package trust path.
    No target-neutral SDK default is substituted for the selected writer epoch.
    """

    def __init__(self, backend: Backend, manifest: Mapping[str, Any]) -> None:
        self.backend = backend
        try:
            host = manifest["ecosystem"]["host"]
            config = host["snapshots"]
            if host["enable"] is not True or config["enable"] is not True:
                raise CohesixError("host snapshots not enabled")
            self.mount = host["mount_at"]
            self.epoch = manifest["authority"]["writer_epoch"]
            self.config = dict(config)
            self.publishers = {row["source_id"]: tuple(row["providers"])
                               for row in config["publishers"]}
            if len(self.publishers) != len(config["publishers"]):
                raise CohesixError("duplicate enrolled snapshot source")
            for field, cap in (("max_bytes", 8192), ("max_entries", 64),
                               ("max_value_bytes", 4096), ("max_ttl_ms", 30000)):
                value = config[field]
                if type(value) is not int or not 1 <= value <= cap:
                    raise CohesixError("invalid generated snapshot bound")
            if type(self.epoch) is not int or self.epoch <= 0:
                raise CohesixError("invalid snapshot epoch")
            if not isinstance(self.mount, str) or not self.mount.startswith("/"):
                raise CohesixError("invalid snapshot mount")
            for source, providers in self.publishers.items():
                if (not _id(source) or len(source) > 32 or not providers
                        or len(set(providers)) != len(providers)
                        or any(not _id(provider) for provider in providers)):
                    raise CohesixError("invalid snapshot enrollment")
        except (KeyError, TypeError) as error:
            raise CohesixError("invalid target snapshot manifest") from error

    def read(self, provider: str, source_id: str) -> HostObservation:
        """Read bounded bytes, then require the receiver still names that digest.

        A replacement or expiry between reads is refused, without an implicit
        retry that could conceal a failure. Call again for a new observation.
        """
        if provider not in self.publishers.get(source_id, ()):
            raise CohesixError("snapshot source/provider not enrolled")
        root = f"{self.mount}/snapshots/{provider}/{source_id}"
        raw = self.backend.read_file(f"{root}/snapshot", self.config["max_bytes"])
        if len(raw) > self.config["max_bytes"]:
            raise CohesixError("snapshot exceeds target bound")
        if not raw:
            raise CohesixError("snapshot unavailable or expired")
        try:
            value = json.loads(raw, object_pairs_hook=_unique)
            self._validate(value, provider, source_id)
            digest = hashlib.sha256(raw).hexdigest()
            status_raw = self.backend.read_file(f"{root}/status", 1024).decode("ascii")
            pairs = [item.split("=", 1) for item in status_raw.split()]
            status = _unique([(key, item) for key, item in pairs])
            expected = {"schema": "host-snapshot-status/v1", "source": source_id,
                        "provider": provider, "epoch": str(self.epoch),
                        "sequence": str(value["sequence"]), "sha256": digest,
                        "state": "available" if value["available"] else "unavailable"}
            if any(status.get(key) != item for key, item in expected.items()):
                raise CohesixError("snapshot replaced, unavailable or expired during read")
            return HostObservation(
                provider, source_id, value["sequence"], value["observed_unix_ms"],
                value["available"], value["reason"],
                tuple((entry["path"], entry["value"]) for entry in value["entries"]), digest,
            )
        except (ValueError, TypeError, KeyError, UnicodeError) as error:
            raise CohesixError("invalid native snapshot response") from error

    def _validate(self, value: Any, provider: str, source_id: str) -> None:
        if not isinstance(value, dict) or set(value) != set(_FIELDS):
            raise CohesixError("invalid snapshot fields")
        if (value["schema"] != "cohesix-host-snapshot/v1"
                or value["provider"] != provider or value["source_id"] != source_id
                or type(value["epoch"]) is not int or value["epoch"] != self.epoch):
            raise CohesixError("invalid snapshot domain")
        for field in ("sequence", "observed_unix_ms", "ttl_ms"):
            if type(value[field]) is not int or not 1 <= value[field] <= 2**64 - 1:
                raise CohesixError("invalid snapshot counter")
        if value["ttl_ms"] > self.config["max_ttl_ms"]:
            raise CohesixError("invalid snapshot ttl")
        available, reason, entries = value["available"], value["reason"], value["entries"]
        if (type(available) is not bool or (available and reason is not None)
                or (not available and (not isinstance(reason, str) or reason not in _REASONS))
                or not isinstance(entries, list) or len(entries) > self.config["max_entries"]
                or (not available and entries)):
            raise CohesixError("invalid snapshot availability")
        previous = ""
        for entry in entries:
            if not isinstance(entry, dict) or set(entry) != {"path", "value"}:
                raise CohesixError("invalid snapshot entry")
            path, text = entry["path"], entry["value"]
            if (not isinstance(path, str) or len(path.encode()) > 192 or path <= previous
                    or len(path.split("/")) > 6 or any(not _id(part) for part in path.split("/"))
                    or not isinstance(text, str) or "\0" in text
                    or len(text.encode()) > self.config["max_value_bytes"]):
                raise CohesixError("invalid snapshot entry bound")
            previous = path
