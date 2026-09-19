#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Qualify compiled MODBUS RTU/TCP and DNP3 clients against independent owned loopback references with exact source and ACK evidence.
# Copyright 2026 Lukas Bower

"""Native host protocol interoperability, never physical-device or Root authority proof.

Run from the checkout using an isolated Python environment with PyModbus 3.11.4,
pyserial 3.5 and cryptography. An independently built OpenDNP3 3.1.2 static library
is required. The command freezes a private generated map, builds exact native
clients, restores all canonical outputs and exercises only its own references.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
from importlib.metadata import version
import json
import os
from pathlib import Path
import platform
import pty
import shlex
import shutil
import socket
import subprocess
import time
import tomllib
import tty
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
REFERENCE_COMMIT = "c1dc7165a79cc08edbf4b55d2ff4162efb176f92"
MAX_LOG = 4 * 1024 * 1024


def encoded(value: Any) -> bytes:
    """Preserve the evidence core's declared field order and compact JSON."""
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode()


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def save(path: Path, value: Any) -> None:
    path.write_bytes(encoded(value))


async def command(
    argv: list[str], log: Path, timeout: int = 600, *, env: dict[str, str] | None = None
) -> None:
    """A bounded subprocess owns its readers and entire process group."""
    process = await asyncio.create_subprocess_exec(
        *argv,
        cwd=ROOT,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
        start_new_session=True,
        env=env,
    )
    assert process.stdout is not None
    try:
        async with asyncio.timeout(timeout):
            with log.open("xb") as stream:
                total = 0
                while data := await process.stdout.read(65536):
                    total += len(data)
                    if total > MAX_LOG:
                        raise ValueError("reference command output bound")
                    stream.write(data)
            if await process.wait() != 0:
                raise ValueError(f"reference command failed; inspect {log.name}")
    finally:
        if process.returncode is None:
            import signal

            os.killpg(process.pid, signal.SIGKILL)
            await process.wait()


def reserve_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


async def build_profile(
    out: Path, endpoints: list[dict[str, Any]], source_manifest: Path | None
) -> None:
    """Capture every generated output before selecting only owned reference maps."""
    import tomllib

    source = (ROOT / "configs/host_integration_acceptance.toml").read_text()
    if tomllib.loads(source)["providers"].get("field_bus"):
        raise ValueError(
            "use an unselected canonical field-bus source for reference qualification"
        )
    for endpoint in endpoints:
        source += "\n[[providers.field_bus]]\n"
        for key, value in endpoint.items():
            if key not in ("transport", "points"):
                source += f"{key} = {json.dumps(value)}\n"
        source += "[providers.field_bus.transport]\n"
        source += "".join(
            f"{key} = {json.dumps(value)}\n"
            for key, value in endpoint["transport"].items()
        )
        for point in endpoint["points"]:
            source += "[[providers.field_bus.points]]\n"
            source += f'id = {json.dumps(point["id"])}\napproval_required = {str(point["approval_required"]).lower()}\n'
            source += "[providers.field_bus.points.operation]\n"
            source += "".join(
                f"{key} = {json.dumps(value)}\n"
                for key, value in point["operation"].items()
            )
    (out / "source.toml").write_text(source)
    script = (ROOT / "scripts/cohsh/run_regression_batch.sh").read_text()
    paths = shlex.split(
        script.split("GENERATED_OUTPUT_PATHS=(\n", 1)[1].split("\n)", 1)[0]
    )
    original: dict[Path, tuple[bytes, int] | None] = {}
    directories: dict[Path, set[Path]] = {}
    for relative in paths:
        path = ROOT / relative
        if path.is_dir():
            files = {child for child in path.rglob("*") if child.is_file()}
            directories[path] = files
        else:
            files = {path}
        for path in files:
            original[path] = (
                (path.read_bytes(), path.stat().st_mode & 0o777)
                if path.exists()
                else None
            )
    if source_manifest is not None:
        source_record = verify_source_manifest(source_manifest)
    else:
        source_record = checkout_source_manifest()
    save(out / "source-inventory.json", source_record)
    try:
        await compile_profile(
            out, source_record if source_manifest is not None else None
        )
    finally:
        for directory, old in directories.items():
            for path in directory.rglob("*"):
                if path.is_file() and path not in old:
                    path.unlink()
        for path, value in original.items():
            if value is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(value[0])
                path.chmod(value[1])
        if any(
            path.read_bytes() != value[0]
            for path, value in original.items()
            if value is not None
        ):
            raise ValueError("canonical generated restoration failed")


