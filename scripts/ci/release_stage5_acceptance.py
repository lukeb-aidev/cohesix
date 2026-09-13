#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Admit the exact approved 1.0.0-beta evidence carry-forward decision.
# Copyright 2026 Lukas Bower

"""Release-owner acceptance is separate from current-source staged PASS."""

from __future__ import annotations

import argparse
from datetime import date, datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
from typing import Any

import test_plan_evidence as evidence


RELEASE = "1.0.0-beta"
POLICY = Path("docs/audit/RELEASE_1_0_0_BETA_CARRY_FORWARD.toml")
BASE = "aeaa8edb2fba51b82e530f9248e60180f51f23a9"
PRIOR = "22e3d08ffb5d8ca2a5b3312ceb354f6395532219"
PROPOSAL_SHA = "1d5c5b076cb98e86480b1696a19df8cdd552cae9490af585092ab46d5cfdd90b"
APPROVAL_SHA = "009fdb80aaf6be0206636bd5886337bcbc9d84101260a3521e7afaa44f2befc1"
REVIEW_SHA = "31f84049561cabce88182450be0e977838eeb9adc17b7728701beeb6fd652cf2"
HOST_PATCH_SHA = "54c1ea4bc418b41400c450f47de8e354c3c21aa5e4181748602fb6bfdf85a738"
HOST_FILES = {
    "apps/root-task/src/sel4.rs":
        "d006cb0fd0da25f27751a96d49bf227e862000d6b98e3d6971e318cfa56010da",
    "apps/root-task/src/event/mod.rs":
        "403019bd1576a989eeaa2a367303830fa75e19d6ca98813bf1e604230e2302e1",
}
PRIOR_PINS = [
    ("a276ed4880334790e82d3e7e81196cf1d46a0d6ffe5aa3862e6c12cad0dbe54e",
     "3c85816faf5c4c1583283d6722a4eaef5487305321791ff76311570490ab413e"),
    ("8e8d3ad90972383ebff58ba28fa603ebcc216dbb02786126dfadf0dc6c2149f7",
     "aefb0f2516226e931d401a53353af2ddcaad7d68a7ddb4157cc8f54d8295c051"),
    ("6539864d49544f0f59939f12e0513087391d7397e0dcb430dee7ae4a8ed217f5",
     "13f534ec79bcc7ca0c94bfe26ad7317d1dacf85dd2f39dec9b7684bedc094dc9"),
    ("65cff2471d1fb18c69980dadcbe731337c00ceb5c0b154593e297f19aaa2dd93",
     "167a05319d6cd5a189d07fd67a11dc98f418fc0daecfe30532ba6a62b887bbd1"),
]
SCRIPT_PATHS = {
    "scripts/ci/release_stage5_acceptance.py",
    "scripts/ci/test_release_stage5_acceptance.py",
    "scripts/ci/due_diligence_lifecycle.py",
    "scripts/ci/due_diligence_gate.sh",
    "scripts/ci/test_plan_evidence.py",
}
DOCUMENT_PATHS = {
    "docs/BUILD_PLAN.md", "docs/TEST_PLAN.md",
    "docs/audit/AUDIT_REPORT_2026-09-13.md", "docs/audit/BLOCKERS.md",
    "docs/audit/CONTROL_TRACEABILITY.md", "docs/audit/DUE_DILIGENCE_PLAN.md",
    "docs/audit/EXCEPTIONS.md", str(POLICY), "docs/audit/findings.csv",
    "docs/audit/checklists/ARCHITECTURE_CHECKLIST.md",
    "docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md",
    "docs/audit/checklists/SECURITY_CHECKLIST.md",
}
ADVISORY_COMMANDS = [["cargo", "audit"], ["cargo", "deny", "check", "advisories"]]
RELEASE_CHECKS = (
    "required-audit-assets", "release-carry-forward-validation",
    "release-guardrails-findings", "release-guardrails-exceptions",
    "hardcoded-secret-scan",
)
REQUIRED_EVIDENCE_SHA = "4deed34b04768a8aff6a836ded0f44ecd7156bfbc7024542d3ba355d5ce54173"
REQUIRED_EVIDENCE_IDS = frozenset({
    "dd26-review", "dd27-review", "dd28-review", "dd29-review", "dd30-review",
    "native-base-tcp", "keyboard-absent", "exception-renewal",
    "qemu-worker-convergence", "burn-in-current", "burn-in-run-record",
    "burn-in-review", "burn-in-timed-result", "burn-in-focused-maintenance",
    "burn-in-host-checks", "burn-in-human-approval", "burn-in-publication",
    "failed-fresh-attempt",
})


