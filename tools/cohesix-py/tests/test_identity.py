# Author: Lukas Bower
# Purpose: Check bounded identity exchange, independent response binding, and credential redirect refusal.
# Copyright 2026 Lukas Bower
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sys
import threading

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cohesix.errors import CohesixError
from cohesix.identity import exchange_identity, local_subject, _credential, _endpoint

# Parser-only fixture, not a signed capability or external identity proof.
TICKET = "cohesix-ticket-02001e01c80107006d692d74657374c0843d000000000112002f686f73742f7469636b6574732f737065630100000000." + "00" * 32
CREDENTIAL = "aWQ.c3Vi.c2lnbmF0dXJl"
GRAPH = "a" * 64


@contextmanager
def gateway(response: dict, *, redirect: bool = False):
    seen = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            size = int(self.headers["Content-Length"])
            seen.append((self.path, dict(self.headers), json.loads(self.rfile.read(size))))
            self.send_response(307 if redirect else 200)
            if redirect:
                self.send_header("Location", "/must-not-follow")
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(response).encode())

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", seen
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def response() -> dict:
    return {
        "schema": "cohesix-identity-exchange/v1", "status": "OK",
        "authoritative": False, "identity_class": "gateway_enforced",
        "mapping_id": "operator", "provider_graph_sha256": GRAPH,
        "credential_sha256": hashlib.sha256(CREDENTIAL.encode()).hexdigest(),
        "expires_unix_s": 1200, "ticket": TICKET,
    }


def test_exchange_preserves_subject_binding_and_never_follows_redirect(monkeypatch) -> None:
    monkeypatch.setenv("M27B_IDENTITY_JWT", CREDENTIAL)
    monkeypatch.setenv("M27B_IDENTITY_AUTH", "test-enrolled-request-auth")
    args = ("operator", "env:M27B_IDENTITY_JWT", "env:M27B_IDENTITY_AUTH", GRAPH)
    with gateway(response()) as (url, seen):
        issued = exchange_identity(url, *args)
        assert issued.ticket == TICKET
        assert TICKET not in repr(issued)
        assert CREDENTIAL not in repr(issued)
        assert len(seen) == 1
        assert seen[0][0] == "/v1/identity/exchange"
        assert seen[0][2] == {"mapping_id": "operator", "credential": CREDENTIAL}
        assert seen[0][1]["X-Cohesix-Auth"] == "test-enrolled-request-auth"
        assert "X-Cohesix-Ticket" not in seen[0][1]
    with gateway(response(), redirect=True) as (url, seen):
        with pytest.raises(CohesixError, match="redirect"):
            exchange_identity(url, *args)
        assert len(seen) == 1
    for field, value in (("provider_graph_sha256", "b" * 64),
                         ("credential_sha256", "b" * 64), ("authoritative", True)):
        bad = response()
        bad[field] = value
        with gateway(bad) as (url, _seen):
            with pytest.raises(CohesixError, match="binding"):
                exchange_identity(url, *args)


def test_identity_transport_and_local_credential_bounds(tmp_path, monkeypatch) -> None:
    for url in ("http://edge.example.test", "https://user:pass@edge.example.test",
                "https://edge.example.test?credential=x", "https://edge.example.test/#x"):
        with pytest.raises(CohesixError):
            _endpoint(url)
    assert _endpoint("https://edge.example.test") == "https://edge.example.test/v1/identity/exchange"
    path = tmp_path / "credential"
    path.write_text(CREDENTIAL + "\n")
    assert _credential("file:" + str(path)) == CREDENTIAL
    link = tmp_path / "link"
    link.symlink_to(path)
    with pytest.raises(CohesixError):
        _credential("file:" + str(link))
    monkeypatch.setenv("M27B_IDENTITY_JWT", "a" * 8192 + ".b.c")
    with pytest.raises(CohesixError):
        _credential("env:M27B_IDENTITY_JWT")
    assert local_subject() == str(os.geteuid())
