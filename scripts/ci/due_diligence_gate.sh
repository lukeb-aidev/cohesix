#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the Cohesix due-diligence gate checks, record evidence logs, and fail on assurance regressions.
# Copyright 2026 Lukas Bower
#
# Environment:
#   DD_GATE_LOG_DIR            Override log output root (default: out/audit/gate/<utc-timestamp>)
#   DD_SKIP_TEST_PLAN_CHECK=1  Mark test-plan hash check as incomplete (run still fails)
#   DD_SKIP_REGRESSION_BATCH=1 Mark regression batch check as incomplete (run still fails)
#   DD_SKIP_CARGO_AUDIT=1      Mark cargo-audit as incomplete (run still fails)
#   DD_SKIP_CARGO_DENY=1       Mark cargo-deny advisories check as incomplete (run still fails)
#   DD_REUSE_REGRESSION_BATCH_FROM
#                                Validate a prior full Stage 03 regression log root
#                                instead of rerunning the batch. Used only by
#                                scripts/ci/test_plan_stage_05_due_diligence.sh.
#   DD_REGRESSION_GROUPS        Full due-diligence requires all regression groups.
#                                Subsets are incomplete and fail the gate.
#   DD_REGRESSION_READY_TIMEOUT  Override run_regression_batch READY_TIMEOUT (default: 900)
#   DD_REGRESSION_PORT_TIMEOUT   Override run_regression_batch PORT_TIMEOUT (default: 60)
#   DD_REGRESSION_AUTH_TIMEOUT   Override run_regression_batch AUTH_READY_TIMEOUT (default: 120)
#   DD_REGRESSION_QUIT_TIMEOUT   Override run_regression_batch QUIT_CLOSE_TIMEOUT (default: 60)

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"

run_id=$(date -u +"%Y%m%dT%H%M%SZ")
log_root="${DD_GATE_LOG_DIR:-${repo_root}/out/audit/gate/${run_id}}"
dd_regression_ready_timeout="${DD_REGRESSION_READY_TIMEOUT:-900}"
dd_regression_port_timeout="${DD_REGRESSION_PORT_TIMEOUT:-60}"
dd_regression_auth_timeout="${DD_REGRESSION_AUTH_TIMEOUT:-120}"
dd_regression_quit_timeout="${DD_REGRESSION_QUIT_TIMEOUT:-60}"
dd_reuse_regression_batch_from="${DD_REUSE_REGRESSION_BATCH_FROM:-}"
dd_regression_groups="${DD_REGRESSION_GROUPS:-${COHSH_BATCH_GROUPS:-all}}"

declare -a failures=()
declare -a incomplete_steps=()

run_step() {
  local name="$1"
  shift
  local log_file="${log_root}/${name}.log"
  printf "\n[dd-gate] START %s\n" "$name"
  printf "[dd-gate] CMD   %s\n" "$*"
  if "$@" >"$log_file" 2>&1; then
    printf "[dd-gate] PASS  %s (log: %s)\n" "$name" "$log_file"
  else
    printf "[dd-gate] FAIL  %s (log: %s)\n" "$name" "$log_file"
    tail -n 40 "$log_file" >&2 || true
    failures+=("$name")
  fi
}

mark_incomplete_step() {
  local name="$1"
  local reason="$2"
  printf "\n[dd-gate] INCOMPLETE %s\n" "$name"
  printf "[dd-gate] REASON %s\n" "$reason"
  incomplete_steps+=("$name")
  failures+=("INCOMPLETE:${name}")
}

check_required_audit_assets() {
  local -a required_paths=(
    "docs/audit/findings.csv"
    "docs/audit/BLOCKERS.md"
    "docs/audit/CONTROL_TRACEABILITY.md"
    "docs/audit/EXCEPTIONS.md"
    "docs/audit/rust_risk_baseline.toml"
    "docs/audit/checklists/ARCHITECTURE_CHECKLIST.md"
    "docs/audit/checklists/SECURITY_CHECKLIST.md"
    "docs/audit/checklists/RELEASE_EVIDENCE_CHECKLIST.md"
  )
  local missing=0
  local path
  for path in "${required_paths[@]}"; do
    if [[ ! -f "$path" ]]; then
      printf "missing required audit asset: %s\n" "$path" >&2
      missing=1
    fi
  done
  return "$missing"
}

