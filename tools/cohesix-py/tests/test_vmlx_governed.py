# Author: Lukas Bower
# Purpose: Check accepted-generation refusal and exact orphan ownership before optional vMLX process recovery.
# Copyright 2026 Lukas Bower
"""Focused pure and mocked custody contracts; live evidence remains separate."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from cohesix.hf_native import encode
from cohesix import vmlx_governed


def _selection(tmp_path: Path, monkeypatch: pytest.MonkeyPatch
               ) -> vmlx_governed.GovernedSelection:
    root = tmp_path / "release"
    root.mkdir(mode=0o700)
    accepted = root / "accepted.json"
    accepted.write_bytes(encode({
        "generation": 1, "adapter_sha256": "a" * 64,
        "served_artifact_sha256": "a" * 64,
        "runtime_sha256": "b" * 64, "healthy": True,
        "rollback_verified": True}))
    stage = root / "stage"
    stage.mkdir(mode=0o700)
    monkeypatch.setattr(vmlx_governed.VmlxSelection, "validate",
                        lambda self: Path("/Applications/vMLX.app/engine"))
    return vmlx_governed.GovernedSelection(
        root, accepted, 1, "a" * 64, "a" * 64,
        root / "fused", "c" * 64, "d" * 64,
        Path("/Applications/vMLX.app"), "1.6.65", "e" * 40,
        "f" * 64, stage, 18085, ("1" * 64,), 5000, 4_294_967_296)


def test_changed_accepted_generation_refuses_selected_vmlx(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    selection = _selection(tmp_path, monkeypatch)
    selection.validate()
    accepted = json.loads(selection.accepted_path.read_bytes())
    accepted["generation"] = 2
    selection.accepted_path.write_bytes(encode(accepted))
    with pytest.raises(ValueError, match="accepted_generation_changed"):
        selection.validate()


def test_orphan_recovery_never_kills_foreign_process(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    selection = _selection(tmp_path, monkeypatch)
    selection.custody_path.write_bytes(encode({
        "schema": "cohesix-vmlx-custody/v1",
        "release_graph_sha256": selection.release_graph_sha256,
        "native": {"engine_sha256": selection.engine_sha256,
                   "source_sha256": selection.source_sha256,
                   "generation": selection.generation,
                   "model_id": selection.model_id, "pid": 1234},
    }))
    monkeypatch.setattr(vmlx_governed, "_pid_command",
                        lambda _pid: "/bin/sleep 30")
    killed: list[tuple[int, object]] = []
    monkeypatch.setattr(vmlx_governed.os, "kill",
                        lambda pid, sig: killed.append((pid, sig)))
    with pytest.raises(ValueError, match="foreign_vmlx_process"):
        vmlx_governed.recover_orphan(selection)
    assert not killed
    assert selection.custody_path.exists()
