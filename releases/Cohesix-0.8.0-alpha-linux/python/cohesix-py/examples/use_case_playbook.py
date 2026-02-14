# Author: Lukas Bower
# Purpose: Execute built-in Cohesix orchestration playbooks from the examples directory.
# Copyright 2026 Lukas Bower

"""Run Cohesix world-class playbooks with a simple example command."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT_DIR))

from cohesix.playbook_cli import main  # noqa: E402


if __name__ == "__main__":
    main()
