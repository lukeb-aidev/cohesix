# Author: Lukas Bower
# Purpose: Validate audit lifecycles while preserving the retired DD30 evidence gap.
# Copyright 2026 Lukas Bower

"""Keep verified remediation, active risk exceptions and accepted gaps distinct."""

from __future__ import annotations

import argparse
import csv
from datetime import date
import pathlib
import re
import subprocess
import sys
import tomllib


RETIRED_FINDING = "DD-2026-0030"
RETIRED_EXCEPTION = "EX-2026-0030"
RETIREMENT_RECORD = pathlib.Path("docs/audit/AUDIT_REPORT_2026-09-13.md")
RETIREMENT_DISPOSITION = "RETIRED_ACCEPTED_GAP"
RETIREMENT_REFERENCE = f"{RETIREMENT_RECORD}#dd30-retirement"


class LifecycleError(ValueError):
    """An audit record does not support its requested disposition."""


def validate_retirement(
    root: pathlib.Path,
    finding: dict[str, str],
    exception: dict[str, str],
) -> str:
    """Validate the recorded owner decision, without source or expiry gates.

    This terminal disposition belongs only to DD30's repaired defect and
    unexecuted dynamic test. A reopened defect must use the ordinary lifecycle.
    """
    expected = {
        "finding_id": RETIRED_FINDING,
        "severity": "P1",
        "disposition": RETIREMENT_DISPOSITION,
        "commit_sha": "3746e659fc9a96b7d623e037ae7931f92ebd051c",
        "closed_date": "2026-09-14",
        "closure_evidence": RETIREMENT_REFERENCE,
        "risk_owner": "Lukas Bower",
        "risk_expiration": "",
    }
    for field, value in expected.items():
        if finding.get(field, "").strip() != value:
            raise LifecycleError(f"retirement finding {field} mismatch")
    for field, value in {
        "exception_id": RETIRED_EXCEPTION,
        "finding_id": RETIRED_FINDING,
        "severity": "P1",
        "scope": "dd30-restricted-ipc-dynamic-fault-wake",
        "risk_owner": "Lukas Bower",
        "approved_by": "Lukas Bower",
        "decision_date": "2026-09-14",
        "expiration_date": "N/A",
        "status": "RETIRED",
    }.items():
        if exception.get(field) != value:
            raise LifecycleError(f"retirement exception {field} mismatch")
    record = root / RETIREMENT_RECORD
    if (not record.is_file() or record.is_symlink()
            or '<a id="dd30-retirement"></a>' not in record.read_text()):
        raise LifecycleError(f"missing regular retirement record: {record}")
    return (
        f"historical finding retired: {RETIRED_FINDING} "
        f"(P1, {RETIREMENT_DISPOSITION}); "
        f"dynamic_fault_wake=NOT_EXECUTED; record={RETIREMENT_REFERENCE}"
    )


def validate_rust_review(root: pathlib.Path, milestone: str) -> None:
    """Check an explicitly selected historical Rust review independently of DD30.

    Archived approval files retain their original DD30 dates and hashes. Only
    the review identity and reviewed implementation govern this source check.
    Future reviews use their own normal contribution workflow.
    """
    reviews = {
        "27": (
            "DD30_M27_APPROVAL.toml",
            "7c3b82abbaf938f82f958dc40886d24fcf1c9f01",
            "Consider DD30 and Rust review signed off",
        ),
        "27a": (
            "DD30_M27A_APPROVAL.toml",
            "58140c1a5c79124a8dd7a4ff4bd547a52c8bd362", "Sign off",
        ),
    }
    if milestone not in reviews:
        raise LifecycleError("no recorded Rust review for the selected milestone")
    name, source, quote = reviews[milestone]
    path = root / "docs/audit" / name
    if not path.is_file() or path.is_symlink():
        raise LifecycleError(f"missing regular Rust review record: {path}")
    record = tomllib.loads(path.read_text())
    for key, value in {
        "schema": "cohesix.milestone-evidence-waiver/v1",
        "milestone_id": milestone, "rust_review": "APPROVED",
        "status": "APPROVED_ACTIVE", "approved_by": "Lukas Bower",
        "decision_date": "2026-09-14", "reviewed_source_commit": source,
        "approval_quote": quote,
    }.items():
        if record.get(key) != value:
            raise LifecycleError(f"Rust review {key} does not match approval")
    validate_reviewed_source(root, source)


