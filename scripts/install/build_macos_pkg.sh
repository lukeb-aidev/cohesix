#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Invoke the canonical Apple Silicon package builder with credentials held outside the payload.
# Copyright 2026 Lukas Bower
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "${script_dir}/build_macos_pkg.py" "$@"
