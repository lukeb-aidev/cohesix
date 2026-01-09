#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the cohsh .coh regression pack against a live QEMU TCP console.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/../.." && pwd)

COHSH_BIN="${COHSH_BIN:-${REPO_ROOT}/out/cohesix/host-tools/cohsh}"
TCP_HOST="${COHSH_TCP_HOST:-127.0.0.1}"
TCP_PORT="${COHSH_TCP_PORT:-31337}"
SCRIPTS_DIR="${SCRIPT_DIR}"
AUTH_TOKEN="${AUTH_TOKEN:-${COHSH_AUTH_TOKEN:-}}"
COHSH_TIMEOUT="${COHSH_TIMEOUT:-20}"

usage() {
    cat <<'USAGE'
Usage: scripts/cohsh/run_regression_pack.sh [--cohsh <path>] [--tcp-host <host>] [--tcp-port <port>] [--auth-token <token>] [--timeout <seconds>]

Runs the .coh regression scripts against an already-running QEMU TCP console.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --cohsh)
            COHSH_BIN="$2"
            shift 2
            ;;
        --tcp-host)
            TCP_HOST="$2"
            shift 2
            ;;
        --tcp-port)
            TCP_PORT="$2"
            shift 2
            ;;
        --auth-token)
            AUTH_TOKEN="$2"
            shift 2
            ;;
        --timeout)
            COHSH_TIMEOUT="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

if [[ ! -x "${COHSH_BIN}" ]]; then
    echo "cohsh binary not found or not executable: ${COHSH_BIN}" >&2
    exit 1
fi

run_with_timeout() {
    local timeout="$1"
    shift
    python3 - "$timeout" "$@" <<'PY'
import subprocess
import sys

timeout = float(sys.argv[1])
cmd = sys.argv[2:]
try:
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout)
    sys.stdout.buffer.write(proc.stdout)
    sys.stdout.buffer.flush()
    sys.exit(proc.returncode)
except subprocess.TimeoutExpired as err:
    if err.stdout:
        sys.stdout.buffer.write(err.stdout)
        sys.stdout.buffer.flush()
    print(f"[regression-pack] error: command timed out after {timeout}s", file=sys.stderr)
    sys.exit(124)
PY
}

run_cohsh_script() {
    local script_path="$1"
    local timeout="$2"
    local -a cmd=(
        "${COHSH_BIN}"
        --transport tcp
        --tcp-host "${TCP_HOST}"
        --tcp-port "${TCP_PORT}"
        --script "${script_path}"
    )
    if [[ -n "${AUTH_TOKEN}" ]]; then
        cmd+=(--auth-token "${AUTH_TOKEN}")
    fi
    run_with_timeout "${timeout}" "${cmd[@]}"
}

probe_console() {
    local deadline=$((SECONDS + 30))
    local probe_script
    local last_error=""
    probe_script=$(mktemp -t cohsh_probe)
    cat >"${probe_script}" <<'COMMANDS'
attach queen
ping
quit
COMMANDS

    while (( SECONDS < deadline )); do
        local output=""
        if output=$(run_cohsh_script "${probe_script}" "${COHSH_TIMEOUT}" 2>&1); then
            rm -f "${probe_script}"
            return 0
        fi
        last_error="${output}"
        sleep 1
    done

    rm -f "${probe_script}"
    echo "QEMU TCP console not reachable at ${TCP_HOST}:${TCP_PORT}" >&2
    if [[ -n "${last_error}" ]]; then
        echo "[regression-pack] last cohsh output:" >&2
        echo "${last_error}" >&2
    fi
    return 1
}

probe_console

scripts=(
    "boot_v0.coh"
    "9p_batch.coh"
    "telemetry_ring.coh"
    "observe_watch.coh"
    "cas_roundtrip.coh"
)

for script in "${scripts[@]}"; do
    script_path="${SCRIPTS_DIR}/${script}"
    if [[ ! -f "${script_path}" ]]; then
        echo "Missing script: ${script_path}" >&2
        exit 1
    fi

    probe_console
    echo "running ${script}"
    if ! output=$(run_cohsh_script "${script_path}" "${COHSH_TIMEOUT}" 2>&1); then
        echo "FAILED: ${script_path}" >&2
        echo "${output}" >&2
        exit 1
    fi
    echo "ok: ${script}"
    sleep 1
done

echo "regression pack complete: ${#scripts[@]} scripts passed"
