# Author: Lukas Bower
# Purpose: Preserve service rendering boundaries for native accounts, paths and GPU device access.
# Copyright 2026 Lukas Bower
"""Independent service enrollment and template vectors; no service activation."""

import copy
import importlib.util
from pathlib import Path
import plistlib

import pytest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "render_host_services", ROOT / "scripts/install/render_host_services.py"
)
assert SPEC and SPEC.loader
services = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(services)


def enrollment() -> dict:
    return {
        "schema": "cohesix-host-service-enrollment/v1",
        "user": "cohesix",
        "group": "cohesix",
        "state": "/var/lib/cohesix",
        "credentials": "/etc/cohesix/credentials",
        "target_host": "127.0.0.1",
        "target_port": 31337,
        "snapshot_source": "linux-reference",
    }


def substitutions(value: dict, profile: str = "linux-aarch64-controller") -> dict:
    package = {"manifest": {"profile": {"id": profile, "os": "linux"}}}
    return services.substitutions(
        value, Path("/opt/cohesix/release"), Path("/etc/cohesix/services"), package
    )


def test_service_enrollment_rejects_privilege_path_and_directive_injection() -> None:
    assert substitutions(enrollment())["TARGET_PORT"] == "31337"
    for field, value in [
        ("user", "root"),
        ("user", "cohesix\nUser=root"),
        ("group", "wheel"),
        ("state", "/var/lib/../root"),
        ("state", "/opt/cohesix/release/state"),
        ("state", "/home/cohesix/state"),
        ("credentials", "/etc/cohesix/%d"),
        ("snapshot_source", "host\nExecStart=/bin/sh"),
        ("snapshot_source", "../other"),
        ("target_port", True),
        ("target_host", "0.0.0.0"),
    ]:
        candidate = enrollment()
        candidate[field] = value
        with pytest.raises(ValueError):
            substitutions(candidate)
    with pytest.raises(ValueError, match="placeholder"):
        services.render("User=@UNDECLARED@", substitutions(enrollment()))


def test_worker_witness_service_keeps_enrollment_separate_from_native_custody() -> None:
    candidate = enrollment()
    candidate["worker_evidence_enrollment_dir"] = "/etc/cohesix/worker-evidence"
    with pytest.raises(ValueError, match="native evidence"):
        substitutions(candidate)
    candidate["evidence_enrollment_dir"] = "/etc/cohesix/native-evidence"
    values = substitutions(candidate)
    assert values["EVIDENCE_AGENT_ARGS"] == (
        "--evidence-enrollment-dir /etc/cohesix/native-evidence "
        "--worker-evidence-enrollment-dir /etc/cohesix/worker-evidence"
    )
    assert (
        "<string>/etc/cohesix/worker-evidence</string>"
        in values["EVIDENCE_AGENT_PLIST"]
    )
    candidate["worker_evidence_enrollment_dir"] = candidate["evidence_enrollment_dir"]
    with pytest.raises(ValueError, match="separate enrollment"):
        substitutions(candidate)


def test_cuda_devices_require_exact_profile_and_never_a_directory_wildcard() -> None:
    candidate = enrollment()
    candidate["gpu"] = {
        "id": "GPU-0",
        "uuid": "12" * 16,
        "device_nodes": ["/dev/nvidia0", "/dev/nvidiactl"],
    }
    values = substitutions(candidate, "linux-aarch64-cuda")
    assert (
        values["GPU_DEVICE_POLICY"]
        == "DeviceAllow=/dev/nvidia0 rw\nDeviceAllow=/dev/nvidiactl rw"
    )
    assert "--execution-lanes 2" in values["GPU_AGENT_ARGS"]
    unit = services.render(
        (ROOT / "packaging/systemd/cohesix-gpu-executor.service.in").read_text(),
        values,
    )
    for required in (
        "TasksMax=64",
        "MemoryMax=1G",
        "MemorySwapMax=0",
        "CPUQuota=200%",
        "NoNewPrivileges=yes",
        "DevicePolicy=closed",
    ):
        assert required in unit.splitlines()
    with pytest.raises(ValueError):
        substitutions(candidate)
    for nodes in [[], ["/dev/*"], ["/etc/key"], ["/dev/nvidia0", "/dev/nvidia0"]]:
        invalid = copy.deepcopy(candidate)
        invalid["gpu"]["device_nodes"] = nodes
        with pytest.raises(ValueError):
            substitutions(invalid, "linux-aarch64-cuda")


