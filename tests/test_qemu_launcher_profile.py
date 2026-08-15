# Author: Lukas Bower
# Purpose: Verify QEMU launchers preserve the canonical HVF machine and timer contract.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]
BUILD_RUN = ROOT / "scripts" / "cohesix-build-run.sh"
QEMU_RUN = ROOT / "scripts" / "qemu-run.sh"
RELEASE_BUNDLE = ROOT / "scripts" / "release_bundle.sh"


def _write_executable(path: Path, source: str) -> None:
    path.write_text(source, encoding="utf-8")
    path.chmod(0o755)


def test_build_run_darwin_defaults_to_hvf_profile_envelope(tmp_path: Path) -> None:
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    _write_executable(fake_bin / "uname", "#!/bin/sh\nprintf 'Darwin\\n'\n")
    qemu = fake_bin / "qemu-system-aarch64"
    _write_executable(
        qemu,
        "#!/bin/sh\n"
        "if [ \"$1\" = \"-accel\" ] && [ \"$2\" = \"help\" ]; then\n"
        "  printf 'Accelerators supported in QEMU binary:\\nhvf\\ntcg\\n'\n"
        "fi\n",
    )
    environment = os.environ.copy()
    environment["PATH"] = f"{fake_bin}{os.pathsep}{environment['PATH']}"
    for key in (
        "COHESIX_QEMU_ACCEL",
        "QEMU_ACCEL",
        "COHESIX_QEMU_VIRT",
        "QEMU_VIRT",
        "COHESIX_QEMU_MACHINE_EXTRA",
        "QEMU_MACHINE_EXTRA",
    ):
        environment.pop(key, None)

    result = subprocess.run(
        [
            "bash",
            "-c",
            'source "$1"; QEMU_BIN="$2"; '
            "resolve_qemu_accel; resolve_qemu_virt_arg; "
            'printf "%s\\n" "$QEMU_MACHINE_EXTRA"',
            "launcher-test",
            str(BUILD_RUN),
            str(qemu),
        ],
        cwd=ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr
    assert result.stdout.splitlines() == ["hvf", "off", "kernel-irqchip=off"]


def test_build_run_timer_guard_accepts_match_and_rejects_split(
    tmp_path: Path,
) -> None:
    manifest = tmp_path / "root_task_resolved.json"
    header = tmp_path / "platform_gen.h"
    manifest.write_text(
        json.dumps({"console_network_service": {"timer_clock_hz": 24_000_000}}),
        encoding="utf-8",
    )
    header.write_text(
        "#define TIMER_CLOCK_HZ ULL_CONST(24000000)\n", encoding="utf-8"
    )
    command = [
        "bash",
        "-c",
        'source "$1"; validate_generated_timer_clock "$2" "$3"',
        "timer-test",
        str(BUILD_RUN),
        str(manifest),
        str(header),
    ]

    matching = subprocess.run(
        command,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    assert matching.returncode == 0, matching.stderr
    assert "24000000 Hz" in matching.stdout

    manifest.write_text(
        json.dumps({"console_network_service": {"timer_clock_hz": 62_500_000}}),
        encoding="utf-8",
    )
    split = subprocess.run(
        command,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    assert split.returncode != 0
    assert (
        "console_network_service.timer_clock_hz=62500000, "
        "selected seL4 TIMER_CLOCK_HZ=24000000"
    ) in split.stderr


def test_all_shell_launchers_bind_hvf_off_and_diagnostic_tcg() -> None:
    build_run = BUILD_RUN.read_text(encoding="utf-8")
    qemu_run = QEMU_RUN.read_text(encoding="utf-8")
    release_bundle = RELEASE_BUNDLE.read_text(encoding="utf-8")

    assert 'CANONICAL_QEMU_VIRT="off"' in build_run
    assert 'CANONICAL_QEMU_VIRT="off"' in qemu_run
    assert 'DEFAULT_QEMU_VIRT="off"' in release_bundle
    assert 'echo "hvf"' in build_run
    assert 'echo "hvf"' in qemu_run
    assert 'DEFAULT_QEMU_ACCEL="hvf"' in release_bundle
    assert "virtualization=${QEMU_VIRT_ARG}" in build_run
    assert "virtualization=${QEMU_VIRT_ARG}" in qemu_run
    assert "virtualization=${virt}" in release_bundle
    for source in (build_run, qemu_run, release_bundle):
        assert "cntfrq=24000000" in source or (
            "CANONICAL_QEMU_TIMER_CLOCK_HZ=\"24000000\"" in source
            and "cntfrq=${CANONICAL_QEMU_TIMER_CLOCK_HZ}" in source
        )
        assert "claim-ineligible" in source
        assert "PSCI HVC" not in source

