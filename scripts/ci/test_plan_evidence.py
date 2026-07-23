#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Create and verify immutable provenance for Cohesix test-plan runs.
# Copyright 2026 Lukas Bower

"""Create, qualify, import, and verify staged test-plan evidence."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import platform
import re
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
import uuid
from typing import Any, NoReturn, Sequence

from test_plan_catalog import load_catalog, select_actions


ACTION_SCHEMA = "cohesix.test-plan-action/v1"
ATTEMPT_SCHEMA = "cohesix.test-plan-attempt/v1"
CONTEXT_SCHEMA = "cohesix.test-plan-input-context/v1"
PENDING_SCHEMA = "cohesix.test-plan-pending-attempt/v1"
QUALIFICATION_SCHEMA = "cohesix.test-plan-target-qualification/v1"
REF_SCHEMA = "cohesix.test-plan-attestation-ref/v1"
STAGE_SCHEMA = "cohesix.test-plan-stage-attestation/v1"
SECRET_TERMS = (
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "TICKET",
    "API_KEY",
    "AUTHORIZATION",
    "CREDENTIAL",
)
STAGE_SCRIPTS = {
    1: "scripts/ci/test_plan_stage_01_integrity.sh",
    2: "scripts/ci/test_plan_stage_02_host_fast.sh",
    3: "scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh",
    4: "scripts/ci/test_plan_stage_04_rest_multiplexer.sh",
    5: "scripts/ci/test_plan_stage_05_due_diligence.sh",
}


class EvidenceError(ValueError):
    """Report invalid, stale, or incomplete test evidence."""


def fail(message: str) -> NoReturn:
    """Raise a user-facing evidence error."""

    raise EvidenceError(message)


def utc_now() -> str:
    """Return a stable UTC timestamp."""

    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def new_id() -> str:
    """Return a sortable, collision-resistant evidence identifier."""

    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime(
        "%Y%m%dT%H%M%S.%fZ"
    )
    return f"{timestamp}-{os.getpid()}-{uuid.uuid4().hex[:12]}"


def canonical_digest(value: Any) -> str:
    """Hash a JSON-compatible value using canonical serialization."""

    encoded = json.dumps(
        value,
        ensure_ascii=True,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    """Hash a file without loading it all into memory."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_digest(path: pathlib.Path) -> str:
    """Hash one file, symlink, or complete directory tree."""

    digest = hashlib.sha256()
    if path.is_symlink():
        digest.update(b"symlink\0")
        digest.update(os.readlink(path).encode("utf-8"))
        return digest.hexdigest()
    if path.is_file():
        digest.update(b"file\0")
        digest.update(sha256_file(path).encode("ascii"))
        return digest.hexdigest()
    if not path.is_dir():
        fail(f"required artifact is missing: {path}")
    digest.update(b"dir\0")
    for child in sorted(path.rglob("*")):
        relative = child.relative_to(path).as_posix()
        metadata = child.lstat()
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(stat.S_IMODE(metadata.st_mode)).encode("ascii"))
        digest.update(b"\0")
        if child.is_symlink():
            digest.update(b"symlink\0")
            digest.update(os.readlink(child).encode("utf-8"))
        elif child.is_file():
            digest.update(b"file\0")
            digest.update(sha256_file(child).encode("ascii"))
        elif child.is_dir():
            digest.update(b"dir")
        digest.update(b"\n")
    return digest.hexdigest()


def atomic_write_json(path: pathlib.Path, payload: Any) -> None:
    """Write JSON durably and replace its active path atomically."""

    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f".{path.name}.",
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary_name, path)
    finally:
        if os.path.exists(temporary_name):
            os.unlink(temporary_name)


def atomic_write_text(path: pathlib.Path, text: str) -> None:
    """Write text durably and replace its active path atomically."""

    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f".{path.name}.",
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary_name, path)
    finally:
        if os.path.exists(temporary_name):
            os.unlink(temporary_name)


def load_json(path: pathlib.Path) -> dict[str, Any]:
    """Load a JSON object with a useful error."""

    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"invalid JSON evidence {path}: {error}")
    if not isinstance(payload, dict):
        fail(f"JSON evidence must contain an object: {path}")
    return payload


def confined_path(
    root: pathlib.Path,
    relative_text: str,
) -> pathlib.Path:
    """Resolve a recorded relative path without permitting traversal."""

    resolved_root = root.resolve()
    relative = pathlib.PurePosixPath(relative_text)
    if relative.is_absolute() or ".." in relative.parts:
        fail(f"unsafe evidence path: {relative_text}")
    path = (resolved_root / pathlib.Path(*relative.parts)).resolve()
    try:
        path.relative_to(resolved_root)
    except ValueError as error:
        raise EvidenceError(f"evidence path escapes its root: {path}") from error
    return path


def relative_to_state(state: pathlib.Path, path: pathlib.Path) -> str:
    """Return a safe state-relative path."""

    try:
        return path.resolve().relative_to(state.resolve()).as_posix()
    except ValueError as error:
        raise EvidenceError(
            f"evidence path escapes state directory: {path}"
        ) from error


def run_text(
    command: Sequence[str],
    *,
    cwd: pathlib.Path,
    required: bool = True,
) -> str:
    """Run a small metadata command and return merged output."""

    result = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    output = result.stdout.rstrip()
    if required and result.returncode != 0:
        fail(
            f"context command failed ({result.returncode}): "
            f"{shlex.join(command)}\n{output}"
        )
    return output


