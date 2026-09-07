#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Qualify extracted native bundles and the distributed Pi SD image before publication.
# Copyright 2026 Lukas Bower

"""Record installation smoke evidence; never substitute it for M26e acceptance."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import signal
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
from typing import Any

from release_inputs import evidence, verify_payload

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "cohesix-release-qualification/v1"
HOST_CHECKS = ["manifest", "archive", "native-tools", "replay", "python", "tcp", "ui"]
PI_CHECKS = ["manifest", "archive", "sd-layout", "media-readback", "fresh-boot", "tcp"]


def digest(path: Path) -> str:
    """Hash a file using the repository's bounded streaming implementation."""
    return evidence.sha256_file(path).removeprefix("sha256:")


def file_record(path: Path) -> dict[str, Any]:
    """Record a file without retaining machine-specific absolute paths."""
    return {"path": path.name, "size": path.stat().st_size, "sha256": digest(path)}


def verify_tested_bytes(bundle: Path, manifest_sha256: str) -> None:
    """Ensure installation checks did not alter the payload they qualified."""
    manifest = bundle / "MANIFEST.sha256"
    if digest(manifest) != manifest_sha256:
        raise ValueError("release manifest changed during qualification")
    for line in manifest.read_text().splitlines():
        expected, relative = line.split("  ", 1)
        path = evidence.safe_relative_file(bundle, relative)
        if digest(path) != expected:
            raise ValueError(
                f"release payload changed during qualification: {relative}"
            )


def inspect_bundle(bundle: Path, archive: Path) -> dict[str, Any]:
    """Compare an extracted archive against the compiler's exact file inventory."""
    inventory = evidence.read_json(
        ROOT / "configs/generated/implementation_surface_inventory.json"
    )
    release = inventory["release"]
    version = release["version"]
    suffix = bundle.name.removeprefix(f"Cohesix-{version}-")
    if (
        suffix not in {"MacOS", "linux", "Pi4"}
        or bundle.name != f"Cohesix-{version}-{suffix}"
    ):
        raise ValueError("bundle name does not match the selected release version")
    if archive.name != f"{bundle.name}.tar.gz":
        raise ValueError("archive name differs from the selected bundle")
    key = "expected_pi4_bundle_files" if suffix == "Pi4" else "expected_bundle_files"
    expected = set(release[key])
    entries = list(bundle.rglob("*"))
    if any(path.is_symlink() for path in entries):
        raise ValueError("extracted bundle contains a symlink")
    actual = {path.relative_to(bundle).as_posix() for path in entries if path.is_file()}
    if actual != expected:
        raise ValueError(
            f"bundle file-set drift: missing={expected - actual}, extra={actual - expected}"
        )
    if (bundle / "VERSION.txt").read_text().strip() != version:
        raise ValueError("VERSION.txt differs from the compiler inventory")
    hashes = {name: digest(bundle / name) for name in expected}
    recorded: dict[str, str] = {}
    for line in (bundle / "MANIFEST.sha256").read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        if match is None or match[2] in recorded:
            raise ValueError("invalid or duplicate release manifest entry")
        recorded[match[2]] = match[1]
    if recorded != {
        name: value for name, value in hashes.items() if name != "MANIFEST.sha256"
    }:
        raise ValueError("release manifest does not match the extracted bytes")
    archived: dict[str, str] = {}
    with tarfile.open(archive, "r:gz") as handle:
        for member in handle:
            parts = PurePosixPath(member.name).parts
            if not parts or parts[0] != bundle.name or ".." in parts:
                raise ValueError("archive contains an unconfined path")
            if member.isdir():
                continue
            name = PurePosixPath(*parts[1:]).as_posix()
            if not member.isfile() or name in archived:
                raise ValueError("archive contains a link, special file or duplicate")
            stream = handle.extractfile(member)
            if stream is None:
                raise ValueError("archive member cannot be read")
            value = hashlib.sha256()
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                value.update(chunk)
            archived[name] = value.hexdigest()
    if archived != hashes:
        raise ValueError("archive differs from the extracted bundle")
    if suffix == "Pi4":
        metadata = evidence.read_json(bundle / "image/cohesix-pi4-sd.json")
        verify_sd_layout(bundle / "image/cohesix-pi4-sd.img", metadata)
        if {row["path"] for row in metadata["files"]} != set(
            release["pi4_stage_files"]
        ):
            raise ValueError(
                "SD image payload inventory differs from compiler selection"
            )
        commit = metadata["boot_identity"]["git_commit"]
    else:
        provenance = evidence.read_json(bundle / "BUILD_PROVENANCE.json")
        host = "macos" if suffix == "MacOS" else "linux"
        if (
            provenance.get("schema") != "cohesix-release-build-provenance/v1"
            or provenance.get("host") != host
        ):
            raise ValueError("bundle has invalid native build provenance")
        required = set(release["host_tools"]) | (
            set(release["target_images"]) - {"image/gic-version.txt"}
        )
        if set(provenance["files"]) != required:
            raise ValueError(
                "build provenance does not cover every native tool and guest image"
            )
        verify_payload(bundle, provenance["files"])
        commit = provenance["source_commit"]
    return {
        "bundle": bundle.name,
        "version": version,
        "source_commit": commit,
        "archive": file_record(archive),
        "manifest_sha256": hashes["MANIFEST.sha256"],
    }


