# Author: Lukas Bower
# Purpose: Validate finding lifecycles and the scoped DD30 release waiver.
# Copyright 2026 Lukas Bower

"""Keep verified remediation distinct from a scoped release-owner decision."""

from __future__ import annotations

import argparse
import csv
from dataclasses import dataclass
from datetime import date
import hashlib
import pathlib
import re
import subprocess
import sys
import tomllib


WAIVER_FINDING = "DD-2026-0030"
WAIVER_EXCEPTION = "EX-2026-0030"
WAIVER_RELEASE = "1.0.0-beta"
WAIVER_SCOPE = "dd30-restricted-ipc-dynamic-fault-wake"
WAIVER_RECORD = pathlib.Path("docs/audit/DD30_RELEASE_WAIVER.toml")
WAIVER_APPROVAL = (
    "There is no debugger, dd30 was a one-off issue. "
    "Mark it as a pass, we are ready for release"
)
WAIVER_SOURCE = "6d7c16e4a0f5d89c32a4fa0f815c2bfe6babc065"
PROTECTED_FILES = frozenset({
    "apps/root-task/src/console/mod.rs",
    "apps/root-task/src/hal/console_network.rs",
    "apps/root-task/src/hal/critical_tcb.rs",
    "apps/root-task/src/hal/driver_task.rs",
    "apps/root-task/src/hal/mod.rs",
    "apps/root-task/src/hal/worker_task.rs",
    "apps/root-task/src/kernel.rs",
    "apps/root-task/src/sel4.rs",
    "apps/root-task/src/sel4/syscall.rs",
    "crates/sel4-sys/src/lib.rs",
})


class LifecycleError(ValueError):
    """A lifecycle record cannot authorize its requested disposition."""


@dataclass(frozen=True)
class WaiverAdmission:
    """Content binding for an owner decision; never a target-test result."""

    sha256: str
    path: pathlib.Path

    def describe(self, *, admitted: bool) -> str:
        action = "admitted" if admitted else "validated (not release admission)"
        return (
            f"release-owner evidence waiver {action}: "
            f"DD_RELEASE_ID={WAIVER_RELEASE} finding={WAIVER_FINDING} "
            f"exception={WAIVER_EXCEPTION} scope={WAIVER_SCOPE} "
            f"sha256={self.sha256} record={self.path} "
            "dynamic_fault_wake=NOT_EXECUTED"
        )


def reviewed_source_bytes(root: pathlib.Path, relative: str) -> bytes:
    """Resolve the approval's immutable Git object, never the working tree."""
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "show", f"{WAIVER_SOURCE}:{relative}"],
            check=True,
            capture_output=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise LifecycleError(
            f"cannot resolve reviewed IPC source: {relative} at {WAIVER_SOURCE}"
        ) from error
    return result.stdout


