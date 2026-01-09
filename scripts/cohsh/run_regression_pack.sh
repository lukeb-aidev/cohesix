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

usage() {
    cat <<'USAGE'
Usage: scripts/cohsh/run_regression_pack.sh [--cohsh <path>] [--tcp-host <host>] [--tcp-port <port>]

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

wait_for_console() {
    if ! command -v nc >/dev/null 2>&1; then
        return 0
    fi
    local deadline=$((SECONDS + 30))
    while ! nc -z "${TCP_HOST}" "${TCP_PORT}" >/dev/null 2>&1; do
        if (( SECONDS >= deadline )); then
            echo "QEMU TCP console not reachable at ${TCP_HOST}:${TCP_PORT}" >&2
            return 1
        fi
        sleep 1
    done
}

wait_for_console

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

    tmp_script=$(mktemp)
    {
        printf 'attach queen\nEXPECT OK\n'
        cat "${script_path}"
    } >"${tmp_script}"

    wait_for_console
    echo "running ${script}"
    if ! output=$("${COHSH_BIN}" --transport tcp --tcp-host "${TCP_HOST}" --tcp-port "${TCP_PORT}" --script "${tmp_script}" 2>&1); then
        echo "FAILED: ${script_path}" >&2
        echo "${output}" >&2
        rm -f "${tmp_script}"
        exit 1
    fi
    rm -f "${tmp_script}"
    echo "ok: ${script}"
    sleep 1
done

echo "regression pack complete: ${#scripts[@]} scripts passed"
