#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Retain exact native and Mac Metal release observations with explicit proof limits for provider conformance.
# Copyright 2026 Lukas Bower

"""Run selected matrix contracts or native observations with separate proof classes."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

from cohesix.native_providers import discover
from cohesix.providers import ProviderUnavailable, registry


def main() -> int:
    """Run selected bounded native observations into a fresh evidence directory."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument(
        "--matrix",
        type=Path,
        default=Path(__file__).resolve().parents[2]
        / "configs/provider_conformance.toml",
    )
    parser.add_argument(
        "--validate-only",
        action="store_true",
        help="Validate matrix and record selection without executing tests",
    )
    group = parser.add_mutually_exclusive_group()
    for name in [
        "executors",
        "evidence",
        "observability",
        "packaging",
        "identity",
        "registry",
        "perf",
    ]:
        group.add_argument(
            "--" + name + "-only", dest="group", action="store_const", const=name
        )
    group.add_argument(
        "--playbooks", dest="group", action="store_const", const="playbooks"
    )
    parser.add_argument("--host-profile", default="jetson-orin-nano-jp7")
    parser.add_argument("--case", action="append", help="Run one exact matrix case")
    parser.add_argument(
        "--reference-config", type=Path,
        help="Private exact-source configuration for a selected live case",
    )
    parser.add_argument("--provider", action="append")
    parser.add_argument("--native-providers", action="store_true")
    parser.add_argument("--live-reference", action="store_true")
    parser.add_argument("--gpu-bridge", type=Path)
    parser.add_argument("--cuda-build", type=Path)
    parser.add_argument("--execution-lane", choices=["systemd", "nvidia-container"])
    parser.add_argument("--execution-user")
    parser.add_argument("--sidecar-bridge", type=Path)
    parser.add_argument("--container-image")
    parser.add_argument("--opendnp3-source", type=Path)
    parser.add_argument("--opendnp3-library", type=Path)
    parser.add_argument(
        "--workload-ipc-contract",
        action="store_true",
        help="Real native CUDA with fixture admission; no root/Worker proof",
    )
    for name in ["source-agent", "source-manifest", "source-policy", "target-manifest"]:
        parser.add_argument("--" + name, type=Path)
    for name in [
        "source-url",
        "target-url",
        "source-auth-ref",
        "source-ticket-ref",
        "target-auth-ref",
        "target-ticket-ref",
        "native-unit",
    ]:
        parser.add_argument("--" + name)
    args = parser.parse_args()
    contract = registry()
    from provider_matrix import load_matrix, run_matrix

    try:
        matrix = load_matrix(args.matrix, contract)
    except (ValueError, KeyError, TypeError, OSError) as exc:
        parser.error("invalid conformance matrix: " + str(exc))
    if args.case and (
        len(args.case) != 1
        or args.group
        or args.provider
        or args.native_providers
        or args.live_reference
    ):
        parser.error("--case requires one unique id and no group, provider or native selector")
    if args.case and args.case[0] in {"m28-jobs-live", "m28-authority-live"}:
        if not args.reference_config or args.validate_only:
            parser.error("M28 live cases require --reference-config and real execution")
        from provider_m28_live import run_live

        try:
            return run_live(args.case[0], args.reference_config, args.host_profile, args.state_dir)
        except (ValueError, OSError, KeyError, TypeError) as exc:
            parser.error(str(exc))
    if args.case and args.case[0] in {"m28a-workloads-live", "m28a-recovery-live"}:
        if not args.reference_config or args.validate_only:
            parser.error("M28a live workload requires --reference-config and real execution")
        from provider_m28a_live import run_live

        try:
            return run_live(args.case[0], args.reference_config, args.host_profile, args.state_dir)
        except (ValueError, OSError, KeyError, TypeError) as exc:
            parser.error(str(exc))
    if args.case and args.case[0] in {"m28b-peft-live", "m28b-serving-live"}:
        if not args.reference_config or args.validate_only:
            parser.error("M28b live PEFT requires --reference-config and real execution")
        from provider_m28b_live import run_live

        try:
            return run_live(args.case[0], args.reference_config, args.host_profile, args.state_dir)
        except (ValueError, OSError, KeyError, TypeError) as exc:
            parser.error(str(exc))
    if args.case and args.case[0] == "m28c-platform-live":
        if not args.reference_config or args.validate_only:
            parser.error("M28c platform case requires --reference-config and real execution")
        from provider_m28c_platform import run_live

        try:
            return run_live(args.case[0], args.reference_config, args.host_profile,
                            args.state_dir)
        except (ValueError, OSError, KeyError, TypeError, subprocess.TimeoutExpired) as exc:
            parser.error(str(exc))
    if args.case and args.case[0] in {"m28c1-mlx-live", "m28c1-vmlx-live"}:
        if not args.reference_config or args.validate_only:
            parser.error("M28c1 Mac release requires --reference-config and real execution")
        from provider_m28c1_live import run_live

        try:
            return run_live(args.case[0], args.reference_config, args.host_profile,
                            args.state_dir)
        except (ValueError, OSError, KeyError, TypeError, subprocess.TimeoutExpired) as exc:
            parser.error(str(exc))
    if args.reference_config:
        parser.error("--reference-config is only valid for a selected M28 through M28c1 live case")
    if args.provider == ["mac_release"] and args.live_reference:
        if args.group or args.validate_only or args.native_providers:
            parser.error("live macOS release has its own owned Xcode lane")
        from provider_macos_release import run_native

        return run_native(args.state_dir)
    if args.provider == ["launchd"] and args.live_reference:
        if args.group or args.validate_only or args.native_providers:
            parser.error("live launchd has its own owned native service lane")
        from provider_launchd import run_native

        return run_native(args.state_dir)
    if args.provider in (["modbus"], ["dnp3"]) and args.live_reference:
        if (
            args.group
            or args.validate_only
            or args.native_providers
            or not args.opendnp3_source
            or not args.opendnp3_library
        ):
            parser.error(
                "field-bus native reference requires only its provider, --live-reference and exact OpenDNP3 source/library"
            )
        import asyncio
        import os
        from field_bus_conformance import run

        os.umask(0o077)
        asyncio.run(run(args))
        return 0
    if args.provider == ["federation"] and args.live_reference:
        if args.group or args.validate_only or args.native_providers:
            parser.error("live federation has its own proof lane")
        from provider_federation import run_live

        try:
            return run_live(args)
        except (ValueError, OSError) as exc:
            parser.error(str(exc))
    if (
        args.case
        or args.group
        or args.validate_only
        or args.provider
        in (["federation"], ["launchd"], ["mac_release"], ["endpoint_compliance"])
        or (args.provider in (["modbus"], ["dnp3"]) and not args.native_providers)
        or not (args.native_providers or args.provider or args.live_reference)
    ):
        if args.native_providers or args.live_reference:
            parser.error("matrix host contracts and native discovery use separate runs")
        if args.provider and len(args.provider) != 1:
            parser.error("matrix provider selection accepts exactly one id")
        try:
            return run_matrix(
                matrix,
                contract,
                args.state_dir,
                args.group,
                args.provider[0] if args.provider else None,
                args.validate_only,
                args.case[0] if args.case else None,
            )
        except (ValueError, OSError) as exc:
            parser.error(str(exc))
    selected = args.provider or ["jetson", "network", "docker"]
    if len(selected) > 32 or len(set(selected)) != len(selected):
        parser.error("provider selection must contain at most 32 unique ids")
    if args.host_profile not in matrix["profiles"]:
        parser.error("native profile is not selected in the matrix")
    provider_ids = {item["id"] for item in contract["contract"]["families"]}
    if any(item not in provider_ids for item in selected):
        parser.error("provider is not registered")
    if "gpu.workload" in selected:
        if (
            selected != ["gpu.workload"]
            or not args.live_reference
            or not args.gpu_bridge
            or not args.cuda_build
        ):
            parser.error(
                "GPU reference requires only --provider gpu.workload, --live-reference, --gpu-bridge and --cuda-build"
            )
        if args.workload_ipc_contract:
            from provider_gpu_workload import run_workload_ipc

            summary = run_workload_ipc(args.gpu_bridge, args.cuda_build, args.state_dir)
        elif args.execution_lane:
            if not args.execution_user:
                parser.error("deployment lane requires --execution-user")
            from provider_execution_lane import run_lane

            summary = run_lane(
                args.execution_lane,
                args.gpu_bridge,
                args.cuda_build,
                args.state_dir,
                user=args.execution_user,
                sidecar=args.sidecar_bridge,
                image=args.container_image,
            )
        else:
            from provider_cuda_reference import run_reference

            summary = run_reference(args.gpu_bridge, args.cuda_build, args.state_dir)
        print(
            json.dumps(
                {
                    "reference_result": summary["reference_result"],
                    "result": "INCOMPLETE",
                    "summary": str(args.state_dir / "summary.json"),
                }
            )
        )
        return 2
    args.state_dir.mkdir(parents=True, exist_ok=False, mode=0o700)
    results = []
    for provider_id in selected:
        try:
            observation = discover(
                provider_id, args.host_profile, time.time_ns() // 1_000_000
            )
            result = (
                "OBSERVED"
                if not observation["profile_mismatches"]
                else "PROFILE_MISMATCH"
            )
        except (ProviderUnavailable, ValueError, KeyError) as exc:
            observation = {"error": getattr(exc, "code", "invalid_observation")}
            result = "UNAVAILABLE"
        raw = (json.dumps(observation, sort_keys=True, indent=2) + "\n").encode()
        filename = provider_id + ".json"
        (args.state_dir / filename).write_bytes(raw)
        results.append(
            {
                "provider_id": provider_id,
                "result": result,
                "observation": filename,
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )
    source = subprocess.run(
        ["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=False
    )
    summary = {
        "schema": "cohesix-provider-conformance-run/v1",
        "phase": "discover",
        "claiming": False,
        "production_proven": False,
        "result": "INCOMPLETE",
        "profile_id": args.host_profile,
        "graph_sha256": contract["graph_sha256"],
        "source_commit": source.stdout.strip() if source.returncode == 0 else None,
        "results": results,
        "missing_phases": [
            "preflight",
            "execute",
            "observe",
            "verify",
            "compensate",
            "export_evidence",
        ],
    }
    (args.state_dir / "summary.json").write_text(
        json.dumps(summary, sort_keys=True, indent=2) + "\n"
    )
    print(
        json.dumps(
            {
                "result": summary["result"],
                "summary": str(args.state_dir / "summary.json"),
            }
        )
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