check_rust_risk_ratchet() {
  scripts/ci/rust_risk_gate.sh \
    --root "$repo_root" \
    --baseline docs/audit/rust_risk_baseline.toml
}

scan_hardcoded_secrets() {
  local matches_file
  matches_file="$(mktemp -t dd-secret-scan.XXXXXX)"

  rg -n '"changeme"' apps/root-task/src apps/hive-gateway/src apps/coh/src apps/cohsh/src -g '*.rs' >"$matches_file"
  local rg_status=$?
  if [[ $rg_status -eq 1 ]]; then
    rm -f "$matches_file"
    return 0
  fi
  if [[ $rg_status -ne 0 ]]; then
    rm -f "$matches_file"
    printf "secret scan failed due to rg error: %s\n" "$rg_status" >&2
    return 1
  fi

  python3 - "$matches_file" <<'PY'
from datetime import date
import csv
import pathlib
import sys

matches_path = pathlib.Path(sys.argv[1])
findings_path = pathlib.Path("docs/audit/findings.csv")
if not findings_path.is_file():
    print("missing docs/audit/findings.csv", file=sys.stderr)
    sys.exit(1)

lines = [line for line in findings_path.read_text().splitlines() if line.strip() and not line.lstrip().startswith("#")]
reader = csv.DictReader(lines)
if reader.fieldnames is None:
    print("unable to parse findings header", file=sys.stderr)
    sys.exit(1)

if "disposition" in reader.fieldnames:
    disposition_field = "disposition"
elif "status" in reader.fieldnames:
    disposition_field = "status"
else:
    print("findings header missing disposition/status column", file=sys.stderr)
    sys.exit(1)

rows = list(reader)
today = date.today()

def parse_date(value: str):
    value = (value or "").strip()
    if not value:
        return None
    try:
        return date.fromisoformat(value)
    except ValueError:
        return None

def row_matches_file(row_file: str, matched_file: str) -> bool:
    row_norm = row_file.replace("\\", "/").strip()
    match_norm = matched_file.replace("\\", "/").strip()
    if not row_norm or not match_norm:
        return False
    return row_norm == match_norm or row_norm.endswith("/" + match_norm) or row_norm.endswith(match_norm)

match_lines = [line.strip() for line in matches_path.read_text().splitlines() if line.strip()]
matched_files = sorted({line.split(":", 1)[0] for line in match_lines})

untracked = []
overdue = []
regression = []
deferred = []

for matched_file in matched_files:
    related = [
        row
        for row in rows
        if row_matches_file(row.get("file", ""), matched_file)
        and row.get("severity", "").strip().upper() in {"P0", "P1"}
    ]
    if not related:
        untracked.append(matched_file)
        continue

    open_related = [row for row in related if row.get(disposition_field, "").strip().upper() != "CLOSED_VERIFIED"]
    if not open_related:
        regression.append(matched_file)
        continue

    due_dates = [parse_date(row.get("target_date", "")) for row in open_related]
    if due_dates and all(due is not None and due > today for due in due_dates):
        deferred.append((matched_file, min(due_dates)))
    else:
        overdue.append(matched_file)

if untracked or overdue or regression:
    print("hardcoded secret scan failures:", file=sys.stderr)
    if untracked:
        print("  untracked files (no P0/P1 finding):", file=sys.stderr)
        for path in untracked:
            print(f"    - {path}", file=sys.stderr)
    if overdue:
        print("  files with overdue/missing P0/P1 finding target_date:", file=sys.stderr)
        for path in overdue:
            print(f"    - {path}", file=sys.stderr)
    if regression:
        print("  files with CLOSED_VERIFIED finding but matching secret literal still present:", file=sys.stderr)
        for path in regression:
            print(f"    - {path}", file=sys.stderr)
    print("\nmatched lines:", file=sys.stderr)
    for line in match_lines:
        print(f"  {line}", file=sys.stderr)
    sys.exit(1)

if deferred:
    print("hardcoded-secret-scan deferred for tracked findings with future target dates:")
    for path, due in deferred:
        print(f"  - {path} (target_date={due.isoformat()})")
sys.exit(0)
PY
  local py_status=$?
  rm -f "$matches_file"
  return "$py_status"
}

