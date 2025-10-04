# CLASSIFICATION: COMMUNITY
# Filename: demo_federation_test.sh v0.2
# Date Modified: 2029-10-05
# Author: Lukas Bower

#!/usr/bin/env bash
# Simple demo showing queen federation and agent migration
set -euo pipefail

TRACE_BASE="${COHESIX_TRACE_TMP:-${TMPDIR:-}}"

if [[ -z "${TRACE_BASE}" ]]; then
    echo "[E1-F7] federation demo requires COHESIX_TRACE_TMP or TMPDIR" >&2
    exit 1
fi

TRACE_BASE="${TRACE_BASE%/}"

mkdir -p "${TRACE_BASE}"

NAMESPACE_ROOT=$(mktemp -d "${TRACE_BASE}/cohesix_fed_demo.XXXXXX")

cleanup() {
    if [[ -n "${NAMESPACE_ROOT}" && -d "${NAMESPACE_ROOT}" ]]; then
        find "${NAMESPACE_ROOT}" -type f -exec rm -f {} +
        find "${NAMESPACE_ROOT}" -depth -type d -exec rmdir {} + 2>/dev/null || true
    fi
}

trap cleanup EXIT

QUEEN_A="${NAMESPACE_ROOT}/queen_a"
QUEEN_B="${NAMESPACE_ROOT}/queen_b"

if [[ -z "${COHUP_BIN:-}" ]]; then
    for candidate in \
        "./workspace/target/debug/cohup" \
        "./target/debug/cohup" \
        "./workspace/target/aarch64-unknown-linux-gnu/release/cohup" \
        "./target/aarch64-unknown-linux-gnu/release/cohup"; do
        if [[ -x "${candidate}" ]]; then
            COHUP_BIN="${candidate}"
            break
        fi
    done
fi

COHUP_BIN="${COHUP_BIN:-./workspace/target/debug/cohup}"

if [[ ! -x "${COHUP_BIN}" ]]; then
    echo "[E1-F7] cohup binary not found at ${COHUP_BIN}" >&2
    exit 1
fi

setup_queen() {
    local dir=$1
    mkdir -p "$dir/srv/federation/known_hosts" "$dir/srv/orch" "$dir/srv/agents"
    echo "QueenPrimary" > "$dir/srv/cohrole"
}

setup_queen "$QUEEN_A"
setup_queen "$QUEEN_B"

COHROLE=QueenPrimary QUEEN_DIR="${QUEEN_A}" "${COHUP_BIN}" join --peer B || true
COHROLE=QueenPrimary QUEEN_DIR="${QUEEN_B}" "${COHUP_BIN}" join --peer A || true

echo "Federation setup complete"
