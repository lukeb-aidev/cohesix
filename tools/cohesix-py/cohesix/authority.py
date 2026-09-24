# Author: Lukas Bower
# Purpose: Encode bounded authority identities without synthesizing future admission decisions.
# Copyright 2026 Lukas Bower

"""Strict Queen intent and optional admission correlation contracts."""

from __future__ import annotations

import json
import re
from dataclasses import asdict, dataclass
from typing import Any, Mapping, Optional

from .defaults import DEFAULTS
from .errors import CohesixError


def validate_provider_v1(
    action: str, target: str | None, args: Mapping[str, object]
) -> None:
    """Check generated provider fields and unambiguous operands before submission."""
    from .providers import action as provider_action

    allowed = provider_action(action).get("legacy_argument_fields")
    if allowed is None or not isinstance(args, Mapping) or set(args) - set(allowed):
        raise CohesixError("EPERM unsupported provider action or field")
    components = []
    if target is not None:
        if len(target.encode("utf-8")) > 255:
            raise CohesixError("ELIMIT provider target")
        components = target.removeprefix("/").split("/")
        for part in components:
            authority_id(part)
    path_fields = {
        "out_dir",
        "out",
        "adapter_dir",
        "from",
        "export_root",
        "export",
        "registry_root",
        "registry",
    }
    for name, value in args.items():
        if name in {
            "ttl_s",
            "mem_mb",
            "budget_ttl_s",
            "budget_ops",
            "priority",
            "streams",
        }:
            authority_u64(value, positive=name != "priority")
            maximum = 255 if name in {"priority", "streams"} else 2**32 - 1
            if value > maximum:
                raise CohesixError("EPERM provider numeric bound")
        elif name == "publish":
            if not isinstance(value, bool):
                raise CohesixError("EPERM provider boolean")
        elif name in path_fields:
            if not isinstance(value, str) or len(value.encode("utf-8")) > 1024:
                raise CohesixError("EPERM provider path")
            for part in value.removeprefix("/").split("/"):
                authority_id(part)
        else:
            authority_id(value)
    for name, alias in [
        ("job_id", "job"),
        ("model_id", "model"),
        ("out_dir", "out"),
        ("adapter_dir", "from"),
        ("export_root", "export"),
        ("registry_root", "registry"),
    ]:
        if name in args and alias in args:
            raise CohesixError("EPERM ambiguous provider alias")
    if components and components[0] == "host":
        components = components[1:]
    provider, suffix = action.split(".", 1)
    if provider in {"mac_release", "endpoint_compliance"}:
        from .providers import macos_target

        if set(args) != {"target_id"} or target != args["target_id"]:
            raise CohesixError("EPERM ambiguous macOS target")
        macos_target(args["target_id"], action)
        return
    if provider == "launchd":
        from .providers import launchd_target

        if set(args) != {"service"} or target != args["service"]:
            raise CohesixError("EPERM ambiguous launchd target")
        launchd_target(args["service"])
        return
    if provider in {"modbus", "dnp3"}:
        from .providers import field_bus_point

        if set(args) != {"endpoint", "point"} or target != args["endpoint"]:
            raise CohesixError("EPERM ambiguous field bus target")
        endpoint, point = field_bus_point(args["endpoint"], args["point"])
        expected = (
            f"{provider}_{'control' if provider == 'dnp3' else 'write'}"
            if suffix == "control"
            else f"{provider}_read"
        )
        if (
            endpoint["protocol"] != provider
            or point["operation"]["kind"] != expected
            or point["approval_required"] != (suffix == "control")
        ):
            raise CohesixError("EPERM field bus action map mismatch")
        return
    if provider == "peft":
        if components:
            raise CohesixError("EPERM PEFT target must use args")
        required = {
            "peft.export": [("job_id", "job")],
            "peft.import": [
                ("job_id", "job"),
                ("model_id", "model"),
                ("adapter_dir", "from"),
            ],
            "peft.activate": [("model_id", "model")],
            "peft.rollback": [],
        }[action]
        if any(name not in args and alias not in args for name, alias in required):
            raise CohesixError("EPERM missing provider field")
        return
    field, index = {
        "systemd": ("unit", 1),
        "docker": ("container", 1),
        "k8s": ("node", 2),
        "gpu": ("gpu_id", 1),
    }[provider]
    if components:
        if (
            components[0] != provider
            or len(components) not in {index + 1, index + 2}
            or (provider == "k8s" and components[1] != "node")
        ):
            raise CohesixError("EPERM invalid provider target")
        endpoints = (
            {"lease"} if provider == "gpu" else {suffix, suffix.replace(".", "-")}
        )
        if len(components) == index + 2 and components[-1] not in endpoints:
            raise CohesixError("EPERM provider target action mismatch")
        if field in args and args[field] != components[index]:
            raise CohesixError("EPERM ambiguous provider target")
    elif field not in args and action != "docker.status-check":
        raise CohesixError("EPERM missing provider target")