check_blocking_findings() {
  python3 <<'PY'
import csv
from datetime import date
import pathlib
import sys

path = pathlib.Path("docs/audit/findings.csv")
if not path.is_file():
    print("missing docs/audit/findings.csv", file=sys.stderr)
    sys.exit(1)

with path.open(newline="") as handle:
    lines = [line for line in handle if line.strip() and not line.lstrip().startswith("#")]

if not lines:
    print("findings register is empty", file=sys.stderr)
    sys.exit(1)

reader = csv.DictReader(lines)
if reader.fieldnames is None:
    print("unable to parse findings header", file=sys.stderr)
    sys.exit(1)

required = {"finding_id", "severity"}
missing = sorted(required.difference(reader.fieldnames))
if missing:
    print(f"findings header missing required columns: {', '.join(missing)}", file=sys.stderr)
    sys.exit(1)

if "disposition" in reader.fieldnames:
    disposition_field = "disposition"
elif "status" in reader.fieldnames:
    disposition_field = "status"
else:
    print("findings header missing disposition/status column", file=sys.stderr)
    sys.exit(1)

blocking = []
deferred = []
today = date.today()

def parse_target_date(raw: str):
    value = (raw or "").strip()
    if not value:
        return None
    try:
        return date.fromisoformat(value)
    except ValueError:
        return None

for row in reader:
    severity = row.get("severity", "").strip().upper()
    disposition = row.get(disposition_field, "").strip().upper()
    finding_id = row.get("finding_id", "UNKNOWN")
    target_date = parse_target_date(row.get("target_date", ""))
    if severity == "P0" and disposition != "CLOSED_VERIFIED":
        blocking.append((finding_id, severity, disposition))
        continue
    if severity == "P1" and disposition != "CLOSED_VERIFIED":
        if target_date is not None and target_date > today:
            deferred.append((finding_id, severity, disposition, target_date.isoformat()))
        else:
            blocking.append((finding_id, severity, disposition))

if blocking:
    print("blocking findings remain open:", file=sys.stderr)
    for finding_id, severity, disposition in blocking:
        print(f"  - {finding_id} ({severity}, {disposition})", file=sys.stderr)
    sys.exit(1)

if deferred:
    print("deferred findings (target_date in future):")
    for finding_id, severity, disposition, target_date in deferred:
        print(f"  - {finding_id} ({severity}, {disposition}, target_date={target_date})")

print("blocking findings gate passed")
PY
}

check_exceptions_register() {
  local findings_path="${1:-docs/audit/findings.csv}"
  local exceptions_path="${2:-docs/audit/EXCEPTIONS.md}"
  python3 - "$findings_path" "$exceptions_path" <<'PY'
import csv
from datetime import date
import pathlib
import re
import sys

findings_path = pathlib.Path(sys.argv[1])
exceptions_path = pathlib.Path(sys.argv[2])
if not findings_path.is_file():
    print(f"missing {findings_path}", file=sys.stderr)
    sys.exit(1)
if not exceptions_path.is_file():
    print(f"missing {exceptions_path}", file=sys.stderr)
    sys.exit(1)

with findings_path.open(newline="") as handle:
    findings_lines = [
        line
        for line in handle
        if line.strip() and not line.lstrip().startswith("#")
    ]
if not findings_lines:
    print("findings register is empty", file=sys.stderr)
    sys.exit(1)

reader = csv.DictReader(findings_lines)
if reader.fieldnames is None:
    print("unable to parse findings header", file=sys.stderr)
    sys.exit(1)
if (
    any(not field for field in reader.fieldnames)
    or len(reader.fieldnames) != len(set(reader.fieldnames))
):
    print("findings header contains empty or duplicate columns", file=sys.stderr)
    sys.exit(1)
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
missing_finding_columns = sorted(required_finding_columns.difference(reader.fieldnames))
if missing_finding_columns:
    print(
        "findings header missing required columns: "
        + ", ".join(missing_finding_columns),
        file=sys.stderr,
    )
    sys.exit(1)
unknown_finding_columns = sorted(set(reader.fieldnames).difference(allowed_finding_columns))
if unknown_finding_columns:
    print(
        "findings header contains unknown columns: "
        + ", ".join(unknown_finding_columns),
        file=sys.stderr,
    )
    sys.exit(1)
if "disposition" in reader.fieldnames and "status" in reader.fieldnames:
    print(
        "findings header cannot contain both disposition and legacy status columns",
        file=sys.stderr,
    )
    sys.exit(1)
if "disposition" in reader.fieldnames:
    disposition_field = "disposition"
elif "status" in reader.fieldnames:
    disposition_field = "status"
else:
    print("findings header missing disposition/status column", file=sys.stderr)
    sys.exit(1)

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
    findings[finding_id] = {
        "disposition": disposition,
        "severity": severity,
    }

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
    sys.exit(1)

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
        f"exceptions register requires exactly one canonical header, found {header_count}"
    )
