#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Configure, build, and validate pinned seL4 profile contracts without hand-editing generated trees.
# Copyright 2026 Lukas Bower

"""Manage source-pinned seL4 build profiles for Cohesix."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tomllib
from typing import Any, Iterable, Mapping, Sequence


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONTRACT = ROOT / "configs" / "sel4" / "profiles.toml"
WRAPPER_PROJECT = ROOT / "tools" / "sel4-profile-project"
TRACKED_SEL4_ROOT = ROOT / "seL4"
REPO_MANAGED_PROFILE_BUILDS = {
    "pi4_diagnostic": TRACKED_SEL4_ROOT / "build_UBOOT",
}
GIC_DETECTOR = ROOT / "scripts" / "lib" / "detect_gic_version.py"
VALIDATOR = Path(__file__).resolve()
WRAPPER_CMAKE = WRAPPER_PROJECT / "CMakeLists.txt"
BUILD_INPUT_STAMP_NAME = "cohesix-profile-build-inputs.json"
SHA256_RE = re.compile(r"[0-9a-f]{64}")
GIT_COMMIT_RE = re.compile(r"[0-9a-f]{40}")


class ProfileError(RuntimeError):
    """Report invalid profile input or failed conformance."""


def sha256_bytes(data: bytes) -> str:
    """Return a lowercase SHA-256 digest for *data*."""

    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    """Return a lowercase SHA-256 digest for a file."""

    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_sha256(value: Any) -> str:
    """Hash a JSON-compatible value in a deterministic representation."""

    encoded = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return sha256_bytes(encoded)


def file_evidence(path: Path) -> dict[str, Any]:
    """Record immutable identity for one local file."""

    resolved = path.expanduser().resolve()
    record: dict[str, Any] = {
        "path": str(resolved),
        "exists": resolved.is_file(),
    }
    if resolved.is_file():
        record.update(
            {
                "size": resolved.stat().st_size,
                "sha256": sha256_file(resolved),
            }
        )
    return record


def cohesix_repository_evidence() -> tuple[dict[str, Any], list[str]]:
    """Bind evidence to the current Cohesix commit and dirty state."""

    errors: list[str] = []
    evidence: dict[str, Any] = {"root": str(ROOT)}
    try:
        commit = git_output(ROOT, "rev-parse", "HEAD")
        status = git_output(
            ROOT,
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
        )
    except ProfileError as exc:
        errors.append(f"cannot bind evidence to the Cohesix repository: {exc}")
        evidence["errors"] = list(errors)
        return evidence, errors
    evidence.update(
        {
            "commit": commit,
            "dirty": bool(status),
            "status_sha256": sha256_bytes(status.encode("utf-8")),
            "status_entry_count": len([line for line in status.splitlines() if line]),
        }
    )
    return evidence, errors


def require_contract_string(
    table: Mapping[str, Any],
    key: str,
    label: str,
) -> str:
    """Return one required non-empty contract string."""

    value = table.get(key)
    if not isinstance(value, str) or not value:
        raise ProfileError(f"profile contract {label}.{key} is missing")
    return value


def require_contract_sha256(
    table: Mapping[str, Any],
    key: str,
    label: str,
) -> str:
    """Return one required lowercase SHA-256 contract value."""

    value = require_contract_string(table, key, label)
    if SHA256_RE.fullmatch(value) is None:
        raise ProfileError(
            f"profile contract {label}.{key} must be 64 lowercase hex digits"
        )
    return value


def contract_repo_path(
    value: Any,
    label: str,
    *,
    allow_resolved_outside: bool = False,
) -> Path:
    """Resolve a fail-closed repository-relative contract path."""

    if not isinstance(value, str) or not value or Path(value).is_absolute():
        raise ProfileError(
            f"profile contract {label} must be a non-empty repository-relative path"
        )
    lexical = Path(os.path.abspath(ROOT / value))
    if not is_relative_to(lexical, ROOT):
        raise ProfileError(f"profile contract {label} escapes the repository")
    resolved = lexical.resolve()
    if not allow_resolved_outside and not is_relative_to(resolved, ROOT):
        raise ProfileError(f"profile contract {label} resolves outside the repository")
    return resolved


def canonical_distribution_name(value: str) -> str:
    """Normalize one Python distribution name using PEP 503 rules."""

    return re.sub(r"[-_.]+", "-", value).lower()


def parse_python_lock(path: Path) -> dict[str, dict[str, Any]]:
    """Parse an exact hashed-requirements lock without accepting loose options."""

    requirements: dict[str, dict[str, Any]] = {}
    current: dict[str, Any] | None = None
    for line_number, raw_line in enumerate(
        path.read_text(encoding="utf-8", errors="strict").splitlines(),
        start=1,
    ):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        requirement = re.fullmatch(
            r"([A-Za-z0-9_.-]+)==([^\s\\]+)\s*\\",
            line,
        )
        if requirement is not None:
            if current is not None and not current["hashes"]:
                raise ProfileError(
                    f"Python lock requirement lacks a hash in {path}:{line_number}"
                )
            name = canonical_distribution_name(requirement.group(1))
            if name in requirements:
                raise ProfileError(f"duplicate Python lock requirement {name!r}: {path}")
            current = {
                "name": name,
                "version": requirement.group(2),
                "hashes": [],
            }
            requirements[name] = current
            continue
        digest = re.fullmatch(r"--hash=sha256:([0-9a-f]{64})", line)
        if digest is not None and current is not None:
            current["hashes"].append(digest.group(1))
            continue
        raise ProfileError(f"unsupported Python lock syntax in {path}:{line_number}")
    if current is not None and not current["hashes"]:
        raise ProfileError(f"Python lock requirement lacks a hash at EOF: {path}")
    if not requirements:
        raise ProfileError(f"Python lock contains no requirements: {path}")
    return {
        name: {
            "version": record["version"],
            "hashes": sorted(set(record["hashes"])),
        }
        for name, record in sorted(requirements.items())
    }


def validate_python_lock_contract(
    python_contract: Mapping[str, Any],
) -> dict[str, dict[str, Any]]:
    """Validate both Python lock files, digests, count, and named version pins."""

    combined: dict[str, dict[str, Any]] = {}
    for field in ("bootstrap_lock", "requirements_lock"):
        lock_path = contract_repo_path(
            python_contract.get(field),
            f"toolchain.python.{field}",
        )
        if not lock_path.is_file():
            raise ProfileError(f"profile Python lock is missing: {lock_path}")
        digest_field = f"{field}_sha256"
        expected_digest = require_contract_sha256(
            python_contract,
            digest_field,
            "toolchain.python",
        )
        actual_digest = sha256_file(lock_path)
        if actual_digest != expected_digest:
            raise ProfileError(
                f"profile Python lock digest mismatch for {lock_path}: expected "
                f"{expected_digest}, got {actual_digest}"
            )
        for name, record in parse_python_lock(lock_path).items():
            if name in combined:
                raise ProfileError(
                    f"Python distribution {name!r} is duplicated across lock files"
                )
            combined[name] = record
    expected_count = python_contract.get("distribution_count")
    if (
        not isinstance(expected_count, int)
        or isinstance(expected_count, bool)
        or expected_count < 1
    ):
        raise ProfileError(
            "profile contract toolchain.python.distribution_count must be positive"
        )
    if len(combined) != expected_count:
        raise ProfileError(
            "profile Python lock distribution count mismatch: expected "
            f"{expected_count}, got {len(combined)}"
        )
    named_versions = {
        "sel4-deps": python_contract.get("sel4_deps_version"),
        "protobuf": python_contract.get("protobuf_version"),
        "setuptools": python_contract.get("setuptools_version"),
    }
    for name, expected_version in named_versions.items():
        observed = combined.get(name, {}).get("version")
        if observed != expected_version:
            raise ProfileError(
                f"profile Python {name} pin mismatch: expected "
                f"{expected_version!r}, lock has {observed!r}"
            )
    return dict(sorted(combined.items()))


def load_contract(path: Path = DEFAULT_CONTRACT) -> dict[str, Any]:
    """Load and fail-closed validate the profile and supply-chain contract."""

    try:
        with path.open("rb") as stream:
            contract = tomllib.load(stream)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise ProfileError(f"cannot load profile contract {path}: {exc}") from exc

    if contract.get("schema_version") != 2:
        raise ProfileError(
            f"unsupported seL4 profile schema in {path}: "
            f"{contract.get('schema_version')!r}"
        )
    source = contract.get("source")
    toolchain = contract.get("toolchain")
    profiles = contract.get("profiles")
    if (
        not isinstance(source, dict)
        or not isinstance(toolchain, dict)
        or not isinstance(profiles, dict)
    ):
        raise ProfileError(
            f"profile contract {path} is missing source/toolchain/profiles tables"
        )
    for key in ("family", "cross_prefix", "target_triple", "version"):
        require_contract_string(toolchain, key, "toolchain")

    compiler_contract = toolchain.get("compiler")
    if not isinstance(compiler_contract, dict):
        raise ProfileError("profile contract toolchain.compiler table is missing")
    for key in (
        "provider",
        "source_url",
        "source_archive",
        "source_version",
        "install_path",
        "bin_path",
        "provenance_path",
    ):
        require_contract_string(compiler_contract, key, "toolchain.compiler")
    if compiler_contract["provider"] != "arm-gnu-toolchain-release-tarball":
        raise ProfileError("unsupported toolchain.compiler.provider")
    for key in (
        "source_archive_sha256",
        "gcc_sha256",
        "gxx_sha256",
        "cpp_sha256",
        "as_sha256",
        "ld_sha256",
        "objcopy_sha256",
        "ar_sha256",
        "ranlib_sha256",
    ):
        require_contract_sha256(compiler_contract, key, "toolchain.compiler")
    archive_size = compiler_contract.get("source_archive_size")
    if not isinstance(archive_size, int) or isinstance(archive_size, bool) or archive_size < 1:
        raise ProfileError(
            "profile contract toolchain.compiler.source_archive_size must be positive"
        )
    if not compiler_contract["source_url"].startswith("https://"):
        raise ProfileError("profile contract toolchain.compiler.source_url must use HTTPS")
    compiler_paths = {
        key: contract_repo_path(
            compiler_contract[key],
            f"toolchain.compiler.{key}",
        )
        for key in ("source_archive", "install_path", "bin_path", "provenance_path")
    }
    if compiler_paths["bin_path"] != compiler_paths["install_path"] / "bin":
        raise ProfileError("toolchain.compiler.bin_path is not inside install_path")
    if compiler_paths["provenance_path"] != (
        compiler_paths["install_path"] / "cohesix-compiler-provenance.json"
    ):
        raise ProfileError("toolchain.compiler.provenance_path is not canonical")
    path_prefixes = compiler_contract.get("path_prefixes")
    expected_prefixes = [str(Path(str(compiler_contract["bin_path"])))]
    if path_prefixes != expected_prefixes:
        raise ProfileError(
            "toolchain.compiler.path_prefixes must exactly bind the archive bin"
        )
    cross_prefix = str(toolchain["cross_prefix"])
    required_programs = compiler_contract.get("required_programs")
    if required_programs != [
        f"{cross_prefix}gcc",
        f"{cross_prefix}g++",
        f"{cross_prefix}cpp",
        f"{cross_prefix}as",
        f"{cross_prefix}ld",
        f"{cross_prefix}objcopy",
        f"{cross_prefix}ar",
        f"{cross_prefix}ranlib",
    ]:
        raise ProfileError("toolchain.compiler.required_programs is not the exact closure")

    cpio_contract = toolchain.get("cpio")
    if not isinstance(cpio_contract, dict):
        raise ProfileError("profile contract toolchain.cpio table is missing")
    for key in ("path", "provider", "formula", "version"):
        require_contract_string(cpio_contract, key, "toolchain.cpio")
    require_contract_sha256(cpio_contract, "sha256", "toolchain.cpio")
    if cpio_contract["provider"] != "homebrew":
        raise ProfileError("unsupported toolchain.cpio.provider")
    if cpio_contract["formula"] != "cpio":
        raise ProfileError("toolchain.cpio.formula must be cpio")
    if re.fullmatch(r"[0-9]+(?:\.[0-9]+)+", cpio_contract["version"]) is None:
        raise ProfileError("toolchain.cpio.version is invalid")
    cpio_path = Path(cpio_contract["path"])
    expected_cpio_path = (
        Path("/opt/homebrew/Cellar")
        / cpio_contract["formula"]
        / cpio_contract["version"]
        / "bin"
        / "cpio"
    )
    if not cpio_path.is_absolute() or cpio_path != expected_cpio_path:
        raise ProfileError(
            "toolchain.cpio.path must bind the exact Apple Silicon Homebrew "
            f"Cellar binary: {expected_cpio_path}"
        )
    required_cpio_options = cpio_contract.get("required_options")
    if required_cpio_options != [
        "--append",
        "--owner",
        "--quiet",
        "--format",
        "--file",
        "--reproducible",
    ]:
        raise ProfileError(
            "toolchain.cpio.required_options must exactly declare the upstream "
            "seL4 archive option closure"
        )

    python_contract = toolchain.get("python")
    if not isinstance(python_contract, dict):
        raise ProfileError("profile contract toolchain.python table is missing")
    for key in (
        "path",
        "provider",
        "formula",
        "implementation",
        "major_minor_version",
        "bootstrap_lock",
        "requirements_lock",
        "sel4_deps_version",
        "protobuf_version",
        "setuptools_version",
    ):
        require_contract_string(python_contract, key, "toolchain.python")
    contract_repo_path(
        python_contract["path"],
        "toolchain.python.path",
        allow_resolved_outside=True,
    )
    if python_contract["provider"] != "homebrew":
        raise ProfileError("unsupported toolchain.python.provider")
    if python_contract["implementation"] != "CPython":
        raise ProfileError("toolchain.python.implementation must be CPython")
    if re.fullmatch(r"[0-9]+\.[0-9]+", python_contract["major_minor_version"]) is None:
        raise ProfileError("toolchain.python.major_minor_version is invalid")
    validate_python_lock_contract(python_contract)

    mkimage_contract = toolchain.get("mkimage")
    if not isinstance(mkimage_contract, dict):
        raise ProfileError("profile contract toolchain.mkimage table is missing")
    for key in (
        "path",
        "provider",
        "source_url",
        "source_archive",
        "source_version",
        "source_commit",
        "snapshot_path",
        "build_path",
        "provenance_path",
        "version",
    ):
        require_contract_string(mkimage_contract, key, "toolchain.mkimage")
    if mkimage_contract["provider"] != "denx-release-tarball":
        raise ProfileError("unsupported toolchain.mkimage.provider")
    if not mkimage_contract["source_url"].startswith("https://"):
        raise ProfileError("toolchain.mkimage.source_url must use HTTPS")
    require_contract_sha256(
        mkimage_contract,
        "source_archive_sha256",
        "toolchain.mkimage",
    )
    source_archive_size = mkimage_contract.get("source_archive_size")
    source_date_epoch = mkimage_contract.get("source_date_epoch")
    if (
        not isinstance(source_archive_size, int)
        or isinstance(source_archive_size, bool)
        or source_archive_size < 1
    ):
        raise ProfileError("toolchain.mkimage.source_archive_size must be positive")
    if (
        not isinstance(source_date_epoch, int)
        or isinstance(source_date_epoch, bool)
        or source_date_epoch < 1
    ):
        raise ProfileError("toolchain.mkimage.source_date_epoch must be positive")
    if GIT_COMMIT_RE.fullmatch(mkimage_contract["source_commit"]) is None:
        raise ProfileError(
            "profile contract toolchain.mkimage.source_commit must be 40 lowercase hex digits"
        )
    mkimage_paths = {
        key: contract_repo_path(
            mkimage_contract[key],
            f"toolchain.mkimage.{key}",
        )
        for key in (
            "path",
            "source_archive",
            "snapshot_path",
            "build_path",
            "provenance_path",
        )
    }
    if mkimage_paths["path"] != mkimage_paths["build_path"] / "tools" / "mkimage":
        raise ProfileError("toolchain.mkimage.path is not inside build_path/tools")
    if mkimage_paths["provenance_path"] != (
        mkimage_paths["build_path"] / "cohesix-mkimage-provenance.json"
    ):
        raise ProfileError("toolchain.mkimage.provenance_path is not canonical")
    if not profiles:
        raise ProfileError(f"profile contract {path} defines no profiles")
    evidence_class_eligibility = {
        "production": (True, True),
        "diagnostic": (False, True),
        "proof-eligibility": (False, False),
    }
    for name, profile in profiles.items():
        if not isinstance(profile, dict):
            raise ProfileError(f"seL4 profile {name!r} is not a table")
        evidence_class = profile.get("evidence_class")
        if evidence_class not in evidence_class_eligibility:
            allowed = ", ".join(sorted(evidence_class_eligibility))
            raise ProfileError(
                f"seL4 profile {name!r} has unsupported evidence_class "
                f"{evidence_class!r}; expected one of {allowed}"
            )
        release_eligible = profile.get("release_eligible")
        runtime_eligible = profile.get("runtime_eligible")
        if not isinstance(release_eligible, bool) or not isinstance(
            runtime_eligible, bool
        ):
            raise ProfileError(
                f"seL4 profile {name!r} release/runtime eligibility must be boolean"
            )
        expected_eligibility = evidence_class_eligibility[evidence_class]
        observed_eligibility = (release_eligible, runtime_eligible)
        if observed_eligibility != expected_eligibility:
            raise ProfileError(
                f"seL4 profile {name!r} {evidence_class!r} evidence must set "
                "(release_eligible, runtime_eligible)="
                f"{expected_eligibility!r}, got {observed_eligibility!r}"
            )
        cmake = profile.get("cmake")
        if not isinstance(cmake, dict):
            raise ProfileError(f"seL4 profile {name!r} has no CMake contract")
        if cmake.get("CROSS_COMPILER_PREFIX") != toolchain["cross_prefix"]:
            raise ProfileError(
                f"seL4 profile {name!r} compiler prefix does not match toolchain pin"
            )
        minimum_archive = profile.get("minimum_elfloader_archive_bytes")
        reserve_bytes = cmake.get("COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES")
        if profile.get("target") == "qemu" and release_eligible:
            if (
                not isinstance(minimum_archive, int)
                or isinstance(minimum_archive, bool)
                or minimum_archive < 1
            ):
                raise ProfileError(
                    f"release QEMU profile {name!r} must declare a positive "
                    "minimum_elfloader_archive_bytes"
                )
            if (
                not isinstance(reserve_bytes, str)
                or re.fullmatch(r"[0-9]+", reserve_bytes) is None
                or int(reserve_bytes, 10) < 1
            ):
                raise ProfileError(
                    f"release QEMU profile {name!r} must reserve positive "
                    "COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES"
                )
        elif minimum_archive is not None or reserve_bytes is not None:
            raise ProfileError(
                f"profile {name!r} cannot declare an elfloader archive reserve "
                "without release-QEMU capacity policy"
            )
        if profile.get("memoization_cache") != "disabled":
            raise ProfileError(
                f"seL4 profile {name!r} must disable the unbound memoization cache"
            )
        python_tool = profile.get("python_tool")
        if not isinstance(python_tool, str) or not python_tool:
            raise ProfileError(f"seL4 profile {name!r} has no python_tool")
        if python_tool != python_contract["path"]:
            raise ProfileError(
                f"seL4 profile {name!r} does not use toolchain.python.path"
            )
        if cmake.get("ElfloaderImage") == "uimage":
            for tool_field in ("objcopy_stdout_wrapper", "mkimage_tool"):
                tool_value = profile.get(tool_field)
                if not isinstance(tool_value, str) or not tool_value:
                    raise ProfileError(
                        f"uImage seL4 profile {name!r} has no {tool_field}"
                    )
            if profile.get("mkimage_tool") != mkimage_contract["path"]:
                raise ProfileError(
                    f"uImage seL4 profile {name!r} does not use toolchain.mkimage.path"
                )
            if Path(str(profile["mkimage_tool"])).name != "mkimage":
                raise ProfileError(
                    f"uImage seL4 profile {name!r} mkimage_tool must be named mkimage"
                )
        if not isinstance(profile.get("artifact_policy"), dict):
            raise ProfileError(f"seL4 profile {name!r} has no artifact policy")
        if not isinstance(profile.get("dtb"), dict):
            raise ProfileError(f"seL4 profile {name!r} has no DTB semantic policy")
    return contract


def canonical_profile_name(name: str) -> str:
    """Accept command-line profile names with hyphens or underscores."""

    return name.strip().replace("-", "_")


def get_profile(contract: Mapping[str, Any], name: str) -> tuple[str, dict[str, Any]]:
    """Resolve one named profile from a loaded contract."""

    canonical = canonical_profile_name(name)
    profiles = contract.get("profiles")
    if not isinstance(profiles, dict) or canonical not in profiles:
        available = ", ".join(sorted(profiles or {}))
        raise ProfileError(f"unknown seL4 profile {name!r}; available: {available}")
    profile = profiles[canonical]
    if not isinstance(profile, dict):
        raise ProfileError(f"seL4 profile {canonical!r} is not a table")
    return canonical, profile


def parse_cmake_cache(path: Path) -> dict[str, str]:
    """Parse values from a CMakeCache.txt file."""

    try:
        text = path.read_text(encoding="utf-8", errors="strict")
    except OSError as exc:
        raise ProfileError(f"cannot read CMake cache {path}: {exc}") from exc

    result: dict[str, str] = {}
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith(("#", "//")) or "=" not in line:
            continue
        left, value = line.split("=", 1)
        if ":" not in left:
            continue
        key, _value_type = left.split(":", 1)
        if key:
            result[key] = value
    return result


def find_generated_config(build_dir: Path) -> Path:
    """Find the generated seL4 kernel JSON configuration."""

    candidates = (
        build_dir / "kernel" / "gen_config" / "kernel" / "gen_config.json",
        build_dir / "gen_config" / "kernel" / "gen_config.json",
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    joined = ", ".join(str(candidate) for candidate in candidates)
    raise ProfileError(f"generated kernel configuration not found; tried: {joined}")


def load_generated_config(path: Path) -> dict[str, Any]:
    """Load a generated seL4 JSON configuration."""

    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ProfileError(f"cannot load generated config {path}: {exc}") from exc
    if not isinstance(data, dict):
        raise ProfileError(f"generated config {path} is not a JSON object")
    return data


def expected_matches(actual: Any, expected: Any) -> bool:
    """Compare cache/JSON values without weakening boolean or numeric truth."""

    if isinstance(expected, bool):
        if isinstance(actual, bool):
            return actual is expected
        if isinstance(actual, str):
            lowered = actual.strip().lower()
            if lowered in {"on", "true", "yes", "y", "1"}:
                return expected is True
            if lowered in {"off", "false", "no", "n", "0"}:
                return expected is False
        return False
    return str(actual) == str(expected)


def is_relative_to(path: Path, parent: Path) -> bool:
    """Return whether a resolved path is within *parent*."""

    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True


def pi4_overlay_patch(
    contract: Mapping[str, Any],
) -> tuple[dict[str, Any], Path, bytes]:
    """Load and authenticate the source-controlled Pi overlay patch."""

    source = contract.get("source")
    if not isinstance(source, dict):
        raise ProfileError("profile contract source table is invalid")
    overlay = source.get("pi4_overlay")
    if not isinstance(overlay, dict):
        raise ProfileError("profile contract has no Pi overlay table")
    overlay_rel = overlay.get("path")
    if (
        not isinstance(overlay_rel, str)
        or not overlay_rel.startswith("kernel/")
        or Path(overlay_rel).is_absolute()
        or ".." in Path(overlay_rel).parts
    ):
        raise ProfileError(
            "Pi overlay path must be a kernel-relative repository path"
        )
    if overlay.get("diff_format") != "git-diff-binary-full-index-v1":
        raise ProfileError(
            f"unsupported Pi overlay diff format: {overlay.get('diff_format')!r}"
        )
    expected_sha256 = overlay.get("diff_sha256")
    if (
        not isinstance(expected_sha256, str)
        or re.fullmatch(r"[0-9a-f]{64}", expected_sha256) is None
    ):
        raise ProfileError("Pi overlay diff_sha256 must be 64 lowercase hex digits")
    patch_relative = overlay.get("patch_file")
    if (
        not isinstance(patch_relative, str)
        or not patch_relative
        or Path(patch_relative).is_absolute()
    ):
        raise ProfileError("Pi overlay patch_file must be a repository-relative path")
    lexical = Path(os.path.abspath(ROOT / patch_relative))
    resolved = lexical.resolve()
    if not is_relative_to(lexical, ROOT.resolve()) or not is_relative_to(
        resolved,
        ROOT.resolve(),
    ):
        raise ProfileError(
            f"Pi overlay patch_file escapes the Cohesix repository: {patch_relative}"
        )
    try:
        patch_bytes = resolved.read_bytes()
    except OSError as exc:
        raise ProfileError(f"cannot read Pi overlay patch {resolved}: {exc}") from exc
    actual_sha256 = sha256_bytes(patch_bytes)
    if actual_sha256 != expected_sha256:
        raise ProfileError(
            "Pi overlay patch digest mismatch: "
            f"expected {expected_sha256}, got {actual_sha256}"
        )
    path_in_kernel = overlay_rel.removeprefix("kernel/")
    expected_header = (
        f"diff --git a/{path_in_kernel} b/{path_in_kernel}\n".encode("utf-8")
    )
    if not patch_bytes.startswith(expected_header) or patch_bytes.count(
        b"\ndiff --git "
    ):
        raise ProfileError(
            "Pi overlay patch must contain exactly the declared kernel file diff"
        )
    return overlay, resolved, patch_bytes


def resolve_profile_tool(
    profile: Mapping[str, Any],
    field: str,
    *,
    preserve_symlink: bool = False,
) -> Path | None:
    """Resolve one optional repository-relative executable profile input."""

    relative = profile.get(field)
    if relative is None:
        return None
    if not isinstance(relative, str) or not relative:
        raise ProfileError(f"profile {field} must be a non-empty relative path")
    lexical = Path(os.path.abspath(ROOT / relative))
    if Path(relative).is_absolute() or not is_relative_to(lexical, ROOT):
        raise ProfileError(f"profile {field} escapes the Cohesix repository: {relative}")
    candidate = lexical if preserve_symlink else lexical.resolve()
    if not candidate.is_file():
        raise ProfileError(f"profile {field} is missing: {candidate}")
    if not os.access(candidate, os.X_OK):
        raise ProfileError(f"profile {field} is not executable: {candidate}")
    return candidate


PYTHON_ENVIRONMENT_PROBE = r"""
import hashlib
import importlib.metadata
import json
from pathlib import Path
import platform
import re
import sys

