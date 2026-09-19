#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Exercise durable relay recovery between explicitly enrolled disposable hives with an observed native terminal.
# Copyright 2026 Lukas Bower
"""The target agent runs independently; this runner never invents its result."""

from __future__ import annotations

import hashlib
import http.client
import http.server
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from typing import Any
from urllib.parse import urlsplit

from cohesix.backends import RestBackend
from cohesix.providers import registry
from cohesix.auth import resolve_secret_reference


def terminal(rows: bytes, request: dict[str, Any]) -> dict[str, Any] | None:
    """Accept one exact target terminal and reject conflicting or duplicate rows."""
    if len(rows) > 131072 or len(rows.splitlines()) > 1024:
        raise ValueError("ELIMIT federation terminal observations")
    selected = []
    for line in rows.splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        if not isinstance(row, dict) or row.get("schema") != "host-ticket-result/v1":
            raise ValueError("invalid federation result schema")
        if row.get("id") != request["id"] or row.get("state") not in {
            "succeeded",
            "failed",
            "expired",
        }:
            continue
        for field in (
            "idempotency_key",
            "action",
            "writer_epoch",
            "source_hive",
            "target_hive",
        ):
            if row.get(field) != request[field]:
                raise ValueError("EPERM federation terminal correlation")
        correlation = ":".join(
            request[key]
            for key in ("id", "idempotency_key", "source_hive", "target_hive")
        )
        if row.get("relay_hop") != 1 or row.get("relay_correlation_id") != correlation:
            raise ValueError("EPERM federation relay correlation")
        selected.append(row)
    if len(selected) > 1:
        raise ValueError("EPERM duplicate federation terminal")
    return selected[0] if selected else None


def endpoint(url: str, *, relay: bool = False):
    """Credentials travel only over TLS or an explicit loopback connection."""
    parsed = urlsplit(url)
    if (
        parsed.scheme not in {"http", "https"}
        or not parsed.hostname
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
        or parsed.path not in {"", "/"}
        or (
            parsed.scheme == "http"
            and parsed.hostname not in {"127.0.0.1", "::1", "localhost"}
        )
    ):
        raise ValueError("invalid federation endpoint")
    if relay and (
        parsed.scheme != "http" or parsed.hostname != "127.0.0.1" or parsed.port is None
    ):
        raise ValueError("conformance relay peer requires explicit IPv4 loopback port")
    return parsed


