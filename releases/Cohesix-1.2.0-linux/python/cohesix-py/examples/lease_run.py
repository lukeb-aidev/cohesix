"""Lease -> run -> release example for Cohesix Python client."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parents[1]
EXAMPLES_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT_DIR))
sys.path.insert(0, str(EXAMPLES_DIR))

from cohesix.audit import CohesixAudit  # noqa: E402
from cohesix.client import CohesixClient, GpuLeaseArgs, last_non_empty_line  # noqa: E402
from cohesix.defaults import DEFAULTS  # noqa: E402

from common import (  # noqa: E402
    add_backend_args,
    build_backend,
    resolve_output_root,
    write_audit,
)


def select_gpu_id(entries: list[dict[str, object]], prefer_mig: bool, default_id: str) -> str:
    ids = [entry.get("id") for entry in entries if isinstance(entry.get("id"), str)]
    mig_ids = [gpu_id for gpu_id in ids if gpu_id.startswith("MIG-")]
    if prefer_mig and mig_ids:
        return mig_ids[0]
    if ids:
        return ids[0]
    return default_id


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    add_backend_args(parser)
    parser.add_argument("--gpu", default=None, help="GPU id override")
    parser.add_argument("--prefer-mig", action="store_true", help="use MIG if available")
    parser.add_argument(
        "--include-mig", action="store_true", help="seed MIG entries in mock backend"
    )
    parser.add_argument("--mem-mb", type=int, default=4096)
    parser.add_argument("--streams", type=int, default=1)
    parser.add_argument("--ttl-s", type=int, default=120)
    parser.add_argument("--priority", type=int, default=1)
    parser.add_argument(
        "--command",
        nargs=argparse.REMAINDER,
        help="command to run (default: echo ok)",
    )
    parser.add_argument("--skip-release", action="store_true")
    args = parser.parse_args()

    backend = build_backend(args, include_mig=args.include_mig)
    client = CohesixClient(backend)
    audit = CohesixAudit()

    output_root = resolve_output_root(args.out)
    out_dir = output_root / "lease_run"
    out_dir.mkdir(parents=True, exist_ok=True)

    gpus = client.gpu_list(audit)
    (out_dir / "gpu_list.json").write_text(
        json.dumps(gpus, indent=2, sort_keys=True), encoding="utf-8"
    )

    defaults = DEFAULTS.get("examples", {})
    gpu_id = args.gpu or select_gpu_id(
        gpus, args.prefer_mig, defaults.get("gpu_id", "GPU-0")
    )

    lease_args = GpuLeaseArgs(
        gpu_id=gpu_id,
        mem_mb=args.mem_mb,
        streams=args.streams,
        ttl_s=args.ttl_s,
        priority=args.priority,
    )
    client.gpu_lease(lease_args, audit)

    lease_path = f"/gpu/{gpu_id}/lease"
    lease_bytes = backend.read_file(lease_path, 65536)
    audit.push_ack("OK", "CAT", f"path={lease_path}")
    lease_line = last_non_empty_line(lease_bytes) or ""
    (out_dir / "lease.json").write_text(lease_line + "\n", encoding="utf-8")

    command = args.command or ["echo", "ok"]
    if command and command[0] == "--":
        command = command[1:]
    client.run_command(gpu_id, command, audit)

    if not args.skip_release:
        try:
            lease_entry = json.loads(lease_line) if lease_line else {}
            worker_id = lease_entry.get("worker_id")
            if isinstance(worker_id, str) and worker_id:
                client.queen_kill(worker_id, audit)
        except Exception:
            pass

    write_audit(out_dir, audit)


if __name__ == "__main__":
    main()