import google.protobuf
import jsonschema
import libarchive
import lxml
import yaml


def canonical(value):
    return re.sub(r"[-_.]+", "-", value).lower()


def digest(value):
    encoded = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


prefix = Path(sys.prefix).resolve()
distributions = {}
content = {}
for distribution in importlib.metadata.distributions():
    name = canonical(distribution.metadata["Name"])
    if name in distributions:
        raise SystemExit(f"duplicate installed Python distribution: {name}")
    version = distribution.version
    distributions[name] = version
    files = []
    for installed in distribution.files or ():
        if installed.suffix == ".pyc" or "__pycache__" in installed.parts:
            continue
        candidate = Path(distribution.locate_file(installed)).resolve()
        try:
            relative = candidate.relative_to(prefix)
        except ValueError as exc:
            raise SystemExit(
                f"installed Python file escapes the isolated environment: {candidate}"
            ) from exc
        if not candidate.is_file():
            raise SystemExit(f"installed Python file is missing: {candidate}")
        payload = candidate.read_bytes()
        files.append(
            {
                "path": relative.as_posix(),
                "size": len(payload),
                "sha256": hashlib.sha256(payload).hexdigest(),
            }
        )
    files.sort(key=lambda record: record["path"])
    content[name] = {
        "version": version,
        "file_count": len(files),
        "sha256": digest(files),
    }
