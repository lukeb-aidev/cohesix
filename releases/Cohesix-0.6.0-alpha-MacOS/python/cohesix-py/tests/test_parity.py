"""Parity tests for the Cohesix Python client."""

from __future__ import annotations

import json
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.audit import CohesixAudit
from cohesix.backends import MockBackend, RestBackend
from cohesix.client import CohesixClient, GpuLeaseArgs
from cohesix.ticket import TicketError, normalize_ticket


def repo_root() -> Path:
    path = Path(__file__).resolve()
    for parent in path.parents:
        if (parent / "tests" / "fixtures" / "transcripts").is_dir():
            return parent
    raise RuntimeError("repo root not found")


def load_fixture(scenario: str, name: str) -> list[str]:
    path = repo_root() / "tests" / "fixtures" / "transcripts" / scenario / name
    return [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def normalize_lines(lines: list[str]) -> list[str]:
    out: list[str] = []
    for line in lines:
        if line == "END":
            out.append(line)
            continue
        if line.startswith("OK AUTH") or line.startswith("ERR AUTH"):
            continue
        if line.startswith("OK ") or line.startswith("ERR "):
            out.append(line)
    return out


def lease_entry(state: str) -> str:
    return (
        f"{{\"schema\":\"gpu-lease/v1\",\"state\":\"{state}\",\"gpu_id\":\"GPU-0\","
        f"\"worker_id\":\"worker-1\",\"mem_mb\":1024,\"streams\":1,\"ttl_s\":60,\"priority\":1}}\n"
    )


def test_cohesix_parity_converge() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        client = CohesixClient(backend)
        audit = CohesixAudit()

        client.gpu_list(audit)
        lease_args = GpuLeaseArgs(
            gpu_id="GPU-0",
            mem_mb=4096,
            streams=2,
            ttl_s=120,
            priority=1,
        )
        client.gpu_lease(lease_args, audit)
        client.telemetry_pull(Path(tmp) / "telemetry", audit)

        expected = load_fixture("converge_v0", "coh.txt")
        assert normalize_lines(audit.lines) == expected


def test_cohesix_parity_rest_backend() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        server, base_url = start_rest_server(backend)
        try:
            client = CohesixClient(RestBackend(base_url))
            audit = CohesixAudit()

            client.gpu_list(audit)
            lease_args = GpuLeaseArgs(
                gpu_id="GPU-0",
                mem_mb=4096,
                streams=2,
                ttl_s=120,
                priority=1,
            )
            client.gpu_lease(lease_args, audit)
            client.telemetry_pull(Path(tmp) / "telemetry", audit)

            expected = load_fixture("converge_v0", "coh.txt")
            assert normalize_lines(audit.lines) == expected
        finally:
            server.shutdown()
            server.server_close()


def test_cohesix_parity_run_demo() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        client = CohesixClient(backend)
        audit = CohesixAudit()

        lease_path = "/gpu/GPU-0/lease"
        payload = lease_entry("ACTIVE").encode("utf-8")
        written = backend.write_append(lease_path, payload)
        audit.push_ack("OK", "ECHO", f"path={lease_path} bytes={written}")

        client.run_command("GPU-0", ["echo", "ok"], audit)

        status_path = "/gpu/GPU-0/status"
        _ = backend.read_file(status_path, 65536)
        audit.push_ack("OK", "CAT", f"path={status_path}")

        payload = lease_entry("RELEASED").encode("utf-8")
        written = backend.write_append(lease_path, payload)
        audit.push_ack("OK", "ECHO", f"path={lease_path} bytes={written}")

        expected = load_fixture("run_demo_v0", "cohsh.txt")
        assert normalize_lines(audit.lines) == expected


def test_cohesix_parity_peft_roundtrip() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        backend = MockBackend(root=tmp)
        client = CohesixClient(backend)
        audit = CohesixAudit()

        export_out = Path(tmp) / "export"
        adapter_dir = Path(tmp) / "adapter"
        registry_root = Path(tmp) / "registry"
        adapter_dir.mkdir(parents=True, exist_ok=True)
        registry_root.mkdir(parents=True, exist_ok=True)

        (adapter_dir / "adapter.safetensors").write_bytes(b"adapter-bytes")
        (adapter_dir / "lora.json").write_bytes(b"{\"rank\":8}")

        client.peft_export("job_8932", export_out, audit)
        model_id = "llama3-edge-v7"
        previous_model_id = "llama3-edge-v6"

        client.peft_import(
            model_id=model_id,
            adapter_dir=adapter_dir,
            export_root=export_out,
            job_id="job_8932",
            registry_root=registry_root,
            audit=audit,
        )
        client.peft_import(
            model_id=previous_model_id,
            adapter_dir=adapter_dir,
            export_root=export_out,
            job_id="job_8932",
            registry_root=registry_root,
            audit=None,
        )
        client.peft_activate(previous_model_id, registry_root, None)

        manifest_path = f"/gpu/models/available/{model_id}/manifest.toml"
        _ = backend.read_file(manifest_path, 8192)
        audit.push_ack("OK", "CAT", f"path={manifest_path}")

        client.peft_activate(model_id, registry_root, audit)
        client.peft_rollback(registry_root, audit)

        expected = load_fixture("peft_roundtrip_v0", "cohsh.txt")
        assert normalize_lines(audit.lines) == expected


def test_invalid_ticket_rejected() -> None:
    try:
        normalize_ticket("worker-gpu", "invalid-ticket", queen_validate=True)
    except TicketError as exc:
        assert "ticket" in str(exc)
    else:  # pragma: no cover
        raise AssertionError("invalid ticket was accepted")


def start_rest_server(backend: MockBackend) -> tuple[ThreadingHTTPServer, str]:
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
            parsed = urlparse(self.path)
            if parsed.path == "/v1/fs/ls":
                query = parse_qs(parsed.query)
                path = (query.get("path") or [None])[0]
                if not path:
                    return self._send_error("LS", "", "path missing", 400)
                try:
                    lines = backend.list_dir(path)
                except Exception as exc:  # pragma: no cover - indicates backend error
                    return self._send_error("LS", path, str(exc), 400)
                return self._send_ok("LS", path, lines, None)
            if parsed.path == "/v1/fs/cat":
                query = parse_qs(parsed.query)
                path = (query.get("path") or [None])[0]
                max_bytes = (query.get("max_bytes") or [None])[0]
                if not path or not max_bytes:
                    return self._send_error("CAT", path or "", "missing path or max_bytes", 400)
                try:
                    payload = backend.read_file(path, int(max_bytes))
                    lines = payload.decode("utf-8").splitlines()
                except Exception as exc:  # pragma: no cover - indicates backend error
                    return self._send_error("CAT", path, str(exc), 400)
                return self._send_ok("CAT", path, lines, len(payload))
            self.send_error(404)

        def do_POST(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
            parsed = urlparse(self.path)
            if parsed.path == "/v1/fs/echo":
                length = int(self.headers.get("Content-Length", "0") or "0")
                payload = self.rfile.read(length) if length > 0 else b"{}"
                try:
                    data = json.loads(payload.decode("utf-8"))
                except Exception:
                    return self._send_error("ECHO", "", "invalid json", 400)
                path = data.get("path") or ""
                line = data.get("line") or ""
                if not path:
                    return self._send_error("ECHO", "", "path missing", 400)
                try:
                    line = str(line)
                    payload = line + ("\n" if line else "")
                    written = backend.write_append(path, payload.encode("utf-8"))
                except Exception as exc:  # pragma: no cover - indicates backend error
                    return self._send_error("ECHO", path, str(exc), 400)
                return self._send_ok("ECHO", path, [], written)
            self.send_error(404)

        def log_message(self, *_args) -> None:  # type: ignore[override]
            return

        def _send_ok(
            self, verb: str, path: str, lines: list[str], bytes_written: int | None
        ) -> None:
            body = {
                "status": "OK",
                "verb": verb,
                "path": path,
                "end": True,
                "lines": lines,
            }
            if bytes_written is not None:
                body["bytes"] = bytes_written
            self._send_json(200, body)

        def _send_error(self, verb: str, path: str, error: str, status: int) -> None:
            body = {
                "status": "ERR",
                "verb": verb,
                "path": path,
                "end": True,
                "lines": [],
                "error": error,
            }
            self._send_json(status, body)

        def _send_json(self, status: int, body: dict) -> None:
            payload = json.dumps(body).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    base_url = f"http://127.0.0.1:{server.server_port}"
    return server, base_url
