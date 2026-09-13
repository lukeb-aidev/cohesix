# Author: Lukas Bower
# Purpose: Keep the approved release exception narrow and normal stage reuse strict.
# Copyright 2026 Lukas Bower

"""Deterministic policy, provenance, and failure-publication contracts."""

from __future__ import annotations

import copy
from datetime import date
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import release_stage5_acceptance as release
import test_plan_evidence as evidence


def toml_value(value: object) -> str:
    """Serialize only the simple literal shapes in this independent fixture."""
    if isinstance(value, dict):
        return "{ " + ", ".join(
            f"{key} = {toml_value(item)}" for key, item in value.items()
        ) + " }"
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(item) for item in value) + "]"
    return json.dumps(value)


class ReleasePolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        packet = self.root / "REVIEW.md"
        packet.write_text("Exact test approval packet.\n")
        self.proposal = self.root / "proposal.json"
        self.proposal.write_text(json.dumps({
            "host_only_files": release.HOST_FILES,
            "host_only_patch_sha256": release.HOST_PATCH_SHA,
            "prior_stage_state": str(self.root / "original-state"),
        }))
        self.approval = self.root / "approval.json"
        self.approval.write_text(json.dumps({
            "decision": "APPROVED", "exact_answer": "Approved",
            "release_id": "1.0.0-beta", "packet": str(packet),
            "packet_sha256": release.digest(packet.read_bytes()),
        }))
        review = self.root / "review.json"
        review.write_text('{"source_review": "PASS"}')
        historical = self.root / "retained-incomplete.json"
        historical.write_text('{"status":"INCOMPLETE","fault_test":"NOT_EXECUTED"}')
        self.required = [{
            "id": "required-original-gap", "path": str(historical),
            "sha256": release.digest(historical.read_bytes()),
            "original_status": "INCOMPLETE; fault test not executed",
        }]
        hashes = {
            "PROPOSAL_SHA": release.digest(self.proposal.read_bytes()),
            "APPROVAL_SHA": release.digest(self.approval.read_bytes()),
            "REVIEW_SHA": release.digest(review.read_bytes()),
            "REQUIRED_EVIDENCE_SHA": evidence.canonical_digest(self.required),
            "REQUIRED_EVIDENCE_IDS": frozenset({"required-original-gap"}),
        }
        patcher = mock.patch.multiple(release, **hashes)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.policy = {
            "schema": "cohesix.release-carry-forward-policy/v1",
            "release_id": "1.0.0-beta",
            "scope_id": "stage1-4-carry-forward-and-dd30-host-model",
            "status": "APPROVED_ACTIVE", "risk_owner": "Lukas Bower",
            "approved_by": "Lukas Bower", "decision_date": "2026-09-13",
            "expiration_date": "2026-10-13", "base_commit": release.BASE,
            "prior_commit": release.PRIOR,
            "prior_state": str(self.root / "original-state"),
            "proposal_path": str(self.proposal), "proposal_sha256": hashes["PROPOSAL_SHA"],
            "approval_path": str(self.approval), "approval_sha256": hashes["APPROVAL_SHA"],
            "approval_quote": "Approved", "independent_review_path": str(review),
            "independent_review_sha256": hashes["REVIEW_SHA"],
            "allowed_closure_paths": sorted(release.SCRIPT_PATHS | release.DOCUMENT_PATHS),
            "prior_stage_refs": [
                {"stage": n, "ref_sha256": ref, "context_digest": context}
                for n, (ref, context) in enumerate(release.PRIOR_PINS, 1)
            ],
            "required_supplemental_evidence": self.required,
        }

    def validate(self, policy: dict | None = None, *, day: date = date(2026, 9, 13)) -> dict:
        path = self.root / release.POLICY
        path.parent.mkdir(parents=True, exist_ok=True)
        value = self.policy if policy is None else policy
        path.write_text("\n".join(
            f"{key} = {toml_value(item)}" for key, item in value.items()
        ))
        return release.load_policy(self.root, today=day)

    def test_exact_approval_is_valid_only_in_its_bounded_window(self) -> None:
        self.assertEqual(self.validate()["release_id"], "1.0.0-beta")
        self.validate(day=date(2026, 10, 13))
        for day in (date(2026, 9, 12), date(2026, 10, 14)):
            with self.subTest(day=day), self.assertRaises(release.AcceptanceError):
                self.validate(day=day)

    def test_owner_status_scope_dates_and_sources_cannot_be_repurposed(self) -> None:
        mutations = {
            "release_id": "1.0.1", "scope_id": "all-p1",
            "status": "REVOKED", "risk_owner": "another owner",
            "approved_by": "automated test", "decision_date": "2026-09-12",
            "expiration_date": "2026-10-14", "base_commit": "f" * 40,
            "prior_commit": "e" * 40, "approval_quote": "looks good",
        }
        for key, value in mutations.items():
            with self.subTest(key=key), self.assertRaises(release.AcceptanceError):
                self.validate({**self.policy, key: value})
        for status in ("PROPOSED", "EXPIRED", "APPROVED"):
            with self.subTest(status=status), self.assertRaises(release.AcceptanceError):
                self.validate({**self.policy, "status": status})

    def test_unknown_fields_or_product_allowlist_entries_are_rejected(self) -> None:
        with self.assertRaises(release.AcceptanceError):
            self.validate({**self.policy, "skip_other_findings": True})
        for path in ("apps/root-task/src/kernel.rs", "configs/root_task.toml",
                     "scripts/ci/test_plan_run.sh", "docs/../Cargo.toml", "/docs/x.md"):
            changed = {**self.policy, "allowed_closure_paths": [str(release.POLICY), path]}
            with self.subTest(path=path), self.assertRaises(release.AcceptanceError):
                self.validate(changed)

    def test_original_context_and_stage_reference_are_exact(self) -> None:
        for field in ("ref_sha256", "context_digest"):
            changed = copy.deepcopy(self.policy)
            changed["prior_stage_refs"][2][field] = "0" * 64
            with self.subTest(field=field), self.assertRaises(release.AcceptanceError):
                self.validate(changed)
        with self.assertRaises(release.AcceptanceError):
            self.validate({**self.policy, "prior_state": "/different-state"})

    def test_changed_approval_bytes_cannot_refresh_the_record_hash(self) -> None:
        self.approval.write_text('{"decision":"PROPOSED"}')
        with self.assertRaises(release.AcceptanceError):
            self.validate()
        with self.assertRaises(release.AcceptanceError):
            self.validate({**self.policy, "approval_sha256":
                           release.digest(self.approval.read_bytes())})

    def test_required_history_preserves_gaps_and_cannot_be_replaced_by_extra_files(self) -> None:
        self.assertEqual(self.validate()["required_supplemental_evidence"], self.required)
        invalid_sets = [[], self.required * 2, [{
            **self.required[0], "id": "unrelated-green-test",
        }], [{**self.required[0], "original_status": "PASS"}]]
        for records in invalid_sets:
            with self.subTest(records=records), self.assertRaises(release.AcceptanceError):
                self.validate({**self.policy, "required_supplemental_evidence": records})
        Path(self.required[0]["path"]).write_text('{"status":"PASS"}')
        with self.assertRaises(release.AcceptanceError):
            self.validate()


