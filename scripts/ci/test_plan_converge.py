#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Run non-claiming target-first Cohesix convergence diagnostics.
# Copyright 2026 Lukas Bower

"""Select and run the shortest safe path to target evidence during development."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import signal
import subprocess
import sys
import uuid
from typing import Any, NoReturn

from test_plan_catalog import (
    DEFAULT_CATALOG,
    CatalogError,
    catalog_digest,
    convergence_actions,
    find_focus,
    load_catalog,
    select_convergence_focus,
)
from test_plan_evidence import environment_secret_values, redact_text


ROOT = pathlib.Path(__file__).resolve().parents[2]
BANNER = "NON-CLAIMING TARGET DIAGNOSTIC"
RESULT_SCHEMA = "cohesix-test-plan-convergence/v1"
OBSERVATION_SCHEMA = "cohesix-target-observation/v2"


class ConvergenceError(ValueError):
    """Report an unsafe, incomplete, or invalid convergence run."""


def fail(message: str) -> NoReturn:
    """Raise one user-facing convergence error."""

    raise ConvergenceError(message)


def utc_now() -> str:
    """Return the current time as a stable UTC string."""

    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha256_file(path: pathlib.Path) -> str:
    """Hash a file without loading it all into memory."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_write_json(path: pathlib.Path, payload: Any) -> None:
    """Write JSON atomically inside a fresh convergence directory."""

    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    if temporary.exists():
        fail(f"temporary convergence path already exists: {temporary}")
    with temporary.open("x", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


def run_text(arguments: list[str], *, required: bool = True) -> str:
    """Run a read-only repository command and return stripped output."""

    result = subprocess.run(
        arguments,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if required and result.returncode != 0:
        fail(
            f"command failed ({result.returncode}): {' '.join(arguments)}: "
            f"{result.stderr.strip()}"
        )
    return result.stdout.strip()


def source_identity() -> dict[str, Any]:
    """Bind the commit and complete dirty-tree identity for this run."""

    commit = run_text(["git", "rev-parse", "HEAD"])
    status = subprocess.run(
        ["git", "status", "--porcelain=v2", "--untracked-files=all", "-z"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if status.returncode != 0:
        fail(f"unable to record dirty-tree identity: {status.stderr!r}")
    digest_command = ROOT / "scripts" / "ci" / "qemu_artifact.py"
    source_digest = run_text(
        [str(digest_command), "source-digest", "--repo-root", str(ROOT)]
    )
    if not source_digest.startswith("sha256:"):
        source_digest = f"sha256:{source_digest}"
    return {
        "git_commit": commit,
        "dirty": bool(status.stdout),
        "git_status_porcelain_v2_sha256": hashlib.sha256(
            status.stdout
        ).hexdigest(),
        "source_digest": source_digest,
    }


def changed_paths(changed_from: str | None) -> list[str]:
    """Return tracked and untracked paths relevant to automatic routing."""

    working = run_text(
        ["git", "diff", "--name-only", "--diff-filter=ACMRTUXB", "HEAD"]
    ).splitlines()
    committed: list[str] = []
    if changed_from:
        committed = run_text(
            [
                "git",
                "diff",
                "--name-only",
                "--diff-filter=ACMRTUXB",
                f"{changed_from}...HEAD",
            ]
        ).splitlines()
    untracked = run_text(
        ["git", "ls-files", "--others", "--exclude-standard"],
    ).splitlines()
    return sorted(
        {path for path in working + committed + untracked if path}
    )


def fresh_state_dir(raw: pathlib.Path | None, run_id: str) -> pathlib.Path:
    """Create one immutable convergence state directory below out/."""

    state = raw or ROOT / "out" / "test-plan-convergence" / run_id
    if not state.is_absolute():
        state = ROOT / state
    state = state.resolve(strict=False)
    allowed = (ROOT / "out").resolve()
    try:
        state.relative_to(allowed)
    except ValueError as error:
        raise ConvergenceError(
            f"convergence state must be below {allowed}: {state}"
        ) from error
    if state.exists():
        fail(f"convergence state already exists; results are immutable: {state}")
    state.mkdir(parents=True)
    return state


def action_environment(
    args: argparse.Namespace,
    *,
    state: pathlib.Path,
    run_id: str,
    focus_id: str,
    observation: pathlib.Path,
) -> dict[str, str]:
    """Build the non-secret selectors consumed by diagnostic actions."""

    environment = os.environ.copy()
    environment.update(
        {
            "TEST_PLAN_ROOT": str(ROOT),
            "TEST_PLAN_CONVERGENCE": "1",
            "TEST_PLAN_CONVERGENCE_STATE_DIR": str(state),
            "TEST_PLAN_CONVERGENCE_RUN_ID": run_id,
            "TEST_PLAN_CONVERGENCE_FOCUS": focus_id,
            "TEST_PLAN_CONVERGENCE_TARGET": args.target,
            "TEST_PLAN_TARGET_OBSERVATION": str(observation),
            "TEST_PLAN_CONVERGENCE_LAUNCH_EXISTING": (
                "1" if args.launch_existing else "0"
            ),
        }
    )
    optional = {
        "TEST_PLAN_PI4_TARGET_EVIDENCE": args.pi4_target_evidence,
        "TEST_PLAN_PI4_READBACK_IMAGE": args.pi4_readback_image,
        "TEST_PLAN_PI4_IDENTITY_METADATA": args.pi4_identity_metadata,
        "TEST_PLAN_PI4_SERIAL_LOG": args.pi4_serial_log,
        "TEST_PLAN_PI4_HOST": args.pi4_host,
        "TEST_PLAN_CONVERGENCE_READY_MARKER": args.ready_marker,
        "TEST_PLAN_CONVERGENCE_OPERATION_SCRIPT": args.operation_script,
    }
    for name, value in optional.items():
        if value:
            environment[name] = str(value)
    return environment


def run_action(
    action: dict[str, Any],
    *,
    environment: dict[str, str],
    log_path: pathlib.Path,
) -> tuple[int, str, str]:
    """Run one catalog action with its timeout/policy and a redacted log."""

    helper = ROOT / "scripts" / "ci" / "test_plan_evidence.py"
    command = [
        sys.executable,
        str(helper),
        "run-catalog-command",
        "--root",
        str(ROOT),
        "--timeout-seconds",
        str(action["timeout_seconds"]),
        "--test-policy",
        action["test_policy"],
        "--command",
        action["command"],
    ]
    minimum = action.get("minimum_test_count")
    if minimum is not None:
        command.extend(["--minimum-test-count", str(minimum)])
    started = utc_now()
    secret_values = environment_secret_values()
    with log_path.open("x", encoding="utf-8") as log:
        process = subprocess.Popen(
            command,
            cwd=ROOT,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            start_new_session=True,
        )
        assert process.stdout is not None
        try:
            for line in process.stdout:
                safe = redact_text(line, secret_values)
                sys.stdout.write(safe)
                sys.stdout.flush()
                log.write(safe)
                log.flush()
            return_code = process.wait(timeout=10)
        except (KeyboardInterrupt, subprocess.TimeoutExpired):
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            process.wait(timeout=10)
            raise
    return return_code, started, utc_now()


def load_observation(path: pathlib.Path) -> dict[str, Any] | None:
    """Load and minimally validate one target observation if produced."""

    if not path.is_file():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ConvergenceError(f"invalid target observation {path}: {error}")
    if not isinstance(payload, dict) or payload.get("schema") != OBSERVATION_SCHEMA:
        fail(f"unsupported target observation schema: {path}")
    if payload.get("claiming") is not False:
        fail(f"target observation is not explicitly non-claiming: {path}")
    return payload


def validate_result(path: pathlib.Path) -> dict[str, Any]:
    """Validate a convergence record as diagnostic evidence only."""

    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ConvergenceError(f"unable to read convergence result: {error}")
    if not isinstance(payload, dict) or payload.get("schema") != RESULT_SCHEMA:
        fail("unsupported convergence result schema")
    if payload.get("banner") != BANNER:
        fail("convergence result is missing the non-claiming banner")
    if payload.get("claiming") is not False:
        fail("convergence result must set claiming=false")
    if payload.get("promotion_eligible") is not False:
        fail("convergence result must set promotion_eligible=false")
    if payload.get("result") not in {"PASS", "FAIL", "BLOCKED"}:
        fail("convergence result has an invalid terminal status")
    forbidden = {"stage", "stage_attestation", "acceptance_record"}
    present = sorted(forbidden & payload.keys())
    if present:
        fail(f"convergence result contains acceptance-only fields: {present}")
    result_root = path.resolve().parent
    actions = payload.get("actions")
    if not isinstance(actions, list):
        fail("convergence result actions must be a list")
    for action in actions:
        if not isinstance(action, dict):
            fail("convergence result action must be an object")
        if action.get("evidence_class") != "diagnostic":
            fail("convergence action is not explicitly diagnostic")
        relative_log = pathlib.PurePosixPath(str(action.get("log", "")))
        if relative_log.is_absolute() or ".." in relative_log.parts:
            fail(f"unsafe convergence action log path: {relative_log}")
        log = (result_root / pathlib.Path(relative_log)).resolve()
        try:
            log.relative_to(result_root)
        except ValueError as error:
            raise ConvergenceError(
                f"convergence action log escapes result directory: {log}"
            ) from error
        if not log.is_file():
            fail(f"convergence action log is missing: {log}")
        if sha256_file(log) != action.get("log_sha256"):
            fail(f"convergence action log digest mismatch: {log}")
    for field in ("uart_serial_log", "built_image", "image_identity"):
        record = payload.get(field)
        if record is None:
            continue
        if not isinstance(record, dict) or record.get("present") is not True:
            fail(f"convergence {field} record is incomplete")
        artifact = pathlib.Path(str(record.get("path", "")))
        if not artifact.is_absolute() or not artifact.is_file():
            fail(f"convergence {field} artifact is unavailable: {artifact}")
        if artifact.stat().st_size != record.get("size_bytes"):
            fail(f"convergence {field} size mismatch: {artifact}")
        if sha256_file(artifact) != record.get("sha256"):
            fail(f"convergence {field} digest mismatch: {artifact}")
    return payload


def run(args: argparse.Namespace) -> int:
    """Execute the selected convergence plan and write its terminal result."""

    catalog = load_catalog(args.catalog.resolve())
    paths = sorted(set(args.path or changed_paths(args.changed_from)))
    if args.focus:
        focus = find_focus(catalog, args.focus)
        if args.target not in focus["targets"]:
            fail(f"focus {args.focus} does not support target {args.target}")
    else:
        focus, paths = select_convergence_focus(
            catalog,
            target=args.target,
            changed_paths=paths,
        )
    actions = convergence_actions(
        catalog,
        target=args.target,
        focus_id=focus["id"],
    )
    if not actions:
        fail(f"no convergence actions selected for {args.target}/{focus['id']}")

    run_id = (
        datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        + f"-{uuid.uuid4().hex[:12]}"
    )
    state = fresh_state_dir(args.state_dir, run_id)
    logs = state / "logs"
    logs.mkdir()
    observation_path = state / "target-observation.json"
    environment = action_environment(
        args,
        state=state,
        run_id=run_id,
        focus_id=focus["id"],
        observation=observation_path,
    )
    identity = source_identity()
    environment["TEST_PLAN_SOURCE_DIGEST"] = identity["source_digest"]
    started_at = utc_now()
    action_results: list[dict[str, Any]] = []
    status = "PASS"
    first_failed_layer: str | None = None

    print(BANNER)
    print(
        f"run={run_id} target={args.target} focus={focus['id']} "
        f"actions={','.join(action['id'] for action in actions)}"
    )
    for ordinal, action in enumerate(actions, start=1):
        phase = action["convergence_phase"]
        print(f"[{ordinal}/{len(actions)}] {phase}: {action['id']}")
        log_path = logs / f"{ordinal:02d}-{action['id']}.log"
        return_code, action_started, action_finished = run_action(
            action,
            environment=environment,
            log_path=log_path,
        )
        if return_code == 3:
            action_status = "BLOCKED"
        elif return_code == 0:
            action_status = "PASS"
        else:
            action_status = "FAIL"
        action_results.append(
            {
                "id": action["id"],
                "evidence_class": "diagnostic",
                "catalog_evidence_class": action.get(
                    "evidence_class", "acceptance"
                ),
                "phase": phase,
                "status": action_status,
                "exit_code": return_code,
                "started_at_utc": action_started,
                "finished_at_utc": action_finished,
                "log": log_path.relative_to(state).as_posix(),
                "log_sha256": sha256_file(log_path),
            }
        )
        if return_code != 0:
            status = action_status
            first_failed_layer = phase
            break

    observation = load_observation(observation_path)
    if observation and observation.get("first_failing_proof_layer"):
        first_failed_layer = str(observation["first_failing_proof_layer"])
    selected_canary = any(
        action["convergence_phase"] == "target-canary" for action in actions
    )
    if status == "PASS" and selected_canary:
        if observation is None:
            status = "FAIL"
            first_failed_layer = "target-observation"
        elif observation.get("result") != "PASS":
            status = str(observation.get("result", "FAIL"))
            first_failed_layer = str(
                observation.get("first_failing_proof_layer", "target-observation")
            )

    payload: dict[str, Any] = {
        "schema": RESULT_SCHEMA,
        "banner": BANNER,
        "claiming": False,
        "promotion_eligible": False,
        "run_id": run_id,
        "session_id": run_id,
        "started_at_utc": started_at,
        "finished_at_utc": utc_now(),
        "result": status,
        "first_failing_proof_layer": first_failed_layer,
        "target": args.target,
        "focus": focus["id"],
        "profile": focus["profile"],
        "changed_paths": paths,
        "selected_actions": [action["id"] for action in actions],
        "catalog_sha256": catalog_digest(catalog),
        "source": identity,
        "hypothesis": args.hypothesis,
        "diagnostic_note": args.note,
        "actions": action_results,
        "target_observation": observation,
        "uart_serial_log": (
            observation.get("serial_log") if observation else None
        ),
        "built_image": (
            observation.get("built_image") if observation else None
        ),
        "image_identity": (
            observation.get("image_identity") if observation else None
        ),
    }
    result_path = state / "convergence-result.json"
    atomic_write_json(result_path, payload)
    (state / "convergence-result.sha256").write_text(
        f"{sha256_file(result_path)}  convergence-result.json\n",
        encoding="utf-8",
    )
    validate_result(result_path)
    print(
        f"{BANNER}: result={status} first_failed_layer="
        f"{first_failed_layer or 'none'} record={result_path}"
    )
    return 0 if status == "PASS" else (3 if status == "BLOCKED" else 1)


def parser() -> argparse.ArgumentParser:
    """Build the convergence command-line interface."""

    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--catalog", type=pathlib.Path, default=DEFAULT_CATALOG)
    subparsers = result.add_subparsers(dest="subcommand")
    validate = subparsers.add_parser("validate-result")
    validate.add_argument("--result", type=pathlib.Path, required=True)
    result.add_argument("--target", choices=("qemu", "pi4"))
    result.add_argument("--focus")
    result.add_argument("--path", action="append", default=[])
    result.add_argument("--changed-from")
    result.add_argument("--state-dir", type=pathlib.Path)
    result.add_argument("--launch-existing", action="store_true")
    result.add_argument("--hypothesis")
    result.add_argument("--note")
    result.add_argument("--ready-marker")
    result.add_argument("--operation-script", type=pathlib.Path)
    result.add_argument("--pi4-target-evidence", type=pathlib.Path)
    result.add_argument("--pi4-readback-image", type=pathlib.Path)
    result.add_argument("--pi4-identity-metadata", type=pathlib.Path)
    result.add_argument("--pi4-serial-log", type=pathlib.Path)
    result.add_argument("--pi4-host")
    result.add_argument("--list-focus", action="store_true")
    return result


def main(argv: list[str] | None = None) -> int:
    """Run a convergence diagnostic or validate its result."""

    args = parser().parse_args(argv)
    try:
        if args.subcommand == "validate-result":
            payload = validate_result(args.result.resolve())
            print(
                f"valid non-claiming convergence result: "
                f"{payload['run_id']} {payload['result']}"
            )
            return 0
        catalog = load_catalog(args.catalog.resolve())
        if args.list_focus:
            for focus in catalog["convergence_focus"]:
                print(
                    f"{focus['id']}\t{','.join(focus['targets'])}\t"
                    f"{focus['authoritative_evidence']}"
                )
            return 0
        if not args.target:
            fail("--target is required")
        return run(args)
    except (
        CatalogError,
        ConvergenceError,
        OSError,
        subprocess.SubprocessError,
    ) as error:
        print(f"test-plan convergence: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
