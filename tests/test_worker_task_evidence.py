# Author: Lukas Bower
# Purpose: Verify strict Milestone 26e Worker evidence validation and promotion.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from scripts import worker_task_evidence as evidence


ROOT = Path(__file__).resolve().parents[1]


def _hash(label: str) -> str:
    return hashlib.sha256(label.encode("utf-8")).hexdigest()


def _write(path: Path, value: dict[str, object]) -> bytes:
    raw = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    return raw


def _session(target: str) -> dict[str, str]:
    return {
        "target": target,
        "source_sha256": _hash(f"{target}-source"),
        "manifest_sha256": _hash(f"{target}-manifest"),
        "kernel_sha256": _hash(f"{target}-kernel"),
        "root_image_sha256": _hash(f"{target}-root"),
        "driver_archive_sha256": _hash(f"{target}-driver-archive"),
        "driver_manifest_sha256": _hash(f"{target}-driver-manifest"),
        "cyw43_coexistence_record_sha256": _hash(f"{target}-cyw43"),
        "worker_archive_sha256": _hash(f"{target}-worker-archive"),
        "worker_image_manifest_sha256": _hash(f"{target}-worker-manifest"),
        "worker_abi_sha256": _hash("worker-abi"),
    }


def _artifact(identifier: str) -> dict[str, object]:
    return {"id": identifier, "sha256": _hash(identifier), "bytes": 64}


def _outcomes(identifiers: tuple[str, ...]) -> list[dict[str, str]]:
    return [
        {"id": identifier, "class": "observation", "result": "pass"}
        for identifier in identifiers
    ]


def _integration(target: str, dependency_id: str) -> dict[str, object]:
    return {
        "schema": evidence.INTEGRATION_SCHEMA,
        "record_kind": "worker-integration",
        "dependency_id": dependency_id,
        "owner_milestone": "m26e-host-worker-integration",
        "obligation": "role_required",
        "observed_mode": "live",
        "dependency_graph_sha256": _hash("dependency-graph"),
        "manifest_sha256": _session(target)["manifest_sha256"],
        "component_sha256": _hash(dependency_id),
        "config_sha256": _hash("host-matrix"),
        "host": {
            "profile": "macos-arm64",
            "os": "macOS 26",
            "architecture": "arm64",
        },
        "target_session": _session(target),
        "execution_proof": evidence.TARGET_PROOF[target],
        "outcomes": [
            {"id": "receipt", "class": "receipt", "result": "confirmed"}
        ],
        "raw_evidence": [_artifact(f"{target}-{dependency_id}")],
        "verdict": "PASS",
        "blockers": [],
    }


def _component(
    target: str,
    references: list[dict[str, str]] | None = None,
) -> dict[str, object]:
    if references is None:
        references = [
            {
                "id": dependency_id,
                "record_kind": "worker-integration",
                "sha256": _hash(f"{target}-{dependency_id}"),
            }
            for dependency_id in evidence.REQUIRED_INTEGRATIONS
        ]
    roles = {
        "worker-heartbeat": {"role_index": 1, "core": 3, "budget_us": 300},
        "worker-gpu": {"role_index": 2, "core": 2, "budget_us": 400},
        "worker-lora": {"role_index": 4, "core": 2, "budget_us": 400},
    }
    workers = []
    for index, role in enumerate(evidence.REQUIRED_ROLES):
        role_config = roles[role]
        workers.append(
            {
                "identity": {
                    "role": role,
                    "slot": 0,
                    "lease_epoch": 1,
                    "supervisor_generation": index + 1,
                    "cap_generation": 1,
                },
                "state": {
                    "declaration": "executable",
                    "lifecycle": "ready",
                    "artifact": "verified",
                    "receipt": "none" if role == "worker-heartbeat" else "confirmed",
                    "execution_proof": evidence.TARGET_PROOF[target],
                },
                "image_sha256": _hash(role),
                "ready_sequence": 1,
                "completion_sequence": 2,
                "endpoint_badge": 638_324_736
                + ((role_config["role_index"] << 8) | 1),
                "fault_badge": 652_279_808 + index,
                "core": role_config["core"],
                "scheduling_context": {
                    "budget_us": role_config["budget_us"],
                    "period_us": 10_000,
                },
                "object_inventory": _per_slot_inventory(),
            }
        )
    return {
        "schema": evidence.COMPONENT_SCHEMA,
        "record_kind": "target-component",
        "target": target,
        "target_session": _session(target),
        "topology_sha256": _generated_record(target)["topology_sha256"],
        "workers": workers,
        "integration_evidence": references,
        "outcomes": _outcomes(evidence.COMPONENT_REQUIRED_OUTCOMES),
        "raw_evidence": [_artifact(f"{target}-worker-transcript")],
        "verdict": "PASS",
        "blockers": [],
    }


def _inventory() -> dict[str, int]:
    return {
        "tcbs": 10,
        "cnodes": 10,
        "vspaces": 10,
        "page_tables": 280,
        "asids": 10,
        "frames": 2_096,
        "endpoints": 16,
        "notifications": 19,
        "fault_caps": 10,
        "timeout_fault_caps": 10,
        "reply_objects": 7,
        "scheduling_contexts": 10,
        "cspace_slots": 4_288,
        "untyped_bytes": 70_254_592,
    }


def _per_slot_inventory() -> dict[str, int]:
    return {
        "tcbs": 1,
        "cnodes": 1,
        "vspaces": 1,
        "page_tables": 8,
        "asids": 1,
        "frames": 16,
        "endpoints": 0,
        "notifications": 1,
        "fault_caps": 1,
        "timeout_fault_caps": 1,
        "reply_objects": 0,
        "scheduling_contexts": 1,
        "cspace_slots": 64,
        "untyped_bytes": 1_048_576,
    }


def _fixed_inventory() -> dict[str, int]:
    inventory = _inventory()
    per_slot = _per_slot_inventory()
    return {key: inventory[key] - 3 * per_slot[key] for key in evidence.INVENTORY_KEYS}


def _generated_record(target: str) -> dict[str, object]:
    role_config = (
        ("worker-heartbeat", 3, 300),
        ("worker-gpu", 2, 400),
        ("worker-lora", 2, 400),
    )
    topology = {
        "profile": {"name": evidence.TARGET_PROFILE[target], "kernel": True},
        "root_task": {},
        "worker_runtime": {
            "endpoint_caps": {
                "required": True,
                "attach_badge_base": 638_324_736,
                "epoch_bits": 8,
            }
        },
        "temporal_authority": {
            "tasks": [
                {
                    "id": f"{role}-slot-0",
                    "kind": "worker",
                    "core": core,
                    "budget_us": budget,
                    "period_us": 10_000,
                }
                for role, core, budget in role_config
            ]
        },
        "worker_resource_admission": {
            "enabled": True,
            "fixed_objects": _fixed_inventory(),
            "executable_roles": [
                {
                    "role": role,
                    "task_prefix": f"{role}-slot-",
                    "executable_slots": 1,
                    "core": core,
                    "per_slot": _per_slot_inventory(),
                }
                for role, core, _ in role_config
            ],
            "allowed_role_mixes": [
                {
                    "id": "maximum-three-role-mix",
                    "maximum": True,
                    "roles": [
                        {"role": role, "count": 1} for role, _, _ in role_config
                    ],
                }
            ],
            "handoff": {
                "worker_fault_badges": {
                    "base": 652_279_808,
                    "count": 3,
                    "stride": 1,
                }
            },
        },
        "ninedoor_service": {},
        "console_network_service": {},
    }
    return {
        "schema": evidence.GENERATED_INVENTORY_SCHEMA,
        "profile": evidence.TARGET_PROFILE[target],
        "manifest_sha256": _session(target)["manifest_sha256"],
        "topology_sha256": evidence._canonical_json_sha256(topology),  # noqa: SLF001
        "topology": topology,
        "inventory": _inventory(),
    }


def _root(target: str, worker_raw: bytes) -> dict[str, object]:
    return {
        "schema": evidence.ROOT_SCHEMA,
        "record_kind": "root-tcb",
        "target": target,
        "target_session": _session(target),
        "worker_component": {
            "id": f"worker-component-{target}",
            "record_kind": "target-component",
            "sha256": hashlib.sha256(worker_raw).hexdigest(),
        },
        "topology_sha256": _generated_record(target)["topology_sha256"],
        "generated_inventory": _inventory(),
        "inventory_scope": "admitted-maximum",
        "observed_inventory": _inventory(),
        "outcomes": _outcomes(evidence.ROOT_REQUIRED_OUTCOMES),
        "raw_evidence": [_artifact(f"{target}-root-transcript")],
        "verdict": "PASS",
        "blockers": [],
    }


def _system(
    target: str,
    worker: dict[str, object],
    worker_raw: bytes,
    root: dict[str, object],
    root_raw: bytes,
) -> dict[str, object]:
    return {
        "schema": evidence.SYSTEM_SCHEMA,
        "record_kind": "full-system",
        "target": target,
        "target_session": worker["target_session"],
        "worker_component": {
            "id": f"worker-component-{target}",
            "record_kind": "target-component",
            "sha256": hashlib.sha256(worker_raw).hexdigest(),
        },
        "root_tcb": {
            "id": f"root-tcb-{target}",
            "record_kind": "root-tcb",
            "sha256": hashlib.sha256(root_raw).hexdigest(),
        },
        "topology_sha256": root["topology_sha256"],
        "core_admission": [
            {"core": core, "capacity_us": 1000, "reserve_us": 100, "admitted_us": 700}
            for core in range(4)
        ],
        "outcomes": _outcomes(evidence.SYSTEM_REQUIRED_OUTCOMES),
        "raw_evidence": [_artifact(f"{target}-system-transcript")],
        "verdict": "PASS",
        "blockers": [],
    }


