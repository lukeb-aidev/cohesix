# CLASSIFICATION: COMMUNITY
# Filename: main.py v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
"""Entry point for bee_learns demo."""

from demos.common import run_demo
from .simulation import run as run_sim


def main() -> None:
    run_sim()
    run_demo("bee_learns")


if __name__ == "__main__":
    main()
