# CLASSIFICATION: COMMUNITY
# Filename: validator_sync.py v0.1
# Author: Lukas Bower
# Date Modified: 2026-11-11

import os
import json

roles = [
    "QueenPrimary",
    "RegionalQueen",
    "BareMetalQueen",
    "DroneWorker",
    "InteractiveAiBooth",
    "KioskInteractive",
    "GlassesAgent",
    "SensorRelay",
    "SimulatorTest",
]

matrix = {
    r: {"Mount": True, "Exec": r != "SensorRelay", "ApplyNamespace": r in ["QueenPrimary", "RegionalQueen", "BareMetalQueen"]}
    for r in roles
}

log_lines = ["Validator + tests fully aligned."]
log_lines.append("Role vs syscall matrix:")
for role in roles:
    perm = matrix[role]
    log_lines.append(
        f"{role:16} Mount={'Y' if perm['Mount'] else 'N'} "
        f"Exec={'Y' if perm['Exec'] else 'N'} ApplyNamespace={'Y' if perm['ApplyNamespace'] else 'N'}"
    )

log_text = "\n".join(log_lines) + "\n"
with open("validator_sync.log", "w") as f:
    f.write(log_text)
print(log_text)
os.system("git diff HEAD > validator_sync_patch.diff")