def _target_graph(root_dir: Path, target: str) -> tuple[Path, Path, Path]:
    integration_references = []
    component_dir = root_dir / target
    for dependency_id in evidence.REQUIRED_INTEGRATIONS:
        record = _integration(target, dependency_id)
        raw = _write(component_dir / "integration" / f"{dependency_id}.json", record)
        integration_references.append(
            {
                "id": dependency_id,
                "record_kind": "worker-integration",
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )
    worker = _component(target, integration_references)
    worker_path = component_dir / "worker-task-evidence.json"
    worker_raw = _write(worker_path, worker)
    root = _root(target, worker_raw)
    root_path = component_dir / "root-tcb-acceptance.json"
    root_raw = _write(root_path, root)
    system = _system(target, worker, worker_raw, root, root_raw)
    system_path = component_dir / "system-acceptance.json"
    _write(system_path, system)
    return worker_path, root_path, system_path


def _component_emitter_inputs(
    root_dir: Path,
    target: str,
) -> tuple[Path, Path, Path, Path]:
    session_path = root_dir / "target-session.json"
    session_raw = _write(session_path, _session(target))
    generated_path = root_dir / "root_task_topology.json"
    _write(generated_path, _generated_record(target))
    integration_dir = root_dir / "integration"
    for dependency_id in evidence.REQUIRED_INTEGRATIONS:
        _write(
            integration_dir / f"{dependency_id}.json",
            _integration(target, dependency_id),
        )
    component = _component(target)
    observations = {
        "schema": evidence.COMPONENT_OBSERVATIONS_SCHEMA,
        "target": target,
        "target_session_sha256": hashlib.sha256(session_raw).hexdigest(),
        "workers": component["workers"],
        "outcomes": component["outcomes"],
        "raw_evidence": component["raw_evidence"],
        "verdict": "PASS",
        "blockers": [],
    }
    observations_path = root_dir / "component-observations.json"
    _write(observations_path, observations)
    return session_path, generated_path, observations_path, integration_dir


def _root_emitter_inputs(
    root_dir: Path,
    target: str,
    session_path: Path,
) -> tuple[Path, Path]:
    session_raw = session_path.read_bytes()
    generated_path = root_dir / "generated-inventory.json"
    generated = _generated_record(target)
    _write(generated_path, generated)
    observations_path = root_dir / "root-observations.json"
    _write(
        observations_path,
        {
            "schema": evidence.ROOT_OBSERVATIONS_SCHEMA,
            "target": target,
            "target_session_sha256": hashlib.sha256(session_raw).hexdigest(),
            "topology_sha256": generated["topology_sha256"],
            "inventory_scope": "admitted-maximum",
            "observed_inventory": _inventory(),
            "outcomes": _outcomes(evidence.ROOT_REQUIRED_OUTCOMES),
            "raw_evidence": [_artifact(f"{target}-root-transcript")],
            "verdict": "PASS",
            "blockers": [],
        },
    )
    return generated_path, observations_path


def _live_qemu_inputs(root_dir: Path) -> SimpleNamespace:
    role_names = {
        "worker-heartbeat": "worker-heart",
        "worker-gpu": "worker-gpu",
        "worker-lora": "worker-lora",
    }
    elf_paths: dict[str, Path] = {}
    elf_raw: dict[str, bytes] = {}
    image_hashes: dict[str, str] = {}
    for role in evidence.REQUIRED_ROLES:
        raw = f"unstripped:{role}".encode("utf-8")
        path = root_dir / "target" / role_names[role]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        elf_paths[role] = path
        elf_raw[role] = raw
        image_hashes[role] = _hash(f"canonical:{role}")

    archive_raw = b"canonical-worker-archive"
    archive_path = root_dir / "cohesix-worker-images.cpio"
    archive_path.write_bytes(archive_raw)
    manifest = {
        "schema": "cohesix-worker-image-manifest/v1",
        "profile": "release",
        "target": "aarch64-unknown-none",
        "archive": {
            "bytes": len(archive_raw),
            "sha256": hashlib.sha256(archive_raw).hexdigest(),
        },
        "images": [
            {
                "name": role_names[role],
                "role": role,
                "entry_symbol": "_start",
                "source_sha256": hashlib.sha256(elf_raw[role]).hexdigest(),
                "image_sha256": image_hashes[role],
            }
            for role in evidence.REQUIRED_ROLES
        ],
    }
    manifest_path = root_dir / "cohesix-worker-image-manifest.json"
    manifest_raw = _write(manifest_path, manifest)

    driver_archive_raw = b"canonical-driver-runtime-archive"
    driver_archive_path = root_dir / "cohesix-driver-runtimes.cpio"
    driver_archive_path.write_bytes(driver_archive_raw)
    service_paths: dict[str, Path] = {}
    service_raw: dict[str, bytes] = {}
    for service in evidence.QEMU_SERVICE_SYMBOLS:
        raw = f"unstripped:{service}".encode("utf-8")
        path = root_dir / "target" / service
        path.write_bytes(raw)
        service_paths[service] = path
        service_raw[service] = raw
    root_elf_raw = b"unstripped:root-task"
    root_elf_path = root_dir / "target" / "root-task"
    root_elf_path.write_bytes(root_elf_raw)

    qemu_out = root_dir / "qemu-out"
    sel4_build = root_dir / "sel4-build"
    sel4_build.mkdir(parents=True)
    launch_bytes = {
        "elfloader": b"canonical-elfloader",
        "kernel": b"canonical-kernel",
        "rootserver": root_elf_raw,
        "initrd": b"canonical-system-cpio",
    }
    launch_rows = []
    for identifier, relative in evidence.QEMU_LAUNCH_ARTIFACTS:
        path = qemu_out / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        raw = launch_bytes[identifier]
        path.write_bytes(raw)
        launch_rows.append(
            {
                "id": identifier,
                "path": relative.as_posix(),
                "bytes": len(raw),
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )
    launch_record_path = qemu_out / "cohesix-qemu-launch-artifacts.json"
    _write(
        launch_record_path,
        {
            "schema": evidence.QEMU_LAUNCH_SCHEMA,
            "profile": "release",
            "cargo_target": "aarch64-unknown-none",
            "root_task_features": "release-qemu,bootstrap-trace",
            "sel4_build_dir": str(sel4_build.resolve()),
            "gic_version": "3",
            "artifacts": launch_rows,
        },
    )

    session = _session("qemu")
    source_inventory_raw = b'{"schema":"cohesix-source-inventory/v1"}\n'
    worker_abi_raw = b'{"schema":"cohesix-worker-abi-identity/v1"}\n'
    cyw43_raw = b'{"schema":"cohesix-cyw43-coexistence-binding/v1"}\n'
    (root_dir / "source-inventory.json").write_bytes(source_inventory_raw)
    (root_dir / "worker-abi-identity.json").write_bytes(worker_abi_raw)
    (root_dir / "qemu-cyw43-coexistence.json").write_bytes(cyw43_raw)
    session["source_sha256"] = hashlib.sha256(source_inventory_raw).hexdigest()
    session["worker_abi_sha256"] = hashlib.sha256(worker_abi_raw).hexdigest()
    session["cyw43_coexistence_record_sha256"] = hashlib.sha256(cyw43_raw).hexdigest()
    session["kernel_sha256"] = hashlib.sha256(launch_bytes["kernel"]).hexdigest()
    session["root_image_sha256"] = hashlib.sha256(root_elf_raw).hexdigest()
    session["driver_archive_sha256"] = hashlib.sha256(driver_archive_raw).hexdigest()
    session["worker_archive_sha256"] = hashlib.sha256(archive_raw).hexdigest()
    session["worker_image_manifest_sha256"] = hashlib.sha256(manifest_raw).hexdigest()
    session_path = root_dir / "target-session.json"
    session_raw = _write(session_path, session)

    auth_uart_path = root_dir / "auth-uart.log"
    auth_uart_path.write_text(
        "[cohsh-net][auth] auth OK, session established (generation=1 conn_id=1)\n",
        encoding="utf-8",
    )
    operation_path = root_dir / "scripts" / "cohsh" / "9p_batch.coh"
    operation_path.parent.mkdir(parents=True)
    operation_path.write_text("attach queen\nEXPECT OK\nls /\n", encoding="utf-8")

    def observation_file(path: Path) -> dict[str, object]:
        raw = path.read_bytes()
        return {
            "path": str(path.resolve()),
            "present": True,
            "size_bytes": len(raw),
            "sha256": hashlib.sha256(raw).hexdigest(),
        }

    auth_observation_path = root_dir / "target-observation.json"
    _write(
        auth_observation_path,
        {
            "schema": evidence.QEMU_AUTH_OBSERVATION_SCHEMA,
            "banner": "NON-CLAIMING TARGET DIAGNOSTIC",
            "claiming": False,
            "result": "PASS",
            "first_failing_proof_layer": None,
            "detail": "root/service readiness and one live operation proved",
            "target": "qemu",
            "focus": "ninedoor",
            "run_id": "fixture-auth-pass",
            "profile": evidence.QEMU_AUTH_OBSERVATION_PROFILE,
            "serial_log": observation_file(auth_uart_path),
            "serial_source_log": str(auth_uart_path.resolve()),
            "built_image": observation_file(qemu_out / "cohesix-system.cpio"),
            "image_identity": observation_file(launch_record_path),
            "operation_script": observation_file(operation_path),
        },
    )

    generated = _generated_record("qemu")
    temporal = generated["topology"]["temporal_authority"]
    temporal["core_admission"] = [
        {"core": core, "capacity_us": 10_000, "reserve_us": 1_000}
        for core in range(4)
    ]
    for index, task in enumerate(temporal["tasks"]):
        task["admitted"] = True
        task["timeout_badge"] = 653_131_784 + index
    generated["topology_sha256"] = evidence._canonical_json_sha256(  # noqa: SLF001
        generated["topology"]
    )
    generated_path = root_dir / "root_task_topology.json"
    _write(generated_path, generated)

    integration_dir = root_dir / "integration"
    for dependency_id in evidence.REQUIRED_INTEGRATIONS:
        record = _integration("qemu", dependency_id)
        record["target_session"] = session
        record["manifest_sha256"] = session["manifest_sha256"]
        _write(integration_dir / f"{dependency_id}.json", record)

    roles = {
        row["role"]: row
        for row in generated["topology"]["worker_resource_admission"][
            "executable_roles"
        ]
    }
    worker_tasks = [
        task
        for task in temporal["tasks"]
        if task.get("kind") == "worker"
    ]
    fault_base = generated["topology"]["worker_resource_admission"]["handoff"][
        "worker_fault_badges"
    ]["base"]
    endpoint_base = generated["topology"]["worker_runtime"]["endpoint_caps"][
        "attach_badge_base"
    ]
    role_bits = {"worker-heartbeat": 1, "worker-gpu": 2, "worker-lora": 4}

    lines = [
        "[critical] exact generated fault registry sealed sources=10",
        "[critical] independent fault/emergency/Worker/driver duties active",
        "[worker] target supervisor armed after exact registry and critical activation",
        "GPU_BRIDGE_FIXTURE_ADMISSION source=gpu-bridge-host/mock mode=fixture profile=qemu gate=bootstrap-trace state=admitted",
        "LORA_EXPORT_FIXTURE_ADMISSION source=gpu-bridge-host/mock job=qemu-evidence-job mode=fixture profile=qemu gate=bootstrap-trace state=admitted",
        "[ninedoor-service] generation=1 terminal-fault class=Standard sequence=1",
        "[console-network] generation=1 terminal-fault class=Standard sequence=2",
        "[console-network] generation=2 terminal-fault class=Timeout sequence=3",
        "NINEDOOR_SERVICE_TEARDOWN generation=1 tcb_suspended=yes mappings_scrubbed=yes recovery_reply_revoked=yes capabilities_revoked=yes generation_fenced=yes state=terminal",
        "CONSOLE_NETWORK_TEARDOWN generation=1 tcb_suspended=yes scheduling_context_unbound=yes mappings_scrubbed=yes capabilities_revoked=yes objects_deleted=yes generation_fenced=yes state=terminal",
        "CONSOLE_NETWORK_TEARDOWN generation=2 tcb_suspended=yes scheduling_context_unbound=yes mappings_scrubbed=yes capabilities_revoked=yes objects_deleted=yes generation_fenced=yes state=terminal",
        "ROOT_TCB_INVENTORY "
        "scope=admitted-maximum "
        + " ".join(f"{key}={_inventory()[key]}" for key in evidence.INVENTORY_KEYS)
        + " state=sealed",
        "ROOT_CRITICAL_OBJECTS scope=constructed-actual duties=5 restricted_children=4 "
        "tcbs=5 scheduling_contexts=5 reply_objects=6 standard_fault_caps=4 "
        "timeout_fault_caps=4 fault_registrations=10 state=sealed",
    ]

    def identity(role: str, generation: int) -> str:
        return (
            f"role={role} slot=0 lease_epoch={generation} "
            f"supervisor_generation={generation} cap_generation={generation}"
        )

    def admission(role: str, generation: int) -> None:
        role_row = roles[role]
        task = next(task for task in worker_tasks if task["id"] == f"{role}-slot-0")
        ordinal = worker_tasks.index(task)
        endpoint = endpoint_base + ((role_bits[role] << 8) | generation)
        fields = [
            "WORKER_TASK_ADMISSION",
            identity(role, generation),
            f"image_sha256={image_hashes[role]}",
            f"endpoint_badge={endpoint}",
            f"fault_badge={fault_base + ordinal}",
            f"core={role_row['core']}",
            f"sc_budget_us={task['budget_us']}",
            f"sc_period_us={task['period_us']}",
            *(f"{key}={role_row['per_slot'][key]}" for key in evidence.INVENTORY_KEYS),
            "state=admitted",
        ]
        lines.append(" ".join(fields))

    def ready(role: str, generation: int) -> None:
        lines.append(f"WORKER_TASK_READY {identity(role, generation)} sequence=1")

    def control(
        role: str, generation: int, action: int, outcome: int, sequence: int
    ) -> None:
        lines.append(
            f"WORKER_TASK_CONTROL {identity(role, generation)} "
            f"action=0x{action:04x} outcome={outcome} sequence={sequence} state=admitted"
        )

    def receipt_completion(
        role: str, generation: int, action: int, outcome: int, sequence: int
    ) -> None:
        lines.append(
            f"WORKER_TASK_RECEIPT {identity(role, generation)} "
            f"action=0x{action:04x} outcome={outcome} sequence={sequence}"
        )
        lines.append(
            f"WORKER_TASK_COMPLETION {identity(role, generation)} "
            f"action=0x{action:04x} status={outcome} sequence={sequence}"
        )

    def fault(role: str, generation: int, fault_class: str) -> None:
        task = next(task for task in worker_tasks if task["id"] == f"{role}-slot-0")
        ordinal = worker_tasks.index(task)
        badge = task["timeout_badge"] if fault_class == "Timeout" else fault_base + ordinal
        lines.append(
            f"WORKER_TASK_FAULT {identity(role, generation)} class={fault_class} "
            f"observed_badge={badge} state=faulted"
        )
        lines.append(
            f"WORKER_TASK_TEARDOWN {identity(role, generation)} reason="
            f"{'timeout' if fault_class == 'Timeout' else 'fault'} "
            "tcb_suspended=yes records_cleared=yes scheduling_context_unbound=yes "
            "mappings_scrubbed=yes descendants_revoked=yes objects_deleted=yes "
            "generation_fenced=yes state=terminal"
        )

    def teardown(role: str, generation: int, reason: str) -> None:
        lines.append(
            f"WORKER_TASK_TEARDOWN {identity(role, generation)} reason={reason} "
            "tcb_suspended=yes records_cleared=yes scheduling_context_unbound=yes "
            "mappings_scrubbed=yes descendants_revoked=yes objects_deleted=yes "
            "generation_fenced=yes state=terminal"
        )

    admission("worker-heartbeat", 1)
    fault("worker-heartbeat", 1, "Standard")
    admission("worker-heartbeat", 2)
    ready("worker-heartbeat", 2)
    control("worker-heartbeat", 2, 0x0101, 0, 1)
    fault("worker-heartbeat", 2, "Standard")
    admission("worker-heartbeat", 3)
    ready("worker-heartbeat", 3)
    control("worker-heartbeat", 3, 0x0101, 0, 1)
    fault("worker-heartbeat", 3, "Timeout")
    admission("worker-heartbeat", 4)
    ready("worker-heartbeat", 4)
    control("worker-heartbeat", 4, 0x0101, 0, 1)
    lines.append(
        f"WORKER_TASK_COMPLETION {identity('worker-heartbeat', 4)} "
        "action=0x0101 status=1 sequence=1"
    )

    final_generation = {"worker-heartbeat": 4, "worker-gpu": 5, "worker-lora": 4}
    final_sequences = {"worker-heartbeat": 1, "worker-gpu": 1, "worker-lora": 12}
    for role in ("worker-gpu", "worker-lora"):
        admission(role, 1)
        fault(role, 1, "Standard")
        admission(role, 2)
        ready(role, 2)
        control(role, 2, 0x0201 if role == "worker-gpu" else 0x0301, 1, 1)
        fault(role, 2, "Standard")
        admission(role, 3)
        ready(role, 3)
        control(role, 3, 0x0201 if role == "worker-gpu" else 0x0301, 1, 1)
        fault(role, 3, "Timeout")
        admission(role, 4)
        ready(role, 4)
        sequence = 0
        actions = (
            (0x0201, 0x0202, 0x0203)
            if role == "worker-gpu"
            else (0x0301, 0x0302, 0x0303, 0x0304)
        )
        for action in actions:
            for outcome in (1, 2, 8):
                sequence += 1
                control(role, 4, action, outcome, sequence)
                receipt_completion(role, 4, action, outcome, sequence)
        if role == "worker-gpu":
            teardown(role, 4, "shutdown")
            admission(role, 5)
            ready(role, 5)
            control(role, 5, 0x0201, 1, 1)
            receipt_completion(role, 5, 0x0201, 1, 1)

    uart_text = "\n".join(lines) + "\n"
    def gdb_text(inject_role: str) -> str:
        return (
            "\n".join(
                [
                    "M26E_QEMU_SESSION target=qemu machine=virt gic_version=3 "
                    f"root_image_sha256={session['root_image_sha256']} "
                    f"worker_archive_sha256={session['worker_archive_sha256']} "
                    f"topology_sha256={generated['topology_sha256']}",
                    *(
                        f"M26E_GDB_ELF role={role} "
                        f"elf_sha256={hashlib.sha256(elf_raw[role]).hexdigest()} "
                        f"image_sha256={image_hashes[role]}"
                        for role in evidence.REQUIRED_ROLES
                    ),
                    f"M26E_GDB_INJECTION role={inject_role} phase=pre-ready symbol=_start action=zero-x0 result=continued",
                    f"M26E_GDB_INJECTION role={inject_role} phase=during-ipc symbol=cohesix_worker_qemu_evidence_control_handler action=redirect-standard-fault result=continued",
                    f"M26E_GDB_INJECTION role={inject_role} phase=budget-exhaustion symbol=cohesix_worker_qemu_evidence_control_handler action=redirect-timeout-spin result=continued",
                ]
            )
            + "\n"
        )

    def service_gdb_text(service: str, mode: str) -> str:
        handler = evidence.QEMU_SERVICE_SYMBOLS[service][0]
        lines = [
            "M26E_QEMU_SESSION target=qemu machine=virt gic_version=3 "
            f"root_image_sha256={session['root_image_sha256']} "
            f"worker_archive_sha256={session['worker_archive_sha256']} "
            f"topology_sha256={generated['topology_sha256']}",
            "M26E_QEMU_AUTH result=PASS "
            f"observation_sha256={_hash('auth-observation')} "
            "observation_bytes=64 "
            f"serial_sha256={_hash('auth-uart')} "
            "serial_bytes=64 "
            f"launch_record_sha256={_hash('launch-record')} "
            "launch_record_bytes=64 "
            f"target_session_sha256={hashlib.sha256(session_raw).hexdigest()} "
            f"target_session_bytes={len(session_raw)}",
            f"M26E_GDB_SERVICE_ELF service={service} mode={mode} "
            f"elf_sha256={hashlib.sha256(service_raw[service]).hexdigest()} "
            f"elf_bytes={len(service_raw[service])} "
            f"root_image_sha256={session['root_image_sha256']}",
        ]
        if mode == "between-calls-revoke":
            lines.extend(
                [
                    "M26E_GDB_SERVICE_ROOT_ELF "
                    f"service={service} mode={mode} "
                    f"elf_sha256={hashlib.sha256(root_elf_raw).hexdigest()} "
                    f"elf_bytes={len(root_elf_raw)} "
                    f"root_image_sha256={session['root_image_sha256']}",
                    "M26E_GDB_SERVICE_INJECTION service=ninedoor-service "
                    "phase=between-calls "
                    f"symbol={evidence.QEMU_NINEDOOR_ROOT_SYMBOLS[0]} "
                    "action=redirect-local-revoke result=continued",
                ]
            )
        else:
            if mode == "budget-exhaustion-timeout":
                lines.append(
                    "M26E_GDB_SERVICE_INJECTION service=console-network "
                    f"phase=budget-exhaustion symbol={handler} "
                    "action=redirect-timeout-spin result=continued"
                )
            else:
                lines.append(
                    f"M26E_GDB_SERVICE_INJECTION service={service} "
                    f"phase=during-call symbol={handler} "
                    "action=redirect-standard-fault result=continued"
                )
        return "\n".join(lines) + "\n"

    critical_gdb_text = "\n".join(
        [
            "M26E_QEMU_SESSION target=qemu machine=virt gic_version=3 "
            f"root_image_sha256={session['root_image_sha256']} "
            f"worker_archive_sha256={session['worker_archive_sha256']} "
            f"topology_sha256={generated['topology_sha256']}",
            "M26E_GDB_ROOT_ELF "
            f"elf_sha256={hashlib.sha256(root_elf_raw).hexdigest()} "
            f"root_image_sha256={session['root_image_sha256']}",
            *(
                "M26E_GDB_CRITICAL_OBSERVATION "
                f"duty={duty} symbol={symbol} result=observed"
                for symbol, duty in evidence.QEMU_CRITICAL_DUTIES.items()
            ),
        ]
    ) + "\n"

    service_gdb_paths = []
    for service, mode in evidence.QEMU_SERVICE_EVIDENCE_PLAN:
        path = root_dir / f"service-gdb-{service}-{mode}.log"
        path.write_text(service_gdb_text(service, mode), encoding="utf-8")
        service_gdb_paths.append(path)

    ninedoor_teardown = (
        "NINEDOOR_SERVICE_TEARDOWN generation=1 tcb_suspended=yes "
        "mappings_scrubbed=yes recovery_reply_revoked=yes "
        "capabilities_revoked=yes generation_fenced=yes state=terminal"
    )
    console_teardown = (
        "CONSOLE_NETWORK_TEARDOWN generation={generation} tcb_suspended=yes "
        "scheduling_context_unbound=yes mappings_scrubbed=yes "
        "capabilities_revoked=yes objects_deleted=yes "
        "generation_fenced=yes state=terminal"
    )
    service_uart_texts = (
        "[ninedoor-service] generation=1 terminal-fault class=Standard sequence=1\n"
        f"{ninedoor_teardown}\n",
        "[ninedoor-service] generation=1 terminal-revoke state=local\n"
        f"{ninedoor_teardown}\n",
        "[console-network] generation=1 terminal-fault class=Standard sequence=1\n"
        + console_teardown.format(generation=1)
        + "\n",
        "[console-network] generation=1 terminal-fault class=Timeout sequence=1\n"
        + console_teardown.format(generation=1)
        + "\n",
    )
    service_uart_paths = []
    for index, text in enumerate(service_uart_texts, start=1):
        path = root_dir / f"service-uart-{index}.log"
        path.write_text(text, encoding="utf-8")
        service_uart_paths.append(path)
    critical_gdb_path = root_dir / "critical-gdb.log"
    critical_gdb_path.write_text(critical_gdb_text, encoding="utf-8")

    cohsh_path = root_dir / "cohsh.log"
    cohsh_path.write_text(
        "OK SPAWN role=worker-heartbeat\n"
        "ERR SPAWN role=worker-heartbeat reason=slot-busy maximum=1\n"
        "ERR SPAWN role=worker-bus reason=model-only\n"
        "OK KILL role=worker-heartbeat\n",
        encoding="utf-8",
    )

    def worker_row(role: str) -> dict[str, object]:
        generation = final_generation[role]
        role_row = roles[role]
        task = next(task for task in worker_tasks if task["id"] == f"{role}-slot-0")
        return {
            "role": role,
            "slot": 0,
            "lease_epoch": generation,
            "supervisor_generation": generation,
            "cap_generation": generation,
            "worker": {
                "worker-heartbeat": "worker-40",
                "worker-gpu": "worker-20",
                "worker-lora": "worker-30",
            }[role],
            "lifecycle": "ready",
            "artifact": "verified",
            "receipt": "none" if role == "worker-heartbeat" else "confirmed",
            "execution_proof": "qemu",
            "ready_sequence": 1,
            "control_sequence": final_sequences[role],
            "receipt_sequence": 0 if role == "worker-heartbeat" else final_sequences[role],
            "completion_sequence": final_sequences[role],
            "image_sha256": image_hashes[role],
            "core": role_row["core"],
            "scheduling_context": {
                "budget_us": task["budget_us"],
                "period_us": task["period_us"],
            },
            "object_inventory": role_row["per_slot"],
        }

    proc = {}
    for key in evidence.QEMU_PROC_KEYS:
        proc_raw = f"projection={key} state=live".encode("utf-8")
        proc[key] = {
            "lines": [proc_raw.decode("utf-8")],
            "sha256": hashlib.sha256(proc_raw).hexdigest(),
            "bytes": len(proc_raw),
        }
    workers = [worker_row(role) for role in evidence.REQUIRED_ROLES]
    session_projection = {
        key: session[key]
        for key in (
            "manifest_sha256",
            "root_image_sha256",
            "worker_archive_sha256",
            "worker_image_manifest_sha256",
            "worker_abi_sha256",
        )
    }
    required_markers = [
        "uart:WORKER_TASK_ADMISSION",
        "uart:WORKER_TASK_READY",
        "uart:WORKER_TASK_RECEIPT",
        "uart:WORKER_TASK_COMPLETION",
        "uart:WORKER_TASK_FAULT",
        "uart:WORKER_TASK_TEARDOWN",
        "gdb:M26E_GDB_ELF",
        "gdb:M26E_GDB_INJECTION",
    ]
    pressure_paths: list[Path] = []
    uart_paths: list[Path] = []
    gdb_paths: list[Path] = []
    for index, intensity in enumerate((0.7, 1.2), start=1):
        uart_path = root_dir / f"uart-{index}.log"
        gdb_path = root_dir / f"gdb-{index}.log"
        uart_path.write_text(uart_text, encoding="utf-8")
        gdb_path.write_text(gdb_text("worker-heartbeat"), encoding="utf-8")
        uart_raw = uart_path.read_bytes()
        gdb_raw = gdb_path.read_bytes()
        before = {
            "role": "worker-gpu",
            "slot": 0,
            "lease_epoch": 4,
            "supervisor_generation": 4,
            "cap_generation": 4,
        }
        after = {
            "role": "worker-gpu",
            "slot": 0,
            "lease_epoch": 5,
            "supervisor_generation": 5,
            "cap_generation": 5,
        }
        phase = {"workers": workers, "proc": proc}
        executable = {
            "topology_sha256": generated["topology_sha256"],
            "target_session": session_projection,
            "pre": phase,
            "post": phase,
            "lifecycle_cycles": [
                {
                    "role": "worker-gpu",
                    "before": before,
                    "after": after,
                    "kill_admitted": True,
                    "recreate_admitted": True,
                    "terminal_observed": True,
                    "ready_observed": True,
                }
            ],
            "receipt_operations": [
                {
                    "action": "gpu.lease.grant",
                    "role": "worker-gpu",
                    "worker_id": "worker-20",
                    "sequence_before": {"receipt": 0, "completion": 0},
                    "sequence_after": {"receipt": 1, "completion": 1},
                    "status": "succeeded",
                }
            ],
            "fault_artifacts": {
                "uart": {
                    "sha256": hashlib.sha256(uart_raw).hexdigest(),
                    "bytes": len(uart_raw),
                },
                "gdb": {
                    "sha256": hashlib.sha256(gdb_raw).hexdigest(),
                    "bytes": len(gdb_raw),
                },
            },
            "required_fault_markers": required_markers,
        }
        report = {
            "schema": "cohesix-benchmark-report/v1",
            "workload": {
                "population_mode": "executable",
                "control_write_outcome": "admitted",
                "intensity_max": intensity,
            },
            "reliability": {"error_budget_pass": True},
            "population": {
                "mode": "executable",
                "maximum_live_tasks": 3,
                "requested": 3,
                "discovered": 3,
                "ready": 3,
                "backend_class": "console-projection",
                "proof_class": "qemu",
            },
            "executable_state": executable,
        }
        summary = {
            "target_session_sha256": hashlib.sha256(session_raw).hexdigest(),
            "report": report,
        }
        pressure_path = root_dir / f"pressure-{index}.summary.json"
        _write(pressure_path, summary)
        pressure_paths.append(pressure_path)
        uart_paths.append(uart_path)
        gdb_paths.append(gdb_path)

    preflight_uart = root_dir / "preflight-uart.log"
    preflight_uart.write_text(uart_text, encoding="utf-8")
    preflight_gdb_paths = []
    for role in evidence.REQUIRED_ROLES:
        path = root_dir / f"preflight-gdb-{role}.log"
        path.write_text(gdb_text(role), encoding="utf-8")
        preflight_gdb_paths.append(path)
    run_dir = root_dir / "run"
    run_dir.mkdir()
    for stage in range(1, 6):
        (run_dir / f"stage_{stage:02d}.done").write_text("PASS\n", encoding="utf-8")
        (run_dir / f"stage_{stage:02d}.qemu.done").write_text(
            "PASS\n", encoding="utf-8"
        )
    return SimpleNamespace(
        target_session=session_path,
        generated_inventory=generated_path,
        preflight_uart=preflight_uart,
        preflight_gdb_log=preflight_gdb_paths,
        preflight_service_gdb_log=service_gdb_paths,
        preflight_service_uart=service_uart_paths,
        preflight_critical_gdb_log=critical_gdb_path,
        uart=uart_paths,
        cohsh=cohsh_path,
        gdb_log=gdb_paths,
        pressure=pressure_paths,
        worker_elf=[f"{role}={elf_paths[role]}" for role in evidence.REQUIRED_ROLES],
        service_elf=[
            f"{service}={service_paths[service]}"
            for service in evidence.QEMU_SERVICE_SYMBOLS
        ],
        root_elf=root_elf_path,
        qemu_out=qemu_out,
        auth_observation=auth_observation_path,
        worker_archive=archive_path,
        driver_archive=driver_archive_path,
        worker_image_manifest=manifest_path,
        integration_dir=integration_dir,
        run_dir=run_dir,
        out_dir=root_dir / "collected",
    )


def _qemu_target_session_inputs(root_dir: Path) -> SimpleNamespace:
    repo = root_dir / "repo"
    repo.mkdir()
    (repo / ".gitignore").write_text("/out/\n", encoding="utf-8")
    abi_manifest = repo / "crates/worker-task-abi/Cargo.toml"
    abi_source = repo / "crates/worker-task-abi/src/lib.rs"
    abi_manifest.parent.mkdir(parents=True)
    abi_source.parent.mkdir(parents=True)
    abi_manifest.write_text("[package]\nname = \"worker-task-abi\"\n", encoding="utf-8")
    abi_source.write_text(
        "#![no_std]\npub const WORKER_TASK_ABI_VERSION: u16 = 1;\n",
        encoding="utf-8",
    )

    manifest_path = repo / "configs/generated/root_task_resolved.json"
    topology_path = repo / "configs/generated/root_task_topology.json"
    manifest = {
        "profile": {"name": "virt-aarch64"},
        "worker_runtime": {"task_abi": {"enabled": True, "version": 1}},
    }
    manifest_raw = _write(manifest_path, manifest)
    topology = _generated_record("qemu")
    topology["manifest_sha256"] = hashlib.sha256(manifest_raw).hexdigest()
    topology["topology"]["worker_runtime"]["task_abi"] = {
        "enabled": True,
        "version": 1,
    }
    topology["topology_sha256"] = evidence._canonical_json_sha256(  # noqa: SLF001
        topology["topology"]
    )
    _write(topology_path, topology)

    subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
    subprocess.run(["git", "add", "."], cwd=repo, check=True)

    qemu_out = repo / "out/cohesix"
    sel4_build = repo / "out/sel4-build"
    sel4_build.mkdir(parents=True)
    launch_rows = []
    for index, (identifier, relative) in enumerate(
        evidence.QEMU_LAUNCH_ARTIFACTS, start=1
    ):
        raw = f"launch-{identifier}-{index}\n".encode("utf-8")
        path = qemu_out / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        launch_rows.append(
            {
                "id": identifier,
                "path": relative.as_posix(),
                "bytes": len(raw),
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )
    launch_record = {
        "schema": evidence.QEMU_LAUNCH_SCHEMA,
        "profile": "release",
        "cargo_target": "aarch64-unknown-none",
        "root_task_features": "release-qemu,bootstrap-trace",
        "sel4_build_dir": str(sel4_build.resolve()),
        "gic_version": "3",
        "artifacts": launch_rows,
    }
    _write(qemu_out / "cohesix-qemu-launch-artifacts.json", launch_record)

    worker_members = []
    worker_rows = []
    for index, (name, role) in enumerate(
        (
            ("worker-heart", "worker-heartbeat"),
            ("worker-gpu", "worker-gpu"),
            ("worker-lora", "worker-lora"),
        ),
        start=1,
    ):
        raw = f"canonical-{role}-{index}\n".encode("utf-8")
        archive_path = f"cohesix/worker/{name}"
        worker_members.append((archive_path, raw))
        canonical = qemu_out / "worker-images/canonical" / name
        canonical.parent.mkdir(parents=True, exist_ok=True)
        canonical.write_bytes(raw)
        worker_rows.append(
            {
                "name": name,
                "role": role,
                "abi_version": 1,
                "entry_version": 1,
                "entry_symbol": "_start",
                "entry_vaddr": 0x210000,
                "flags": ["pointer-free", "init-page-in-x0"],
                "archive_path": archive_path,
                "source_sha256": _hash(f"source-{role}"),
                "image_sha256": hashlib.sha256(raw).hexdigest(),
                "image_bytes": len(raw),
                "load_base_vaddr": 0x200000,
                "load_limit_vaddr": 0x220000,
                "load_span_bytes": 0x20000,
                "metadata_vaddr": 0x200100,
                "metadata_sha256": _hash(f"metadata-{role}"),
                "stack_bytes": 16_384,
                "ipc_buffer_bytes": 1_024,
                "shared_page_bytes": 4_096,
            }
        )
    worker_archive = evidence.worker_images.build_newc(tuple(sorted(worker_members)))
    worker_archive_path = (
        qemu_out / "worker-images/cohesix-worker-images.cpio"
    )
    worker_archive_path.parent.mkdir(parents=True, exist_ok=True)
    worker_archive_path.write_bytes(worker_archive)
    worker_manifest_path = (
        qemu_out / "worker-images/cohesix-worker-image-manifest.json"
    )
    _write(
        worker_manifest_path,
        {
            "schema": "cohesix-worker-image-manifest/v1",
            "target": "aarch64-unknown-none",
            "profile": "release",
            "archive": {
                "bytes": len(worker_archive),
                "sha256": hashlib.sha256(worker_archive).hexdigest(),
            },
            "images": worker_rows,
        },
    )

    driver_members = []
    driver_rows = []
    for index, (name, hot_path) in enumerate(
        evidence.driver_runtimes.COMPONENTS, start=1
    ):
        raw = f"driver-{name}-{index}\n".encode("utf-8")
        archive_path = f"cohesix/bin/{name}"
        driver_members.append((archive_path, raw))
        driver_rows.append(
            {
                "name": name,
                "hot_path": hot_path,
                "archive_path": archive_path,
                "bytes": len(raw),
                "sha256": hashlib.sha256(raw).hexdigest(),
            }
        )
    driver_archive = evidence.driver_runtimes.build_newc(
        tuple(sorted(driver_members))
    )
    driver_archive_path = (
        qemu_out / "driver-runtimes/cohesix-driver-runtimes.cpio"
    )
    driver_archive_path.parent.mkdir(parents=True, exist_ok=True)
    driver_archive_path.write_bytes(driver_archive)
    driver_manifest_path = (
        qemu_out / "driver-runtimes/cohesix-driver-runtime-manifest.json"
    )
    _write(
        driver_manifest_path,
        {
            "schema": evidence.driver_runtimes.SCHEMA,
            "target": evidence.driver_runtimes.EXPECTED_TARGET,
            "profile": "release",
            "scheduler": "mcs-active-sc",
            "runtime_init_abi_version": (
                evidence.driver_runtimes.RUNTIME_INIT_ABI_VERSION
            ),
            "archive": {
                "name": evidence.driver_runtimes.ARCHIVE_NAME,
                "bytes": len(driver_archive),
                "sha256": hashlib.sha256(driver_archive).hexdigest(),
            },
            "classic_comparator": {
                "provenance": "retired-26d-classic-driver-archive",
                "sha256": _hash("classic-driver"),
                "record_sha256": _hash("classic-record"),
            },
            "components": driver_rows,
        },
    )
    run_dir = repo / "out/run"
    run_dir.mkdir()
    return SimpleNamespace(
        repo_root=repo,
        qemu_out=qemu_out,
        resolved_manifest=manifest_path,
        topology=topology_path,
        out_dir=run_dir / "session",
        worker_archive=worker_archive_path,
        driver_archive=driver_archive_path,
        launch_root=qemu_out / "staging/rootserver",
    )


def _stub_session_manifest_validators(monkeypatch: pytest.MonkeyPatch) -> None:
    def verify_worker(manifest_path: Path, archive_path: Path) -> dict[str, object]:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        archive = archive_path.read_bytes()
        if manifest["archive"] != {
            "bytes": len(archive),
            "sha256": hashlib.sha256(archive).hexdigest(),
        }:
            raise evidence.worker_images.WorkerImageError(
                "Worker archive identity does not match"
            )
        return manifest

    def verify_driver(manifest_path: Path, archive_path: Path) -> dict[str, object]:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        archive = archive_path.read_bytes()
        if manifest["archive"] != {
            "name": evidence.driver_runtimes.ARCHIVE_NAME,
            "bytes": len(archive),
            "sha256": hashlib.sha256(archive).hexdigest(),
        }:
            raise evidence.driver_runtimes.DriverRuntimeManifestError(
                "driver archive identity does not match"
            )
        return manifest

    monkeypatch.setattr(evidence.worker_images, "verify_manifest", verify_worker)
    monkeypatch.setattr(evidence.driver_runtimes, "verify_manifest", verify_driver)


def test_qemu_target_session_emitter_derives_exact_frozen_graph(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _stub_session_manifest_validators(monkeypatch)
    inputs = _qemu_target_session_inputs(tmp_path)
    evidence._emit_qemu_target_session(inputs)  # noqa: SLF001

    assert {path.name for path in inputs.out_dir.iterdir()} == {
        "source-inventory.json",
        "worker-abi-identity.json",
        "qemu-cyw43-coexistence.json",
        "target-session.json",
    }
    session = json.loads((inputs.out_dir / "target-session.json").read_text())
    evidence._target_session(session, "qemu")  # noqa: SLF001
    assert session["root_image_sha256"] == hashlib.sha256(
        inputs.launch_root.read_bytes()
    ).hexdigest()
    assert session["worker_archive_sha256"] == hashlib.sha256(
        inputs.worker_archive.read_bytes()
    ).hexdigest()
    assert session["driver_archive_sha256"] == hashlib.sha256(
        inputs.driver_archive.read_bytes()
    ).hexdigest()
    for name, field in (
        ("source-inventory.json", "source_sha256"),
        ("worker-abi-identity.json", "worker_abi_sha256"),
        ("qemu-cyw43-coexistence.json", "cyw43_coexistence_record_sha256"),
    ):
        assert hashlib.sha256((inputs.out_dir / name).read_bytes()).hexdigest() == session[
            field
        ]
    with pytest.raises(evidence.EvidenceError, match="must not already exist"):
        evidence._emit_qemu_target_session(inputs)  # noqa: SLF001


def test_qemu_target_session_emitter_rejects_launch_byte_tamper(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _stub_session_manifest_validators(monkeypatch)
    inputs = _qemu_target_session_inputs(tmp_path)
    inputs.launch_root.write_bytes(b"tampered-root\n")
    with pytest.raises(evidence.EvidenceError, match="launch artifact bytes differ"):
        evidence._emit_qemu_target_session(inputs)  # noqa: SLF001
    assert not inputs.out_dir.exists()


def test_qemu_target_session_emitter_rejects_manifest_topology_drift(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _stub_session_manifest_validators(monkeypatch)
    inputs = _qemu_target_session_inputs(tmp_path)
    topology = json.loads(inputs.topology.read_text())
    topology["manifest_sha256"] = "0" * 64
    _write(inputs.topology, topology)
    with pytest.raises(evidence.EvidenceError, match="selected target manifest"):
        evidence._emit_qemu_target_session(inputs)  # noqa: SLF001
    assert not inputs.out_dir.exists()


def test_qemu_target_session_emitter_rejects_worker_archive_tamper(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _stub_session_manifest_validators(monkeypatch)
    inputs = _qemu_target_session_inputs(tmp_path)
    inputs.worker_archive.write_bytes(inputs.worker_archive.read_bytes() + b"tamper")
    with pytest.raises(evidence.EvidenceError, match="canonical Worker/driver"):
        evidence._emit_qemu_target_session(inputs)  # noqa: SLF001
    assert not inputs.out_dir.exists()


def test_component_validator_accepts_exact_three_role_graph() -> None:
    evidence.validate_component(_component("qemu"), "qemu")


def test_live_qemu_preflight_and_final_collection_are_semantically_derived(
    tmp_path: Path,
) -> None:
    inputs = _live_qemu_inputs(tmp_path)
    preflight_out = tmp_path / "preflight-component"
    evidence._collect_qemu_preflight(  # noqa: SLF001 - production collector contract
        SimpleNamespace(
            target_session=inputs.target_session,
            generated_inventory=inputs.generated_inventory,
            qemu_out=inputs.qemu_out,
            auth_observation=inputs.auth_observation,
            uart=inputs.preflight_uart,
            cohsh=inputs.cohsh,
            gdb_log=inputs.preflight_gdb_log,
            service_gdb_log=inputs.preflight_service_gdb_log,
            service_uart=inputs.preflight_service_uart,
            critical_gdb_log=inputs.preflight_critical_gdb_log,
            worker_archive=inputs.worker_archive,
            driver_archive=inputs.driver_archive,
            worker_image_manifest=inputs.worker_image_manifest,
            worker_elf=inputs.worker_elf,
            service_elf=inputs.service_elf,
            root_elf=inputs.root_elf,
            integration_dir=inputs.integration_dir,
            out_dir=preflight_out,
        )
    )
    preflight, _ = evidence._load(  # noqa: SLF001 - focused record validation
        preflight_out / "worker-task-evidence.json"
    )
    evidence.validate_component(preflight, "qemu")

    evidence._collect_qemu(inputs)  # noqa: SLF001 - production collector contract
    worker, worker_raw = evidence._load(  # noqa: SLF001
        inputs.out_dir / "worker-task-evidence.json"
    )
    root, root_raw = evidence._load(  # noqa: SLF001
        inputs.out_dir / "root-tcb-acceptance.json"
    )
    system, _ = evidence._load(  # noqa: SLF001
        inputs.out_dir / "system-acceptance.json"
    )
    evidence.validate_component(worker, "qemu")
    evidence.validate_root(root, "qemu", worker_raw, worker)
    evidence.validate_system(system, "qemu", worker, worker_raw, root, root_raw)
    assert [row["identity"]["role"] for row in worker["workers"]] == list(
        evidence.REQUIRED_ROLES
    )
    assert all(row["state"]["execution_proof"] == "qemu" for row in worker["workers"])


def test_live_qemu_collection_rejects_tamper_missing_marker_and_target_mismatch(
    tmp_path: Path,
) -> None:
    tampered = _live_qemu_inputs(tmp_path / "tampered")
    with tampered.uart[0].open("ab") as handle:
        handle.write(b"\n")
    with pytest.raises(evidence.EvidenceError, match="fault artifacts"):
        evidence._collect_qemu(tampered)  # noqa: SLF001
    assert not tampered.out_dir.exists()

    missing = _live_qemu_inputs(tmp_path / "missing")
    missing_gdb = missing.preflight_gdb_log[0]
    text = missing_gdb.read_text(encoding="utf-8")
    missing_gdb.write_text(
        "\n".join(
            line for line in text.splitlines() if "phase=budget-exhaustion" not in line
        )
        + "\n",
        encoding="utf-8",
    )
    with pytest.raises(evidence.EvidenceError, match="three-phase injection"):
        evidence._collect_qemu(missing)  # noqa: SLF001
    assert not missing.out_dir.exists()

    driver_mismatch = _live_qemu_inputs(tmp_path / "driver-mismatch")
    driver_mismatch.driver_archive.write_bytes(b"different-driver-archive")
    with pytest.raises(evidence.EvidenceError, match="driver archive"):
        evidence._collect_qemu(driver_mismatch)  # noqa: SLF001
    assert not driver_mismatch.out_dir.exists()

    mismatch = _live_qemu_inputs(tmp_path / "mismatch")
    session = json.loads(mismatch.target_session.read_text(encoding="utf-8"))
    session["target"] = "pi4"
    _write(mismatch.target_session, session)
    with pytest.raises(evidence.EvidenceError, match="wrong target"):
        evidence._collect_qemu(mismatch)  # noqa: SLF001
    assert not mismatch.out_dir.exists()


def test_qemu_gdb_runner_binds_symbols_images_and_three_injections(
    tmp_path: Path,
) -> None:
    inputs = _live_qemu_inputs(tmp_path)
    fake_gdb = tmp_path / "fake-gdb"
    fake_gdb.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' "
        "'M26E_GDB_INJECTION role=worker-heartbeat phase=pre-ready symbol=_start action=zero-x0 result=continued' "
        "'M26E_GDB_INJECTION role=worker-heartbeat phase=during-ipc symbol=cohesix_worker_qemu_evidence_control_handler action=redirect-standard-fault result=continued' "
        "'M26E_GDB_INJECTION role=worker-heartbeat phase=budget-exhaustion symbol=cohesix_worker_qemu_evidence_control_handler action=redirect-timeout-spin result=continued'\n",
        encoding="utf-8",
    )
    fake_gdb.chmod(0o755)
    fake_nm = tmp_path / "fake-nm"
    fake_nm.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' "
        "'0000000000210000 T _start' "
        "'0000000000210100 T cohesix_worker_qemu_evidence_control_handler' "
        "'0000000000210200 T cohesix_worker_qemu_evidence_standard_fault' "
        "'0000000000210300 T cohesix_worker_qemu_evidence_timeout_spin'\n",
        encoding="utf-8",
    )
    fake_nm.chmod(0o755)
    output = tmp_path / "runner-gdb.log"
    evidence._qemu_gdb(  # noqa: SLF001 - production runner contract
        SimpleNamespace(
            gdb=fake_gdb,
            nm=fake_nm,
            remote="127.0.0.1:1234",
            target_session=inputs.target_session,
            generated_inventory=inputs.generated_inventory,
            worker_image_manifest=inputs.worker_image_manifest,
            worker_elf=inputs.worker_elf,
            inject_role="worker-heartbeat",
            timeout_secs=5,
            out=output,
        )
    )
    transcript = output.read_text(encoding="utf-8")
    assert transcript.count("M26E_QEMU_SESSION") == 1
    assert transcript.count("M26E_GDB_ELF") == 3
    assert transcript.count("M26E_GDB_INJECTION") == 3
    assert "gic_version=3" in transcript


def test_qemu_service_and_critical_gdb_runners_bind_exact_elfs(
    tmp_path: Path,
) -> None:
    inputs = _live_qemu_inputs(tmp_path)
    service_paths = dict(value.split("=", maxsplit=1) for value in inputs.service_elf)
    fake_gdb = tmp_path / "fake-root-gdb"
    fake_nm = tmp_path / "fake-root-nm"
    symbol_lines = []
    address = 0x220000
    for symbols in evidence.QEMU_SERVICE_SYMBOLS.values():
        for symbol in symbols:
            symbol_lines.append(f"{address:016x} T {symbol}")
            address += 0x100
    for symbol in evidence.QEMU_NINEDOOR_ROOT_SYMBOLS:
        symbol_lines.append(
            f"{address:016x} T {evidence.QEMU_NINEDOOR_ROOT_MODULE}::{symbol}"
        )
        address += 0x100
    for symbol in evidence.QEMU_CRITICAL_SYMBOLS:
        symbol_lines.append(f"{address:016x} T {symbol}")
        address += 0x100
    fake_nm.write_text(
        "#!/bin/sh\nprintf '%s\\n' "
        + " ".join(repr(line) for line in symbol_lines)
        + "\n",
        encoding="utf-8",
    )
    fake_nm.chmod(0o755)

    for service, mode in evidence.QEMU_SERVICE_EVIDENCE_PLAN:
        handler = evidence.QEMU_SERVICE_SYMBOLS[service][0]
        if mode == "between-calls-revoke":
            rows = [
                "M26E_GDB_SERVICE_INJECTION service=ninedoor-service "
                "phase=between-calls "
                f"symbol={evidence.QEMU_NINEDOOR_ROOT_SYMBOLS[0]} "
                "action=redirect-local-revoke result=continued"
            ]
        else:
            rows = [
                "M26E_GDB_SERVICE_INJECTION "
                f"service={service} phase=during-call symbol={handler} "
                "action=redirect-standard-fault result=continued"
            ]
        if mode == "budget-exhaustion-timeout":
            rows = [
                "M26E_GDB_SERVICE_INJECTION service=console-network "
                f"phase=budget-exhaustion symbol={handler} "
                "action=redirect-timeout-spin result=continued"
            ]
        fake_gdb.write_text(
            "#!/bin/sh\nprintf '%s\\n' "
            + " ".join(repr(row) for row in rows)
            + "\n",
            encoding="utf-8",
        )
        fake_gdb.chmod(0o755)
        output = tmp_path / f"runner-{service}-{mode}.log"
        evidence._qemu_service_gdb(  # noqa: SLF001 - production runner contract
            SimpleNamespace(
                gdb=fake_gdb,
                nm=fake_nm,
                remote="127.0.0.1:1234",
                target_session=inputs.target_session,
                generated_inventory=inputs.generated_inventory,
                qemu_out=inputs.qemu_out,
                auth_observation=inputs.auth_observation,
                service=service,
                mode=mode,
                service_elf=Path(service_paths[service]),
                root_elf=(
                    inputs.root_elf if mode == "between-calls-revoke" else None
                ),
                timeout_secs=5,
                out=output,
            )
        )
        transcript = output.read_text(encoding="utf-8")
        assert f"M26E_GDB_SERVICE_ELF service={service} mode={mode}" in transcript
        assert "M26E_QEMU_AUTH result=PASS" in transcript
        assert ("M26E_GDB_SERVICE_ROOT_ELF" in transcript) == (
            mode == "between-calls-revoke"
        )

    critical_rows = [
        "M26E_GDB_CRITICAL_OBSERVATION "
        f"duty={duty} symbol={symbol} result=observed"
        for symbol, duty in evidence.QEMU_CRITICAL_DUTIES.items()
    ]
    fake_gdb.write_text(
        "#!/bin/sh\nprintf '%s\\n' "
        + " ".join(repr(row) for row in critical_rows)
        + "\n",
        encoding="utf-8",
    )
    fake_gdb.chmod(0o755)
    critical_output = tmp_path / "runner-critical.log"
    evidence._qemu_critical_gdb(  # noqa: SLF001 - production runner contract
        SimpleNamespace(
            gdb=fake_gdb,
            nm=fake_nm,
            remote="127.0.0.1:1234",
            target_session=inputs.target_session,
            generated_inventory=inputs.generated_inventory,
            root_elf=inputs.root_elf,
            timeout_secs=5,
            out=critical_output,
        )
    )
    assert critical_output.read_text(encoding="utf-8").count(
        "M26E_GDB_CRITICAL_OBSERVATION"
    ) == 4


def test_qemu_service_evidence_rejects_auth_bypass_and_copied_session(
    tmp_path: Path,
) -> None:
    inputs = _live_qemu_inputs(tmp_path)
    session_raw = inputs.target_session.read_bytes()
    session = json.loads(session_raw)

    observation = json.loads(inputs.auth_observation.read_text(encoding="utf-8"))
    uart_path = Path(observation["serial_log"]["path"])
    uart_path.write_text("Cohesix console ready\n", encoding="utf-8")
    uart_raw = uart_path.read_bytes()
    observation["serial_log"]["size_bytes"] = len(uart_raw)
    observation["serial_log"]["sha256"] = hashlib.sha256(uart_raw).hexdigest()
    _write(inputs.auth_observation, observation)
    with pytest.raises(evidence.EvidenceError, match="live authenticated cohsh"):
        evidence._validate_authenticated_qemu_observation(  # noqa: SLF001
            inputs.auth_observation,
            inputs.qemu_out,
            inputs.target_session,
            session,
            session_raw,
        )

    copied_dir = tmp_path / "copied-session"
    copied_dir.mkdir()
    copied_session = copied_dir / "target-session.json"
    copied_session.write_bytes(session_raw)
    with pytest.raises(evidence.EvidenceError, match="source-inventory"):
        evidence._validate_emitted_target_session_bundle(  # noqa: SLF001
            copied_session,
            session,
            session_raw,
        )


def test_component_emitter_binds_explicit_session_integrations_and_observations(
    tmp_path: Path,
) -> None:
    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path, "qemu"
    )
    output = tmp_path / "worker-task-evidence.json"
    evidence._emit_component(  # noqa: SLF001 - focused emitter contract test
        SimpleNamespace(
            target="qemu",
            target_session=session,
            generated_inventory=generated,
            observations=observations,
            integration_dir=integration_dir,
            out=output,
        )
    )

    record = json.loads(output.read_text(encoding="utf-8"))
    evidence.validate_component(record, "qemu")
    artifact_ids = {item["id"] for item in record["raw_evidence"]}
    assert {
        "component-observations-input",
        "generated-inventory-input",
        "target-session-input",
    } <= artifact_ids
    for reference in record["integration_evidence"]:
        copied = output.parent / "integration" / f"{reference['id']}.json"
        assert hashlib.sha256(copied.read_bytes()).hexdigest() == reference["sha256"]


def test_component_emitter_rejects_tampered_session_or_nonlive_row(
    tmp_path: Path,
) -> None:
    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path, "qemu"
    )
    session_record = json.loads(session.read_text(encoding="utf-8"))
    session_record["kernel_sha256"] = _hash("different-kernel")
    _write(session, session_record)
    with pytest.raises(evidence.EvidenceError, match="exact target session"):
        evidence._emit_component(  # noqa: SLF001 - focused tamper test
            SimpleNamespace(
                target="qemu",
                target_session=session,
                generated_inventory=generated,
                observations=observations,
                integration_dir=integration_dir,
                out=tmp_path / "must-not-exist.json",
            )
        )
    assert not (tmp_path / "must-not-exist.json").exists()

    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path / "mode", "qemu"
    )
    row_path = integration_dir / "worker-control.json"
    row = json.loads(row_path.read_text(encoding="utf-8"))
    row["observed_mode"] = "fixture"
    _write(row_path, row)
    with pytest.raises(evidence.EvidenceError, match="must be live"):
        evidence._emit_component(  # noqa: SLF001 - focused mode test
            SimpleNamespace(
                target="qemu",
                target_session=session,
                generated_inventory=generated,
                observations=observations,
                integration_dir=integration_dir,
                out=tmp_path / "mode-must-not-exist.json",
            )
        )


def test_component_emitter_rejects_generated_and_observed_topology_drift(
    tmp_path: Path,
) -> None:
    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path / "generated", "qemu"
    )
    generated_record = json.loads(generated.read_text(encoding="utf-8"))
    generated_record["topology"]["temporal_authority"]["tasks"][0]["core"] = 0
    _write(generated, generated_record)
    generated_output = tmp_path / "generated-drift-must-not-exist.json"
    with pytest.raises(evidence.EvidenceError, match="topology digest mismatch"):
        evidence._emit_component(  # noqa: SLF001 - focused tamper test
            SimpleNamespace(
                target="qemu",
                target_session=session,
                generated_inventory=generated,
                observations=observations,
                integration_dir=integration_dir,
                out=generated_output,
            )
        )
    assert not generated_output.exists()

    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path / "observed", "qemu"
    )
    observed_record = json.loads(observations.read_text(encoding="utf-8"))
    observed_record["workers"][0]["object_inventory"]["frames"] += 1
    _write(observations, observed_record)
    observed_output = tmp_path / "observed-drift-must-not-exist.json"
    with pytest.raises(evidence.EvidenceError, match="differs from generated topology"):
        evidence._emit_component(  # noqa: SLF001 - focused tamper test
            SimpleNamespace(
                target="qemu",
                target_session=session,
                generated_inventory=generated,
                observations=observations,
                integration_dir=integration_dir,
                out=observed_output,
            )
        )
    assert not observed_output.exists()


