# Author: Lukas Bower
# Purpose: Own a temporary native macOS service and prove bounded lifecycle and executable-mismatch refusal.
# Copyright 2026 Lukas Bower
"""Native reference only; does not claim Root, Worker, or whole-use-case proof."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import subprocess


def run_native(state: Path) -> int:
    """Build the pinned helper, test one owned service, and always remove that service."""
    if platform.system() != "Darwin":
        raise ValueError("not_supported launchd-host")
    root = Path(__file__).resolve().parents[2]
    state = state.resolve()
    state.mkdir(parents=True, mode=0o700, exist_ok=False)
    label = f"org.cohesix.m27b-owned-{os.getpid()}"
    domain = f"gui/{os.getuid()}"
    helper = state / "process-macos"
    plist = state / "owned.plist"
    report = {
        "schema": "cohesix-launchd-conformance/v1",
        "status": "FAIL",
        "proof_class": "native_provider_contract",
        "root_admission": False,
        "worker_proof": False,
        "use_case_qualified": False,
        "host": platform.platform(),
        "graph_sha256": json.loads(
            (root / "configs/generated/provider_registry.json").read_bytes()
        )["graph_sha256"],
    }
    booted = False
    try:
        with (state / "helper-build.log").open("xb") as log:
            subprocess.run(
                [
                    "/usr/bin/xcrun",
                    "swiftc",
                    "-O",
                    str(root / "scripts/providers/process_macos.swift"),
                    "-o",
                    str(helper),
                ],
                cwd=root,
                stdout=log,
                stderr=subprocess.STDOUT,
                timeout=60,
                check=True,
            )
        report["sdk"] = subprocess.check_output(
            ["/usr/bin/xcrun", "--show-sdk-version"], text=True, timeout=10
        ).strip()
        plist.write_bytes(
            plistlib.dumps(
                {
                    "Label": label,
                    "ProgramArguments": ["/bin/sleep", "120"],
                    "RunAtLoad": False,
                    "KeepAlive": False,
                    "ProcessType": "Background",
                    "StandardOutPath": str(state / "job.stdout"),
                    "StandardErrorPath": str(state / "job.stderr"),
                }
            )
        )
        target = {
            "id": "m27b-owned-job",
            "domain": domain,
            "label": label,
            "plist": str(plist),
            "plist_sha256": hashlib.sha256(plist.read_bytes()).hexdigest(),
            "executable_sha256": hashlib.sha256(
                Path("/bin/sleep").read_bytes()
            ).hexdigest(),
        }
        report["target"] = target
        report["helper_sha256"] = hashlib.sha256(helper.read_bytes()).hexdigest()
        environment = os.environ.copy()
        environment.update(
            {
                "COHESIX_LAUNCHD_CONFORMANCE_TARGET": json.dumps(target),
                "COHESIX_MACOS_PROCESS_HELPER": str(helper),
                "COHESIX_MACOS_PROCESS_HELPER_SHA256": report["helper_sha256"],
            }
        )
        completed = subprocess.run(
            ["/bin/launchctl", "bootstrap", domain, str(plist)],
            capture_output=True,
            timeout=10,
        )
        (state / "bootstrap.stderr").write_bytes(completed.stderr)
        completed.check_returncode()
        booted = True
        with (state / "native-test.log").open("xb") as log:
            subprocess.run(
                [
                    "cargo",
                    "test",
                    "--locked",
                    "-p",
                    "host-sidecar-bridge",
                    "--test",
                    "native_launchd",
                    "owned_service_native_lifecycle",
                    "--",
                    "--ignored",
                    "--exact",
                    "--nocapture",
                ],
                cwd=root,
                env=environment,
                stdout=log,
                stderr=subprocess.STDOUT,
                timeout=240,
                check=True,
            )
        text = (state / "native-test.log").read_text()
        records = [
            json.loads(line)
            for line in text.splitlines()
            if line.startswith('{"action":')
        ]
        if [record["action"] for record in records] != [
            "launchd.start",
            "launchd.restart",
            "launchd.stop",
        ]:
            raise ValueError("missing native lifecycle observations")
        if "test result: ok. 1 passed; 0 failed; 0 ignored" not in text:
            raise ValueError("missing focused native test completion")
        report["observations"] = records
        report["executable_mismatch_refused_before_dispatch"] = True
        report["status"] = "PASS"
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report["error"] = str(error)
    finally:
        if booted:
            try:
                completed = subprocess.run(
                    ["/bin/launchctl", "bootout", f"{domain}/{label}"],
                    capture_output=True,
                    timeout=10,
                )
                (state / "cleanup.stderr").write_bytes(completed.stderr)
                completed.check_returncode()
                report["owned_service_removed"] = True
            except (OSError, subprocess.SubprocessError) as error:
                report.update(status="FAIL", cleanup_error=str(error))
        (state / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps({"status": report["status"], "summary": str(state / "summary.json")})
    )
    return 0 if report["status"] == "PASS" else 1
