#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

PROJECT_ROOT="$(git rev-parse --show-toplevel)"
SCRIPT_FILE="$(mktemp)"
trap 'rm -f "$SCRIPT_FILE"' EXIT

cat <<'CMDS' >"$SCRIPT_FILE"
attach queen
echo '{"spawn":"heartbeat","ticks":5}' > /queen/ctl
tail /trace/events
tail /proc/worker-1/trace
quit
CMDS

OUTPUT=$(cd "$PROJECT_ROOT" && cargo run -p cohsh -- --script "$SCRIPT_FILE")

if ! grep -q '"spawned worker-1"' <<<"$OUTPUT"; then
    echo "[tracefs] ERROR: expected spawn event not found" >&2
    exit 1
fi

if ! grep -q '/proc/worker-1/trace' <<<"$OUTPUT"; then
    echo "[tracefs] ERROR: worker trace output missing" >&2
    exit 1
fi

echo "[tracefs] PASS: Cohsh trace commands exercised successfully"