class SourceAndPublicationTests(unittest.TestCase):
    def test_normal_stage_verifier_still_rejects_stale_current_context(self) -> None:
        manifest = {"inputs": {"context_digest": "original"}, "target": "qemu"}
        with (
            mock.patch.object(evidence, "verify_recorded_stage",
                              return_value=Path("stage.json")),
            mock.patch.object(evidence, "load_json", return_value=manifest),
            mock.patch.object(evidence, "capture_context",
                              return_value={"context_digest": "new"}) as capture,
        ):
            with self.assertRaisesRegex(evidence.EvidenceError, "stale stage"):
                evidence.verify_stage(Path("."), Path("."), 1, "qemu")
            capture.assert_called_once()

    def test_current_context_match_still_passes_after_integrity_verification(self) -> None:
        manifest = {"inputs": {"context_digest": "original"}, "target": "qemu"}
        with (
            mock.patch.object(evidence, "verify_recorded_stage",
                              return_value=Path("stage.json")) as integrity,
            mock.patch.object(evidence, "load_json", return_value=manifest),
            mock.patch.object(evidence, "capture_context",
                              return_value={"context_digest": "original"}),
        ):
            self.assertEqual(
                evidence.verify_stage(Path("."), Path("."), 1, "qemu"), Path("stage.json")
            )
            integrity.assert_called_once()

    def test_unselected_release_is_rejected_before_any_evidence_admission(self) -> None:
        for selected in ("", "1.0.0", "1.0.1-beta"):
            with self.subTest(selected=selected), self.assertRaises(release.AcceptanceError):
                release.validate_input(Path("."), Path("missing"), selected)

    def test_host_successor_requires_exact_patch_and_no_additional_product_delta(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            name = "apps/root-task/src/sel4.rs"
            path = root / name
            path.parent.mkdir(parents=True)
            path.write_bytes(b"reviewed host bytes")
            policy = {"allowed_closure_paths": ["docs/TEST_PLAN.md"]}
            def output(_root: Path, *args: str) -> bytes:
                if args[:2] == ("diff", "--binary"):
                    return b"exact approved patch"
                if args[:2] == ("diff", "--name-only"):
                    return (name + "\ndocs/TEST_PLAN.md\n").encode()
                return b""
            with (
                mock.patch.object(release, "load_policy", return_value=policy),
                mock.patch.object(release, "HOST_FILES",
                                  {name: release.digest(path.read_bytes())}),
                mock.patch.object(release, "HOST_PATCH_SHA",
                                  release.digest(b"exact approved patch")),
                mock.patch.object(release, "git", side_effect=output),
            ):
                release.validate_host_successor(root, today=date(2026, 9, 13))
                with (
                    mock.patch.object(release, "HOST_PATCH_SHA", "0" * 64),
                    self.assertRaises(release.AcceptanceError),
                ):
                    release.validate_host_successor(root, today=date(2026, 9, 13))
                def extra_product(_root: Path, *args: str) -> bytes:
                    if args[0] == "ls-files":
                        return b"apps/root-task/src/new_runtime.rs\n"
                    return output(_root, *args)
                with (
                    mock.patch.object(release, "git", side_effect=extra_product),
                    self.assertRaises(release.AcceptanceError),
                ):
                    release.validate_host_successor(root, today=date(2026, 9, 13))
                path.write_bytes(b"later runtime bytes")
                with self.assertRaises(release.AcceptanceError):
                    release.validate_host_successor(root, today=date(2026, 9, 13))

    def test_final_source_requires_clean_tree(self) -> None:
        with mock.patch.object(release, "git", return_value=b" M file\n"):
            with self.assertRaisesRegex(release.AcceptanceError, "clean"):
                release.source_binding(Path("."))

    def test_a_later_commit_cannot_reseal_the_original_owner_decision(self) -> None:
        def output(_root: Path, *args: str) -> bytes:
            return b"" if args[0] == "status" else b"unapproved-parent\n"
        with mock.patch.object(release, "git", side_effect=output):
            with self.assertRaisesRegex(release.AcceptanceError, "single atomic successor"):
                release.source_binding(Path("."))

    def test_sealed_input_cannot_move_to_a_new_commit(self) -> None:
        record = {
            "schema": "cohesix.release-stage5-input/v1", "release_id": "1.0.0-beta",
            "source": {"commit": "approved-final"}, "policy": {"sha256": "fixed"},
            "original_stages": [], "advisories": {}, "host_repair_checks": {},
            "required_supplemental_evidence": [], "finalization_receipts": [],
            "created_utc": "2026-09-13T11:00:00Z",
        }
        with (
            mock.patch.object(release, "regular", return_value=json.dumps(record).encode()),
            mock.patch.object(release, "validate_host_successor", return_value={}),
            mock.patch.object(release, "bound_file", return_value={"sha256": "fixed"}),
            mock.patch.object(release, "source_binding", return_value={"commit": "later"}),
            self.assertRaisesRegex(release.AcceptanceError, "final source identity"),
        ):
            release.validate_input(Path("."), Path("input.json"), "1.0.0-beta")

    def test_retained_advisories_require_the_exact_commands_and_lock(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.lock").write_bytes(b"original-lock")
            for index in range(2):
                (root / f"{index}.log").write_text("retained successful check\n")
            results = root / "results.json"
            rows = [{"command": command, "exit_code": 0,
                     "completed_utc": "2026-09-13T11:23:00+00:00"}
                    for command in (["cargo", "audit"],
                                    ["cargo", "deny", "check", "advisories"])]
            results.write_text(json.dumps(rows))
            with (
                mock.patch.object(release, "git", return_value=b"original-lock"),
                mock.patch.object(release, "tool_binding", return_value={"sha256": "tool"}),
            ):
                original = release.advisory_binding(root, results)
                for replacement in (
                    {**rows[0], "exit_code": 1},
                    {**rows[0], "command": ["cargo", "audit", "--ignore", "advisory"]},
                ):
                    results.write_text(json.dumps([replacement, rows[1]]))
                    with self.assertRaises(release.AcceptanceError):
                        release.advisory_binding(root, results)
                results.write_text(json.dumps(rows))
                with mock.patch.object(release, "tool_binding",
                                       return_value={"sha256": "changed-tool"}):
                    self.assertNotEqual(release.advisory_binding(root, results), original)
                (root / "Cargo.lock").write_bytes(b"later-lock")
                with self.assertRaises(release.AcceptanceError):
                    release.advisory_binding(root, results)

    def test_exact_host_addendum_does_not_cover_other_protected_files(self) -> None:
        from test_due_diligence_lifecycle import ReleaseOwnerWaiverTests
        import due_diligence_lifecycle as lifecycle

        fixture = ReleaseOwnerWaiverTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        (fixture.root / "apps/root-task/src/sel4.rs").write_bytes(b"approved successor")
        with mock.patch.object(release, "validate_host_successor",
                               return_value={"approval_sha256": "approved"}):
            admission = fixture.validate()
            self.assertIn("not release admission", admission.describe(admitted=False))
            (fixture.root / "apps/root-task/src/kernel.rs").write_bytes(b"new runtime")
            with self.assertRaisesRegex(lifecycle.LifecycleError, "source changed"):
                fixture.validate()

    def test_release_publication_requires_all_unique_successful_checks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = []
            for name in release.RELEASE_CHECKS:
                log = root / f"{name}.log"
                log.write_text("completed\n")
                rows.append({"name": name, "exit_code": 0, "command": ["check", name],
                             "log": release.bound_file(log)})
            receipt = root / "release-checks.jsonl"
            def write(items: list[dict]) -> None:
                receipt.write_text("\n".join(json.dumps(row) for row in items) + "\n")
            write(rows)
            self.assertEqual(release.checked_governance_logs(root), rows)
            for invalid in (rows[:-1], [*rows, rows[-1]], list(reversed(rows))):
                write(invalid)
                with self.assertRaises(release.AcceptanceError):
                    release.checked_governance_logs(root)
            failed = copy.deepcopy(rows)
            failed[2]["exit_code"] = 1
            write(failed)
            with self.assertRaises(release.AcceptanceError):
                release.checked_governance_logs(root)
            write(rows)
            Path(rows[0]["log"]["path"]).write_text("changed\n")
            with self.assertRaises(release.AcceptanceError):
                release.checked_governance_logs(root)

    def test_acceptance_output_cannot_replace_an_existing_result(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "release-stage5-acceptance.json"
            release.write_once(path, {"status": "preserved"})
            with self.assertRaises(FileExistsError):
                release.write_once(path, {"status": "replacement"})
            self.assertEqual(json.loads(path.read_text())["status"], "preserved")


if __name__ == "__main__":
    unittest.main()