def verify_sd_layout(image: Path, metadata: dict[str, Any]) -> None:
    """Verify compact MBR/FAT32 geometry and the full raw-image digest."""
    size = image.stat().st_size
    if (
        metadata.get("schema") != "cohesix-pi4-portable-sd-image/v2"
        or metadata.get("minimum_target_bytes") != size
        or metadata.get("image_size_bytes") != size
        or metadata.get("image_sha256") != digest(image)
        or metadata.get("sector_size_bytes") != 512
    ):
        raise ValueError("SD image identity or capacity metadata is invalid")
    with image.open("rb") as handle:
        mbr = handle.read(512)
        if len(mbr) != 512 or mbr[510:] != b"\x55\xaa" or mbr[450] != 0x0C:
            raise ValueError("SD image is not an MBR FAT32 LBA image")
        start, count = struct.unpack_from("<II", mbr, 454)
        if start != 2048 or (start + count) * 512 != size or any(mbr[462:510]):
            raise ValueError(
                "SD image does not contain exactly the compact boot partition"
            )
        if (
            metadata.get("partition_start_lba") != start
            or metadata.get("partition_sector_count") != count
        ):
            raise ValueError("SD partition metadata differs from the MBR")
        handle.seek(start * 512)
        boot = handle.read(512)
        if boot[82:90] != b"FAT32   " or boot[510:] != b"\x55\xaa":
            raise ValueError("SD boot partition is not FAT32")


def run(
    command: list[str], cwd: Path, log: Path, env: dict[str, str], timeout: int = 300
) -> None:
    """Execute an actual qualification check and retain its output."""
    with log.open("xb") as output:
        result = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            timeout=timeout,
            check=False,
        )
    if result.returncode:
        raise ValueError(
            f"qualification check failed: {log.name} (exit {result.returncode})"
        )


def tcp_smoke(
    bundle: Path, host: str, port: int, log: Path, env: dict[str, str]
) -> None:
    """Use the packaged authenticated client and its packaged EXPECT script."""
    if not env.get("COH_AUTH_TOKEN") and not env.get("COHSH_AUTH_TOKEN"):
        raise ValueError(
            "configure COH_AUTH_TOKEN or COHSH_AUTH_TOKEN before qualification"
        )
    run(
        [
            str(bundle / "bin/cohsh"),
            "--transport",
            "tcp",
            "--tcp-host",
            host,
            "--tcp-port",
            str(port),
            "--role",
            "queen",
            "--script",
            str(bundle / "scripts/cohsh/boot_v0.coh"),
        ],
        bundle,
        log,
        env,
    )


def stop_process(process: subprocess.Popen[bytes]) -> None:
    """Terminate only the process group created by this qualification run."""
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=10)


