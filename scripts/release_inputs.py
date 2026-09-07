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
    result_path: Path,
    source: str,
    host: str,
    expected_manifest_sha256: str | None = None,
) -> dict[str, Any]:
    """Require an immutable passing TCP result for this exact native artifact."""

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


def main() -> int:
    """Resolve tested artifacts or verify the bytes copied into a host bundle."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--result", type=Path, required=True)
    parser.add_argument("--source-digest", required=True)
    parser.add_argument("--host", choices=HOSTS, required=True)
    parser.add_argument("--bundle", type=Path)
    args = parser.parse_args()
    try:
        artifact = verified_inputs(
            args.artifact,
            args.result,
            args.source_digest,
            args.host,
            evidence.sha256_file(
                Path(__file__).resolve().parents[1] / "configs/root_task.toml"
            ),
        )
        if args.bundle is None:
            print(artifact["_resolved_artifact_root"])
        else:
            records = payload_records(artifact)
            verify_payload(args.bundle, records)
            evidence.atomic_write_json(
                args.bundle / "BUILD_PROVENANCE.json",
                {
                    "schema": "cohesix-release-build-provenance/v1",
                    "host": args.host,
                    "source_commit": subprocess.check_output(
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
                    "result_sha256": evidence.sha256_file(args.result),
                    "sel4_profile": artifact["sel4"]["profile"],
                    "timer_clock_hz": artifact["sel4"]["timer_clock_hz"],
                    "files": records,
                    "packaging_is_target_acceptance": False,
                },
            )
    except (evidence.EvidenceError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"release-inputs: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
