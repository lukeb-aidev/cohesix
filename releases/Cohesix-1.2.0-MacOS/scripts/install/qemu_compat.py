#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check that a selected QEMU binary starts the Cohesix HVF or KVM machine envelope.
# Copyright 2026 Lukas Bower
"""Bounded host QEMU startup check; a passing result is not guest acceptance."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def inspect(binary: Path, profile: str, hold_seconds: float = 2.0) -> dict[str, str]:
    """Require an actual four-core accelerator startup, beyond version/help text."""
    if profile not in {"macos-hvf", "linux-kvm"}:
        raise ValueError("unknown QEMU host profile")
    binary = binary.expanduser().resolve(strict=True)
    if not binary.is_file() or binary.stat().st_size > 256 * 1024 * 1024:
        raise ValueError("QEMU binary is missing or exceeds the probe bound")
    version_lines = subprocess.run(
        [str(binary), "--version"], capture_output=True, text=True, timeout=10,
        check=True,
    ).stdout.splitlines()
    if not version_lines:
        raise ValueError("QEMU did not report a version")
    version = version_lines[0]
    if re.fullmatch(r"QEMU emulator version [0-9]+\.[0-9]+\.[0-9]+.*", version) is None:
        raise ValueError("QEMU did not report a recognizable version")
    accelerator = "hvf" if profile == "macos-hvf" else "kvm"
    advertised = subprocess.run(
        [str(binary), "-accel", "help"], capture_output=True, text=True,
        timeout=10, check=True,
    ).stdout.splitlines()
    if accelerator not in advertised:
        raise ValueError(f"QEMU does not advertise {accelerator}")
    command = [str(binary), "-accel", accelerator, "-machine",
               "virt,gic-version=3,virtualization=off" +
               (",kernel-irqchip=off" if profile == "macos-hvf" else ""),
               "-cpu", "cortex-a57" if profile == "macos-hvf"
               else "host,cntfrq=31250000", "-smp",
               "4,cores=4,threads=1,sockets=1", "-S", "-nographic",
               "-nodefaults", "-monitor", "none"]
    process = subprocess.Popen(
        command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE, text=True,
    )
    try:
        time.sleep(hold_seconds)
        if process.poll() is not None:
            _, stderr = process.communicate(timeout=5)
            raise ValueError(
                f"QEMU {accelerator} four-core startup failed: {stderr[:800]}"
            )
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate(timeout=5)
    digest = hashlib.sha256()
    with binary.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
    return {"profile": profile, "binary": str(binary), "sha256": digest.hexdigest(),
            "version": version, "result": "startup-pass-guest-unverified"}


def main() -> None:
    """Return one machine-readable diagnostic for an explicitly selected host."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--profile", required=True, choices=("macos-hvf", "linux-kvm"))
    args = parser.parse_args()
    try:
        result = inspect(args.binary, args.profile)
    except (ValueError, OSError, subprocess.SubprocessError, IndexError) as exc:
        parser.error(str(exc))
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
