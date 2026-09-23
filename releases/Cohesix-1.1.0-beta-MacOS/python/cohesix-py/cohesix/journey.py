# Author: Lukas Bower
# Purpose: Reconcile durable CUDA and LoRA operations in ordinary CI and gate only on the shared verifier's requested outcome.
# Copyright 2026 Lukas Bower
"""Installed workflow/CI entrypoint; no new executor, tickets or receipt authority."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import time
from typing import Any, Iterator

from .auth import resolve_secret_reference
from .native_providers import bounded_command
from .playbooks import execute_workflow, run_peft_release
from .providers import ProviderUnavailable

EXIT = {
    "verified": 0,
    "pending": 10,
    "running": 11,
    "ambiguous": 12,
    "refused": 13,
    "cancelled": 14,
    "recovered_failure": 15,
    "failed": 16,
    "timeout": 17,
    "invalid_evidence": 18,
}
LIMIT = 256 * 1024


def encode(value: Any) -> bytes:
    """Canonical public identity encoding, independent of a CI runner attempt."""
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode()


def read(path: Path, maximum: int = LIMIT) -> bytes:
    """Read a bounded regular document without following symlinks."""
    if not path.is_absolute() or ".." in path.parts:
        raise ValueError("absolute document path required")
    if any(p.is_symlink() for p in (path, *path.parents)):
        raise ValueError("document symlink refused")
    with os.fdopen(
        os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb"
    ) as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= maximum:
            raise ValueError("document kind or bound")
        raw = stream.read(maximum + 1)
        after = os.fstat(stream.fileno())
    if len(raw) != before.st_size or after.st_mtime_ns != before.st_mtime_ns:
        raise ValueError("document changed")
    return raw


def document(path: Path) -> dict[str, Any]:
    """Refuse duplicate keys and non-finite JSON instead of accepting aliases."""

    def pairs(rows: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in rows:
            if key in result:
                raise ValueError("duplicate document key")
            result[key] = value
        return result

    def invalid(_: str) -> None:
        raise ValueError("non-finite document value")

    value = json.loads(read(path), object_pairs_hook=pairs, parse_constant=invalid)
    if not isinstance(value, dict):
        raise ValueError("document object required")
    return value


def logical_inputs(kind: str, deployment: dict[str, Any]) -> dict[str, Any]:
    """Bind workload semantics before ticket issuance; authority is checked by coh."""
    if kind == "lora":
        request = dict(deployment["request"])
        request.pop("operation_id", None)
        return request
    if kind != "cuda":
        raise ValueError("unsupported journey")
    stages = deployment["stages"]
    if not isinstance(stages, list) or not 1 <= len(stages) <= 16:
        raise ValueError("bounded CUDA stages required")
    return {
        "contract_sha256": deployment["contract_sha256"],
        "topology": deployment["topology"],
        "stages": [
            {
                "id": row["id"],
                "after": row["after"],
                "runtime": row["runtime"],
                "input_sha256": hashlib.sha256(read(Path(row["input"]))).hexdigest(),
            }
            for row in stages
        ],
    }


def identity(kind: str, purpose: str, rerun: str, deployment: dict[str, Any]) -> str:
    """Changed immutable inputs or explicit rerun intent creates a new operation."""
    for value in (purpose, rerun):
        if not isinstance(value, str) or not re.fullmatch(
            r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}", value
        ):
            raise ValueError("bounded purpose and rerun intent required")
    body = {
        "schema": "cohesix-ci-identity/v1",
        "kind": kind,
        "purpose": purpose,
        "rerun": rerun,
        "inputs": logical_inputs(kind, deployment),
    }
    return "ci-" + hashlib.sha256(encode(body)).hexdigest()[:40]


def load(path: Path) -> tuple[dict[str, Any], dict[str, Any], str]:
    """One explicit configuration supplies both Python and CLI workflow inputs."""
    config = document(path)
    required = {"schema", "kind", "purpose", "rerun", "coh", "deployment", "state"}
    optional = {
        "host",
        "port",
        "rest_url",
        "auth_ref",
        "ticket_ref",
        "package",
        "trust",
        "credential_refs",
    }
    if set(config) - required - optional or not required <= set(config):
        raise ValueError("journey configuration fields")
    if config["schema"] != "cohesix-journey/v1" or config["kind"] not in {
        "cuda",
        "lora",
    }:
        raise ValueError("journey configuration schema")
    for name in ("coh", "deployment", "state"):
        if not isinstance(config[name], str) or not Path(config[name]).is_absolute():
            raise ValueError("absolute configuration paths required")
    deployment = document(Path(config["deployment"]))
    operation = identity(config["kind"], config["purpose"], config["rerun"], deployment)
    actual = (
        deployment["operation_id"]
        if config["kind"] == "cuda"
        else deployment["request"]["operation_id"]
    )
    if actual != operation:
        raise ValueError(
            "operation identity mismatch; derive identity before issuing tickets"
        )
    journal = Path(config["state"]) / operation / "journal"
    if deployment["journal"] != str(journal):
        raise ValueError("journal must reside in the durable operation directory")
    return config, deployment, operation


def save(path: Path, value: dict[str, Any]) -> None:
    """Publish durable bookkeeping atomically before entering a submission path."""
    descriptor, temporary = tempfile.mkstemp(prefix=".journey-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(encode(value) + b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(temporary).unlink(missing_ok=True)


@contextmanager
def operation_lock(
    config: dict[str, Any], deployment: dict[str, Any], operation: str
) -> Iterator[Path]:
    """Keep CI attempts on the same externally retained journal and exact admission."""
    state = Path(config["state"])
    # The operator must provision this durable directory outside runner scratch.
    metadata = state.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_mode & 0o077
        or metadata.st_uid != os.getuid()
        or any(p.is_symlink() for p in (state, *state.parents))
    ):
        raise ValueError("state must be a private owner-controlled durable directory")
    directory = state / operation
    directory.mkdir(mode=0o700, exist_ok=True)
    if directory.is_symlink() or directory.stat().st_mode & 0o077:
        raise ValueError("unsafe operation directory")
    descriptor = os.open(
        directory / "ci.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600
    )
    with os.fdopen(descriptor, "r+b") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        binding = {
            "schema": "cohesix-ci-binding/v1",
            "operation_id": operation,
            "deployment_sha256": hashlib.sha256(encode(deployment)).hexdigest(),
        }
        path = directory / "binding.json"
        if path.exists():
            if document(path) != binding:
                raise ValueError("changed admission or deployment on retry")
            if (directory / "planned.json").exists() and not (
                directory / "journal/recipe.json"
            ).is_file():
                raise ValueError(
                    "lost durable journal; restore state before reconciliation"
                )
        else:
            if (directory / "journal").exists():
                raise ValueError("unenrolled existing journal")
            save(path, binding)
        yield directory


def call(config: dict[str, Any], mode: str) -> dict[str, Any]:
    """Invoke only the installed fixed lifecycle API, with fresh authority on apply."""
    kwargs = {
        "coh_binary": Path(config["coh"]),
        "deployment": Path(config["deployment"]),
    }
    if mode == "apply":
        if bool(config.get("host")) == bool(config.get("rest_url")):
            raise ValueError("select exactly one control target endpoint")
        for name in ("auth_ref", "ticket_ref"):
            if not isinstance(config.get(name), str):
                raise ValueError("fresh scoped authority references required")
            resolve_secret_reference(config[name])
        kwargs.update(
            {
                name: config[name]
                for name in ("host", "port", "rest_url", "auth_ref", "ticket_ref")
                if name in config
            }
        )
    if config["kind"] == "cuda":
        return execute_workflow("cuda-reference", mode, recipe=True, **kwargs)
    return run_peft_release(mode, **kwargs)


def classify(kind: str, report: dict[str, Any], operation: str) -> str:
    """Classify only shared-verifier reports; submit and rollback never mean success."""
    expected = (
        "cohesix-recipe-operation-report/v1"
        if kind == "cuda"
        else "cohesix-peft-report/v1"
    )
    if (
        report.get("schema") != expected
        or report.get("operation_id") != operation
        or report.get("authoritative") is not False
        or report.get("production_use_case_accepted") is not False
    ):
        return "invalid_evidence"
    if kind == "lora":
        result = report.get("result")
        if result is not None:
            if not isinstance(result, dict) or not re.fullmatch(
                r"[a-f0-9]{64}", str(result.get("graph_sha256", ""))
            ):
                return "invalid_evidence"
            return {
                "succeeded": "verified",
                "failed": "failed",
                "cancelled": "cancelled",
                "recovered_failure": "recovered_failure",
                "rollback_failed": "failed",
            }.get(result.get("state"), "invalid_evidence")
        return (
            "running"
            if report.get("acknowledged") is True
            else "ambiguous" if report.get("submitted") is True else "pending"
        )
    stages, attempts = report.get("stages"), report.get("attempts")
    if not isinstance(stages, list) or not stages or not isinstance(attempts, list):
        return "invalid_evidence"
    if report.get("all_steps_verified") is True:
        return (
            "verified"
            if all(row.get("verified") is True and row.get("output") for row in stages)
            else "invalid_evidence"
        )
    for row in attempts:
        if row.get("state") == "verified_terminal" and not row.get("cancel"):
            terminal = row.get("evidence") or {}
            if terminal.get("output") is None:
                return "cancelled" if terminal.get("state") == "cancelled" else "failed"
    unresolved = [row for row in attempts if row.get("state") != "verified_terminal"]
    if unresolved:
        return (
            "running"
            if all(row.get("acknowledged") is True for row in unresolved)
            else "ambiguous"
        )
    return "refused" if report.get("blocker") else "pending"


def outcome(
    config: dict[str, Any],
    deployment: dict[str, Any],
    operation: str,
    state: str,
    report: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Publish a compact CI result without embedding native data, credentials or tickets."""
    steps = (
        [row["execution"] for row in deployment["stages"]]
        if config["kind"] == "cuda"
        else [deployment["execution"]]
    )
    return {
        "schema": "cohesix-ci-outcome/v1",
        "operation_id": operation,
        "kind": config["kind"],
        "state": state,
        "exit_code": EXIT[state],
        "requested_outcome_verified": state == "verified",
        "evidence_refs": [
            {key: step[key] for key in ("graph", "trust", "cas")} for step in steps
        ],
        "report_sha256": hashlib.sha256(encode(report)).hexdigest() if report else None,
        "authoritative": False,
        "production_use_case_accepted": False,
    }


