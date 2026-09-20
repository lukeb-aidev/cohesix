#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Produce the deterministic SwarmUI community capture from passing native evidence.
# Copyright 2026 Lukas Bower
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
exec python3 "$repo_root/tools/swarmui-native/capture.py" "$@"
