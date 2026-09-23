# Author: Lukas Bower
# Purpose: Assemble an owner-reviewed CUDA recipe from enrolled stage files and validate it with the installed host CLI.
# Copyright 2026 Lukas Bower

"""Plan a bounded recipe; stage files carry tickets and evidence references, not secrets.

Run the existing cohesix.playbook_cli lifecycle to apply, watch, verify or recover.
Native workload inputs are produced by the pinned GPU provider before admission.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from cohesix.playbooks import execute_workflow


def main() -> None:
    """Create one immutable deployment file and its private durable journal."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--coh", type=Path, required=True)
    parser.add_argument("--operation-id", required=True)
    parser.add_argument("--controller", required=True)
    parser.add_argument("--target-hive", required=True)
    parser.add_argument("--provider-host", required=True)
    parser.add_argument("--stage", type=Path, action="append", required=True)
    parser.add_argument("--journal", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    contract = execute_workflow(
        "cuda-reference", "plan", coh_binary=args.coh, recipe=True,
    )
    if not 1 <= len(args.stage) <= contract["contract"]["max_stages"]:
        parser.error("stage count exceeds the generated recipe bound")
    stages = []
    for path in args.stage:
        with path.open("rb") as stream:
            raw = stream.read(65537)
        if len(raw) > 65536:
            parser.error("stage input exceeds 65536 bytes")
        stages.append(json.loads(raw))
    deployment = {
        "schema": "cohesix-cuda-recipe/v1",
        "operation_id": args.operation_id,
        "contract_sha256": contract["contract_sha256"],
        "topology": {
            "controller": args.controller,
            "target-hive": args.target_hive,
            "provider-host": args.provider_host,
        },
        "journal": str(args.journal.absolute()),
        "stages": stages,
        "recovery": [],
    }
    # The shared Rust planner validates exact input hashes, topology, dependency
    # order, enrolled identity and bounds. This example never issues a ticket.
    with args.out.open("x", encoding="utf-8") as stream:
        json.dump(deployment, stream, indent=2, allow_nan=False)
        stream.write("\n")
    result = execute_workflow(
        "cuda-reference", "plan", coh_binary=args.coh,
        deployment=args.out, recipe=True,
    )
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
