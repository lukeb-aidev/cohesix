# Author: Lukas Bower
# Purpose: Submit bounded WorkerGpu workload tickets through authenticated backend control files without host executor access.
# Copyright 2026 Lukas Bower

"""GPU workload submission acknowledgements never constitute execution proof."""

from __future__ import annotations

import json
import re
from typing import Any, Mapping

from .backends import Backend
from .errors import CohesixError
from .providers import action, registry

_FIELDS = frozenset((
    "schema", "id", "idempotency_key", "writer_epoch", "admission", "action",
    "args", "expires_unix_ms", "receipt_mode", "operation_id", "subject_ref",
    "receipt_worker_role", "receipt_worker_id", "receipt_supervisor_generation",
    "receipt_cap_generation",
))
_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", re.ASCII)
_HASH = re.compile(r"[a-f0-9]{64}", re.ASCII)


def enqueue(backend: Backend, spec: Mapping[str, Any]) -> dict[str, Any]:
    """Append a caller request; root supplies admitted Worker identity fields.

    The backend retains its delegated authentication and read scopes. Workload
    input is an immutable CAS reference on the GPU host, never a local command.
    """
    value = dict(spec)
    operation = value.get("action")
    if (value.keys() - _FIELDS or value.get("schema") != "host-ticket/v2"
            or value.get("receipt_mode") != "worker"
            or value.get("receipt_worker_role") != "worker-gpu"
            or operation not in {"gpu.workload.submit", "gpu.workload.cancel",
                                 "gpu.workload.observe"}):
        raise CohesixError("EPERM invalid or caller-enriched GPU workload ticket")
    contract = action(operation)
    for field in ("id", "idempotency_key", "operation_id", "subject_ref",
                  "receipt_worker_id"):
        identity = value.get(field)
        if not isinstance(identity, str) or not _ID.fullmatch(identity):
            raise CohesixError("EPERM invalid GPU workload identity")
    if len(value["receipt_worker_id"]) > 32:
        raise CohesixError("ELIMIT Worker identity")
    for field in ("writer_epoch", "expires_unix_ms",
                  "receipt_supervisor_generation", "receipt_cap_generation"):
        bound = value.get(field)
        if type(bound) is not int or not 0 < bound <= 2**64 - 1:
            raise CohesixError("EPERM invalid GPU workload bound")
    args = value.get("args")
    if not isinstance(args, dict):
        raise CohesixError("EPERM invalid GPU workload arguments")
    if operation == "gpu.workload.submit":
        valid = (set(args) == {"lease_id", "request_sha256"}
                 and isinstance(args["lease_id"], str)
                 and _ID.fullmatch(args["lease_id"])
                 and len(args["lease_id"]) <= 32
                 and isinstance(args["request_sha256"], str)
                 and _HASH.fullmatch(args["request_sha256"]))
    else:
        valid = (set(args) == {"job_id"} and isinstance(args["job_id"], str)
                 and _ID.fullmatch(args["job_id"]))
    if not valid:
        raise CohesixError("EPERM invalid GPU workload references")
    encoded = json.dumps(value, separators=(",", ":"), allow_nan=False).encode() + b"\n"
    if len(encoded) > contract["maximum_request_bytes"]:
        raise CohesixError("ELIMIT GPU workload ticket")
    if backend.write_append("/host/tickets/spec", encoded) != len(encoded):
        raise CohesixError("partial GPU workload ticket acknowledgement")
    return {"schema": "cohesix-control-submission/v1", "authoritative": False,
            "proof_class": "control_submission", "ticket_id": value["id"],
            "action": operation, "provider_graph_sha256": registry()["graph_sha256"],
            "execution": "unverified", "worker_proof": "unverified"}
