# Author: Lukas Bower
# Purpose: Guard M28g protocol-mode evidence, shared job identity and fail-closed latency.
# Copyright 2026 Lukas Bower
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/ci"))
import provider_m28g_integration as integration  # noqa: E402

SOURCE = "a" * 40
ADMISSION = "m28g-shared-job-01"
RESULT = "b" * 64
TICKET_BINDING = {"admission_id": ADMISSION, "ticket_id": "ticket-01",
                  "idempotency_key": "idem-01"}
CURRENT = b"HOST_TICKET_CURRENT schema=host-ticket-current/v1 state=confirmed role=gpu"


def evidence_file(tmp_path: Path, name: str) -> dict[str, object]:
    path = tmp_path / name
    raw = f"raw-{name}".encode()
    path.write_bytes(raw)
    return {"path": str(path), "sha256": hashlib.sha256(raw).hexdigest(),
            "size": len(raw)}


def latency_samples(tmp_path: Path) -> dict[str, object]:
    """Retain call-level windows with the same identity as the report fixture."""
    path = tmp_path / "latency.csv"
    lines = ["mode,protocol,sample,elapsed_ms,status"]
    for mode, (_, mcp, a2a) in integration.MODES.items():
        protocols = {"rest": 80, "mcp": 110 if mcp else None,
                     "a2a": 120 if a2a else None}
        for protocol, elapsed in protocols.items():
            if elapsed is not None:
                lines.extend(f"{mode},{protocol},{index},{elapsed},ok"
                             for index in range(1, 31))
    raw = ("\n".join(lines) + "\n").encode()
    path.write_bytes(raw)
    return {"path": str(path), "sha256": hashlib.sha256(raw).hexdigest(),
            "size": len(raw)}


def external(tmp_path: Path) -> dict[str, object]:
    paths = {name: tmp_path / f"{name}.json" for name in integration.EVIDENCE}
    refusals = {"schema": "cohesix-m28g-refusals/v1",
                "source_commit": SOURCE, "admission_id": ADMISSION,
                "cross_subject_denied": True, "revoked_scope_denied": True,
                "budget_refused_no_effect": True,
                "raw_evidence": {"client": evidence_file(tmp_path, "refusals.raw")}}
    queen = {"schema": "cohesix-m28g-queen-loss/v1",
             "source_commit": SOURCE, "admission_id": ADMISSION,
             "refused_new_effect": True, "recovered_original_id": True,
             "no_vm_persistence_claim": True,
             "raw_evidence": {"serial": evidence_file(tmp_path, "queen.raw")}}
    latency = {"schema": "cohesix-m28g-protocol-latency/v1",
               "source_commit": SOURCE,
               "modes": {mode: {"samples": 30, "rest_p95_ms": 80,
                                 "mcp_p95_ms": 110 if mcp else None,
                                 "a2a_p95_ms": 120 if a2a else None}
                         for mode, (_, mcp, a2a) in integration.MODES.items()},
               "raw_evidence": {"samples": latency_samples(tmp_path),
                                "client": evidence_file(tmp_path, "client.raw")}}
    for name, data in (("refusals", refusals), ("queen_loss", queen),
                       ("latency", latency), ("native_cuda", {
                           "result": "PASS", "source_commit": SOURCE,
                           "admission_id": ADMISSION, "terminal": {"state": "succeeded"},
                       })):
        paths[name].write_text(json.dumps(data))
    for name in ("mcp", "a2a_task", "a2a_recovery"):
        paths[name].write_text("{}")
    return {"evidence": {name: str(path) for name, path in paths.items()},
            "current_path": integration.current_path_for(TICKET_BINDING)}


def sdk_proofs(monkeypatch: pytest.MonkeyPatch) -> None:
    recovered = {"record": {"binding": TICKET_BINDING, "result_sha256": RESULT},
                 "target_results": [{"state": "succeeded"}]}
    monkeypatch.setattr(integration, "mcp_result", lambda path, state: recovered)
    monkeypatch.setattr(integration, "recovery", lambda path, admission: recovered)
    monkeypatch.setattr(integration, "task", lambda path, admission: {
        "metadata": {"resultSha256": RESULT},
    })


