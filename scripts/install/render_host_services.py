#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Render verified package service templates without enabling services or embedding credentials.
# Copyright 2026 Lukas Bower

"""Render exact signed service assets with bounded explicit enrollment values."""

from __future__ import annotations

import argparse
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import subprocess
import tempfile
import tomllib
from typing import Any

MAX_DOCUMENT = 4 * 1024 * 1024
TOKEN = re.compile(r"@[A-Z_]+@")
SAFE_PATH = re.compile(r"/[A-Za-z0-9_./-]+\Z")
ACCOUNT = re.compile(r"[a-z_][a-z0-9_-]{0,31}\Z")


def read_regular(path: Path) -> bytes:
    """Bound a regular file and reject symlink replacement before reading it."""
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_DOCUMENT:
        raise ValueError("EPERM service document kind or bound")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | os.O_NONBLOCK
    with os.fdopen(os.open(path, flags), "rb") as stream:
        opened = os.fstat(stream.fileno())
        if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
            raise ValueError("EPERM service document replacement")
        data = stream.read(MAX_DOCUMENT + 1)
        after = os.fstat(stream.fileno())
    if len(data) != opened.st_size or after.st_mtime_ns != opened.st_mtime_ns:
        raise ValueError("EPERM service document changed")
    return data


def absolute_path(value: Any) -> str:
    """Use a deliberately narrow path grammar shared by systemd and XML."""
    if (
        not isinstance(value, str)
        or len(value) > 512
        or not SAFE_PATH.fullmatch(value)
        or any(part in ("", ".", "..") for part in value.split("/")[1:])
    ):
        raise ValueError("EPERM service path; use an absolute path without spaces")
    return value


