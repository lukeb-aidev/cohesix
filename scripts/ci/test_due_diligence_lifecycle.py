# Author: Lukas Bower
# Purpose: Verify due-diligence finding and exception lifecycle consistency checks.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
GATE = REPO_ROOT / "scripts" / "ci" / "due_diligence_gate.sh"
FINDINGS_HEADER = (
    "finding_id,severity,disposition,commit_sha,closed_date,closure_evidence\n"
)
EXCEPTIONS_HEADER = (
    "| Exception ID | Related Finding | Severity | Scope | Rationale | "
    "Compensating Controls | Risk Owner | Approved By | Decision Date | "
    "Expiration Date | Status |\n"
    "|---|---|---|---|---|---|---|---|---|---|---|\n"
)


def exception_row(
    exception_id: str,
    finding_id: str,
    status: str,
    expiration: str = "2099-07-15",
) -> str:
    """Return a complete exceptions-register fixture row."""
    return (
        f"| `{exception_id}` | `{finding_id}` | `P2` | scope | reason | "
        f"controls | owner | approver | 2026-07-15 | {expiration} | "
        f"`{status}` |\n"
    )


def finding_row(
    finding_id: str,
    disposition: str,
    *,
    severity: str = "P2",
    commit_sha: str = "",
    closed_date: str = "",
    closure_evidence: str = "",
) -> str:
    """Return a finding fixture with complete closure metadata when closed."""
    if disposition == "CLOSED_VERIFIED":
        commit_sha = commit_sha or ("a" * 40)
        closed_date = closed_date or "2026-07-15"
        closure_evidence = closure_evidence or "cargo test -p rust-risk-audit"
    return (
        f"{finding_id},{severity},{disposition},{commit_sha},{closed_date},"
        f"{closure_evidence}\n"
    )


