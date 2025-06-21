# CLASSIFICATION: COMMUNITY
# Filename: cohrun.py v0.4
# Author: Lukas Bower
# Date Modified: 2026-01-25
"""cohrun – wrapper for Cohesix demo launcher."""

import argparse
import os
import platform
import shutil
import subprocess
import sys
import shlex
from datetime import datetime
from pathlib import Path
from typing import List
import traceback


LOG_DIR = Path(os.getenv("COHESIX_LOG", Path.home() / ".cohesix" / "log"))
LOG_DIR.mkdir(parents=True, exist_ok=True)


def cohlog(msg: str) -> None:
    with (LOG_DIR / "cli_tool.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {msg}\n")
    print(msg)


def repo_root() -> Path:
    try:
        root = subprocess.check_output([
            "git",
            "rev-parse",
            "--show-toplevel",
        ], text=True).strip()
        return Path(root)
    except Exception:
        return Path.cwd()


def safe_run(cmd: List[str]) -> int:
    quoted = [shlex.quote(c) for c in cmd]
    with (LOG_DIR / "cli_exec.log").open("a") as f:
        f.write(f"{datetime.utcnow().isoformat()} {' '.join(quoted)}\n")
    result = subprocess.run(cmd)
    return result.returncode


def find_latest_iso(root: Path) -> Path | None:
    iso_dir = root / "out"
    iso_files = sorted(iso_dir.glob("*.iso"), key=lambda p: p.stat().st_mtime, reverse=True)
    return iso_files[0] if iso_files else None


def run_qemu(iso: Path, arch: str | None) -> int:
    arch = arch or platform.machine()
    if arch.startswith("arm") or arch.startswith("aarch64"):
        qemu = shutil.which("qemu-system-aarch64")
    else:
        qemu = shutil.which("qemu-system-x86_64")
    if not qemu:
        cohlog("qemu-system not found")
        return 1
    cmd = [qemu, "-cdrom", str(iso), "-nographic", "-m", "512M", "-serial", "mon:stdio", "-net", "none"]
    return safe_run(cmd)


def main():
    parser = argparse.ArgumentParser(description="Run Cohesix demo scenarios")
    parser.add_argument("--man", action="store_true", help="Show man page")
    parser.add_argument("--iso", help="Boot the given ISO in QEMU")
    parser.add_argument("--arch", help="Override architecture for QEMU")
    parser.add_argument(
        "args", nargs=argparse.REMAINDER, help="Arguments passed to rust cohrun"
    )
    opts = parser.parse_args()

    if opts.man:
        man = os.path.join(os.path.dirname(__file__), "../bin/man")
        page = os.path.join(os.path.dirname(__file__), "../docs/man/cohrun.1")
        os.execv(man, [man, page])

    if opts.iso:
        root = repo_root()
        iso_path = Path(opts.iso)
        if iso_path.is_dir():
            latest = find_latest_iso(iso_path)
            if not latest:
                cohlog("no ISO found in directory")
                sys.exit(1)
            iso_path = latest
        elif not iso_path.exists():
            # fallback to latest in out/
            iso_path = find_latest_iso(root)
            if not iso_path:
                cohlog("ISO not found")
                sys.exit(1)
        rc = run_qemu(iso_path, opts.arch)
        sys.exit(rc)

    if not opts.args:
        parser.print_help()
        sys.exit(1)

    bin_path = os.environ.get("COHRUN_BIN")
    if not bin_path:
        bin_path = shutil.which("cohrun") or os.path.join("target", "debug", "cohrun")
    try:
        rc = safe_run([bin_path] + opts.args)
        if rc != 0:
            cohlog(f"cohrun exited with code {rc}")
            sys.exit(rc)
    except FileNotFoundError:
        cohlog("cohrun binary not found")
        sys.exit(1)
    except Exception as e:
        cohlog(f"cohrun failed: {e}")
        sys.exit(1)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        with (LOG_DIR / "cli_error.log").open("a") as f:
            f.write(f"{datetime.utcnow().isoformat()} {traceback.format_exc()}\n")
        cohlog("Unhandled error, see cli_error.log")
        sys.exit(1)