def verify_source_manifest(path: Path) -> dict[str, Any]:
    """Verify explicit archive source provenance before compiling without a Git checkout."""
    import re

    if (
        path.is_symlink()
        or not path.is_file()
        or path.stat().st_size > 32 * 1024 * 1024
    ):
        raise ValueError("source inventory file bound")
    record = json.loads(path.read_bytes())
    if (
        set(record) != {"commit", "files"}
        or not re.fullmatch(r"[0-9a-f]{40}", record["commit"])
        or not isinstance(record["files"], dict)
        or not 1 <= len(record["files"]) <= 100000
    ):
        raise ValueError("source inventory schema")
    if set(record["files"]) != set(archive_source_files()):
        raise ValueError(
            "source inventory does not cover the complete native source snapshot"
        )
    for name, expected in record["files"].items():
        relative = Path(name)
        if (
            relative.is_absolute()
            or any(part in ("", ".", "..") for part in name.split("/"))
            or not re.fullmatch(r"[0-9a-f]{64}", expected)
        ):
            raise ValueError("source inventory path or digest")
        if source_file_digest(ROOT / relative) != expected:
            raise ValueError("source inventory changed: " + name)
    return record


def archive_source_files() -> list[str]:
    """Every native build/configuration/document input, excluding build and interpreter caches."""
    files = []
    directories = [
        "apps",
        "crates",
        "tools",
        "configs",
        "docs",
        "scripts",
        "resources",
        "packaging",
        "third_party",
        ".cargo",
        "tests",
    ]
    for directory in directories:
        for parent, children, names in os.walk(ROOT / directory):
            if any((Path(parent) / name).is_symlink() for name in children):
                raise ValueError("source snapshot contains a directory symlink")
            children[:] = [
                name
                for name in children
                if name
                not in {
                    ".git",
                    "out",
                    "target",
                    "__pycache__",
                    ".pytest_cache",
                    "node_modules",
                }
            ]
            for name in names:
                path = Path(parent) / name
                if not path.is_file():
                    raise ValueError("source snapshot contains a non-regular input")
                files.append(str(path.relative_to(ROOT)))
    files += [
        name
        for name in (
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "README.md",
            "AGENTS.md",
        )
        if (ROOT / name).is_file()
    ]
    release = tomllib.loads(
        (ROOT / "configs/implementation_surfaces.toml").read_text()
    )["release"]
    for field in (
        "public_documents",
        "host_assets",
        "operator_scripts",
        "python_artifacts",
        "cas_fixtures",
        "trace_fixtures",
        "transcript_fixtures",
        "ui_assets",
        "support_files",
        "versioned_migrations",
    ):
        for name in release[field]:
            path = Path(name)
            if path.is_absolute() or ".." in path.parts or not (ROOT / path).is_file():
                raise ValueError("missing or invalid selected release source: " + name)
            files.append(name)
    return sorted(set(files))


def source_file_digest(path: Path) -> str:
    """Bind regular bytes or an in-tree source symlink and its resolved bytes."""
    if not path.is_file():
        raise ValueError("source input must resolve to a regular file")
    resolved = path.resolve(strict=True)
    if not resolved.is_relative_to(ROOT):
        raise ValueError("source symlink escapes the native snapshot")
    value = digest(resolved.read_bytes())
    if path.is_symlink():
        return digest(encoded({"symlink": os.readlink(path), "target_sha256": value}))
    return value


def checkout_source_manifest() -> dict[str, Any]:
    """Capture the current source tree, including uncommitted scoped work."""
    tracked = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT,
    ).split(b"\0")
    inputs = {}
    for relative in sorted(set(tracked)):
        if relative and (ROOT / os.fsdecode(relative)).is_file():
            inputs[os.fsdecode(relative)] = digest(
                (ROOT / os.fsdecode(relative)).read_bytes()
            )
    return {
        "commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
        "files": inputs,
    }


