# CLASSIFICATION: COMMUNITY
# Filename: run_oss_audit.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12

#!/usr/bin/env bash
set -euo pipefail
python -m pip install --quiet tomli
python -m tools.oss_audit.scan "$@"