if separator_count != 1:
    errors.append(
        f"exceptions register requires exactly one separator row, found {separator_count}"
    )
table_rows = data_rows

today = date.today()
active_exception_findings = set()
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

    finding = findings.get(related_finding)
    if finding is None:
        errors.append(
            f"{exception_id}: related finding {related_finding or '<missing>'} does not exist"
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
    if decision_date is not None and expiry_date is not None and expiry_date < decision_date:
        errors.append(f"{exception_id}: expiration date precedes decision date")

    if status not in allowed_statuses:
        errors.append(f"{exception_id}: unknown status '{status or '<empty>'}'")
        continue

    if status == "APPROVED_ACTIVE":
        if related_finding in active_exception_findings:
            errors.append(
                f"{exception_id}: duplicate APPROVED_ACTIVE exception for {related_finding}"
            )
        else:
            active_exception_findings.add(related_finding)
        if severity in {"P0", "P1"}:
            errors.append(f"{exception_id}: {severity} findings cannot be accepted as residual risk")
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
                f"to be CLOSED_VERIFIED, found {finding['disposition'] or '<empty>'}"
            )

for finding_id, finding in findings.items():
    if finding["disposition"] == "ACCEPTED_RISK" and finding_id not in active_exception_findings:
        errors.append(
            f"{finding_id}: ACCEPTED_RISK requires a matching APPROVED_ACTIVE exception"
        )

if errors:
    print("exceptions register validation failed:", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

print("exceptions register gate passed")
PY
}