class AcceptanceError(ValueError):
    """The exact owner decision does not cover these inputs."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AcceptanceError(message)


def digest(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def regular(path: Path) -> bytes:
    require(path.is_file() and not path.is_symlink(), f"not a regular file: {path}")
    return path.read_bytes()


def bound_file(path: Path) -> dict[str, str]:
    return {"path": str(path.absolute()), "sha256": digest(regular(path))}


def verify_file(record: dict[str, str]) -> Path:
    require(set(record) == {"path", "sha256"}, "invalid file binding")
    path = Path(record["path"])
    require(path.is_absolute(), "evidence path must be absolute")
    require(digest(regular(path)) == record["sha256"], f"changed evidence: {path}")
    return path


def git(root: Path, *arguments: str) -> bytes:
    return subprocess.run(
        ["git", "-C", str(root), *arguments], check=True, capture_output=True,
    ).stdout


def load_policy(root: Path, *, today: date) -> dict[str, Any]:
    """Validate one immutable approval; the date is injectable only in Python."""
    policy = tomllib.loads(regular(root / POLICY).decode())
    fixed = {
        "schema": "cohesix.release-carry-forward-policy/v1",
        "release_id": RELEASE,
        "scope_id": "stage1-4-carry-forward-and-dd30-host-model",
        "status": "APPROVED_ACTIVE",
        "risk_owner": "Lukas Bower",
        "approved_by": "Lukas Bower",
        "decision_date": "2026-09-13",
        "expiration_date": "2026-10-13",
        "base_commit": BASE,
        "prior_commit": PRIOR,
        "proposal_sha256": PROPOSAL_SHA,
        "approval_sha256": APPROVAL_SHA,
        "approval_quote": "Approved",
        "independent_review_sha256": REVIEW_SHA,
    }
    extra = {
        "prior_state", "proposal_path", "approval_path",
        "independent_review_path", "allowed_closure_paths", "prior_stage_refs",
        "required_supplemental_evidence",
    }
    require(set(policy) == set(fixed) | extra, "unknown or missing policy fields")
    for key, value in fixed.items():
        require(policy[key] == value, f"unapproved policy {key}")
    require(date(2026, 9, 13) <= today <= date(2026, 10, 13),
            "release decision is expired or not yet effective")
    for prefix in ("proposal", "approval", "independent_review"):
        verify_file({"path": policy[f"{prefix}_path"],
                     "sha256": policy[f"{prefix}_sha256"]})
    proposal = json.loads(regular(Path(policy["proposal_path"])))
    require(proposal["host_only_files"] == HOST_FILES, "host approval file mismatch")
    require(proposal["host_only_patch_sha256"] == HOST_PATCH_SHA,
            "host approval patch mismatch")
    require(policy["prior_state"] == proposal["prior_stage_state"],
            "original state differs from owner approval")
    approval = json.loads(regular(Path(policy["approval_path"])))
    require(approval["decision"] == "APPROVED"
            and approval["exact_answer"] == "Approved"
            and approval["release_id"] == RELEASE,
            "owner approval does not admit this release")
    verify_file({"path": approval["packet"], "sha256": approval["packet_sha256"]})
    require(policy["prior_stage_refs"] == [
        {"stage": n, "ref_sha256": ref, "context_digest": context}
        for n, (ref, context) in enumerate(PRIOR_PINS, 1)
    ], "original stage identity mismatch")
    paths = policy["allowed_closure_paths"]
    require(isinstance(paths, list) and len(paths) == len(set(paths)),
            "invalid closure path set")
    require(set(paths) == SCRIPT_PATHS | DOCUMENT_PATHS,
            "closure allowlist differs from the approved implementation scope")
    for name in paths:
        path = Path(name)
        require(not path.is_absolute() and ".." not in path.parts,
                "unconfined closure path")
        require(name in SCRIPT_PATHS or (
            name.startswith("docs/") and path.suffix in {".md", ".toml", ".csv"}
        ), f"non-governance closure path: {name}")
    require(str(POLICY) in paths, "policy must be within its closure scope")
    required_evidence(policy)
    return policy


def required_evidence(policy: dict[str, Any]) -> list[dict[str, str]]:
    """Verify the finite approved historical set without rewriting its verdicts."""
    records = policy["required_supplemental_evidence"]
    require(isinstance(records, list) and bool(records), "missing required historical evidence")
    require(evidence.canonical_digest(records) == REQUIRED_EVIDENCE_SHA,
            "required historical evidence set differs from owner-approved closure")
    ids = []
    for record in records:
        require(isinstance(record, dict) and set(record) == {
            "id", "path", "sha256", "original_status",
        }, "invalid required historical evidence record")
        ids.append(record["id"])
        require(isinstance(record["original_status"], str)
                and bool(record["original_status"]), "missing original historical status")
        verify_file({"path": record["path"], "sha256": record["sha256"]})
    require(len(ids) == len(set(ids)) and set(ids) == REQUIRED_EVIDENCE_IDS,
            "required historical evidence IDs are missing, duplicated, or unapproved")
    return records


def validate_host_successor(root: Path, *, today: date) -> dict[str, Any]:
    """Permit only the two exactly approved host/test byte changes."""
    policy = load_policy(root, today=today)
    for name, expected in HOST_FILES.items():
        require(digest(regular(root / name)) == expected,
                f"host successor changed: {name}")
    patch = git(root, "diff", "--binary", BASE, "--", *HOST_FILES)
    require(digest(patch) == HOST_PATCH_SHA, "host successor patch changed")
    changed = set(git(root, "diff", "--name-only", BASE).decode().splitlines())
    changed.update(git(root, "ls-files", "--others", "--exclude-standard").decode().splitlines())
    require(changed <= set(HOST_FILES) | set(policy["allowed_closure_paths"]),
            "source delta contains unapproved implementation or configuration")
    return policy


def original_stages(policy: dict[str, Any]) -> list[dict[str, Any]]:
    """Retain old identities and verify their complete immutable evidence graph."""
    state = Path(policy["prior_state"])
    records = []
    for stage, (ref_sha, context_sha) in enumerate(PRIOR_PINS, 1):
        ref = state / f"stage_{stage:02d}.qemu.attestation.json"
        require(digest(regular(ref)) == ref_sha, "original stage reference changed")
        manifest_path = evidence.verify_recorded_stage(state, stage, "qemu")
        manifest = evidence.load_json(manifest_path)
        require(manifest["inputs"]["context_digest"] == context_sha,
                "original context changed")
        context_path = manifest_path.parent / manifest["inputs"]["manifest"]["path"]
        context = evidence.load_json(context_path)
        require(context["source"]["git_head"] == PRIOR,
                "original source commit changed")
        records.append({
            "stage": stage, "target": "qemu", "source_commit": PRIOR,
            "context_digest": context_sha, "reference": bound_file(ref),
            "manifest": bound_file(manifest_path), "context": bound_file(context_path),
            "marker": bound_file(state / f"stage_{stage:02d}.qemu.done"),
        })
    return records


def source_binding(root: Path) -> dict[str, str]:
    require(not git(root, "status", "--porcelain", "--untracked-files=normal"),
            "final release source must be clean and committed")
    require(git(root, "rev-parse", "HEAD^").decode().strip() == BASE,
            "release closure must be the single atomic successor of approved base")
    return {
        "commit": git(root, "rev-parse", "HEAD").decode().strip(),
        "tree": git(root, "rev-parse", "HEAD^{tree}").decode().strip(),
        "index_sha256": digest(git(root, "ls-files", "--stage", "-z")),
    }


def tool_binding(name: str) -> dict[str, str]:
    path = shutil.which(name)
    require(path is not None, f"missing retained advisory tool: {name}")
    resolved = Path(path).resolve()
    result = subprocess.run([name, "--version"], check=True, capture_output=True)
    return {**bound_file(resolved), "version": result.stdout.decode().strip()}


def advisory_binding(root: Path, results: Path) -> dict[str, Any]:
    """Bind existing successful commands; this never reexecutes advisory checks."""
    rows = json.loads(regular(results))
    require(isinstance(rows, list) and len(rows) == 2, "expected two advisory results")
    for row, command in zip(rows, ADVISORY_COMMANDS, strict=True):
        require(row["command"] == command and type(row["exit_code"]) is int
                and row["exit_code"] == 0,
                "advisory command is missing or failed")
        completed = datetime.fromisoformat(row["completed_utc"])
        require(completed.date() == date(2026, 9, 13), "advisory receipt date mismatch")
    lock = regular(root / "Cargo.lock")
    require(lock == git(root, "show", f"{BASE}:Cargo.lock"), "advisory lockfile changed")
    return {
        "results": bound_file(results),
        "logs": [bound_file(results.parent / f"{n}.log") for n in range(2)],
        "cargo_lock_sha256": digest(lock),
        "tools": {name: tool_binding(name) for name in ("cargo-audit", "cargo-deny")},
        "identity_collection": "post-run binding of retained commands; checks not rerun",
    }


def host_check_binding(policy: dict[str, Any]) -> dict[str, Any]:
    """Bind the exact repaired suite reported in the approved review packet."""
    directory = Path(policy["proposal_path"]).parent.parent / "checks/host-model-repair"
    results = directory / "results.json"
    rows = json.loads(regular(results))
    command = ["cargo", "test", "-p", "root-task", "--no-default-features",
               "--features", "driver-tests-pi4", "--lib", "--", "--test-threads=1"]
    require(len(rows) == 2 and rows[1]["command"] == command
            and rows[1]["exit_code"] == 0, "approved host repair test did not pass")
    log = directory / "1.log"
    require(b"test result: ok. 2388 passed; 0 failed; 0 ignored;" in regular(log),
            "approved host repair inventory differs from its retained result")
    return {"results": bound_file(results), "log": bound_file(log),
            "source_files": HOST_FILES, "command": command}


def validate_input(root: Path, path: Path, release: str) -> dict[str, Any]:
    require(release == RELEASE, "explicit DD_RELEASE_ID=1.0.0-beta is required")
    record = json.loads(regular(path))
    require(set(record) == {
        "schema", "release_id", "source", "policy", "original_stages",
        "advisories", "host_repair_checks", "required_supplemental_evidence",
        "finalization_receipts", "created_utc",
    }, "unknown or missing finalized input fields")
    require(record["schema"] == "cohesix.release-stage5-input/v1"
            and record["release_id"] == RELEASE, "invalid finalized release context")
    policy = validate_host_successor(root, today=date.today())
    require(record["policy"] == bound_file(root / POLICY), "finalized policy changed")
    require(record["source"] == source_binding(root), "final source identity changed")
    require(record["host_repair_checks"] == host_check_binding(policy),
            "approved host repair evidence changed")
    require(record["original_stages"] == original_stages(policy),
            "original stage graph or bound inputs changed")
    original_advisory = record["advisories"]
    results = verify_file(original_advisory["results"])
    require(original_advisory == advisory_binding(root, results),
            "retained advisory inputs or tools changed")
    require(record["required_supplemental_evidence"] == required_evidence(policy),
            "finalized required historical evidence changed")
    require(bool(record["finalization_receipts"]), "missing closure verification receipts")
    for item in record["finalization_receipts"]:
        verify_file(item)
    return record


def write_once(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")


def checked_governance_logs(log_root: Path) -> list[dict[str, Any]]:
    """Require every unique check to have completed successfully in this run."""
    rows = [json.loads(line) for line in regular(
        log_root / "release-checks.jsonl"
    ).decode().splitlines()]
    require([row["name"] for row in rows] == list(RELEASE_CHECKS),
            "missing, duplicate, or reordered release governance checks")
    for row in rows:
        require(type(row["exit_code"]) is int and row["exit_code"] == 0
                and bool(row["command"]),
                f"release governance check did not pass: {row['name']}")
        require(verify_file(row["log"]) ==
                (log_root / f"{row['name']}.log").absolute(),
                "release check log escaped this run")
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("seal-input", "validate", "publish"))
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--advisories", type=Path)
    parser.add_argument("--finalization-receipt", type=Path, action="append", default=[])
    parser.add_argument("--log-root", type=Path)
    args = parser.parse_args()
    try:
        release = os.environ.get("DD_RELEASE_ID", "")
        require(release == RELEASE, "explicit DD_RELEASE_ID=1.0.0-beta is required")
        if args.mode == "seal-input":
            policy = validate_host_successor(args.root, today=date.today())
            require(args.advisories is not None and bool(args.finalization_receipt),
                    "seal requires retained advisories and later scoped receipts")
            record = {
                "schema": "cohesix.release-stage5-input/v1", "release_id": RELEASE,
                "source": source_binding(args.root), "policy": bound_file(args.root / POLICY),
                "original_stages": original_stages(policy),
                "advisories": advisory_binding(args.root, args.advisories),
                "host_repair_checks": host_check_binding(policy),
                "required_supplemental_evidence": required_evidence(policy),
                "finalization_receipts": [bound_file(p) for p in args.finalization_receipt],
                "created_utc": datetime.now(timezone.utc).isoformat(),
            }
            write_once(args.input, record)
            print(f"sealed release input: {args.input}; no stage acceptance")
        else:
            record = validate_input(args.root, args.input, release)
            if args.mode == "publish":
                require(args.log_root is not None, "publication requires unique gate logs")
                logs = checked_governance_logs(args.log_root)
                write_once(args.log_root / "release-stage5-acceptance.json", {
                    "schema": "cohesix.release-stage5-acceptance/v1",
                    "status": "PASS_WITH_RESIDUAL_RISK", "release_id": RELEASE,
                    "accepted_utc": datetime.now(timezone.utc).isoformat(),
                    "source": record["source"], "input": bound_file(args.input),
                    "policy": record["policy"], "original_stages": record["original_stages"],
                    "advisories": record["advisories"], "governance_logs": logs,
                    "host_repair_checks": record["host_repair_checks"],
                    "required_supplemental_evidence": record["required_supplemental_evidence"],
                    "finalization_receipts": record["finalization_receipts"],
                    "dd30_dynamic_fault_wake": "NOT_EXECUTED",
                    "current_source_stages_01_04": "NOT_RERUN_OWNER_ACCEPTED_GAP",
                    "generic_stage_markers_created": False,
                })
            print(f"release decision validated: DD_RELEASE_ID={RELEASE} "
                  f"input_sha256={digest(regular(args.input))}; "
                  "original source identities retained; dynamic_fault_wake=NOT_EXECUTED")
    except (AcceptanceError, evidence.EvidenceError, OSError, ValueError,
            KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f"release acceptance blocked: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
