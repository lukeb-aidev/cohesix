#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: CI hook for transcript convergence tests.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
"${repo_root}/scripts/regression/transcript_compare.sh"
