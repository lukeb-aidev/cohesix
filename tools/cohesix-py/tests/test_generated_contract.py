# Author: Lukas Bower
# Purpose: Validate generated target-qualified Python contracts and target-neutral defaults.
# Copyright 2026 Lukas Bower

"""Tests for the compiler-owned Cohesix Python profile contracts."""

from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

import pytest

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = PACKAGE_ROOT.parents[1]
sys.path.insert(0, str(PACKAGE_ROOT))

from cohesix.defaults import manifest_hash  # noqa: E402
from cohesix.errors import CohesixError  # noqa: E402
from cohesix.generated import (  # noqa: E402
    DEFAULTS,
    GPU_RECEIPT_ACTIONS,
    HOST_TICKET_REQUEST_SCHEMAS,
    HOST_TICKET_RESULT_SCHEMAS,
    PEFT_RECEIPT_ACTIONS,
)
from cohesix.worker import load_profile_contract  # noqa: E402

QEMU_CONTRACT = (
    REPO_ROOT / "configs/generated/cohesix_python_qemu_smp_production.json"
)
PI4_CONTRACT = REPO_ROOT / "configs/generated/cohesix_python_pi4_production.json"


def _payload(path: Path = QEMU_CONTRACT) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def test_generated_defaults_are_target_neutral() -> None:
    assert DEFAULTS["contract_kind"] == "target-neutral-fallback"
    assert DEFAULTS["manifest_sha256"] is None
    assert DEFAULTS["execution_proof"] == "none"
    assert manifest_hash() == "unknown"
    assert set(DEFAULTS).isdisjoint({"target", "target_profile"})
    assert HOST_TICKET_REQUEST_SCHEMAS == ("host-ticket/v1", "host-ticket/v2")
    assert HOST_TICKET_RESULT_SCHEMAS == (
        "host-ticket-result/v1",
        "host-ticket-result/v2",
    )
    assert GPU_RECEIPT_ACTIONS == (
        "gpu.lease.grant",
        "gpu.lease.renew",
        "gpu.lease.release",
    )
    assert PEFT_RECEIPT_ACTIONS == (
        "peft.export",
        "peft.import",
        "peft.activate",
        "peft.rollback",
    )


def test_qemu_and_pi_contracts_are_independent_exact_targets() -> None:
    qemu = load_profile_contract(QEMU_CONTRACT, expected_target="qemu")
    pi4 = load_profile_contract(PI4_CONTRACT, expected_target="pi4")

    assert qemu.establishes_target_identity
    assert pi4.establishes_target_identity
    assert qemu.target_profile == "qemu_smp_production"
    assert pi4.target_profile == "pi4_production"
    assert qemu.manifest_sha256 != pi4.manifest_sha256
    assert qemu.contract_sha256 != pi4.contract_sha256
    assert qemu.maximum_live_tasks == 256
    assert pi4.maximum_live_tasks == 64
    assert qemu.role_declaration("heartbeat") == "executable"
    assert qemu.role_declaration("gpu") == "executable"
    assert qemu.role_declaration("lora") == "executable"
    assert qemu.role_declaration("bus") == "model-only"


def test_mapping_is_validation_input_not_target_identity() -> None:
    mapped = load_profile_contract(_payload(), expected_target="qemu")
    assert not mapped.establishes_target_identity
    assert mapped.source == "mapping"


def test_canonical_shard_vector_and_legacy_gate() -> None:
    contract = load_profile_contract(QEMU_CONTRACT)
    assert contract.telemetry_path("worker-7") == (
        "/shard/22/worker/worker-7/telemetry"
    )
    assert contract.legacy_telemetry_path("worker-7") == (
        "/worker/worker-7/telemetry"
    )

    payload = _payload()
    payload["namespace"]["legacy_worker_alias"] = False  # type: ignore[index]
    without_alias = load_profile_contract(payload)
    with pytest.raises(CohesixError, match="legacy /worker alias is disabled"):
        without_alias.legacy_telemetry_path("worker-7")


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (lambda value: value.pop("receipts"), "fields differ"),
        (
            lambda value: value["worker"].update({"maximum_live_tasks": 4}),
            "maximum live task count",
        ),
        (
            lambda value: value["schemas"]["host_ticket_accepted"].pop(),
            "host-ticket compatibility matrix",
        ),
        (
            lambda value: value["receipts"]["gpu_actions"].reverse(),
            "receipt action matrix",
        ),
        (
            lambda value: value["worker"]["roles"][0].update(
                {"ticket_scope": "/gpu"}
            ),
            "Worker declaration is inconsistent",
        ),
        (
            lambda value: value["proof_boundary"].update(
                {"python_projection_is_authority": True}
            ),
            "widens Python proof authority",
        ),
        (
            lambda value: value["meta"].update({"authorization": "Bearer secret"}),
            "prohibited authority data",
        ),
        (
            lambda value: value["meta"].update({"purpose": "token=raw-secret"}),
            "prohibited authority data",
        ),
        (
            lambda value: value["bounds"].update({"secure9p_msize": 16384}),
            "Secure9P red lines",
        ),
        (
            lambda value: value["namespace"].update({"shard_bits": 4}),
            "canonical target shard width",
        ),
    ],
)
def test_contract_tampering_fails_closed(mutation, message: str) -> None:
    payload = copy.deepcopy(_payload())
    mutation(payload)
    with pytest.raises(CohesixError, match=message):
        load_profile_contract(payload)


def test_wrong_target_missing_and_symlink_contracts_fail(tmp_path: Path) -> None:
    with pytest.raises(CohesixError, match="wrong target"):
        load_profile_contract(QEMU_CONTRACT, expected_target="pi4")
    with pytest.raises(CohesixError, match="regular non-symlink"):
        load_profile_contract(tmp_path / "missing.json")

    link = tmp_path / "contract.json"
    link.symlink_to(QEMU_CONTRACT)
    with pytest.raises(CohesixError, match="regular non-symlink"):
        load_profile_contract(link)
