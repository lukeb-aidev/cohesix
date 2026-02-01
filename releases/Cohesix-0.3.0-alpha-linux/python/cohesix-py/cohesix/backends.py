"""Backend implementations for Cohesix Python client."""

from __future__ import annotations

import json
import os
import socket
import struct
from pathlib import Path
from typing import List, Optional

from .defaults import DEFAULTS
from .errors import CohesixError
from .paths import MAX_PATH_LEN, join_root, validate_path
from .ticket import normalize_role, normalize_ticket

_CONSOLE = DEFAULTS.get("console", {})
_SECURE9P = DEFAULTS.get("secure9p", {})
MAX_LINE_LEN = int(_CONSOLE.get("max_line_len", 256))
MAX_ECHO_LEN = int(_CONSOLE.get("max_echo_len", 128))
MAX_FRAME_LEN = int(_SECURE9P.get("msize", 8192))


class Backend:
    def list_dir(self, path: str) -> List[str]:
        raise NotImplementedError

    def read_file(self, path: str, max_bytes: int) -> bytes:
        raise NotImplementedError

    def write_append(self, path: str, payload: bytes) -> int:
        raise NotImplementedError


class FilesystemBackend(Backend):
    """Filesystem backend operating on a mounted Secure9P namespace."""

    def __init__(self, root: str) -> None:
        self.root = os.path.abspath(root)

    def _resolve(self, path: str) -> str:
        _ = validate_path(path)
        resolved = join_root(self.root, path)
        resolved = os.path.abspath(resolved)
        if not resolved.startswith(self.root):
            raise CohesixError("path escapes mount root")
        return resolved

    def list_dir(self, path: str) -> List[str]:
        resolved = self._resolve(path)
        if not os.path.isdir(resolved):
            raise CohesixError(f"{path} is not a directory")
        entries = sorted(os.listdir(resolved))
        return [entry for entry in entries if entry]

    def read_file(self, path: str, max_bytes: int) -> bytes:
        resolved = self._resolve(path)
        if not os.path.isfile(resolved):
            raise CohesixError(f"{path} is not a file")
        with open(resolved, "rb") as handle:
            data = handle.read(max_bytes + 1)
        if len(data) > max_bytes:
            raise CohesixError(f"read {path} exceeds max bytes {max_bytes}")
        return data

    def write_append(self, path: str, payload: bytes) -> int:
        resolved = self._resolve(path)
        os.makedirs(os.path.dirname(resolved), exist_ok=True)
        with open(resolved, "ab") as handle:
            handle.write(payload)
        return len(payload)