def run(
    config_path: Path,
    *,
    submit: bool = False,
    wait_seconds: int = 0,
    clock: Any = time.monotonic,
    sleep: Any = time.sleep,
) -> dict[str, Any]:
    """Bounded resume on persistent state; a runner retry never mints a new ticket."""
    if type(wait_seconds) is not int or not 0 <= wait_seconds <= 3600:
        raise ValueError("wait must be between 0 and 3600 seconds")
    config, deployment, operation = load(config_path)
    with operation_lock(config, deployment, operation) as directory:
        planned = directory / "planned.json"
        if not planned.exists():
            call(config, "plan")
            save(planned, {"operation_id": operation})
        deadline = clock() + wait_seconds
        while True:
            report = None
            try:
                report = call(config, "watch")
                state = classify(config["kind"], report, operation)
                if state == "pending" and submit:
                    try:
                        report = call(config, "apply")
                        state = classify(config["kind"], report, operation)
                    except (ProviderUnavailable, OSError, ValueError):
                        # The durable native controller has already recorded dispatch intent.
                        # Re-read it once; never turn a failed submit into a new submission.
                        report = call(config, "watch")
                        state = classify(config["kind"], report, operation)
                        if state == "pending":
                            state = "refused"
                if state == "verified":
                    report = call(config, "verify")
                    state = classify(config["kind"], report, operation)
            except (ProviderUnavailable, OSError, ValueError):
                state = "invalid_evidence"
            if state in {"pending", "running", "ambiguous"} and wait_seconds:
                if clock() < deadline:
                    sleep(min(2, max(0, deadline - clock())))
                    continue
                state = "timeout"
            result = outcome(config, deployment, operation, state, report)
            save(directory / "outcome.json", result)
            return result


