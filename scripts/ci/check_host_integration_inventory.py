#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Validate and execute the compiler-owned Milestone 26e host-integration matrix.
# Copyright 2026 Lukas Bower

"""Strict inventory and evidence runner for host-integration dependencies."""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
from pathlib import Path
import re
import sys
import time
import tomllib
from typing import Any, Iterable, Mapping, Sequence


SOURCE_SCHEMA = "host-integration-dependency-source/v1"
GRAPH_SCHEMA = "host-integration-dependency/v1"
RUN_SCHEMA = "cohesix-host-integration-run/v1"
OBSERVATION_SCHEMA = "cohesix-host-integration-observations/v1"
EVIDENCE_SCHEMA = "cohesix-worker-integration-evidence/v1"
MANDATORY_TARGET_ROWS = (
    "gpu-receipt-path",
    "peft-receipt-path",
    "worker-control",
)
EXPECTED_PLAYBOOKS = (
    "jetson-critical-infra",
    "jetson-manufacturing-safety",
    "jetson-traffic-safety",
    "mac-endpoint-compliance",
    "mac-private-peft-grid",
    "mac-release-factory",
    "mixed-closed-loop-ai-factory",
    "mixed-logistics-digital-twin",
    "mixed-medical-edge-ai",
)
TARGET_SESSION_FIELDS = (
    "cyw43_coexistence_record_sha256",
    "driver_archive_sha256",
    "driver_manifest_sha256",
    "kernel_sha256",
    "manifest_sha256",
    "root_image_sha256",
    "source_sha256",
    "target",
    "worker_abi_sha256",
    "worker_archive_sha256",
    "worker_image_manifest_sha256",
)
HASH_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,127}$")
SECRET_KEY_RE = re.compile(
    r"(?:authorization|bearer|credential|password|private[_-]?key|secret|token)",
    re.IGNORECASE,
)
SECRET_VALUE_RE = re.compile(
    r"(?:authorization\s*:|bearer\s+[A-Za-z0-9._~+/-]+|-----BEGIN [A-Z ]+PRIVATE KEY-----)",
    re.IGNORECASE,
)
ADVERTISED_TERMS = {
    "a2a-gateway": (r"\bA2A\b",),
    "cas-artifact": (r"\bCAS\b", r"\bartifact"),
    "docker-provider": (r"\bDocker\b",),
    "federation-provider": (r"\bfederat", r"multi-hive"),
    "fuse-mount-projection": (r"\bFUSE\b",),
    "general-inference-provider": (r"\binference\b",),
    "general-training-provider": (r"\btraining\b",),
    "gpu-host-provider": (r"\bCUDA\b", r"\bNVML\b"),
    "kubernetes-provider": (r"\bKubernetes\b", r"\bk8s\b"),
    "mcp-gateway": (r"\bMCP\b", r"Model Context Protocol"),
    "nemo-provider": (r"\bNeMo\b",),
    "packaging": (r"\bpackag", r"\brelease bundle"),
    "peft-host-provider": (r"\bPEFT\b", r"\bLoRA\b"),
    "prometheus-otel-export": (r"\bPrometheus\b", r"\bOpenTelemetry\b"),
    "sidecar-provider": (r"\bsidecar\b", r"\bModbus\b", r"\bDNP3\b"),
    "siem-evidence-export": (r"\bSIEM\b",),
    "swarmui-projection": (r"\bSwarmUI\b",),
    "systemd-provider": (r"\bsystemd\b",),
}


class HostIntegrationError(ValueError):
    """Host-integration source, graph, or evidence is invalid."""


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _load_json(path: Path) -> tuple[dict[str, Any], bytes]:
    try:
        raw = path.read_bytes()
        payload = json.loads(raw)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise HostIntegrationError(f"cannot load JSON {path}: {exc}") from exc
    if not isinstance(payload, dict):
        raise HostIntegrationError(f"JSON root must be an object: {path}")
    return payload, raw