def test_root_emitter_binds_component_topology_and_exact_inventories(
    tmp_path: Path,
) -> None:
    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path, "qemu"
    )
    worker_path = tmp_path / "worker-task-evidence.json"
    evidence._emit_component(  # noqa: SLF001 - focused emitter contract test
        SimpleNamespace(
            target="qemu",
            target_session=session,
            generated_inventory=generated,
            observations=observations,
            integration_dir=integration_dir,
            out=worker_path,
        )
    )
    root_generated, root_observations = _root_emitter_inputs(
        tmp_path, "qemu", session
    )
    output = tmp_path / "root-tcb-acceptance.json"
    evidence._emit_root(  # noqa: SLF001 - focused emitter contract test
        SimpleNamespace(
            target="qemu",
            target_session=session,
            worker=worker_path,
            generated_inventory=root_generated,
            observations=root_observations,
            out=output,
        )
    )

    root = json.loads(output.read_text(encoding="utf-8"))
    evidence.validate_root(root, "qemu", worker_path.read_bytes())
    assert root["inventory_scope"] == "admitted-maximum"
    assert root["generated_inventory"] == root["observed_inventory"]


def test_root_emitter_rejects_inventory_tamper_without_publishing(
    tmp_path: Path,
) -> None:
    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path, "qemu"
    )
    worker_path = tmp_path / "worker-task-evidence.json"
    evidence._emit_component(  # noqa: SLF001 - focused setup
        SimpleNamespace(
            target="qemu",
            target_session=session,
            generated_inventory=generated,
            observations=observations,
            integration_dir=integration_dir,
            out=worker_path,
        )
    )
    root_generated, root_observations = _root_emitter_inputs(
        tmp_path, "qemu", session
    )
    observed = json.loads(root_observations.read_text(encoding="utf-8"))
    observed["observed_inventory"]["reply_objects"] += 1
    _write(root_observations, observed)
    output = tmp_path / "must-not-exist-root.json"
    with pytest.raises(evidence.EvidenceError, match="inventory differs"):
        evidence._emit_root(  # noqa: SLF001 - focused tamper test
            SimpleNamespace(
                target="qemu",
                target_session=session,
                worker=worker_path,
                generated_inventory=root_generated,
                observations=root_observations,
                out=output,
            )
        )
    assert not output.exists()


