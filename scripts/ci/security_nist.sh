#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: CI hook for NIST 800-53 evidence checks.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

cd "$repo_root"

cargo run -p security-nist -- check
bash tests/security/nist_evidence_smoke.sh