def substitutions(
    enrollment: dict[str, Any], prefix: Path, output: Path, package: dict[str, Any]
) -> dict[str, str]:
    """Validate all native-service substitutions before emitting any output."""
    required = {
        "schema",
        "user",
        "group",
        "state",
        "credentials",
        "target_host",
        "target_port",
    }
    if not required <= set(enrollment) or set(enrollment) - required - {
        "gpu",
        "snapshot_source",
        "field_bus",
        "evidence_enrollment_dir",
        "worker_evidence_enrollment_dir",
    }:
        raise ValueError("EPERM service enrollment fields")
    if enrollment["schema"] != "cohesix-host-service-enrollment/v1":
        raise ValueError("EPERM service enrollment schema")
    for field in ("user", "group"):
        value = enrollment[field]
        if (
            not isinstance(value, str)
            or not ACCOUNT.fullmatch(value)
            or value in ("root", "wheel")
        ):
            raise ValueError(
                "EPERM service account; select a dedicated unprivileged account"
            )
    host = enrollment["target_host"]
    try:
        address = ipaddress.ip_address(host)
    except (ValueError, TypeError) as error:
        raise ValueError("EPERM target host requires an explicit IP address") from error
    if address.is_unspecified or address.is_multicast:
        raise ValueError("EPERM target host address")
    port = enrollment["target_port"]
    if type(port) is not int or not 1 <= port <= 65535:
        raise ValueError("EPERM target port")
    values = {
        "PREFIX": absolute_path(str(prefix)),
        "ENROLLMENT": absolute_path(str(output)),
        "STATE": absolute_path(enrollment["state"]),
        "CREDENTIALS": absolute_path(enrollment["credentials"]),
        "USER": enrollment["user"],
        "GROUP": enrollment["group"],
        "TARGET_HOST": str(address),
        "TARGET_PORT": str(port),
        "SNAPSHOT_SOURCE": "",
        "GPU_AGENT_ARGS": "",
        "GPU_AGENT_CREDENTIAL": "",
        "GPU_DEVICE_POLICY": "",
        "FIELD_BUS_AGENT_ARGS": "",
        "FIELD_BUS_AGENT_PLIST": "",
        "EVIDENCE_AGENT_ARGS": "",
        "EVIDENCE_AGENT_PLIST": "",
        "AGENT_DEVICE_ISOLATION": "PrivateDevices=yes",
    }
    roots = [values[key] for key in ("PREFIX", "ENROLLMENT", "STATE", "CREDENTIALS")]
    if any(
        a == b or a.startswith(b + "/") or b.startswith(a + "/")
        for index, a in enumerate(roots)
        for b in roots[index + 1 :]
    ):
        raise ValueError(
            "EPERM package, state, credentials and enrollment must be separate trees"
        )
    profile = package["manifest"]["profile"]
    if profile["os"] == "linux" and any(
        path == protected or path.startswith(protected + "/")
        for path in roots
        for protected in ("/home", "/root", "/run/user")
    ):
        raise ValueError(
            "EPERM Linux service paths must remain visible under ProtectHome=yes"
        )
    if profile["os"] == "linux":
        source = enrollment.get("snapshot_source")
        if (
            not isinstance(source, str)
            or len(source) > 32
            or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", source)
            or ".." in source
        ):
            raise ValueError("EPERM explicit snapshot source required")
        values["SNAPSHOT_SOURCE"] = source
    if "field_bus" in enrollment and type(enrollment["field_bus"]) is not bool:
        raise ValueError("EPERM field bus enrollment must be boolean")
    if "evidence_enrollment_dir" in enrollment:
        evidence = absolute_path(enrollment["evidence_enrollment_dir"])
        values["EVIDENCE_AGENT_ARGS"] = f"--evidence-enrollment-dir {evidence}"
        values["EVIDENCE_AGENT_PLIST"] = (
            f"<string>--evidence-enrollment-dir</string><string>{evidence}</string>"
        )
    if "worker_evidence_enrollment_dir" in enrollment:
        if "evidence_enrollment_dir" not in enrollment:
            raise ValueError("EPERM Worker witness requires native evidence enrollment")
        witness = absolute_path(enrollment["worker_evidence_enrollment_dir"])
        if witness == enrollment["evidence_enrollment_dir"]:
            raise ValueError(
                "EPERM Worker witness requires a separate enrollment directory"
            )
        values["EVIDENCE_AGENT_ARGS"] += f" --worker-evidence-enrollment-dir {witness}"
        values[
            "EVIDENCE_AGENT_PLIST"
        ] += f"<string>--worker-evidence-enrollment-dir</string><string>{witness}</string>"
    if enrollment.get("field_bus"):
        if "evidence_enrollment_dir" not in enrollment:
            raise ValueError(
                "EPERM field bus service requires independent signed admission"
            )
        state = values["STATE"] + "/bus"
        values["FIELD_BUS_AGENT_ARGS"] = f"--field-bus-state-root {state}"
        values["FIELD_BUS_AGENT_PLIST"] = (
            f"<string>--field-bus-state-root</string><string>{state}</string>"
        )
    cuda = profile["id"] == "linux-aarch64-cuda"
    if cuda != ("gpu" in enrollment):
        raise ValueError("EPERM GPU enrollment must match the selected CUDA profile")
    if cuda:
        gpu = enrollment["gpu"]
        if not isinstance(gpu, dict) or set(gpu) != {"id", "uuid", "device_nodes"}:
            raise ValueError("EPERM GPU enrollment fields")
        if not isinstance(gpu["id"], str) or not re.fullmatch(
            r"GPU-[0-9]{1,3}", gpu["id"]
        ):
            raise ValueError("EPERM GPU identifier")
        if not isinstance(gpu["uuid"], str) or not re.fullmatch(
            r"[0-9a-f]{32}", gpu["uuid"]
        ):
            raise ValueError("EPERM GPU UUID")
        nodes = gpu["device_nodes"]
        if not isinstance(nodes, list) or not 1 <= len(nodes) <= 32:
            raise ValueError("ELIMIT GPU device allowlist")
        for node in nodes:
            absolute_path(node)
            if not node.startswith("/dev/"):
                raise ValueError("EPERM GPU device outside /dev")
        if len(set(nodes)) != len(nodes):
            raise ValueError("EPERM duplicate GPU device")
        values["GPU_DEVICE_POLICY"] = "\n".join(
            f"DeviceAllow={node} rw" for node in nodes
        )
        values["GPU_AGENT_CREDENTIAL"] = (
            f"LoadCredential=gpu_mac:{values['CREDENTIALS']}/gpu_mac"
        )
        values["GPU_AGENT_ARGS"] = (
            f"--execution-lanes 2 --gpu-executor-socket {values['STATE']}/gpu/executor.sock "
            "--gpu-executor-credential-ref file:%d/gpu_mac "
            f"--gpu-request-root {values['STATE']}/gpu/requests"
        )
    return values


