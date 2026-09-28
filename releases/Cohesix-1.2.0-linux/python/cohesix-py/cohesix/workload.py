# Author: Lukas Bower
# Purpose: Submit bounded WorkerGpu tickets and independently verify registered batch-edge output.
# Copyright 2026 Lukas Bower

"""GPU workload submission acknowledgements never constitute execution proof."""

from __future__ import annotations

import json
import hashlib
import os
import re
import stat
from pathlib import Path
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


def batch_edges_expected(data: bytes, width: int, height: int, frames: int) -> bytes:
    """Compute the public integer Sobel contract without a CUDA runtime.

    The byte-level calculation is independent of the installed GPU package.
    It is intentionally finite so an operator can freeze an expected digest
    before admission and verify every returned pixel after native execution.
    """
    if (type(width) is not int or type(height) is not int
            or type(frames) is not int or not 3 <= width <= 256
            or not 3 <= height <= 256 or not 1 <= frames <= 4
            or width * height * frames > 262_144
            or len(data) != width * height * frames):
        raise CohesixError("ELIMIT batch-edge dimensions or input bytes")
    result = bytearray(len(data))
    plane = width * height
    for frame in range(frames):
        base = frame * plane
        for y in range(1, height - 1):
            row = base + y * width
            for x in range(1, width - 1):
                index = row + x
                tl = data[index - width - 1]
                tc = data[index - width]
                tr = data[index - width + 1]
                ml = data[index - 1]
                mr = data[index + 1]
                bl = data[index + width - 1]
                bc = data[index + width]
                br = data[index + width + 1]
                gx = -tl + tr - 2 * ml + 2 * mr - bl + br
                gy = -tl - 2 * tc - tr + bl + 2 * bc + br
                result[index] = min(255, abs(gx) + abs(gy))
    return bytes(result)


def verify_batch_edges(input_path: Path, output_path: Path, width: int,
                       height: int, frames: int) -> dict[str, Any]:
    """Check all retained output bytes against the frozen task-specific model."""
    data = read_batch_artifact(input_path)
    expected = batch_edges_expected(data, width, height, frames)
    observed = read_batch_artifact(output_path)
    if len(observed) != len(expected):
        raise CohesixError("output_mismatch batch-edge length")
    maximum_error = max((abs(left - right) for left, right in
                         zip(expected, observed)), default=0)
    if maximum_error != 0:
        raise CohesixError("output_mismatch batch-edge pixel")
    return {
        "schema": "cohesix-batch-edges-verification/v1",
        "authoritative": False,
        "proof_class": "independent_output_verifier",
        "input_sha256": hashlib.sha256(data).hexdigest(),
        "expected_output_sha256": hashlib.sha256(expected).hexdigest(),
        "observed_output_sha256": hashlib.sha256(observed).hexdigest(),
        "maximum_absolute_error": maximum_error,
        "tolerance": 0,
        "pixels_checked": len(expected),
        "result": "verified",
    }


def read_batch_artifact(path: Path, maximum: int = 262_144) -> bytes:
    """Read a bounded regular batch file without following a changed symlink."""
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > maximum:
            raise CohesixError("ELIMIT batch-edge artifact")
        data = stream.read(maximum + 1)
        if len(data) > maximum:
            raise CohesixError("ELIMIT batch-edge artifact")
        return data


def recovery_guidance(record: Mapping[str, Any]) -> dict[str, str]:
    """Explain the original job's next safe step without changing its state."""
    execution = record.get("execution")
    delivery = record.get("delivery")
    if (execution not in {"reserved", "dispatching", "uncertain",
                          "confirmed", "refused_no_effect"}
            or delivery not in {"pending", "acknowledged"}):
        raise CohesixError("EPERM invalid selected job status")
    if execution == "confirmed":
        next_step = ("Verify the original signed terminal and output bytes."
                     if delivery == "acknowledged" else
                     "Reconcile pending result delivery for the original admission.")
    elif execution == "refused_no_effect":
        next_step = "Inspect the refusal; a corrected request needs a new admission."
    else:
        next_step = ("Read status and reconcile the original admission. "
                     "Keep its capacity reserved until native termination is observed.")
    return {
        "execution": execution,
        "delivery": delivery,
        "next_step": next_step,
        "checkpoint_resume": "unsupported for registered batch edges",
        "restart": "new authorization only after the original outcome is reconciled",
    }
