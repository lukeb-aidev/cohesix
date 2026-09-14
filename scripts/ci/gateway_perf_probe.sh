#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the bounded M27a gateway authority microbenchmark with no operation retries.
# Copyright 2026 Lukas Bower
set -euo pipefail
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec python3 "${script_dir}/gateway_perf_probe.py" "$@"