content = dict(sorted(content.items()))
record = {
    "schema": "cohesix-python-environment/v1",
    "implementation": platform.python_implementation(),
    "major_minor_version": f"{sys.version_info.major}.{sys.version_info.minor}",
    "version": platform.python_version(),
    "executable": str(Path(sys.executable).resolve()),
    "prefix": str(prefix),
    "distributions": dict(sorted(distributions.items())),
    "installed_content": {
        "algorithm": "sha256-canonical-installed-files-v1",
        "distributions": content,
        "sha256": digest(content),
    },
}
print(json.dumps(record, separators=(",", ":"), sort_keys=True))
"""


def executable_identity(path: Path | None) -> dict[str, Any] | None:
    """Bind one executable launcher and its resolved file content."""

    if path is None:
        return None
    resolved = path.resolve()
    return {
        "path": str(path),
        "resolved_path": str(resolved),
        "sha256": sha256_file(resolved),
    }


def gnu_cpio_supply_chain_input(
    contract: Mapping[str, Any],
) -> dict[str, Any]:
    """Validate the pinned GNU cpio required by upstream seL4 archive rules."""

    toolchain = contract.get("toolchain")
    declared = toolchain.get("cpio") if isinstance(toolchain, dict) else None
    if not isinstance(declared, dict):
        raise ProfileError("profile contract toolchain.cpio table is invalid")
    path_value = declared.get("path")
    if not isinstance(path_value, str) or not path_value:
        raise ProfileError("profile contract toolchain.cpio.path is invalid")
    tool = Path(path_value).expanduser()
    if not tool.is_absolute():
        tool = Path(os.path.abspath(ROOT / tool))
    if tool.name != "cpio" or not tool.is_file() or not os.access(tool, os.X_OK):
        raise ProfileError(f"pinned GNU cpio is missing or not executable: {tool}")
    identity = executable_identity(tool)
    expected_sha256 = declared.get("sha256")
    if identity is None or identity["sha256"] != expected_sha256:
        observed = identity.get("sha256") if identity is not None else None
        raise ProfileError(
            "pinned GNU cpio binary digest mismatch: expected "
            f"{expected_sha256}, got {observed}"
        )
    version = run_checked((str(tool), "--version")).stdout.splitlines()
    observed_version = version[0].strip() if version else ""
    expected_version = f"cpio (GNU cpio) {declared.get('version')}"
    if observed_version != expected_version:
        raise ProfileError(
            "pinned GNU cpio version mismatch: expected "
            f"{expected_version!r}, got {observed_version!r}"
        )
    help_result = run_checked((str(tool), "--help"))
    help_text = f"{help_result.stdout}\n{help_result.stderr}"
    required_options = declared.get("required_options")
    if not isinstance(required_options, list) or not all(
        isinstance(option, str) and option for option in required_options
    ):
        raise ProfileError("profile contract toolchain.cpio.required_options is invalid")
    missing_options = [
        option for option in required_options if option not in help_text
    ]
    if missing_options:
        raise ProfileError(
            "pinned GNU cpio lacks upstream seL4 archive options: "
            + ", ".join(missing_options)
        )
    return {
        "schema": "cohesix-gnu-cpio-host-input/v1",
        "declared": dict(declared),
        "provider": declared.get("provider"),
        "formula": declared.get("formula"),
        "version": observed_version,
        "required_options": list(required_options),
        "executable": identity,
    }


def python_supply_chain_inputs(
    contract: Mapping[str, Any],
    profile: Mapping[str, Any],
) -> dict[str, Any]:
    """Validate and bind the exact locked Python environment and its contents."""

    toolchain = contract.get("toolchain")
    python_contract = toolchain.get("python") if isinstance(toolchain, dict) else None
    if not isinstance(python_contract, dict):
        raise ProfileError("profile contract toolchain.python table is invalid")
    locked = validate_python_lock_contract(python_contract)
    lock_files: dict[str, Any] = {}
    for field in ("bootstrap_lock", "requirements_lock"):
        path = contract_repo_path(
            python_contract.get(field),
            f"toolchain.python.{field}",
        )
        lock_files[field] = {
            **file_evidence(path),
            "expected_sha256": python_contract[f"{field}_sha256"],
            "requirements": parse_python_lock(path),
        }

    python_tool = resolve_profile_tool(
        profile,
        "python_tool",
        preserve_symlink=True,
    )
    if python_tool is None:
        raise ProfileError("profile has no bound Python tool")
    identity = executable_identity(python_tool)
    probe_output = run_checked(
        (str(python_tool), "-c", PYTHON_ENVIRONMENT_PROBE)
    ).stdout.strip()
    try:
        environment = json.loads(probe_output)
    except json.JSONDecodeError as exc:
        raise ProfileError(
            "profile Python environment returned invalid identity JSON"
        ) from exc
    if not isinstance(environment, dict):
        raise ProfileError("profile Python environment identity is not an object")
    if environment.get("schema") != "cohesix-python-environment/v1":
        raise ProfileError("profile Python environment identity schema mismatch")
    for key in ("implementation", "major_minor_version"):
        if environment.get(key) != python_contract.get(key):
            raise ProfileError(
                f"profile Python {key} mismatch: expected "
                f"{python_contract.get(key)!r}, got {environment.get(key)!r}"
            )
    executable = environment.get("executable")
    prefix = environment.get("prefix")
    expected_prefix = python_tool.parent.parent.resolve()
    if not isinstance(executable, str) or Path(executable).resolve() != python_tool.resolve():
        raise ProfileError("profile Python executable escaped the bound environment")
    if not isinstance(prefix, str) or Path(prefix).resolve() != expected_prefix:
        raise ProfileError("profile Python prefix does not match the isolated environment")
    expected_versions = {
        name: str(record["version"]) for name, record in locked.items()
    }
    observed_versions = environment.get("distributions")
    if observed_versions != expected_versions:
        raise ProfileError(
            "profile Python distribution mismatch: expected "
            f"{expected_versions!r}, got {observed_versions!r}"
        )
    installed = environment.get("installed_content")
    if not isinstance(installed, dict) or installed.get("algorithm") != (
        "sha256-canonical-installed-files-v1"
    ):
        raise ProfileError("profile Python installed-content identity is invalid")
    distribution_content = installed.get("distributions")
    if not isinstance(distribution_content, dict) or set(distribution_content) != set(
        expected_versions
    ):
        raise ProfileError(
            "profile Python installed-content distribution closure mismatch"
        )
    for name, expected_version in expected_versions.items():
        record = distribution_content.get(name)
        if not isinstance(record, dict):
            raise ProfileError(f"profile Python content identity is missing {name}")
        if record.get("version") != expected_version:
            raise ProfileError(
                f"profile Python content version mismatch for {name}"
            )
        file_count = record.get("file_count")
        digest = record.get("sha256")
        if (
            not isinstance(file_count, int)
            or isinstance(file_count, bool)
            or file_count < 1
            or not isinstance(digest, str)
            or SHA256_RE.fullmatch(digest) is None
        ):
            raise ProfileError(
                f"profile Python installed-file identity is invalid for {name}"
            )
    expected_content_digest = canonical_sha256(distribution_content)
    if installed.get("sha256") != expected_content_digest:
        raise ProfileError("profile Python installed-content digest mismatch")
    if identity is None:
        raise ProfileError("profile Python executable identity is missing")
    identity.update(
        {
            "contract": dict(python_contract),
            "lock_files": lock_files,
            "environment": environment,
        }
    )
    return identity


def load_json_object(path: Path, label: str) -> dict[str, Any]:
    """Load one required JSON object with a typed error."""

    try:
        value = json.loads(path.read_text(encoding="utf-8", errors="strict"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ProfileError(f"cannot load {label} {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ProfileError(f"{label} is not a JSON object: {path}")
    return value


def compiler_supply_chain_inputs(contract: Mapping[str, Any]) -> dict[str, Any]:
    """Validate the Arm release archive, extracted tools, and provenance receipt."""

    toolchain = contract.get("toolchain")
    compiler = toolchain.get("compiler") if isinstance(toolchain, dict) else None
    if not isinstance(toolchain, dict) or not isinstance(compiler, dict):
        raise ProfileError("profile contract compiler supply chain is invalid")
    archive = contract_repo_path(
        compiler.get("source_archive"),
        "toolchain.compiler.source_archive",
    )
    if not archive.is_file():
        raise ProfileError(f"pinned compiler source archive is missing: {archive}")
    archive_record = file_evidence(archive)
    if (
        archive_record.get("sha256") != compiler.get("source_archive_sha256")
        or archive_record.get("size") != compiler.get("source_archive_size")
    ):
        raise ProfileError("pinned compiler source archive identity mismatch")
    install_path = contract_repo_path(
        compiler.get("install_path"),
        "toolchain.compiler.install_path",
    )
    bin_path = contract_repo_path(
        compiler.get("bin_path"),
        "toolchain.compiler.bin_path",
    )
    if not install_path.is_dir() or not bin_path.is_dir():
        raise ProfileError("pinned compiler installation or bin directory is missing")

    hash_fields = {
        "gcc": "gcc_sha256",
        "g++": "gxx_sha256",
        "cpp": "cpp_sha256",
        "as": "as_sha256",
        "ld": "ld_sha256",
        "objcopy": "objcopy_sha256",
        "ar": "ar_sha256",
        "ranlib": "ranlib_sha256",
    }
    programs: dict[str, Any] = {}
    cross_prefix = str(toolchain["cross_prefix"])
    for suffix, hash_field in hash_fields.items():
        name = f"{cross_prefix}{suffix}"
        program = bin_path / name
        if not program.is_file() or not os.access(program, os.X_OK):
            raise ProfileError(f"pinned compiler program is missing: {program}")
        record = executable_identity(program)
        expected_hash = compiler[hash_field]
        if record is None or record["sha256"] != expected_hash:
            observed = record.get("sha256") if record is not None else None
            raise ProfileError(
                f"pinned compiler binary digest mismatch for {name}: expected "
                f"{expected_hash}, got {observed}"
            )
        record["expected_sha256"] = expected_hash
        programs[name] = record

    gcc = bin_path / f"{toolchain['cross_prefix']}gcc"
    live_version = run_checked((str(gcc), "-dumpfullversion")).stdout.strip()
    live_target = run_checked((str(gcc), "-dumpmachine")).stdout.strip()
    if live_version != toolchain.get("version") or live_target != toolchain.get(
        "target_triple"
    ):
        raise ProfileError(
            "pinned compiler live version or target does not match the contract"
        )

    provenance_path = contract_repo_path(
        compiler.get("provenance_path"),
        "toolchain.compiler.provenance_path",
    )
    provenance = load_json_object(provenance_path, "compiler provenance")
    program_sha256 = {
        suffix: compiler[hash_field] for suffix, hash_field in hash_fields.items()
    }
    expected_provenance = {
        "schema": "cohesix-compiler-provenance/v1",
        "source": {
            "provider": compiler["provider"],
            "url": compiler["source_url"],
            "archive_path": str(archive.resolve()),
            "archive_sha256": compiler["source_archive_sha256"],
            "archive_size": compiler["source_archive_size"],
            "release": compiler["source_version"],
        },
        "compiler": {
            "version": toolchain["version"],
            "target": toolchain["target_triple"],
            "bin_path": str(bin_path.resolve()),
            "program_sha256": program_sha256,
        },
        "setup_script_sha256": sha256_file(ROOT / "toolchain" / "setup_macos_arm64.sh"),
        "profile_contract_sha256": sha256_file(DEFAULT_CONTRACT),
    }
    if provenance != expected_provenance:
        raise ProfileError("compiler provenance does not match current declared inputs")
    return {
        "schema": "cohesix-compiler-supply-chain/v1",
        "declared": dict(compiler),
        "source_archive": archive_record,
        "path_prefixes": [str(bin_path)],
        "programs": programs,
        "identity": {
            "version": live_version,
            "target_triple": live_target,
        },
        "provenance": {
            **file_evidence(provenance_path),
            "record": provenance,
        },
    }


def mkimage_supply_chain_inputs(
    contract: Mapping[str, Any],
    profile: Mapping[str, Any],
) -> dict[str, Any] | None:
    """Validate DENX archive and generated mkimage provenance for a Pi profile."""

    mkimage_tool = resolve_profile_tool(profile, "mkimage_tool")
    if mkimage_tool is None:
        return None
    toolchain = contract.get("toolchain")
    declared = toolchain.get("mkimage") if isinstance(toolchain, dict) else None
    if not isinstance(declared, dict):
        raise ProfileError("profile contract toolchain.mkimage table is invalid")
    archive = contract_repo_path(
        declared.get("source_archive"),
        "toolchain.mkimage.source_archive",
    )
    if not archive.is_file():
        raise ProfileError(f"pinned U-Boot source archive is missing: {archive}")
    archive_record = file_evidence(archive)
    if (
        archive_record.get("sha256") != declared.get("source_archive_sha256")
        or archive_record.get("size") != declared.get("source_archive_size")
    ):
        raise ProfileError("pinned U-Boot source archive identity mismatch")
    version = run_checked((str(mkimage_tool), "-V"))
    observed_version = (version.stdout or version.stderr).strip()
    if observed_version != declared.get("version"):
        raise ProfileError(
            f"profile mkimage version mismatch: expected {declared.get('version')!r}, "
            f"got {observed_version!r}"
        )
    mkimage_identity = executable_identity(mkimage_tool)
    if mkimage_identity is None:
        raise ProfileError("profile mkimage identity is missing")
    provenance_path = contract_repo_path(
        declared.get("provenance_path"),
        "toolchain.mkimage.provenance_path",
    )
    provenance = load_json_object(provenance_path, "mkimage provenance")
    expected_provenance = {
        "schema": "cohesix-mkimage-provenance/v1",
        "source": {
            "provider": declared["provider"],
            "url": declared["source_url"],
            "archive_path": str(archive.resolve()),
            "archive_sha256": declared["source_archive_sha256"],
            "archive_size": declared["source_archive_size"],
            "version": declared["source_version"],
            "commit": declared["source_commit"],
        },
        "mkimage": {
            "path": str(mkimage_tool.resolve()),
            "sha256": mkimage_identity["sha256"],
            "version": declared["version"],
        },
        "setup_script_sha256": sha256_file(ROOT / "toolchain" / "setup_macos_arm64.sh"),
        "profile_contract_sha256": sha256_file(DEFAULT_CONTRACT),
        "source_date_epoch": declared["source_date_epoch"],
    }
    if provenance != expected_provenance:
        raise ProfileError("mkimage provenance does not match current declared inputs")
    return {
        "schema": "cohesix-mkimage-supply-chain/v1",
        "declared": dict(declared),
        "source_archive": archive_record,
        "mkimage": {
            **mkimage_identity,
            "version": observed_version,
        },
        "provenance": {
            **file_evidence(provenance_path),
            "record": provenance,
        },
    }


def wrapper_build_inputs(
    contract: Mapping[str, Any],
    profile_name: str,
    profile: Mapping[str, Any],
) -> dict[str, Any]:
    """Describe exact wrapper-side host inputs used for one artifact build."""

    objcopy_wrapper = resolve_profile_tool(profile, "objcopy_stdout_wrapper")
    cpio_tool = (
        gnu_cpio_supply_chain_input(contract)
        if profile.get("build_mode") == "wrapper"
        else None
    )

    return {
        "schema": "cohesix-sel4-wrapper-host-inputs/v4",
        "profile": profile_name,
        "target": "rootserver_image",
        "wrapper_sha256": sha256_file(WRAPPER_CMAKE),
        "compiler": compiler_supply_chain_inputs(contract),
        "python_tool": python_supply_chain_inputs(contract, profile),
        "cpio_tool": cpio_tool,
        "objcopy_stdout_wrapper": executable_identity(objcopy_wrapper),
        "mkimage_tool": mkimage_supply_chain_inputs(contract, profile),
    }


def verified_config_build_inputs(
    contract: Mapping[str, Any],
    profile_name: str,
    profile: Mapping[str, Any],
) -> dict[str, Any]:
    """Describe exact host inputs used by an upstream verified build."""

    declared_inputs = wrapper_build_inputs(contract, profile_name, profile)
    return {
        "schema": "cohesix-sel4-verified-host-inputs/v2",
        "profile": profile_name,
        "target": "kernel.elf",
        "compiler": declared_inputs["compiler"],
        "python_tool": declared_inputs["python_tool"],
    }


def write_wrapper_build_input_stamp(
    build_dir: Path,
    inputs: Mapping[str, Any],
) -> Path:
    """Atomically write the exact wrapper input stamp before artifact build."""

    path = build_dir / BUILD_INPUT_STAMP_NAME
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(inputs, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)
    return path


def critical_configuration_inputs(
    build_dir: Path,
    profile: Mapping[str, Any],
) -> dict[str, dict[str, Any]]:
    """Hash the generated configuration surfaces that control built artifacts."""

    paths: dict[str, Path] = {
        "cmake_cache": build_dir / "CMakeCache.txt",
        "build_graph": build_dir / "build.ninja",
    }
    try:
        paths["generated_config"] = find_generated_config(build_dir)
    except ProfileError:
        paths["generated_config"] = (
            build_dir / "kernel" / "gen_config" / "kernel" / "gen_config.json"
        )
    dts_files = profile.get("dts_files", [])
    if isinstance(dts_files, list):
        for index, relative in enumerate(dts_files):
            if isinstance(relative, str) and relative:
                paths[f"dts_{index}"] = build_dir / relative
    gic_header = launcher_gic_header(build_dir)
    if profile.get("qemu_gic_version") is not None:
        paths["launcher_gic_header"] = gic_header or (
            build_dir / "kernel" / "gen_config" / "kernel_config.h"
        )
    timer_header = timer_header_candidate(build_dir)
    if profile.get("timer_clock_hz") is not None:
        paths["timer_header"] = timer_header or (
            build_dir / "kernel" / "gen_headers" / "plat" / "platform_gen.h"
        )
    cmake = profile.get("cmake")
    if isinstance(cmake, dict) and str(cmake.get("ElfloaderRootserversLast", "")).upper() == "ON":
        paths["elfloader_platform_info"] = (
            build_dir / "elfloader" / "gen_headers" / "platform_info.h"
        )
    return {label: file_evidence(path) for label, path in sorted(paths.items())}


def require_complete_configuration_inputs(
    records: Mapping[str, Mapping[str, Any]],
) -> None:
    """Reject a build stamp when a declared generated configuration input is absent."""

    missing = [label for label, record in records.items() if not record.get("exists")]
    if missing:
        raise ProfileError(
            "cannot bind incomplete generated configuration inputs: "
            + ", ".join(sorted(missing))
        )


def required_artifact_inputs(
    build_dir: Path,
    profile: Mapping[str, Any],
) -> dict[str, dict[str, Any]]:
    """Hash every artifact required by the selected profile after a build."""

    records: dict[str, dict[str, Any]] = {}
    missing: list[str] = []
    for label, candidates in artifact_candidates(build_dir, profile).items():
        selected = next((path for path in candidates if path.is_file()), None)
        if selected is None:
            missing.append(label)
            continue
        record = file_evidence(selected)
        if int(record.get("size", 0)) <= 0:
            raise ProfileError(f"built artifact is empty: {selected}")
        records[label] = record
    if missing:
        raise ProfileError(
            "cannot complete build-input stamp; required artifacts are missing: "
            + ", ".join(sorted(missing))
        )
    return records


def causal_build_output_labels(profile: Mapping[str, Any]) -> tuple[str, ...]:
    """Return artifact labels that must be created by the build invocation."""

    policy = profile.get("artifact_policy")
    labels = policy.get("elf_artifacts") if isinstance(policy, dict) else None
    if (
        not isinstance(labels, list)
        or not labels
        or any(not isinstance(label, str) or not label for label in labels)
        or len(set(labels)) != len(labels)
    ):
        raise ProfileError("profile artifact_policy.elf_artifacts is invalid")
    return tuple(sorted(labels))


def build_start_evidence_template(
    build_dir: Path,
    profile: Mapping[str, Any],
) -> dict[str, Any]:
    """Describe the required absence of stamps and build-created outputs."""

    candidates_by_label = artifact_candidates(build_dir, profile)
    outputs: dict[str, dict[str, Any]] = {}
    for label in causal_build_output_labels(profile):
        candidates = candidates_by_label.get(label)
        if not candidates:
            raise ProfileError(
                f"causal build output {label!r} has no declared artifact path"
            )
        outputs[label] = {
            "candidates": [str(path.resolve()) for path in candidates],
            "existing": [],
        }
    return {
        "schema": "cohesix-sel4-build-start/v1",
        "stamp": {
            "path": str((build_dir / BUILD_INPUT_STAMP_NAME).resolve()),
            "exists": False,
        },
        "outputs": outputs,
    }


def require_fresh_profile_build_start(
    build_dir: Path,
    profile: Mapping[str, Any],
) -> dict[str, Any]:
    """Reject reuse and record that this invocation starts without built outputs."""

    expected = build_start_evidence_template(build_dir, profile)
    stamp_path = build_dir / BUILD_INPUT_STAMP_NAME
    if stamp_path.exists() or stamp_path.is_symlink():
        raise ProfileError(
            "profile build tree already has a build-input stamp; configure a fresh "
            f"tree instead of re-stamping or reusing {build_dir}"
        )
    preexisting: list[str] = []
    candidates_by_label = artifact_candidates(build_dir, profile)
    for label in causal_build_output_labels(profile):
        for path in candidates_by_label[label]:
            if path.exists() or path.is_symlink():
                preexisting.append(f"{label}={path}")
    if preexisting:
        raise ProfileError(
            "profile build tree already contains build-created outputs; configure "
            "a fresh tree instead of re-stamping or reusing it: "
            + ", ".join(preexisting)
        )
    return expected


def validate_build_start_evidence(
    build_dir: Path,
    profile: Mapping[str, Any],
    evidence: Mapping[str, Any],
) -> dict[str, Any]:
    """Validate the recorded pre-build absence against declared output paths."""

    expected = build_start_evidence_template(build_dir, profile)
    if dict(evidence) != expected:
        raise ProfileError(
            "profile build-input stamp lacks valid fresh-build start evidence"
        )
    return expected


def profile_build_stamp(
    contract: Mapping[str, Any],
    profile_name: str,
    profile: Mapping[str, Any],
    source_root: Path,
    build_dir: Path,
    source_evidence: Mapping[str, Any],
    *,
    build_start: Mapping[str, Any],
    jobs: int,
    status: str,
    require_outputs: bool,
) -> dict[str, Any]:
    """Bind source, commands, configuration, tools, and outputs for one build."""

    if status not in {"pending", "complete"}:
        raise ProfileError(f"invalid build-input stamp status: {status!r}")
    if source_evidence.get("errors"):
        raise ProfileError("cannot stamp a build from invalid source evidence")
    validated_build_start = validate_build_start_evidence(
        build_dir,
        profile,
        build_start,
    )
    build_mode = str(profile.get("build_mode", ""))
    configure_command: list[str] | None = None
    host_inputs: dict[str, Any] | None = None
    if build_mode == "wrapper":
        configure_command = wrapper_configure_command(profile, source_root, build_dir)
        build_command = wrapper_build_command(build_dir, jobs)
        host_inputs = wrapper_build_inputs(contract, profile_name, profile)
    elif build_mode == "verified-config":
        build_command = verified_config_build_command(profile, source_root)
        host_inputs = verified_config_build_inputs(
            contract,
            profile_name,
            profile,
        )
    else:
        raise ProfileError(f"unsupported build mode: {build_mode!r}")

    configuration = critical_configuration_inputs(build_dir, profile)
    if require_outputs:
        require_complete_configuration_inputs(configuration)
        artifacts: dict[str, dict[str, Any]] | None = required_artifact_inputs(
            build_dir,
            profile,
        )
        post_build_outputs: dict[str, dict[str, Any]] | None = {}
        for label in causal_build_output_labels(profile):
            record = artifacts.get(label)
            if record is None:
                raise ProfileError(
                    f"causal build output {label!r} is absent after the build"
                )
            post_build_outputs[label] = record
    else:
        artifacts = None
        post_build_outputs = None
    environment: dict[str, Any] = {
        "SEL4_CACHE_DIR": "",
        "PATH_prepend": [
            str(path) for path in wrapper_path_prefixes(contract, profile)
        ],
    }
    mkimage_tool = resolve_profile_tool(profile, "mkimage_tool")
    if mkimage_tool is not None:
        environment["mkimage_command"] = str(mkimage_tool)
    return {
        "schema": "cohesix-sel4-profile-build-inputs/v2",
        "status": status,
        "profile": profile_name,
        "build_mode": build_mode,
        "build_dir": str(build_dir.resolve()),
        "contract_values_sha256": canonical_sha256(contract),
        "validator_sha256": sha256_file(VALIDATOR),
        "memoization_cache": profile.get("memoization_cache"),
        "source": dict(source_evidence),
        "commands": {
            "configure": configure_command,
            "build": build_command,
            "jobs": jobs,
        },
        "environment": environment,
        "host_inputs": host_inputs,
        "causal_freshness": {
            "schema": "cohesix-sel4-causal-freshness/v1",
            "build_start": validated_build_start,
            "post_build_outputs": post_build_outputs,
        },
        "configuration": configuration,
        "artifacts": artifacts,
    }


def run_checked(
    argv: Sequence[str],
    *,
    cwd: Path | None = None,
    env: Mapping[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run a subprocess without a shell and retain text output."""

    try:
        return subprocess.run(
            list(argv),
            cwd=cwd,
            env=dict(env) if env is not None else None,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except FileNotFoundError as exc:
        raise ProfileError(f"required command not found: {argv[0]}") from exc
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or exc.stdout or "").strip()
        if detail:
            detail = f": {detail}"
        raise ProfileError(f"command failed ({' '.join(argv)}){detail}") from exc


