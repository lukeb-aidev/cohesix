# Author: Lukas Bower
# Purpose: Retain native Xcode result identities for an owned release fixture without distribution credentials.
# Copyright 2026 Lukas Bower
"""Focused macOS release adapter lane; no signing or App Store publication."""

from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess


def run_native(state: Path) -> int:
    """Build, test and archive one disposable native framework with XcodeGen."""
    if platform.system() != "Darwin":
        raise ValueError("not_supported macos-release-host")
    generator = shutil.which("xcodegen")
    if generator is None:
        raise ValueError("not_enabled xcodegen-reference-dependency")
    root = Path(__file__).resolve().parents[2]
    state = state.resolve()
    state.mkdir(parents=True, exist_ok=False, mode=0o700)
    source = state / "source"
    shutil.copytree(root / "tests/fixtures/macos_release", source)
    results = state / "results"
    results.mkdir(mode=0o700)
    report = {
        "schema": "cohesix-macos-release-conformance/v1",
        "status": "FAIL",
        "proof_class": "native_provider_contract",
        "host": platform.platform(),
        "root_admission": False,
        "use_case_qualified": False,
        "distribution_credentials_used": False,
        "graph_sha256": json.loads(
            (root / "configs/generated/provider_registry.json").read_bytes()
        )["graph_sha256"],
    }
    try:
        with (state / "generate.log").open("xb") as log:
            subprocess.run(
                [generator, "generate", "--spec", str(source / "project.yml")],
                cwd=source,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
                timeout=30,
            )
        report["generator_sha256"] = hashlib.sha256(
            Path(generator).resolve().read_bytes()
        ).hexdigest()
        environment = dict(
            os.environ,
            COHESIX_MACOS_RELEASE_REFERENCE=str(source),
            COHESIX_MACOS_RELEASE_RESULTS=str(results),
        )
        with (state / "native-test.log").open("xb") as log:
            subprocess.run(
                [
                    "cargo",
                    "test",
                    "--locked",
                    "-p",
                    "host-sidecar-bridge",
                    "--test",
                    "native_macos_release",
                    "owned_xcode_release_results",
                    "--",
                    "--ignored",
                    "--exact",
                    "--nocapture",
                ],
                cwd=root,
                env=environment,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
                timeout=600,
            )
        output = (state / "native-test.log").read_text()
        records = [
            json.loads(line)
            for line in output.splitlines()
            if line.startswith('{"action":')
        ]
        if [r["action"] for r in records] != [
            "mac_release.build",
            "mac_release.test",
            "mac_release.archive",
        ]:
            raise ValueError("missing native Xcode results")
        if "test result: ok. 1 passed; 0 failed; 0 ignored" not in output:
            raise ValueError("missing native test completion")
        report.update(
            status="PASS", observations=records, duplicate_attempt_refused=True
        )
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report["error"] = str(error)
    (state / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps({"status": report["status"], "summary": str(state / "summary.json")})
    )
    return 0 if report["status"] == "PASS" else 1
