#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the Cohesix due-diligence gate checks, record evidence logs, and fail on assurance regressions.
# Copyright 2026 Lukas Bower
#
# Environment:
#   DD_GATE_LOG_DIR            Override log output root (default: out/audit/gate/<utc-timestamp>)
#   DD_SKIP_TEST_PLAN_CHECK=1  Mark test-plan hash check as incomplete (run still fails)
#   DD_SKIP_REGRESSION_BATCH=1 Mark regression batch check as incomplete (run still fails)

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"

run_id=$(date -u +"%Y%m%dT%H%M%SZ")
log_root="${DD_GATE_LOG_DIR:-${repo_root}/out/audit/gate/${run_id}}"
mkdir -p "$log_root"

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

scan_hardcoded_secrets() {
  if rg -n '"changeme"' apps/root-task/src apps/hive-gateway/src apps/coh/src apps/cohsh/src -g '*.rs'; then
    return 1
  fi

  local rg_status=$?
  if [[ $rg_status -eq 1 ]]; then
    return 0
  fi

  printf "secret scan failed due to rg error: %s\n" "$rg_status" >&2
  return 1
}

check_blocking_findings() {
  python3 <<'PY'
import csv
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
for row in reader:
    severity = row.get("severity", "").strip().upper()
    disposition = row.get(disposition_field, "").strip().upper()
    finding_id = row.get("finding_id", "UNKNOWN")
    if severity in {"P0", "P1"} and disposition != "CLOSED_VERIFIED":
        blocking.append((finding_id, severity, disposition))

if blocking:
    print("blocking findings remain open:", file=sys.stderr)
    for finding_id, severity, disposition in blocking:
        print(f"  - {finding_id} ({severity}, {disposition})", file=sys.stderr)
    sys.exit(1)

print("blocking findings gate passed")
PY
}

check_exceptions_register() {
  python3 <<'PY'
from datetime import date
import pathlib
import sys

path = pathlib.Path("docs/audit/EXCEPTIONS.md")
if not path.is_file():
    print("missing docs/audit/EXCEPTIONS.md", file=sys.stderr)
    sys.exit(1)

lines = path.read_text().splitlines()
table_rows = []
for line in lines:
    stripped = line.strip()
    if stripped.startswith("|") and stripped.endswith("|"):
        cells = [cell.strip() for cell in stripped.strip("|").split("|")]
        if len(cells) == 11:
            table_rows.append(cells)

if not table_rows:
    print("exceptions register table not found", file=sys.stderr)
    sys.exit(1)

today = date.today()
errors = []
for cells in table_rows:
    first = cells[0]
    if first == "Exception ID":
        continue
    if set(first.replace("-", "").strip()) == set():
        continue

    exception_id = first.strip("`")
    expiration = cells[9].strip("`")
    status = cells[10].strip("`")

    if status == "EXPIRED":
        errors.append(f"{exception_id}: status is EXPIRED")
        continue

    if status == "APPROVED_ACTIVE":
        try:
            expiry_date = date.fromisoformat(expiration)
        except ValueError:
            errors.append(f"{exception_id}: invalid expiration date '{expiration}'")
            continue
        if expiry_date < today:
            errors.append(f"{exception_id}: expired on {expiry_date.isoformat()}")

if errors:
    print("exceptions register validation failed:", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

print("exceptions register gate passed")
PY
}

run_step "required-audit-assets" check_required_audit_assets
run_step "cargo-check-workspace" cargo check --workspace
run_step "secure9p-codec-tests" cargo test -p secure9p-codec
run_step "integration-tests" cargo test -p tests
run_step "workspace-tests" cargo test --workspace
run_step "generated-artifacts" scripts/check-generated.sh
if [[ "${DD_SKIP_TEST_PLAN_CHECK:-0}" == "1" ]]; then
  mark_incomplete_step "test-plan-hash-check" "DD_SKIP_TEST_PLAN_CHECK=1"
else
  run_step "test-plan-hash-check" scripts/ci/check_test_plan.sh
fi
if [[ "${DD_SKIP_REGRESSION_BATCH:-0}" == "1" ]]; then
  mark_incomplete_step "regression-batch" "DD_SKIP_REGRESSION_BATCH=1"
else
  run_step "regression-batch" scripts/cohsh/run_regression_batch.sh
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
