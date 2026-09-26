#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Invoke the canonical Ubuntu ARM64 native package builder from its repository location.
# Copyright 2026 Lukas Bower
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "${script_dir}/build_ubuntu_arm64_deb.py" "$@"