async def compile_profile(out: Path, archive: dict[str, Any] | None) -> None:
    """Generate then compile the reference profile while the caller owns restoration."""
    index = out / "source-index.git"
    generation_env = None
    if archive is not None:
        # coh-rtc inventories Git paths. A temporary index enumerates the verified
        # archive without inventing a commit or modifying its source directory.
        await command(
            ["git", "init", "--quiet", "--bare", str(index)],
            out / "source-index-init.log",
        )
        generation_env = {
            **os.environ,
            "GIT_DIR": str(index),
            "GIT_WORK_TREE": str(ROOT),
            "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_LITERAL_PATHSPECS": "1",
        }
        paths = out / "source-paths.nul"
        paths.write_bytes(
            b"\0".join(name.encode() for name in archive["files"]) + b"\0"
        )
        await command(
            [
                "git",
                "add",
                "--force",
                "--pathspec-from-file=" + str(paths),
                "--pathspec-file-nul",
            ],
            out / "source-index-add.log",
            env=generation_env,
        )
    try:
        await generate_profile(out, generation_env)
    finally:
        if index.exists():
            shutil.rmtree(index)
    for name in (
        "provider_registry.json",
        "root_task_resolved.json",
        "implementation_surface_inventory.json",
    ):
        shutil.copyfile(ROOT / "configs/generated" / name, out / name)
    target = Path(
        os.environ.get(
            "CARGO_TARGET_DIR",
            str(ROOT / "out/provider-conformance-build/field-bus"),
        )
    ).absolute()
    await command(
        [
            "cargo",
            "build",
            "--locked",
            "--release",
            "--target-dir",
            str(target),
            "-p",
            "sidecar-bus",
            "-p",
            "host-sidecar-bridge",
            "--features",
            "sidecar-bus/live,sidecar-bus/modbus,sidecar-bus/dnp3",
        ],
        out / "build.log",
    )
    for name in ("sidecar-bus", "host-sidecar-bridge"):
        shutil.copyfile(target / "release" / name, out / name)
        (out / name).chmod(0o500)


async def generate_profile(out: Path, env: dict[str, str] | None) -> None:
    """Run the compiler against either a checkout or its verified temporary archive index."""
    await command(
        [
            "cargo",
            "run",
            "--locked",
            "-p",
            "coh-rtc",
            "--",
            "configs/root_task.toml",
            "--out",
            "apps/root-task/src/generated",
            "--manifest",
            "configs/generated/root_task_resolved.json",
            "--host-integration-source",
            str(out / "source.toml"),
        ],
        out / "generate.log",
        env=env,
    )


