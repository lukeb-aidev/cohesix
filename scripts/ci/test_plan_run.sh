#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run staged Cohesix test-plan scripts with deterministic stage progression.
# Copyright 2026 Lukas Bower

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "${script_dir}/../.." && pwd)

usage() {
  cat <<'USAGE'
Usage: scripts/ci/test_plan_run.sh [--state-dir <path>] [--stage <1..5>] [--list]

Runs the scripted test plan stages in order:
  1 integrity
  2 host-fast
  3 qemu-tcp-regression
  4 rest-multiplexer
  5 due-diligence

Options:
  --state-dir <path>  Shared state/log directory for stage markers and logs.
  --stage <n>         Run exactly one stage (requires previous stage markers for n>1).
  --list              Print stage map and exit.
  --help              Show this help.

Environment pass-through:
  TEST_PLAN_STATE_DIR
  COHESIX_GATEWAY_URL / HIVE_GATEWAY_URL / COHSH_REST_URL / COH_REST_URL
  TP_SKIP_GENERATED_CHECK, TP_SKIP_PYTHON, TP_WRITE_TRACE_FIXTURES
USAGE
}

list_stages() {
  cat <<'STAGES'
1  scripts/ci/test_plan_stage_01_integrity.sh
2  scripts/ci/test_plan_stage_02_host_fast.sh
3  scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh
4  scripts/ci/test_plan_stage_04_rest_multiplexer.sh
5  scripts/ci/test_plan_stage_05_due_diligence.sh
STAGES
}

state_dir="${TEST_PLAN_STATE_DIR:-${repo_root}/out/test-plan/$(date -u +%Y%m%dT%H%M%SZ)}"
single_stage=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --state-dir)
      shift
      [[ $# -gt 0 ]] || {
        echo "--state-dir requires a value" >&2
        exit 2
      }
      state_dir="$1"
      ;;
    --stage)
      shift
      [[ $# -gt 0 ]] || {
        echo "--stage requires a value" >&2
        exit 2
      }
      single_stage="$1"
      ;;
    --list)
      list_stages
      exit 0
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

stage_script_path() {
  local stage="$1"
  case "${stage}" in
    1) printf "%s/scripts/ci/test_plan_stage_01_integrity.sh" "${repo_root}" ;;
    2) printf "%s/scripts/ci/test_plan_stage_02_host_fast.sh" "${repo_root}" ;;
    3) printf "%s/scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh" "${repo_root}" ;;
    4) printf "%s/scripts/ci/test_plan_stage_04_rest_multiplexer.sh" "${repo_root}" ;;
    5) printf "%s/scripts/ci/test_plan_stage_05_due_diligence.sh" "${repo_root}" ;;
    *) return 1 ;;
  esac
}

declare -a stages
if [[ -n "${single_stage}" ]]; then
  if ! stage_script_path "${single_stage}" >/dev/null; then
    echo "invalid --stage value: ${single_stage}" >&2
    exit 2
  fi
  stages=("${single_stage}")
else
  stages=(1 2 3 4 5)
fi

mkdir -p "${state_dir}"
echo "[test-plan] root: ${repo_root}"
echo "[test-plan] state-dir: ${state_dir}"

for stage in "${stages[@]}"; do
  script_path="$(stage_script_path "${stage}")"
  if [[ ! -x "${script_path}" ]]; then
    echo "stage script is missing or not executable: ${script_path}" >&2
    exit 1
  fi
  echo "[test-plan] running stage ${stage}: ${script_path}"
  TEST_PLAN_STATE_DIR="${state_dir}" "${script_path}"
done

echo "[test-plan] completed stages: ${stages[*]}"
echo "[test-plan] logs: ${state_dir}/logs"
