#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: ci_role_runner.py v0.1
# Date Modified: 2025-07-01
# Author: Lukas Bower

"""CI helper to run tests under a specific Cohesix role."""

import argparse
import os
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--role", required=True)
    args = parser.parse_args()

    os.makedirs("/srv", exist_ok=True)
    with open("/srv/cohrole", "w") as f:
        f.write(args.role)

    subprocess.run(["cargo", "test"], check=False)


if __name__ == "__main__":
    main()
