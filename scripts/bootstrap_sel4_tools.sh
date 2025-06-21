// CLASSIFICATION: COMMUNITY
// Filename: bootstrap_sel4_tools.sh v0.1
// Author: Lukas Bower
// Date Modified: 2026-01-27
#!/usr/bin/env bash
set -euo pipefail

# Ensure Python packages for seL4 tools are installed
python3 -m pip install --user jinja2 pyyaml
