#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Run the provenance-bound native SwarmUI acceptance workflow over the real Tauri bridge.
# Copyright 2026 Lukas Bower
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
exec python3 "${repo_root}/tools/swarmui-native/acceptance.py" "$@"