def authority_id(value: str) -> str:
    """Validate a single bounded identity shared with provider argv operands."""
    if (
        not isinstance(value, str)
        or not re.fullmatch(r"[A-Za-z0-9._-]{1,128}", value)
        or value.startswith("-")
        or ".." in value
    ):
        raise CohesixError("EPERM invalid-authority-identifier")
    return value


def authority_u64(value: int, *, positive: bool = False) -> int:
    """Reject bools, negatives and values outside the wire unsigned integer."""
    if type(value) is not int or not (int(positive) <= value <= 2**64 - 1):
        raise CohesixError("EPERM invalid-authority-integer")
    return value


@dataclass(frozen=True)
class AdmissionCorrelation:
    """Structural correlation; standing scope presence requires executor validation."""

    admission_id: str
    intent_hash: str
    policy_hash: str
    state_epoch: int
    resource_generation: int
    decision_expiry: int
    standing_scope_id: Optional[str] = None

    def __post_init__(self) -> None:
        authority_id(self.admission_id)
        for value in (self.intent_hash, self.policy_hash):
            if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
                raise CohesixError("EPERM invalid-admission-hash")
        authority_u64(self.state_epoch)
        authority_u64(self.resource_generation)
        authority_u64(self.decision_expiry, positive=True)
        if self.standing_scope_id is not None:
            authority_id(self.standing_scope_id)

    def to_payload(self) -> dict[str, object]:
        """Preserve the historical wire shape when no standing scope is selected."""
        payload = asdict(self)
        if self.standing_scope_id is None:
            payload.pop("standing_scope_id")
        return payload


@dataclass(frozen=True)
class QueenIntent:
    """Immutable retry identity for the versioned strict Queen control path."""

    id: str
    idempotency_key: str
    issued_unix_ms: int
    cmd: Mapping[str, Any]
    writer_epoch: Optional[int] = None
    admission: Optional[AdmissionCorrelation] = None

    def __post_init__(self) -> None:
        authority_id(self.id)
        authority_id(self.idempotency_key)
        authority_u64(self.issued_unix_ms, positive=True)
        if not isinstance(self.cmd, Mapping) or not self.cmd:
            raise CohesixError("EPERM Queen cmd must be a control-command object")
        if self.writer_epoch is not None:
            authority_u64(self.writer_epoch, positive=True)
        if self.admission is not None and not isinstance(
            self.admission, AdmissionCorrelation
        ):
            raise CohesixError("EPERM invalid admission correlation")

    def encode(self, policy: Optional[Mapping[str, Any]] = None) -> bytes:
        """Serialize canonical bounded bytes; callers retain these for retries."""
        policy = DEFAULTS["authority"] if policy is None else policy
        if not policy["strict_queen_intents"]:
            raise CohesixError(
                "EPERM strict Queen intents disabled by selected profile"
            )
        epoch = self.writer_epoch
        if (policy["writer_epoch_required"] or epoch is not None) and epoch != policy[
            "writer_epoch"
        ]:
            raise CohesixError("EPERM stale-writer")
        try:
            payload = {
                "schema": policy["queen_intent_schema"],
                "id": self.id,
                "idempotency_key": self.idempotency_key,
                "issued_unix_ms": self.issued_unix_ms,
                "cmd": json.dumps(
                    dict(self.cmd),
                    sort_keys=True,
                    separators=(",", ":"),
                    allow_nan=False,
                ),
            }
            if epoch is not None:
                payload["writer_epoch"] = epoch
            if self.admission is not None:
                payload["admission"] = self.admission.to_payload()
            encoded = json.dumps(
                payload, separators=(",", ":"), allow_nan=False
            ).encode("utf-8")
        except (ValueError, TypeError, RecursionError) as exc:
            raise CohesixError("EPERM invalid Queen command JSON") from exc
        if len(encoded) > min(2048, policy["queen_intent_max_bytes"]):
            raise CohesixError("ELIMIT Queen intent byte bound")
        return encoded
