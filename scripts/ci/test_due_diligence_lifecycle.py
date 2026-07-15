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


if __name__ == "__main__":
    unittest.main()
