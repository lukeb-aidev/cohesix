# Author: Lukas Bower
# Purpose: Reject unexplained production feature, policy and capacity drift between target manifests.
# Copyright 2026 Lukas Bower

"""Guard the common production contract in docs/PRODUCTION_PROFILES.md."""

from pathlib import Path
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def _manifest(name: str) -> dict:
    with (ROOT / "configs" / name).open("rb") as stream:
        return tomllib.load(stream)


def _common_contract(manifest: dict) -> dict:
    """Remove only documented hardware, memory and temporal-topology fields."""
    manifest.pop("hw", None)
    manifest["meta"].pop("purpose")
    manifest["profile"].pop("name")
    manifest["dma"].pop("protection_profile")
    manifest["root_task"]["driver_images"].pop("irqs")
    console = manifest["console_network_service"]
    console.pop("direct_virtio", None)
    console.pop("timer_clock_hz")
    for key in ("frames", "cspace_slots"):
        console["objects"].pop(key)
    temporal = manifest["temporal_authority"]
    tasks = []
    for task in temporal["tasks"]:
        if task["kind"] == "driver":
            continue
        for key in ("core", "sched_control_core", "budget_us", "wcet_us",
                    "response_time_us", "wcet_provenance"):
            task.pop(key, None)
        if task["id"] == "root-control":
            for key in ("scheduling_context_bits", "max_refills",
                        "virtio_operator_serial_io_bytes_per_turn"):
                task.pop(key)
        tasks.append(task)
    temporal["tasks"] = tasks
    for worker in temporal["worker_classes"]:
        worker.pop("timeout_badge_base")
    admission = manifest["worker_resource_admission"]
    admission["capacity"].pop("cspace_slots")
    admission.pop("fixed_objects")
    for task in admission["critical_tcbs"]:
        if task["id"] in ("root-driver-supervisor", "root-fault"):
            task.pop("cspace_cap_count")
    for key in ("capacity", "driver_tcbs"):
        admission["fault_registry"].pop(key)
    admission["handoff"].pop("driver_fault_records")
    for key in ("driver_fault_badges", "timeout_fault_badges"):
        admission["handoff"][key].pop("count")
    return manifest


def test_common_production_contract_matches() -> None:
    qemu = _manifest("root_task.toml")
    pi = _manifest("root_task_pi4_uboot_aarch64.toml")
    assert _common_contract(qemu) == _common_contract(pi)


def test_full_supported_population_and_features_are_enabled() -> None:
    for name in ("root_task.toml", "root_task_pi4_uboot_aarch64.toml"):
        manifest = _manifest(name)
        assert manifest["worker_runtime"]["max_workers"] == 256
        assert sum(role["executable_slots"] for role in
                   manifest["worker_resource_admission"]["executable_roles"]) == 256
        assert manifest["sharding"]["shard_bits"] == 8
        assert manifest["ecosystem"]["audit"]["enable"] is True
        assert manifest["ecosystem"]["audit"]["replay_enable"] is True
        assert manifest["ecosystem"]["models"]["enable"] is True
        assert manifest["sidecars"]["modbus"]["enable"] is True
        assert manifest["sidecars"]["dnp3"]["enable"] is False


def test_paired_production_kernels_retain_release_and_memory_contracts() -> None:
    profiles = _manifest("sel4/profiles.toml")["profiles"]
    for name, cnode_bits in (("qemu_smp_production", "14"),
                             ("pi4_production", "16")):
        config = profiles[name]["cmake"]
        assert config["RELEASE"] == "ON"
        assert config["KernelDebugBuild"] == "OFF"
        assert config["KernelPrinting"] == "OFF"
        assert config["KernelIsMCS"] == "ON"
        assert config["KernelMaxNumNodes"] == "4"
        assert config["KernelRootCNodeSizeBits"] == cnode_bits


def test_regression_profile_admits_its_declared_feature_fixtures() -> None:
    """Replay, model binding, and Modbus fixtures need their manifest-owned surfaces."""
    manifest = _manifest("root_task_regression.toml")
    assert manifest["ecosystem"]["policy"]["enable"] is True
    assert manifest["ecosystem"]["audit"]["enable"] is True
    assert manifest["ecosystem"]["audit"]["replay_enable"] is True
    assert manifest["ecosystem"]["models"]["enable"] is True
    assert manifest["sidecars"]["modbus"]["enable"] is True
    assert manifest["sidecars"]["dnp3"]["enable"] is False
    assert manifest["sidecars"]["modbus"]["adapters"] == [{
        "id": "modbus-main", "mount": "modbus-main", "scope": "modbus-main",
        "link": "serial", "baud": 19200,
        "spool": {"max_entries": 8, "max_bytes": 512},
    }]