def validate_waiver(
    root: pathlib.Path,
    finding: dict[str, str],
    exception: dict[str, str],
    *,
    today: date,
) -> WaiverAdmission:
    """Check the one approved waiver with an injected evaluation date.

    The CLI supplies the actual date. No environment or CLI option can replace
    it. Register validity alone does not select release acceptance.
    """
    path = root / WAIVER_RECORD
    if not path.is_file() or path.is_symlink():
        raise LifecycleError(f"missing regular release waiver record: {path}")
    payload = path.read_bytes()
    try:
        record = tomllib.loads(payload.decode("utf-8"))
    except (UnicodeError, tomllib.TOMLDecodeError) as error:
        raise LifecycleError(f"invalid release waiver record: {error}") from error
    required = {
        "schema", "exception_id", "finding_id", "release_id", "scope_id",
        "severity", "disposition", "status", "risk_owner", "approved_by",
        "decision_date", "expiration_date", "reviewed_source_commit",
        "approval_quote", "protected_files",
    }
    if set(record) != required:
        raise LifecycleError("release waiver record has missing or unknown fields")
    expected = {
        "schema": "cohesix.release-evidence-waiver/v1",
        "exception_id": WAIVER_EXCEPTION,
        "finding_id": WAIVER_FINDING,
        "release_id": WAIVER_RELEASE,
        "scope_id": WAIVER_SCOPE,
        "severity": "P1",
        "disposition": "ACCEPTED_RISK",
        "status": "APPROVED_ACTIVE",
        "risk_owner": "Lukas Bower",
        "approved_by": "Lukas Bower",
        "reviewed_source_commit": WAIVER_SOURCE,
        "approval_quote": WAIVER_APPROVAL,
    }
    for field, value in expected.items():
        if record[field] != value:
            raise LifecycleError(f"release waiver {field} does not match approval")
    for field in ("decision_date", "expiration_date"):
        if not isinstance(record[field], str):
            raise LifecycleError(f"release waiver {field} must be an ISO date string")
    try:
        decision = date.fromisoformat(record["decision_date"])
        expiration = date.fromisoformat(record["expiration_date"])
    except ValueError as error:
        raise LifecycleError("release waiver has an invalid date") from error
    if decision != date(2026, 9, 13):
        raise LifecycleError("release waiver decision date does not match approval")
    if not decision <= expiration <= date(2026, 10, 13):
        raise LifecycleError(
            "release waiver expiration exceeds its release-specific bound"
        )
    if today < decision:
        raise LifecycleError("release waiver decision is not yet effective")
    if today > expiration:
        raise LifecycleError(f"release waiver expired on {expiration.isoformat()}")
    required_finding = {
        "finding_id": WAIVER_FINDING,
        "severity": "P1",
        "disposition": "ACCEPTED_RISK",
        "risk_owner": record["risk_owner"],
        "risk_expiration": record["expiration_date"],
    }
    for field, value in required_finding.items():
        if finding.get(field, "").strip() != value:
            raise LifecycleError(f"release waiver finding {field} mismatch")
    required_exception = {
        "exception_id": WAIVER_EXCEPTION,
        "finding_id": WAIVER_FINDING,
        "severity": "P1",
        "scope": f"release={WAIVER_RELEASE}; scope={WAIVER_SCOPE}",
        "risk_owner": record["risk_owner"],
        "approved_by": record["approved_by"],
        "decision_date": record["decision_date"],
        "expiration_date": record["expiration_date"],
        "status": "APPROVED_ACTIVE",
    }
    for field, value in required_exception.items():
        if exception.get(field) != value:
            raise LifecycleError(f"release waiver exception {field} mismatch")
    files = record["protected_files"]
    if not isinstance(files, list) or len(files) != len(PROTECTED_FILES):
        raise LifecycleError("release waiver requires exactly the protected IPC files")
    seen: set[str] = set()
    for entry in files:
        if not isinstance(entry, dict) or set(entry) != {"path", "sha256"}:
            raise LifecycleError("invalid protected IPC file record")
        relative = entry["path"]
        digest = entry["sha256"]
        if not isinstance(relative, str) or relative not in PROTECTED_FILES:
            raise LifecycleError("unknown protected IPC path")
        if relative in seen:
            raise LifecycleError("duplicate protected IPC path")
        seen.add(relative)
        if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
            raise LifecycleError("invalid protected IPC digest")
        reviewed = hashlib.sha256(reviewed_source_bytes(root, relative)).hexdigest()
        if digest != reviewed:
            raise LifecycleError(
                f"protected IPC digest differs from approval: {relative}"
            )
        source = root / relative
        if not source.is_file() or source.is_symlink():
            raise LifecycleError(f"missing regular protected IPC source: {relative}")
        if hashlib.sha256(source.read_bytes()).hexdigest() != digest:
            if relative != "apps/root-task/src/sel4.rs":
                raise LifecycleError(f"protected IPC source changed: {relative}")
            # This is a separate exact owner-approved host-only successor,
            # never a refreshed version of the original 6d7 approval.
            from release_stage5_acceptance import (
                AcceptanceError,
                validate_host_successor,
            )

            try:
                successor = validate_host_successor(root, today=today)
            except (AcceptanceError, OSError, ValueError,
                    subprocess.CalledProcessError) as error:
                raise LifecycleError(
                    f"protected IPC source changed: {relative}; "
                    f"host successor rejected: {error}"
                ) from error
            print(
                "DD30 exact host-only successor validated (not target proof): "
                f"approval_sha256={successor['approval_sha256']} "
                f"original_source={WAIVER_SOURCE} dynamic_fault_wake=NOT_EXECUTED"
            )
    if seen != PROTECTED_FILES:
        raise LifecycleError("incomplete protected IPC source coverage")
    return WaiverAdmission(hashlib.sha256(payload).hexdigest(), path)


