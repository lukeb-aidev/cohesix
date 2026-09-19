#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the compiler-owned provider discovery and conformance workflow without implicit live promotion.
# Copyright 2026 Lukas Bower
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export PYTHONPATH="$repo_root/tools/cohesix-py${PYTHONPATH:+:$PYTHONPATH}"
exec python3 "$repo_root/scripts/ci/provider_conformance_run.py" "$@"
