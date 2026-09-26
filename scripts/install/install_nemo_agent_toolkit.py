#!/usr/bin/env python3
"""Install the locked NeMo kit wheel into a fresh isolated Python environment.

Author: Lukas Bower
Purpose: Reproduce the selected Linux client without editable source or private setup.
Copyright 2026 Lukas Bower
"""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import sys


WHEEL = re.compile(r"cohesix_nemo_kit-0\.1\.0-py3-none-any\.whl\Z")
SHA = re.compile(r"[0-9a-f]{64}\Z")


def bounded_sha256(path: Path, maximum: int) -> str:
    """Hash a bounded regular artifact without following a symlink."""
    if not path.is_file() or path.is_symlink() or path.stat().st_size > maximum:
        raise ValueError("install artifact must be a bounded regular file")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def install(venv: Path, wheel: Path, expected_sha256: str,
            lock: Path, expected_lock_sha256: str) -> None:
    """Verify the exact artifact, then create and check a clean locked venv."""
    if not venv.is_absolute() or venv.exists():
        raise ValueError("venv must be an unused absolute path")
    if (not wheel.is_absolute() or not wheel.is_file()
            or not WHEEL.fullmatch(wheel.name) or not SHA.fullmatch(expected_sha256)):
        raise ValueError("wheel path, version or SHA-256 is invalid")
    digest = bounded_sha256(wheel, 32 * 1024 * 1024)
    if digest != expected_sha256:
        raise ValueError("kit wheel SHA-256 mismatch")
    if not lock.is_absolute() or not SHA.fullmatch(expected_lock_sha256):
        raise ValueError("lock needs an absolute path and SHA-256")
    if bounded_sha256(lock, 1024 * 1024) != expected_lock_sha256:
        raise ValueError("dependency lock SHA-256 mismatch")
    if sys.platform != "linux":
        raise ValueError("selected lock is for Linux AArch64")
    import platform
    if platform.machine() != "aarch64":
        raise ValueError("selected lock is for Linux AArch64")
    subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)
    python = str(venv / "bin/python")
    subprocess.run([python, "-m", "pip", "install", "--no-cache-dir",
                    "-r", str(lock)], check=True)
    subprocess.run([python, "-m", "pip", "install", "--no-deps", str(wheel)], check=True)
    subprocess.run([python, "-m", "pip", "check"], check=True)
    subprocess.run([str(venv / "bin/cohesix-nemo"), "--help"], check=True,
                   stdout=subprocess.DEVNULL)


def main() -> None:
    """Accept only a prebuilt, digest-pinned wheel and unused destination."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--venv", type=Path, required=True)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--lock", type=Path, required=True)
    parser.add_argument("--lock-sha256", required=True)
    args = parser.parse_args()
    install(args.venv, args.wheel, args.sha256, args.lock, args.lock_sha256)
    print(f"installed Cohesix NeMo kit 0.1.0 at {args.venv}")


if __name__ == "__main__":
    main()
