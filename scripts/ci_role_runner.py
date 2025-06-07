#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: ci_role_runner.py v0.2
# Date Modified: 2025-07-03
# Author: Lukas Bower

"""CI helper to run tests under a specific Cohesix role."""

import argparse
import os
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--role", action="append")
    args = parser.parse_args()

    roles = args.role or ["QueenPrimary", "DroneWorker", "KioskInteractive"]
    for role in roles:
        os.makedirs("/srv", exist_ok=True)
        with open("/srv/cohrole", "w") as f:
            f.write(role)

        env = os.environ.copy()
        env["COHROLE"] = role
        subprocess.run(["cargo", "test"], check=False, env=env)


if __name__ == "__main__":
    main()