def test_root_emitter_rejects_constructed_actual_budget_claim(
    tmp_path: Path,
) -> None:
    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path, "qemu"
    )
    worker_path = tmp_path / "worker-task-evidence.json"
    evidence._emit_component(  # noqa: SLF001 - focused setup
        SimpleNamespace(
            target="qemu",
            target_session=session,
            generated_inventory=generated,
            observations=observations,
            integration_dir=integration_dir,
            out=worker_path,
        )
    )
    root_generated, root_observations = _root_emitter_inputs(
        tmp_path, "qemu", session
    )
    observed = json.loads(root_observations.read_text(encoding="utf-8"))
    observed["inventory_scope"] = "constructed-actual"
    _write(root_observations, observed)
    output = tmp_path / "must-not-exist-root.json"
    with pytest.raises(evidence.EvidenceError, match="scope is not admitted-maximum"):
        evidence._emit_root(  # noqa: SLF001 - focused scope test
            SimpleNamespace(
                target="qemu",
                target_session=session,
                worker=worker_path,
                generated_inventory=root_generated,
                observations=root_observations,
                out=output,
            )
        )
    assert not output.exists()


def test_root_emitter_rejects_compiler_inventory_tamper(tmp_path: Path) -> None:
    session, generated, observations, integration_dir = _component_emitter_inputs(
        tmp_path, "qemu"
    )
    worker_path = tmp_path / "worker-task-evidence.json"
    evidence._emit_component(  # noqa: SLF001 - focused setup
        SimpleNamespace(
            target="qemu",
            target_session=session,
            generated_inventory=generated,
            observations=observations,
            integration_dir=integration_dir,
            out=worker_path,
        )
    )
    root_generated, root_observations = _root_emitter_inputs(
        tmp_path, "qemu", session
    )
    generated_record = json.loads(root_generated.read_text(encoding="utf-8"))
    generated_record["inventory"]["reply_objects"] += 1
    _write(root_generated, generated_record)
    output = tmp_path / "compiler-tamper-must-not-exist.json"
    with pytest.raises(evidence.EvidenceError, match="differs from compiler topology"):
        evidence._emit_root(  # noqa: SLF001 - focused tamper test
            SimpleNamespace(
                target="qemu",
                target_session=session,
                worker=worker_path,
                generated_inventory=root_generated,
                observations=root_observations,
                out=output,
            )
        )
    assert not output.exists()