def qualify_host(args: argparse.Namespace, record: dict[str, Any]) -> None:
    """Run native binaries, packaged Python, QEMU TCP and replay presentation."""
    bundle, output = args.bundle, args.output.parent
    if ROOT in bundle.parents or bundle == ROOT:
        raise ValueError("qualify an extraction outside the source checkout")
    kind = "macos" if platform.system() == "Darwin" else "linux"
    suffix = "MacOS" if kind == "macos" else "linux"
    if not bundle.name.endswith(f"-{suffix}") or platform.machine() not in {
        "arm64",
        "aarch64",
    }:
        raise ValueError(
            "host qualification must run on the bundle's native ARM64 host"
        )
    env = os.environ.copy()
    for binary in sorted((bundle / "bin").iterdir()):
        command = [str(binary), "--help"]
        if kind == "linux" and binary.name == "swarmui":
            command = ["xvfb-run", "-a", *command]
        run(command, bundle, output / f"help-{binary.name}.log", env)
    run(
        [
            str(bundle / "bin/cohsh"),
            "--transport",
            "mock",
            "--replay-trace",
            str(bundle / "traces/trace_v0.trace"),
        ],
        bundle,
        output / "replay.log",
        env,
    )
    with tempfile.TemporaryDirectory(prefix="cohesix-release-python-") as directory:
        venv = Path(directory) / "venv"
        run(
            [sys.executable, "-m", "venv", "--system-site-packages", str(venv)],
            bundle,
            output / "python-venv.log",
            env,
        )
        python = str(venv / "bin/python")
        wheels = list((bundle / "python/dist").glob("*.whl"))
        if len(wheels) != 1:
            raise ValueError("release must contain exactly one Python wheel")
        run(
            [
                python,
                "-m",
                "pip",
                "install",
                "--no-index",
                "--no-deps",
                "--force-reinstall",
                str(wheels[0]),
            ],
            Path(directory),
            output / "python-install.log",
            env,
        )
        python_env = {key: value for key, value in env.items() if key != "PYTHONPATH"}
        run(
            [
                python,
                "-I",
                "-c",
                "import cohesix; import importlib.metadata; print(importlib.metadata.version('cohesix'))",
            ],
            Path(directory),
            output / "python-wheel-import.log",
            python_env,
        )
        run(
            [
                python,
                "-I",
                "-m",
                "pytest",
                "--import-mode=importlib",
                "-q",
                str(bundle / "python/cohesix-py/tests"),
            ],
            Path(directory),
            output / "python-tests.log",
            python_env,
        )
    # These ports are explicit operator inputs; an occupied port must fail.
    env.update(
        TCP_PORT=str(args.port),
        UDP_PORT=str(args.port + 1),
        SMOKE_PORT=str(args.port + 2),
    )
    for key in list(env):
        if key.startswith("COHESIX_QEMU_") or key in {
            "QEMU_ACCEL",
            "QEMU_CPU",
            "QEMU_SMP",
            "QEMU_SMP_TOPO",
            "QEMU_VIRT",
            "QEMU_MACHINE_EXTRA",
        }:
            env.pop(key)
    with (output / "qemu.log").open("xb") as log:
        process = subprocess.Popen(
            [str(bundle / "qemu/run.sh")],
            cwd=bundle,
            env=env,
            stdin=subprocess.PIPE,
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            deadline = time.monotonic() + 180
            while (
                b"[mark] root-console.start.ok"
                not in (output / "qemu.log").read_bytes()
            ):
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise ValueError(
                        "packaged QEMU did not reach root-console readiness"
                    )
                time.sleep(0.1)
            boot_log = (output / "qemu.log").read_bytes()
            native_accel = b"hvf" if kind == "macos" else b"kvm"
            if (
                b"[qemu] Using QEMU accel: " + native_accel not in boot_log
                or b"claim-ineligible" in boot_log
            ):
                raise ValueError("packaged QEMU is outside the native release envelope")
            tcp_smoke(bundle, "127.0.0.1", args.port, output / "tcp.log", env)
        finally:
            stop_process(process)
            if process.stdin is not None:
                process.stdin.close()
    env["SWARMUI_RELEASE_DIR"] = str(bundle)
    env.pop("SWARMUI_UI_ROOT", None)
    run(
        ["npm", "test"],
        ROOT / "tools/swarmui-ui-tests",
        output / "ui.log",
        env,
        timeout=1800,
    )
    verify_tested_bytes(bundle, record["manifest_sha256"])
    record.update(kind=kind, checks=HOST_CHECKS)


def media_readback(args: argparse.Namespace, record: dict[str, Any]) -> None:
    """Read exactly the distributed image prefix from an explicitly selected device."""
    image = args.bundle / "image/cohesix-pi4-sd.img"
    device = args.device
    mode = device.stat().st_mode
    if not (stat.S_ISBLK(mode) or stat.S_ISCHR(mode)) or not str(device).startswith(
        "/dev/"
    ):
        raise ValueError(
            "media readback requires an explicit physical whole-disk device under /dev"
        )
    remaining = image.stat().st_size
    value = hashlib.sha256()
    with device.open("rb", buffering=0) as handle:
        while remaining:
            chunk = handle.read(min(1024 * 1024, remaining))
            if not chunk:
                raise ValueError("media is smaller than the distributed image")
            value.update(chunk)
            remaining -= len(chunk)
    if value.hexdigest() != digest(image):
        raise ValueError("media readback differs from the distributed SD image")
    record.update(
        kind="media",
        checks=["manifest", "archive", "sd-layout", "media-readback"],
        device=str(device),
        image_sha256=value.hexdigest(),
        image_bytes=image.stat().st_size,
    )


def read_result(path: Path, kind: str) -> dict[str, Any]:
    """Verify the immutable output record and every retained evidence attachment."""
    value = evidence.read_json(path)
    if (
        value.get("schema") != SCHEMA
        or value.get("kind") != kind
        or value.get("status") != "pass"
    ):
        raise ValueError(f"missing passing {kind} qualification: {path}")
    identity = value.pop("result_sha256", None)
    if identity != hashlib.sha256(evidence.canonical_bytes(value)).hexdigest():
        raise ValueError("qualification record identity mismatch")
    for row in value["logs"]:
        log = evidence.safe_relative_file(path.parent, row["path"])
        if digest(log) != row["sha256"] or log.stat().st_size != row["size"]:
            raise ValueError("qualification attachment changed")
    return value


def qualify_pi(args: argparse.Namespace, record: dict[str, Any]) -> None:
    """Join exact media readback, one fresh serial boot and packaged-client TCP."""
    if not args.provisioning_verified:
        raise ValueError(
            "operator must verify initial configuration on the freshly flashed card"
        )
    media = read_result(args.media_result, "media")
    for field in ("bundle", "archive", "manifest_sha256", "source_commit"):
        if media[field] != record[field]:
            raise ValueError("media readback belongs to a different release")
    metadata = evidence.read_json(args.bundle / "image/cohesix-pi4-sd.json")
    marker = metadata["boot_identity"]["build_marker"].encode()
    serial = args.serial_log.read_bytes()
    if serial.count(marker) != 1 or serial.count(b"[mark] root-console.start.ok") != 1:
        raise ValueError(
            "serial evidence must contain exactly one fresh boot of the released image"
        )
    if media.get("image_sha256") != metadata["image_sha256"]:
        raise ValueError(
            "media receipt image digest differs from the distributed image"
        )
    if args.serial_log.stat().st_mtime_ns < media["recorded_at_ns"]:
        raise ValueError("serial evidence predates media verification")
    host_archive = args.host_bundle.with_name(args.host_bundle.name + ".tar.gz")
    host = inspect_bundle(args.host_bundle, host_archive)
    if host["source_commit"] != record["source_commit"]:
        raise ValueError("Pi smoke client belongs to a different source commit")
    output = args.output.parent
    (output / "serial.log").write_bytes(serial)
    (output / "media.json").write_bytes(args.media_result.read_bytes())
    tcp_smoke(
        args.host_bundle, args.host, args.port, output / "tcp.log", os.environ.copy()
    )
    record.update(
        kind="pi4",
        checks=PI_CHECKS,
        provisioning_verified=True,
        host=args.host,
        boot_marker_sha256=hashlib.sha256(marker).hexdigest(),
        media_image_sha256=media["image_sha256"],
    )


def verify_release(args: argparse.Namespace) -> dict[str, Any]:
    """Require all three installation results for the exact archives being shipped."""
    results = []
    qualifications = []
    for kind, path in (
        ("macos", args.macos_result),
        ("linux", args.linux_result),
        ("pi4", args.pi4_result),
    ):
        row = read_result(path, kind)
        if row.get("checks") != (PI_CHECKS if kind == "pi4" else HOST_CHECKS):
            raise ValueError(f"{kind} qualification omits required checks")
        suffix = {"macos": "MacOS", "linux": "linux", "pi4": "Pi4"}[kind]
        name = f"Cohesix-{row['version']}-{suffix}.tar.gz"
        if row["archive"]["path"] != name:
            raise ValueError(f"{kind} qualification has the wrong archive name")
        archive = evidence.safe_relative_file(args.releases_dir, name)
        if file_record(archive) != row["archive"]:
            raise ValueError(f"{kind} archive changed after qualification")
        results.append(row)
        qualifications.append({"kind": kind, **file_record(path)})
    if len({(row["version"], row["source_commit"]) for row in results}) != 1:
        raise ValueError("release results do not share a version and source commit")
    current = subprocess.check_output(
        ["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True
    ).strip()
    inventory = evidence.read_json(
        ROOT / "configs/generated/implementation_surface_inventory.json"
    )
    if (
        results[0]["source_commit"] != current
        or results[0]["version"] != inventory["release"]["version"]
    ):
        raise ValueError(
            "qualified release differs from the current selected source/version"
        )
    return {
        "kind": "release",
        "checks": ["macos", "linux", "pi4"],
        "version": results[0]["version"],
        "source_commit": current,
        "archives": [row["archive"] for row in results],
        "qualifications": qualifications,
    }


def main() -> int:
    """Run one qualification phase or validate all completed release records."""
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("host", "media", "pi4"):
        item = sub.add_parser(command)
        item.add_argument("--bundle", type=Path, required=True)
        item.add_argument("--archive", type=Path, required=True)
        item.add_argument("--output", type=Path, required=True)
        if command in {"host", "pi4"}:
            item.add_argument("--port", type=int, default=31337)
        if command == "media":
            item.add_argument("--device", type=Path, required=True)
        if command == "pi4":
            item.add_argument("--media-result", type=Path, required=True)
            item.add_argument("--serial-log", type=Path, required=True)
            item.add_argument("--host-bundle", type=Path, required=True)
            item.add_argument("--host", required=True)
            item.add_argument("--provisioning-verified", action="store_true")
    verify = sub.add_parser("verify")
    for host in ("macos", "linux", "pi4"):
        verify.add_argument(f"--{host}-result", type=Path, required=True)
    verify.add_argument("--releases-dir", type=Path, required=True)
    verify.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if getattr(args, "port", 31337) not in range(1, 65534):
            raise ValueError("port must be between 1 and 65533")
        if args.output.exists() or (
            args.output.parent.exists() and any(args.output.parent.iterdir())
        ):
            raise ValueError("use a fresh empty qualification output directory")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        if args.command == "verify":
            record = verify_release(args)
        else:
            args.bundle = args.bundle.resolve(strict=True)
            args.archive = args.archive.resolve(strict=True)
            if args.command == "pi4":
                args.host_bundle = args.host_bundle.resolve(strict=True)
            record = inspect_bundle(args.bundle, args.archive)
            {"host": qualify_host, "media": media_readback, "pi4": qualify_pi}[
                args.command
            ](args, record)
        record.update(
            schema=SCHEMA,
            status="pass",
            recorded_at_ns=time.time_ns(),
            claim="release-installation-smoke",
            replaces_m26e_acceptance=False,
            logs=[
                file_record(path)
                for path in sorted(args.output.parent.iterdir())
                if path.is_file()
            ],
        )
        record["result_sha256"] = hashlib.sha256(
            evidence.canonical_bytes(record)
        ).hexdigest()
        evidence.atomic_write_json(args.output, record)
        print(f"release qualification {record['kind']}: PASS ({args.output})")
    except (
        OSError,
        ValueError,
        KeyError,
        TypeError,
        evidence.EvidenceError,
        subprocess.SubprocessError,
    ) as error:
        print(f"release-qualification: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
