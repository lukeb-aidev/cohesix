// CLASSIFICATION: COMMUNITY
// Filename: bootstrap_sel4_tools.sh v0.2
// Author: Lukas Bower
// Date Modified: 2026-02-02
#!/bin/bash
set -euo pipefail

# Ensure Python packages for seL4 tools are installed
if python3 - <<'EOF'
import sys, os
in_venv = bool(os.environ.get('VIRTUAL_ENV')) or \
    (getattr(sys, 'base_prefix', sys.prefix) != sys.prefix)
exit(0 if in_venv else 1)
EOF
then
    python3 -m pip install jinja2 pyyaml
else
    python3 -m pip install --user jinja2 pyyaml
fi