def test_selected_queen_address_rejects_public_or_dns_destinations() -> None:
    assert integration.private_target("127.0.0.1")
    assert integration.private_target("192.168.86.139")
    assert not integration.private_target("8.8.8.8")
    assert not integration.private_target("example.com")
    assert integration.current_path_for(TICKET_BINDING).startswith(
        "/host/tickets/current/")
    changed = {**TICKET_BINDING, "idempotency_key": "idem-02"}
    assert integration.current_path_for(changed) != integration.current_path_for(TICKET_BINDING)


def test_target_binding_preserves_distinct_kvm_and_physical_pi_proof(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(integration, "qemu_image_identity",
                        lambda pid, source, artifact_root: {
                            "qemu_pid": str(pid), "rootserver_sha256": "e" * 64,
                        })
    qemu = {"target_kind": "qemu", "target_profile": "qemu-smp-production",
            "target_qemu_pid": 123, "target_proof": None,
            "target_image_sha256": "e" * 64, "source_root": str(ROOT)}
    assert integration.target_binding(qemu, SOURCE, "jetson-orin-nano-jp7")["kind"] == "qemu"
    with pytest.raises(ValueError, match="Jetson KVM"):
        integration.target_binding(qemu, SOURCE, "mac-apple-m4-macos27")
    proof = tmp_path / "pi4.json"
    proof.write_text("{}")
    monkeypatch.setattr(integration, "read_result", lambda path, kind: {
        "source_commit": SOURCE, "media_image_sha256": "f" * 64,
        "provisioning_verified": True,
    })
    pi = {**qemu, "target_kind": "pi4", "target_profile": "pi4-production",
          "target_qemu_pid": None, "target_proof": str(proof),
          "target_image_sha256": "f" * 64}
    assert integration.target_binding(pi, SOURCE, "mac-apple-m4-macos27")["kind"] == "pi4"
    pi["target_image_sha256"] = "d" * 64
    with pytest.raises(ValueError, match="SD image qualification"):
        integration.target_binding(pi, SOURCE, "mac-apple-m4-macos27")


def test_shared_result_and_frozen_latency_require_raw_evidence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    config = external(tmp_path)
    sdk_proofs(monkeypatch)
    summary = integration.validate_external(config, SOURCE, ADMISSION, RESULT)
    assert summary["native_result_sha256"] == RESULT
    assert len(summary["evidence_sha256"]) == 7
    latency_path = Path(config["evidence"]["latency"])
    latency = json.loads(latency_path.read_text())
    latency["modes"]["both"]["mcp_p95_ms"] = 181
    latency_path.write_text(json.dumps(latency))
    with pytest.raises(ValueError, match="control latency"):
        integration.validate_external(config, SOURCE, ADMISSION, RESULT)
    latency["modes"]["both"]["mcp_p95_ms"] = 110
    latency["modes"]["neither"]["a2a_p95_ms"] = 1
    latency_path.write_text(json.dumps(latency))
    with pytest.raises(ValueError, match="control latency"):
        integration.validate_external(config, SOURCE, ADMISSION, RESULT)
    latency["modes"]["neither"]["a2a_p95_ms"] = None
    latency_path.write_text(json.dumps(latency))
    Path(tmp_path / "queen.raw").write_text("changed")
    with pytest.raises(ValueError, match="observation changed"):
        integration.validate_external(config, SOURCE, ADMISSION, RESULT)


def test_latency_recomputation_rejects_gaps_duplicates_and_false_p95(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    config = external(tmp_path)
    sdk_proofs(monkeypatch)
    latency_path = Path(config["evidence"]["latency"])
    latency = json.loads(latency_path.read_text())
    sample_path = Path(latency["raw_evidence"]["samples"]["path"])
    original = sample_path.read_text()

    def replace_samples(value: str) -> None:
        raw = value.encode()
        sample_path.write_bytes(raw)
        latency["raw_evidence"]["samples"].update(
            sha256=hashlib.sha256(raw).hexdigest(), size=len(raw))
        latency_path.write_text(json.dumps(latency))

    replace_samples(original.replace("both,mcp,30,110,ok\n", ""))
    with pytest.raises(ValueError, match="incomplete latency window"):
        integration.validate_external(config, SOURCE, ADMISSION, RESULT)

    replace_samples(original.replace("both,mcp,30,110,ok\n",
                                     "both,mcp,29,110,ok\n"))
    with pytest.raises(ValueError, match="duplicate latency sample"):
        integration.validate_external(config, SOURCE, ADMISSION, RESULT)

    replace_samples(original.replace("both,mcp,29,110,ok\n",
                                     "both,mcp,29,111,ok\n")
                            .replace("both,mcp,30,110,ok\n",
                                     "both,mcp,30,111,ok\n"))
    with pytest.raises(ValueError, match="reported p95 differs"):
        integration.validate_external(config, SOURCE, ADMISSION, RESULT)


def test_four_live_probe_modes_keep_rest_and_original_result(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    received: list[dict[str, str]] = []

    class Process:
        pid = 12345

        def poll(self) -> int:
            return 0

        def wait(self, timeout: int) -> int:
            return 0

    def launch(command: list[str], **kwargs: object) -> Process:
        assert "--mock" not in command
        assert "--standing-ledger" in command
        received.append(kwargs["env"])
        return Process()

    class Backend:
        def __init__(self, *args: object, **kwargs: object) -> None:
            pass

        def read_file(self, path: str, size: int) -> bytes:
            if path == "/proc/boot":
                return b"manifest.sha256=" + b"c" * 64
            return CURRENT

    def status(port: int, path: str) -> int:
        if path == "/docs":
            return 200
        env = received[-1]
        mcp = env.get("HIVE_GATEWAY_AGENT_PROTOCOLS_ENABLED") != "false"
        a2a = mcp and env.get("HIVE_GATEWAY_A2A_ENABLED") != "false"
        mcp = mcp and env.get("HIVE_GATEWAY_MCP_ENABLED") != "false"
        return {"/mcp": 401 if mcp else 404,
                "/.well-known/agent-card.json": 401 if a2a else 404,
                "/a2a": 405 if a2a else 404}[path]

    monkeypatch.setattr(integration, "resolve_secret_reference", lambda ref: "secret")
    monkeypatch.setattr(integration.subprocess, "Popen", launch)
    monkeypatch.setattr(integration, "RestBackend", Backend)
    monkeypatch.setattr(integration, "http_code", status)
    monkeypatch.setattr(integration, "free_port", lambda: 44444)
    config = {"gateway_binary": "/installed/hive-gateway",
              "source_root": str(ROOT), "target_host": "127.0.0.1",
              "target_port": 3133, "target_profile": "qemu-smp-production",
              "delegation_key_ref": "file:/private/key",
              "standing_ledger": "/private/ledger", "standing_scopes": "/private/scopes",
              "queen_auth_ref": "file:/private/queen",
              "gateway_ticket_ref": "file:/private/gateway-ticket",
              "request_auth_ref": "file:/private/request",
              "delegated_ticket_ref": "file:/private/caller-ticket",
              "target_manifest_sha256": "c" * 64,
              "current_path": "/host/tickets/current/" + "d" * 64,
              "current_sha256": hashlib.sha256(CURRENT).hexdigest()}
    rows = integration.probe_modes(config, tmp_path)
    assert [row["mode"] for row in rows] == list(integration.MODES)
    assert len(received) == 4
    assert all(row["current_sha256"] == config["current_sha256"] for row in rows)
    assert received[-1]["HIVE_GATEWAY_AGENT_PROTOCOLS_ENABLED"] == "false"