def doctor(config_path: Path) -> dict[str, Any]:
    """Diagnose each selected boundary without submitting work or claiming execution."""
    config, deployment, operation = load(config_path)
    checks = []

    def check(name: str, probe: Any, remedy: str) -> None:
        try:
            probe()
            status = "ready"
        except (OSError, ValueError, KeyError, ProviderUnavailable):
            status = "unavailable"
        checks.append(
            {
                "component": name,
                "status": status,
                "remediation": remedy if status != "ready" else None,
            }
        )

    check(
        "controller",
        lambda: bounded_command([config["coh"], "--help"]),
        "Install the matching signed native coh package.",
    )

    def package() -> None:
        bounded_command(
            [
                config["coh"],
                "package",
                "verify",
                "--input",
                config["package"],
                "--trust",
                config["trust"],
            ]
        )

    check(
        "package-drift",
        package,
        "Select the independently enrolled package/trust and reinstall exact verified files.",
    )

    def authority() -> None:
        resolve_secret_reference(config["auth_ref"])
        resolve_secret_reference(config["ticket_ref"])
        steps = (
            [row["execution"] for row in deployment["stages"]]
            if config["kind"] == "cuda"
            else [deployment["execution"]]
        )
        now = int(time.time() * 1000)
        if any(
            type(row["request"]["expires_unix_ms"]) is not int
            or row["request"]["expires_unix_ms"] <= now
            for row in steps
        ):
            raise ValueError("expired authority")

    check(
        "secrets-authority",
        authority,
        "Enroll fresh bounded tickets and private file:/env: secret references; never embed credentials.",
    )

    def cache() -> None:
        with operation_lock(config, deployment, operation):
            call(config, "plan")

    check(
        "cache-evidence-bounds",
        cache,
        "Provision private durable state and exact input/CAS/trust files; run the shared planner to locate a bound or binding refusal.",
    )
    # Target and remote native health require authenticated observations. Local
    # package success must not invent remote service health or a resource claim.
    for name, remedy in [
        (
            "control-target",
            "Use authenticated cohsh cat /proc/boot and confirm the selected manifest and READY Worker role.",
        ),
        (
            "cuda-executor",
            "Run packaged coh doctor --local-gpu on the enrolled Linux CUDA host; verify the allowlisted helper digest and device nodes.",
        ),
        (
            "native-runtime",
            "For LoRA, run the shipped pinned helper validation and serving health check on the executor; for CUDA, compare the requested driver/runtime.",
        ),
        (
            "resource-bounds",
            "Measure native memory headroom and inspect systemd MemoryMax/CPUQuota/TasksMax and ticket expiry before admission.",
        ),
    ]:
        checks.append(
            {"component": name, "status": "not_observed", "remediation": remedy}
        )
    return {
        "schema": "cohesix-journey-doctor/v1",
        "operation_id": operation,
        "authoritative": False,
        "ready_for_submission": False,
        "checks": checks,
        "optional_providers": "unavailable providers do not enable or block the selected workflows",
    }