def test_root_validator_rejects_partial_containment_outcomes() -> None:
    worker = _component("qemu")
    worker_raw = (json.dumps(worker, sort_keys=True) + "\n").encode("utf-8")
    root = _root("qemu", worker_raw)
    root["outcomes"].pop()
    with pytest.raises(evidence.EvidenceError, match="exact required PASS"):
        evidence.validate_root(root, "qemu", worker_raw)


def test_component_rejects_unconfirmed_gpu_receipt_and_missing_integration() -> None:
    component = _component("qemu")
    component["workers"][1]["state"]["receipt"] = "pending"
    with pytest.raises(evidence.EvidenceError, match="role state"):
        evidence.validate_component(component, "qemu")

    component = _component("qemu")
    component["integration_evidence"].pop()
    with pytest.raises(evidence.EvidenceError, match="mandatory integration"):
        evidence.validate_component(component, "qemu")


def test_component_rejects_badge_sc_and_outcome_inventory_tamper() -> None:
    component = _component("qemu")
    component["workers"][1]["endpoint_badge"] = component["workers"][0][
        "fault_badge"
    ]
    with pytest.raises(evidence.EvidenceError, match="badge inventories overlap"):
        evidence.validate_component(component, "qemu")

    component = _component("qemu")
    component["workers"][0]["scheduling_context"]["budget_us"] = 10_001
    with pytest.raises(evidence.EvidenceError, match="scheduling_context"):
        evidence.validate_component(component, "qemu")

    component = _component("qemu")
    component["outcomes"].pop()
    with pytest.raises(evidence.EvidenceError, match="exact required PASS"):
        evidence.validate_component(component, "qemu")


