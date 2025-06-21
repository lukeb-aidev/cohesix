#!/bin/bash
set -euo pipefail

# Ensure Python packages for seL4 tools are installed
python3 -m pip install --user jinja2 pyyaml