def main() -> int:
    """Expose the same bounded outcome contract to shells and Python callers."""
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    derive = commands.add_parser(
        "identity", help="derive stable identity before ticket issuance"
    )
    derive.add_argument("--kind", choices=("cuda", "lora"), required=True)
    derive.add_argument("--purpose", required=True)
    derive.add_argument("--rerun", default="initial")
    derive.add_argument("--deployment", type=Path, required=True)
    for name in ("run", "doctor", "validate"):
        command = commands.add_parser(name)
        command.add_argument("--config", type=Path, required=True)
        if name == "run":
            command.add_argument(
                "--submit",
                action="store_true",
                help="advance only unsubmitted work using fresh scoped authority",
            )
            command.add_argument("--wait-seconds", type=int, default=0)
    args = parser.parse_args()
    try:
        if args.command == "identity":
            result = {
                "operation_id": identity(
                    args.kind, args.purpose, args.rerun, document(args.deployment)
                )
            }
        elif args.command == "run":
            result = run(
                args.config, submit=args.submit, wait_seconds=args.wait_seconds
            )
        elif args.command == "doctor":
            result = doctor(args.config)
        else:
            _, _, operation = load(args.config)
            result = {
                "operation_id": operation,
                "state": "validated",
                "authority_used": False,
            }
        print(json.dumps(result, indent=2, allow_nan=False))
        return result.get(
            "exit_code",
            (
                0
                if args.command != "doctor"
                or all(row["status"] != "unavailable" for row in result["checks"])
                else 13
            ),
        )
    except (OSError, ValueError, KeyError, TypeError, ProviderUnavailable):
        # No exception text: external errors may contain private paths or native data.
        print(
            json.dumps(
                {
                    "schema": "cohesix-ci-outcome/v1",
                    "state": "refused",
                    "exit_code": 13,
                    "requested_outcome_verified": False,
                    "remediation": "Check the installed adoption guide, exact identity, durable state and authority references.",
                }
            )
        )
        return 13


if __name__ == "__main__":
    raise SystemExit(main())
