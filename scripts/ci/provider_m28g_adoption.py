#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify exact Release B native packages and a named clean-install walkthrough.
# Copyright 2026 Lukas Bower
"""Retain installed-host adoption evidence without promoting it to native target proof."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any

from provider_m28_live import read_artifact
from provider_matrix import require

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from release_qualify import read_result  # noqa: E402

SCHEMA = "cohesix-m28g-adoption-reference/v1"
WALKTHROUGH_SCHEMA = "cohesix-m28g-adoption-walkthrough/v2"
JOURNEYS = {"cuda", "peft", "client-composition"}
ATTACHMENTS = {"macos-gui", "linux-gui", "doctor", "rollback", "uninstall",
               "cuda", "peft", "client-composition"}
MAX_STEPS = 30
MAX_SETUP_SECONDS = 45 * 60
MAX_CORE_DOWNLOAD_BYTES = 100 * 1024 * 1024
MAX_INTERVENTIONS = 2
MAX_FIRST_CUDA_SECONDS = 20 * 60


def document(path: Path, maximum: int = 1024 * 1024) -> dict[str, Any]:
    """Read a bounded, regular JSON object from the retained private packet."""
    value = json.loads(read_artifact(path, maximum))
    require(isinstance(value, dict), "M28g adoption document must be an object")
    return value


def bound_file(record: dict[str, Any], maximum: int = 16 * 1024 * 1024) -> dict[str, Any]:
    """Rehash one raw walkthrough attachment without following a symlink."""
    require(isinstance(record, dict) and set(record) == {"path", "sha256", "size"},
            "M28g attachment fields")
    path = Path(record["path"])
    require(path.is_absolute() and isinstance(record["sha256"], str)
            and re.fullmatch(r"[0-9a-f]{64}", record["sha256"])
            and type(record["size"]) is int and 0 < record["size"] <= maximum,
            "M28g attachment identity or size")
    raw = read_artifact(path, maximum)
    require(len(raw) == record["size"]
            and hashlib.sha256(raw).hexdigest() == record["sha256"],
            "M28g attachment changed")
    return record


def validate_walkthrough(record: dict[str, Any], source: str,
                         package_hashes: dict[str, set[str]]) -> dict[str, Any]:
    """Check the fixed adoption budgets and all required observed journeys."""
    require(set(record) == {"schema", "source_commit", "evaluator",
                            "installations", "first_cuda_seconds", "journeys", "gui_launch",
                            "doctor", "lifecycle", "attachments"}
            and record["schema"] == WALKTHROUGH_SCHEMA
            and record["source_commit"] == source,
            "M28g walkthrough identity")
    evaluator = record["evaluator"]
    require(isinstance(evaluator, dict)
            and set(evaluator) == {"kind", "id", "independent", "assistance"}
            and evaluator["kind"] in {"person", "agent"}
            and isinstance(evaluator["id"], str)
            and 1 <= len(evaluator["id"]) <= 128
            and evaluator["id"].strip() == evaluator["id"]
            and type(evaluator["independent"]) is bool
            and isinstance(evaluator["assistance"], list)
            and len(evaluator["assistance"]) <= 32
            and all(isinstance(item, str) and 1 <= len(item) <= 256
                    for item in evaluator["assistance"]),
            "M28g evaluator record")
    owner_evaluated = evaluator["id"].casefold() == "lukas bower"
    require((evaluator["kind"] == "person" and evaluator["independent"] is False)
            if owner_evaluated else evaluator["independent"] is True,
            "M28g evaluator independence must match recorded identity")
    installations = record["installations"]
    require(isinstance(installations, dict)
            and set(installations) == {"macos", "linux"}
            and type(record["first_cuda_seconds"]) is int
            and 0 < record["first_cuda_seconds"] <= MAX_FIRST_CUDA_SECONDS,
            "M28g host installation or first-work budget")
    for host, installation in installations.items():
        require(isinstance(installation, dict)
                and set(installation) == {"steps", "setup_seconds", "core_download_bytes",
                                          "interventions", "optional_downloads", "package_sha256"}
                and all(type(installation[name]) is int and 0 <= installation[name] <= limit
                        for name, limit in (("steps", MAX_STEPS),
                                            ("setup_seconds", MAX_SETUP_SECONDS),
                                            ("core_download_bytes", MAX_CORE_DOWNLOAD_BYTES),
                                            ("interventions", MAX_INTERVENTIONS)))
                and installation["steps"] > 0
                and installation["core_download_bytes"] > 0
                and isinstance(installation["package_sha256"], list)
                and bool(installation["package_sha256"])
                and len(installation["package_sha256"])
                == len(set(installation["package_sha256"]))
                and all(isinstance(value, str) and value in package_hashes[host]
                        for value in installation["package_sha256"])
                and isinstance(installation["optional_downloads"], list)
                and all(isinstance(row, dict)
                        and set(row) == {"name", "bytes", "opted_in"}
                        and isinstance(row["name"], str) and 1 <= len(row["name"]) <= 128
                        and type(row["bytes"]) is int and row["bytes"] >= 0
                        and row["opted_in"] is True
                        for row in installation["optional_downloads"]),
                "M28g installation budget or package binding")
        require(set(installation["package_sha256"]) == package_hashes[host],
                f"M28g walkthrough omits a {host} native package")
    journeys = record["journeys"]
    require(isinstance(journeys, list) and len(journeys) == len(JOURNEYS)
            and {row.get("id") for row in journeys if isinstance(row, dict)} == JOURNEYS,
            "M28g three selected journeys")
    for row in journeys:
        require(set(row) == {"id", "status", "original_job_id", "native_report"}
                and row["status"] == "PASS"
                and isinstance(row["original_job_id"], str)
                and re.fullmatch(r"[A-Za-z0-9._-]{1,96}", row["original_job_id"]),
                "M28g journey status or original identity")
        bound_file(row["native_report"])
    gui = record["gui_launch"]
    require(isinstance(gui, dict)
            and set(gui) == {"macos", "linux"}
            and gui["macos"] == ["Finder", "Spotlight", "Dock"]
            and gui["linux"] == ["GNOME application grid", "GNOME search"],
            "M28g native desktop launch evidence")
    require(record["doctor"] == "PASS"
            and isinstance(record["lifecycle"], dict)
            and set(record["lifecycle"]) == {
                "rollback", "uninstall", "user_data_retained",
                "external_credentials_retained",
            }
            and record["lifecycle"]["rollback"] == "PASS"
            and record["lifecycle"]["uninstall"] == "PASS"
            and record["lifecycle"]["user_data_retained"] is True
            and record["lifecycle"]["external_credentials_retained"] is True,
            "M28g doctor or installed lifecycle")
    attachments = record["attachments"]
    require(isinstance(attachments, dict) and set(attachments) == ATTACHMENTS
            and len({row.get("path") for row in attachments.values()
                     if isinstance(row, dict)}) == len(ATTACHMENTS),
            "M28g raw attachment inventory")
    for row in attachments.values():
        bound_file(row)
    require(all(row["native_report"] == attachments[row["id"]]
                for row in journeys),
            "M28g native reports omitted from named raw attachments")
    return {"evaluator_kind": evaluator["kind"], "evaluator_id": evaluator["id"],
            "evaluation_scope": "owner" if owner_evaluated else "independent",
            "installations": installations,
            "journey_ids": sorted(JOURNEYS), "attachment_count": len(attachments)}


def run_live(case: str, reference: Path, host_profile: str, state_dir: Path) -> int:
    """Revalidate both native installer receipts and the named walkthrough."""
    require(case == "m28g-adoption-live" and host_profile == "mac-apple-m4-macos27",
            "M28g adoption aggregate host")
    config = document(reference)
    require(set(config) == {"schema", "source_root", "source_commit",
                            "release_result", "macos_installer_result",
                            "linux_installer_result", "walkthrough"}
            and config["schema"] == SCHEMA
            and isinstance(config["source_commit"], str)
            and re.fullmatch(r"[0-9a-f]{40}", config["source_commit"]),
            "M28g adoption reference fields")
    root = Path(config["source_root"])
    paths = {name: Path(config[name]) for name in
             ("release_result", "macos_installer_result",
              "linux_installer_result", "walkthrough")}
    require(root.is_absolute() and root.is_dir()
            and all(path.is_absolute() for path in paths.values()),
            "M28g absolute source and evidence paths")
    current = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"],
                                      text=True, timeout=5).strip()
    require(current == config["source_commit"], "M28g source changed")
    release = read_result(paths["release_result"], "release")
    require(release["version"] == "1.2.0"
            and release["source_commit"] == current
            and set(release["checks"]) == {"macos", "linux", "pi4",
                                           "macos-installer", "linux-installer"},
            "M28g exact assembled release result")
    packages: dict[str, set[str]] = {}
    for host in ("macos", "linux"):
        result = read_result(paths[f"{host}_installer_result"], "installer")
        qualified = next((row for row in release["qualifications"]
                          if row["kind"] == f"{host}-installer"), None)
        require(result["host"] == host and result["version"] == "1.2.0"
                and result["source_commit"] == current
                and result["claim"] == "native-installer-publisher-and-installed-readback"
                and qualified is not None
                and qualified["sha256"]
                == hashlib.sha256(read_artifact(paths[f"{host}_installer_result"],
                                                1024 * 1024)).hexdigest(),
                f"M28g {host} installed package receipt")
        require(len(result["packages"]) == (1 if host == "macos" else 2),
                f"M28g {host} native package count")
        packages[host] = {row["sha256"] for row in result["packages"]}
        require(len(packages[host]) == len(result["packages"]),
                f"M28g {host} duplicate native package")
    walkthrough = validate_walkthrough(document(paths["walkthrough"]), current,
                                       packages)
    state_dir.mkdir(parents=True, exist_ok=False)
    summary = {"schema": "cohesix-m28g-adoption-live-summary/v1",
               "case": case, "result": "PASS", "proof_class": "live_host",
               "source_commit": current, "version": release["version"],
               "release_result_sha256": hashlib.sha256(
                   read_artifact(paths["release_result"], 1024 * 1024)).hexdigest(),
               "walkthrough_sha256": hashlib.sha256(
                   read_artifact(paths["walkthrough"], 1024 * 1024)).hexdigest(),
               "native_outcome_reverified": False,
               **walkthrough}
    (state_dir / "summary.json").write_text(json.dumps(summary, sort_keys=True, indent=2) + "\n")
    return 0