class TcpBackend(Backend):
    """TCP console backend using the cohsh-core console grammar."""

    def __init__(
        self,
        host: str,
        port: int,
        auth_token: str,
        role: str,
        ticket: Optional[str],
        timeout_s: float = 2.0,
        max_retries: int = 3,
    ) -> None:
        self.host = host
        self.port = port
        self.auth_token = auth_token
        self.role = normalize_role(role)
        self.ticket = normalize_ticket(self.role, ticket, queen_validate=True)
        self.timeout_s = timeout_s
        self.max_retries = max_retries
        self._sock: Optional[socket.socket] = None
        self._connect()

    def close(self) -> None:
        if self._sock is not None:
            try:
                self._sock.close()
            finally:
                self._sock = None

    def _connect(self) -> None:
        self.close()
        sock = socket.create_connection((self.host, self.port), timeout=self.timeout_s)
        sock.settimeout(self.timeout_s)
        self._sock = sock
        self._auth()
        self._attach()

    def _send_line(self, line: str) -> None:
        if len(line) > MAX_LINE_LEN:
            raise CohesixError(f"console line exceeds {MAX_LINE_LEN} bytes")
        try:
            payload = line.encode("ascii")
        except UnicodeEncodeError as exc:
            raise CohesixError("console line must be ASCII") from exc
        total_len = len(payload) + 4
        if total_len < 4 or total_len > MAX_FRAME_LEN:
            raise CohesixError("console frame length invalid")
        frame = struct.pack("<I", total_len) + payload
        assert self._sock is not None
        self._sock.sendall(frame)

    def _recv_exact(self, size: int) -> bytes:
        assert self._sock is not None
        buf = b""
        while len(buf) < size:
            chunk = self._sock.recv(size - len(buf))
            if not chunk:
                raise CohesixError("connection closed")
            buf += chunk
        return buf

    def _recv_line(self) -> str:
        header = self._recv_exact(4)
        total_len = struct.unpack("<I", header)[0]
        if total_len < 4 or total_len > MAX_FRAME_LEN:
            raise CohesixError("invalid console frame length")
        payload_len = total_len - 4
        payload = self._recv_exact(payload_len)
        try:
            return payload.decode("utf-8").strip("\r\n")
        except UnicodeDecodeError as exc:
            raise CohesixError("console payload not UTF-8") from exc

    def _auth(self) -> None:
        self._send_line(f"AUTH {self.auth_token}")
        for _ in range(self.max_retries * 4):
            line = self._recv_line()
            if line.startswith("OK AUTH"):
                return
            if line.startswith("ERR AUTH"):
                raise CohesixError(line)
        raise CohesixError("auth timed out")

    def _attach(self) -> None:
        ticket_payload = self.ticket or ""
        self._send_line(f"ATTACH {self.role} {ticket_payload}")
        for _ in range(self.max_retries * 4):
            line = self._recv_line()
            if line.startswith("OK ATTACH"):
                return
            if line.startswith("ERR ATTACH"):
                raise CohesixError(line)
        raise CohesixError("attach timed out")

    def _stream_command(self, verb: str, path: str) -> List[str]:
        validate_path(path)
        self._send_line(f"{verb} {path}")
        lines: List[str] = []
        summary_line: Optional[str] = None
        while True:
            response = self._recv_line()
            if response.startswith("OK ") or response.startswith("ERR "):
                # ACK line
                if response.startswith(f"ERR {verb}"):
                    raise CohesixError(f"{verb} failed: {response}")
                if response.startswith(f"OK {verb}"):
                    if verb == "CAT" and summary_line is None and "data=" in response:
                        summary_line = response.split("data=", 1)[1].strip()
                continue
            if response == "END":
                if not lines and summary_line is not None:
                    lines.append(summary_line)
                return lines
            lines.append(response)

    def list_dir(self, path: str) -> List[str]:
        return self._stream_command("LS", path)

    def read_file(self, path: str, max_bytes: int) -> bytes:
        lines = self._stream_command("CAT", path)
        data = "\n".join(lines).encode("utf-8")
        if len(data) > max_bytes:
            raise CohesixError(f"read {path} exceeds max bytes {max_bytes}")
        return data

    def write_append(self, path: str, payload: bytes) -> int:
        validate_path(path)
        try:
            payload_str = payload.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise CohesixError("payload must be UTF-8") from exc
        trimmed = payload_str.rstrip("\n")
        if "\n" in trimmed or "\r" in trimmed:
            raise CohesixError("echo payload must be a single line")
        if len(trimmed.encode("utf-8")) > MAX_ECHO_LEN:
            raise CohesixError(f"echo payload exceeds {MAX_ECHO_LEN} bytes")
        if trimmed:
            line = f"ECHO {path} {trimmed}"
        else:
            line = f"ECHO {path}"
        self._send_line(line)
        while True:
            response = self._recv_line()
            if response.startswith("OK ECHO"):
                return len(payload)
            if response.startswith("ERR ECHO"):
                raise CohesixError(response)