def validate_register(
    findings_path: pathlib.Path,
    exceptions_path: pathlib.Path,
    root: pathlib.Path,
    *,
    today: date,
) -> list[WaiverAdmission] | None:
    """Validate every finding/exception lifecycle before any waiver admission."""
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
        if finding_id == WAIVER_FINDING and severity != "P1":
            errors.append(f"{finding_id}: the release waiver cannot change P1 severity")
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
    admissions: list[WaiverAdmission] = []
    exception_ids = set()
    allowed_statuses = {"PROPOSED", "APPROVED_ACTIVE", "EXPIRED", "REVOKED", "CLOSED"}
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
        if exception_id == WAIVER_EXCEPTION and (
            related_finding != WAIVER_FINDING or severity != "P1"
        ):
            errors.append(
                f"{exception_id}: reserved for {WAIVER_FINDING} severity P1"
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
                if (
                    exception_id == WAIVER_EXCEPTION
                    and related_finding == WAIVER_FINDING
                    and severity == "P1"
                    and finding is not None
                ):
                    try:
                        admissions.append(validate_waiver(
                            root,
                            finding,
                            {
                                "exception_id": exception_id,
                                "finding_id": related_finding,
                                "severity": severity,
                                "scope": scope,
                                "risk_owner": risk_owner,
                                "approved_by": approved_by,
                                "decision_date": decision,
                                "expiration_date": expiration,
                                "status": status,
                            },
                            today=today,
                        ))
                    except LifecycleError as error:
                        errors.append(f"{exception_id}: {error}")
                else:
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

    return admissions


def check_blocking_findings(path: pathlib.Path) -> bool | None:
    """Return whether the sole scoped waiver needs full validation.

    None is failure; False is an ordinary passing blocker predicate. True
    requires the shared register validator plus explicit release selection.
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
    waiver_requested = False

    for row in reader:
        severity = row.get("severity", "").strip().upper()
        disposition = row.get(disposition_field, "").strip().upper()
        finding_id = row.get("finding_id", "UNKNOWN")
        if finding_id == WAIVER_FINDING and severity != "P1":
            blocking.append((finding_id, severity, disposition))
            continue
        # Remediation dates schedule work; they never authorize an open P0/P1.
        if severity in {"P0", "P1"} and disposition != "CLOSED_VERIFIED":
            if (
                finding_id == WAIVER_FINDING
                and severity == "P1"
                and disposition == "ACCEPTED_RISK"
            ):
                waiver_requested = True
            else:
                blocking.append((finding_id, severity, disposition))

    if blocking:
        print("blocking findings remain open:", file=sys.stderr)
        for finding_id, severity, disposition in blocking:
            print(f"  - {finding_id} ({severity}, {disposition})", file=sys.stderr)
        return None

    return waiver_requested


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("blockers", "register"), required=True)
    parser.add_argument("--root", type=pathlib.Path, required=True)
    parser.add_argument("--findings", type=pathlib.Path, required=True)
    parser.add_argument("--exceptions", type=pathlib.Path, required=True)
    parser.add_argument("--release", default="")
    args = parser.parse_args()
    try:
        if args.mode == "blockers":
            requested = check_blocking_findings(args.findings)
            if requested is None:
                return 1
            if not requested:
                print("blocking findings gate passed")
                return 0
        admissions = validate_register(
            args.findings, args.exceptions, args.root, today=date.today()
        )
        if admissions is None:
            return 1
        if args.mode == "blockers":
            if len(admissions) != 1:
                print("DD30 requires one validated release waiver", file=sys.stderr)
                return 1
            if args.release != WAIVER_RELEASE:
                print(
                    f"blocking finding {WAIVER_FINDING} (P1, ACCEPTED_RISK): "
                    f"requires explicit DD_RELEASE_ID={WAIVER_RELEASE}",
                    file=sys.stderr,
                )
                return 1
            print(admissions[0].describe(admitted=True))
            print("blocking findings gate passed")
        else:
            for admission in admissions:
                print(admission.describe(admitted=False))
            print("exceptions register gate passed")
        return 0
    except (LifecycleError, OSError) as error:
        print(f"due-diligence lifecycle validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
