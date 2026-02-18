"""Telemetry push + pull example."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parents[1]
EXAMPLES_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT_DIR))
sys.path.insert(0, str(EXAMPLES_DIR))

from cohesix.audit import CohesixAudit  # noqa: E402
from cohesix.client import CohesixClient  # noqa: E402
from cohesix.defaults import DEFAULTS  # noqa: E402

from common import (  # noqa: E402
    add_backend_args,
    build_backend,
    resolve_output_root,
    resolve_auth_token,
    write_audit,
)


def load_payload(args: argparse.Namespace) -> str:
    if args.payload_file is not None:
        return args.payload_file.read_text(encoding="utf-8")
    if args.payload:
        return "\n".join(args.payload)
    return "telemetry demo line 1\ntelemetry demo line 2\n"


def run_coh_pull(args: argparse.Namespace, out_dir: Path) -> None:
    if args.coh_bin is None:
        return
    cmd = [
        str(args.coh_bin),
        "--role",
        args.role,
        "--host",
        args.tcp_host,
        "--port",
        str(args.tcp_port),
    ]
    if args.ticket:
        cmd.extend(["--ticket", args.ticket])
    auth_token = resolve_auth_token(args.auth_token)
    if auth_token:
        cmd.extend(["--auth-token", auth_token])
    cmd.extend(["telemetry", "pull", "--out", str(out_dir)])
    subprocess.run(cmd, check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    add_backend_args(parser)
    parser.add_argument("--device-id", default=None, help="telemetry device id")
    parser.add_argument("--mime", default="text/plain")
    parser.add_argument("--payload", action="append", help="payload line (repeatable)")
    parser.add_argument("--payload-file", type=Path, default=None)
    parser.add_argument(
        "--coh-bin",
        type=Path,
        default=None,
        help="optional coh binary to run telemetry pull",
    )
    args = parser.parse_args()

    backend = build_backend(args)
    client = CohesixClient(backend)
    audit = CohesixAudit()

    defaults = DEFAULTS.get("examples", {})
    device_id = args.device_id or defaults.get("device_id", "device-1")

    output_root = resolve_output_root(args.out)
    out_dir = output_root / "telemetry_write_pull"
    out_dir.mkdir(parents=True, exist_ok=True)

    payload = load_payload(args)
    client.telemetry_push(device_id=device_id, payload=payload, mime=args.mime, audit=audit)

    py_out = out_dir / "pull_py"
    client.telemetry_pull(py_out, audit)

    if args.coh_bin is not None and not args.mock:
        coh_out = out_dir / "pull_coh"
        coh_out.mkdir(parents=True, exist_ok=True)
        run_coh_pull(args, coh_out)

    write_audit(out_dir, audit)


if __name__ == "__main__":
    main()