class DueDiligenceLifecycleTests(unittest.TestCase):
    def run_check(
        self,
        findings: str,
        exceptions: str,
        *,
        findings_header: str = FINDINGS_HEADER,
        exceptions_header: str = EXCEPTIONS_HEADER,
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            findings_path = root / "findings.csv"
            exceptions_path = root / "EXCEPTIONS.md"
            findings_path.write_text(findings_header + findings)
            exceptions_path.write_text(exceptions_header + exceptions)
            environment = os.environ.copy()
            environment["DD_GATE_LOG_DIR"] = str(root / "gate-logs")
            return subprocess.run(
                [
                    "bash",
                    str(GATE),
                    "--check-exceptions-register",
                    str(findings_path),
                    str(exceptions_path),
                ],
                cwd=REPO_ROOT,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

    def test_matching_active_and_closed_lifecycles_pass(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "ACCEPTED_RISK")
            + finding_row("DD-B", "CLOSED_VERIFIED"),
            exception_row("EX-A", "DD-A", "APPROVED_ACTIVE")
            + exception_row("EX-B", "DD-B", "CLOSED", "2026-07-15"),
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("exceptions register gate passed", result.stdout)

    def test_active_exception_requires_accepted_risk_finding(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "CLOSED_VERIFIED"),
            exception_row("EX-A", "DD-A", "APPROVED_ACTIVE"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "APPROVED_ACTIVE requires DD-A to be ACCEPTED_RISK",
            result.stderr,
        )

    def test_closed_exception_requires_closed_verified_finding(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "ACCEPTED_RISK"),
            exception_row("EX-A", "DD-A", "CLOSED", "2026-07-15"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CLOSED requires DD-A to be CLOSED_VERIFIED", result.stderr)

    def test_accepted_risk_requires_active_exception(self) -> None:
        result = self.run_check(finding_row("DD-A", "ACCEPTED_RISK"), "")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "DD-A: ACCEPTED_RISK requires a matching APPROVED_ACTIVE exception",
            result.stderr,
        )

    def test_unknown_exception_status_is_rejected(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "ACCEPTED_RISK"),
            exception_row("EX-A", "DD-A", "ACTIVEISH"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown status 'ACTIVEISH'", result.stderr)

    def test_duplicate_active_exception_mapping_is_rejected(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "ACCEPTED_RISK"),
            exception_row("EX-A", "DD-A", "APPROVED_ACTIVE")
            + exception_row("EX-B", "DD-A", "APPROVED_ACTIVE"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "duplicate APPROVED_ACTIVE exception for DD-A",
            result.stderr,
        )

    def test_every_exception_requires_an_existing_related_finding(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "CLOSED_VERIFIED"),
            exception_row("EX-A", "DD-MISSING", "REVOKED"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("related finding DD-MISSING does not exist", result.stderr)

    def test_expired_active_exception_is_rejected(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "ACCEPTED_RISK"),
            exception_row("EX-A", "DD-A", "APPROVED_ACTIVE", "2000-01-01"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("expired on 2000-01-01", result.stderr)

    def test_exception_policy_metadata_is_required(self) -> None:
        exception = exception_row("EX-A", "DD-A", "APPROVED_ACTIVE").replace(
            "| owner |", "| N/A |"
        )
        result = self.run_check(finding_row("DD-A", "ACCEPTED_RISK"), exception)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing risk owner", result.stderr)

    def test_closed_finding_requires_commit_date_and_evidence(self) -> None:
        result = self.run_check(
            finding_row(
                "DD-A",
                "CLOSED_VERIFIED",
                commit_sha="short",
                closed_date="not-a-date",
                closure_evidence=" ",
            ),
            exception_row("EX-A", "DD-A", "CLOSED", "2026-07-15"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("full 40-hex commit_sha", result.stderr)
        self.assertIn("valid closed_date", result.stderr)
        self.assertIn("requires closure_evidence", result.stderr)

    def test_malformed_exception_row_arity_is_rejected(self) -> None:
        malformed = exception_row(
            "EX-A", "DD-A", "CLOSED", "2026-07-15"
        ).replace("| `CLOSED` |", "| `CLOSED` | unexpected |")
        result = self.run_check(
            finding_row("DD-A", "CLOSED_VERIFIED"),
            malformed,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("expected 11 cells, found 12", result.stderr)

    def test_unknown_finding_disposition_is_rejected(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "RESOLVEDISH"),
            exception_row("EX-A", "DD-A", "REVOKED"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown disposition 'RESOLVEDISH'", result.stderr)

    def test_unknown_finding_severity_is_rejected(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "OPEN", severity="PX"),
            exception_row("EX-A", "DD-A", "REVOKED"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown severity 'PX'", result.stderr)

    def test_extra_findings_csv_field_is_rejected(self) -> None:
        result = self.run_check(
            finding_row("DD-A", "OPEN").rstrip("\n") + ",unexpected\n",
            exception_row("EX-A", "DD-A", "REVOKED"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match the 6-column header", result.stderr)

    def test_missing_findings_csv_field_is_rejected(self) -> None:
        result = self.run_check(
            "DD-A,P2,OPEN,,,\n".rstrip(",\n") + "\n",
            exception_row("EX-A", "DD-A", "REVOKED"),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match the 6-column header", result.stderr)

    def test_empty_exception_id_is_rejected(self) -> None:
        exception = exception_row(
            "EX-A", "DD-A", "CLOSED", "2026-07-15"
        ).replace("| `EX-A` |", "|  |")
        result = self.run_check(
            finding_row("DD-A", "CLOSED_VERIFIED"),
            exception,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("empty exception ID", result.stderr)

    def test_exception_table_requires_the_canonical_header_once(self) -> None:
        wrong_header = EXCEPTIONS_HEADER.replace("Related Finding", "Finding")
        result = self.run_check(
            finding_row("DD-A", "CLOSED_VERIFIED"),
            exception_row("EX-A", "DD-A", "CLOSED", "2026-07-15"),
            exceptions_header=wrong_header,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exactly one canonical header, found 0", result.stderr)

    def test_findings_header_cannot_have_both_lifecycle_columns(self) -> None:
        ambiguous_header = FINDINGS_HEADER.replace(
            "disposition,", "disposition,status,"
        )
        result = self.run_check(
            "DD-A,P2,OPEN,CLOSED_VERIFIED,,,\n",
            exception_row("EX-A", "DD-A", "REVOKED"),
            findings_header=ambiguous_header,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("both disposition and legacy status", result.stderr)

    def test_unknown_findings_header_column_is_rejected(self) -> None:
        unknown_header = FINDINGS_HEADER.replace(
            "closure_evidence", "closure_evidence,mystery"
        )
        result = self.run_check(
            "DD-A,P2,OPEN,,,,\n",
            exception_row("EX-A", "DD-A", "REVOKED"),
            findings_header=unknown_header,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown columns: mystery", result.stderr)

    def run_missing_attestation_gate(
        self,
        log_root: Path,
        *,
        collect_all: bool,
    ) -> subprocess.CompletedProcess[str]:
        """Run the full gate against intentionally missing staged evidence."""

        environment = os.environ.copy()
        environment.update(
            {
                "DD_GATE_LOG_DIR": str(log_root),
                "DD_REUSE_STAGED_EVIDENCE_FROM": str(
                    log_root.parent / "missing-evidence"
                ),
                "DD_REUSE_STAGED_EVIDENCE_TARGET": "qemu",
                "DD_SKIP_CARGO_AUDIT": "1",
                "DD_SKIP_CARGO_DENY": "1",
                "DD_SKIP_REGRESSION_BATCH": "1",
            }
        )
        arguments = ["bash", str(GATE)]
        if collect_all:
            arguments.append("--collect-all")
        return subprocess.run(
            arguments,
            cwd=REPO_ROOT,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_full_gate_fails_fast_by_default(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            log_root = Path(temp_dir) / "gate-logs"
            result = self.run_missing_attestation_gate(
                log_root,
                collect_all=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((log_root / "staged-evidence-state.log").is_file())
            self.assertFalse((log_root / "stage-01-attestation.log").exists())
            self.assertIn("FAILURES (1)", result.stdout)

    def test_collect_all_mode_accumulates_later_failures(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            log_root = Path(temp_dir) / "gate-logs"
            result = self.run_missing_attestation_gate(
                log_root,
                collect_all=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((log_root / "staged-evidence-state.log").is_file())
            self.assertTrue((log_root / "stage-01-attestation.log").is_file())
            self.assertTrue((log_root / "stage-02-attestation.log").is_file())
            self.assertTrue((log_root / "stage-03-attestation.log").is_file())
            self.assertTrue((log_root / "stage-04-attestation.log").is_file())
            self.assertIn("INCOMPLETE RUN (3)", result.stdout)

    def test_staged_reuse_requires_an_explicit_target(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            environment = os.environ.copy()
            environment.update(
                {
                    "DD_GATE_LOG_DIR": str(root / "gate-logs"),
                    "DD_REUSE_STAGED_EVIDENCE_FROM": str(root / "evidence"),
                }
            )

            result = subprocess.run(
                ["bash", str(GATE)],
                cwd=REPO_ROOT,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 2)
            self.assertIn(
                "requires DD_REUSE_STAGED_EVIDENCE_TARGET=qemu|pi4",
                result.stderr,
            )

    def test_reused_regression_requires_content_bound_aggregate(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            log_root = root / "gate-logs"
            stage_three_root = root / "stage-three"
            stage_three_root.mkdir()
            environment = os.environ.copy()
            environment.update(
                {
                    "DD_GATE_LOG_DIR": str(log_root),
                    "DD_REUSE_STAGED_EVIDENCE_FROM": str(
                        root / "missing-evidence"
                    ),
                    "DD_REUSE_STAGED_EVIDENCE_TARGET": "qemu",
                    "DD_REUSE_REGRESSION_BATCH_FROM": str(stage_three_root),
                    "DD_SKIP_CARGO_AUDIT": "1",
                    "DD_SKIP_CARGO_DENY": "1",
                }
            )
            result = subprocess.run(
                ["bash", str(GATE), "--collect-all"],
                cwd=REPO_ROOT,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            regression_log = log_root / "regression-batch-reuse.log"
            self.assertTrue(regression_log.is_file())
            self.assertIn(
                "transport aggregate",
                regression_log.read_text(encoding="utf-8"),
            )

    def write_target_metadata(self, state: Path, target: str = "qemu") -> None:
        """Write the exact target metadata emitted by the staged runner."""

        state.mkdir(parents=True, exist_ok=True)
        (state / "target.env").write_text(
            "\n".join(
                [
                    f"TEST_PLAN_TARGET={target}",
                    "TEST_PLAN_TARGET_MATRIX_VERSION=2",
                    f"TEST_PLAN_STATE_DIR={state.resolve()}",
                    f"TEST_PLAN_REPO_ROOT={REPO_ROOT.resolve()}",
                    "TEST_PLAN_STARTED_AT_UTC=2026-07-23T00:00:00Z",
                    "",
                ]
            ),
            encoding="utf-8",
        )

    def run_reused_state_gate(
        self,
        root: Path,
        state: Path,
    ) -> subprocess.CompletedProcess[str]:
        """Run the gate far enough to validate reused staged-state integrity."""

        environment = os.environ.copy()
        environment.update(
            {
                "DD_GATE_LOG_DIR": str(root / "gate-logs"),
                "DD_REUSE_STAGED_EVIDENCE_FROM": str(state),
                "DD_REUSE_STAGED_EVIDENCE_TARGET": "qemu",
                "DD_SKIP_CARGO_AUDIT": "1",
                "DD_SKIP_CARGO_DENY": "1",
                "DD_SKIP_REGRESSION_BATCH": "1",
            }
        )
        return subprocess.run(
            ["bash", str(GATE)],
            cwd=REPO_ROOT,
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_reused_state_rejects_target_metadata_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state = root / "evidence"
            self.write_target_metadata(state, target="pi4")

            result = self.run_reused_state_gate(root, state)

            self.assertNotEqual(result.returncode, 0)
            validation_log = root / "gate-logs" / "staged-evidence-state.log"
            self.assertIn(
                "target.env target mismatch: expected qemu, found pi4",
                validation_log.read_text(encoding="utf-8"),
            )

    def test_reused_state_rejects_unknown_target_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            state = root / "evidence"
            self.write_target_metadata(state)
            with (state / "target.env").open("a", encoding="utf-8") as handle:
                handle.write("UNTRUSTED_FIELD=value\n")

            result = self.run_reused_state_gate(root, state)

            self.assertNotEqual(result.returncode, 0)
            validation_log = root / "gate-logs" / "staged-evidence-state.log"
            self.assertIn(
                "target.env contains unknown key: UNTRUSTED_FIELD",
                validation_log.read_text(encoding="utf-8"),
            )

    def test_reused_state_rejects_incomplete_markers_and_records(self) -> None:
        for incomplete_path in (
            Path("stage_03.incomplete"),
            Path("incomplete") / "stage-03.md",
        ):
            with self.subTest(incomplete_path=incomplete_path):
                with tempfile.TemporaryDirectory() as temp_dir:
                    root = Path(temp_dir)
                    state = root / "evidence"
                    self.write_target_metadata(state)
                    marker = state / incomplete_path
                    marker.parent.mkdir(parents=True, exist_ok=True)
                    marker.write_text("incomplete\n", encoding="utf-8")

                    result = self.run_reused_state_gate(root, state)

                    self.assertNotEqual(result.returncode, 0)
                    validation_log = (
                        root / "gate-logs" / "staged-evidence-state.log"
                    )
                    self.assertIn(
                        "active incomplete",
                        validation_log.read_text(encoding="utf-8"),
                    )

    def test_stage_five_records_audit_versions_and_publishes_logs(self) -> None:
        gate_source = GATE.read_text(encoding="utf-8")
        stage_source = (
            REPO_ROOT
            / "scripts"
            / "ci"
            / "test_plan_stage_05_due_diligence.sh"
        ).read_text(encoding="utf-8")

        self.assertIn(
            'run_step "cargo-audit-version" cargo audit --version',
            gate_source,
        )
        self.assertIn(
            'run_step "cargo-deny-version" cargo deny --version',
            gate_source,
        )
        self.assertIn('stage5_root="${TP_ATTEMPT_DIR}/governance"', stage_source)
        self.assertIn('DD_GATE_LOG_DIR="${audit_root}"', stage_source)
        self.assertIn("stage_05_artifact_root.path", stage_source)
        self.assertIn("publish-root", stage_source)


if __name__ == "__main__":
    unittest.main()
