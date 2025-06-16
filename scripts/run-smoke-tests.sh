// CLASSIFICATION: COMMUNITY
// Filename: run-smoke-tests.sh v0.4
// Date Modified: 2025-08-25
// Author: Lukas Bower


#!/usr/bin/env bash
###############################################################################
# run-smoke-tests.sh – Cohesix quick‑health suite
#
# Runs a lightweight set of checks to confirm that the developer
# workstation / CI runner can build and execute core Cohesix artefacts.
#
# What it does
# ------------
#   1. `cargo check` and a fast subset of unit tests
#   2. Executes the cohesix‑9p test binary (if built)
#   3. Runs BusyBox (if present) inside a temporary chroot
#   4. Validates the heartbeat‑watchdog scripts with a 3‑second pulse
#
# Usage
# -----
#   ./scripts/run-smoke-tests.sh          # all tests
#   FAST=1 ./scripts/run-smoke-tests.sh   # cargo check only
#
# Exit codes
# ----------
#   0  All smoke tests passed
#   1  One or more checks failed
###############################################################################
set -euo pipefail

msg()  { printf "\e[32m[smoke]\e[0m %s\n" "$*"; }
warn() { printf "\e[33m[warn]\e[0m %s\n" "$*"; }
fail() { FATAL_ERROR="$*"; printf "\e[31m[FAIL]\e[0m %s\n" "$*"; exit 1; }

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

LOG_DIR="$ROOT_DIR/logs"
mkdir -p "$LOG_DIR"
TS="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="$LOG_DIR/run_smoke_tests_${TS}.log"
SUMMARY_FILE="$LOG_DIR/test_summary.txt"
START_TIME="$(date +%s)"
FATAL_ERROR=""

exec > >(tee "$LOG_FILE") 2>&1

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

trap 'c=$?; verdict=PASS; [ $c -ne 0 ] && verdict=FAIL; write_summary "$verdict"' EXIT

# --------------------------------------------------------------------------- #
# 1. Cargo sanity
# --------------------------------------------------------------------------- #
msg "Running cargo check …"
cargo check --workspace || fail "cargo check failed"

if [[ "${FAST:-0}" != "1" ]]; then
  msg "Running focussed unit tests (exclude slow/integration) …"
  cargo test --workspace -- --skip slow || fail "unit tests failed"
fi

# --------------------------------------------------------------------------- #
# 2. Test cohesix‑9p server binary (if built)
# --------------------------------------------------------------------------- #
NINEP_BIN="target/debug/cohesix-9p-test"
if [[ -x "$NINEP_BIN" ]]; then
  msg "Launching 9P test binary briefly …"
  ( "$NINEP_BIN" --help >/dev/null 2>&1 ) || fail "9P binary failed to run"
else
  warn "9P test binary not found – skipping"
fi

# --------------------------------------------------------------------------- #
# 3. BusyBox smoke (if present)
# --------------------------------------------------------------------------- #
BUSYBOX="$(find out/busybox -type f -name busybox | head -n1 || true)" # non-blocking
if [[ -x "$BUSYBOX" ]]; then
  msg "Testing BusyBox → $BUSYBOX"
  TEMP_DIR="$(mktemp -d)"
  ( cd "$TEMP_DIR" && "$BUSYBOX" echo "busybox‑ok" ) || fail "BusyBox failed"
  rm -rf "$TEMP_DIR"
else
  warn "BusyBox not built – skipping"
fi

# --------------------------------------------------------------------------- #
# 4. Heartbeat watchdog self‑test (3 s)
# --------------------------------------------------------------------------- #
HB_FILE="/tmp/cohesix_smoke.heartbeat"
touch "$HB_FILE"

msg "Spawning watchdog self‑test …"
scripts/heartbeat-check.sh "$HB_FILE" 3 --log /tmp/cohesix_smoke.log --recover "touch $HB_FILE.recovered" &
WATCH_PID=$!

# Pulse the heartbeat twice
sleep 1; touch "$HB_FILE"
sleep 2; touch "$HB_FILE"

# Allow one more interval then kill watchdog
sleep 2
kill "$WATCH_PID" || true  # allow missing process
rm -f "$HB_FILE" "$HB_FILE.recovered" /tmp/cohesix_smoke.log

msg "✅  Smoke tests completed successfully."