def run_live(args) -> int:
    """Run three fresh source processes around one lost accepted-forward reply."""
    required = [
        "source_agent",
        "source_manifest",
        "source_policy",
        "target_manifest",
        "source_url",
        "target_url",
        "source_auth_ref",
        "source_ticket_ref",
        "target_auth_ref",
        "target_ticket_ref",
        "native_unit",
    ]
    if any(getattr(args, field, None) is None for field in required):
        raise ValueError(
            "live federation requires explicit source/target manifests, endpoints, credentials, source agent/policy and native unit"
        )
    from provider_matrix import bounded_read

    source = json.loads(bounded_read(args.source_manifest, 4 * 1024 * 1024))
    target = json.loads(bounded_read(args.target_manifest, 4 * 1024 * 1024))
    federation = source["ecosystem"]["host"]["federation"]
    target_hive = target["ecosystem"]["host"]["federation"]["local_hive"]
    if (
        not federation["enable"]
        or federation["local_hive"] == target_hive
        or "systemd.status-check" not in federation["action_allowlist"]
        or source["authority"]["writer_epoch"] != target["authority"]["writer_epoch"]
    ):
        raise ValueError("federation profiles do not admit the selected read")
    if any(not document["authority"]["production"] for document in (source, target)):
        raise ValueError(
            "live conformance requires explicitly provisioned production profiles"
        )
    peers = [peer for peer in federation["peers"] if peer["name"] == target_hive]
    if len(peers) != 1:
        raise ValueError("missing unique target peer")
    peer = peers[0]
    listener = endpoint(peer["rest_url"], relay=True)
    target_address = endpoint(args.target_url)
    endpoint(args.source_url)
    if (listener.hostname, listener.port) == (
        target_address.hostname,
        target_address.port,
    ):
        raise ValueError("proxy listener cannot be its target")
    from cohesix.providers import validate_target

    validate_target("systemd.status-check", args.native_unit)
    credentials = [
        row
        for row in registry()["contract"]["relay_credentials"]
        if row["peer"] == target_hive
    ]
    if (
        len(credentials) != 1
        or credentials[0]["request_auth_ref"] != "env:" + peer["auth_ref"]
    ):
        raise ValueError("relay credential contract mismatch")
    auth_a, ticket_a, auth_b, ticket_b = [
        resolve_secret_reference(getattr(args, field))
        for field in (
            "source_auth_ref",
            "source_ticket_ref",
            "target_auth_ref",
            "target_ticket_ref",
        )
    ]
    backends = [
        RestBackend(
            url,
            timeout_s=10,
            max_attempts=1,
            request_auth_token=auth,
            delegated_ticket=ticket,
        )
        for url, auth, ticket in [
            (args.source_url, auth_a, ticket_a),
            (args.target_url, auth_b, ticket_b),
        ]
    ]
    for backend, manifest in zip(
        backends, [args.source_manifest, args.target_manifest]
    ):
        boot = backend.read_file("/proc/boot", 8192)
        measured = [
            line.removeprefix(b"manifest.sha256=").decode()
            for line in boot.splitlines()
            if line.startswith(b"manifest.sha256=")
        ]
        if measured != [
            hashlib.sha256(bounded_read(manifest, 4 * 1024 * 1024)).hexdigest()
        ]:
            raise ValueError("target Root manifest differs from enrolled profile")
        for path in (
            "/host/tickets/spec",
            "/host/tickets/status",
            "/host/tickets/deadletter",
        ):
            if backend.read_file(path, 131072).strip():
                raise ValueError(
                    "live conformance requires disposable hives with empty ticket streams"
                )
    out = args.state_dir.resolve()
    out.mkdir(parents=True, exist_ok=False, mode=0o700)
    events: list[dict[str, Any]] = []

    class Proxy(http.server.BaseHTTPRequestHandler):
        dropped = False

        def log_message(self, *_):
            pass

        def setup(self):
            super().setup()
            self.connection.settimeout(5)

        def do_GET(self):
            self.forward()

        def do_POST(self):
            self.forward()

        def forward(self):
            size = int(self.headers.get("Content-Length", 0))
            if not 0 <= size <= 32768 or len(events) >= 64:
                self.send_error(413)
                return
            body = self.rfile.read(size)
            client_type = (
                http.client.HTTPSConnection
                if target_address.scheme == "https"
                else http.client.HTTPConnection
            )
            client = client_type(
                target_address.hostname, target_address.port, timeout=5
            )
            try:
                client.request(
                    self.command,
                    self.path,
                    body,
                    {
                        key: value
                        for key, value in self.headers.items()
                        if key.lower()
                        not in {"host", "connection", "transfer-encoding"}
                    },
                )
                response = client.getresponse()
                data = response.read(262145)
                if len(data) > 262144:
                    raise ValueError("ELIMIT peer HTTP response")
                drop = (
                    self.command == "POST"
                    and 200 <= response.status < 300
                    and not Proxy.dropped
                )
                events.append(
                    {
                        "method": self.command,
                        "status": response.status,
                        "response_dropped": drop,
                    }
                )
                if drop:
                    Proxy.dropped = True
                    self.close_connection = True
                    return
                self.send_response(response.status)
                self.send_header("Content-Length", str(len(data)))
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(data)
            finally:
                client.close()

    server = http.server.ThreadingHTTPServer((listener.hostname, listener.port), Proxy)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    environment = os.environ.copy()
    environment.update(HIVE_GATEWAY_REQUEST_AUTH_TOKEN=auth_a, COH_REST_TICKET=ticket_a)
    for key, value in [
        ("request_auth_ref", auth_b),
        ("delegated_ticket_ref", ticket_b),
    ]:
        environment[credentials[0][key].removeprefix("env:")] = value
    command = [
        str(args.source_agent.resolve()),
        "--rest-url",
        args.source_url,
        "--manifest",
        str(args.source_manifest.resolve()),
        "--policy",
        str(args.source_policy.resolve()),
        "--agent-lock",
        str(out / "agent.lock"),
        "--cursor",
        str(out / "cursor.json"),
        "--execution-journal",
        str(out / "execution.json"),
        "--provider-evidence-root",
        str(out / "observations"),
        "--relay",
        "--relay-wal",
        str(out / "relay.json"),
        "--run-once",
    ]

    def relay_pass(number: int) -> None:
        with (out / f"source-{number}.log").open("xb") as output:
            result = subprocess.run(
                command,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=output,
                stderr=subprocess.STDOUT,
                timeout=30,
                check=False,
            )
        if result.returncode:
            raise ValueError("relay source process failed")
        (out / f"source-{number}.wal.json").write_bytes(
            bounded_read(out / "relay.json", 524288)
        )

    request = {
        "schema": "host-ticket/v1",
        "id": "conformance-federation-read",
        "idempotency_key": "native-read-once",
        "action": "systemd.status-check",
        "args": {"unit": args.native_unit},
        "writer_epoch": source["authority"]["writer_epoch"],
        "receipt_mode": "none",
        "expires_unix_ms": time.time_ns() // 1000000 + 60000,
        "source_hive": federation["local_hive"],
        "target_hive": target_hive,
    }
    try:
        payload = json.dumps(request, separators=(",", ":")).encode()
        (out / "request.json").write_bytes(payload)
        backends[0].write_append("/host/tickets/spec", payload)
        relay_pass(1)
        if not Proxy.dropped:
            raise ValueError("accepted forward ACK was not dropped")
        deadline = time.monotonic() + 30
        observed = None
        while observed is None and time.monotonic() < deadline:
            rows = (
                backends[1].read_file("/host/tickets/status", 131072)
                + b"\n"
                + backends[1].read_file("/host/tickets/deadletter", 131072)
            )
            observed = terminal(rows.strip(), request)
            if observed is None:
                time.sleep(0.1)
        if observed is None or observed["state"] != "succeeded":
            raise ValueError("native target did not reach a successful terminal")
        relay_pass(2)
        relay_pass(3)
        returned = terminal(
            backends[0].read_file("/host/tickets/status", 131072), request
        )
        if (
            returned != observed
            or len([event for event in events if event["method"] == "POST"]) != 2
        ):
            raise ValueError("terminal return or deduplication mismatch")
        raw = json.dumps(observed, sort_keys=True, separators=(",", ":")).encode()
        (out / "target-terminal.json").write_bytes(raw)
        (out / "source-terminal.json").write_bytes(
            json.dumps(returned, sort_keys=True, separators=(",", ":")).encode()
        )
        summary = {
            "schema": "cohesix-federation-conformance/v1",
            "result": "PASS",
            "mode": "live_safe",
            "claiming": False,
            "production_proven": False,
            "signed_graph_proof": False,
            "worker_proof": False,
            "source_processes": 3,
            "target_terminal_sha256": hashlib.sha256(raw).hexdigest(),
            "source_agent_sha256": hashlib.sha256(
                bounded_read(args.source_agent, 512 * 1024 * 1024)
            ).hexdigest(),
            "source_manifest_sha256": hashlib.sha256(
                bounded_read(args.source_manifest, 4 * 1024 * 1024)
            ).hexdigest(),
            "target_manifest_sha256": hashlib.sha256(
                bounded_read(args.target_manifest, 4 * 1024 * 1024)
            ).hexdigest(),
            "provider_graph_sha256": registry()["graph_sha256"],
            "events": events,
            "proof_class": "observed_correlated_native_terminal_return",
            "proof_limits": [
                "Native target evidence and signed custody require their separate verifier.",
                "The runner neither creates nor signs the target terminal.",
            ],
        }
        (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(
            json.dumps(
                {
                    "result": "PASS",
                    "summary": str(out / "summary.json"),
                    "production_proven": False,
                }
            )
        )
        return 0
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        (out / "proxy-events.json").write_text(json.dumps(events, indent=2) + "\n")
