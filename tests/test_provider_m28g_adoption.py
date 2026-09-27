# Author: Lukas Bower
# Purpose: Reject altered Release B installs and walkthroughs outside frozen budgets.
# Copyright 2026 Lukas Bower
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import sys

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/ci"))
import provider_m28g_adoption as adoption  # noqa: E402

SOURCE = "a" * 40
PACKAGES = {"macos": {"b" * 64}, "linux": {"c" * 64, "d" * 64}}


def attachment(tmp_path: Path, name: str) -> dict[str, object]:
    path = tmp_path / name
    raw = json.dumps({"name": name, "result": "PASS"}).encode()
    path.write_bytes(raw)
    return {"path": str(path), "sha256": hashlib.sha256(raw).hexdigest(),
            "size": len(raw)}


def walkthrough(tmp_path: Path) -> dict[str, object]:
    attachments = {name: attachment(tmp_path, f"{name}.json")
                   for name in adoption.ATTACHMENTS}
    return {
        "schema": adoption.WALKTHROUGH_SCHEMA,
        "source_commit": SOURCE,
        "evaluator": {"kind": "agent", "id": "independent-evaluator-01",
                      "independent": True, "assistance": ["published setup guide"]},
        "installations": {
            host: {"steps": 24, "setup_seconds": 1800,
                   "core_download_bytes": 80 * 1024 * 1024,
                   "interventions": 1,
                   "optional_downloads": [{"name": "model", "bytes": 1000,
                                           "opted_in": True}],
                   "package_sha256": sorted(PACKAGES[host])}
            for host in ("macos", "linux")},
        "first_cuda_seconds": 600,
        "journeys": [{"id": name, "status": "PASS", "original_job_id": f"job-{name}",
                      "native_report": attachments[name]}
                     for name in ("cuda", "peft", "client-composition")],
        "gui_launch": {"macos": ["Finder", "Spotlight", "Dock"],
                       "linux": ["GNOME application grid", "GNOME search"]},
        "doctor": "PASS",
        "lifecycle": {"rollback": "PASS", "uninstall": "PASS",
                      "user_data_retained": True,
                      "external_credentials_retained": True},
        "attachments": attachments,
    }


def test_walkthrough_requires_all_evidence_and_fixed_budgets(tmp_path: Path) -> None:
    record = walkthrough(tmp_path)
    accepted = adoption.validate_walkthrough(record, SOURCE, PACKAGES)
    assert accepted["journey_ids"] == ["client-composition", "cuda", "peft"]
    assert accepted["attachment_count"] == 8
    for mutation in [
        lambda row: row["installations"]["macos"].update(steps=31),
        lambda row: row["installations"]["linux"].update(core_download_bytes=101 * 1024 * 1024),
        lambda row: row.update(first_cuda_seconds=1201),
        lambda row: row["installations"]["macos"].update(package_sha256=["d" * 64]),
        lambda row: row["evaluator"].update(independent=False),
        lambda row: row["journeys"][0].update(original_job_id="../new"),
        lambda row: row["gui_launch"].update(macos=["Terminal"]),
        lambda row: row["lifecycle"].update(user_data_retained=False),
        lambda row: row["attachments"].pop("doctor"),
    ]:
        changed = walkthrough(tmp_path)
        mutation(changed)
        with pytest.raises((ValueError, KeyError, TypeError)):
            adoption.validate_walkthrough(changed, SOURCE, PACKAGES)
    changed = walkthrough(tmp_path)
    Path(changed["attachments"]["cuda"]["path"]).write_text("altered")
    with pytest.raises(ValueError, match="attachment changed"):
        adoption.validate_walkthrough(changed, SOURCE, PACKAGES)


def test_aggregate_binds_release_and_both_installer_results(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                     text=True).strip()
    install_files = {}
    for host in ("macos", "linux"):
        path = tmp_path / f"{host}-installer.json"
        path.write_text("{}")
        install_files[host] = path
    release_path = tmp_path / "release.json"
    release_path.write_text("{}")
    packet = walkthrough(tmp_path)
    packet["source_commit"] = source
    walk_path = tmp_path / "walkthrough.json"
    walk_path.write_text(json.dumps(packet))
    reference = tmp_path / "reference.json"
    reference.write_text(json.dumps({
        "schema": adoption.SCHEMA, "source_root": str(ROOT),
        "source_commit": source, "release_result": str(release_path),
        "macos_installer_result": str(install_files["macos"]),
        "linux_installer_result": str(install_files["linux"]),
        "walkthrough": str(walk_path),
    }))

    def result(path: Path, kind: str) -> dict[str, object]:
        if kind == "release":
            return {"version": "1.2.0", "source_commit": source,
                    "checks": ["macos", "linux", "pi4", "macos-installer",
                               "linux-installer"],
                    "qualifications": [{"kind": f"{host}-installer",
                                        "sha256": hashlib.sha256(
                                            install_files[host].read_bytes()).hexdigest()}
                                       for host in ("macos", "linux")], }
        host = path.name.removesuffix("-installer.json")
        return {"host": host, "version": "1.2.0", "source_commit": source,
                "claim": "native-installer-publisher-and-installed-readback",
                "packages": [{"sha256": digest} for digest in sorted(PACKAGES[host])]}

    monkeypatch.setattr(adoption, "read_result", result)
    assert adoption.run_live("m28g-adoption-live", reference,
                             "mac-apple-m4-macos27", tmp_path / "result") == 0
    summary = json.loads((tmp_path / "result/summary.json").read_text())
    assert summary["result"] == "PASS"
    assert summary["native_outcome_reverified"] is False
    with pytest.raises(ValueError, match="aggregate host"):
        adoption.run_live("m28g-adoption-live", reference,
                          "jetson-orin-nano-jp7", tmp_path / "wrong-host")
    assert not (tmp_path / "wrong-host").exists()