check_reused_regression_batch() {
  local reuse_root="$1"
  python3 - "$reuse_root" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
if not root.is_dir():
    print(f"missing regression log root: {root}", file=sys.stderr)
    sys.exit(1)

summary = root / "summary.log"
if summary.is_file():
    text = summary.read_text(errors="ignore")
    if "INFO target=pi4" not in text:
        print(f"Pi 4 summary does not record target=pi4: {summary}", file=sys.stderr)
        sys.exit(1)
    match = re.search(r"RESULT pass=(\d+) fail=(\d+) total=(\d+)", text)
    if not match:
        print(f"Pi 4 summary is missing RESULT line: {summary}", file=sys.stderr)
        sys.exit(1)
    passed, failed, total = (int(value) for value in match.groups())
    if failed != 0 or total == 0 or passed != total:
        print(
            f"Pi 4 regression summary is not a full pass: pass={passed} fail={failed} total={total}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"reused Pi 4 regression batch: {summary} pass={passed} total={total}")
    sys.exit(0)

expected = {
    "base": [
        "boot_v0",
        "9p_batch",
        "host_absent",
        "host_sidecar_mock",
        "observe_watch",
        "root_cut_basic",
        "session_lifecycle",
        "busy_backpressure",
        "cas_roundtrip",
        "tcp_basic",
        "session_pool",
    ],
    "base-telemetry": [
        "telemetry_ring",
        "telemetry_push_create",
    ],
    "base-shard": [
        "shard_1k",
    ],
    "gated": [
        "replay_journal",
        "policy_gate",
        "model_cas_bind",
        "sidecar_integration",
    ],
}

missing = []
for group, scripts in expected.items():
    group_root = root / group
    if not group_root.is_dir():
        missing.append(str(group_root))
        continue
    for script in scripts:
        for suffix in ("out.log", "qemu.log"):
            path = group_root / f"{script}.{suffix}"
            if not path.is_file() or path.stat().st_size == 0:
                missing.append(str(path))

if missing:
    print("reused QEMU regression batch is incomplete:", file=sys.stderr)
    for path in missing:
        print(f"  - {path}", file=sys.stderr)
    sys.exit(1)

print(f"reused QEMU regression batch: {root} groups={len(expected)} scripts=18")
PY
}

if [[ $# -gt 0 ]]; then
  if [[ "$1" == "--check-exceptions-register" && $# -eq 3 ]]; then
    check_exceptions_register "$2" "$3"
    exit $?
  fi
  printf "usage: %s [--check-exceptions-register <findings.csv> <EXCEPTIONS.md>]\n" "$0" >&2
  exit 2
fi

mkdir -p "$log_root"

run_step "required-audit-assets" check_required_audit_assets
run_step "cargo-lockfile" cargo metadata --locked --no-deps
run_step "cargo-fmt-check" cargo fmt --all -- --check
run_step "cargo-clippy-workspace" env CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
run_step "cargo-check-workspace" env CARGO_INCREMENTAL=0 cargo check --workspace
run_step "secure9p-codec-tests" cargo test -p secure9p-codec
run_step "integration-tests" cargo test -p tests
run_step "workspace-tests" env CARGO_INCREMENTAL=0 cargo test --workspace
run_step "rust-risk-bootstrap-tests" python3 scripts/ci/test_rust_risk_gate.py
if [[ "${DD_SKIP_CARGO_AUDIT:-0}" == "1" ]]; then
  mark_incomplete_step "cargo-audit" "DD_SKIP_CARGO_AUDIT=1"
else
  run_step "cargo-audit" cargo audit
fi
if [[ "${DD_SKIP_CARGO_DENY:-0}" == "1" ]]; then
  mark_incomplete_step "cargo-deny-advisories" "DD_SKIP_CARGO_DENY=1"
else
  run_step "cargo-deny-advisories" cargo deny check advisories
fi
run_step "rust-risk-ratchet" check_rust_risk_ratchet
run_step "generated-artifacts" scripts/check-generated.sh
if [[ "${DD_SKIP_TEST_PLAN_CHECK:-0}" == "1" ]]; then
  mark_incomplete_step "test-plan-hash-check" "DD_SKIP_TEST_PLAN_CHECK=1"
else
  run_step "test-plan-hash-check" scripts/ci/check_test_plan.sh
fi
if [[ "${DD_SKIP_REGRESSION_BATCH:-0}" == "1" ]]; then
  mark_incomplete_step "regression-batch" "DD_SKIP_REGRESSION_BATCH=1"
elif [[ -n "${dd_reuse_regression_batch_from}" ]]; then
  run_step "regression-batch-reuse" check_reused_regression_batch "${dd_reuse_regression_batch_from}"
elif [[ -n "${dd_regression_groups}" && "${dd_regression_groups}" != "all" ]]; then
  mark_incomplete_step "regression-batch" "DD_REGRESSION_GROUPS=${dd_regression_groups}"
else
  run_step \
    "regression-batch" \
    env \
    COHSH_BATCH_GROUPS="${dd_regression_groups}" \
    COHSH_LOG_ROOT="${log_root}/regression-logs" \
    READY_TIMEOUT="${dd_regression_ready_timeout}" \
    PORT_TIMEOUT="${dd_regression_port_timeout}" \
    AUTH_READY_TIMEOUT="${dd_regression_auth_timeout}" \
    QUIT_CLOSE_TIMEOUT="${dd_regression_quit_timeout}" \
    scripts/cohsh/run_regression_batch.sh
fi
run_step "release-guardrails-findings" check_blocking_findings
run_step "release-guardrails-exceptions" check_exceptions_register
run_step "hardcoded-secret-scan" scan_hardcoded_secrets

if [[ ${#failures[@]} -eq 0 ]]; then
  printf "\n[dd-gate] ALL CHECKS PASSED\n"
  printf "[dd-gate] LOG ROOT %s\n" "$log_root"
  exit 0
fi

printf "\n[dd-gate] FAILURES (%s):\n" "${#failures[@]}"
for failure in "${failures[@]}"; do
  printf "  - %s\n" "$failure"
done
if [[ ${#incomplete_steps[@]} -gt 0 ]]; then
  printf "\n[dd-gate] INCOMPLETE RUN (%s):\n" "${#incomplete_steps[@]}"
  local_step=""
  for local_step in "${incomplete_steps[@]}"; do
    printf "  - %s\n" "$local_step"
  done
  printf "[dd-gate] INCOMPLETE runs cannot be PASS per docs/audit/DUE_DILIGENCE_PLAN.md\n"
fi
printf "[dd-gate] LOG ROOT %s\n" "$log_root"
exit 1
