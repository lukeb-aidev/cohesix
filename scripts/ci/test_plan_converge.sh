#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Provide the supported entry point for non-claiming target convergence.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
exec python3 "${repo_root}/scripts/ci/test_plan_converge.py" "$@"
