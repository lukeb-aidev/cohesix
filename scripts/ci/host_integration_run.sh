#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run bounded Milestone 26e host-integration matrix and target-session lanes.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

exec python3 "$repo_root/scripts/ci/check_host_integration_inventory.py" \
  --repo-root "$repo_root" \
  --run \
  "$@"
