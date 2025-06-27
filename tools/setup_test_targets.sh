# CLASSIFICATION: COMMUNITY
# Filename: setup_test_targets.sh v0.1
# Author: Lukas Bower
# Date Modified: 2026-09-30
#!/usr/bin/env bash
set -euo pipefail

rustup target add aarch64-unknown-linux-gnu 2>/dev/null || true
rustup target add x86_64-unknown-linux-gnu 2>/dev/null || true