def test_launchd_templates_parse_with_separate_file_credentials_and_explicit_activation() -> (
    None
):
    values = substitutions(enrollment())
    paths = sorted((ROOT / "packaging/launchd").glob("*.plist.in"))
    assert len(paths) == 2
    for path in paths:
        content = services.render(path.read_text(), values)
        parsed = plistlib.loads(content.encode())
        assert parsed["RunAtLoad"] is False
        assert parsed["KeepAlive"] is False
        assert parsed["Umask"] == 0o077
        assert parsed["ProgramArguments"][0].startswith("/opt/cohesix/release/bin/")
        for name, value in parsed["EnvironmentVariables"].items():
            if name != "RUST_LOG":
                assert value.startswith("file:/etc/cohesix/credentials/")


def test_snapshot_service_source_requires_the_verified_compiler_pair() -> None:
    manifest = {
        "ecosystem": {
            "host": {
                "enable": True,
                "snapshots": {
                    "enable": True,
                    "publishers": [
                        {"source_id": "linux-reference", "providers": ["systemd"]}
                    ],
                },
            }
        }
    }
    services.validate_snapshot_source("linux-reference", manifest)
    with pytest.raises(ValueError, match="not enrolled"):
        services.validate_snapshot_source("foreign-source", manifest)
    manifest["ecosystem"]["host"]["snapshots"]["publishers"][0]["providers"] = [
        "network",
        "modbus",
    ]
    services.validate_snapshot_source("linux-reference", manifest)
    manifest["ecosystem"]["host"]["snapshots"]["publishers"][0]["providers"] = [
        "unknown"
    ]
    with pytest.raises(ValueError, match="not enrolled"):
        services.validate_snapshot_source("linux-reference", manifest)


def test_field_bus_service_requires_signed_admission_and_exact_serial_devices() -> None:
    candidate = enrollment()
    candidate["field_bus"] = True
    with pytest.raises(ValueError, match="signed admission"):
        substitutions(candidate)
    candidate["evidence_enrollment_dir"] = "/var/lib/cohesix/enrollments/native"
    values = substitutions(candidate)
    assert (
        values["FIELD_BUS_AGENT_ARGS"] == "--field-bus-state-root /var/lib/cohesix/bus"
    )
    registry = {
        "contract": {
            "field_bus": [{"transport": {"kind": "serial", "path": "/dev/ttyUSB0"}}]
        }
    }
    assert (
        services.field_bus_devices(registry)
        == "PrivateDevices=no\nDevicePolicy=closed\nDeviceAllow=/dev/ttyUSB0 rw"
    )
    registry["contract"]["field_bus"][0]["transport"] = {
        "kind": "tcp",
        "address": "127.0.0.1:502",
    }
    assert services.field_bus_devices(registry) == "PrivateDevices=yes"
    for path in [
        "/dev/*",
        "/etc/secret",
        "/dev/../etc/key",
        "/dev/ttyUSB0\nDeviceAllow=/dev/* rw",
    ]:
        registry["contract"]["field_bus"][0]["transport"] = {
            "kind": "serial",
            "path": path,
        }
        with pytest.raises(ValueError):
            services.field_bus_devices(registry)


def test_peft_requires_cuda_profile_private_state_and_independent_custody() -> None:
    candidate = enrollment()
    candidate.update(
        {
            "gpu": {"id": "GPU-0", "uuid": "12" * 16, "device_nodes": ["/dev/nvidia0"]},
            "evidence_enrollment_dir": "/etc/cohesix/native",
            "worker_evidence_enrollment_dir": "/etc/cohesix/worker",
            "peft": {
                "agent_config": "/var/lib/cohesix/peft/agent.json",
                "runtime_dir": "/run/user/1001",
            },
        }
    )
    values = substitutions(candidate, "linux-aarch64-cuda")
    assert (
        values["PEFT_AGENT_ARGS"]
        == "--peft-release-config /var/lib/cohesix/peft/agent.json"
    )
    unit = services.render(
        (ROOT / "packaging/systemd/cohesix-ticket-agent.service.in").read_text(), values
    )
    assert "Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1001/bus" in unit
    assert "ProtectHome=tmpfs" in unit
    assert "BindReadOnlyPaths=/run/user/1001/bus /run/user/1001/systemd/private" in unit
    for field, value in [
        ("agent_config", "/etc/outside.json"),
        ("runtime_dir", "/run/user/1001\nUser=root"),
        ("runtime_dir", "/run/user/0"),
    ]:
        invalid = copy.deepcopy(candidate)
        invalid["peft"][field] = value
        with pytest.raises(ValueError):
            substitutions(invalid, "linux-aarch64-cuda")
    del candidate["worker_evidence_enrollment_dir"]
    with pytest.raises(ValueError, match="custody"):
        substitutions(candidate, "linux-aarch64-cuda")
    with pytest.raises(ValueError, match="CUDA"):
        substitutions(candidate)