def git_bytes(root: pathlib.Path, arguments: Sequence[str]) -> bytes:
    """Run Git and return exact bytes."""

    result = subprocess.run(
        ["git", *arguments],
        cwd=root,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        fail(
            f"git {' '.join(arguments)} failed ({result.returncode}): "
            f"{result.stderr.decode(errors='replace').rstrip()}"
        )
    return result.stdout


def index_modes(root: pathlib.Path) -> dict[str, str]:
    """Return tracked path to Git index mode."""

    output = git_bytes(root, ["ls-files", "--stage", "-z"])
    modes: dict[str, str] = {}
    for entry in output.split(b"\0"):
        if not entry:
            continue
        metadata, raw_path = entry.split(b"\t", 1)
        modes[os.fsdecode(raw_path)] = metadata.split(b" ", 1)[0].decode(
            "ascii"
        )
    return modes


def file_record(
    root: pathlib.Path,
    relative: str,
    tracked_mode: str | None,
) -> dict[str, Any]:
    """Describe one source/config input by path, mode, kind, and digest."""

    path = root / relative
    record: dict[str, Any] = {
        "path": relative,
        "index_mode": tracked_mode or "untracked",
    }
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        record["kind"] = "missing"
        return record
    record["mode"] = stat.S_IMODE(metadata.st_mode)
    if path.is_symlink():
        record["kind"] = "symlink"
        record["sha256"] = hashlib.sha256(
            os.readlink(path).encode("utf-8")
        ).hexdigest()
    elif path.is_file():
        record["kind"] = "file"
        record["sha256"] = sha256_file(path)
    elif path.is_dir() and tracked_mode == "160000":
        record["kind"] = "submodule"
        record["head"] = run_text(
            ["git", "rev-parse", "HEAD"],
            cwd=path,
            required=False,
        )
        record["status"] = run_text(
            ["git", "status", "--porcelain=v2", "--untracked-files=all"],
            cwd=path,
            required=False,
        )
    else:
        record["kind"] = "directory"
    return record


def safe_environment_value(key: str, value: str) -> str:
    """Remove URL credentials and query material from recorded selectors."""

    if "://" not in value and not any(
        marker in key.upper() for marker in ("URL", "URI")
    ):
        return value
    try:
        parsed = urllib.parse.urlsplit(value)
        hostname = parsed.hostname
        port = parsed.port
    except ValueError:
        return "<redacted-url>"
    if not parsed.scheme or not hostname:
        return "<redacted-url>"
    rendered_host = f"[{hostname}]" if ":" in hostname else hostname
    netloc = rendered_host if port is None else f"{rendered_host}:{port}"
    return urllib.parse.urlunsplit(
        (parsed.scheme, netloc, parsed.path, "", "")
    )


def selected_environment(scope: str) -> dict[str, str]:
    """Collect non-secret environment selectors that affect test behavior."""

    excluded = {
        "TEST_PLAN_ROOT",
        "TEST_PLAN_STATE_DIR",
        "TEST_PLAN_ATTEMPT_ID",
        "TEST_PLAN_ATTEMPT_MANIFEST",
        "TEST_PLAN_SOURCE_DIGEST",
        "TEST_PLAN_CONTEXT_DIGEST",
        "TEST_PLAN_ITERATION",
        "TEST_PLAN_FORCE",
        "TEST_PLAN_STAGED_RUN",
        "TEST_PLAN_TARGET",
        "COHSH_BATCH_TARGET",
        "COHSH_LOG_ROOT",
        "COHSH_QEMU_ARTIFACT_ROOT",
        "COHSH_TRANSPORT_RESULT_ROOT",
        "TP_PYTHON_PLAYBOOK_OUT",
    }
    selected: dict[str, str] = {}
    for key, value in sorted(os.environ.items()):
        if key in excluded or any(term in key.upper() for term in SECRET_TERMS):
            continue
        relevant = (
            key.startswith(("SEL4_", "RUST", "CARGO_", "TP_", "DD_"))
            or key
            in {
                "CC",
                "CFLAGS",
                "CXX",
                "CXXFLAGS",
                "MACOSX_DEPLOYMENT_TARGET",
            }
        )
        if scope == "target":
            relevant = relevant or key.startswith(
                ("COHSH_", "COHESIX_", "COH_", "HIVE_", "PI4_")
            )
        if not relevant:
            continue
        selected[key] = safe_environment_value(key, value)
    return selected


def stage_scope(stage: int) -> str:
    """Return whether a whole-stage attestation is cross-target reusable."""

    if stage == 1:
        return "common"
    if stage in {2, 3, 4, 5}:
        return "target"
    fail(f"invalid test-plan stage: {stage}")


def selected_sel4_dir(
    root: pathlib.Path,
    target: str,
) -> pathlib.Path | None:
    """Resolve the explicit or canonical profile bound to target stages."""

    override = os.environ.get("SEL4_BUILD_DIR")
    if override:
        return pathlib.Path(override).expanduser().resolve(strict=False)
    if target == "qemu":
        return (
            root / "out/sel4/profile-v2/qemu-smp-production"
        ).resolve(strict=False)
    if target == "pi4":
        return (
            root / "out/sel4/profile-v2/pi4-diagnostic"
        ).resolve(strict=False)
    return None


def external_sel4_records(
    root: pathlib.Path,
    target: str,
    scope: str,
) -> list[dict[str, Any]]:
    """Record selected seL4 profile truth without scanning the whole build."""

    if scope != "target":
        return []
    build_dir = selected_sel4_dir(root, target)
    if build_dir is None:
        return [{"kind": "sel4-build-dir", "path": "", "exists": False}]
    records: list[dict[str, Any]] = [
        {
            "kind": "sel4-build-dir",
            "path": str(build_dir),
            "exists": build_dir.is_dir(),
        }
    ]
    candidates = (
        "CMakeCache.txt",
        ".config",
        "profile.json",
        "profile.env",
        "cohesix-profile-build-inputs.json",
        ".cohesix-sel4-profile.json",
        "sel4_profile.json",
        "kernel/autoconf/autoconf.h",
        "kernel/gen_config/kernel/gen_config.h",
        "kernel/gen_config/kernel/gen_config.json",
        "libsel4/autoconf/autoconf.h",
        "libsel4/include/sel4/gen_config.h",
        "libsel4/include/sel4/autoconf.h",
        "kernel/kernel.elf",
        "libsel4/libsel4.a",
        "elfloader/elfloader",
        "elfloader/kernel.elf",
        "elfloader/rootserver",
        "kernel.elf",
        "images/kernel.elf",
    )
    for relative in candidates:
        path = build_dir / relative
        if path.is_file():
            records.append(
                {
                    "kind": "file",
                    "path": relative,
                    "sha256": sha256_file(path),
                }
            )
    return records


def tool_record(root: pathlib.Path, command: Sequence[str]) -> dict[str, Any]:
    """Record executable identity and version output."""

    resolved = shutil.which(command[0])
    if resolved is None:
        return {
            "command": list(command),
            "path": None,
            "returncode": 127,
            "output": "missing",
        }
    result = subprocess.run(
        command,
        cwd=root,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    return {
        "command": list(command),
        "path": str(pathlib.Path(resolved).resolve()),
        "returncode": result.returncode,
        "output": result.stdout.rstrip(),
    }


def capture_context(
    root: pathlib.Path,
    state: pathlib.Path,
    stage: int,
    target: str,
) -> dict[str, Any]:
    """Capture exact source, configuration, toolchain, target, and actions."""

    root = root.resolve()
    state = state.resolve()
    scope = stage_scope(stage)
    modes = index_modes(root)
    head = run_text(["git", "rev-parse", "HEAD"], cwd=root)
    untracked_output = git_bytes(
        root,
        ["ls-files", "--others", "--exclude-standard", "-z"],
    )
    untracked_paths = sorted(
        {os.fsdecode(value) for value in untracked_output.split(b"\0") if value}
    )
    tracked_diff = git_bytes(
        root,
        [
            "--no-pager",
            "diff",
            "--binary",
            "--full-index",
            "--no-ext-diff",
            "HEAD",
            "--",
        ],
    )
    status = git_bytes(
        root,
        ["status", "--porcelain=v2", "--untracked-files=all", "-z"],
    )
    source = {
        "git_head": head,
        "tracked_diff_sha256": hashlib.sha256(tracked_diff).hexdigest(),
        "status_sha256": hashlib.sha256(status).hexdigest(),
        "untracked": [
            file_record(root, relative, modes.get(relative))
            for relative in untracked_paths
        ],
        "submodules": [
            file_record(root, relative, mode)
            for relative, mode in sorted(modes.items())
            if mode == "160000"
        ],
    }
    source_digest = canonical_digest(source)

    config_output = git_bytes(
        root,
        [
            "ls-files",
            "-z",
            "--",
            "Cargo.lock",
            "Cargo.toml",
            ":(glob)**/Cargo.toml",
            "rust-toolchain",
            "rust-toolchain.toml",
            "configs/**",
            "apps/root-task/src/generated/**",
            "docs/snippets/**",
        ],
    )
    config_paths = {
        os.fsdecode(value)
        for value in config_output.split(b"\0")
        if value
    }
    config_prefixes = (
        "configs/",
        "apps/root-task/src/generated/",
        "docs/snippets/",
    )
    config_names = {
        "Cargo.lock",
        "Cargo.toml",
        "rust-toolchain",
        "rust-toolchain.toml",
    }
    config_paths.update(
        relative
        for relative in untracked_paths
        if relative in config_names
        or relative.endswith("/Cargo.toml")
        or relative.startswith(config_prefixes)
    )
    config = {
        "files": [
            file_record(root, relative, modes.get(relative))
            for relative in sorted(config_paths)
        ],
        "environment": selected_environment(scope),
        "external": external_sel4_records(root, target, scope),
    }
    config_digest = canonical_digest(config)

    action_paths = [
        "scripts/ci/test_plan_common.sh",
        "scripts/ci/test_plan_evidence.py",
        "scripts/ci/test_plan_resources.sh",
        "scripts/ci/test_plan_run.sh",
        STAGE_SCRIPTS[stage],
        "configs/test_plan_actions.toml",
        "scripts/ci/test_plan_catalog.py",
    ]
    action_files = [
        file_record(root, relative, modes.get(relative))
        for relative in action_paths
    ]
    catalog_path = root / "configs/test_plan_actions.toml"
    selected_catalog_actions: list[dict[str, Any]] = []
    if catalog_path.is_file():
        catalog = load_catalog(catalog_path)
        selected_catalog_actions = select_actions(
            catalog,
            stage=stage,
            target=target if target in {"qemu", "pi4"} else None,
        )
    action_set = {
        "runner_files": action_files,
        "selected_catalog_actions": selected_catalog_actions,
    }
    action_digest = canonical_digest(action_set)

    toolchain = {
        "platform": {
            "system": platform.system(),
            "machine": platform.machine(),
            "release": platform.release(),
        },
        "tools": [
            tool_record(root, ["bash", "--version"]),
            tool_record(root, ["git", "--version"]),
            tool_record(root, ["cargo", "--version"]),
            tool_record(root, ["rustc", "-Vv"]),
            tool_record(root, ["python3", "--version"]),
        ],
    }
    toolchain_digest = canonical_digest(toolchain)

    dependencies: list[dict[str, Any]] = []
    if stage > 1:
        dependency_stage = stage - 1
        ref_path = state / f"stage_{dependency_stage:02d}.attestation.json"
        dependency_sha = "missing"
        if ref_path.is_file():
            try:
                dependency_sha = str(load_json(ref_path).get("sha256", ""))
            except EvidenceError:
                dependency_sha = "invalid"
        dependencies.append(
            {
                "stage": dependency_stage,
                "attestation_sha256": dependency_sha,
            }
        )

    binding = {
        "schema": CONTEXT_SCHEMA,
        "stage": stage,
        "scope": scope,
        "target_binding": "common" if scope == "common" else target,
        "source_digest": source_digest,
        "config_digest": config_digest,
        "toolchain_digest": toolchain_digest,
        "action_digest": action_digest,
        "dependencies": dependencies,
    }
    return {
        **binding,
        "context_digest": canonical_digest(binding),
        "execution_target": target,
        "source": source,
        "config": config,
        "toolchain": toolchain,
        "actions": action_set,
    }


def write_ref(
    state: pathlib.Path,
    ref_path: pathlib.Path,
    manifest_path: pathlib.Path,
) -> None:
    """Atomically point an active reference at an immutable manifest."""

    manifest_path = manifest_path.resolve()
    payload = {
        "schema": REF_SCHEMA,
        "manifest": relative_to_state(state, manifest_path),
        "sha256": sha256_file(manifest_path),
    }
    atomic_write_json(ref_path, payload)


def write_marker(path: pathlib.Path, manifest_path: pathlib.Path) -> None:
    """Write a compatibility marker bound to a manifest digest."""

    atomic_write_text(
        path,
        f"completed_at_utc={utc_now()}\n"
        f"attestation_sha256={sha256_file(manifest_path)}\n",
    )


def verify_marker(path: pathlib.Path, manifest_path: pathlib.Path) -> None:
    """Require one well-formed compatibility marker for a manifest."""

    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise EvidenceError(f"missing or unreadable PASS marker {path}: {error}")
    expected = f"attestation_sha256={sha256_file(manifest_path)}"
    if (
        len(lines) != 2
        or not lines[0].startswith("completed_at_utc=")
        or not lines[0].removeprefix("completed_at_utc=")
        or lines[1] != expected
    ):
        fail(f"PASS marker is malformed or stale: {path}")


def verify_fingerprint(
    path: pathlib.Path,
    *,
    stage: int,
    context_digest: str,
    expected_target: str | None = None,
) -> None:
    """Require the compatibility input fingerprint to match the attestation."""

    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise EvidenceError(
            f"missing or unreadable stage fingerprint {path}: {error}"
        )
    fields: dict[str, str] = {}
    for line in lines:
        key, separator, value = line.partition("=")
        if not separator or not key or key in fields:
            fail(f"stage fingerprint is malformed: {path}")
        fields[key] = value
    if set(fields) != {
        "stage",
        "target",
        "fingerprint",
        "written_at_utc",
    }:
        fail(f"stage fingerprint has unexpected fields: {path}")
    if fields["stage"] != f"{stage:02d}":
        fail(f"stage fingerprint stage mismatch: {path}")
    if fields["fingerprint"] != context_digest:
        fail(f"stage fingerprint digest mismatch: {path}")
    if expected_target is not None and fields["target"] != expected_target:
        fail(
            f"stage fingerprint target mismatch: {path}: "
            f"recorded={fields['target']} expected={expected_target}"
        )
    if not fields["target"] or not fields["written_at_utc"]:
        fail(f"stage fingerprint has empty required fields: {path}")


def load_ref(state: pathlib.Path, ref_path: pathlib.Path) -> pathlib.Path:
    """Resolve and validate one active attestation reference."""

    payload = load_json(ref_path)
    if payload.get("schema") != REF_SCHEMA:
        fail(f"unsupported attestation reference schema: {ref_path}")
    manifest = confined_path(state, str(payload.get("manifest", "")))
    if not manifest.is_file():
        fail(f"missing referenced attestation manifest: {manifest}")
    actual = sha256_file(manifest)
    if actual != payload.get("sha256"):
        fail(
            f"attestation reference digest mismatch: {manifest} "
            f"recorded={payload.get('sha256')} actual={actual}"
        )
    return manifest


def verify_action(
    manifest_root: pathlib.Path,
    record: dict[str, Any],
    *,
    expected_ordinal: int,
    log_payload: bytes,
    selected_actions: dict[str, dict[str, Any]],
) -> None:
    """Verify one immutable command-result manifest."""

    if not isinstance(record, dict):
        fail("stage action record must be an object")
    action_path = confined_path(manifest_root, str(record.get("path", "")))
    if not action_path.is_file():
        fail(f"missing action evidence: {action_path}")
    if sha256_file(action_path) != record.get("sha256"):
        fail(f"action evidence digest mismatch: {action_path}")
    action = load_json(action_path)
    if action.get("schema") != ACTION_SCHEMA:
        fail(f"unsupported action evidence schema: {action_path}")
    if action.get("ordinal") != expected_ordinal:
        fail(f"action ordinal mismatch: {action_path}")
    if action.get("status") != "pass":
        fail(f"non-passing action in passing stage: {action_path}")
    if action.get("exit_code") != 0:
        fail(f"passing action has a nonzero exit code: {action_path}")
    duration = action.get("duration_ms")
    if (
        not isinstance(duration, int)
        or isinstance(duration, bool)
        or duration < 0
    ):
        fail(f"action duration is invalid: {action_path}")
    log = action.get("log")
    if not isinstance(log, dict):
        fail(f"action log record is invalid: {action_path}")
    start = log.get("start_byte")
    end = log.get("end_byte")
    if (
        not isinstance(start, int)
        or isinstance(start, bool)
        or not isinstance(end, int)
        or isinstance(end, bool)
        or start < 0
        or end < start
        or end > len(log_payload)
    ):
        fail(f"action log byte range is invalid: {action_path}")
    actual_segment = hashlib.sha256(log_payload[start:end]).hexdigest()
    if actual_segment != log.get("sha256"):
        fail(f"action log segment digest mismatch: {action_path}")

    catalog = action.get("catalog")
    if catalog is None:
        return
    if not isinstance(catalog, dict):
        fail(f"action catalog record is invalid: {action_path}")
    action_id = catalog.get("action_id")
    selected = selected_actions.get(str(action_id))
    if selected is None:
        fail(f"action is not in the selected catalog closure: {action_path}")
    if catalog.get("action_digest") != canonical_digest(selected):
        fail(f"catalog action digest mismatch: {action_path}")


def verify_stage(
    root: pathlib.Path,
    state: pathlib.Path,
    stage: int,
    target: str | None,
) -> pathlib.Path:
    """Fail closed unless active evidence is complete, current, and untampered."""

    state = state.resolve()
    if target:
        ref_path = state / f"stage_{stage:02d}.{target}.attestation.json"
    else:
        ref_path = state / f"stage_{stage:02d}.attestation.json"
    if not ref_path.is_file():
        fail(f"missing stage attestation reference: {ref_path}")
    manifest_path = load_ref(state, ref_path)

    if target:
        verify_marker(
            state / f"stage_{stage:02d}.{target}.done",
            manifest_path,
        )
        qualification = load_json(manifest_path)
        if qualification.get("schema") != QUALIFICATION_SCHEMA:
            fail(f"unsupported target qualification schema: {manifest_path}")
        if qualification.get("stage") != stage:
            fail(f"target qualification stage mismatch: {manifest_path}")
        if qualification.get("target") != target:
            fail(f"target qualification target mismatch: {manifest_path}")
        if qualification.get("status") != "pass":
            fail(f"target qualification is not passing: {manifest_path}")
        stage_manifest = confined_path(
            state,
            str(qualification.get("stage_manifest", "")),
        )
        if not stage_manifest.is_file():
            fail(f"missing qualified stage manifest: {stage_manifest}")
        if sha256_file(stage_manifest) != qualification.get(
            "stage_manifest_sha256"
        ):
            fail(f"qualified stage manifest digest mismatch: {stage_manifest}")
        artifacts = qualification.get("required_artifacts")
        if not isinstance(artifacts, list):
            fail(f"target qualification artifact list is invalid: {manifest_path}")
        for artifact in artifacts:
            if not isinstance(artifact, dict):
                fail(f"target qualification artifact is invalid: {manifest_path}")
            if artifact.get("location") == "state":
                artifact_path = confined_path(
                    state,
                    str(artifact.get("path", "")),
                )
            elif artifact.get("location") == "external":
                artifact_path = pathlib.Path(str(artifact.get("path", "")))
                if not artifact_path.is_absolute():
                    fail(
                        "external qualified artifact path is not absolute: "
                        f"{artifact_path}"
                    )
            else:
                fail(f"invalid artifact location in {manifest_path}")
            if tree_digest(artifact_path) != artifact.get("sha256"):
                fail(f"required artifact digest mismatch: {artifact_path}")
        generic_ref = state / f"stage_{stage:02d}.attestation.json"
        active_stage_manifest = load_ref(state, generic_ref)
        if active_stage_manifest != stage_manifest:
            fail(
                "target qualification does not reference the active generic "
                f"stage attestation: {manifest_path}"
            )
        verify_marker(
            state / f"stage_{stage:02d}.done",
            stage_manifest,
        )
        manifest_path = stage_manifest
    else:
        verify_marker(
            state / f"stage_{stage:02d}.done",
            manifest_path,
        )

    manifest = load_json(manifest_path)
    if manifest.get("schema") != STAGE_SCHEMA:
        fail(f"unsupported stage manifest schema: {manifest_path}")
    if manifest.get("stage") != stage:
        fail(f"stage manifest stage mismatch: {manifest_path}")
    if manifest.get("status") != "pass":
        fail(f"stage manifest is not passing: {manifest_path}")
    if manifest.get("iteration") is not False:
        fail(f"iteration evidence cannot qualify as PASS: {manifest_path}")
    if target and manifest.get("scope") == "target":
        if manifest.get("target") != target:
            fail(f"target-bound stage manifest mismatch: {manifest_path}")
    elif manifest.get("scope") not in {"common", "target"}:
        fail(f"invalid stage scope: {manifest_path}")

    manifest_root = manifest_path.parent.resolve()
    log_record = manifest.get("log", {})
    if not isinstance(log_record, dict):
        fail(f"stage log record is invalid: {manifest_path}")
    log_path = confined_path(
        manifest_root,
        str(log_record.get("path", "")),
    )
    if not log_path.is_file():
        fail(f"missing stage log: {log_path}")
    if sha256_file(log_path) != log_record.get("sha256"):
        fail(f"stage log digest mismatch: {log_path}")
    log_payload = log_path.read_bytes()

    inputs = manifest.get("inputs", {})
    if not isinstance(inputs, dict):
        fail(f"stage input record is invalid: {manifest_path}")
    context_record = inputs.get("manifest", {})
    if not isinstance(context_record, dict):
        fail(f"stage input manifest record is invalid: {manifest_path}")
    context_path = confined_path(
        manifest_root,
        str(context_record.get("path", "")),
    )
    if not context_path.is_file():
        fail(f"missing stage input manifest: {context_path}")
    if sha256_file(context_path) != context_record.get("sha256"):
        fail(f"stage input manifest digest mismatch: {context_path}")
    recorded_context = load_json(context_path)
    if recorded_context.get("context_digest") != inputs.get("context_digest"):
        fail(f"stage input digest is internally inconsistent: {manifest_path}")
    for field, value in context_binding(recorded_context).items():
        if inputs.get(field) != value:
            fail(
                f"stage input field {field} is internally inconsistent: "
                f"{manifest_path}"
            )
    manifest_scope = str(manifest.get("scope", ""))
    expected_fingerprint_target = (
        str(manifest.get("target", ""))
        if manifest_scope == "target"
        else None
    )
    verify_fingerprint(
        state / f"stage_{stage:02d}.inputs.sha256",
        stage=stage,
        context_digest=str(inputs.get("context_digest", "")),
        expected_target=expected_fingerprint_target,
    )

    final_inputs = manifest.get("final_inputs")
    if not isinstance(final_inputs, dict):
        fail(f"passing stage has no final input record: {manifest_path}")
    final_record = final_inputs.get("manifest")
    if not isinstance(final_record, dict):
        fail(f"passing stage final input manifest is invalid: {manifest_path}")
    final_path = confined_path(
        manifest_root,
        str(final_record.get("path", "")),
    )
    if not final_path.is_file():
        fail(f"missing final stage input manifest: {final_path}")
    if sha256_file(final_path) != final_record.get("sha256"):
        fail(f"final stage input manifest digest mismatch: {final_path}")
    final_context = load_json(final_path)
    for field, value in context_binding(final_context).items():
        if final_inputs.get(field) != value:
            fail(
                f"final stage input field {field} is inconsistent: "
                f"{manifest_path}"
            )
    if final_context.get("context_digest") != recorded_context.get(
        "context_digest"
    ):
        fail(f"stage inputs changed before PASS publication: {manifest_path}")

    context_actions = recorded_context.get("actions")
    if not isinstance(context_actions, dict):
        fail(f"stage catalog closure is invalid: {context_path}")
    selected_catalog = context_actions.get("selected_catalog_actions")
    if not isinstance(selected_catalog, list):
        fail(f"stage selected catalog actions are invalid: {context_path}")
    selected_actions: dict[str, dict[str, Any]] = {}
    for selected in selected_catalog:
        if not isinstance(selected, dict) or not isinstance(
            selected.get("id"),
            str,
        ):
            fail(f"selected catalog action is invalid: {context_path}")
        action_id = str(selected["id"])
        if action_id in selected_actions:
            fail(f"selected catalog action is duplicated: {action_id}")
        selected_actions[action_id] = selected

    actions = manifest.get("actions")
    if not isinstance(actions, list) or not actions:
        fail(f"passing stage has no action evidence: {manifest_path}")
    seen_action_paths: set[str] = set()
    for ordinal, action in enumerate(actions, start=1):
        if not isinstance(action, dict):
            fail(f"stage action record is invalid: {manifest_path}")
        action_relative = str(action.get("path", ""))
        if action_relative in seen_action_paths:
            fail(f"stage action record is duplicated: {action_relative}")
        seen_action_paths.add(action_relative)
        verify_action(
            manifest_root,
            action,
            expected_ordinal=ordinal,
            log_payload=log_payload,
            selected_actions=selected_actions,
        )

    current_context = capture_context(
        root,
        state,
        stage,
        target or str(manifest.get("target", "unknown")),
    )
    if recorded_context.get("context_digest") != current_context.get(
        "context_digest"
    ):
        fail(
            "stale stage attestation inputs: "
            f"recorded={recorded_context.get('context_digest')} "
            f"current={current_context.get('context_digest')}"
        )
    return manifest_path


_SECRET_NAME = (
    r"[A-Z0-9_-]*(?:TOKEN|SECRET|PASSWORD|TICKET|API[_-]?KEY|"
    r"AUTHORIZATION|CREDENTIAL)[A-Z0-9_-]*"
)
_SECRET_SCALAR = r'(?:"([^"]*)"|\'([^\']*)\'|([^\s;,}]+))'
_SECRET_ASSIGNMENT = re.compile(
    rf"(?i)(?<![A-Z0-9_-])({_SECRET_NAME}\s*=\s*){_SECRET_SCALAR}"
)
_SECRET_FLAG_VALUE = re.compile(
    rf"(?i)(?<![A-Z0-9_-])(--?{_SECRET_NAME}(?:\s*=\s*|\s+))"
    rf"{_SECRET_SCALAR}"
)
_SECRET_COLON_VALUE = re.compile(
    rf"(?i)(?<![A-Z0-9_-])([\"']?{_SECRET_NAME}[\"']?\s*:\s*)"
    rf"{_SECRET_SCALAR}"
)
_SECRET_FLAG_ONLY = re.compile(rf"(?i)^--?{_SECRET_NAME}$")
_AUTHORIZATION_HEADER = re.compile(
    r"(?i)(?<![A-Z0-9_-])(Authorization\s*:\s*)[^\r\n]+"
)


def environment_secret_values() -> set[str]:
    """Return nonempty values selected by secret-shaped environment names."""

    return {
        value
        for key, value in os.environ.items()
        if value and any(term in key.upper() for term in SECRET_TERMS)
    }


def discover_secret_values(text: str) -> set[str]:
    """Extract values paired with secret-shaped assignments and flags."""

    values: set[str] = set()
    for pattern in (
        _SECRET_ASSIGNMENT,
        _SECRET_FLAG_VALUE,
        _SECRET_COLON_VALUE,
    ):
        for match in pattern.finditer(text):
            value = next(
                (
                    candidate
                    for candidate in match.groups()[1:]
                    if candidate is not None
                ),
                "",
            )
            if value and value != "<redacted>":
                values.add(value)
    return values


def redact_text(text: str, known_values: set[str]) -> str:
    """Mask known secrets and values attached to secret-shaped labels."""

    secret_values = known_values | discover_secret_values(text)
    for secret in sorted(secret_values, key=len, reverse=True):
        text = text.replace(secret, "<redacted>")
    text = _AUTHORIZATION_HEADER.sub(r"\1<redacted>", text)
    for pattern in (
        _SECRET_ASSIGNMENT,
        _SECRET_FLAG_VALUE,
        _SECRET_COLON_VALUE,
    ):
        text = pattern.sub(r"\1<redacted>", text)
    return text


def redact_command(arguments: Sequence[str]) -> str:
    """Render command arguments while masking assignments, flags, and values."""

    secret_values = environment_secret_values()
    redact_next = False
    for argument in arguments:
        if redact_next:
            if argument:
                secret_values.add(argument)
            redact_next = False
            continue
        secret_values.update(discover_secret_values(argument))
        if _SECRET_FLAG_ONLY.fullmatch(argument):
            redact_next = True

    redacted: list[str] = []
    redact_next = False
    for original in arguments:
        if redact_next:
            value = "<redacted>"
            redact_next = False
        else:
            value = redact_text(original, secret_values)
            if _SECRET_FLAG_ONLY.fullmatch(original):
                redact_next = True
        redacted.append(value.replace("\r", "\\r").replace("\n", "\\n"))
    return shlex.join(redacted)


def redact_stream() -> None:
    """Copy command output while masking known and label-derived secrets."""

    secret_values = environment_secret_values()
    for line in sys.stdin.buffer:
        text = line.decode("utf-8", errors="surrogateescape")
        redacted = redact_text(text, secret_values)
        sys.stdout.buffer.write(
            redacted.encode("utf-8", errors="surrogateescape")
        )
    sys.stdout.buffer.flush()


def write_action(
    path: pathlib.Path,
    *,
    ordinal: int,
    name: str,
    command: str,
    started_at: str,
    finished_at: str,
    duration_ms: int,
    exit_code: int,
    log_start: int,
    log_end: int,
    log_sha256: str,
    catalog_action_id: str | None,
    catalog_action_digest: str | None,
    catalog_timeout_seconds: int | None,
    catalog_test_policy: str | None,
    catalog_minimum_test_count: int | None,
) -> None:
    """Write one immutable structured command result."""

    payload: dict[str, Any] = {
        "schema": ACTION_SCHEMA,
        "ordinal": ordinal,
        "name": name,
        "command": command,
        "started_at_utc": started_at,
        "finished_at_utc": finished_at,
        "duration_ms": duration_ms,
        "exit_code": exit_code,
        "status": "pass" if exit_code == 0 else "fail",
        "log": {
            "start_byte": log_start,
            "end_byte": log_end,
            "sha256": log_sha256,
        },
    }
    if catalog_action_id:
        catalog: dict[str, Any] = {
            "action_id": catalog_action_id,
            "action_digest": catalog_action_digest,
            "timeout_seconds": catalog_timeout_seconds,
            "test_policy": catalog_test_policy,
        }
        if catalog_minimum_test_count is not None:
            catalog["minimum_test_count"] = catalog_minimum_test_count
        payload["catalog"] = catalog
    if path.exists():
        fail(f"action manifest already exists: {path}")
    atomic_write_json(path, payload)


def context_binding(context: dict[str, Any]) -> dict[str, Any]:
    """Return the small binding subset embedded in a stage manifest."""

    fields = (
        "schema",
        "stage",
        "scope",
        "target_binding",
        "source_digest",
        "config_digest",
        "toolchain_digest",
        "action_digest",
        "dependencies",
        "context_digest",
    )
    return {field: context[field] for field in fields}


def finalize_attempt(
    attempt_dir: pathlib.Path,
    status: str,
    *,
    final_context_path: pathlib.Path | None = None,
) -> pathlib.Path:
    """Write the single terminal manifest for an immutable attempt."""

    attempt_dir = attempt_dir.resolve()
    if final_context_path is not None:
        final_context_path = final_context_path.resolve()
    manifest_path = attempt_dir / "stage.json"
    if manifest_path.exists():
        return manifest_path
    attempt = load_json(attempt_dir / "attempt.json")
    context_path = attempt_dir / str(attempt["context"])
    context = load_json(context_path)
    actions = [
        {
            "path": action_path.relative_to(attempt_dir).as_posix(),
            "sha256": sha256_file(action_path),
        }
        for action_path in sorted((attempt_dir / "actions").glob("*.json"))
    ]
    log_path = attempt_dir / str(attempt["log"])
    payload: dict[str, Any] = {
        "schema": STAGE_SCHEMA,
        "attempt_id": attempt["attempt_id"],
        "stage": attempt["stage"],
        "stage_name": attempt["stage_name"],
        "scope": attempt["scope"],
        "target": attempt["target"],
        "iteration": attempt["iteration"],
        "status": status,
        "started_at_utc": attempt["started_at_utc"],
        "finished_at_utc": utc_now(),
        "duration_ms": max(
            0,
            time.time_ns() // 1_000_000 - attempt["started_at_epoch_ms"],
        ),
        "inputs": {
            **context_binding(context),
            "manifest": {
                "path": context_path.relative_to(attempt_dir).as_posix(),
                "sha256": sha256_file(context_path),
            },
        },
        "actions": actions,
        "log": {
            "path": log_path.relative_to(attempt_dir).as_posix(),
            "sha256": sha256_file(log_path),
        },
    }
    if final_context_path and final_context_path.is_file():
        final_context = load_json(final_context_path)
        payload["final_inputs"] = {
            **context_binding(final_context),
            "manifest": {
                "path": final_context_path.relative_to(
                    attempt_dir
                ).as_posix(),
                "sha256": sha256_file(final_context_path),
            },
        }
    atomic_write_json(manifest_path, payload)
    return manifest_path


def finalize_pending(
    state: pathlib.Path,
    pending_path: pathlib.Path,
    status: str,
) -> pathlib.Path | None:
    """Record an interrupted attempt and clear only its active pending ref."""

    if not pending_path.is_file():
        return None
    pending = load_json(pending_path)
    if pending.get("schema") != PENDING_SCHEMA:
        fail(f"unsupported pending-attempt schema: {pending_path}")
    attempt_dir = confined_path(state, str(pending.get("attempt_dir", "")))
    manifest = finalize_attempt(attempt_dir, status)
    pending_path.unlink(missing_ok=True)
    return manifest


def link_log(link_path: pathlib.Path, target: pathlib.Path) -> None:
    """Atomically update a compatibility log symlink."""

    link_path.parent.mkdir(parents=True, exist_ok=True)
    relative = os.path.relpath(target.resolve(), start=link_path.parent.resolve())
    temporary = link_path.parent / f".{link_path.name}.{os.getpid()}.tmp"
    temporary.unlink(missing_ok=True)
    os.symlink(relative, temporary)
    os.replace(temporary, link_path)


def resolve_state_pointer(
    state: pathlib.Path,
    pointer: pathlib.Path,
) -> pathlib.Path:
    """Resolve a one-line state-relative directory pointer fail closed."""

    try:
        lines = pointer.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise EvidenceError(f"unable to read artifact pointer {pointer}: {error}")
    if len(lines) != 1 or not lines[0].strip():
        fail(f"artifact pointer must contain one non-empty line: {pointer}")
    value = lines[0].strip()
    if value != lines[0]:
        fail(f"artifact pointer contains surrounding whitespace: {pointer}")
    resolved = confined_path(state, value)
    if not resolved.is_dir():
        fail(f"artifact pointer does not resolve to a directory: {resolved}")
    return resolved


def artifact_record(
    state: pathlib.Path,
    value: pathlib.Path,
) -> dict[str, Any]:
    """Bind a required artifact whether inside or outside the state dir."""

    path = value.resolve()
    try:
        recorded = path.relative_to(state.resolve()).as_posix()
    except ValueError:
        location = "external"
        recorded = str(path)
    else:
        location = "state"
    return {
        "location": location,
        "path": recorded,
        "sha256": tree_digest(path),
    }


def qualify_stage(
    root: pathlib.Path,
    state: pathlib.Path,
    stage: int,
    target: str,
    artifacts: Sequence[pathlib.Path],
) -> pathlib.Path:
    """Create target qualification after generic stage and artifact checks."""

    state = state.resolve()
    generic_ref = state / f"stage_{stage:02d}.attestation.json"
    if not generic_ref.is_file():
        fail(f"missing stage attestation reference: {generic_ref}")
    candidate_manifest = load_ref(state, generic_ref)
    candidate_payload = load_json(candidate_manifest)
    if (
        candidate_payload.get("schema") == STAGE_SCHEMA
        and candidate_payload.get("stage") == stage
        and candidate_payload.get("scope") == "target"
        and candidate_payload.get("target") != target
    ):
        fail(
            "cannot qualify target-bound stage evidence for another target: "
            f"recorded={candidate_payload.get('target')} requested={target}"
        )
    stage_manifest = verify_stage(root, state, stage, None)
    qualification_dir = (
        state / "evidence/qualifications" / f"stage-{stage:02d}"
    )
    qualification_dir.mkdir(parents=True, exist_ok=True)
    qualification = qualification_dir / f"{new_id()}.{target}.json"
    payload = {
        "schema": QUALIFICATION_SCHEMA,
        "stage": stage,
        "target": target,
        "status": "pass",
        "qualified_at_utc": utc_now(),
        "stage_manifest": relative_to_state(state, stage_manifest),
        "stage_manifest_sha256": sha256_file(stage_manifest),
        "required_artifacts": [
            artifact_record(state, artifact) for artifact in artifacts
        ],
    }
    with qualification.open("x", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    write_ref(
        state,
        state / f"stage_{stage:02d}.{target}.attestation.json",
        qualification,
    )
    write_marker(
        state / f"stage_{stage:02d}.{target}.done",
        qualification,
    )
    return qualification


def import_common(
    root: pathlib.Path,
    source: pathlib.Path,
    destination: pathlib.Path,
    stage: int,
    target: str,
) -> pathlib.Path:
    """Import content-addressed common evidence after exact current validation."""

    if stage_scope(stage) != "common":
        fail(
            f"stage {stage:02d} is target-bound and cannot be reused "
            "across runs"
        )
    source = source.resolve()
    destination = destination.resolve()
    source_manifest = verify_stage(root, source, stage, None)
    manifest_sha = sha256_file(source_manifest)
    import_dir = (
        destination / "evidence/imports/sha256" / manifest_sha
    )
    imported_manifest = import_dir / "stage.json"
    if import_dir.exists():
        if (
            not imported_manifest.is_file()
            or sha256_file(imported_manifest) != manifest_sha
        ):
            fail(f"incomplete or corrupt imported evidence: {import_dir}")
    else:
        import_dir.parent.mkdir(parents=True, exist_ok=True)
        temporary = pathlib.Path(
            tempfile.mkdtemp(
                dir=import_dir.parent,
                prefix=f".{manifest_sha}.",
            )
        )
        try:
            for child in source_manifest.parent.iterdir():
                copied = temporary / child.name
                if child.is_symlink():
                    os.symlink(os.readlink(child), copied)
                elif child.is_dir():
                    shutil.copytree(child, copied, symlinks=True)
                else:
                    shutil.copy2(child, copied)
            if sha256_file(temporary / "stage.json") != manifest_sha:
                fail("copied stage manifest digest mismatch")
            os.replace(temporary, import_dir)
        finally:
            if temporary.exists():
                shutil.rmtree(temporary)
    manifest = load_json(imported_manifest)
    context_digest = str(
        manifest.get("inputs", {}).get("context_digest", "")
    )
    if not context_digest:
        fail(f"imported stage has no input digest: {imported_manifest}")
    atomic_write_text(
        destination / f"stage_{stage:02d}.inputs.sha256",
        f"stage={stage:02d}\n"
        f"target={target}\n"
        f"fingerprint={context_digest}\n"
        f"written_at_utc={utc_now()}\n",
    )
    destination.mkdir(parents=True, exist_ok=True)
    write_ref(
        destination,
        destination / f"stage_{stage:02d}.attestation.json",
        imported_manifest,
    )
    write_marker(
        destination / f"stage_{stage:02d}.done",
        imported_manifest,
    )
    imported_log = imported_manifest.parent / str(manifest["log"]["path"])
    link_log(
        destination
        / "logs"
        / f"stage-{stage:02d}-{manifest['stage_name']}.log",
        imported_log,
    )
    verify_stage(root, destination, stage, None)
    context_path = confined_path(
        imported_manifest.parent,
        str(manifest["inputs"]["manifest"]["path"]),
    )
    return context_path


def _signal_process_group(process: subprocess.Popen[bytes], value: int) -> bool:
    """Signal a catalog process group, tolerating an exit race."""

    try:
        os.killpg(process.pid, value)
    except ProcessLookupError:
        return False
    return True


def _wait_process(
    process: subprocess.Popen[bytes],
    timeout_seconds: float,
) -> bool:
    """Wait for a catalog process without permitting an unbounded cleanup."""

    try:
        process.wait(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        return False
    return True


def run_catalog_command(
    root: pathlib.Path,
    timeout_seconds: int,
    test_policy: str,
    minimum_test_count: int | None,
    command: str,
) -> int:
    """Execute one catalog command with timeout and test-count enforcement."""

    environment = os.environ.copy()
    environment.pop("BASH_ENV", None)
    environment.pop("ENV", None)
    with tempfile.TemporaryFile(mode="w+b") as output:
        process = subprocess.Popen(
            ["bash", "--noprofile", "--norc", "-c", command],
            cwd=root,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        pump_errors: list[BaseException] = []

        def pump_output() -> None:
            """Stream action output to the caller while retaining a test log."""

            assert process.stdout is not None
            try:
                while True:
                    chunk = process.stdout.read1(64 * 1024)
                    if not chunk:
                        break
                    output.write(chunk)
                    output.flush()
                    sys.stdout.buffer.write(chunk)
                    sys.stdout.buffer.flush()
            except (OSError, BrokenPipeError) as error:
                pump_errors.append(error)

        pump = threading.Thread(
            target=pump_output,
            name="test-plan-output-pump",
            daemon=True,
        )
        pump.start()
        timed_out = False
        try:
            return_code = process.wait(timeout=timeout_seconds)
        except subprocess.TimeoutExpired:
            timed_out = True
            _signal_process_group(process, signal.SIGTERM)
            if not _wait_process(process, 5):
                _signal_process_group(process, signal.SIGKILL)
                if not _wait_process(process, 5):
                    print(
                        "catalog process did not terminate after SIGKILL",
                        file=sys.stderr,
                    )
                    return 125
            return_code = 124
        pump.join(timeout=5)
        if pump.is_alive():
            _signal_process_group(process, signal.SIGKILL)
            _wait_process(process, 5)
            pump.join(timeout=5)
        if pump.is_alive():
            print("catalog output pump did not terminate", file=sys.stderr)
            return 125
        if pump_errors:
            print(
                f"catalog output streaming failed: {pump_errors[0]}",
                file=sys.stderr,
            )
            return 125
        output.seek(0)
        if timed_out:
            print(
                f"catalog action timed out after {timeout_seconds} seconds",
                file=sys.stderr,
            )
            return 124
        if return_code != 0:
            return return_code
        if test_policy == "nonzero":
            observed = 0
            patterns = (
                re.compile(
                    r"test result:\s+ok\.\s+(\d+)\s+passed",
                    flags=re.IGNORECASE,
                ),
                re.compile(
                    r"(?:^|\s)(\d+)\s+passed(?:\s|,|$)",
                    flags=re.IGNORECASE,
                ),
            )
            for raw_line in output:
                line = raw_line.decode("utf-8", errors="replace")
                for pattern in patterns:
                    for value in pattern.findall(line):
                        observed = max(observed, int(value))
            required = minimum_test_count or 1
            if observed < required:
                print(
                    "catalog nonzero-test policy failed: "
                    f"observed={observed} required={required}",
                    file=sys.stderr,
                )
                return 1
        elif test_policy not in {"none", "artifact-count"}:
            print(
                f"unsupported executable catalog test policy: {test_policy}",
                file=sys.stderr,
            )
            return 2
    return 0


def parser() -> argparse.ArgumentParser:
    """Build the command-line interface."""

    result = argparse.ArgumentParser(description=__doc__)
    subparsers = result.add_subparsers(dest="subcommand", required=True)

    subparsers.add_parser("new-id")
    subparsers.add_parser("now-ms")

    sha_parser = subparsers.add_parser("sha256")
    sha_parser.add_argument("--path", type=pathlib.Path, required=True)

    tree_parser = subparsers.add_parser("tree-digest")
    tree_parser.add_argument("--path", type=pathlib.Path, required=True)

    context_parser = subparsers.add_parser("capture-context")
    context_parser.add_argument("--root", type=pathlib.Path, required=True)
    context_parser.add_argument("--state-dir", type=pathlib.Path, required=True)
    context_parser.add_argument("--stage", type=int, required=True)
    context_parser.add_argument("--target", required=True)
    context_parser.add_argument("--output", type=pathlib.Path, required=True)

    field_parser = subparsers.add_parser("field")
    field_parser.add_argument("--path", type=pathlib.Path, required=True)
    field_parser.add_argument("--name", required=True)

    ref_parser = subparsers.add_parser("write-ref")
    ref_parser.add_argument("--state-dir", type=pathlib.Path, required=True)
    ref_parser.add_argument("--ref", type=pathlib.Path, required=True)
    ref_parser.add_argument("--manifest", type=pathlib.Path, required=True)

    marker_parser = subparsers.add_parser("write-marker")
    marker_parser.add_argument("--path", type=pathlib.Path, required=True)
    marker_parser.add_argument("--manifest", type=pathlib.Path, required=True)

    ref_manifest_parser = subparsers.add_parser("ref-manifest")
    ref_manifest_parser.add_argument(
        "--state-dir", type=pathlib.Path, required=True
    )
    ref_manifest_parser.add_argument("--ref", type=pathlib.Path, required=True)

    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--root", type=pathlib.Path, required=True)
    verify_parser.add_argument("--state-dir", type=pathlib.Path, required=True)
    verify_parser.add_argument("--stage", type=int, required=True)
    verify_parser.add_argument("--target")

    redact_parser = subparsers.add_parser("redact")
    redact_parser.add_argument("arguments", nargs=argparse.REMAINDER)
    subparsers.add_parser("redact-stream")

    segment_parser = subparsers.add_parser("log-segment-sha")
    segment_parser.add_argument("--path", type=pathlib.Path, required=True)
    segment_parser.add_argument("--start", type=int, required=True)
    segment_parser.add_argument("--end", type=int, required=True)

    action_parser = subparsers.add_parser("write-action")
    action_parser.add_argument("--path", type=pathlib.Path, required=True)
    action_parser.add_argument("--ordinal", type=int, required=True)
    action_parser.add_argument("--name", required=True)
    action_parser.add_argument("--command", required=True)
    action_parser.add_argument("--started-at", required=True)
    action_parser.add_argument("--finished-at", required=True)
    action_parser.add_argument("--duration-ms", type=int, required=True)
    action_parser.add_argument("--exit-code", type=int, required=True)
    action_parser.add_argument("--log-start", type=int, required=True)
    action_parser.add_argument("--log-end", type=int, required=True)
    action_parser.add_argument("--log-sha256", required=True)
    action_parser.add_argument("--catalog-action-id")
    action_parser.add_argument("--catalog-action-digest")
    action_parser.add_argument("--catalog-timeout-seconds", type=int)
    action_parser.add_argument("--catalog-test-policy")
    action_parser.add_argument("--catalog-minimum-test-count", type=int)

    attempt_parser = subparsers.add_parser("write-attempt")
    attempt_parser.add_argument("--path", type=pathlib.Path, required=True)
    attempt_parser.add_argument("--attempt-id", required=True)
    attempt_parser.add_argument("--stage", type=int, required=True)
    attempt_parser.add_argument("--stage-name", required=True)
    attempt_parser.add_argument("--scope", required=True)
    attempt_parser.add_argument("--target", required=True)
    attempt_parser.add_argument("--iteration", action="store_true")
    attempt_parser.add_argument("--started-at", required=True)
    attempt_parser.add_argument("--started-ms", type=int, required=True)
    attempt_parser.add_argument("--log", required=True)
    attempt_parser.add_argument("--context", required=True)

    pending_parser = subparsers.add_parser("write-pending")
    pending_parser.add_argument("--state-dir", type=pathlib.Path, required=True)
    pending_parser.add_argument("--path", type=pathlib.Path, required=True)
    pending_parser.add_argument("--attempt-dir", type=pathlib.Path, required=True)
    pending_parser.add_argument("--stage", type=int, required=True)
    pending_parser.add_argument("--target", required=True)
    pending_parser.add_argument("--iteration", action="store_true")

    finalize_parser = subparsers.add_parser("finalize-attempt")
    finalize_parser.add_argument(
        "--attempt-dir", type=pathlib.Path, required=True
    )
    finalize_parser.add_argument("--status", required=True)
    finalize_parser.add_argument("--final-context", type=pathlib.Path)

    finalize_pending_parser = subparsers.add_parser("finalize-pending")
    finalize_pending_parser.add_argument(
        "--state-dir", type=pathlib.Path, required=True
    )
    finalize_pending_parser.add_argument(
        "--pending", type=pathlib.Path, required=True
    )
    finalize_pending_parser.add_argument("--status", required=True)

    link_parser = subparsers.add_parser("link-log")
    link_parser.add_argument("--link", type=pathlib.Path, required=True)
    link_parser.add_argument("--target", type=pathlib.Path, required=True)

    pointer_parser = subparsers.add_parser("resolve-state-pointer")
    pointer_parser.add_argument(
        "--state-dir", type=pathlib.Path, required=True
    )
    pointer_parser.add_argument("--pointer", type=pathlib.Path, required=True)

    fingerprint_parser = subparsers.add_parser("write-fingerprint")
    fingerprint_parser.add_argument("--path", type=pathlib.Path, required=True)
    fingerprint_parser.add_argument("--stage", type=int, required=True)
    fingerprint_parser.add_argument("--target", required=True)
    fingerprint_parser.add_argument("--digest", required=True)

    incomplete_parser = subparsers.add_parser("write-incomplete")
    incomplete_parser.add_argument("--active", type=pathlib.Path, required=True)
    incomplete_parser.add_argument("--attempt", type=pathlib.Path, required=True)
    incomplete_parser.add_argument("--timestamp", required=True)
    incomplete_parser.add_argument("--stage-tag", required=True)
    incomplete_parser.add_argument("--stage-name", required=True)
    incomplete_parser.add_argument("--step", required=True)
    incomplete_parser.add_argument("--reason", required=True)
    incomplete_parser.add_argument("--impact", required=True)

    qualify_parser = subparsers.add_parser("qualify")
    qualify_parser.add_argument("--root", type=pathlib.Path, required=True)
    qualify_parser.add_argument("--state-dir", type=pathlib.Path, required=True)
    qualify_parser.add_argument("--stage", type=int, required=True)
    qualify_parser.add_argument("--target", required=True)
    qualify_parser.add_argument(
        "--artifact", type=pathlib.Path, action="append", default=[]
    )

    import_parser = subparsers.add_parser("import-common")
    import_parser.add_argument("--root", type=pathlib.Path, required=True)
    import_parser.add_argument(
        "--source-state", type=pathlib.Path, required=True
    )
    import_parser.add_argument(
        "--destination-state", type=pathlib.Path, required=True
    )
    import_parser.add_argument("--stage", type=int, required=True)
    import_parser.add_argument("--target", required=True)

    catalog_parser = subparsers.add_parser("run-catalog-command")
    catalog_parser.add_argument("--root", type=pathlib.Path, required=True)
    catalog_parser.add_argument("--timeout-seconds", type=int, required=True)
    catalog_parser.add_argument("--test-policy", required=True)
    catalog_parser.add_argument("--minimum-test-count", type=int)
    catalog_parser.add_argument("--command", required=True)
    return result


def nested_field(payload: dict[str, Any], name: str) -> Any:
    """Read a dotted object field."""

    value: Any = payload
    for component in name.split("."):
        if not isinstance(value, dict) or component not in value:
            fail(f"missing field {name}")
        value = value[component]
    return value


def main(argv: Sequence[str] | None = None) -> int:
    """Run the evidence CLI."""

    args = parser().parse_args(argv)
    try:
        if args.subcommand == "new-id":
            print(new_id())
        elif args.subcommand == "now-ms":
            print(time.time_ns() // 1_000_000)
        elif args.subcommand == "sha256":
            print(sha256_file(args.path))
        elif args.subcommand == "tree-digest":
            print(tree_digest(args.path))
        elif args.subcommand == "capture-context":
            context = capture_context(
                args.root,
                args.state_dir,
                args.stage,
                args.target,
            )
            atomic_write_json(args.output, context)
        elif args.subcommand == "field":
            value = nested_field(load_json(args.path), args.name)
            if isinstance(value, (dict, list)):
                print(json.dumps(value, sort_keys=True))
            else:
                print(value)
        elif args.subcommand == "write-ref":
            write_ref(args.state_dir, args.ref, args.manifest)
        elif args.subcommand == "write-marker":
            write_marker(args.path, args.manifest)
        elif args.subcommand == "ref-manifest":
            print(load_ref(args.state_dir, args.ref))
        elif args.subcommand == "verify":
            print(
                verify_stage(
                    args.root,
                    args.state_dir,
                    args.stage,
                    args.target,
                )
            )
        elif args.subcommand == "redact":
            values = args.arguments
            if values and values[0] == "--":
                values = values[1:]
            print(redact_command(values))
        elif args.subcommand == "redact-stream":
            redact_stream()
        elif args.subcommand == "log-segment-sha":
            with args.path.open("rb") as handle:
                handle.seek(args.start)
                payload = handle.read(args.end - args.start)
            print(hashlib.sha256(payload).hexdigest())
        elif args.subcommand == "write-action":
            write_action(
                args.path,
                ordinal=args.ordinal,
                name=args.name,
                command=args.command,
                started_at=args.started_at,
                finished_at=args.finished_at,
                duration_ms=args.duration_ms,
                exit_code=args.exit_code,
                log_start=args.log_start,
                log_end=args.log_end,
                log_sha256=args.log_sha256,
                catalog_action_id=args.catalog_action_id,
                catalog_action_digest=args.catalog_action_digest,
                catalog_timeout_seconds=args.catalog_timeout_seconds,
                catalog_test_policy=args.catalog_test_policy,
                catalog_minimum_test_count=args.catalog_minimum_test_count,
            )
        elif args.subcommand == "write-attempt":
            atomic_write_json(
                args.path,
                {
                    "schema": ATTEMPT_SCHEMA,
                    "attempt_id": args.attempt_id,
                    "stage": args.stage,
                    "stage_name": args.stage_name,
                    "scope": args.scope,
                    "target": args.target,
                    "iteration": args.iteration,
                    "started_at_utc": args.started_at,
                    "started_at_epoch_ms": args.started_ms,
                    "log": args.log,
                    "context": args.context,
                },
            )
        elif args.subcommand == "write-pending":
            atomic_write_json(
                args.path,
                {
                    "schema": PENDING_SCHEMA,
                    "attempt_dir": relative_to_state(
                        args.state_dir,
                        args.attempt_dir,
                    ),
                    "stage": args.stage,
                    "target": args.target,
                    "iteration": args.iteration,
                },
            )
        elif args.subcommand == "finalize-attempt":
            print(
                finalize_attempt(
                    args.attempt_dir,
                    args.status,
                    final_context_path=args.final_context,
                )
            )
        elif args.subcommand == "finalize-pending":
            manifest = finalize_pending(
                args.state_dir,
                args.pending,
                args.status,
            )
            if manifest:
                print(manifest)
        elif args.subcommand == "link-log":
            link_log(args.link, args.target)
        elif args.subcommand == "resolve-state-pointer":
            print(resolve_state_pointer(args.state_dir, args.pointer))
        elif args.subcommand == "write-fingerprint":
            atomic_write_text(
                args.path,
                f"stage={args.stage:02d}\n"
                f"target={args.target}\n"
                f"fingerprint={args.digest}\n"
                f"written_at_utc={utc_now()}\n",
            )
        elif args.subcommand == "write-incomplete":
            content = f"""# INCOMPLETE: {args.step}

- timestamp: {args.timestamp}
- stage: {args.stage_tag} ({args.stage_name})
- step: {args.step}
- reason: {args.reason}
- impact: {args.impact}

Remediation: run the skipped step(s) and re-run this stage in the same state dir.
"""
            atomic_write_text(args.active, content)
            atomic_write_text(args.attempt, content)
        elif args.subcommand == "qualify":
            print(
                qualify_stage(
                    args.root,
                    args.state_dir,
                    args.stage,
                    args.target,
                    args.artifact,
                )
            )
        elif args.subcommand == "import-common":
            print(
                import_common(
                    args.root,
                    args.source_state,
                    args.destination_state,
                    args.stage,
                    args.target,
                )
            )
        elif args.subcommand == "run-catalog-command":
            return run_catalog_command(
                args.root,
                args.timeout_seconds,
                args.test_policy,
                args.minimum_test_count,
                args.command,
            )
    except (EvidenceError, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
