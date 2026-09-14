#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Bind release payloads to previously tested native host artifacts.
# Copyright 2026 Lukas Bower

"""Inspect retained target evidence without promoting packaging to target proof."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path
import sys
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent / "ci"))
import qemu_artifact as evidence  # noqa: E402
import authority_release_gate as authority

HOSTS = {
    "macos": ("Darwin", "qemu_smp_production", 24_000_000),
    "linux": ("Linux", "qemu_smp_kvm_production", 31_250_000),
}
IMAGE_PATHS = {
    "image/elfloader": "staging/elfloader",
    "image/kernel.elf": "staging/kernel.elf",
    "image/rootserver": "staging/rootserver",
    "image/cohesix-system.cpio": "cohesix-system.cpio",
    "image/manifest.json": "staging/cohesix/manifest.json",
}


def verified_inputs(
    artifact_path: Path,
    result_path: Path | None,
    source: str,
    host: str,
    expected_manifest_sha256: str | None = None,
    *,
    build_only: bool = False,
) -> dict[str, Any]:
    """Verify native bytes and require TCP proof unless build-only is selected."""

    artifact = evidence.verify_artifact_document(
        artifact_path,
        expected_source_digest=source,
        verify_local_runtime=False,
    )
    if (
        expected_manifest_sha256 is not None
        and artifact["input_manifest"]["sha256"] != expected_manifest_sha256
    ):
        raise evidence.EvidenceError(
            "release requires the selected default manifest, not a regression variant"
        )
    system, profile, clock = HOSTS[host]
    if (
        artifact["qemu"]["host_system"] != system
        or artifact["sel4"]["profile"] != profile
        or artifact["sel4"]["timer_clock_hz"] != clock
        or artifact["qemu"]["claim"].get("eligible") is not True
        or artifact["build"]["cargo_profile"] != "release"
        or artifact["build"]["cargo_target"] != "aarch64-unknown-none"
    ):
        raise evidence.EvidenceError(
            f"{host} release artifact has the wrong production profile"
        )
    authority.validate_policy(authority.read_manifest(
        Path(artifact["_resolved_artifact_root"]) / "release-configs/configs/generated/root_task_resolved.json"
    ))
    if build_only:
        if (
            artifact.get("action_id") != "release.build-only"
            or artifact.get("attempt_manifest") is not None
            or result_path is not None
        ):
            raise evidence.EvidenceError("build-only inputs cannot claim a test attempt or result")
        return artifact
    if result_path is None:
        raise evidence.EvidenceError("tested assembly requires a passing TCP result")
    result = evidence.read_json(result_path)
    root = result.get("evidence_root")
    if not isinstance(root, str) or Path(root).is_absolute():
        raise evidence.EvidenceError(
            "release TCP result requires a relocatable evidence root"
        )
    result = evidence.verify_result_document(
        result_path,
        expected_source_digest=source,
        expected_target="qemu",
        expected_tier="qemu-integration",
        expected_action_id="qemu.tcp-regression",
        expected_catalog_action_digest=artifact["catalog_action_digest"],
        expected_evidence_root=result_path.parent / root,
        verify_local_runtime=False,
    )
    if result.get("artifact", {}).get("artifact_id") != artifact["artifact_id"]:
        raise evidence.EvidenceError(
            "release input differs from the artifact that passed TCP"
        )
    if result.get("group") != "base" or "boot_v0.coh" not in result.get("scripts", []):
        raise evidence.EvidenceError(
            "release requires the default base TCP boot result"
        )
    return artifact


def payload_records(artifact: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Map the tested byte records into their release destinations."""

    by_path = {row["path"]: row for row in artifact["files"]}
    mappings = dict(IMAGE_PATHS)
    for path in evidence.QEMU_REQUIRED_FILES:
        if path.startswith("host-tools/"):
            mappings[f"bin/{Path(path).name}"] = path
    root = Path(artifact["_resolved_artifact_root"])
    for path in evidence.retained_release_config_paths(root):
        if path not in by_path:
            raise evidence.EvidenceError(
                f"release configuration is not bound to the tested artifact: {path}"
            )
        mappings[path.removeprefix("release-configs/")] = path
    key_record = "release-configs/configs/generated/cas_verification_key.hex"
    if key_record not in by_path:
        raise evidence.EvidenceError("release lacks compiler-bound public verification material")
    mappings["resources/keys/cas_verification_key.hex"] = key_record
    profile = evidence.read_json(
        root / "release-configs/configs/generated/cohesix_python_qemu_smp_production.json"
    )
    if profile.get("target") != "qemu" or profile.get("target_profile") != artifact["sel4"]["profile"]:
        raise evidence.EvidenceError("retained Python contract has the wrong native QEMU profile")
    for generated, field in (
        ("root_task_resolved.json", "resolved_manifest"),
        ("cohsh_policy.toml", "policy"),
    ):
        record = by_path.get(f"release-configs/configs/generated/{generated}")
        if record is None or record["sha256"] != artifact[field]["sha256"]:
            raise evidence.EvidenceError(f"retained {generated} differs from the tested configuration")
    return {
        destination: {
            "sha256": by_path[source]["sha256"],
            "size": by_path[source]["size"],
        }
        for destination, source in mappings.items()
    }