def grant_fixture(
    out: Path, name: str, request: dict[str, Any], registry: dict[str, Any]
) -> tuple[Path, Path]:
    """Ephemeral fixture authority can test native I/O only; it is never enrolled by a Root or release verifier."""
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives import serialization

    now = time.time_ns() // 1_000_000
    expiry = now + 30000
    family = "dnp3" if request["endpoint"] == "reference-dnp3" else "modbus"
    admitted = {
        "schema": "host-ticket/v1",
        "id": name,
        "idempotency_key": request["idempotency_key"],
        "action": f"{family}.control",
        "writer_epoch": 1,
        "target": request["endpoint"],
        "args": {"endpoint": request["endpoint"], "point": request["point"]},
    }
    binding = {
        "ticket_id": name,
        "subject": "loopback-fixture-only",
        "action": admitted["action"],
        "idempotency_key": request["idempotency_key"],
        "writer_epoch": 1,
        "target_manifest_sha256": registry["resolved_manifest_sha256"],
        "provider_graph_sha256": registry["graph_sha256"],
        "implementation_graph_sha256": digest(
            (out / "source-inventory.json").read_bytes()
        ),
        "use_case_graph_sha256": digest(b"non-authoritative-loopback-fixture"),
        "component_sha256": {"sidecar-bus": digest((out / "sidecar-bus").read_bytes())},
        "worker": None,
    }
    enrollment = out / (name + "-enrollment")
    store = out / (name + "-evidence")
    for path in (enrollment, store, store / "cas", store / digest(encoded(binding))):
        path.mkdir(mode=0o700)
    for path in (
        store / "cas/owner.lock",
        store / digest(encoded(binding)) / "owner.lock",
    ):
        path.write_bytes(b"")
    keys = [Ed25519PrivateKey.generate(), Ed25519PrivateKey.generate()]
    phases = [
        ["intent", "facts", "approval", "grant"],
        ["execution", "observation", "verification", "terminal"],
    ]
    trust = {
        "schema": "cohesix-evidence-trust/v1",
        "expected": binding,
        "keys": [
            {
                "id": f"fixture-{i}",
                "source": f"owned-loopback-fixture-{i}",
                "public_key": key.public_key()
                .public_bytes(
                    serialization.Encoding.Raw, serialization.PublicFormat.Raw
                )
                .hex(),
                "kinds": phases[i],
                "not_before_unix_ms": now - 1000,
                "not_after_unix_ms": expiry,
            }
            for i, key in enumerate(keys)
        ],
        "verification_unix_ms": now,
        "maximum_record_ttl_ms": 30000,
    }
    payloads = [
        admitted,
        {"scope": "owned-loopback-reference"},
        {"scope": "fixture-only"},
        {"scope": "fixture-only"},
    ]
    records = []
    for index, (kind, payload) in enumerate(zip(phases[0], payloads)):
        raw = encoded(payload)
        (store / "cas" / digest(raw)).write_bytes(raw)
        record = {
            "schema": "cohesix-causal-record/v1",
            "binding": binding,
            "kind": kind,
            "source": "owned-loopback-fixture-0",
            "sequence": index + 1,
            "observed_unix_ms": now,
            "expires_unix_ms": expiry,
            "parents": [row["sha256"] for row in records],
            "artifacts": [
                {
                    "sha256": digest(raw),
                    "bytes": len(raw),
                    "media_type": "application/json",
                }
            ],
            "native_identity": None,
            "resource_generation": 0,
            "event_cursor": None,
            "outcome": "admitted" if index >= 2 else "observed",
        }
        raw = encoded(record)
        records.append(
            {
                "record": record,
                "sha256": digest(raw),
                "key_id": "fixture-0",
                "signature": keys[0].sign(raw).hex(),
            }
        )
    save(
        store / digest(encoded(binding)) / "graph.json",
        {
            "schema": "cohesix-causal-evidence/v1",
            "binding": binding,
            "records": records,
        },
    )
    key_path = enrollment / "native-seed"
    key_path.write_text(
        keys[1]
        .private_bytes(
            serialization.Encoding.Raw,
            serialization.PrivateFormat.Raw,
            serialization.NoEncryption(),
        )
        .hex()
    )
    save(
        enrollment / (digest(encoded([name, request["idempotency_key"]])) + ".json"),
        {
            "schema": "cohesix-producer-enrollment/v1",
            "store": str(store),
            "key_id": "fixture-1",
            "signing_key_ref": f"file:{key_path}",
            "trust": trust,
            "expires_unix_ms": expiry,
        },
    )
    admitted_path = out / (name + "-admitted.json")
    save(admitted_path, admitted)
    return admitted_path, enrollment


def dnp3_frame(payload: bytes) -> bytes:
    """Commission only the independently owned fixture's restart indication."""

    def crc(data: bytes) -> bytes:
        value = 0
        for byte in data:
            value ^= byte
            for _ in range(8):
                value = (value >> 1) ^ 0xA6BC if value & 1 else value >> 1
        return (value ^ 0xFFFF).to_bytes(2, "little")

    header = bytes([5, 100, len(payload) + 5, 196, 10, 0, 1, 0])
    wire = header + crc(header)
    for offset in range(0, len(payload), 16):
        block = payload[offset : offset + 16]
        wire += block + crc(block)
    return wire