class MockBackend(FilesystemBackend):
    """Deterministic mock backend for tests and examples."""

    def __init__(self, root: Optional[str] = None, include_mig: bool = False) -> None:
        if root is None:
            root = os.path.join("out", "examples", "mockfs")
        super().__init__(root)
        self._telemetry_counts: dict[str, int] = {}
        self._include_mig = include_mig
        self._next_worker_id = 1
        self._gpu_for_worker: dict[str, str] = {}
        self._worker_for_gpu: dict[str, str] = {}
        self._seed()

    def _seed(self) -> None:
        root = Path(self.root)
        (root / "gpu" / "GPU-0").mkdir(parents=True, exist_ok=True)
        (root / "gpu" / "GPU-1").mkdir(parents=True, exist_ok=True)
        gpu_info = {
            "id": "GPU-0",
            "name": "MockGPU",
            "memory_mb": 8192,
            "sm_count": 80,
            "driver_version": "mock",
            "runtime_version": "mock",
        }
        (root / "gpu" / "GPU-0" / "info").write_text(
            json.dumps(gpu_info), encoding="utf-8"
        )
        gpu_info["id"] = "GPU-1"
        (root / "gpu" / "GPU-1" / "info").write_text(
            json.dumps(gpu_info), encoding="utf-8"
        )
        (root / "gpu" / "GPU-0" / "status").touch()
        (root / "gpu" / "GPU-0" / "lease").touch()
        (root / "gpu" / "GPU-1" / "status").touch()
        (root / "gpu" / "GPU-1" / "lease").touch()

        if self._include_mig:
            mig_dir = root / "gpu" / "MIG-0"
            mig_dir.mkdir(parents=True, exist_ok=True)
            mig_info = {
                "id": "MIG-0",
                "name": "MockMIG",
                "memory_mb": 1024,
                "sm_count": 14,
                "driver_version": "mock",
                "runtime_version": "mock",
            }
            (mig_dir / "info").write_text(json.dumps(mig_info), encoding="utf-8")
            (mig_dir / "status").touch()
            (mig_dir / "lease").touch()

        export_root = root / "queen" / "export" / "lora_jobs" / "job_8932"
        export_root.mkdir(parents=True, exist_ok=True)
        (export_root / "telemetry.cbor").write_bytes(b"telemetry-v1\n")
        (export_root / "base_model.ref").write_text("vision-base-v1\n", encoding="utf-8")
        (export_root / "policy.toml").write_text('[policy]\nname = "default"\n', encoding="utf-8")

        registry_manifest = (
            root
            / "gpu"
            / "models"
            / "available"
            / "llama3-edge-v7"
            / "manifest.toml"
        )
        registry_manifest.parent.mkdir(parents=True, exist_ok=True)
        registry_manifest.write_text("[model]\nid=\"llama3-edge-v7\"\n", encoding="utf-8")

        telemetry_path = root / "queen" / "telemetry" / "device-1" / "seg"
        telemetry_path.mkdir(parents=True, exist_ok=True)
        seg_path = telemetry_path / "seg-000001"
        seg_path.write_text("{\"seq\":1}\n", encoding="utf-8")
        latest_path = root / "queen" / "telemetry" / "device-1" / "latest"
        latest_path.parent.mkdir(parents=True, exist_ok=True)
        latest_path.write_text("seg-000001\n", encoding="utf-8")
        self._telemetry_counts["device-1"] = 1

    def write_append(self, path: str, payload: bytes) -> int:
        # Special-case telemetry ctl for deterministic segment creation.
        if path.startswith("/queen/telemetry/") and path.endswith("/ctl"):
            parts = path.split("/")
            if len(parts) >= 4:
                device_id = parts[3]
                count = self._telemetry_counts.get(device_id, 0) + 1
                self._telemetry_counts[device_id] = count
                seg_id = f"seg-{count:06d}"
                base = Path(self.root) / "queen" / "telemetry" / device_id
                (base / "seg").mkdir(parents=True, exist_ok=True)
                (base / "latest").write_text(f"{seg_id}\n", encoding="utf-8")
                (base / "seg" / seg_id).touch()
        if path == "/queen/ctl":
            self._handle_ctl(payload)
        return super().write_append(path, payload)

    def _handle_ctl(self, payload: bytes) -> None:
        try:
            text = payload.decode("utf-8").strip()
            if text.endswith("\n"):
                text = text[:-1]
            data = json.loads(text)
        except Exception:
            return
        if "kill" in data:
            worker_id = data.get("kill")
            if isinstance(worker_id, str):
                gpu_id = self._gpu_for_worker.pop(worker_id, None)
                if gpu_id:
                    self._worker_for_gpu.pop(gpu_id, None)
                    lease_path = Path(self.root) / "gpu" / gpu_id / "lease"
                    entry = {
                        "schema": "gpu-lease/v1",
                        "state": "RELEASED",
                        "gpu_id": gpu_id,
                        "worker_id": worker_id,
                        "mem_mb": 1024,
                        "streams": 1,
                        "ttl_s": 0,
                        "priority": 1,
                    }
                    lease_path.write_text(json.dumps(entry) + "\n", encoding="utf-8")
            return
        if data.get("spawn") != "gpu":
            return
        lease = data.get("lease", {})
        gpu_id = lease.get("gpu_id")
        if not gpu_id:
            return
        worker_id = f"worker-{self._next_worker_id}"
        self._next_worker_id += 1
        self._gpu_for_worker[worker_id] = gpu_id
        self._worker_for_gpu[gpu_id] = worker_id
        lease_path = Path(self.root) / "gpu" / gpu_id / "lease"
        lease_path.parent.mkdir(parents=True, exist_ok=True)
        entry = {
            "schema": "gpu-lease/v1",
            "state": "ACTIVE",
            "gpu_id": gpu_id,
            "worker_id": worker_id,
            "mem_mb": lease.get("mem_mb", 1024),
            "streams": lease.get("streams", 1),
            "ttl_s": lease.get("ttl_s", 60),
            "priority": lease.get("priority", 1),
        }
        lease_path.write_text(json.dumps(entry) + "\n", encoding="utf-8")