def validate_reviewed_source(root: pathlib.Path, source: str) -> None:
    """Reject changed, missing or new implementation outside the human review."""
    paths = ("apps", "crates", "tools", "configs", "Cargo.toml", "Cargo.lock")
    changed = subprocess.run(
        ["git", "-C", str(root), "diff", "--exit-code", source, "--", *paths],
        capture_output=True, check=False,
    )
    untracked = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--others", "--exclude-standard",
         "--", *paths], capture_output=True, check=True,
    )
    if changed.returncode != 0 or untracked.stdout:
        raise LifecycleError("Rust reviewed implementation changed or is unavailable")


def validate_register(
    findings_path: pathlib.Path,
    exceptions_path: pathlib.Path,
    root: pathlib.Path,
    *,
    today: date,
) -> list[str] | None:
    """Validate active exceptions and historical retirement records separately."""
    if not findings_path.is_file():
        print(f"missing {findings_path}", file=sys.stderr)
        return None
    if not exceptions_path.is_file():
        print(f"missing {exceptions_path}", file=sys.stderr)
        return None

    with findings_path.open(newline="") as handle:
        findings_lines = [
            line
            for line in handle
            if line.strip() and not line.lstrip().startswith("#")
        ]
    if not findings_lines:
        print("findings register is empty", file=sys.stderr)
        return None

    reader = csv.DictReader(findings_lines)
    if reader.fieldnames is None:
        print("unable to parse findings header", file=sys.stderr)
        return None
    if (
        any(not field for field in reader.fieldnames)
        or len(reader.fieldnames) != len(set(reader.fieldnames))
    ):
        print("findings header contains empty or duplicate columns", file=sys.stderr)
        return None
    required_finding_columns = {
        "finding_id",
        "severity",
        "commit_sha",
        "closed_date",
        "closure_evidence",
    }
    allowed_finding_columns = {
        "finding_id",
        "severity",
        "disposition",
        "status",
        "title",
        "component",
        "file",
        "line",
        "first_observed_date",
        "last_observed_date",
        "evidence",
        "owner",
        "target_date",
        "reviewer",
        "commit_sha",
        "closed_date",
        "closure_evidence",
        "root_cause",
        "preventive_action",
        "risk_owner",
        "risk_expiration",
    }
    missing_finding_columns = sorted(
        required_finding_columns.difference(reader.fieldnames)
    )
    if missing_finding_columns:
        print(
            "findings header missing required columns: "
            + ", ".join(missing_finding_columns),
            file=sys.stderr,
        )
        return None
    unknown_finding_columns = sorted(
        set(reader.fieldnames).difference(allowed_finding_columns)
    )
    if unknown_finding_columns:
        print(
            "findings header contains unknown columns: "
            + ", ".join(unknown_finding_columns),
            file=sys.stderr,
        )
        return None
    if "disposition" in reader.fieldnames and "status" in reader.fieldnames:
        print(
            "findings header cannot contain both disposition and legacy status columns",
            file=sys.stderr,
        )
        return None
    if "disposition" in reader.fieldnames:
        disposition_field = "disposition"
    elif "status" in reader.fieldnames:
        disposition_field = "status"
    else:
        print("findings header missing disposition/status column", file=sys.stderr)
        return None

    errors = []
    findings = {}
    allowed_finding_dispositions = {
        "OPEN",
        "IN_REMEDIATION",
        "PENDING_VERIFY",
        "CLOSED_VERIFIED",
        "ACCEPTED_RISK",
        RETIREMENT_DISPOSITION,
    }
    allowed_finding_severities = {"P0", "P1", "P2", "P3"}
    for line_number, row in enumerate(reader, start=2):
        if None in row or any(value is None for value in row.values()):
            errors.append(
                f"findings register line {line_number} does not match the "
                f"{len(reader.fieldnames)}-column header"
            )
            continue
        finding_id = row.get("finding_id", "").strip().strip("`")
        disposition = row.get(disposition_field, "").strip().upper()
        severity = row.get("severity", "").strip().upper()
        if not finding_id:
            errors.append("findings register contains an empty finding_id")
            continue
        if finding_id in findings:
            errors.append(f"{finding_id}: duplicate finding_id")
            continue
        if disposition not in allowed_finding_dispositions:
            errors.append(
                f"{finding_id}: unknown disposition '{disposition or '<empty>'}'"
            )
        if severity not in allowed_finding_severities:
            errors.append(f"{finding_id}: unknown severity '{severity or '<empty>'}'")
        if finding_id == RETIRED_FINDING and severity != "P1":
            errors.append(f"{finding_id}: retirement cannot change P1 severity")
        if (disposition == RETIREMENT_DISPOSITION
                and finding_id != RETIRED_FINDING):
            errors.append(
                f"{finding_id}: {RETIREMENT_DISPOSITION} is reserved for DD30"
            )
        if disposition == "CLOSED_VERIFIED" and severity in {"P0", "P1", "P2"}:
            commit_sha = row.get("commit_sha", "").strip()
            closed_date = row.get("closed_date", "").strip()
            closure_evidence = row.get("closure_evidence", "").strip()
            if re.fullmatch(r"[0-9a-fA-F]{40}", commit_sha) is None:
                errors.append(
                    f"{finding_id}: CLOSED_VERIFIED requires a full 40-hex commit_sha"
                )
            try:
                date.fromisoformat(closed_date)
            except ValueError:
                errors.append(
                    f"{finding_id}: CLOSED_VERIFIED requires a valid closed_date"
                )
            if not closure_evidence:
                errors.append(
                    f"{finding_id}: CLOSED_VERIFIED requires closure_evidence"
                )
        findings[finding_id] = {**row, "disposition": disposition, "severity": severity}

    lines = exceptions_path.read_text().splitlines()
    table_rows = []
    for line_number, line in enumerate(lines, start=1):
        stripped = line.strip()
        if not stripped.startswith("|"):
            continue
        if not stripped.endswith("|"):
            errors.append(
                f"exceptions register line {line_number} expected 11 cells, "
                "but the row has no closing pipe"
            )
            continue
        cells = [cell.strip() for cell in stripped.strip("|").split("|")]
        if len(cells) != 11:
            errors.append(
                f"exceptions register line {line_number} expected 11 cells, "
                f"found {len(cells)}"
            )
            continue
        table_rows.append(cells)

    if not table_rows:
        print("exceptions register table not found", file=sys.stderr)
        return None

    expected_exception_header = [
        "Exception ID",
        "Related Finding",
        "Severity",
        "Scope",
        "Rationale",
        "Compensating Controls",
        "Risk Owner",
        "Approved By",
        "Decision Date",
        "Expiration Date",
        "Status",
    ]
    header_count = 0
    separator_count = 0
    data_rows = []
    for cells in table_rows:
        if cells == expected_exception_header:
            header_count += 1
        elif all(re.fullmatch(r":?-{3,}:?", cell) is not None for cell in cells):
            separator_count += 1
        else:
            data_rows.append(cells)
    if header_count != 1:
        errors.append(
            "exceptions register requires exactly one canonical header, "
            f"found {header_count}"
        )
    if separator_count != 1:
        errors.append(
            "exceptions register requires exactly one separator row, "
            f"found {separator_count}"
        )
    table_rows = data_rows

    active_exception_findings = set()
    retirements: list[str] = []
    retired_findings: set[str] = set()
    exception_ids = set()
    allowed_statuses = {
        "PROPOSED", "APPROVED_ACTIVE", "EXPIRED", "REVOKED", "CLOSED", "RETIRED",
    }
    for cells in table_rows:
        first = cells[0]

        exception_id = first.strip("`")
        related_finding = cells[1].strip("`")
        severity = cells[2].strip("`").upper()
        scope = cells[3].strip("`").strip()
        rationale = cells[4].strip("`").strip()
        controls = cells[5].strip("`").strip()
        risk_owner = cells[6].strip("`").strip()
        approved_by = cells[7].strip("`").strip()
        decision = cells[8].strip("`").strip()
        expiration = cells[9].strip("`")
        status = cells[10].strip("`").upper()

        if exception_id == "None" and related_finding == "N/A":
            continue
        if not exception_id:
            errors.append("exceptions register contains an empty exception ID")
            continue
        if exception_id in exception_ids:
            errors.append(f"{exception_id}: duplicate exception ID")
            continue
        exception_ids.add(exception_id)
        if exception_id == RETIRED_EXCEPTION and (
            related_finding != RETIRED_FINDING or severity != "P1"
        ):
            errors.append(
                f"{exception_id}: reserved for {RETIRED_FINDING} severity P1"
            )

        finding = findings.get(related_finding)
        if finding is None:
            errors.append(
                f"{exception_id}: related finding "
                f"{related_finding or '<missing>'} does not exist"
            )
        elif severity != finding["severity"]:
            errors.append(
                f"{exception_id}: severity {severity or '<empty>'} does not match "
                f"{related_finding} severity {finding['severity'] or '<empty>'}"
            )

        if status == "RETIRED":
            if finding is None:
                continue
            try:
                if not rationale or not controls:
                    raise LifecycleError("retirement requires rationale and controls")
                retirements.append(validate_retirement(root, finding, {
                    "exception_id": exception_id, "finding_id": related_finding,
                    "severity": severity, "scope": scope, "risk_owner": risk_owner,
                    "approved_by": approved_by, "decision_date": decision,
                    "expiration_date": expiration, "status": status,
                }))
                retired_findings.add(related_finding)
            except LifecycleError as error:
                errors.append(f"{exception_id}: {error}")
            continue

        for label, value in [
            ("scope", scope),
            ("rationale", rationale),
            ("compensating controls", controls),
            ("risk owner", risk_owner),
            ("approved by", approved_by),
            ("decision date", decision),
            ("expiration date", expiration),
        ]:
            if not value or value.upper() == "N/A":
                errors.append(f"{exception_id}: missing {label}")

        try:
            decision_date = date.fromisoformat(decision)
        except ValueError:
            decision_date = None
            errors.append(f"{exception_id}: invalid decision date '{decision}'")
        try:
            expiry_date = date.fromisoformat(expiration)
        except ValueError:
            expiry_date = None
            errors.append(f"{exception_id}: invalid expiration date '{expiration}'")
        if (
            decision_date is not None
            and expiry_date is not None
            and expiry_date < decision_date
        ):
            errors.append(f"{exception_id}: expiration date precedes decision date")

        if status not in allowed_statuses:
            errors.append(f"{exception_id}: unknown status '{status or '<empty>'}'")
            continue

        if status == "APPROVED_ACTIVE":
            if related_finding in active_exception_findings:
                errors.append(
                    f"{exception_id}: duplicate APPROVED_ACTIVE exception "
                    f"for {related_finding}"
                )
            else:
                active_exception_findings.add(related_finding)
            if severity in {"P0", "P1"}:
                errors.append(
                    f"{exception_id}: {severity} findings cannot be accepted "
                    "as residual risk"
                )
            if finding is not None and finding["disposition"] != "ACCEPTED_RISK":
                errors.append(
                    f"{exception_id}: APPROVED_ACTIVE requires {related_finding} "
                    f"to be ACCEPTED_RISK, found {finding['disposition'] or '<empty>'}"
                )
            if expiry_date is not None and expiry_date < today:
                errors.append(f"{exception_id}: expired on {expiry_date.isoformat()}")
        elif status == "EXPIRED":
            errors.append(f"{exception_id}: status is EXPIRED")
        elif status == "CLOSED":
            if finding is not None and finding["disposition"] != "CLOSED_VERIFIED":
                errors.append(
                    f"{exception_id}: CLOSED requires {related_finding} "
                    "to be CLOSED_VERIFIED, found "
                    f"{finding['disposition'] or '<empty>'}"
                )

    for finding_id, finding in findings.items():
        if (finding["disposition"] == RETIREMENT_DISPOSITION
                and finding_id not in retired_findings):
            errors.append(
                f"{finding_id}: {RETIREMENT_DISPOSITION} requires a matching "
                "RETIRED exception"
            )
        if (
            finding["disposition"] == "ACCEPTED_RISK"
            and finding_id not in active_exception_findings
        ):
            errors.append(
                f"{finding_id}: ACCEPTED_RISK requires a matching "
                "APPROVED_ACTIVE exception"
            )

    if errors:
        print("exceptions register validation failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return None

    return retirements


def check_blocking_findings(path: pathlib.Path) -> bool | None:
    """Return whether DD30's retirement needs register validation.

    None is failure; False is an ordinary passing blocker predicate. True
    requires the recorded retirement, never a release or milestone selection.
    """
    if not path.is_file():
        print(f"missing {path}", file=sys.stderr)
        return None

    with path.open(newline="") as handle:
        lines = [
            line for line in handle
            if line.strip() and not line.lstrip().startswith("#")
        ]

    if not lines:
        print("findings register is empty", file=sys.stderr)
        return None

    reader = csv.DictReader(lines)
    if reader.fieldnames is None:
        print("unable to parse findings header", file=sys.stderr)
        return None

    required = {"finding_id", "severity"}
    missing = sorted(required.difference(reader.fieldnames))
    if missing:
        print(
            f"findings header missing required columns: {', '.join(missing)}",
            file=sys.stderr,
        )
        return None

    if "disposition" in reader.fieldnames:
        disposition_field = "disposition"
    elif "status" in reader.fieldnames:
        disposition_field = "status"
    else:
        print("findings header missing disposition/status column", file=sys.stderr)
        return None

    blocking = []
    retirement_requested = False

    for row in reader:
        severity = row.get("severity", "").strip().upper()
        disposition = row.get(disposition_field, "").strip().upper()
        finding_id = row.get("finding_id", "UNKNOWN")
        if finding_id == RETIRED_FINDING and severity != "P1":
            blocking.append((finding_id, severity, disposition))
            continue
        # Remediation dates schedule work; they never authorize an open P0/P1.
        if severity in {"P0", "P1"} and disposition != "CLOSED_VERIFIED":
            if (
                finding_id == RETIRED_FINDING
                and severity == "P1"
                and disposition == RETIREMENT_DISPOSITION
            ):
                retirement_requested = True
            else:
                blocking.append((finding_id, severity, disposition))

    if blocking:
        print("blocking findings remain open:", file=sys.stderr)
        for finding_id, severity, disposition in blocking:
            print(f"  - {finding_id} ({severity}, {disposition})", file=sys.stderr)
        return None

    return retirement_requested


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode", choices=("blockers", "register", "rust-review"), required=True,
    )
    parser.add_argument("--root", type=pathlib.Path, required=True)
    parser.add_argument("--findings", type=pathlib.Path)
    parser.add_argument("--exceptions", type=pathlib.Path)
    parser.add_argument("--milestone", default="")
    args = parser.parse_args()
    try:
        if args.mode == "rust-review":
            validate_rust_review(args.root, args.milestone)
            print(f"Rust review source check passed: milestone={args.milestone}")
            return 0
        if args.milestone:
            raise LifecycleError("milestone selection applies only to rust-review")
        if args.findings is None or args.exceptions is None:
            raise LifecycleError("finding and exception registers are required")
        if args.mode == "blockers":
            requested = check_blocking_findings(args.findings)
            if requested is None:
                return 1
            if not requested:
                print("blocking findings gate passed")
                return 0
        retirements = validate_register(
            args.findings, args.exceptions, args.root, today=date.today(),
        )
        if retirements is None:
            return 1
        if args.mode == "blockers" and len(retirements) != 1:
            print("DD30 requires one validated retirement record", file=sys.stderr)
            return 1
        for retirement in retirements:
            print(retirement)
        if args.mode == "blockers":
            print("blocking findings gate passed")
        else:
            print("exceptions register gate passed")
        return 0
    except (LifecycleError, OSError, tomllib.TOMLDecodeError,
            subprocess.CalledProcessError) as error:
        print(f"due-diligence lifecycle validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
