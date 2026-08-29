#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Enforce compiler-owned implementation-surface and runtime-closure truth.
# Copyright 2026 Lukas Bower

"""Validate the generated Milestone 26e implementation-surface inventory."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Iterable


SCHEMA = "cohesix-implementation-surface-inventory/v1"
ALLOWED_CLASSES = {
    "production_live",
    "fixture",
    "host_model",
    "diagnostic",
    "contract",
    "not_enabled",
    "deferred",
    "retired",
    "model_only",
}
PRODUCTION_FORBIDDEN_CLASSES = {
    "fixture",
    "model_only",
    "not_enabled",
    "deferred",
    "retired",
}
REQUIRED_CLASSIFICATION_FIELDS = {
    "implementation_class",
    "owner",
    "milestone",
    "production_reachable",
    "selection_source",
    "package_disposition",
    "evidence_requirement",
    "current_observed_mode",
    "evidence_eligible",
}
PUBLIC_PATH_PREFIXES = ("docs/",)
PUBLIC_PATH_SUFFIXES = ("/README.md",)
HOST_ADAPTER_PREFIXES = (
    "tools/cohesix-py/cohesix/",
    "tools/cohesix-py/examples/",
    "scripts/cohsh/",
)


class SurfaceCheckError(RuntimeError):
    """Raised for invalid or drifting implementation-surface state."""


def _run(repo_root: Path, *args: str) -> str:
    result = subprocess.run(
        args,
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise SurfaceCheckError(f"{' '.join(args)} failed: {detail}")
    return result.stdout


def _classification_records(payload: dict[str, Any]) -> Iterable[dict[str, Any]]:
    for package in payload.get("packages", []):
        yield package
        yield from package.get("targets", [])
        yield from package.get("features", [])
    yield from payload.get("surfaces", [])
    yield from payload.get("tracked_surfaces", [])
    yield from payload.get("release", {}).get("asset_records", [])


def validate_inventory_payload(payload: dict[str, Any]) -> list[str]:
    """Return structural inventory violations without reading the repository."""

    errors: list[str] = []
    if payload.get("schema") != SCHEMA:
        errors.append(f"schema must be {SCHEMA}")
    ids: set[str] = set()
    model_only: list[str] = []
    for record in _classification_records(payload):
        record_id = record.get("id")
        if not isinstance(record_id, str) or not record_id:
            errors.append("surface record has missing/empty id")
            continue
        if record_id in ids:
            errors.append(f"duplicate surface id: {record_id}")
        ids.add(record_id)
        missing = REQUIRED_CLASSIFICATION_FIELDS.difference(record)
        if missing:
            errors.append(f"{record_id}: missing fields {sorted(missing)}")
            continue
        implementation_class = record.get("implementation_class")
        if implementation_class not in ALLOWED_CLASSES:
            errors.append(
                f"{record_id}: unsupported implementation class "
                f"{implementation_class!r}"
            )
        for field in (
            "owner",
            "milestone",
            "selection_source",
            "package_disposition",
            "evidence_requirement",
            "current_observed_mode",
        ):
            value = record.get(field)
            if not isinstance(value, str) or not value.strip():
                errors.append(f"{record_id}: {field} must be non-empty")
        if record.get("production_reachable") and (
            implementation_class in PRODUCTION_FORBIDDEN_CLASSES
        ):
            errors.append(
                f"{record_id}: production-reachable surface is "
                f"{implementation_class}"
            )
        if implementation_class == "model_only":
            model_only.append(str(record.get("path", "")))
        expected_eligible = implementation_class == "production_live"
        if record.get("evidence_eligible") != expected_eligible:
            errors.append(
                f"{record_id}: evidence_eligible must be {expected_eligible} "
                f"for {implementation_class}"
            )
    if not model_only or any("worker-bus" not in path for path in model_only):
        errors.append(
            "WorkerBus must be present and every model_only surface must be WorkerBus"
        )
    return errors


def _tracked_files(repo_root: Path) -> set[str]:
    return {
        line
        for line in _run(
            repo_root,
            "git",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
        ).splitlines()
        if line
    }


def _required_tracked_surface(path: str) -> bool:
    return (
        path == "README.md"
        or (path.startswith(PUBLIC_PATH_PREFIXES) and path.endswith(".md"))
        or path.endswith(PUBLIC_PATH_SUFFIXES)
        or (
            path.startswith(HOST_ADAPTER_PREFIXES)
            and path.endswith((".py", ".coh"))
        )
        or path.startswith("resources/fixtures/")
        or path.startswith("resources/keys/")
        or path.startswith("resources/openapi/")
        or path.startswith("resources/systemd/")
    )


def validate_tracked_coverage(
    repo_root: Path, payload: dict[str, Any]
) -> list[str]:
    """Require exactly one generated row for every tracked public/adapter surface."""

    errors: list[str] = []
    release = payload.get("release", {})
    release_sources = {
        path
        for key in (
            "public_documents",
            "host_assets",
            "operator_scripts",
            "python_artifacts",
            "cas_fixtures",
            "trace_fixtures",
            "transcript_fixtures",
            "ui_assets",
            "support_files",
            "versioned_migrations",
        )
        for path in release.get(key, [])
        if isinstance(path, str)
    }
    tracked = {
        path
        for path in _tracked_files(repo_root)
        if _required_tracked_surface(path) or path in release_sources
    }
    rows = [
        record.get("path")
        for record in payload.get("tracked_surfaces", [])
        if isinstance(record.get("path"), str)
    ]
    row_set = set(rows)
    duplicates = sorted(path for path in row_set if rows.count(path) != 1)
    missing = sorted(tracked.difference(row_set))
    stale = sorted(row_set.difference(tracked))
    if duplicates:
        errors.append(f"duplicate tracked surface rows: {duplicates}")
    if missing:
        errors.append(f"unclassified tracked surfaces: {missing}")
    if stale:
        errors.append(f"stale tracked surface rows: {stale}")
    return errors


def _cargo_metadata(repo_root: Path) -> dict[str, Any]:
    return json.loads(
        _run(
            repo_root,
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
        )
    )


def validate_cargo_coverage(
    repo_root: Path, payload: dict[str, Any]
) -> list[str]:
    """Compare generated package, binary/library-target and feature rows to Cargo."""

    errors: list[str] = []
    metadata = _cargo_metadata(repo_root)
    root = repo_root.resolve()
    actual_packages: dict[str, dict[str, Any]] = {}
    for package in metadata["packages"]:
        manifest = Path(package["manifest_path"]).resolve()
        try:
            relative = manifest.parent.relative_to(root).as_posix()
        except ValueError:
            continue
        actual_packages[relative] = package
    generated_packages = {
        package.get("path"): package for package in payload.get("packages", [])
    }
    if set(actual_packages) != set(generated_packages):
        errors.append(
            "workspace package coverage drift: "
            f"missing={sorted(set(actual_packages).difference(generated_packages))} "
            f"stale={sorted(set(generated_packages).difference(actual_packages))}"
        )
        return errors
    for path, package in actual_packages.items():
        generated = generated_packages[path]
        actual_targets = {
            (kind, target["name"])
            for target in package["targets"]
            for kind in target["kind"]
            if kind in {"lib", "bin"}
        }
        generated_targets = {
            (
                record["path"].split("#", 1)[1].split(":", 1)[0],
                record["path"].rsplit(":", 1)[1],
            )
            for record in generated.get("targets", [])
        }
        if actual_targets != generated_targets:
            errors.append(
                f"{path}: Cargo target drift actual={sorted(actual_targets)} "
                f"generated={sorted(generated_targets)}"
            )
        actual_features = set(package.get("features", {}))
        generated_features = {
            record["path"].rsplit(":", 1)[1]
            for record in generated.get("features", [])
        }
        if actual_features != generated_features:
            errors.append(
                f"{path}: Cargo feature drift "
                f"missing={sorted(actual_features.difference(generated_features))} "
                f"stale={sorted(generated_features.difference(actual_features))}"
            )
    return errors


def _function_body(source: str, function_name: str) -> str | None:
    match = re.search(rf"\bfn\s+{re.escape(function_name)}\b[^{{]*\{{", source)
    if match is None:
        return None
    start = match.end()
    depth = 1
    index = start
    while index < len(source) and depth:
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
        index += 1
    if depth:
        return None
    return source[start : index - 1]


def detect_compiled_spin_stub(source: str, function_name: str) -> bool:
    """Detect an entrypoint whose complete behavior is a spin-only loop."""

    body = _function_body(source, function_name)
    if body is None:
        return False
    stripped = re.sub(r"//[^\n]*|/\*.*?\*/", "", body, flags=re.DOTALL)
    stripped = re.sub(r"\s+", "", stripped)
    return bool(
        re.fullmatch(
            r"loop\{(?:(?:core::hint::)?spin_loop\(\);?)+\}", stripped
        )
    )


def detect_noop_success_provider(source: str) -> list[str]:
    """Find provider/action functions whose entire implementation is `Ok(())`."""

    violations: list[str] = []
    names = re.findall(
        r"\bfn\s+((?:publish|provide|activate|dispatch|reboot|apply)[A-Za-z0-9_]*)\b",
        source,
    )
    for name in names:
        body = _function_body(source, name)
        if body is None:
            continue
        stripped = re.sub(r"//[^\n]*|/\*.*?\*/", "", body, flags=re.DOTALL)
        if re.sub(r"\s+", "", stripped) in {"Ok(())", "returnOk(());"}:
            violations.append(name)
    return violations


def root_ninedoor_gpu_mode_gate_is_strict(source: str) -> bool:
    """Accept only production plus the exact QEMU fixture and LoRA projection gates."""

    body = _function_body(source, "gpu_snapshot_mode_allowed")
    lora_body = _function_body(source, "qemu_lora_export_fixture_allowed")
    if body is None or lora_body is None:
        return False
    compact = re.sub(r"\s+", "", body)
    lora_compact = re.sub(r"\s+", "", lora_body)
    exact_gate = (
        'source_mode=="production"'
        '||(qemu_evidence_gate&&source_mode=="fixture")'
    )
    call_gate = re.sub(r"\s+", "", source)
    return (
        exact_gate in compact
        and 'source_mode=="fixture"&&qemu_evidence_gate&&snapshot_live'
        in lora_compact
        and 'cfg!(all(feature="bootstrap-trace",feature="release-qemu"))'
        in call_gate
        and 'source_mode=="mock"' not in compact
        and 'source_mode=="production"' not in lora_compact
        and "LORA_EXPORT_FIXTURE_ADMISSION" in source
        and "telemetry.cbor" in source
        and "base_model.ref" in source
        and "policy.toml" in source
    )


def validate_source_closure(repo_root: Path, payload: dict[str, Any]) -> list[str]:
    """Inspect selected entrypoints and known authority transitions structurally."""

    errors: list[str] = []
    exact_forbidden = {
        "apps/root-task/src/trace.rs": (
            "TraceSink::Ipc",
            "unwired TraceSink::Ipc alias remains",
        ),
        "apps/root-task/src/userland/mod.rs": (
            "KernelSerialDriver::null()",
            "operational userland can silently select null serial",
        ),
        "apps/root-task/src/uart/pl011.rs": (
            "console_main()",
            "duplicate legacy bootstrap console remains",
        ),
        "apps/gpu-bridge-host/src/main.rs": (
            '"changeme"',
            "GPU bridge retains placeholder live auth",
        ),
    }
    for relative, (needle, detail) in exact_forbidden.items():
        path = repo_root / relative
        if path.is_file() and needle in path.read_text(encoding="utf-8"):
            errors.append(f"{relative}: {detail}")

    gpu_library = repo_root / "apps/gpu-bridge-host/src/lib.rs"
    if gpu_library.is_file():
        text = gpu_library.read_text(encoding="utf-8")
        if re.search(
            r"fn\s+build_model_catalog[^}]+default_model_catalog\(\)",
            text,
            flags=re.DOTALL,
        ):
            errors.append(
                "gpu-bridge-host live registry failure installs fixture model catalog"
            )
        required_gpu_contract = {
            '"gpu-bridge-snapshot/v2"': "versioned GPU snapshot wire schema",
            "source_mode": "source-mode identity",
            "observed_unix_ms": "observation time",
            "ttl_ms": "bounded snapshot TTL",
            "catalog_sha256": "catalog manifest binding",
            "activation_receipt": "active-model receipt binding",
            "base_model_id": "base/adapter compatibility",
            "empty_model_catalog()": "fail-closed empty live catalog",
        }
        for marker, label in required_gpu_contract.items():
            if marker not in text:
                errors.append(f"gpu-bridge-host lacks {label}")

    root_ninedoor = repo_root / "apps/root-task/src/ninedoor.rs"
    if root_ninedoor.is_file():
        text = root_ninedoor.read_text(encoding="utf-8")
        required_target_contract = {
            '"gpu-bridge-snapshot/v2"': "versioned GPU snapshot ingestion",
            "withdraw_expired": "expired provider withdrawal",
            '"unavailable source=none"': "fail-closed initial provider state",
            "gpu_activation_receipt": "activation receipt verification",
            "gpu_catalog_sha256": "catalog manifest verification",
        }
        for marker, label in required_target_contract.items():
            if marker not in text:
                errors.append(f"root NineDoor lacks {label}")
        if not root_ninedoor_gpu_mode_gate_is_strict(text):
            errors.append(
                "root NineDoor lacks production-only plus explicit QEMU "
                "bootstrap-trace fixture-source gate"
            )
        for marker in ("Mock 4090", "Mock 4060", "NVIDIA_GPUS"):
            if marker in text:
                errors.append(
                    f"root NineDoor retains preseeded/fabricated GPU state: {marker}"
                )

    nine_door_manifest = repo_root / "apps/nine-door/Cargo.toml"
    if (repo_root / "apps/nine-door/src/main.rs").exists():
        errors.append("nine-door spinning target binary source remains selected")
    if nine_door_manifest.is_file():
        manifest_text = nine_door_manifest.read_text(encoding="utf-8")
        if "[[bin]]" in manifest_text:
            errors.append("nine-door must remain library-only until Milestone 27b")

    root_manifest = (repo_root / "Cargo.toml").read_text(encoding="utf-8")
    if '"crates/domain-intents"' in root_manifest:
        errors.append("retired all-mock domain-intents remains a workspace member")

    for relative in (
        "configs/root_task.toml",
        "configs/root_task_pi4_uboot_aarch64.toml",
    ):
        manifest_text = (repo_root / relative).read_text(encoding="utf-8")
        if "verification_key_path = \"resources/keys/" not in manifest_text:
            errors.append(
                f"{relative}: operational CAS profile lacks public verification-key selection"
            )
        if "cas_signing_key" in manifest_text or "resources/fixtures/" in manifest_text:
            errors.append(
                f"{relative}: operational profile selects fixture/private CAS key material"
            )

    for closure in payload.get("runtime_closures", []):
        for entrypoint in closure.get("selected_entrypoints", []):
            package_name, _, function_name = entrypoint.partition(":")
            package = next(
                (
                    record
                    for record in payload.get("packages", [])
                    if record.get("id") == f"workspace:{package_name}"
                ),
                None,
            )
            if package is None:
                errors.append(
                    f"{closure.get('name')}: selected entrypoint package "
                    f"{package_name} is not inventoried"
                )
                continue
            source_candidates = sorted((repo_root / package["path"] / "src").glob("*.rs"))
            found = False
            for candidate in source_candidates:
                source = candidate.read_text(encoding="utf-8")
                if _function_body(source, function_name) is not None:
                    found = True
                    if detect_compiled_spin_stub(source, function_name):
                        errors.append(
                            f"{candidate.relative_to(repo_root)}:{function_name} "
                            "is a compiled spin-only entrypoint"
                        )
                    break
            if not found:
                errors.append(
                    f"{closure.get('name')}: selected entrypoint {entrypoint} not found"
                )

    provider_files = (
        repo_root / "apps/gpu-bridge-host/src/lib.rs",
        repo_root / "apps/host-ticket-agent/src/lib.rs",
        repo_root / "apps/console-network-runtime/src/lib.rs",
    )
    for path in provider_files:
        if not path.is_file():
            continue
        violations = detect_noop_success_provider(path.read_text(encoding="utf-8"))
        if violations:
            errors.append(
                f"{path.relative_to(repo_root)}: no-op success providers {violations}"
            )
    return errors


def validate_release_manifest(payload: dict[str, Any]) -> list[str]:
    """Check exact release selection and exclusion semantics."""

    errors: list[str] = []
    release = payload.get("release", {})
    if release.get("schema") != "cohesix-runtime-release-manifest/v2":
        errors.append("release schema must be cohesix-runtime-release-manifest/v2")
    version = release.get("version")
    if not isinstance(version, str) or not version:
        errors.append("release.version must be non-empty")
        version = ""
    host_keys = (
        "host_tools",
        "target_images",
        "generated_configs",
        "public_documents",
        "host_assets",
        "operator_scripts",
        "python_artifacts",
        "cas_fixtures",
        "trace_fixtures",
        "transcript_fixtures",
        "ui_assets",
        "support_files",
        "generated_bundle_files",
    )
    host_paths: list[str] = []
    for key in host_keys:
        values = release.get(key)
        if not isinstance(values, list) or not values:
            errors.append(f"release.{key} must be a non-empty exact path list")
            continue
        host_paths.extend(str(value) for value in values)
    for key in ("pi4_stage_files", "pi4_generated_bundle_files"):
        values = release.get(key)
        if not isinstance(values, list) or not values:
            errors.append(f"release.{key} must be a non-empty exact path list")
            continue
        if len(values) != len(set(values)):
            errors.append(f"release.{key} contains duplicate paths")
    migrations = release.get("versioned_migrations")
    if not isinstance(migrations, list):
        errors.append("release.versioned_migrations must be an exact path list")
        migrations = []
    host_paths.extend(str(value) for value in migrations)
    if len(host_paths) != len(set(host_paths)):
        errors.append("release manifest contains duplicate selected paths")
    forbidden = set(release.get("forbidden_paths", []))
    required_forbidden = {
        "bin/coh-status",
        "bin/nine-door",
        "crates/domain-intents",
        "resources/fixtures/cas_signing_key.hex",
    }
    if not required_forbidden.issubset(forbidden):
        errors.append(
            "release manifest missing forbidden paths: "
            f"{sorted(required_forbidden.difference(forbidden))}"
        )
    overlap = sorted(set(host_paths).intersection(forbidden))
    if overlap:
        errors.append(f"release selects forbidden paths: {overlap}")
    expected_notes = f"releases/RELEASE_NOTES-{version}.md"
    if expected_notes not in release.get("support_files", []):
        errors.append(f"release must select version-bound notes: {expected_notes}")

    def destination(kind: str, source: str) -> str:
        if kind == "public_documents" and source == "docs/QUICKSTART.md":
            return "QUICKSTART.md"
        if kind == "python_artifacts":
            return "python/cohesix-py/" + source.removeprefix(
                "tools/cohesix-py/"
            )
        if kind == "cas_fixtures":
            return "cas/" + source.removeprefix("tests/fixtures/cas/")
        if kind == "trace_fixtures":
            return "traces/" + source.removeprefix("tests/fixtures/traces/")
        if kind == "transcript_fixtures":
            return "tests/fixtures/transcripts/" + source.removeprefix(
                "tests/fixtures/transcripts/"
            )
        if kind == "ui_assets":
            return "ui/swarmui/" + source.removeprefix(
                "apps/swarmui/frontend/"
            )
        if kind == "support_files" and source == expected_notes:
            return "RELEASE_NOTES.md"
        return source

    expected_files: list[str] = []
    for key in (
        "host_tools",
        "target_images",
        "generated_configs",
        "public_documents",
        "host_assets",
        "operator_scripts",
        "python_artifacts",
        "cas_fixtures",
        "trace_fixtures",
        "transcript_fixtures",
        "ui_assets",
        "support_files",
        "versioned_migrations",
        "generated_bundle_files",
    ):
        expected_files.extend(
            destination(key, str(source)) for source in release.get(key, [])
        )
    if len(expected_files) != len(set(expected_files)):
        errors.append("release bundle destinations are not unique")
    if release.get("expected_bundle_files") != sorted(expected_files):
        errors.append("release.expected_bundle_files drift from selected sources")
    expected_pi4_files = [
        destination("public_documents", str(source))
        for source in release.get("public_documents", [])
    ]
    expected_pi4_files.extend(
        destination("support_files", str(source))
        for source in release.get("support_files", [])
        if source in ("LICENSE.txt", expected_notes)
    )
    expected_pi4_files.extend(
        str(path) for path in release.get("pi4_generated_bundle_files", [])
    )
    if len(expected_pi4_files) != len(set(expected_pi4_files)):
        errors.append("Pi 4 release bundle destinations are not unique")
    if release.get("expected_pi4_bundle_files") != sorted(expected_pi4_files):
        errors.append(
            "release.expected_pi4_bundle_files drift from selected sources"
        )
    record_paths = sorted(
        str(record.get("path")) for record in release.get("asset_records", [])
    )
    if record_paths != sorted(set(expected_files).union(expected_pi4_files)):
        errors.append("release asset classification rows drift from exact bundle files")
    return errors


def validate_source_hash(
    repo_root: Path, payload: dict[str, Any]
) -> list[str]:
    source_path = repo_root / "configs/implementation_surfaces.toml"
    if not source_path.is_file():
        return [f"missing compiler source: {source_path}"]
    observed = hashlib.sha256(source_path.read_bytes()).hexdigest()
    if payload.get("source_sha256") != observed:
        return [
            "implementation-surface source hash drift: "
            f"generated={payload.get('source_sha256')} observed={observed}"
        ]
    return []


def check(repo_root: Path, inventory_path: Path) -> list[str]:
    """Run the complete implementation-surface guard."""

    try:
        payload = json.loads(inventory_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return [f"cannot load inventory {inventory_path}: {exc}"]
    errors = validate_inventory_payload(payload)
    errors.extend(validate_source_hash(repo_root, payload))
    try:
        errors.extend(validate_tracked_coverage(repo_root, payload))
        errors.extend(validate_cargo_coverage(repo_root, payload))
    except SurfaceCheckError as exc:
        errors.append(str(exc))
    errors.extend(validate_source_closure(repo_root, payload))
    errors.extend(validate_release_manifest(payload))
    return errors


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    repo_root = args.repo_root.resolve()
    inventory = args.inventory
    if not inventory.is_absolute():
        inventory = repo_root / inventory
    errors = check(repo_root, inventory)
    if errors:
        for error in errors:
            print(f"implementation-surface: FAIL: {error}", file=sys.stderr)
        return 1
    print(
        "implementation-surface: PASS: compiler inventory, Cargo closure, "
        "source selection, public claims, and exact release manifest agree"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
