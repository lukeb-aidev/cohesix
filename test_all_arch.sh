# CLASSIFICATION: COMMUNITY
# Filename: test_all_arch.sh v1.1
# Author: Lukas Bower
# Date Modified: 2025-08-25

#!/usr/bin/env bash
###############################################################################
# test_all_arch.sh – run cross-architecture test suite
#
# Executes Rust, Go, and Python tests to validate the workspace across
# supported architectures. Fails fast on any test failure.
#
# Usage:
#   ./test_all_arch.sh
###############################################################################
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

LOG_DIR="$ROOT_DIR/logs"
mkdir -p "$LOG_DIR"
TS="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="$LOG_DIR/test_all_arch_${TS}.log"
SUMMARY_FILE="$LOG_DIR/test_summary.txt"
START_TIME="$(date +%s)"
FATAL_ERROR=""

exec > >(tee "$LOG_FILE") 2>&1

fail() { FATAL_ERROR="$1"; echo "ERROR: $1" >&2; exit 1; }

write_summary() {
    local verdict=$1
    local end_time="$(date +%s)"
    local duration=$(( end_time - START_TIME ))
    cat <<EOF > "$SUMMARY_FILE"
Timestamp: $(date '+%Y-%m-%d %H:%M:%S')
Verdict: $verdict
Duration: ${duration}s
Fatal Error: ${FATAL_ERROR:-none}
EOF
}

trap 'c=$?; v=PASS; [ $c -ne 0 ] && v=FAIL; write_summary "$v"' EXIT

if ! cargo test --workspace; then
    fail "cargo test failed"
fi

if ! GOWORK="$(pwd)/go/go.work" go test ./go/...; then
    fail "go test failed"
fi

if ! pytest -q; then
    fail "pytest failed"
fi

if ! bash tests/demo_edge_failover.sh; then
    fail "demo_edge_failover.sh failed"
fi

if ! bash tests/demo_sensor_feedback.sh; then
    fail "demo_sensor_feedback.sh failed"
fi

for t in tests/demos/test_*.sh; do
    if ! bash "$t"; then
        fail "$(basename "$t") failed"
    fi
done
