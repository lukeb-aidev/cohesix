#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Install the verified QEMU 10.1.0 HVF/GIC fix in a separate user-owned Mac prefix.
# Copyright 2026 Lukas Bower
"""Build an optional QEMU runtime when the current Homebrew binary fails startup."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

from qemu_compat import inspect

VERSION = "10.1.0"
ARCHIVE_URL = "https://download.qemu.org/qemu-10.1.0.tar.xz"
ARCHIVE_SHA256 = "e0517349b50ca73ebec2fa85b06050d5c463ca65c738833bd8fc1f15f180be51"
PATCH_SHA256 = "513faf8c91ee5075aa3c47eeecab305abe3c67d41eb281739ad56fa8bf9a6c42"
HVF_SOURCE_SHA256 = "e3f90397e30485c8bae74db604f0b0b9d114628141b8a76193d221ad7a3e1252"
PATCH = """--- a/target/arm/hvf/hvf.c
+++ b/target/arm/hvf/hvf.c
@@ -2053,6 +2053,8 @@ static int hvf_handle_exception(CPUState *cpu, hv_vcpu_exit_exception_t *excp)
         uint64_t val;
         int sysreg_ret = 0;
\x20
+        cpu_synchronize_state(cpu);
+
         if (isread) {
             sysreg_ret = hvf_sysreg_read(cpu, reg, &val);
             if (!sysreg_ret) {
"""
DEPENDENCIES = ("capstone", "dtc", "glib", "gnutls", "libpng", "libslirp",
                "libssh", "meson", "ninja", "pixman", "pkgconf", "zstd")


def digest(path: Path) -> str:
    """Hash a regular local input in bounded chunks."""
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"QEMU input is not a regular file: {path}")
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


def checked(command: list[str], *, cwd: Path | None = None,
            env: dict[str, str] | None = None, log: Path | None = None) -> str:
    """Run one fixed build command and retain diagnostics outside installed bytes."""
    result = subprocess.run(command, cwd=cwd, env=env, text=True,
                            capture_output=True, timeout=3600, check=False)
    output = result.stdout + result.stderr
    if log is not None:
        with log.open("a", encoding="utf-8") as stream:
            stream.write(f"$ {command[0]}\n{output}\n")
    if result.returncode:
        raise ValueError(f"QEMU setup failed in {command[0]}; log: {log}: {output[-1600:]}")
    return result.stdout


def fetch_archive(path: Path) -> None:
    """Download only the pinned upstream release and enforce its exact digest."""
    with urllib.request.urlopen(ARCHIVE_URL, timeout=60) as response, path.open("xb") as target:
        total = 0
        while block := response.read(1024 * 1024):
            total += len(block)
            if total > 150 * 1024 * 1024:
                raise ValueError("QEMU source archive exceeds the download bound")
            target.write(block)
    if digest(path) != ARCHIVE_SHA256:
        raise ValueError("QEMU source archive hash differs from the pinned release")


def prepare_source(archive: Path, work: Path, log: Path) -> Path:
    """Extract the verified upstream source, then apply the exact reviewed fix."""
    if archive.stat().st_size > 150 * 1024 * 1024 or digest(archive) != ARCHIVE_SHA256:
        raise ValueError("QEMU source archive hash differs from the pinned release")
    source = work / f"qemu-{VERSION}"
    with tarfile.open(archive, "r:xz") as package:
        members = []
        for member in package:
            parts = Path(member.name).parts
            if not parts or parts[0] != source.name or ".." in parts:
                raise ValueError("unsafe QEMU source archive member")
            if member.issym():
                target = Path(os.path.normpath(str((work / member.name).parent /
                                                   member.linkname)))
                if not target.is_relative_to(source):
                    # Upstream's optional X11 header shortcut points outside
                    # the archive; the headless build never needs that link.
                    if (member.name ==
                            f"qemu-{VERSION}/roms/edk2/EmulatorPkg/Unix/Host/X11IncludeHack"
                            and member.linkname == "/opt/X11/include"):
                        continue
                    raise ValueError("QEMU source link escapes its pinned tree")
            elif not (member.isfile() or member.isdir()):
                raise ValueError("QEMU source archive contains a special member")
            members.append(member)
        if hasattr(tarfile, "data_filter"):
            package.extractall(work, members=members, filter="data")
        else:
            # The archive hash and every member/link have been validated above.
            package.extractall(work, members=members)
    if hashlib.sha256(PATCH.encode()).hexdigest() != PATCH_SHA256:
        raise ValueError("local QEMU HVF patch has changed")
    result = subprocess.run(["/usr/bin/patch", "-p1"], cwd=source, input=PATCH,
                            text=True, capture_output=True, timeout=30)
    with log.open("a", encoding="utf-8") as stream:
        stream.write(result.stdout + result.stderr)
    if result.returncode or digest(source / "target/arm/hvf/hvf.c") != HVF_SOURCE_SHA256:
        raise ValueError("QEMU HVF patch did not produce the selected source")
    return source


def install(prefix: Path, archive: Path | None) -> Path:
    """Build in a private sibling and publish only a signed, startup-checked QEMU."""
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("the pinned HVF installer requires Apple Silicon macOS")
    if not prefix.is_absolute() or ".." in prefix.parts or prefix.exists():
        raise ValueError("select a new absolute, non-existing QEMU prefix")
    prefix.parent.mkdir(parents=True, exist_ok=True)
    if prefix.parent.resolve(strict=True) != prefix.parent:
        raise ValueError("QEMU prefix parent may not be a symlink")
    if shutil.disk_usage(prefix.parent).free < 2 * 1024 * 1024 * 1024:
        raise ValueError("QEMU source build needs at least 2 GiB free")
    brew = shutil.which("brew")
    if brew is None:
        raise ValueError("Homebrew is needed for the pinned QEMU build dependencies")
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    log = prefix.parent / f"qemu-{VERSION}-{run_id}-{os.getpid()}-build.log"
    log.touch(mode=0o600, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix="cohesix-qemu-", dir=prefix.parent) as temporary:
        work = Path(temporary)
        selected_archive = archive if archive is not None else work / "qemu.tar.xz"
        if archive is None:
            fetch_archive(selected_archive)
        source = prepare_source(selected_archive, work, log)
        for dependency in DEPENDENCIES:
            checked([brew, "install", dependency], log=log)
        brew_root = checked([brew, "--prefix"], log=log).strip()
        dtc = checked([brew, "--prefix", "dtc"], log=log).strip()
        slirp = checked([brew, "--prefix", "libslirp"], log=log).strip()
        env = os.environ.copy()
        env["PKG_CONFIG_PATH"] = ":".join((
            f"{slirp}/lib/pkgconfig", f"{dtc}/lib/pkgconfig",
            f"{brew_root}/lib/pkgconfig", env.get("PKG_CONFIG_PATH", ""),
        ))
        for variable in ("CFLAGS", "CXXFLAGS", "OBJCFLAGS"):
            env[variable] = f"-I{dtc}/include -O2"
        env["LDFLAGS"] = f"-L{dtc}/lib"
        build = work / "build"
        build.mkdir()
        checked([str(source / "configure"), f"--prefix={prefix}",
                 "--target-list=aarch64-softmmu", f"--python={sys.executable}",
                 "--enable-hvf", "--enable-slirp", "--enable-fdt=system",
                 "--disable-docs", "--disable-tools", "--disable-guest-agent",
                 "--disable-gtk", "--disable-cocoa", "--disable-sdl",
                 "--disable-vnc", "--disable-pvg", "--disable-modules",
                 "--disable-download",
                 "--disable-werror", "--disable-fuse", "--disable-fuse-lseek"],
                cwd=build, env=env, log=log)
        checked(["ninja", "-C", str(build), "-j8", "qemu-system-aarch64"],
                env=env, log=log)
        destination = work / "stage"
        env["DESTDIR"] = str(destination)
        checked(["ninja", "-C", str(build), "install"], env=env, log=log)
        staged = destination / prefix.relative_to("/")
        binary = staged / "bin/qemu-system-aarch64"
        # Meson may copy Finder/resource-fork attributes from the build tree;
        # clear only the staged executable before applying its final signature.
        checked(["/usr/bin/xattr", "-c", str(binary)], log=log)
        checked(["/usr/bin/codesign", "--force", "--sign", "-",
                 "--entitlements", str(source / "accel/hvf/entitlements.plist"),
                 str(binary)], log=log)
        checked(["/usr/bin/codesign", "--verify", "--strict", str(binary)], log=log)
        result = inspect(binary, "macos-hvf")
        if result["version"] != f"QEMU emulator version {VERSION}":
            raise ValueError("built QEMU version differs from the pinned release")
        os.replace(staged, prefix)
    return prefix / "bin/qemu-system-aarch64"


def main() -> None:
    """Expose one optional user-owned QEMU prefix, separate from Cohesix host code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", type=Path, default=Path.home() /
                        ".local/share/cohesix/qemu/10.1.0-hvf-gic-sync")
    parser.add_argument("--source-archive", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if not args.prefix.is_absolute() or ".." in args.prefix.parts:
        parser.error("select an absolute QEMU prefix without parent traversal")
    binary = args.prefix / "bin/qemu-system-aarch64"
    try:
        if args.check:
            result = inspect(binary, "macos-hvf")
            if result["version"] != f"QEMU emulator version {VERSION}":
                raise ValueError("selected QEMU is not the pinned 10.1.0 build")
        else:
            candidates = [args.prefix]
            candidates.extend(sorted(args.prefix.parent.glob(args.prefix.name + "-repair-*"),
                                     reverse=True))
            for candidate in candidates:
                if candidate.is_symlink() or not candidate.is_dir():
                    continue
                selected = candidate / "bin/qemu-system-aarch64"
                try:
                    result = inspect(selected, "macos-hvf")
                    if result["version"] == f"QEMU emulator version {VERSION}":
                        binary = selected
                        break
                except (ValueError, OSError, subprocess.SubprocessError):
                    continue
            else:
                prefix = args.prefix
                if prefix.exists() or prefix.is_symlink():
                    suffix = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
                    prefix = prefix.with_name(f"{prefix.name}-repair-{suffix}-{os.getpid()}")
                binary = install(prefix, args.source_archive)
    except (ValueError, OSError, subprocess.SubprocessError, tarfile.TarError) as exc:
        parser.error(str(exc))
    print(binary)


if __name__ == "__main__":
    main()