def render(text: str, values: dict[str, str]) -> str:
    """Reject unknown placeholders; replacement text is never reparsed as a template."""

    def replace(match: re.Match[str]) -> str:
        key = match.group()[1:-1]
        if key not in values:
            raise ValueError("EPERM undeclared service placeholder")
        return values[key]

    result = TOKEN.sub(replace, text)
    if TOKEN.search(result):
        raise ValueError("EPERM unresolved service placeholder")
    return result


def verified_asset(prefix: Path, artifact: dict[str, Any]) -> bytes:
    """Retain the exact verified asset bytes even if a package file changes later."""
    path = prefix / artifact["requirement"]["path"]
    for parent in (path, *path.parents):
        if parent.is_symlink():
            raise ValueError("EPERM service asset symlink")
    data = read_regular(path)
    if (
        len(data) != artifact["bytes"]
        or hashlib.sha256(data).hexdigest() != artifact["sha256"]
    ):
        raise ValueError("EPERM changed verified service asset")
    return data


def validate_snapshot_source(source: str, manifest: dict[str, Any]) -> None:
    """Bind service publication to the verified package's compiled source pair."""
    try:
        host = manifest["ecosystem"]["host"]
        snapshots = host["snapshots"]
        matches = [row for row in snapshots["publishers"] if row["source_id"] == source]
        if (
            host["enable"] is not True
            or snapshots["enable"] is not True
            or len(matches) != 1
            or not matches[0]["providers"]
            or set(matches[0]["providers"])
            - {
                "systemd",
                "docker",
                "k8s",
                "nvidia",
                "jetson",
                "network",
                "launchd",
                "modbus",
                "dnp3",
            }
        ):
            raise ValueError("EPERM snapshot source/provider not enrolled")
    except (KeyError, TypeError) as error:
        raise ValueError("EPERM package snapshot enrollment missing") from error


def field_bus_devices(registry: dict[str, Any]) -> str:
    """Only compiled serial endpoint paths may become service device permissions."""
    endpoints = registry["contract"].get("field_bus", [])
    if not isinstance(endpoints, list) or not 1 <= len(endpoints) <= 64:
        raise ValueError("not_enabled compiled field bus maps")
    nodes = set()
    for endpoint in endpoints:
        transport = endpoint["transport"]
        if transport["kind"] == "serial":
            path = absolute_path(transport["path"])
            if not path.startswith("/dev/"):
                raise ValueError("EPERM field bus device outside /dev")
            nodes.add(path)
    if not nodes:
        return "PrivateDevices=yes"
    return "PrivateDevices=no\nDevicePolicy=closed\n" + "\n".join(
        f"DeviceAllow={path} rw" for path in sorted(nodes)
    )


