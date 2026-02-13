#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the Cohesix due-diligence gate checks and fail on assurance regressions.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"

declare -a failures=()

run_step() {
  local name="$1"
  shift
  printf "\n[dd-gate] START %s\n" "$name"
  if "$@"; then
    printf "[dd-gate] PASS  %s\n" "$name"
  else
    printf "[dd-gate] FAIL  %s\n" "$name"
    failures+=("$name")
  fi
}

run_step "cargo-check-workspace" cargo check --workspace
run_step "secure9p-codec-tests" cargo test -p secure9p-codec
run_step "integration-tests" cargo test -p tests
run_step "workspace-tests" cargo test --workspace
run_step "generated-artifacts" scripts/check-generated.sh

printf "\n[dd-gate] START hardcoded-secret-scan\n"
if rg -n '"changeme"' apps/root-task/src apps/hive-gateway/src apps/coh/src apps/cohsh/src -g '*.rs'; then
  printf "[dd-gate] FAIL  hardcoded-secret-scan\n"
  failures+=("hardcoded-secret-scan")
else
  rg_status=$?
  if [[ $rg_status -eq 1 ]]; then
    printf "[dd-gate] PASS  hardcoded-secret-scan\n"
  else
    printf "[dd-gate] FAIL  hardcoded-secret-scan (rg error %s)\n" "$rg_status"
    failures+=("hardcoded-secret-scan")
  fi
fi

if [[ ${#failures[@]} -eq 0 ]]; then
  printf "\n[dd-gate] ALL CHECKS PASSED\n"
  exit 0
fi

printf "\n[dd-gate] FAILURES (%s):\n" "${#failures[@]}"
for failure in "${failures[@]}"; do
  printf "  - %s\n" "$failure"
done
exit 1