def verify_payload(bundle: Path, records: dict[str, dict[str, Any]]) -> None:
    """Reject stale or rebuilt bytes before a bundle receives its manifest."""

    for relative, record in records.items():
        evidence.verify_file_record(bundle, {"path": relative, **record})
    authority.verify_release_public_key(bundle)
    authority.scan_bundle(bundle)


def main() -> int:
    """Resolve tested artifacts or verify the bytes copied into a host bundle."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--release-manifest", type=Path,
                        default=Path(__file__).resolve().parents[1] / "configs/root_task.toml")
    parser.add_argument("--result", type=Path)
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--source-digest", required=True)
    parser.add_argument("--host", choices=HOSTS, required=True)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--publication-manifest", type=Path)
    args = parser.parse_args()
    try:
        publication = None
        if args.build_only and args.publication_manifest is not None:
            raise evidence.EvidenceError("build-only assembly requires the current source identity")
        if args.publication_manifest is not None:
            publication = evidence.read_json(args.publication_manifest)
            if (
                publication.get("schema") != "cohesix-release-publication/v1"
                or publication.get("qualified_source_digest") != args.source_digest
                or publication.get("runtime_sources_unchanged") is not True
                or publication.get("packaging_is_target_acceptance") is not False
            ):
                raise evidence.EvidenceError("invalid publication-only source binding")
        artifact = verified_inputs(
            args.artifact,
            args.result,
            args.source_digest,
            args.host,
            evidence.sha256_file(args.release_manifest),
            build_only=args.build_only,
        )
        records = payload_records(artifact)
        if args.bundle is None:
            print(artifact["_resolved_artifact_root"])
        else:
            verify_payload(args.bundle, records)
            evidence.atomic_write_json(
                args.bundle / "BUILD_PROVENANCE.json",
                {
                    "schema": "cohesix-release-build-provenance/v1",
                    "host": args.host,
                    "source_commit": publication["qualified_source_commit"]
                    if publication is not None else subprocess.check_output(
                        [
                            "git",
                            "-C",
                            str(Path(__file__).resolve().parents[1]),
                            "rev-parse",
                            "HEAD",
                        ],
                        text=True,
                    ).strip(),
                    "source_digest": args.source_digest,
                    "artifact_id": artifact["artifact_id"],
                    "result_sha256": None if args.build_only else evidence.sha256_file(args.result),
                    "assembly_mode": "build-only" if args.build_only else "tested-artifacts",
                    "test_status": "NOT_RUN" if args.build_only else "RECORDED_TCP_PASS",
                    "sel4_profile": artifact["sel4"]["profile"],
                    "timer_clock_hz": artifact["sel4"]["timer_clock_hz"],
                    "files": records,
                    "packaging_is_target_acceptance": False,
                    **({"publication": publication} if publication is not None else {}),
                },
            )
    except (evidence.EvidenceError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"release-inputs: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
