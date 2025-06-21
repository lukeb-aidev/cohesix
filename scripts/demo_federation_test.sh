# CLASSIFICATION: COMMUNITY
# Filename: demo_federation_test.sh v0.1
# Date Modified: 2025-07-07
# Author: Lukas Bower

#!/usr/bin/env bash
# Simple demo showing queen federation and agent migration
set -euo pipefail

QUEEN_A=/tmp/queen_a
QUEEN_B=/tmp/queen_b

setup_queen() {
    local dir=$1
    mkdir -p "$dir/srv/federation/known_hosts" "$dir/srv/orch" "$dir/srv/agents"
    echo "QueenPrimary" > "$dir/srv/cohrole"
}

setup_queen "$QUEEN_A"
setup_queen "$QUEEN_B"

COHROLE=QueenPrimary QUEEN_DIR=$QUEEN_A ./target/debug/cohesix-cli federation connect --peer B || true
COHROLE=QueenPrimary QUEEN_DIR=$QUEEN_B ./target/debug/cohesix-cli federation connect --peer A || true

echo "Federation setup complete"
