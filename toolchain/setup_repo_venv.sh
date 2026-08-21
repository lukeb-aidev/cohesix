#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Create and verify the repository-local Cohesix Python environment.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
VENV_DIR="${REPO_ROOT}/.venv"
REQUIREMENTS_LOCK="${REPO_ROOT}/configs/test-plan-python-requirements.lock"
PYTHON_BIN="python3"

usage() {
    cat <<'EOF'
Usage: toolchain/setup_repo_venv.sh [--python PATH]

Create or update the repository-local .venv using the hash-locked host-test
requirements, then install tools/cohesix-py in editable mode without resolving
undeclared dependencies.
EOF
}

fail() {
    printf '[toolchain] error: %s\n' "$*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --python)
            [[ $# -ge 2 ]] || fail "--python requires an interpreter path"
            PYTHON_BIN="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

command -v "${PYTHON_BIN}" >/dev/null 2>&1 || \
    fail "Python interpreter not found: ${PYTHON_BIN}"
[[ -f "${REQUIREMENTS_LOCK}" ]] || \
    fail "Python requirements lock not found: ${REQUIREMENTS_LOCK}"

"${PYTHON_BIN}" - <<'PY' || fail "Cohesix requires Python 3.11 or later"
import sys

if sys.version_info < (3, 11):
    raise SystemExit(1)
PY

if [[ -L "${VENV_DIR}" ]]; then
    fail "refusing symlinked repository environment: ${VENV_DIR}"
fi
if [[ -e "${VENV_DIR}" && ! -x "${VENV_DIR}/bin/python" ]]; then
    fail "${VENV_DIR} exists but is not a usable virtual environment; move it aside and retry"
fi

if [[ ! -d "${VENV_DIR}" ]]; then
    printf '[toolchain] Creating repository Python environment at %s\n' "${VENV_DIR}"
    "${PYTHON_BIN}" -m venv "${VENV_DIR}"
fi

VENV_PYTHON="${VENV_DIR}/bin/python"
"${VENV_PYTHON}" - <<'PY' || fail "existing .venv uses Python older than 3.11"
import sys

if sys.version_info < (3, 11):
    raise SystemExit(1)
PY

printf '[toolchain] Installing hash-locked host-test requirements...\n'
"${VENV_PYTHON}" -m pip install \
    --disable-pip-version-check \
    --require-hashes \
    --only-binary=:all: \
    --requirement "${REQUIREMENTS_LOCK}"

printf '[toolchain] Installing the Cohesix Python client in editable mode...\n'
"${VENV_PYTHON}" -m pip install \
    --disable-pip-version-check \
    --no-build-isolation \
    --no-deps \
    --editable "${REPO_ROOT}/tools/cohesix-py"

"${VENV_PYTHON}" - <<'PY'
import importlib.metadata
import sys

import cohesix

required = {
    "pytest": "9.1.1",
    "pyserial": "3.5",
    "setuptools": "80.9.0",
}
for distribution, expected in required.items():
    actual = importlib.metadata.version(distribution)
    if actual != expected:
        raise SystemExit(
            f"{distribution} version mismatch: expected {expected}, got {actual}"
        )

print(f"[toolchain] Repository Python: {sys.version.split()[0]}")
print(f"[toolchain] Cohesix Python: {importlib.metadata.version('cohesix')}")
PY

printf '[toolchain] Repository Python environment ready: source .venv/bin/activate\n'