def test_integration_rejects_secret_and_wrong_target_mode() -> None:
    record = _integration("qemu", "worker-control")
    record["outcomes"][0]["result"] = "Bearer hidden"
    with pytest.raises(evidence.EvidenceError, match="prohibited"):
        evidence.validate_integration(record, "qemu")

    record = _integration("qemu", "worker-control")
    record["observed_mode"] = "fixture"
    with pytest.raises(evidence.EvidenceError, match="must be live"):
        evidence.validate_integration(record, "qemu")


def test_integration_outcome_class_is_dependency_exact() -> None:
    projection = _integration("qemu", "python-sdk-projection")
    projection["obligation"] = "release_required"
    projection["outcomes"][0]["class"] = "projection-compatibility"
    evidence.validate_integration(projection, "qemu")

    mandatory = _integration("qemu", "worker-control")
    mandatory["outcomes"][0]["class"] = "projection-compatibility"
    with pytest.raises(evidence.EvidenceError, match="invalid integration outcomes class"):
        evidence.validate_integration(mandatory, "qemu")


def test_validate_system_requires_complete_target_markers(tmp_path: Path) -> None:
    worker = _component("qemu")
    worker_raw = _write(tmp_path / "worker.json", worker)
    root = _root("qemu", worker_raw)
    root_raw = _write(tmp_path / "root.json", root)
    run_dir = tmp_path / "run"
    run_dir.mkdir()

    with pytest.raises(evidence.EvidenceError, match="stage_01.done"):
        evidence._system_from_run(  # noqa: SLF001 - focused CLI contract test
            "qemu", worker, worker_raw, root, root_raw, run_dir
        )