def _load_source(path: Path) -> tuple[dict[str, Any], bytes]:
    try:
        raw = path.read_bytes()
        payload = tomllib.loads(raw.decode("utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise HostIntegrationError(f"cannot load TOML {path}: {exc}") from exc
    if payload.get("schema") != SOURCE_SCHEMA:
        raise HostIntegrationError(f"matrix schema must be {SOURCE_SCHEMA}")
    return payload, raw


def _exact_keys(
    value: Mapping[str, Any],
    required: Iterable[str],
    optional: Iterable[str] = (),
    *,
    context: str,
) -> None:
    required_set = set(required)
    allowed = required_set | set(optional)
    missing = sorted(required_set - set(value))
    unexpected = sorted(set(value) - allowed)
    if missing or unexpected:
        raise HostIntegrationError(
            f"{context} field mismatch: missing={missing} unexpected={unexpected}"
        )


def _identifier(value: Any, context: str) -> str:
    if not isinstance(value, str) or ID_RE.fullmatch(value) is None:
        raise HostIntegrationError(f"invalid {context} identifier")
    return value


def _hash(value: Any, context: str) -> str:
    if not isinstance(value, str) or HASH_RE.fullmatch(value) is None:
        raise HostIntegrationError(f"invalid {context} SHA-256")
    return value


def _scan_sensitive(value: Any, *, context: str) -> None:
    def visit(item: Any, key: str | None = None) -> None:
        if key is not None and SECRET_KEY_RE.search(key):
            raise HostIntegrationError(f"{context} contains sensitive material")
        if isinstance(item, dict):
            for child_key, child in item.items():
                visit(child, str(child_key))
        elif isinstance(item, list):
            for child in item:
                visit(child)
        elif isinstance(item, str) and SECRET_VALUE_RE.search(item):
            raise HostIntegrationError(f"{context} contains sensitive material")

    visit(value)


def _validate_dependency_rows(graph: Mapping[str, Any]) -> dict[str, dict[str, Any]]:
    rows = graph.get("dependencies")
    if not isinstance(rows, list) or not rows or len(rows) > 64:
        raise HostIntegrationError("dependency graph must contain 1..64 rows")
    ids = [row.get("id") for row in rows if isinstance(row, dict)]
    if len(ids) != len(rows) or ids != sorted(ids) or len(ids) != len(set(ids)):
        raise HostIntegrationError("dependency ids must be sorted and unique")
    lookup: dict[str, dict[str, Any]] = {}
    allowed_modes = {"unknown", "missing", "disabled", "fixture", "mock", "dry-run", "live"}
    obligations = {"role_required", "release_required", "use_case_required", "optional", "future"}
    for row in rows:
        assert isinstance(row, dict)
        row_id = _identifier(row.get("id"), "dependency")
        if not isinstance(row.get("owner"), str) or not row["owner"].strip():
            raise HostIntegrationError(f"{row_id}: owner is missing")
        _identifier(row.get("owning_milestone"), f"{row_id} owning milestone")
        if row.get("obligation") not in obligations:
            raise HostIntegrationError(f"{row_id}: invalid obligation")
        required_modes = row.get("required_modes")
        row_allowed = row.get("allowed_modes")
        if (
            not isinstance(required_modes, list)
            or not required_modes
            or not isinstance(row_allowed, list)
            or not set(required_modes).issubset(row_allowed)
            or not set(row_allowed).issubset(allowed_modes)
        ):
            raise HostIntegrationError(f"{row_id}: invalid required/allowed modes")
        if row.get("obligation") == "future" and "live" in row_allowed:
            raise HostIntegrationError(f"{row_id}: future provider selected as live")
        if row.get("obligation") == "role_required" and (
            required_modes != ["live"]
            or row.get("mandatory_target_session") is not True
            or row.get("evidence_lane") != "target-session"
        ):
            raise HostIntegrationError(f"{row_id}: role-required row lacks live runtime lane")
        if row.get("obligation") == "release_required" and (
            not row.get("package_requirements") or not row.get("artifact_requirements")
        ):
            raise HostIntegrationError(f"{row_id}: release row lacks package/evidence rule")
        dependencies = row.get("dependencies")
        if not isinstance(dependencies, list):
            raise HostIntegrationError(f"{row_id}: dependencies must be a list")
        lookup[row_id] = row
    if tuple(sorted(set(MANDATORY_TARGET_ROWS).intersection(lookup))) != MANDATORY_TARGET_ROWS:
        raise HostIntegrationError("mandatory target-session rows are missing")
    for row_id, row in lookup.items():
        for dependency in row["dependencies"]:
            if dependency not in lookup:
                raise HostIntegrationError(f"{row_id}: unknown dependency {dependency}")

    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(row_id: str) -> None:
        if row_id in visited:
            return
        if row_id in visiting:
            raise HostIntegrationError(f"circular dependency at {row_id}")
        visiting.add(row_id)
        for dependency in lookup[row_id]["dependencies"]:
            visit(dependency)
        visiting.remove(row_id)
        visited.add(row_id)

    for row_id in lookup:
        visit(row_id)
    return lookup


def validate_graph_payload(graph: Mapping[str, Any]) -> dict[str, dict[str, Any]]:
    """Validate graph structure without consulting repository files."""

    if graph.get("schema") != GRAPH_SCHEMA:
        raise HostIntegrationError(f"graph schema must be {GRAPH_SCHEMA}")
    meta = graph.get("meta")
    if not isinstance(meta, dict):
        raise HostIntegrationError("graph meta is missing")
    for field in (
        "source_sha256",
        "resolved_manifest_sha256",
        "implementation_surface_inventory_sha256",
    ):
        _hash(meta.get(field), f"graph meta {field}")
    rows = _validate_dependency_rows(graph)
    use_cases = graph.get("use_cases")
    playbooks = graph.get("playbooks")
    if not isinstance(use_cases, list) or len(use_cases) != 6:
        raise HostIntegrationError("graph must contain exactly six use cases")
    if not isinstance(playbooks, list) or len(playbooks) != 9:
        raise HostIntegrationError("graph must contain exactly nine playbooks")
    if tuple(item.get("id") for item in playbooks if isinstance(item, dict)) != EXPECTED_PLAYBOOKS:
        raise HostIntegrationError("graph built-in playbook set differs from Python truth")
    for collection, context in ((use_cases, "use case"), (playbooks, "playbook")):
        ids: list[str] = []
        for item in collection:
            if not isinstance(item, dict):
                raise HostIntegrationError(f"{context} row must be an object")
            ids.append(_identifier(item.get("id"), context))
            dependencies = item.get("dependencies")
            if not isinstance(dependencies, list) or not dependencies:
                raise HostIntegrationError(f"{context} {ids[-1]} has no dependencies")
            if any(dependency not in rows for dependency in dependencies):
                raise HostIntegrationError(f"{context} {ids[-1]} has unknown dependency")
        if ids != sorted(ids) or len(ids) != len(set(ids)):
            raise HostIntegrationError(f"{context} ids must be sorted and unique")
    surfaces = graph.get("advertised_surfaces")
    if not isinstance(surfaces, list) or not surfaces:
        raise HostIntegrationError("advertised surface graph is empty")
    surface_ids = [item.get("id") for item in surfaces if isinstance(item, dict)]
    if len(surface_ids) != len(surfaces) or surface_ids != sorted(surface_ids):
        raise HostIntegrationError("advertised surfaces must be sorted")
    if len(surface_ids) != len(set(surface_ids)):
        raise HostIntegrationError("advertised surface ids are duplicated")
    for surface in surfaces:
        if not surface.get("dependencies") or any(
            dependency not in rows for dependency in surface["dependencies"]
        ):
            raise HostIntegrationError(f"surface {surface.get('id')} has invalid dependencies")
    return rows


def _expected_surface_ids(
    source: Mapping[str, Any], inventory: Mapping[str, Any]
) -> set[str]:
    package_paths = source.get("advertised_packages")
    document_paths = source.get("advertised_documents")
    if not isinstance(package_paths, list) or not isinstance(document_paths, list):
        raise HostIntegrationError("source advertised package/document lists are missing")
    expected: set[str] = set()
    packages = inventory.get("packages")
    tracked = inventory.get("tracked_surfaces")
    if not isinstance(packages, list) or not isinstance(tracked, list):
        raise HostIntegrationError("implementation inventory surface lists are missing")
    for path in package_paths:
        matches = [row for row in packages if row.get("path") == path]
        if len(matches) != 1:
            raise HostIntegrationError(f"advertised package must resolve exactly once: {path}")
        expected.add(matches[0]["id"])
        expected.update(target["id"] for target in matches[0].get("targets", []))
    for path in document_paths:
        matches = [row for row in tracked if row.get("path") == path]
        if len(matches) != 1:
            raise HostIntegrationError(f"advertised document/API must resolve exactly once: {path}")
        expected.add(matches[0]["id"])
    return expected


def _discover_playbooks(repo_root: Path) -> tuple[str, ...]:
    source = (repo_root / "tools/cohesix-py/cohesix/playbooks.py").read_text(encoding="utf-8")
    return tuple(sorted(set(re.findall(r'playbook_id="([a-z0-9-]+)"', source))))


def _discover_use_case_titles(repo_root: Path) -> tuple[str, ...]:
    source = (repo_root / "docs/USE_CASES.md").read_text(encoding="utf-8")
    return tuple(sorted(re.findall(r"^### \d+\. (.+)$", source, flags=re.MULTILINE)))


def _scan_advertised_integrations(repo_root: Path, row_ids: set[str]) -> None:
    candidates = [repo_root / "README.md"]
    candidates.extend((repo_root / "docs").glob("*.md"))
    candidates.extend(repo_root.glob("apps/*/README.md"))
    candidates.extend(repo_root.glob("crates/*/README.md"))
    candidates.extend(repo_root.glob("tools/*/README.md"))
    candidates.extend((repo_root / "tools/cohesix-py/examples").glob("*.py"))
    candidates.extend((repo_root / "scripts/cohsh").glob("*.coh"))
    corpus = "\n".join(
        path.read_text(encoding="utf-8", errors="strict")
        for path in sorted(set(candidates))
        if path.is_file()
    )
    for row_id, patterns in ADVERTISED_TERMS.items():
        if any(re.search(pattern, corpus, flags=re.IGNORECASE) for pattern in patterns):
            if row_id not in row_ids:
                raise HostIntegrationError(
                    f"advertised integration is not classified: {row_id}"
                )


def validate_repository(
    repo_root: Path,
    matrix_path: Path,
    graph_path: Path,
    manifest_path: Path,
    inventory_path: Path,
) -> dict[str, Any]:
    """Validate generated graph hashes and exhaustive repository inventory."""

    source, source_raw = _load_source(matrix_path)
    graph, graph_raw = _load_json(graph_path)
    manifest, manifest_raw = _load_json(manifest_path)
    inventory, inventory_raw = _load_json(inventory_path)
    del manifest
    rows = validate_graph_payload(graph)
    meta = graph["meta"]
    expected_hashes = {
        "source_sha256": _sha256(source_raw),
        "resolved_manifest_sha256": _sha256(manifest_raw),
        "implementation_surface_inventory_sha256": _sha256(inventory_raw),
    }
    for field, expected in expected_hashes.items():
        if meta.get(field) != expected:
            raise HostIntegrationError(f"generated graph has stale {field}")
    expected_surface_ids = _expected_surface_ids(source, inventory)
    actual_surface_ids = {row["id"] for row in graph["advertised_surfaces"]}
    if actual_surface_ids != expected_surface_ids:
        missing = sorted(expected_surface_ids - actual_surface_ids)
        stale = sorted(actual_surface_ids - expected_surface_ids)
        raise HostIntegrationError(
            f"advertised host surface inventory drift: missing={missing} stale={stale}"
        )
    if _discover_playbooks(repo_root) != EXPECTED_PLAYBOOKS:
        raise HostIntegrationError("built-in Python playbook inventory drift")
    graph_playbooks = tuple(row["id"] for row in graph["playbooks"])
    if graph_playbooks != EXPECTED_PLAYBOOKS:
        raise HostIntegrationError("generated playbook inventory drift")
    discovered_titles = _discover_use_case_titles(repo_root)
    graph_titles = tuple(sorted(row["title"] for row in graph["use_cases"]))
    if discovered_titles != graph_titles:
        raise HostIntegrationError("six docs/USE_CASES.md scenario titles drifted")
    _scan_advertised_integrations(repo_root, set(rows))
    return {
        "schema": GRAPH_SCHEMA,
        "graph_sha256": _sha256(graph_raw),
        "dependencies": len(rows),
        "advertised_surfaces": len(actual_surface_ids),
        "use_cases": len(graph["use_cases"]),
        "playbooks": len(graph["playbooks"]),
    }


def _validate_outcomes(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list) or not value or len(value) > 128:
        raise HostIntegrationError("outcomes must be a non-empty bounded list")
    values = value
    for outcome in values:
        if not isinstance(outcome, dict):
            raise HostIntegrationError("outcome must be an object")
        _exact_keys(outcome, {"id", "class", "result"}, context="outcome")
        _identifier(outcome["id"], "outcome")
        if outcome["class"] not in ("action", "observation", "receipt"):
            raise HostIntegrationError("invalid outcome class")
        if not isinstance(outcome["result"], str) or not outcome["result"]:
            raise HostIntegrationError("outcome result must be non-empty")
    keys = [(item["id"], item["class"], item["result"]) for item in values]
    if keys != sorted(keys) or len(keys) != len(set(keys)):
        raise HostIntegrationError("outcomes must be sorted and unique")
    return values


def _validate_artifacts(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list) or not value or len(value) > 128:
        raise HostIntegrationError("raw evidence must be a non-empty bounded list")
    values = value
    for artifact in values:
        if not isinstance(artifact, dict):
            raise HostIntegrationError("raw evidence must be an object")
        _exact_keys(artifact, {"id", "sha256", "bytes"}, context="raw evidence")
        _identifier(artifact["id"], "raw evidence")
        _hash(artifact["sha256"], "raw evidence")
        if not isinstance(artifact["bytes"], int) or artifact["bytes"] <= 0:
            raise HostIntegrationError("raw evidence byte count must be positive")
    keys = [(item["id"], item["sha256"], item["bytes"]) for item in values]
    if keys != sorted(keys) or len(keys) != len(set(keys)):
        raise HostIntegrationError("raw evidence must be sorted and unique")
    return values


def _load_target_input(
    path: Path,
    target: str,
    manifest_sha256: str,
    max_age_s: int,
) -> dict[str, Any]:
    payload, _ = _load_json(path)
    _scan_sensitive(payload, context="target session")
    _exact_keys(payload, TARGET_SESSION_FIELDS, context="target_session")
    try:
        age_s = time.time() - path.stat().st_mtime
    except OSError as exc:
        raise HostIntegrationError(f"cannot stat target session: {exc}") from exc
    if age_s < -300 or age_s > max_age_s:
        raise HostIntegrationError("target session is stale")
    if payload.get("target") != target:
        raise HostIntegrationError("target session names wrong target")
    for field in TARGET_SESSION_FIELDS:
        if field != "target":
            _hash(payload[field], f"target_session {field}")
    if payload["manifest_sha256"] != manifest_sha256:
        raise HostIntegrationError("target session identity mismatch: stale manifest")
    return payload


def _load_observations(
    path: Path,
    graph_sha256: str,
    manifest_sha256: str,
) -> dict[str, dict[str, Any]]:
    payload, _ = _load_json(path)
    _scan_sensitive(payload, context="host integration observations")
    _exact_keys(
        payload,
        {"schema", "dependency_graph_sha256", "manifest_sha256", "observations"},
        context="observations",
    )
    if payload["schema"] != OBSERVATION_SCHEMA:
        raise HostIntegrationError(f"observation schema must be {OBSERVATION_SCHEMA}")
    if (
        payload["dependency_graph_sha256"] != graph_sha256
        or payload["manifest_sha256"] != manifest_sha256
    ):
        raise HostIntegrationError("observation identity mismatch")
    observations = payload["observations"]
    if not isinstance(observations, list) or len(observations) > 64:
        raise HostIntegrationError("observations must be a bounded list")
    lookup: dict[str, dict[str, Any]] = {}
    for observation in observations:
        if not isinstance(observation, dict):
            raise HostIntegrationError("observation must be an object")
        _exact_keys(
            observation,
            {"dependency_id", "observed_mode", "outcomes", "raw_evidence"},
            {"artifact_sha256", "provider_version"},
            context="observation",
        )
        row_id = _identifier(observation["dependency_id"], "observation dependency")
        if row_id in lookup:
            raise HostIntegrationError(f"duplicate observation for {row_id}")
        _validate_outcomes(observation["outcomes"])
        _validate_artifacts(observation["raw_evidence"])
        if "artifact_sha256" in observation:
            _hash(observation["artifact_sha256"], "observation artifact")
        if "provider_version" in observation and (
            not isinstance(observation["provider_version"], str)
            or not observation["provider_version"]
            or len(observation["provider_version"]) > 256
        ):
            raise HostIntegrationError("provider version must be non-empty and bounded")
        lookup[row_id] = observation
    return lookup


def _canonical_hash(value: Any) -> str:
    return _sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode())


def _normal_architecture(value: str) -> str:
    return "aarch64" if value.lower() in ("arm64", "aarch64") else value.lower()


def _normal_os(value: str) -> str:
    return "macos" if value.lower() == "darwin" else value.lower()


def _write_json(path: Path, value: Mapping[str, Any]) -> bytes:
    raw = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    return raw


def run_matrix(
    *,
    repo_root: Path,
    matrix_path: Path,
    graph_path: Path,
    manifest_path: Path,
    inventory_path: Path,
    state_dir: Path,
    matrix_only: bool,
    mode: str | None,
    target: str | None,
    target_session_path: Path | None,
    observations_path: Path | None,
    dependencies: Sequence[str],
    use_case: str | None,
    playbook: str | None,
    host_profile: str,
    max_session_age_s: int,
) -> dict[str, Any]:
    """Run one bounded matrix lane and emit hash-only per-row evidence."""

    summary = validate_repository(
        repo_root, matrix_path, graph_path, manifest_path, inventory_path
    )
    graph, graph_raw = _load_json(graph_path)
    rows = validate_graph_payload(graph)
    graph_sha256 = _sha256(graph_raw)
    manifest_sha256 = graph["meta"]["resolved_manifest_sha256"]
    source_sha256 = graph["meta"]["source_sha256"]
    if state_dir.exists() and any(state_dir.iterdir()):
        raise HostIntegrationError(
            f"state directory is non-empty; refuse stale evidence reuse: {state_dir}"
        )
    state_dir.mkdir(parents=True, exist_ok=True)
    if matrix_only:
        record = {
            "schema": RUN_SCHEMA,
            "mode": "matrix-only",
            "dependency_graph_sha256": graph_sha256,
            "manifest_sha256": manifest_sha256,
            "source_sha256": source_sha256,
            "evidence_records": [],
            "verdict": "PASS",
            "blockers": [],
            "inventory": summary,
        }
        _write_json(state_dir / "run.json", record)
        return record

    if mode is None:
        raise HostIntegrationError("non-matrix run requires --mode")
    if use_case is not None and playbook is not None:
        raise HostIntegrationError("select at most one of --use-case and --playbook")
    if mode not in {"unknown", "missing", "disabled", "fixture", "mock", "dry-run", "live"}:
        raise HostIntegrationError(f"invalid observed mode: {mode}")
    profile_lookup = {profile["id"]: profile for profile in graph["host_profiles"]}
    if host_profile not in profile_lookup:
        raise HostIntegrationError(f"unknown host profile: {host_profile}")
    actual_os = _normal_os(platform.system())
    actual_arch = _normal_architecture(platform.machine())
    selected_profile = profile_lookup[host_profile]
    if selected_profile["os"] != actual_os or actual_arch not in selected_profile["architectures"]:
        raise HostIntegrationError(
            f"host profile mismatch: requested={host_profile} actual={actual_os}/{actual_arch}"
        )
    if mode not in selected_profile["allowed_modes"]:
        raise HostIntegrationError(f"mode {mode} is disabled for host profile {host_profile}")

    session: dict[str, Any] | None = None
    promotion: dict[str, str] | None = None
    promotion_dependencies: set[str] = set()
    if use_case is not None:
        matches = [row for row in graph["use_cases"] if row["id"] == use_case]
        if len(matches) != 1:
            raise HostIntegrationError(f"unknown use case: {use_case}")
        promotion = {"kind": "use-case", "id": use_case}
        promotion_dependencies.update(matches[0]["dependencies"])
    elif playbook is not None:
        matches = [row for row in graph["playbooks"] if row["id"] == playbook]
        if len(matches) != 1:
            raise HostIntegrationError(f"unknown playbook: {playbook}")
        promotion = {"kind": "playbook", "id": playbook}
        promotion_dependencies.update(matches[0]["dependencies"])
        linked_use_case = next(
            row for row in graph["use_cases"] if row["id"] == matches[0]["use_case"]
        )
        promotion_dependencies.update(linked_use_case["dependencies"])

    def add_transitive(selected: set[str]) -> set[str]:
        expanded = set(selected)
        pending = list(selected)
        while pending:
            current = pending.pop()
            for dependency in rows[current]["dependencies"]:
                if dependency not in expanded:
                    expanded.add(dependency)
                    pending.append(dependency)
        return expanded

    promotion_dependencies = add_transitive(promotion_dependencies)
    if target is not None:
        if mode != "live" or target_session_path is None:
            raise HostIntegrationError("target-session mode requires live and --target-session")
        session = _load_target_input(
            target_session_path,
            target,
            manifest_sha256,
            max_session_age_s,
        )
        selected = set(MANDATORY_TARGET_ROWS)
        selected.update(promotion_dependencies)
        selected.update(dependencies)
        selected_ids = sorted(add_transitive(selected))
        if (
            dependencies
            and promotion is None
            and tuple(sorted(dependencies)) != MANDATORY_TARGET_ROWS
        ):
            raise HostIntegrationError("target run requires exact mandatory integration rows")
    else:
        if target_session_path is not None:
            raise HostIntegrationError("--target-session requires --target")
        if promotion_dependencies:
            selected_ids = sorted(promotion_dependencies.union(dependencies))
        else:
            selected_ids = (
                sorted(set(dependencies))
                if dependencies
                else [
                    row_id
                    for row_id, row in rows.items()
                    if not row["mandatory_target_session"]
                    and mode in row["allowed_modes"]
                    and row["obligation"] != "future"
                ]
            )
    if not selected_ids:
        raise HostIntegrationError("matrix lane selected no dependency rows")
    if any(row_id not in rows for row_id in selected_ids):
        raise HostIntegrationError("matrix lane selected unknown dependency row")
    observations = (
        _load_observations(observations_path, graph_sha256, manifest_sha256)
        if observations_path is not None
        else {}
    )
    target_ids = {
        row_id for row_id in selected_ids if rows[row_id]["mandatory_target_session"]
    }
    if target_ids and target is None:
        raise HostIntegrationError("selected dependency graph requires a target session")
    expected_observations = set(selected_ids)
    if set(observations) != expected_observations:
        raise HostIntegrationError("observation set must exactly match selected dependency rows")

    evidence_dir = state_dir / "integration"
    evidence_records: list[dict[str, str]] = []
    run_blockers: list[str] = []
    for row_id in selected_ids:
        row = rows[row_id]
        if mode not in row["allowed_modes"]:
            raise HostIntegrationError(f"{row_id}: observed mode is not allowed")
        observation = observations.get(row_id)
        observed_mode = mode if observation is None else observation["observed_mode"]
        if promotion is None and observed_mode != mode:
            raise HostIntegrationError(f"{row_id}: observation mode substitution")
        if observation is not None:
            outcomes = observation["outcomes"]
            raw_evidence = observation["raw_evidence"]
        else:
            raise HostIntegrationError(f"{row_id}: missing observation record")
        blockers: list[str] = []
        if observed_mode not in row["required_modes"]:
            blockers.append(
                f"observed mode {observed_mode} does not satisfy required mode "
                + ",".join(row["required_modes"])
            )
        verdict = "PASS" if not blockers else "FAIL"
        host: dict[str, str] = {
            "profile": host_profile,
            "os": actual_os,
            "architecture": actual_arch,
        }
        if observation is not None and "provider_version" in observation:
            host["provider_version"] = observation["provider_version"]
        record: dict[str, Any] = {
            "schema": EVIDENCE_SCHEMA,
            "record_kind": "worker-integration",
            "dependency_id": row_id,
            "owner_milestone": row["owning_milestone"],
            "obligation": row["obligation"],
            "observed_mode": observed_mode,
            "dependency_graph_sha256": graph_sha256,
            "manifest_sha256": manifest_sha256,
            "component_sha256": _canonical_hash(row),
            "config_sha256": source_sha256,
            "host": host,
            "execution_proof": (
                "qemu"
                if row_id in target_ids and target == "qemu"
                else "fresh-pi"
                if row_id in target_ids and target == "pi4"
                else "host-model"
                if observed_mode in ("fixture", "mock", "dry-run")
                else "none"
            ),
            "outcomes": outcomes,
            "raw_evidence": raw_evidence,
            "verdict": verdict,
            "blockers": blockers,
        }
        if row_id in target_ids and session is not None:
            record["target_session"] = session
        if observation is not None and "artifact_sha256" in observation:
            record["artifact_sha256"] = observation["artifact_sha256"]
        _scan_sensitive(record, context="generated integration evidence")
        raw = _write_json(evidence_dir / f"{row_id}.json", record)
        evidence_records.append(
            {"id": row_id, "sha256": _sha256(raw), "verdict": verdict}
        )
        run_blockers.extend(f"{row_id}: {blocker}" for blocker in blockers)
    run_record = {
        "schema": RUN_SCHEMA,
        "mode": mode,
        "target": target,
        "dependency_graph_sha256": graph_sha256,
        "manifest_sha256": manifest_sha256,
        "source_sha256": source_sha256,
        "evidence_records": evidence_records,
        "verdict": "PASS" if not run_blockers else "FAIL",
        "blockers": sorted(run_blockers),
        "inventory": summary,
        "promotion": promotion,
    }
    _write_json(state_dir / "run.json", run_record)
    if run_blockers:
        raise HostIntegrationError("; ".join(run_blockers))
    return run_record


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument(
        "--matrix",
        type=Path,
        default=Path("configs/host_integration_acceptance.toml"),
    )
    parser.add_argument(
        "--graph",
        type=Path,
        default=Path("configs/generated/host_integration_dependency.json"),
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("configs/generated/root_task_resolved.json"),
    )
    parser.add_argument(
        "--inventory",
        type=Path,
        default=Path("configs/generated/implementation_surface_inventory.json"),
    )
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--matrix-only", action="store_true")
    parser.add_argument("--state-dir", type=Path)
    parser.add_argument(
        "--mode", choices=("unknown", "missing", "disabled", "fixture", "mock", "dry-run", "live")
    )
    parser.add_argument("--target", choices=("qemu", "pi4"))
    parser.add_argument("--target-session", type=Path)
    parser.add_argument("--observations", type=Path)
    parser.add_argument("--dependency", action="append", default=[])
    parser.add_argument("--use-case")
    parser.add_argument("--playbook")
    parser.add_argument("--host-profile", default="macos-arm64")
    parser.add_argument("--max-session-age-s", type=int, default=86400)
    return parser


def _resolve(repo_root: Path, path: Path) -> Path:
    return path if path.is_absolute() else repo_root / path


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    repo_root = args.repo_root.resolve()
    try:
        if args.run or args.matrix_only:
            if args.state_dir is None:
                raise HostIntegrationError("runner requires --state-dir")
            record = run_matrix(
                repo_root=repo_root,
                matrix_path=_resolve(repo_root, args.matrix),
                graph_path=_resolve(repo_root, args.graph),
                manifest_path=_resolve(repo_root, args.manifest),
                inventory_path=_resolve(repo_root, args.inventory),
                state_dir=_resolve(repo_root, args.state_dir),
                matrix_only=args.matrix_only,
                mode=args.mode,
                target=args.target,
                target_session_path=(
                    _resolve(repo_root, args.target_session)
                    if args.target_session is not None
                    else None
                ),
                observations_path=(
                    _resolve(repo_root, args.observations)
                    if args.observations is not None
                    else None
                ),
                dependencies=args.dependency,
                use_case=args.use_case,
                playbook=args.playbook,
                host_profile=args.host_profile,
                max_session_age_s=args.max_session_age_s,
            )
            print(json.dumps(record, sort_keys=True))
        else:
            summary = validate_repository(
                repo_root,
                _resolve(repo_root, args.matrix),
                _resolve(repo_root, args.graph),
                _resolve(repo_root, args.manifest),
                _resolve(repo_root, args.inventory),
            )
            print(json.dumps(summary, sort_keys=True))
    except HostIntegrationError as exc:
        print(f"host-integration: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