def main() -> int:
    """Verify, render and atomically publish fresh service files without activation."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--verifier", required=True, type=Path, help="Already trusted coh executable"
    )
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--trust", required=True, type=Path)
    parser.add_argument("--enrollment", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    prefix = args.package.absolute()
    output = args.out.absolute()
    command = [
        str(args.verifier),
        "package",
        "verify",
        "--input",
        str(prefix),
        "--trust",
        str(args.trust),
    ]
    result = subprocess.run(command, check=True, capture_output=True, timeout=30)
    if len(result.stdout) > MAX_DOCUMENT:
        raise ValueError("ELIMIT verifier report")
    report = json.loads(result.stdout)
    manifest_bytes = read_regular(prefix / "package.json")
    if hashlib.sha256(manifest_bytes).hexdigest() != report["manifest_sha256"]:
        raise ValueError("EPERM package changed after verification")
    package = json.loads(manifest_bytes)
    enrollment = tomllib.loads(read_regular(args.enrollment).decode())
    values = substitutions(enrollment, prefix, output, package)
    artifacts = package["manifest"]["artifacts"]
    if enrollment.get("field_bus"):
        registry = next(
            (
                artifact
                for artifact in artifacts
                if artifact["requirement"]["path"] == "contracts/provider_registry.json"
            ),
            None,
        )
        if registry is None:
            raise ValueError("EPERM package provider registry missing")
        values["AGENT_DEVICE_ISOLATION"] = field_bus_devices(
            json.loads(verified_asset(prefix, registry))
        )
    if package["manifest"]["profile"]["os"] == "linux":
        selected = next(
            (
                artifact
                for artifact in artifacts
                if artifact["requirement"]["path"] == "config/root_task_resolved.json"
            ),
            None,
        )
        if selected is None:
            raise ValueError("EPERM package target manifest missing")
        validate_snapshot_source(
            values["SNAPSHOT_SOURCE"], json.loads(verified_asset(prefix, selected))
        )
    rendered: dict[str, bytes] = {}
    for artifact in artifacts:
        name = artifact["requirement"]["path"]
        if name.startswith(
            ("packaging/systemd/", "packaging/launchd/")
        ) and name.endswith(".in"):
            text = render(verified_asset(prefix, artifact).decode(), values)
            if name.endswith(".plist.in"):
                plistlib.loads(text.encode())
            filename = Path(name).name[:-3]
            if filename == "cohesix-export.service":
                filename = "cohesix-export@.service"
            rendered[filename] = text.encode()
    if not rendered:
        raise ValueError("ENOTSUP selected package has no service templates")
    if "gpu" in enrollment:
        by_name = {row["requirement"]["path"]: row for row in artifacts}
        root_manifest = json.loads(
            verified_asset(prefix, by_name["config/root_task_resolved.json"])
        )
        for service in ("executor", "inventory"):
            config = {
                "schema": "cohesix-gpu-executor-config/v1",
                "execution_lane": "systemd",
                "socket": f"{values['STATE']}/gpu/executor.sock",
                "state_root": f"{values['STATE']}/gpu/{service}",
                "helper": f"{prefix}/bin/cohesix-cuda-reference",
                "helper_sha256": by_name["bin/cohesix-cuda-reference"]["sha256"],
                "credential_ref": f"file:/run/credentials/cohesix-gpu-{service}.service/gpu_mac",
                "writer_epoch": root_manifest["authority"]["writer_epoch"],
                "gpu_id": enrollment["gpu"]["id"],
                "device_uuid": enrollment["gpu"]["uuid"],
                "provider_graph_sha256": package["manifest"]["provider_graph_sha256"],
            }
            rendered[f"gpu-{service}.json"] = (
                json.dumps(config, sort_keys=True, indent=2) + "\n"
            ).encode()
    if output.exists() or output.is_symlink():
        raise ValueError("EEXIST service output")
    if output.parent.stat().st_mode & 0o022:
        raise ValueError("EPERM service output parent writable by others")
    with tempfile.TemporaryDirectory(
        dir=output.parent, prefix=".cohesix-services-"
    ) as temporary:
        stage = Path(temporary)
        for name, data in rendered.items():
            with (stage / name).open("xb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            (stage / name).chmod(0o600)
        stage.rename(output)
    print(
        json.dumps(
            {
                "schema": "cohesix-service-render-report/v1",
                "authoritative": False,
                "profile_id": report["profile_id"],
                "manifest_sha256": report["manifest_sha256"],
                "files": sorted(rendered),
                "activated": False,
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
