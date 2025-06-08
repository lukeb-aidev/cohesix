# CLASSIFICATION: COMMUNITY
# Filename: test_all_arch.sh v1.0
# Author: Lukas Bower
# Date Modified: 2025-07-07

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

cargo test --workspace
GOWORK="$(pwd)/go/go.work" go test ./go/...
pytest -q
bash tests/demo_edge_failover.sh