def test_validate_system_emits_hash_bound_record(tmp_path: Path) -> None:
    worker = _component("qemu")
    worker_raw = _write(tmp_path / "worker.json", worker)
    root = _root("qemu", worker_raw)
    root_raw = _write(tmp_path / "root.json", root)
    run_dir = tmp_path / "run"
    run_dir.mkdir()
    for stage in range(1, 6):
        (run_dir / f"stage_{stage:02d}.done").write_text("PASS\n", encoding="utf-8")
        (run_dir / f"stage_{stage:02d}.qemu.done").write_text("PASS\n", encoding="utf-8")
    system = _system("qemu", worker, worker_raw, root, root_raw)
    run_input = {
        "schema": evidence.SYSTEM_INPUT_SCHEMA,
        "target": "qemu",
        "target_session": system["target_session"],
        "worker_component_sha256": hashlib.sha256(worker_raw).hexdigest(),
        "root_tcb_sha256": hashlib.sha256(root_raw).hexdigest(),
        "topology_sha256": system["topology_sha256"],
        "core_admission": system["core_admission"],
        "outcomes": system["outcomes"],
        "raw_evidence": system["raw_evidence"],
        "verdict": "PASS",
        "blockers": [],
    }
    _write(run_dir / "m26e-system-input.json", run_input)

    generated = evidence._system_from_run(  # noqa: SLF001 - focused CLI contract test
        "qemu", worker, worker_raw, root, root_raw, run_dir
    )
    evidence.validate_system(generated, "qemu", worker, worker_raw, root, root_raw)

    run_input["worker_component_sha256"] = _hash("stale-worker")
    _write(run_dir / "m26e-system-input.json", run_input)
    with pytest.raises(evidence.EvidenceError, match="stale component/root"):
        evidence._system_from_run(  # noqa: SLF001 - focused tamper test
            "qemu", worker, worker_raw, root, root_raw, run_dir
        )


