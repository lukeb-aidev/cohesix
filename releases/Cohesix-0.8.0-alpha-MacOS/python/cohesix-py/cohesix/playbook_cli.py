# Author: Lukas Bower
# Purpose: Provide a frictionless CLI for Cohesix world-class orchestration playbooks.
# Copyright 2026 Lukas Bower

"""CLI entrypoint for Cohesix orchestration playbooks."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from typing import Optional

from .audit import CohesixAudit
from .backends import FilesystemBackend, MockBackend, RestBackend, TcpBackend
from .orchestration import CohesixOrchestrator
from .playbooks import describe_playbooks, execute_playbook, load_playbook, playbook_ids


def _resolve_auth_token(value: Optional[str]) -> str:
    if value and value.strip():
        return value.strip()
    for env_name in ("COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN"):
        env_value = os.environ.get(env_name)
        if env_value and env_value.strip():
            return env_value.strip()
    return "changeme"


def _build_backend(args: argparse.Namespace):
    if args.mock:
        return MockBackend(root=str(args.mock_root), include_mig=args.include_mig)
    if args.mount_root is not None:
        return FilesystemBackend(str(args.mount_root))
    if args.rest_url:
        return RestBackend(
            args.rest_url,
            timeout_s=args.timeout_s,
            request_auth_token=args.auth_token,
        )
    return TcpBackend(
        host=args.tcp_host,
        port=args.tcp_port,
        auth_token=_resolve_auth_token(args.auth_token),
        role=args.role,
        ticket=args.ticket,
        timeout_s=args.timeout_s,
        max_retries=args.max_retries,
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--list",
        action="store_true",
        help="list available built-in playbooks and exit",
    )
    parser.add_argument(
        "--playbook",
        default=playbook_ids()[0],
        help="playbook id to execute",
    )
    parser.add_argument("--dry-run", action="store_true", help="validate without writes")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("out/examples/playbooks"),
        help="output directory for report and audit artifacts",
    )

    parser.add_argument("--mock", action="store_true", help="use deterministic mock backend")
    parser.add_argument(
        "--mock-root",
        type=Path,
        default=Path("out/examples/mockfs"),
        help="mock backend filesystem root",
    )
    parser.add_argument("--include-mig", action="store_true", help="seed MIG mock GPU entries")
    parser.add_argument(
        "--mount-root",
        type=Path,
        default=None,
        help="mounted Secure9P root for filesystem backend",
    )
    parser.add_argument(
        "--rest-url",
        default=None,
        help="hive-gateway base URL (RestBackend)",
    )
    parser.add_argument("--tcp-host", default="127.0.0.1", help="TCP console host")
    parser.add_argument("--tcp-port", type=int, default=31337, help="TCP console port")
    parser.add_argument(
        "--auth-token",
        default=None,
        help="auth token override (TCP AUTH token or REST request auth token)",
    )
    parser.add_argument("--role", default="queen", help="attach role for TCP backend")
    parser.add_argument("--ticket", default=None, help="capability ticket payload")
    parser.add_argument("--timeout-s", type=float, default=2.0, help="transport timeout")
    parser.add_argument("--max-retries", type=int, default=3, help="transport retry count")

    parser.add_argument(
        "--no-proc-snapshot",
        action="store_true",
        help="skip reading /proc schedule and lease snapshots",
    )
    parser.add_argument(
        "--no-host-snapshot",
        action="store_true",
        help="skip host integration probes",
    )
    parser.add_argument(
        "--no-push-host-snapshot",
        action="store_true",
        help="do not push host snapshot to telemetry",
    )
    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()

    if args.list:
        print(json.dumps(describe_playbooks(), indent=2, sort_keys=True))
        return

    playbook = load_playbook(args.playbook)
    backend = _build_backend(args)
    orchestrator = CohesixOrchestrator(backend=backend)
    audit = CohesixAudit()

    try:
        report = execute_playbook(
            orchestrator=orchestrator,
            playbook=playbook,
            dry_run=args.dry_run,
            include_proc_snapshot=not args.no_proc_snapshot,
            include_host_snapshot=not args.no_host_snapshot,
            push_host_snapshot=not args.no_push_host_snapshot,
            audit=audit,
        )
    finally:
        orchestrator.close()

    out_dir = args.out / playbook.playbook_id
    out_dir.mkdir(parents=True, exist_ok=True)
    report_path = out_dir / "report.json"
    report_path.write_text(
        json.dumps(report.to_dict(), indent=2, sort_keys=True),
        encoding="utf-8",
    )
    audit_path = out_dir / "audit.txt"
    audit_path.write_text("\n".join(audit.lines) + ("\n" if audit.lines else ""), encoding="utf-8")
    print(json.dumps({"report": str(report_path), "audit": str(audit_path)}))


if __name__ == "__main__":
    main()