async def run(args: argparse.Namespace) -> None:
    from pymodbus.server import ModbusTcpServer, ModbusSerialServer
    from pymodbus.datastore import (
        ModbusDeviceContext,
        ModbusServerContext,
        ModbusSequentialDataBlock,
    )

    if version("pymodbus") != "3.11.4" or version("pyserial") != "3.5":
        raise ValueError("reference requires PyModbus 3.11.4 and pyserial 3.5")
    source = args.opendnp3_source.resolve(strict=True)
    revision = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    if revision != REFERENCE_COMMIT:
        raise ValueError("reference requires exact OpenDNP3 source commit")
    subprocess.run(["git", "-C", str(source), "diff", "--quiet", "HEAD"], check=True)
    library = args.opendnp3_library.resolve(strict=True)
    out = args.state_dir.absolute()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.mkdir(mode=0o700)
    cases = []
    wire = []
    server = None
    logger = None
    tcp = serial = None
    fds = []
    secrets = []
    try:
        await command(
            [
                "c++",
                "-std=c++17",
                "-I",
                str(source / "cpp/lib/include"),
                str(ROOT / "tests/fixtures/field_bus/outstation.cpp"),
                str(library),
                "-lpthread",
                "-o",
                str(out / "outstation"),
            ],
            out / "reference-build.log",
            60,
        )
        ports = [reserve_port(), reserve_port()]
        server = await asyncio.create_subprocess_exec(
            str(out / "outstation"),
            str(ports[1]),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,
        )
        ready = asyncio.Event()

        async def log_server() -> None:
            assert server is not None and server.stdout is not None
            total = 0
            with (out / "outstation.log").open("xb") as stream:
                while data := await server.stdout.readline():
                    total += len(data)
                    if total > MAX_LOG:
                        raise ValueError("outstation log bound")
                    stream.write(data)
                    stream.flush()
                    if b"READY" in data:
                        ready.set()

        logger = asyncio.create_task(log_server())
        await asyncio.wait_for(ready.wait(), 5)
        left, left_slave = pty.openpty()
        right, right_slave = pty.openpty()
        fds = [left, left_slave, right, right_slave]
        for fd in (left_slave, right_slave):
            tty.setraw(fd)
        for fd in (left, right):
            os.set_blocking(fd, False)
        relay_errors = []

        def relay(source_fd: int, target_fd: int) -> None:
            try:
                data = os.read(source_fd, 1024)
                if data and os.write(target_fd, data) != len(data):
                    relay_errors.append("partial PTY relay")
            except BlockingIOError:
                pass
            except OSError as error:
                relay_errors.append(type(error).__name__)

        loop = asyncio.get_running_loop()
        loop.add_reader(left, relay, left, right)
        loop.add_reader(right, relay, right, left)

        def context() -> Any:
            return ModbusServerContext(
                devices={
                    1: ModbusDeviceContext(
                        hr=ModbusSequentialDataBlock(1, [42, 43, 44] + [0] * 61)
                    )
                },
                single=False,
            )

        def trace(link: str) -> Any:
            def capture(sending: bool, data: bytes) -> bytes:
                if len(wire) >= 128 or len(data) > 1024:
                    raise ValueError("reference wire bound")
                wire.append({"link": link, "sending": sending, "hex": data.hex()})
                return data

            return capture

        tcp = ModbusTcpServer(
            context(), address=("127.0.0.1", ports[0]), trace_packet=trace("tcp")
        )
        serial = ModbusSerialServer(
            context(),
            port=os.ttyname(right_slave),
            baudrate=19200,
            parity="N",
            stopbits=2,
            bytesize=8,
            trace_packet=trace("rtu"),
        )
        await tcp.serve_forever(background=True)
        await serial.serve_forever(background=True)
        endpoints = []
        for family, link, transport in [
            ("modbus", "tcp", {"kind": "tcp", "address": f"127.0.0.1:{ports[0]}"}),
            (
                "modbus",
                "rtu",
                {
                    "kind": "serial",
                    "path": os.ttyname(left_slave),
                    "baud": 19200,
                    "parity": "none",
                },
            ),
            ("dnp3", "tcp", {"kind": "tcp", "address": f"127.0.0.1:{ports[1]}"}),
        ]:
            points = [
                {
                    "id": "value",
                    "approval_required": False,
                    "operation": (
                        {"kind": "modbus_read", "function": 3, "start": 0, "count": 3}
                        if family == "modbus"
                        else {
                            "kind": "dnp3_read",
                            "group": 30,
                            "variation": 1,
                            "start": 0,
                            "count": 1,
                        }
                    ),
                },
                {
                    "id": "control",
                    "approval_required": True,
                    "operation": (
                        {
                            "kind": "modbus_write",
                            "function": 6,
                            "address": 3,
                            "value": 123,
                        }
                        if family == "modbus"
                        else {
                            "kind": "dnp3_control",
                            "index": 0,
                            "code": 3,
                            "on_ms": 0,
                            "off_ms": 0,
                        }
                    ),
                },
            ]
            if family == "modbus":
                points.append(
                    {
                        "id": "absent",
                        "approval_required": False,
                        "operation": {
                            "kind": "modbus_read",
                            "function": 3,
                            "start": 5000,
                            "count": 1,
                        },
                    }
                )
            endpoints.append(
                {
                    "id": (
                        "reference-dnp3"
                        if family == "dnp3"
                        else f"reference-modbus-{link}"
                    ),
                    "protocol": family,
                    "unit": 1 if family == "modbus" else 0,
                    "master": 1 if family == "dnp3" else 0,
                    "outstation": 10 if family == "dnp3" else 0,
                    "timeout_ms": 2000,
                    "poll_interval_ms": 100,
                    "observation_ttl_ms": 30000,
                    "transport": transport,
                    "points": points,
                }
            )
        await build_profile(out, endpoints, args.source_manifest)
        registry = json.loads((out / "provider_registry.json").read_bytes())

        async def client(
            name: str, endpoint: str, point: str, approved: bool = False
        ) -> tuple[int, dict[str, Any] | None]:
            request = {
                "schema": "cohesix-field-bus-request/v1",
                "id": name,
                "idempotency_key": name + "-once",
                "endpoint": endpoint,
                "point": point,
            }
            path = out / (name + ".json")
            save(path, request)
            argv = [
                str(out / "sidecar-bus"),
                "--state-dir",
                str(out / ("spool-" + endpoint)),
                "--request",
                str(path),
            ]
            if approved:
                admitted, enrollment = grant_fixture(out, name, request, registry)
                secrets.append(enrollment / "native-seed")
                argv += [
                    "--admitted-ticket",
                    str(admitted),
                    "--evidence-enrollment-dir",
                    str(enrollment),
                ]
            process = await asyncio.create_subprocess_exec(
                *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
            )
            try:
                stdout, stderr = await asyncio.wait_for(process.communicate(), 10)
            finally:
                if process.returncode is None:
                    process.kill()
                    await process.wait()
            if len(stdout) + len(stderr) > 65536:
                raise ValueError("native client output bound")
            (out / (name + ".stdout")).write_bytes(stdout)
            (out / (name + ".stderr")).write_bytes(stderr)
            value = json.loads(stdout)["entry"] if stdout else None
            cases.append(
                {"case": name, "exit_code": process.returncode, "entry": value}
            )
            return process.returncode, value

        for link in ("tcp", "rtu"):
            endpoint = "reference-modbus-" + link
            code, entry = await client(link + "-read", endpoint, "value")
            assert (
                code == 0
                and entry
                and [row["value"] for row in entry["values"]] == [42, 43, 44]
            ), entry
            before = (out / ("spool-" + endpoint) / "ledger.json").read_bytes()
            count = len(wire)
            code, entry = await client(link + "-read", endpoint, "value")
            assert (
                code == 0
                and before == (out / ("spool-" + endpoint) / "ledger.json").read_bytes()
                and count == len(wire)
            )
            code, entry = await client(link + "-exception", endpoint, "absent")
            assert (
                code != 0 and entry and entry["error"] == "modbus_exception code=2"
            ), entry
            code, entry = await client(link + "-unapproved", endpoint, "control")
            assert code != 0 and entry is None
            code, entry = await client(link + "-control", endpoint, "control", True)
            assert (
                code == 0
                and entry
                and entry["state"] == "delivered"
                and len(entry["acknowledgements_hex"]) == 1
            ), entry
        code, entry = await client("dnp3-restart", "reference-dnp3", "value")
        assert code != 0 and entry and entry["error"] == "dnp3_iin status=130", entry
        reader, writer = await asyncio.open_connection("127.0.0.1", ports[1])
        request = dnp3_frame(bytes([0xC0, 0xC0, 2, 80, 1, 0, 7, 7, 0]))
        writer.write(request)
        await writer.drain()
        response = await asyncio.wait_for(reader.read(292), 2)
        writer.close()
        await writer.wait_closed()
        save(
            out / "fixture-restart-ack.json",
            {
                "request": request.hex(),
                "response": response.hex(),
                "scope": "owned-loopback-fixture-only",
            },
        )
        assert response[:2] == bytes([5, 100])
        # Respect the compiled interval after the separate failed request.
        await asyncio.sleep(0.11)
        code, entry = await client("dnp3-read", "reference-dnp3", "value")
        assert (
            code == 0
            and entry
            and entry["values"] == [{"index": 0, "value": 42, "flags": 1}]
        ), entry
        await command(
            [str(out / "host-sidecar-bridge"), "--native-provider", "dnp3"],
            out / "snapshot.stdout",
            10,
            env={
                **os.environ,
                "COHESIX_FIELD_BUS_STATE_ROOT": str(out / "spool-reference-dnp3"),
            },
        )
        snapshot = json.loads((out / "snapshot.stdout").read_bytes())
        assert snapshot["available"] is True and snapshot["authoritative"] is False
        status = json.loads(
            next(
                row["value"]
                for row in snapshot["entries"]
                if row["path"].endswith("/status")
            )
        )
        assert (
            entry
            and status["observed_unix_ms"] == entry["observed_unix_ms"]
            and status["expires_unix_ms"] == entry["observed_unix_ms"] + 30000
        )
        code, entry = await client("dnp3-control", "reference-dnp3", "control", True)
        assert (
            code == 0
            and entry
            and entry["state"] == "delivered"
            and len(entry["acknowledgements_hex"]) == 2
        ), entry
        if relay_errors:
            raise ValueError("PTY relay failure")
        save(
            out / "summary.json",
            {
                "schema": "cohesix-field-bus-conformance/v1",
                "result": "PASS",
                "proof_class": "native_host_interoperability",
                "authoritative": False,
                "worker_proof": False,
                "physical_field_device": False,
                "physical_uart": False,
                "production_proven": False,
                "os": platform.system(),
                "architecture": platform.machine(),
                "reference_commit": revision,
                "reference_library_sha256": digest(library.read_bytes()),
                "outstation_sha256": digest((out / "outstation").read_bytes()),
                "client_sha256": digest((out / "sidecar-bus").read_bytes()),
                "cases": cases,
            },
        )
        print(
            "PASS MODBUS TCP/RTU PTY and DNP3: remote reads, exceptions, restart refusal, controls and retained ACKs"
        )
    finally:
        for secret in secrets:
            secret.unlink(missing_ok=True)
        if server is not None:
            if server.returncode is None:
                assert server.stdin is not None
                server.stdin.write(b"quit\n")
                await server.stdin.drain()
                try:
                    await asyncio.wait_for(server.wait(), 5)
                except TimeoutError:
                    server.kill()
                    await server.wait()
            if logger is not None:
                await asyncio.wait_for(logger, 5)
        if tcp is not None:
            await tcp.shutdown()
        if serial is not None:
            await serial.shutdown()
        if fds:
            asyncio.get_running_loop().remove_reader(fds[0])
            asyncio.get_running_loop().remove_reader(fds[2])
            for fd in fds:
                os.close(fd)
        save(out / "modbus-wire.json", wire)
        inventory = {
            str(path.relative_to(out)): digest(path.read_bytes())
            for path in out.rglob("*")
            if path.is_file()
            and "build" not in path.relative_to(out).parts
            and path.name != "artifacts.json"
        }
        save(out / "artifacts.json", inventory)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", required=True, type=Path)
    parser.add_argument("--opendnp3-source", required=True, type=Path)
    parser.add_argument("--opendnp3-library", required=True, type=Path)
    parser.add_argument(
        "--source-manifest",
        type=Path,
        help="Exact commit/files digest inventory for a source archive without .git",
    )
    args = parser.parse_args()
    os.umask(0o077)
    asyncio.run(run(args))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