def test_system_pass_requires_exact_full_system_outcome_matrix() -> None:
    worker = _component("qemu")
    worker_raw = (json.dumps(worker, sort_keys=True) + "\n").encode("utf-8")
    root = _root("qemu", worker_raw)
    root_raw = (json.dumps(root, sort_keys=True) + "\n").encode("utf-8")
    system = _system("qemu", worker, worker_raw, root, root_raw)
    system["outcomes"].pop()
    with pytest.raises(evidence.EvidenceError, match="exact required PASS"):
        evidence.validate_system(
            system,
            "qemu",
            worker,
            worker_raw,
            root,
            root_raw,
        )


def test_staged_runner_refuses_m26e_pass_without_explicit_observations(
    tmp_path: Path,
) -> None:
    state_dir = tmp_path / "state"
    completed = subprocess.run(
        [
            str(ROOT / "scripts/ci/test_plan_run.sh"),
            "--target",
            "qemu",
            "--state-dir",
            str(state_dir),
            "--m26e-evidence-kind",
            "component",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    assert completed.returncode == 2
    assert "explicit --m26e-observations" in completed.stderr
    assert not (state_dir / "worker-task-evidence.json").exists()

    state_dir.mkdir()
    (state_dir / "worker-task-evidence.json").write_text("stale\n", encoding="utf-8")
    stale = subprocess.run(
        [
            str(ROOT / "scripts/ci/test_plan_run.sh"),
            "--target",
            "qemu",
            "--state-dir",
            str(state_dir),
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    assert stale.returncode == 2
    assert "existing M26e acceptance output requires explicit" in stale.stderr


def test_release_promotion_requires_and_binds_both_target_graphs(tmp_path: Path) -> None:
    qemu = _target_graph(tmp_path, "qemu")
    pi4 = _target_graph(tmp_path, "pi4")
    output = tmp_path / "worker-release-acceptance.json"
    args = SimpleNamespace(
        worker_qemu=qemu[0],
        root_qemu=qemu[1],
        system_qemu=qemu[2],
        worker_pi4=pi4[0],
        root_pi4=pi4[1],
        system_pi4=pi4[2],
        out=output,
    )

    evidence._promote(args)  # noqa: SLF001 - focused promotion contract test

    release = json.loads(output.read_text(encoding="utf-8"))
    assert release["schema"] == evidence.RELEASE_SCHEMA
    assert len(release["acceptance_records"]) == 6
    assert len(release["integration_evidence"]) == 6
    assert release["verdict"] == "PASS"

    pi4_system = json.loads(pi4[2].read_text(encoding="utf-8"))
    pi4_system["target_session"] = _session("qemu")
    _write(pi4[2], pi4_system)
    with pytest.raises(evidence.EvidenceError, match="target_session"):
        evidence._promote(args)  # noqa: SLF001 - focused tamper test
