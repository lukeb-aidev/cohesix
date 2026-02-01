"""Helpers for Cohesix Python examples."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
from typing import Optional

from cohesix.audit import CohesixAudit
from cohesix.backends import FilesystemBackend, MockBackend, TcpBackend
from cohesix.defaults import DEFAULTS


def add_backend_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--mock", action="store_true", help="use mock backend")
    parser.add_argument(
        "--mock-root",
        type=Path,
        default=None,
        help="mock filesystem root (default: out/examples/mockfs)",
    )
    parser.add_argument(
        "--mount-root",
        type=Path,
        default=None,
        help="filesystem root from coh mount (uses FilesystemBackend)",
    )
    parser.add_argument("--tcp-host", default="127.0.0.1")
    parser.add_argument("--tcp-port", type=int, default=31337)
    parser.add_argument(
        "--auth-token",
        default=None,
        help="console auth token (default: COH_AUTH_TOKEN/COHSH_AUTH_TOKEN/changeme)",
    )
    parser.add_argument("--role", default="queen")
    parser.add_argument("--ticket", default=None)
    parser.add_argument("--timeout-s", type=float, default=2.0)
    parser.add_argument("--max-retries", type=int, default=3)
    parser.add_argument("--out", type=Path, default=None, help="output root override")


def resolve_auth_token(value: Optional[str]) -> str:
    if value:
        trimmed = value.strip()
        if trimmed:
            return trimmed
    for env_var in ("COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN"):
        env_val = os.environ.get(env_var)
        if env_val and env_val.strip():
            return env_val.strip()
    return "changeme"


def resolve_output_root(overridden: Optional[Path]) -> Path:
    if overridden is not None:
        return overridden
    default_root = DEFAULTS.get("examples", {}).get("output_root", "out/examples")
    return Path(default_root)


def build_backend(args: argparse.Namespace, include_mig: bool = False):
    if args.mock:
        root = args.mock_root or Path("out/examples/mockfs")
        return MockBackend(root=str(root), include_mig=include_mig)
    if args.mount_root is not None:
        return FilesystemBackend(str(args.mount_root))
    auth_token = resolve_auth_token(args.auth_token)
    return TcpBackend(
        host=args.tcp_host,
        port=args.tcp_port,
        auth_token=auth_token,
        role=args.role,
        ticket=args.ticket,
        timeout_s=args.timeout_s,
        max_retries=args.max_retries,
    )


def write_audit(out_dir: Path, audit: CohesixAudit) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    audit_path = out_dir / "audit.txt"
    audit_path.write_text("\n".join(audit.lines) + "\n", encoding="utf-8")
