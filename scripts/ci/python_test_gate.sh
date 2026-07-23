#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the complete Cohesix Python test or example lane with a pinned shared pytest environment.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
pytest_version="9.1.1"
pyserial_version="3.5"
requirements_lock="${repo_root}/configs/test-plan-python-requirements.lock"
requested_python="${TP_PYTHON_BIN:-python3}"

usage() {
  cat <<'EOF'
Usage: scripts/ci/python_test_gate.sh MODE

Modes:
  --tests          Run all repository, client, due-diligence, and test-plan tests.
  --examples       Run the four Python client example smoke checks.
  --list-tests     Print the pytest paths without creating an environment.
  --list-examples  Print the example smoke identifiers without running them.
  --cache-key      Print the interpreter- and requirements-specific cache key.
EOF
}

resolve_python() {
  local command_path
  if ! command_path=$(command -v "${requested_python}"); then
    printf "Python interpreter not found: %s\n" "${requested_python}" >&2
    return 1
  fi

  resolved_python=$(
    "${command_path}" -c \
      'import os, sys; print(os.path.realpath(sys.executable))'
  )
  if [[ "${resolved_python}" != /* || ! -x "${resolved_python}" ]]; then
    printf "resolved Python interpreter is not an absolute executable: %s\n" \
      "${resolved_python}" >&2
    return 1
  fi

  python_identity=$(
    "${resolved_python}" -c \
      'import platform, sys; print(f"{platform.python_implementation()}-{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")'
  )
  "${resolved_python}" -c \
    'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)' || {
      printf "Python 3.11 or newer is required: %s (%s)\n" \
        "${resolved_python}" "${python_identity}" >&2
      return 1
    }
}

compute_cache_key() {
  resolve_python
  if [[ ! -f "${requirements_lock}" ]]; then
    printf "Python test requirements lock not found: %s\n" \
      "${requirements_lock}" >&2
    return 1
  fi
  requirements_digest=$(shasum -a 256 "${requirements_lock}" | awk '{print $1}')
  cache_digest=$(
    printf "%s\n%s\nrequirements=%s\n" \
      "${resolved_python}" \
      "${python_identity}" \
      "${requirements_digest}" |
      shasum -a 256 |
      awk '{print $1}'
  )
  python_cache_key="python-tests-${python_identity}-${cache_digest:0:16}"
}

append_test_plan_tests() {
  local absolute_path
  local relative_path
  for absolute_path in "${repo_root}"/scripts/ci/test_*.py; do
    [[ -f "${absolute_path}" ]] || continue
    relative_path="${absolute_path#"${repo_root}/"}"
    # The hostile bootstrap/risk suite is its own catalog action so its
    # subprocess-heavy cases are not duplicated in broad pytest discovery.
    if [[ "${relative_path}" == "scripts/ci/test_rust_risk_gate.py" ]]; then
      continue
    fi
    python_test_paths+=("${relative_path}")
  done
}

list_tests() {
  local path
  python_test_paths=(
    "tests"
    "tools/cohesix-py/tests"
  )
  append_test_plan_tests
  for path in "${python_test_paths[@]}"; do
    printf "%s\n" "${path}"
  done
}

list_examples() {
  printf "%s\n" \
    "lease-run" \
    "peft-roundtrip" \
    "telemetry-write-pull" \
    "mixed-closed-loop-ai-factory"
}

release_venv_lock() {
  if [[ "${venv_lock_owned:-0}" == "1" ]]; then
    rmdir "${venv_lock_dir}" 2>/dev/null || true
    venv_lock_owned=0
  fi
}

prepare_pytest() {
  compute_cache_key
  local venv_root="${TP_PYTHON_TEST_VENV_ROOT:-${repo_root}/out/toolchain/python-test-venvs}"
  venv_dir="${venv_root}/${python_cache_key}"
  venv_python="${venv_dir}/bin/python3"
  venv_lock_dir="${venv_dir}.lock"
  venv_lock_owned=0
  mkdir -p "${venv_root}"

  local attempt
  for attempt in $(seq 1 60); do
    if mkdir "${venv_lock_dir}" 2>/dev/null; then
      venv_lock_owned=1
      break
    fi
    sleep 0.5
  done
  if [[ "${venv_lock_owned}" != "1" ]]; then
    printf "timed out waiting for Python test environment lock: %s\n" \
      "${venv_lock_dir}" >&2
    return 1
  fi
  trap release_venv_lock EXIT

  if [[ -x "${venv_python}" ]] && ! "${venv_python}" -c \
    'import os, sys; expected = os.path.realpath(sys.argv[1]); actual = os.path.realpath(sys._base_executable); raise SystemExit(0 if actual == expected else 1)' \
    "${resolved_python}"
  then
    printf "Recreating Python test environment for %s\n" "${resolved_python}"
    "${resolved_python}" -m venv --clear "${venv_dir}"
  elif [[ ! -x "${venv_python}" ]]; then
    printf "Creating Python test environment for %s\n" "${resolved_python}"
    "${resolved_python}" -m venv "${venv_dir}"
  fi

  if ! "${venv_python}" -c \
    'import importlib.metadata, sys
expected = {"pytest": sys.argv[1], "pyserial": sys.argv[2]}
actual = {name: importlib.metadata.version(name) for name in expected}
raise SystemExit(0 if actual == expected else 1)' \
    "${pytest_version}" "${pyserial_version}" >/dev/null 2>&1
  then
    printf "Installing hashed Python test requirements in %s\n" "${venv_dir}"
    "${venv_python}" -m pip \
      --disable-pip-version-check \
      install \
      --require-hashes \
      --only-binary=:all: \
      --requirement "${requirements_lock}"
  fi

  release_venv_lock
  trap - EXIT
}

run_tests() {
  prepare_pytest
  python_test_paths=(
    "tests"
    "tools/cohesix-py/tests"
  )
  append_test_plan_tests
  cd "${repo_root}"
  "${venv_python}" -m pytest -q "${python_test_paths[@]}"
}

run_examples() {
  compute_cache_key
  local python_playbook_out="${TP_PYTHON_PLAYBOOK_OUT:-${repo_root}/out/test-plan/python-playbooks}"
  cd "${repo_root}"
  "${resolved_python}" tools/cohesix-py/examples/lease_run.py --mock
  "${resolved_python}" tools/cohesix-py/examples/peft_roundtrip.py --mock
  "${resolved_python}" tools/cohesix-py/examples/telemetry_write_pull.py --mock
  "${resolved_python}" tools/cohesix-py/examples/use_case_playbook.py \
    --playbook mixed-closed-loop-ai-factory \
    --dry-run \
    --mock \
    --no-proc-snapshot \
    --no-host-snapshot \
    --no-push-host-snapshot \
    --out "${python_playbook_out}"
}

if [[ $# -ne 1 ]]; then
  usage >&2
  exit 2
fi

case "$1" in
  --tests)
    run_tests
    ;;
  --examples)
    run_examples
    ;;
  --list-tests)
    list_tests
    ;;
  --list-examples)
    list_examples
    ;;
  --cache-key)
    compute_cache_key
    printf "%s\n" "${python_cache_key}"
    ;;
  -h|--help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
