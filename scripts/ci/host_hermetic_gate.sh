#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Execute catalogued host-hermetic tests and optional provisioned target checks exactly once.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
catalog_path="${repo_root}/scripts/ci/test_plan_catalog.py"
# shellcheck source=scripts/ci/test_plan_common.sh
source "${repo_root}/scripts/ci/test_plan_common.sh"
tp_configure_resource_limits
mode=""
target=""
list_only=0

usage() {
  cat <<'EOF'
Usage:
  scripts/ci/host_hermetic_gate.sh --integrity-only [--list]
  scripts/ci/host_hermetic_gate.sh --common-only [--list]
  scripts/ci/host_hermetic_gate.sh --target qemu|pi4 [--list]

--integrity-only runs the Stage 01 integrity actions.
--common-only runs the complete hosted/hermetic host action closure.
--target adds only the selected provisioned-target checks.
--list prints the selected action set without executing it.
EOF
}

if [[ $# -ge 1 && "${!#}" == "--list" ]]; then
  list_only=1
  set -- "${@:1:$(($# - 1))}"
fi

if [[ $# -eq 1 && "$1" == "--integrity-only" ]]; then
  mode="integrity"
elif [[ $# -eq 1 && "$1" == "--common-only" ]]; then
  mode="common"
elif [[ $# -eq 2 && "$1" == "--target" ]]; then
  mode="target"
  target="$2"
  if [[ "${target}" != "qemu" && "${target}" != "pi4" ]]; then
    printf "unsupported test-plan target: %s\n" "${target}" >&2
    exit 2
  fi
else
  usage >&2
  exit 2
fi

TEST_PLAN_ROOT="${TEST_PLAN_ROOT:-${repo_root}}"
TP_EVIDENCE_TOOL="${TEST_PLAN_ROOT}/scripts/ci/test_plan_evidence.py"
export TEST_PLAN_ROOT
if [[ -n "${TEST_PLAN_STATE_DIR:-}" ]]; then
  TP_PYTHON_PLAYBOOK_OUT="${TP_PYTHON_PLAYBOOK_OUT:-${TEST_PLAN_STATE_DIR}/python-playbooks}"
  export TP_PYTHON_PLAYBOOK_OUT
fi

COMMON_ACTION_IDS=()
while IFS= read -r action_id; do
  [[ -n "${action_id}" ]] || continue
  if [[ "${mode}" == "integrity" && "${action_id}" == integrity.* ]]; then
    COMMON_ACTION_IDS+=("${action_id}")
  elif [[ "${mode}" != "integrity" && "${action_id}" == host.* ]]; then
    COMMON_ACTION_IDS+=("${action_id}")
  fi
done < <(python3 "${catalog_path}" list --stage 1 --scope common)

PROVISIONED_ACTION_IDS=()
if [[ "${mode}" == "target" ]]; then
  while IFS= read -r action_id; do
    [[ -n "${action_id}" ]] && PROVISIONED_ACTION_IDS+=("${action_id}")
  done < <(
    python3 "${catalog_path}" list \
      --stage 2 \
      --scope provisioned-target \
      --target "${target}"
  )
fi

ACTION_IDS=(
  "${COMMON_ACTION_IDS[@]}"
)
if [[ "${mode}" == "target" ]]; then
  ACTION_IDS+=("${PROVISIONED_ACTION_IDS[@]}")
fi
if [[ ${#ACTION_IDS[@]} -eq 0 ]]; then
  printf "catalog selected no actions for mode=%s target=%s\n" \
    "${mode}" \
    "${target:-none}" >&2
  exit 1
fi

for action_id in "${COMMON_ACTION_IDS[@]}"; do
  printf "[host-gate] SELECT scope=common action=%s\n" "${action_id}"
done
if [[ "${mode}" == "target" ]]; then
  for action_id in "${PROVISIONED_ACTION_IDS[@]}"; do
    printf "[host-gate] SELECT scope=provisioned-target target=%s action=%s\n" \
      "${target}" \
      "${action_id}"
  done
fi
if [[ "${list_only}" == "1" ]]; then
  exit 0
fi

cd "${repo_root}"
printf "[host-gate] RESOURCE jobs=%s rust_test_threads=%s\n" \
  "${TP_HOST_JOBS}" \
  "${RUST_TEST_THREADS}"
for action_id in "${ACTION_IDS[@]}"; do
  if [[ "${TP_SKIP_PYTHON:-0}" == "1" &&
        "${action_id}" == host.python-* ]]; then
    printf "[host-gate] FAIL  %s cannot be skipped in a passing gate (TP_SKIP_PYTHON=1)\n" \
      "${action_id}" >&2
    exit 1
  fi

  action_command=$(
    python3 "${catalog_path}" action --id "${action_id}" --field command
  )
  timeout_seconds=$(
    python3 "${catalog_path}" \
      action \
      --id "${action_id}" \
      --field timeout_seconds
  )
  test_policy=$(
    python3 "${catalog_path}" \
      action \
      --id "${action_id}" \
      --field test_policy
  )
  minimum_test_count=$(
    python3 "${catalog_path}" \
      action \
      --id "${action_id}" \
      --field minimum_test_count
  )
  printf "[host-gate] START %s\n" "${action_id}"
  printf "[host-gate] CMD   %s\n" "${action_command}"
  if tp_catalog_execute \
    "${timeout_seconds}" \
    "${test_policy}" \
    "${minimum_test_count}" \
    "${action_command}"; then
    printf "[host-gate] PASS  %s\n" "${action_id}"
  else
    status=$?
    printf "[host-gate] FAIL  %s (status=%s)\n" "${action_id}" "${status}" >&2
    exit "${status}"
  fi
done

printf "[host-gate] PASS  actions=%s mode=%s target=%s\n" \
  "${#ACTION_IDS[@]}" \
  "${mode}" \
  "${target:-common}"
