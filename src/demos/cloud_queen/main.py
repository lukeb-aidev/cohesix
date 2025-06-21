# CLASSIFICATION: COMMUNITY
# Filename: main.py v0.1
# Author: Lukas Bower
# Date Modified: 2026-02-11
"""Entry point for cloud_queen demo."""

from demos.common import run_demo
from .simulation import run as run_sim


def main() -> None:
    run_sim()
    run_demo("cloud_queen")


if __name__ == "__main__":
    main()