def git_output(repo: Path, *args: str) -> str:
    """Run a read-only Git query in a pinned source repository."""

    return run_checked(("git", "-C", str(repo), *args)).stdout.rstrip()


def repo_managed_build_evidence(
    profile_name: str,
    build_dir: Path,
    contract: Mapping[str, Any],
) -> tuple[dict[str, Any], list[str]]:
    """Validate one relocated, repository-managed profile artifact tree."""

    errors: list[str] = []
    canonical_name, _profile = get_profile(contract, profile_name)
    expected_dir = REPO_MANAGED_PROFILE_BUILDS.get(canonical_name)
    resolved = build_dir.expanduser().resolve()
    evidence: dict[str, Any] = {
        "schema": "cohesix-sel4-repo-managed-build/v1",
        "profile": canonical_name,
        "build_dir": str(resolved),
        "expected_build_dir": (
            str(expected_dir.resolve()) if expected_dir is not None else None
        ),
        "tracked": False,
        "clean": False,
        "stamp": None,
        "relocated_records": {},
    }
    if expected_dir is None:
        errors.append(
            f"profile {canonical_name!r} has no repository-managed build tree"
        )
        return evidence, errors
    expected_resolved = expected_dir.resolve()
    if resolved != expected_resolved:
        errors.append(
            "repository-managed profile selection mismatch: "
            f"expected {expected_resolved}, got {resolved}"
        )
        return evidence, errors

    relative_dir = resolved.relative_to(ROOT.resolve())
    tracked = git_output(ROOT, "ls-files", "--", str(relative_dir))
    evidence["tracked"] = bool(tracked)
    if not tracked:
        errors.append(f"repository-managed build tree is not tracked: {resolved}")

    diff = subprocess.run(
        (
            "git",
            "-C",
            str(ROOT),
            "diff",
            "--no-ext-diff",
            "--quiet",
            "HEAD",
            "--",
            str(relative_dir),
        ),
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if diff.returncode not in {0, 1}:
        errors.append(
            "cannot compare repository-managed build tree with HEAD: "
            + (diff.stderr.strip() or f"git exited {diff.returncode}")
        )
    untracked = git_output(
        ROOT,
        "ls-files",
        "--others",
        "--exclude-standard",
        "--",
        str(relative_dir),
    )
    evidence["clean"] = diff.returncode == 0 and not untracked
    evidence["untracked_entry_count"] = (
        len(untracked.splitlines()) if untracked else 0
    )
    if diff.returncode == 1:
        errors.append(
            f"repository-managed build tree differs from HEAD: {relative_dir}"
        )
    if untracked:
        errors.append(
            f"repository-managed build tree contains untracked entries: {relative_dir}"
        )

    stamp_path = resolved / BUILD_INPUT_STAMP_NAME
    stamp_record = file_evidence(stamp_path)
    evidence["stamp"] = stamp_record
    if not stamp_path.is_file():
        errors.append(
            f"repository-managed build-input stamp is missing: {stamp_path}"
        )
        return evidence, errors
    try:
        stamp = load_json_object(stamp_path, "repository-managed build-input stamp")
    except ProfileError as exc:
        errors.append(str(exc))
        return evidence, errors

    for key, expected in (
        ("schema", "cohesix-sel4-profile-build-inputs/v2"),
        ("status", "complete"),
        ("profile", canonical_name),
        ("build_mode", "wrapper"),
        ("contract_values_sha256", canonical_sha256(contract)),
    ):
        actual = stamp.get(key)
        if actual != expected:
            errors.append(
                f"repository-managed build stamp {key} mismatch: "
                f"expected {expected!r}, got {actual!r}"
            )

    recorded_root_value = stamp.get("build_dir")
    if not isinstance(recorded_root_value, str) or not recorded_root_value:
        errors.append("repository-managed build stamp has no original build_dir")
        return evidence, errors
    recorded_root = Path(recorded_root_value)

    relocated_records: dict[str, dict[str, Any]] = {}
    record_groups: list[tuple[str, Any]] = [
        ("artifacts", stamp.get("artifacts")),
        ("configuration", stamp.get("configuration")),
    ]
    freshness = stamp.get("causal_freshness")
    if isinstance(freshness, dict):
        record_groups.append(
            (
                "causal_freshness.post_build_outputs",
                freshness.get("post_build_outputs"),
            )
        )
    for group_name, group in record_groups:
        if not isinstance(group, dict):
            errors.append(
                f"repository-managed build stamp lacks {group_name} records"
            )
            continue
        for label, original in group.items():
            record_label = f"{group_name}.{label}"
            if not isinstance(original, dict):
                errors.append(
                    f"repository-managed build stamp record is invalid: {record_label}"
                )
                continue
            original_path = original.get("path")
            if not isinstance(original_path, str) or not original_path:
                errors.append(
                    f"repository-managed build stamp record lacks path: {record_label}"
                )
                continue
            try:
                relative = Path(original_path).relative_to(recorded_root)
            except ValueError:
                errors.append(
                    "repository-managed build stamp record escapes its original "
                    f"build root: {record_label}"
                )
                continue
            relocated = (resolved / relative).resolve()
            if not relocated.is_relative_to(resolved):
                errors.append(
                    "repository-managed build stamp record escapes the relocated "
                    f"build root: {record_label}"
                )
                continue
            observed = file_evidence(relocated)
            relocated_records[record_label] = observed
            if not observed.get("exists"):
                errors.append(
                    f"repository-managed build artifact is missing: {relocated}"
                )
                continue
            for identity_key in ("size", "sha256"):
                expected_identity = original.get(identity_key)
                if observed.get(identity_key) != expected_identity:
                    errors.append(
                        "repository-managed build artifact identity mismatch for "
                        f"{record_label} {identity_key}: expected "
                        f"{expected_identity!r}, got {observed.get(identity_key)!r}"
                    )
    evidence["relocated_records"] = relocated_records
    return evidence, errors


def validate_repo_managed_build(
    contract: Mapping[str, Any],
    profile_name: str,
    build_dir: Path,
    *,
    for_runtime: bool = False,
) -> dict[str, Any]:
    """Validate an immutable repository artifact tree without live build paths."""

    canonical_name, profile = get_profile(contract, profile_name)
    managed, errors = repo_managed_build_evidence(
        canonical_name,
        build_dir,
        contract,
    )
    if for_runtime and not profile.get("runtime_eligible", False):
        errors.append(f"profile {canonical_name} is not runtime eligible")
    return {
        "schema": "cohesix-sel4-profile-evidence/v2",
        "profile": canonical_name,
        "description": profile.get("description"),
        "evidence_class": profile.get("evidence_class"),
        "claim_eligibility": {
            "profile_configuration_for_release": False,
            "runtime": bool(profile.get("runtime_eligible", False)),
            "artifact_set_shipping": False,
            "cohesix_system_image": False,
        },
        "build_mode": "repository-managed-artifacts",
        "build_dir": str(build_dir.expanduser().resolve()),
        "repo_managed": managed,
        "requirements": {
            "source": False,
            "artifacts": True,
            "release": False,
            "runtime": for_runtime,
        },
        "valid": not errors,
        "errors": errors,
    }


def validate_source(
    contract: Mapping[str, Any],
    profile: Mapping[str, Any],
    source_root: Path,
) -> dict[str, Any]:
    """Validate the complete upstream manifest checkout and allowed dirt."""

    source_root = source_root.expanduser().resolve()
    source = contract.get("source")
    if not isinstance(source, dict):
        raise ProfileError("profile contract source table is invalid")
    repositories = source.get("repositories")
    if not isinstance(repositories, dict) or not repositories:
        raise ProfileError("profile contract has no pinned source repositories")

    errors: list[str] = []
    observed: dict[str, dict[str, Any]] = {}
    source_policy = profile.get("source_policy")
    if source_policy not in {"clean", "pi4-overlay"}:
        errors.append(f"unsupported source policy: {source_policy!r}")

    overlay: dict[str, Any] = {}
    overlay_patch_path: Path | None = None
    overlay_patch_bytes: bytes | None = None
    try:
        overlay, overlay_patch_path, overlay_patch_bytes = pi4_overlay_patch(contract)
    except ProfileError as exc:
        errors.append(str(exc))
    overlay_rel = str(overlay.get("path", ""))
    overlay_diff_format = str(overlay.get("diff_format", ""))
    overlay_repo_rel = "kernel"
    overlay_path_in_repo = overlay_rel.removeprefix("kernel/")

    for relative, expected_commit in repositories.items():
        repo = source_root / str(relative)
        record: dict[str, Any] = {
            "path": str(repo),
            "expected_commit": str(expected_commit),
        }
        observed[str(relative)] = record
        if not repo.is_dir():
            errors.append(f"pinned source repository is missing: {repo}")
            continue
        try:
            actual_commit = git_output(repo, "rev-parse", "HEAD")
            status = git_output(repo, "status", "--porcelain=v1", "--untracked-files=all")
        except ProfileError as exc:
            errors.append(str(exc))
            continue
        record["actual_commit"] = actual_commit
        record["dirty"] = bool(status)
        if actual_commit != expected_commit:
            errors.append(
                f"source commit mismatch for {relative}: "
                f"expected {expected_commit}, got {actual_commit}"
            )

        allow_overlay = source_policy == "pi4-overlay" and str(relative) == overlay_repo_rel
        if not allow_overlay:
            if status:
                errors.append(f"source repository must be clean: {repo}")
            continue

        expected_status = f" M {overlay_path_in_repo}"
        status_lines = [line for line in status.splitlines() if line]
        if status_lines != [expected_status]:
            errors.append(
                "Pi source dirt must be exactly the recorded VL805 overlay; "
                f"observed {status_lines!r}"
            )
            continue
        diff = run_checked(
            (
                "git",
                "-C",
                str(repo),
                "diff",
                "--binary",
                "--full-index",
                "--no-ext-diff",
                "--no-renames",
                "--no-color",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--",
                overlay_path_in_repo,
            )
        ).stdout.encode("utf-8")
        actual_diff_hash = sha256_bytes(diff)
        expected_diff_hash = str(overlay.get("diff_sha256", ""))
        record["overlay_diff_format"] = overlay_diff_format
        record["overlay_diff_sha256"] = actual_diff_hash
        record["overlay_patch"] = (
            file_evidence(overlay_patch_path)
            if overlay_patch_path is not None
            else None
        )
        if actual_diff_hash != expected_diff_hash:
            errors.append(
                "Pi VL805 overlay digest mismatch: "
                f"expected {expected_diff_hash}, got {actual_diff_hash}"
            )
        if overlay_patch_bytes is not None and diff != overlay_patch_bytes:
            errors.append(
                "Pi VL805 overlay diff does not match the source-controlled "
                "patch bytes"
            )

    evidence = {
        "root": str(source_root),
        "policy": source_policy,
        "repositories": observed,
        "errors": errors,
    }
    return evidence


def prepare_source(
    contract: Mapping[str, Any],
    profile_name: str,
    source_root: Path,
    *,
    dry_run: bool,
) -> dict[str, Any]:
    """Prepare one pinned source checkout for its declared source policy."""

    canonical, profile = get_profile(contract, profile_name)
    source_root = source_root.expanduser().resolve()
    source_policy = profile.get("source_policy")
    if source_policy == "clean":
        evidence = validate_source(contract, profile, source_root)
        if evidence["errors"]:
            raise ProfileError("; ".join(evidence["errors"]))
        return {
            "profile": canonical,
            "source": str(source_root),
            "action": "already-clean",
            "dry_run": dry_run,
        }
    if source_policy != "pi4-overlay":
        raise ProfileError(f"unsupported source policy: {source_policy!r}")

    existing = validate_source(contract, profile, source_root)
    if not existing["errors"]:
        return {
            "profile": canonical,
            "source": str(source_root),
            "action": "already-applied",
            "dry_run": dry_run,
        }

    source = contract.get("source")
    repositories = source.get("repositories") if isinstance(source, dict) else None
    if not isinstance(repositories, dict) or not repositories:
        raise ProfileError("profile contract has no pinned source repositories")
    for relative, expected_commit in repositories.items():
        repository = source_root / str(relative)
        if not repository.is_dir():
            raise ProfileError(f"pinned source repository is missing: {repository}")
        actual_commit = git_output(repository, "rev-parse", "HEAD")
        if actual_commit != expected_commit:
            raise ProfileError(
                f"source commit mismatch for {relative}: expected "
                f"{expected_commit}, got {actual_commit}"
            )
        status = git_output(
            repository,
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
        )
        if status:
            raise ProfileError(
                "Pi overlay preparation requires a pristine pinned checkout; "
                f"source repository is dirty: {repository}"
            )

    overlay, patch_path, _patch_bytes = pi4_overlay_patch(contract)
    overlay_relative = str(overlay["path"]).removeprefix("kernel/")
    kernel_repository = source_root / "kernel"
    apply_arguments = (
        "git",
        "-C",
        str(kernel_repository),
        "apply",
        "--whitespace=nowarn",
        str(patch_path),
    )
    run_checked((*apply_arguments[:4], "--check", *apply_arguments[4:]))
    if dry_run:
        return {
            "profile": canonical,
            "source": str(source_root),
            "action": "would-apply",
            "patch": str(patch_path),
            "target": str(kernel_repository / overlay_relative),
            "dry_run": True,
        }

    run_checked(apply_arguments)
    prepared = validate_source(contract, profile, source_root)
    if prepared["errors"]:
        raise ProfileError(
            "prepared Pi overlay failed post-application validation: "
            + "; ".join(prepared["errors"])
        )
    return {
        "profile": canonical,
        "source": str(source_root),
        "action": "applied",
        "patch": str(patch_path),
        "target": str(kernel_repository / overlay_relative),
        "dry_run": False,
    }


def declared_dts_paths(
    build_dir: Path,
    profile: Mapping[str, Any],
) -> list[Path]:
    """Resolve the exact DTS inputs declared by a profile."""

    relative_paths = profile.get("dts_files")
    if not isinstance(relative_paths, list) or not all(
        isinstance(relative, str) and relative for relative in relative_paths
    ):
        raise ProfileError("profile dts_files must be a non-empty string list")
    if not relative_paths:
        raise ProfileError("profile dts_files must not be empty")
    return [build_dir / relative for relative in relative_paths]


def launcher_gic_header(build_dir: Path) -> Path | None:
    """Select the same generated GIC header precedence as QEMU launchers."""

    candidates = (
        build_dir / "kernel" / "gen_config" / "kernel_config.h",
        build_dir / "kernel" / "gen_config" / "kernel" / "gen_config.h",
        build_dir / "kernel" / "include" / "autoconf.h",
        build_dir / "kernel" / "autoconf" / "autoconf.h",
    )
    return next((path for path in candidates if path.is_file()), None)


def detect_launcher_gic_version(config_header: Path) -> int:
    """Run the repository launcher detector against a generated header."""

    if not GIC_DETECTOR.is_file() or not os.access(GIC_DETECTOR, os.X_OK):
        raise ProfileError(
            f"QEMU launcher GIC detector is missing or not executable: {GIC_DETECTOR}"
        )
    completed = run_checked((str(GIC_DETECTOR), str(config_header)))
    value = completed.stdout.strip()
    if value not in {"2", "3"}:
        raise ProfileError(
            f"QEMU launcher GIC detector returned invalid value {value!r} "
            f"for {config_header}"
        )
    return int(value, 10)


def artifact_candidates(
    build_dir: Path,
    profile: Mapping[str, Any],
) -> dict[str, tuple[Path, ...]]:
    """Return required kernel, root-server, image, and DTB artifacts."""

    kernel = (
        build_dir / "kernel" / "kernel.elf",
        build_dir / "kernel.elf",
    )
    build_mode = str(profile.get("build_mode", ""))
    if build_mode == "verified-config":
        return {
            "kernel": kernel,
            "kernel_dtb": (build_dir / "kernel.dtb",),
        }
    artifacts = {
        "kernel": kernel,
        "elfloader": (build_dir / "elfloader" / "elfloader",),
        "rootserver": (build_dir / "apps" / "sel4test-driver" / "sel4test-driver",),
        "kernel_dtb": (build_dir / "kernel" / "kernel.dtb",),
    }
    target = profile.get("target")
    if target == "qemu":
        artifacts.update(
            {
                "system_image": (
                    build_dir
                    / "images"
                    / "sel4test-driver-image-arm-qemu-arm-virt",
                ),
                "qemu_dtb": (build_dir / "qemu-arm-virt.dtb",),
            }
        )
        if profile.get("minimum_elfloader_archive_bytes") is not None:
            artifacts["elfloader_archive"] = (
                build_dir / "elfloader" / "archive.archive.o.cpio",
            )
    elif target == "pi4":
        artifacts["system_image"] = (
            build_dir / "images" / "sel4test-driver-image-arm-bcm2711",
        )
    return artifacts


def inspect_elf_load_segments(path: Path) -> dict[str, Any]:
    """Parse a structurally valid little-endian AArch64 ELF64 load image."""

    data = path.read_bytes()
    if len(data) < 64 or data[:4] != b"\x7fELF":
        raise ProfileError(f"artifact is not an ELF image: {path}")
    elf_class = data[4]
    byte_order = data[5]
    identity_version = data[6]
    if elf_class != 2 or byte_order != 1 or identity_version != 1:
        raise ProfileError(
            f"artifact must be little-endian ELF64 with current identity: {path}"
        )
    elf_type = struct.unpack_from("<H", data, 16)[0]
    machine = struct.unpack_from("<H", data, 18)[0]
    file_version = struct.unpack_from("<I", data, 20)[0]
    entry = struct.unpack_from("<Q", data, 24)[0]
    header_size = struct.unpack_from("<H", data, 52)[0]
    phoff = struct.unpack_from("<Q", data, 32)[0]
    phentsize = struct.unpack_from("<H", data, 54)[0]
    phnum = struct.unpack_from("<H", data, 56)[0]
    if machine != 183:
        raise ProfileError(f"artifact is not an AArch64 ELF image: {path}")
    if elf_type != 2:
        raise ProfileError(f"artifact is not an ET_EXEC ELF image: {path}")
    if entry == 0:
        raise ProfileError(f"artifact has a zero ELF entry point: {path}")
    if file_version != 1 or header_size != 64:
        raise ProfileError(f"artifact has an invalid ELF64 header: {path}")
    if phnum == 0:
        raise ProfileError(f"artifact has no ELF program headers: {path}")
    program_format = "<IIQQQQQQ"
    expected_entry_size = struct.calcsize(program_format)
    if phentsize != expected_entry_size or phoff < header_size:
        raise ProfileError(f"artifact has an invalid ELF program-header size: {path}")
    if phoff + (phentsize * phnum) > len(data):
        raise ProfileError(f"artifact has truncated ELF program headers: {path}")

    load_segments: list[dict[str, Any]] = []
    for index in range(phnum):
        values = struct.unpack_from(program_format, data, phoff + (index * phentsize))
        p_type, flags, p_offset, p_vaddr, _paddr, filesz, memsz, align = values
        if p_type != 1:
            continue
        if flags & ~0x7:
            raise ProfileError(
                f"artifact LOAD segment {index} has unknown flags 0x{flags:x}: {path}"
            )
        if not flags & 0x4:
            raise ProfileError(
                f"artifact LOAD segment {index} is not readable: {path}"
            )
        if memsz == 0 or filesz > memsz:
            raise ProfileError(
                f"artifact LOAD segment {index} has invalid file/memory sizes: {path}"
            )
        if p_offset + filesz > len(data):
            raise ProfileError(
                f"artifact LOAD segment {index} extends beyond the file: {path}"
            )
        if align not in {0, 1}:
            if align & (align - 1):
                raise ProfileError(
                    f"artifact LOAD segment {index} has non-power-of-two alignment: {path}"
                )
            if p_offset % align != p_vaddr % align:
                raise ProfileError(
                    f"artifact LOAD segment {index} has incongruent alignment: {path}"
                )
        flag_text = "".join(
            letter for bit, letter in ((4, "R"), (2, "W"), (1, "E")) if flags & bit
        )
        load_segments.append(
            {
                "index": index,
                "offset": p_offset,
                "virtual_address": p_vaddr,
                "file_size": filesz,
                "memory_size": memsz,
                "alignment": align,
                "flags": flag_text,
                "writable": bool(flags & 2),
                "executable": bool(flags & 1),
                "rwx": bool(flags & 2 and flags & 1),
            }
        )
    if not load_segments:
        raise ProfileError(f"artifact has no readable PT_LOAD segment: {path}")
    executable_segments = [
        segment for segment in load_segments if segment["executable"]
    ]
    if not executable_segments:
        raise ProfileError(
            f"artifact has no readable executable PT_LOAD segment: {path}"
        )
    if not any(
        int(segment["virtual_address"])
        <= entry
        < int(segment["virtual_address"]) + int(segment["memory_size"])
        for segment in executable_segments
    ):
        raise ProfileError(
            f"artifact ELF entry point is outside executable PT_LOAD memory: {path}"
        )
    return {
        **file_evidence(path),
        "elf_class": 64,
        "byte_order": "little",
        "elf_type": "ET_EXEC",
        "entry_point": entry,
        "machine": machine,
        "load_segments": load_segments,
    }


def validate_artifact_policy(
    profile: Mapping[str, Any],
    artifacts: Mapping[str, Mapping[str, Any]],
    *,
    require_artifacts: bool,
) -> tuple[dict[str, Any], list[str]]:
    """Inspect declared ELF artifacts and enforce shipping/RWX claim policy."""

    policy = profile.get("artifact_policy")
    errors: list[str] = []
    if not isinstance(policy, dict):
        return {}, ["profile has no artifact_policy table"]
    artifact_class = str(policy.get("class", ""))
    shipping_eligible = policy.get("shipping_eligible")
    rwx_policy = str(policy.get("rwx_policy", ""))
    elf_labels = policy.get("elf_artifacts", [])
    expected_machine = policy.get("elf_machine", 183)
    if not artifact_class:
        errors.append("artifact_policy.class is missing")
    if not isinstance(shipping_eligible, bool):
        errors.append("artifact_policy.shipping_eligible must be boolean")
        shipping_eligible = False
    if rwx_policy not in {"reject", "record-nonshipping-exception"}:
        errors.append(f"unsupported artifact RWX policy: {rwx_policy!r}")
    if shipping_eligible and rwx_policy != "reject":
        errors.append("shipping-eligible artifact sets must reject RWX LOAD segments")
    if not isinstance(elf_labels, list) or not all(
        isinstance(label, str) and label for label in elf_labels
    ):
        errors.append("artifact_policy.elf_artifacts must be a string list")
        elf_labels = []

    inspections: dict[str, Any] = {}
    exceptions: list[dict[str, Any]] = []
    minimum_archive = profile.get("minimum_elfloader_archive_bytes")
    archive_capacity: dict[str, Any] | None = None
    if minimum_archive is not None:
        archive = artifacts.get("elfloader_archive")
        archive_capacity = {
            "minimum_bytes": minimum_archive,
            "observed_bytes": archive.get("size") if archive is not None else None,
        }
        if archive is None:
            if require_artifacts:
                errors.append("elfloader archive capacity artifact is missing")
        elif int(archive.get("size", 0)) < int(minimum_archive):
            errors.append(
                "elfloader archive capacity is below the profile minimum: "
                f"expected at least {minimum_archive}, got {archive.get('size', 0)}"
            )
    for label in elf_labels:
        artifact = artifacts.get(label)
        if artifact is None:
            if require_artifacts:
                errors.append(f"ELF policy artifact is missing: {label}")
            continue
        path = Path(str(artifact.get("path", "")))
        try:
            inspection = inspect_elf_load_segments(path)
        except (OSError, ProfileError) as exc:
            errors.append(f"cannot inspect ELF policy artifact {label}: {exc}")
            continue
        inspections[label] = inspection
        if inspection["machine"] != expected_machine:
            errors.append(
                f"ELF artifact {label} machine mismatch: expected "
                f"{expected_machine}, got {inspection['machine']}"
            )
        rwx_segments = [
            segment for segment in inspection["load_segments"] if segment["rwx"]
        ]
        if not rwx_segments:
            continue
        if shipping_eligible or rwx_policy == "reject":
            errors.append(f"ELF artifact {label} contains RWX LOAD segments")
            continue
        exception_id = str(policy.get("exception_id", ""))
        exception_reason = str(policy.get("exception_reason", ""))
        if not exception_id or not exception_reason:
            errors.append(
                "non-shipping RWX policy requires exception_id and exception_reason"
            )
            continue
        exceptions.append(
            {
                "id": exception_id,
                "artifact": label,
                "reason": exception_reason,
                "segments": rwx_segments,
            }
        )
    evidence = {
        "class": artifact_class,
        "shipping_eligible": shipping_eligible,
        "cohesix_system_image": bool(policy.get("cohesix_system_image", False)),
        "rwx_policy": rwx_policy,
        "elf_machine": expected_machine,
        "elf_artifacts": list(elf_labels),
        "inspections": inspections,
        "exceptions": exceptions,
        "elfloader_archive_capacity": archive_capacity,
    }
    return evidence, errors


def _fdt_strings(value: bytes) -> list[str]:
    """Decode a flattened-device-tree string or string-list value."""

    if not value or value[-1] != 0:
        return []
    try:
        result = [part.decode("utf-8") for part in value[:-1].split(b"\0")]
    except UnicodeDecodeError:
        return []
    if not all(item and all(character.isprintable() for character in item) for item in result):
        return []
    return result


def inspect_dtb(path: Path) -> dict[str, Any]:
    """Parse FDT structure and expose semantic string/node values for validation."""

    data = path.read_bytes()
    if len(data) < 40:
        raise ProfileError(f"DTB has a truncated header: {path}")
    header = struct.unpack_from(">10I", data, 0)
    (
        magic,
        total_size,
        structure_offset,
        strings_offset,
        _reserve_offset,
        version,
        _last_compatible_version,
        _boot_cpu,
        strings_size,
        structure_size,
    ) = header
    if magic != 0xD00DFEED:
        raise ProfileError(f"DTB has an invalid magic value: {path}")
    if total_size > len(data) or total_size < 40:
        raise ProfileError(f"DTB has an invalid total size: {path}")
    structure_end = structure_offset + structure_size
    strings_end = strings_offset + strings_size
    if structure_end > total_size or strings_end > total_size:
        raise ProfileError(f"DTB block extends beyond total size: {path}")
    strings = data[strings_offset:strings_end]
    position = structure_offset
    stack: list[str] = []
    nodes: list[str] = []
    compatible_values: list[str] = []
    string_properties: dict[str, list[str]] = {}
    saw_end = False
    while position + 4 <= structure_end:
        token = struct.unpack_from(">I", data, position)[0]
        position += 4
        if token == 1:
            end = data.find(b"\0", position, structure_end)
            if end < 0:
                raise ProfileError(f"DTB node name is unterminated: {path}")
            try:
                name = data[position:end].decode("utf-8")
            except UnicodeDecodeError as exc:
                raise ProfileError(f"DTB node name is not UTF-8: {path}") from exc
            stack.append(name)
            node_path = "/" + "/".join(item for item in stack if item)
            nodes.append(node_path or "/")
            position = (end + 4) & ~3
        elif token == 2:
            if not stack:
                raise ProfileError(f"DTB contains an unmatched end-node token: {path}")
            stack.pop()
        elif token == 3:
            if position + 8 > structure_end:
                raise ProfileError(f"DTB property header is truncated: {path}")
            value_size, name_offset = struct.unpack_from(">II", data, position)
            position += 8
            value_end = position + value_size
            if value_end > structure_end or name_offset >= len(strings):
                raise ProfileError(f"DTB property range is invalid: {path}")
            name_end = strings.find(b"\0", name_offset)
            if name_end < 0:
                raise ProfileError(f"DTB property name is unterminated: {path}")
            try:
                name = strings[name_offset:name_end].decode("utf-8")
            except UnicodeDecodeError as exc:
                raise ProfileError(f"DTB property name is not UTF-8: {path}") from exc
            values = _fdt_strings(data[position:value_end])
            if values:
                string_properties.setdefault(name, []).extend(values)
                if name == "compatible":
                    compatible_values.extend(values)
            position = (value_end + 3) & ~3
        elif token == 4:
            continue
        elif token == 9:
            saw_end = True
            break
        else:
            raise ProfileError(f"DTB contains unknown structure token {token}: {path}")
    if not saw_end or stack:
        raise ProfileError(f"DTB structure is incomplete: {path}")
    return {
        **file_evidence(path),
        "fdt_version": version,
        "total_size": total_size,
        "node_count": len(nodes),
        "node_names": sorted({node.rsplit("/", 1)[-1] for node in nodes}),
        "compatible": sorted(set(compatible_values)),
        "string_properties": {
            key: sorted(set(values)) for key, values in sorted(string_properties.items())
        },
    }


def validate_dtb_semantics(
    profile: Mapping[str, Any],
    artifacts: Mapping[str, Mapping[str, Any]],
    *,
    require_artifacts: bool,
) -> tuple[dict[str, Any], list[str]]:
    """Validate every declared DTB independently against semantic expectations."""

    policy = profile.get("dtb")
    if not isinstance(policy, dict):
        return {}, ["profile has no dtb table"]
    labels = policy.get("artifacts", [])
    required_compatible = policy.get("required_compatible", [])
    forbidden_compatible = policy.get("forbidden_compatible", [])
    required_nodes = policy.get("required_nodes", [])
    required_properties = policy.get("required_string_properties", [])
    list_fields = {
        "artifacts": labels,
        "required_compatible": required_compatible,
        "forbidden_compatible": forbidden_compatible,
        "required_nodes": required_nodes,
        "required_string_properties": required_properties,
    }
    errors: list[str] = []
    if any(
        not isinstance(values, list)
        or not all(isinstance(item, str) and item for item in values)
        for values in list_fields.values()
    ):
        return {}, ["profile DTB semantic fields must be string lists"]
    parsed_properties: list[tuple[str, str]] = []
    for selector in required_properties:
        if "=" not in selector:
            errors.append(f"invalid required DTB property selector: {selector!r}")
            continue
        name, value = selector.split("=", 1)
        if not name or not value:
            errors.append(f"invalid required DTB property selector: {selector!r}")
            continue
        parsed_properties.append((name, value))

    inspections: dict[str, Any] = {}
    for label in labels:
        artifact = artifacts.get(label)
        if artifact is None:
            if require_artifacts:
                errors.append(f"semantic DTB artifact is missing: {label}")
            continue
        path = Path(str(artifact.get("path", "")))
        try:
            inspection = inspect_dtb(path)
        except (OSError, ProfileError) as exc:
            errors.append(f"cannot inspect semantic DTB artifact {label}: {exc}")
            continue
        inspections[label] = inspection
        compatible = set(inspection["compatible"])
        node_names = set(inspection["node_names"])
        string_properties = inspection["string_properties"]
        for expected in required_compatible:
            if expected not in compatible:
                errors.append(f"DTB {label} lacks compatible value {expected!r}")
        for forbidden in forbidden_compatible:
            if forbidden in compatible:
                errors.append(f"DTB {label} contains forbidden compatible {forbidden!r}")
        for expected in required_nodes:
            if expected not in node_names:
                errors.append(f"DTB {label} lacks required node {expected!r}")
        for name, value in parsed_properties:
            if value not in string_properties.get(name, []):
                errors.append(
                    f"DTB {label} lacks required string property {name}={value!r}"
                )
    return {"policy": dict(policy), "inspections": inspections}, errors


def timer_header_candidate(build_dir: Path) -> Path | None:
    """Return the selected generated platform-timer header."""

    candidates = (
        build_dir / "kernel" / "gen_headers" / "plat" / "platform_gen.h",
        build_dir / "gen_headers" / "plat" / "platform_gen.h",
    )
    return next((path for path in candidates if path.is_file()), None)


def read_timer_clock_hz(build_dir: Path) -> int | None:
    """Read TIMER_CLOCK_HZ from a generated platform header when present."""

    direct_pattern = re.compile(
        r"^#define\s+TIMER_CLOCK_HZ\s+([0-9]+)(?:[uUlL]*)\s*$"
    )
    wrapped_pattern = re.compile(
        r"^#define\s+TIMER_CLOCK_HZ\s+ULL_CONST\(([0-9]+)\)\s*$"
    )
    path = timer_header_candidate(build_dir)
    if path is None:
        return None
    for line in path.read_text(encoding="utf-8", errors="strict").splitlines():
        stripped = line.strip()
        match = direct_pattern.match(stripped) or wrapped_pattern.match(stripped)
        if match:
            return int(match.group(1), 10)
    return None


def read_cmake_set(path: Path, symbol: str) -> str | None:
    """Read one quoted or unquoted set() value from generated CMake metadata."""

    pattern = re.compile(
        rf'^set\(\s*{re.escape(symbol)}\s+(?:"([^"]*)"|([^\s)]+))\s*\)$'
    )
    for line in path.read_text(encoding="utf-8", errors="strict").splitlines():
        match = pattern.match(line.strip())
        if match:
            return match.group(1) if match.group(1) is not None else match.group(2)
    return None


def validate_compilers(
    build_dir: Path,
    cross_prefix: str,
    languages: Sequence[str],
    toolchain: Mapping[str, Any],
) -> tuple[dict[str, list[dict[str, str]]], list[str]]:
    """Validate generated compiler metadata and the resolved binary identity."""

    all_expected = {
        "C": f"{cross_prefix}gcc",
        "CXX": f"{cross_prefix}g++",
        "ASM": f"{cross_prefix}gcc",
    }
    expected = {language: all_expected[language] for language in languages}
    expected_target = str(toolchain.get("target_triple", ""))
    expected_version = str(toolchain.get("version", ""))
    compiler_contract = toolchain.get("compiler")
    if not isinstance(compiler_contract, dict):
        return {}, ["profile contract compiler identity table is missing"]
    try:
        compiler_bin = contract_repo_path(
            compiler_contract.get("bin_path"),
            "toolchain.compiler.bin_path",
        )
    except ProfileError as exc:
        return {}, [str(exc)]
    expected_hash_by_basename = {
        f"{cross_prefix}gcc": str(compiler_contract.get("gcc_sha256", "")),
        f"{cross_prefix}g++": str(compiler_contract.get("gxx_sha256", "")),
    }
    evidence: dict[str, list[dict[str, str]]] = {}
    errors: list[str] = []
    for language, expected_basename in expected.items():
        symbol = f"CMAKE_{language}_COMPILER"
        candidates = sorted(
            build_dir.glob(f"CMakeFiles/*/CMake{language}Compiler.cmake")
        )
        records: list[dict[str, str]] = []
        evidence[language] = records
        if not candidates:
            errors.append(
                f"generated CMake metadata is missing {symbol} for compiler proof"
            )
            continue
        for candidate in candidates:
            value = read_cmake_set(candidate, symbol)
            if value is None:
                errors.append(f"generated {candidate} does not define {symbol}")
                continue
            command_path = Path(value).expanduser()
            basename = command_path.name
            resolved = command_path.resolve() if command_path.is_absolute() else None
            metadata_version = read_cmake_set(candidate, f"{symbol}_VERSION")
            record: dict[str, str] = {
                "metadata": str(candidate),
                "metadata_sha256": sha256_file(candidate),
                "command": value,
                "basename": basename,
                "resolved": str(resolved) if resolved is not None else "",
                "metadata_version": metadata_version or "",
                "expected_basename": expected_basename,
                "expected_command": str(compiler_bin / expected_basename),
                "expected_target_triple": expected_target,
                "expected_version": expected_version,
            }
            records.append(record)
            if not command_path.is_absolute():
                errors.append(f"{symbol} is not an absolute command: {value!r}")
            elif command_path != compiler_bin / expected_basename:
                errors.append(
                    f"{symbol} is not from the pinned compiler bin: expected "
                    f"{compiler_bin / expected_basename}, got {command_path}"
                )
            if basename != expected_basename:
                errors.append(
                    f"{symbol} mismatch: expected basename {expected_basename!r}, "
                    f"got {basename!r} from {candidate}"
                )
            if resolved is None or not resolved.is_file():
                errors.append(
                    f"{symbol} resolved compiler is missing: "
                    f"{str(resolved) if resolved is not None else value}"
                )
                continue
            if not os.access(resolved, os.X_OK):
                errors.append(f"{symbol} resolved compiler is not executable: {resolved}")
                continue
            record["resolved_sha256"] = sha256_file(resolved)
            expected_hash = expected_hash_by_basename[expected_basename]
            record["expected_sha256"] = expected_hash
            if record["resolved_sha256"] != expected_hash:
                errors.append(
                    f"{symbol} compiler binary digest mismatch: expected "
                    f"{expected_hash}, got {record['resolved_sha256']}"
                )
            try:
                live_version = run_checked((str(resolved), "-dumpfullversion")).stdout.strip()
                live_target = run_checked((str(resolved), "-dumpmachine")).stdout.strip()
                version_banner = run_checked((str(resolved), "--version")).stdout.splitlines()
            except ProfileError as exc:
                errors.append(f"cannot inspect {symbol} identity: {exc}")
                continue
            record["live_version"] = live_version
            record["target_triple"] = live_target
            record["version_banner"] = version_banner[0] if version_banner else ""
            if metadata_version and metadata_version != expected_version:
                errors.append(
                    f"{symbol} metadata version mismatch: expected "
                    f"{expected_version!r}, got {metadata_version!r}"
                )
            if live_version != expected_version:
                errors.append(
                    f"{symbol} compiler version mismatch: expected "
                    f"{expected_version!r}, got {live_version!r}"
                )
            if live_target != expected_target:
                errors.append(
                    f"{symbol} compiler target mismatch: expected "
                    f"{expected_target!r}, got {live_target!r}"
                )
    return evidence, errors


def validate_build(
    contract: Mapping[str, Any],
    profile_name: str,
    build_dir: Path,
    *,
    contract_path: Path | None = None,
    source_root: Path | None = None,
    require_source: bool = False,
    require_artifacts: bool = False,
    for_release: bool = False,
    for_runtime: bool = False,
    check_build_stamp: bool = True,
) -> dict[str, Any]:
    """Validate generated configuration against one profile contract."""

    canonical_name, profile = get_profile(contract, profile_name)
    if for_release:
        require_source = True
        require_artifacts = True
    build_dir = build_dir.expanduser().resolve()
    errors: list[str] = []
    cohesix_evidence, cohesix_errors = cohesix_repository_evidence()
    errors.extend(cohesix_errors)
    cache_path = build_dir / "CMakeCache.txt"
    cache: dict[str, str] = {}
    generated_path: Path | None = None
    generated: dict[str, Any] = {}

    if not build_dir.is_dir():
        errors.append(f"seL4 build directory does not exist: {build_dir}")
    if not cache_path.is_file():
        errors.append(f"CMake cache is missing: {cache_path}")
    else:
        try:
            cache = parse_cmake_cache(cache_path)
        except ProfileError as exc:
            errors.append(str(exc))

    if build_dir.is_dir():
        try:
            generated_path = find_generated_config(build_dir)
            generated = load_generated_config(generated_path)
        except ProfileError as exc:
            errors.append(str(exc))

    if for_release and not profile.get("release_eligible", False):
        errors.append(
            f"profile {canonical_name} is {profile.get('evidence_class')} evidence "
            "and is not release eligible"
        )
    if for_runtime and not profile.get("runtime_eligible", False):
        errors.append(f"profile {canonical_name} is not runtime eligible")

    forbidden_keys = profile.get("forbidden_cache_keys", [])
    if not isinstance(forbidden_keys, list):
        errors.append(f"profile {canonical_name} has invalid forbidden_cache_keys")
        forbidden_keys = []
    for forbidden in forbidden_keys:
        matches = [
            key
            for key in cache
            if key == forbidden or key.startswith(f"{forbidden}-")
        ]
        if matches:
            errors.append(
                f"forbidden legacy CMake cache key {forbidden} is present: {matches}"
            )

    memoization_cache_values = {
        key: cache.get(key) for key in ("SEL4_CACHE_DIR", "MEMOIZE_CACHE_DIR")
    }
    if profile.get("memoization_cache") != "disabled":
        errors.append(
            f"profile {canonical_name} does not disable seL4 binary memoization"
        )
    for key, actual in memoization_cache_values.items():
        if actual is None:
            errors.append(f"CMake cache is missing required disabled cache key {key}")
        elif actual != "":
            errors.append(
                f"seL4 memoization cache must be disabled; {key}={actual!r}"
            )

    expected_cache = profile.get("cmake", {})
    if not isinstance(expected_cache, dict):
        errors.append(f"profile {canonical_name} has invalid CMake expectations")
        expected_cache = {}
    cache_values: dict[str, dict[str, Any]] = {}
    for key, expected in expected_cache.items():
        actual = cache.get(key)
        cache_values[key] = {"expected": expected, "actual": actual}
        if actual is None:
            errors.append(f"CMake cache is missing required key {key}")
        elif not expected_matches(actual, expected):
            errors.append(f"CMake {key} mismatch: expected {expected!r}, got {actual!r}")

    build_mode = str(profile.get("build_mode", ""))
    cmake_home = cache.get("CMAKE_HOME_DIRECTORY", "")
    python_tool_record: dict[str, Any] | None = None
    objcopy_wrapper_record: dict[str, Any] | None = None
    mkimage_tool_record: dict[str, Any] | None = None
    host_inputs_record: dict[str, Any] | None = None
    build_input_stamp_path = build_dir / BUILD_INPUT_STAMP_NAME
    build_input_stamp: dict[str, Any] | None = file_evidence(build_input_stamp_path)
    observed_build_inputs: dict[str, Any] | None = None
    if build_input_stamp_path.is_file():
        try:
            loaded_build_inputs = json.loads(
                build_input_stamp_path.read_text(encoding="utf-8")
            )
        except (OSError, json.JSONDecodeError) as exc:
            if check_build_stamp:
                errors.append(f"cannot load profile build-input stamp: {exc}")
        else:
            if isinstance(loaded_build_inputs, dict):
                observed_build_inputs = loaded_build_inputs
                build_input_stamp["observed"] = observed_build_inputs
            elif check_build_stamp:
                errors.append("profile build-input stamp is not a JSON object")
    if build_mode == "wrapper":
        actual_home = Path(cmake_home).expanduser().resolve() if cmake_home else None
        expected_home = WRAPPER_PROJECT.resolve()
        if actual_home != expected_home:
            errors.append(
                "CMAKE_HOME_DIRECTORY is not the Cohesix seL4 profile wrapper: "
                f"expected {expected_home}, got {cmake_home or '<missing>'}"
            )
        current_wrapper_sha = sha256_file(WRAPPER_CMAKE)
        configured_wrapper_sha = cache.get("COHESIX_SEL4_WRAPPER_SHA256")
        if configured_wrapper_sha != current_wrapper_sha:
            errors.append(
                "configured wrapper digest mismatch: expected current "
                f"{current_wrapper_sha}, got {configured_wrapper_sha or '<missing>'}"
            )
        try:
            python_tool = resolve_profile_tool(
                profile,
                "python_tool",
                preserve_symlink=True,
            )
        except ProfileError as exc:
            errors.append(str(exc))
            python_tool = None
        if python_tool is not None:
            python_tool_record = file_evidence(python_tool)
            python_tool_record["launcher_path"] = str(python_tool)
            configured_python = cache.get("PYTHON3")
            python_tool_record["configured_path"] = configured_python
            if configured_python != str(python_tool):
                errors.append(
                    "PYTHON3 is not the profile-bound Python environment: "
                    f"expected {python_tool}, got {configured_python or '<missing>'}"
                )
            try:
                python_version = run_checked((str(python_tool), "--version"))
                run_checked(
                    (
                        str(python_tool),
                        "-c",
                        "import google.protobuf, jsonschema, libarchive, lxml, yaml",
                    )
                )
            except ProfileError as exc:
                errors.append(f"profile-bound Python environment is incomplete: {exc}")
            else:
                python_tool_record["version"] = (
                    python_version.stdout or python_version.stderr
                ).strip()
                python_tool_record["required_modules"] = [
                    "jsonschema",
                    "libarchive",
                    "lxml",
                    "yaml",
                    "google.protobuf",
                ]
        try:
            objcopy_wrapper = resolve_profile_tool(
                profile,
                "objcopy_stdout_wrapper",
            )
        except ProfileError as exc:
            errors.append(str(exc))
            objcopy_wrapper = None
        if objcopy_wrapper is not None:
            objcopy_wrapper_record = file_evidence(objcopy_wrapper)
            configured_objcopy = cache.get("CMAKE_OBJCOPY")
            objcopy_wrapper_record["configured_path"] = configured_objcopy
            if configured_objcopy != str(objcopy_wrapper):
                errors.append(
                    "CMAKE_OBJCOPY is not the profile-bound stdout wrapper: "
                    f"expected {objcopy_wrapper}, got {configured_objcopy or '<missing>'}"
                )
        try:
            mkimage_tool = resolve_profile_tool(profile, "mkimage_tool")
        except ProfileError as exc:
            errors.append(str(exc))
            mkimage_tool = None
        if mkimage_tool is not None:
            mkimage_tool_record = file_evidence(mkimage_tool)
            try:
                mkimage_version = run_checked((str(mkimage_tool), "-V"))
            except ProfileError as exc:
                errors.append(f"cannot inspect mkimage_tool identity: {exc}")
            else:
                mkimage_tool_record["version"] = (
                    mkimage_version.stdout or mkimage_version.stderr
                ).strip()
        try:
            validated_host_inputs = wrapper_build_inputs(
                contract,
                canonical_name,
                profile,
            )
        except ProfileError as exc:
            errors.append(f"cannot verify profile-bound host inputs: {exc}")
        else:
            host_inputs_record = validated_host_inputs
            if python_tool_record is not None:
                python_tool_record["supply_chain"] = validated_host_inputs[
                    "python_tool"
                ]
            if mkimage_tool_record is not None:
                mkimage_tool_record["supply_chain"] = validated_host_inputs[
                    "mkimage_tool"
                ]
    elif build_mode == "verified-config":
        if not cmake_home or Path(cmake_home).name != "kernel":
            errors.append(
                "proof-eligibility build was not configured from the pinned upstream "
                f"kernel source: {cmake_home or '<missing>'}"
            )
        try:
            host_inputs_record = verified_config_build_inputs(
                contract,
                canonical_name,
                profile,
            )
        except ProfileError as exc:
            errors.append(f"cannot verify profile-bound host inputs: {exc}")
        else:
            python_tool_record = host_inputs_record["python_tool"]
    else:
        errors.append(f"unsupported build mode in profile {canonical_name}: {build_mode!r}")

    compiler_evidence: dict[str, list[dict[str, str]]] | None = None
    toolchain = contract.get("toolchain")
    if not isinstance(toolchain, dict):
        errors.append("profile contract has no toolchain identity table")
        toolchain = {}
    cross_prefix = expected_cache.get("CROSS_COMPILER_PREFIX")
    if not isinstance(cross_prefix, str) or not cross_prefix:
        if build_mode == "wrapper":
            errors.append(
                f"profile {canonical_name} has no bound CROSS_COMPILER_PREFIX"
            )
    else:
        compiler_evidence, compiler_errors = validate_compilers(
            build_dir,
            cross_prefix,
            ("C", "CXX", "ASM") if build_mode == "wrapper" else ("C", "ASM"),
            toolchain,
        )
        errors.extend(compiler_errors)

    expected_generated = profile.get("generated", {})
    if not isinstance(expected_generated, dict):
        errors.append(f"profile {canonical_name} has invalid generated expectations")
        expected_generated = {}
    generated_values: dict[str, dict[str, Any]] = {}
    for key, expected in expected_generated.items():
        generated_values[key] = {"expected": expected, "actual": generated.get(key)}
        if key not in generated:
            errors.append(f"generated kernel config is missing required key {key}")
            continue
        actual = generated[key]
        if not expected_matches(actual, expected):
            errors.append(
                f"generated {key} mismatch: expected {expected!r}, got {actual!r}"
            )

    try:
        dts_paths = declared_dts_paths(build_dir, profile)
    except ProfileError as exc:
        errors.append(str(exc))
        dts_paths = []
    required_dts = profile.get("required_dts_literals", [])
    forbidden_dts = profile.get("forbidden_dts_literals", [])
    dts_evidence: dict[str, dict[str, Any]] = {}
    if not isinstance(required_dts, list) or not isinstance(forbidden_dts, list):
        errors.append(f"profile {canonical_name} has invalid DTS literal expectations")
    else:
        for path in dts_paths:
            record = file_evidence(path)
            dts_evidence[str(path)] = record
            if not path.is_file():
                errors.append(f"declared generated DTS is missing: {path}")
                continue
            text = path.read_text(encoding="utf-8", errors="strict")
            record["required_literals"] = list(required_dts)
            record["forbidden_literals"] = list(forbidden_dts)
            for literal in required_dts:
                if literal not in text:
                    errors.append(
                        f"generated DTS is missing required literal {literal!r}: {path}"
                    )
            for literal in forbidden_dts:
                if literal in text:
                    errors.append(
                        f"generated DTS contains forbidden literal {literal!r}: {path}"
                    )

    launcher_gic: dict[str, Any] | None = None
    qemu_gic = profile.get("qemu_gic_version")
    if qemu_gic is not None:
        source_selector = cache.get("QEMU_GIC_VERSION")
        if source_selector is None:
            errors.append("CMake cache is missing required key QEMU_GIC_VERSION")
        elif str(source_selector) != str(qemu_gic):
            errors.append(
                "CMake QEMU_GIC_VERSION mismatch: "
                f"expected {qemu_gic!r}, got {source_selector!r}"
            )
        expected_kernel_gic_v3 = int(qemu_gic) == 3
        actual_kernel_gic_v3 = cache.get("KernelArmGicV3")
        if actual_kernel_gic_v3 is None:
            errors.append("CMake cache is missing derived key KernelArmGicV3")
        elif not expected_matches(actual_kernel_gic_v3, expected_kernel_gic_v3):
            errors.append(
                "derived KernelArmGicV3 does not match QEMU_GIC_VERSION: "
                f"expected {expected_kernel_gic_v3}, got {actual_kernel_gic_v3!r}"
            )
        qemu_machine = cache.get("QEMU_MACHINE", "")
        expected_fragment = f"gic-version={qemu_gic}"
        if expected_fragment not in qemu_machine:
            errors.append(
                f"QEMU_MACHINE must contain {expected_fragment!r}; got {qemu_machine!r}"
            )
        config_header = launcher_gic_header(build_dir)
        launcher_gic = {
            "detector": file_evidence(GIC_DETECTOR),
            "config_header": file_evidence(config_header) if config_header else None,
            "expected_version": int(qemu_gic),
            "source_selector": source_selector,
            "derived_kernel_gic_v3": actual_kernel_gic_v3,
            "detected_version": None,
        }
        if config_header is None:
            errors.append(
                "generated GIC config header is missing at every path inspected "
                "by the Cohesix QEMU launchers"
            )
        else:
            try:
                detected_gic = detect_launcher_gic_version(config_header)
            except ProfileError as exc:
                errors.append(str(exc))
            else:
                launcher_gic["detected_version"] = detected_gic
                if detected_gic != int(qemu_gic):
                    errors.append(
                        "QEMU launcher GIC inference mismatch: "
                        f"expected {qemu_gic}, got {detected_gic} from {config_header}"
                    )

    expected_timer = profile.get("timer_clock_hz")
    if expected_timer is not None:
        actual_timer = read_timer_clock_hz(build_dir)
        if actual_timer != expected_timer:
            errors.append(
                f"TIMER_CLOCK_HZ mismatch: expected {expected_timer}, got {actual_timer}"
            )

    elfloader_platform_info: dict[str, Any] | None = None
    if str(expected_cache.get("ElfloaderRootserversLast", "")).upper() == "ON":
        platform_info_path = (
            build_dir / "elfloader" / "gen_headers" / "platform_info.h"
        )
        elfloader_platform_info = file_evidence(platform_info_path)
        if platform_info_path.is_file():
            platform_info_text = platform_info_path.read_text(
                encoding="utf-8",
                errors="strict",
            )
            has_memory_regions = "memory_region" in platform_info_text
            elfloader_platform_info["has_memory_regions"] = has_memory_regions
            if require_artifacts and not has_memory_regions:
                errors.append(
                    "ElfloaderRootserversLast requires a generated memory_region "
                    f"declaration: {platform_info_path}"
                )
        elif require_artifacts:
            errors.append(
                "ElfloaderRootserversLast platform_info.h is missing after build: "
                f"{platform_info_path}"
            )

    artifacts: dict[str, dict[str, Any]] = {}
    for label, candidates in artifact_candidates(build_dir, profile).items():
        selected = next((path for path in candidates if path.is_file()), None)
        if selected is None:
            if require_artifacts:
                errors.append(
                    f"required {label} artifact is missing; tried "
                    + ", ".join(str(path) for path in candidates)
                )
            continue
        size = selected.stat().st_size
        if require_artifacts and size == 0:
            errors.append(f"required {label} artifact is empty: {selected}")
        artifacts[label] = {
            "path": str(selected),
            "size": size,
            "sha256": sha256_file(selected),
        }

    artifact_policy_evidence, artifact_policy_errors = validate_artifact_policy(
        profile,
        artifacts,
        require_artifacts=require_artifacts,
    )
    errors.extend(artifact_policy_errors)
    dtb_evidence, dtb_errors = validate_dtb_semantics(
        profile,
        artifacts,
        require_artifacts=require_artifacts,
    )
    errors.extend(dtb_errors)

    source_evidence: dict[str, Any] | None = None
    requested_source = (
        source_root.expanduser().resolve() if source_root is not None else None
    )
    cached_source_value = cache.get("COHESIX_SEL4_PROJECT_ROOT")
    configured_source = (
        Path(cached_source_value).expanduser().resolve()
        if cached_source_value
        else None
    )
    if build_mode == "verified-config" and cmake_home:
        kernel_home = Path(cmake_home).expanduser().resolve()
        if kernel_home.name == "kernel":
            configured_source = kernel_home.parent
    if build_mode == "wrapper" and requested_source is not None:
        if configured_source is None:
            errors.append(
                "CMake cache does not bind this wrapper build to "
                "COHESIX_SEL4_PROJECT_ROOT"
            )
        elif configured_source != requested_source:
            errors.append(
                "configured source root does not match the requested source proof: "
                f"configured {configured_source}, requested {requested_source}"
            )
    if build_mode == "verified-config" and requested_source is not None:
        if configured_source != requested_source:
            errors.append(
                "verified-config source root does not match the requested source "
                f"proof: configured {configured_source}, requested {requested_source}"
            )

    effective_source = requested_source
    if require_source:
        effective_source = configured_source
    elif effective_source is None and configured_source is not None:
        if configured_source.is_dir():
            effective_source = configured_source

    if build_mode == "verified-config" and effective_source is not None:
        expected_kernel_home = (effective_source / "kernel").resolve()
        actual_kernel_home = (
            Path(cmake_home).expanduser().resolve() if cmake_home else None
        )
        if actual_kernel_home != expected_kernel_home:
            errors.append(
                "proof-eligibility CMake home does not match the verified source: "
                f"expected {expected_kernel_home}, got {cmake_home or '<missing>'}"
            )
    if effective_source is not None:
        source_evidence = validate_source(contract, profile, effective_source)
        errors.extend(source_evidence["errors"])
    elif require_source:
        errors.append(
            "source verification was required but no complete pinned project checkout "
            "was supplied or available from COHESIX_SEL4_PROJECT_ROOT"
        )

    if require_artifacts and check_build_stamp:
        if observed_build_inputs is None:
            errors.append(
                "completed profile build-input stamp is missing after artifact build: "
                f"{build_input_stamp_path}"
            )
        else:
            commands = observed_build_inputs.get("commands")
            observed_jobs = commands.get("jobs") if isinstance(commands, dict) else None
            freshness = observed_build_inputs.get("causal_freshness")
            observed_build_start = (
                freshness.get("build_start")
                if isinstance(freshness, dict)
                else None
            )
            if not isinstance(observed_build_start, dict):
                errors.append(
                    "profile build-input stamp lacks causal fresh-build evidence"
                )
            if not isinstance(observed_jobs, int) or isinstance(observed_jobs, bool) or observed_jobs < 1:
                errors.append("profile build-input stamp has invalid build parallelism")
            elif isinstance(observed_build_start, dict):
                stamp_source = source_evidence
                stamp_source_root = effective_source
                if stamp_source is None:
                    observed_source = observed_build_inputs.get("source")
                    if isinstance(observed_source, dict):
                        stamp_source = observed_source
                        observed_root = observed_source.get("root")
                        if isinstance(observed_root, str) and observed_root:
                            stamp_source_root = Path(observed_root)
                if stamp_source is None or stamp_source_root is None:
                    errors.append(
                        "profile build-input stamp cannot be tied to source provenance"
                    )
                else:
                    try:
                        expected_build_inputs = profile_build_stamp(
                            contract,
                            canonical_name,
                            profile,
                            stamp_source_root,
                            build_dir,
                            stamp_source,
                            build_start=observed_build_start,
                            jobs=observed_jobs,
                            status="complete",
                            require_outputs=True,
                        )
                    except ProfileError as exc:
                        errors.append(f"cannot verify profile build-input stamp: {exc}")
                    else:
                        build_input_stamp["expected"] = expected_build_inputs
                        if observed_build_inputs != expected_build_inputs:
                            errors.append(
                                "profile build-input stamp does not match current "
                                "source, commands, configuration, tools, or artifacts"
                            )

    contract_record: dict[str, Any] = {
        "values_sha256": canonical_sha256(contract),
        "schema_version": contract.get("schema_version"),
        "profile_values": profile,
    }
    if contract_path is not None:
        contract_record["file"] = file_evidence(contract_path)
        if not contract_record["file"]["exists"]:
            errors.append(f"profile contract file is missing: {contract_path}")
    generated_headers: list[dict[str, Any]] = []
    header_paths = [launcher_gic_header(build_dir), timer_header_candidate(build_dir)]
    for header_path in header_paths:
        if header_path is not None and all(
            record["path"] != str(header_path.resolve()) for record in generated_headers
        ):
            generated_headers.append(file_evidence(header_path))

    configuration = {
        "cmake_cache": {
            **file_evidence(cache_path),
            "validated_values": cache_values,
            "forbidden_keys": {
                key: sorted(
                    candidate
                    for candidate in cache
                    if candidate == key or candidate.startswith(f"{key}-")
                )
                for key in forbidden_keys
            },
            "memoization_cache": memoization_cache_values,
        },
        "generated_config": {
            **(file_evidence(generated_path) if generated_path else {"exists": False}),
            "validated_values": generated_values,
        },
        "generated_headers": generated_headers,
        "elfloader_platform_info": elfloader_platform_info,
        "build_input_stamp": build_input_stamp,
        "dts": dts_evidence,
        "dtb": dtb_evidence,
    }
    claim_eligibility = {
        "profile_configuration_for_release": bool(
            profile.get("release_eligible", False)
        ),
        "runtime": bool(profile.get("runtime_eligible", False)),
        "artifact_set_shipping": bool(
            artifact_policy_evidence.get("shipping_eligible", False)
        ),
        "cohesix_system_image": bool(
            artifact_policy_evidence.get("cohesix_system_image", False)
        ),
    }
    wrapper_record: dict[str, Any] | None = None
    if build_mode == "wrapper":
        wrapper_record = file_evidence(WRAPPER_CMAKE)
        wrapper_record["configured_sha256"] = cache.get(
            "COHESIX_SEL4_WRAPPER_SHA256"
        )
    evidence = {
        "schema": "cohesix-sel4-profile-evidence/v2",
        "profile": canonical_name,
        "description": profile.get("description"),
        "evidence_class": profile.get("evidence_class"),
        "claim_eligibility": claim_eligibility,
        "build_mode": build_mode,
        "build_dir": str(build_dir),
        "cohesix_repository": cohesix_evidence,
        "inputs": {
            "contract": contract_record,
            "validator": file_evidence(VALIDATOR),
            "wrapper": wrapper_record,
            "python_tool": python_tool_record,
            "objcopy_stdout_wrapper": objcopy_wrapper_record,
            "mkimage_tool": mkimage_tool_record,
            "host_inputs": host_inputs_record,
            "gic_detector": file_evidence(GIC_DETECTOR)
            if qemu_gic is not None
            else None,
        },
        "configuration": configuration,
        "launcher_gic": launcher_gic,
        "compilers": compiler_evidence,
        "artifacts": artifacts,
        "artifact_policy": artifact_policy_evidence,
        "source": source_evidence,
        "requirements": {
            "source": require_source,
            "artifacts": require_artifacts,
            "release": for_release,
            "runtime": for_runtime,
        },
        "valid": not errors,
        "errors": errors,
    }
    return evidence


def validate_all_builds(
    contract: Mapping[str, Any],
    *,
    base_dir: Path = ROOT,
    contract_path: Path | None = None,
    source_root: Path | None = None,
    require_source: bool = False,
    require_artifacts: bool = False,
    diagnostic_relaxed: bool = False,
    for_release: bool = False,
    for_runtime: bool = False,
) -> dict[str, Any]:
    """Validate every profile at its deterministic default build directory."""

    if not diagnostic_relaxed:
        require_source = True
        require_artifacts = True
    profiles = contract.get("profiles")
    if not isinstance(profiles, dict):
        raise ProfileError("profile contract profiles table is invalid")
    evidence_by_profile: dict[str, dict[str, Any]] = {}
    for profile_name in sorted(profiles):
        profile = profiles[profile_name]
        if not isinstance(profile, dict):
            raise ProfileError(f"seL4 profile {profile_name!r} is not a table")
        default_build_dir = profile.get("default_build_dir")
        if not isinstance(default_build_dir, str) or not default_build_dir:
            raise ProfileError(
                f"seL4 profile {profile_name!r} has no default_build_dir"
            )
        build_dir = Path(default_build_dir).expanduser()
        if not build_dir.is_absolute():
            build_dir = base_dir / build_dir
        evidence_by_profile[profile_name] = validate_build(
            contract,
            profile_name,
            build_dir,
            contract_path=contract_path,
            source_root=source_root,
            require_source=require_source,
            require_artifacts=require_artifacts,
            for_release=for_release,
            for_runtime=for_runtime,
        )
    failed = [
        name for name, evidence in evidence_by_profile.items() if not evidence["valid"]
    ]
    return {
        "schema": "cohesix-sel4-profile-evidence-set/v2",
        "base_dir": str(base_dir.expanduser().resolve()),
        "requirements": {
            "source": require_source,
            "artifacts": require_artifacts,
            "diagnostic_relaxed": diagnostic_relaxed,
            "release": for_release,
            "runtime": for_runtime,
        },
        "valid": not failed,
        "failed_profiles": failed,
        "profiles": evidence_by_profile,
    }


def ensure_safe_transient_build_dir(build_dir: Path) -> Path:
    """Refuse to configure directly into tracked seL4 reference trees."""

    resolved = build_dir.expanduser().resolve()
    if resolved == ROOT or is_relative_to(resolved, TRACKED_SEL4_ROOT.resolve()):
        raise ProfileError(
            "profile configuration must use a transient external or out/ build "
            f"directory, not the tracked reference tree: {resolved}"
        )
    return resolved


def ensure_fresh_build_dir(build_dir: Path) -> Path:
    """Require a new or empty transient build directory before configuration."""

    resolved = ensure_safe_transient_build_dir(build_dir)
    if resolved.exists() and not resolved.is_dir():
        raise ProfileError(f"profile build path is not a directory: {resolved}")
    if resolved.is_dir():
        try:
            first_entry = next(resolved.iterdir(), None)
        except OSError as exc:
            raise ProfileError(f"cannot inspect profile build directory: {exc}") from exc
        if first_entry is not None:
            raise ProfileError(
                "profile configuration requires a new or empty build directory; "
                f"refusing to reuse {resolved}"
            )
    return resolved


def wrapper_configure_command(
    profile: Mapping[str, Any],
    source_root: Path,
    build_dir: Path,
) -> list[str]:
    """Construct a shell-free CMake configure command for a wrapper profile."""

    cmake_values = profile.get("cmake")
    if not isinstance(cmake_values, dict):
        raise ProfileError("profile CMake table is invalid")
    command = [
        "cmake",
        "-G",
        "Ninja",
        "-S",
        str(WRAPPER_PROJECT),
        "-B",
        str(build_dir),
        f"-DCMAKE_TOOLCHAIN_FILE={source_root / 'kernel' / 'gcc.cmake'}",
        f"-DCOHESIX_SEL4_PROJECT_ROOT={source_root}",
        "-DSEL4_CACHE_DIR=",
    ]
    python_tool = resolve_profile_tool(
        profile,
        "python_tool",
        preserve_symlink=True,
    )
    if python_tool is not None:
        command.append(f"-DPYTHON3={python_tool}")
    objcopy_wrapper = resolve_profile_tool(profile, "objcopy_stdout_wrapper")
    if objcopy_wrapper is not None:
        command.append(f"-DCMAKE_OBJCOPY={objcopy_wrapper}")
    for key in sorted(cmake_values):
        command.append(f"-D{key}={cmake_values[key]}")
    qemu_gic = profile.get("qemu_gic_version")
    if qemu_gic is not None:
        dtb = build_dir / "qemu-arm-virt.dtb"
        qemu_machine = (
            "virt,secure=off,virtualization=on,"
            f"gic-version={qemu_gic},dumpdtb={dtb}"
        )
        command.extend(
            (
                f"-DQEMU_GIC_VERSION={qemu_gic}",
                f"-DQEMU_MACHINE={qemu_machine}",
            )
        )
    return command


def configure_wrapper_profile(
    contract: Mapping[str, Any],
    profile_name: str,
    source_root: Path,
    build_dir: Path,
    *,
    dry_run: bool,
) -> list[str]:
    """Configure a wrapper profile into a transient build directory."""

    _canonical, profile = get_profile(contract, profile_name)
    if profile.get("build_mode") != "wrapper":
        raise ProfileError(
            "verified-config profiles are built with the build subcommand because "
            "the upstream CMake script configures and builds atomically"
        )
    source_root = source_root.expanduser().resolve()
    build_dir = ensure_fresh_build_dir(build_dir)
    source_evidence = validate_source(contract, profile, source_root)
    if source_evidence["errors"]:
        raise ProfileError("; ".join(source_evidence["errors"]))
    command = wrapper_configure_command(profile, source_root, build_dir)
    if not dry_run:
        build_dir.mkdir(parents=True, exist_ok=True)
        completed = run_checked(
            command,
            env=wrapper_build_environment(contract, profile),
        )
        if completed.stdout:
            print(completed.stdout, end="")
        if completed.stderr:
            print(completed.stderr, end="", file=sys.stderr)
    return command


def verified_config_build_command(
    profile: Mapping[str, Any],
    source_root: Path,
) -> list[str]:
    """Construct the upstream verified-config command with a pinned compiler."""

    relative = profile.get("verified_config")
    if not isinstance(relative, str) or not relative:
        raise ProfileError("verified-config profile is missing verified_config")
    config_path = source_root / relative
    if not config_path.is_file():
        raise ProfileError(f"upstream verified configuration is missing: {config_path}")
    cmake_values = profile.get("cmake")
    if not isinstance(cmake_values, dict):
        raise ProfileError("verified-config profile CMake table is invalid")
    cross_prefix = cmake_values.get("CROSS_COMPILER_PREFIX")
    if not isinstance(cross_prefix, str) or not cross_prefix:
        raise ProfileError(
            "verified-config profile must bind CROSS_COMPILER_PREFIX"
        )
    return [
        "cmake",
        "-P",
        str(config_path),
        "FORCE",
        f"-DCROSS_COMPILER_PREFIX={cross_prefix}",
        "-DSEL4_CACHE_DIR=",
        "-DMEMOIZE_CACHE_DIR=",
    ]


def wrapper_build_command(build_dir: Path, jobs: int) -> list[str]:
    """Construct the wrapper build command for its declared artifact set."""

    return [
        "cmake",
        "--build",
        str(build_dir),
        "--target",
        "rootserver_image",
        "--parallel",
        str(jobs),
    ]


def wrapper_path_prefixes(
    contract: Mapping[str, Any],
    profile: Mapping[str, Any],
) -> list[Path]:
    """Return declared host-tool directories in deterministic PATH order."""

    toolchain = contract.get("toolchain")
    compiler = toolchain.get("compiler") if isinstance(toolchain, dict) else None
    path_prefixes = compiler.get("path_prefixes") if isinstance(compiler, dict) else None
    if not isinstance(path_prefixes, list) or not path_prefixes:
        raise ProfileError("profile compiler path_prefixes are invalid")
    prefixes: list[Path] = []
    if profile.get("build_mode") == "wrapper":
        cpio_input = gnu_cpio_supply_chain_input(contract)
        cpio_executable = cpio_input["executable"]
        cpio_path = Path(str(cpio_executable["path"]))
        prefixes.append(cpio_path.parent)
    for index, value in enumerate(path_prefixes):
        candidate = contract_repo_path(
            value,
            f"toolchain.compiler.path_prefixes[{index}]",
        )
        if not candidate.is_dir():
            raise ProfileError(
                f"profile compiler PATH prefix is unavailable: {candidate}"
            )
        prefixes.append(candidate)
    tools = (
        resolve_profile_tool(profile, "python_tool", preserve_symlink=True),
        resolve_profile_tool(profile, "mkimage_tool"),
    )
    for tool in tools:
        if tool is None:
            continue
        parent = tool.parent
        if parent not in prefixes:
            prefixes.append(parent)
    mkimage_tool = tools[1]
    if mkimage_tool is not None and mkimage_tool.name != "mkimage":
        raise ProfileError(
            f"profile mkimage_tool must be named mkimage: {mkimage_tool}"
        )
    return prefixes


def wrapper_build_environment(
    contract: Mapping[str, Any],
    profile: Mapping[str, Any],
    base: Mapping[str, str] | None = None,
) -> dict[str, str] | None:
    """Bind profile-declared host packaging tools into the build environment."""

    environment = dict(os.environ if base is None else base)
    environment["SEL4_CACHE_DIR"] = ""
    prefixes = wrapper_path_prefixes(contract, profile)
    current_path = environment.get("PATH", "")
    environment["PATH"] = os.pathsep.join(str(path) for path in prefixes)
    if current_path:
        if environment["PATH"]:
            environment["PATH"] += os.pathsep
        environment["PATH"] += current_path
    return environment


def build_profile(
    contract: Mapping[str, Any],
    profile_name: str,
    source_root: Path,
    build_dir: Path,
    *,
    jobs: int,
    dry_run: bool,
) -> list[str]:
    """Build a configured wrapper profile or pristine verified configuration."""

    canonical, profile = get_profile(contract, profile_name)
    source_root = source_root.expanduser().resolve()
    build_dir = ensure_safe_transient_build_dir(build_dir)
    source_evidence = validate_source(contract, profile, source_root)
    if source_evidence["errors"]:
        raise ProfileError("; ".join(source_evidence["errors"]))

    build_mode = profile.get("build_mode")
    env: dict[str, str] | None = None
    build_start: dict[str, Any] | None = None
    if build_mode == "wrapper":
        if not (build_dir / "CMakeCache.txt").is_file() and not dry_run:
            raise ProfileError(
                f"wrapper profile is not configured: {build_dir}; run configure first"
            )
        if not dry_run:
            build_start = require_fresh_profile_build_start(build_dir, profile)
            preflight = validate_build(
                contract,
                canonical,
                build_dir,
                source_root=source_root,
                check_build_stamp=False,
            )
            if not preflight["valid"]:
                raise ProfileError(
                    "configured wrapper tree does not match the selected profile: "
                    + "; ".join(preflight["errors"])
                )
        command = wrapper_build_command(build_dir, jobs)
        env = wrapper_build_environment(contract, profile)
    elif build_mode == "verified-config":
        build_dir = ensure_fresh_build_dir(build_dir)
        if not dry_run:
            build_start = require_fresh_profile_build_start(build_dir, profile)
        command = verified_config_build_command(profile, source_root)
        env = wrapper_build_environment(contract, profile)
        env["CMAKE_BUILD_PARALLEL_LEVEL"] = str(jobs)
        env["SEL4_CACHE_DIR"] = ""
    else:
        raise ProfileError(f"unsupported build mode: {build_mode!r}")

    if not dry_run:
        if build_start is None:
            raise ProfileError("profile build did not record a fresh build start")
        build_dir.mkdir(parents=True, exist_ok=True)
        build_inputs = profile_build_stamp(
            contract,
            canonical,
            profile,
            source_root,
            build_dir,
            source_evidence,
            build_start=build_start,
            jobs=jobs,
            status="pending",
            require_outputs=False,
        )
        write_wrapper_build_input_stamp(build_dir, build_inputs)
        completed = run_checked(command, cwd=build_dir, env=env)
        completed_source_evidence = validate_source(contract, profile, source_root)
        if completed_source_evidence["errors"]:
            raise ProfileError(
                "source changed or became invalid during profile build: "
                + "; ".join(completed_source_evidence["errors"])
            )
        build_inputs = profile_build_stamp(
            contract,
            canonical,
            profile,
            source_root,
            build_dir,
            completed_source_evidence,
            build_start=build_start,
            jobs=jobs,
            status="complete",
            require_outputs=True,
        )
        write_wrapper_build_input_stamp(build_dir, build_inputs)
        if completed.stdout:
            print(completed.stdout, end="")
        if completed.stderr:
            print(completed.stderr, end="", file=sys.stderr)
    return command


def write_evidence(path: Path, evidence: Mapping[str, Any]) -> None:
    """Write deterministic JSON evidence to an explicitly selected path."""

    path = path.expanduser()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def positive_jobs(value: str) -> int:
    """Validate a positive build parallelism value for argparse."""

    try:
        result = int(value, 10)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("jobs must be an integer") from exc
    if result < 1:
        raise argparse.ArgumentTypeError("jobs must be positive")
    return result


def add_common_contract_argument(parser: argparse.ArgumentParser) -> None:
    """Add the shared contract-path argument."""

    parser.add_argument(
        "--contract",
        type=Path,
        default=DEFAULT_CONTRACT,
        help=f"profile contract (default: {DEFAULT_CONTRACT})",
    )


def parse_arguments(argv: Iterable[str] | None = None) -> argparse.Namespace:
    """Parse command-line arguments."""

    parser = argparse.ArgumentParser(
        description="Configure, build, or validate Cohesix seL4 profile contracts",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    list_parser = subparsers.add_parser("list", help="list available profiles")
    add_common_contract_argument(list_parser)

    validate_parser = subparsers.add_parser(
        "validate",
        help="validate an existing generated build tree",
    )
    add_common_contract_argument(validate_parser)
    validate_selection = validate_parser.add_mutually_exclusive_group(required=True)
    validate_selection.add_argument("--profile")
    validate_selection.add_argument(
        "--all",
        action="store_true",
        help="validate every profile at its contract default_build_dir",
    )
    validate_parser.add_argument("--build-dir", type=Path)
    validate_parser.add_argument("--source", type=Path)
    validate_parser.add_argument("--require-source", action="store_true")
    validate_parser.add_argument("--require-artifacts", action="store_true")
    validate_parser.add_argument(
        "--diagnostic-relaxed",
        action="store_true",
        help=(
            "with --all, permit configuration-only diagnostics without requiring "
            "source and artifacts"
        ),
    )
    validate_parser.add_argument("--for-release", action="store_true")
    validate_parser.add_argument("--for-runtime", action="store_true")
    validate_parser.add_argument(
        "--repo-managed",
        action="store_true",
        help=(
            "validate the profile's immutable repository-managed artifact tree "
            "without requiring its historical absolute build path"
        ),
    )
    validate_parser.add_argument("--evidence", type=Path)

    prepare_parser = subparsers.add_parser(
        "prepare-source",
        help="materialize a profile-declared source policy in a pinned checkout",
    )
    add_common_contract_argument(prepare_parser)
    prepare_parser.add_argument("--profile", required=True)
    prepare_parser.add_argument("--source", type=Path, required=True)
    prepare_parser.add_argument("--dry-run", action="store_true")

    configure_parser = subparsers.add_parser(
        "configure",
        help="configure a wrapper profile in a transient build tree",
    )
    add_common_contract_argument(configure_parser)
    configure_parser.add_argument("--profile", required=True)
    configure_parser.add_argument("--source", type=Path, required=True)
    configure_parser.add_argument("--build-dir", type=Path, required=True)
    configure_parser.add_argument("--dry-run", action="store_true")

    build_parser = subparsers.add_parser(
        "build",
        help="build a configured wrapper or pristine proof-eligibility profile",
    )
    add_common_contract_argument(build_parser)
    build_parser.add_argument("--profile", required=True)
    build_parser.add_argument("--source", type=Path, required=True)
    build_parser.add_argument("--build-dir", type=Path, required=True)
    build_parser.add_argument(
        "--jobs",
        type=positive_jobs,
        default=max(1, os.cpu_count() or 1),
    )
    build_parser.add_argument("--dry-run", action="store_true")

    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Iterable[str] | None = None) -> int:
    """Run the profile management CLI."""

    args = parse_arguments(argv)
    try:
        contract = load_contract(args.contract)
        if args.command == "list":
            profiles = contract["profiles"]
            for name in sorted(profiles):
                profile = profiles[name]
                print(
                    f"{name}\t{profile['evidence_class']}\t"
                    f"{profile['target']}\t{profile['description']}"
                )
            return 0

        if args.command == "validate":
            if args.all:
                if args.repo_managed:
                    raise ProfileError("--repo-managed cannot be combined with --all")
                if args.build_dir is not None:
                    raise ProfileError(
                        "--build-dir cannot be combined with --all; "
                        "edit default_build_dir in the contract instead"
                    )
                evidence = validate_all_builds(
                    contract,
                    contract_path=args.contract,
                    source_root=args.source,
                    require_source=args.require_source,
                    require_artifacts=args.require_artifacts,
                    diagnostic_relaxed=args.diagnostic_relaxed,
                    for_release=args.for_release,
                    for_runtime=args.for_runtime,
                )
            else:
                if args.diagnostic_relaxed:
                    raise ProfileError("--diagnostic-relaxed requires --all")
                if args.build_dir is None:
                    raise ProfileError("--build-dir is required with --profile")
                if args.repo_managed:
                    if args.source is not None or args.require_source:
                        raise ProfileError(
                            "--repo-managed validates relocated artifacts; source "
                            "validation belongs to a fresh source-build lane"
                        )
                    if args.for_release:
                        raise ProfileError(
                            "--repo-managed artifacts are not release proof"
                        )
                    evidence = validate_repo_managed_build(
                        contract,
                        args.profile,
                        args.build_dir,
                        for_runtime=args.for_runtime,
                    )
                else:
                    evidence = validate_build(
                        contract,
                        args.profile,
                        args.build_dir,
                        contract_path=args.contract,
                        source_root=args.source,
                        require_source=args.require_source,
                        require_artifacts=args.require_artifacts,
                        for_release=args.for_release,
                        for_runtime=args.for_runtime,
                    )
            if args.evidence:
                write_evidence(args.evidence, evidence)
            if not evidence["valid"]:
                if args.all:
                    for name in evidence["failed_profiles"]:
                        profile_evidence = evidence["profiles"][name]
                        for error in profile_evidence["errors"]:
                            print(
                                f"[sel4-profile] ERROR [{name}]: {error}",
                                file=sys.stderr,
                            )
                    print(
                        "[sel4-profile] FAIL "
                        f"profiles={len(evidence['profiles'])} "
                        f"failed={len(evidence['failed_profiles'])}",
                        file=sys.stderr,
                    )
                else:
                    for error in evidence["errors"]:
                        print(f"[sel4-profile] ERROR: {error}", file=sys.stderr)
                return 1
            if args.all:
                print(
                    f"[sel4-profile] PASS profiles={len(evidence['profiles'])} "
                    f"base={evidence['base_dir']}"
                )
                return 0
            print(
                f"[sel4-profile] PASS profile={evidence['profile']} "
                f"class={evidence['evidence_class']} build={evidence['build_dir']}"
            )
            return 0

        if args.command == "prepare-source":
            preparation = prepare_source(
                contract,
                args.profile,
                args.source,
                dry_run=args.dry_run,
            )
            print(json.dumps(preparation, sort_keys=True))
            return 0

        if args.command == "configure":
            command = configure_wrapper_profile(
                contract,
                args.profile,
                args.source,
                args.build_dir,
                dry_run=args.dry_run,
            )
            if args.dry_run:
                print(json.dumps(command))
            return 0

        if args.command == "build":
            command = build_profile(
                contract,
                args.profile,
                args.source,
                args.build_dir,
                jobs=args.jobs,
                dry_run=args.dry_run,
            )
            if args.dry_run:
                print(json.dumps(command))
            return 0
    except ProfileError as exc:
        print(f"[sel4-profile] ERROR: {exc}", file=sys.stderr)
        return 2

    print(f"[sel4-profile] ERROR: unsupported command {args.command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
