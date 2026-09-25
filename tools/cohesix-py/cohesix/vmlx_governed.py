# Author: Lukas Bower
# Purpose: Bind a signed Mac release generation to optional vMLX serving and reconcile exact process custody after interruption.
# Copyright 2026 Lukas Bower
"""Optional generation-fenced vMLX serving for an already verified release.

The caller must verify the signed Cohesix release result before selecting this
profile. vMLX receives no ticket, Cohesix credential, MCP tool or job endpoint.
"""

from __future__ import annotations

from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time
from typing import Any

from .hf_native import encode, keys, read_json, regular, require, sha, write
from .vmlx_runtime import VmlxSelection, VmlxSession


HASH = re.compile(r"[0-9a-f]{64}\Z")
MAX_PROFILE = 8192


def _pid_command(pid: int) -> str | None:
    require(type(pid) is int and pid > 0, "custody_pid")
    result = subprocess.run(["/bin/ps", "-ww", "-p", str(pid), "-o", "command="],
                            capture_output=True, check=False, timeout=5)
    require(len(result.stdout) <= 8192 and len(result.stderr) <= 8192,
            "custody_process_output_bound")
    return result.stdout.decode().strip() if result.returncode == 0 else None


@dataclass(frozen=True)
class GovernedSelection:
    """A release result, accepted state and fused bytes selected before serve."""

    root: Path
    accepted_path: Path
    generation: int
    adapter_sha256: str | None
    served_artifact_sha256: str
    source_model: Path
    source_sha256: str
    release_graph_sha256: str
    app: Path
    version: str
    engine_commit: str
    engine_sha256: str
    stage_parent: Path
    port: int
    expected_output_sha256: tuple[str, ...]
    maximum_latency_ms: int
    maximum_rss_bytes: int

    @classmethod
    def from_profile(cls, path: Path) -> GovernedSelection:
        """Read an exact private selection, including the verified graph digest."""
        value = json.loads(regular(path, MAX_PROFILE))
        keys(value, {"schema", "root", "accepted_path", "generation",
                     "adapter_sha256", "served_artifact_sha256", "source_model",
                     "source_sha256", "release_graph_sha256", "app", "version",
                     "engine_commit", "engine_sha256", "stage_parent", "port",
                     "expected_output_sha256", "maximum_latency_ms",
                     "maximum_rss_bytes"})
        require(value["schema"] == "cohesix-vmlx-governed/v1",
                "governed_profile_schema")
        selected = cls(
            Path(value["root"]), Path(value["accepted_path"]),
            value["generation"], value["adapter_sha256"],
            value["served_artifact_sha256"], Path(value["source_model"]),
            value["source_sha256"], value["release_graph_sha256"],
            Path(value["app"]), value["version"], value["engine_commit"],
            value["engine_sha256"], Path(value["stage_parent"]), value["port"],
            tuple(value["expected_output_sha256"]),
            value["maximum_latency_ms"], value["maximum_rss_bytes"])
        selected.validate()
        return selected

    @property
    def model_id(self) -> str:
        return f"cohesix-g{self.generation}-{self.source_sha256[:16]}"

    @property
    def custody_path(self) -> Path:
        return self.root / "vmlx-custody.json"

    def native(self) -> VmlxSelection:
        return VmlxSelection(self.app, self.version, self.engine_commit,
                             self.engine_sha256, self.source_model,
                             self.source_sha256, self.stage_parent,
                             self.generation, self.port)

    def validate(self) -> None:
        """Refuse a changed accepted generation before opening the engine."""
        require(self.root.is_absolute() and self.root.is_dir()
                and not any(path.is_symlink() for path in
                            (self.root, *self.root.parents))
                and self.root.stat().st_mode & 0o077 == 0
                and self.accepted_path == self.root / "accepted.json"
                and type(self.generation) is int
                and 0 <= self.generation < 2**32
                and (self.adapter_sha256 is None or
                     HASH.fullmatch(self.adapter_sha256) is not None)
                and all(isinstance(value, str) and HASH.fullmatch(value)
                        for value in (self.served_artifact_sha256,
                                      self.source_sha256,
                                      self.release_graph_sha256,
                                      self.engine_sha256))
                and 1 <= len(self.expected_output_sha256) <= 4
                and all(HASH.fullmatch(value)
                        for value in self.expected_output_sha256)
                and type(self.maximum_latency_ms) is int
                and 100 <= self.maximum_latency_ms <= 120_000
                and type(self.maximum_rss_bytes) is int
                and 536_870_912 <= self.maximum_rss_bytes <= 12_884_901_888,
                "governed_selection_bounds")
        accepted = read_json(self.accepted_path)
        require(accepted["generation"] == self.generation
                and accepted["adapter_sha256"] == self.adapter_sha256
                and accepted["served_artifact_sha256"] ==
                self.served_artifact_sha256
                and accepted["healthy"] is True
                and accepted["rollback_verified"] is True,
                "accepted_generation_changed")
        self.native().validate()


class GovernedVmlxSession:
    """Own one signed-engine process and recheck accepted state around inference."""

    def __init__(self, selection: GovernedSelection) -> None:
        self.selection = selection
        self.session: VmlxSession | None = None

    def start(self) -> GovernedVmlxSession:
        self.selection.validate()
        require(not self.selection.custody_path.exists(),
                "unresolved_vmlx_custody")
        session = VmlxSession(self.selection.native()).start()
        try:
            self.selection.validate()
            evidence = session.evidence()
            write(self.selection.custody_path, encode({
                "schema": "cohesix-vmlx-custody/v1",
                "release_graph_sha256": self.selection.release_graph_sha256,
                "accepted_sha256": sha(regular(self.selection.accepted_path, 8192)),
                "native": evidence,
                "observed_unix_ms": int(time.time() * 1000),
            }))
            self.session = session
            return self
        except Exception:
            session.close()
            raise

    def custody(self) -> dict[str, Any]:
        require(self.session is not None, "vmlx_session_not_started")
        record = read_json(self.selection.custody_path)
        evidence = self.session.evidence()
        require(record["schema"] == "cohesix-vmlx-custody/v1"
                and record["release_graph_sha256"] ==
                self.selection.release_graph_sha256
                and record["native"] == evidence,
                "vmlx_custody_changed")
        return record

    def generate(self, prompt: str, max_tokens: int,
                 expected_index: int) -> dict[str, Any]:
        self.selection.validate()
        before = self.custody()
        require(type(expected_index) is int
                and 0 <= expected_index < len(self.selection.expected_output_sha256),
                "vmlx_quality_index")
        started = time.monotonic()
        reply = self.session.generate(prompt, max_tokens)
        elapsed = int((time.monotonic() - started) * 1000)
        self.selection.validate()
        after = self.custody()
        require(before["native"] == after["native"]
                and reply.model == self.selection.model_id
                and reply.output_sha256 ==
                self.selection.expected_output_sha256[expected_index]
                and elapsed <= self.selection.maximum_latency_ms,
                "vmlx_quality_generation_or_latency")
        pid = after["native"]["pid"]
        observed = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "rss="],
                                  capture_output=True, check=False, timeout=5)
        require(observed.returncode == 0 and observed.stdout.strip().isdigit()
                and int(observed.stdout.strip()) * 1024 <=
                self.selection.maximum_rss_bytes,
                "vmlx_resource_bound")
        return {"reply": reply.evidence(), "elapsed_ms": elapsed,
                "peak_observed_rss_bytes": int(observed.stdout.strip()) * 1024,
                "custody": after}

    def close(self) -> None:
        if self.session is None:
            return
        pid = self.session.process.pid if self.session.process else None
        self.session.close()
        require(pid is not None and _pid_command(pid) is None,
                "ambiguous_vmlx_process_stop")
        self.selection.custody_path.unlink()
        self.session = None


def recover_orphan(selection: GovernedSelection) -> dict[str, Any]:
    """Quiesce only a previously recorded, byte-bound engine after owner loss."""
    record = read_json(selection.custody_path)
    require(record["schema"] == "cohesix-vmlx-custody/v1"
            and record["release_graph_sha256"] ==
            selection.release_graph_sha256,
            "foreign_vmlx_custody")
    native = record["native"]
    require(native["engine_sha256"] == selection.engine_sha256
            and native["source_sha256"] == selection.source_sha256
            and native["generation"] == selection.generation
            and native["model_id"] == selection.model_id,
            "foreign_vmlx_process")
    pid = native["pid"]
    command = _pid_command(pid)
    engine = str(selection.native().validate())
    if command is not None:
        require(command.startswith(engine + " serve ")
                and selection.model_id in command
                and str(selection.stage_parent / selection.model_id) in command,
                "foreign_vmlx_process")
        os.kill(pid, signal.SIGTERM)
        deadline = time.monotonic() + 5
        while _pid_command(pid) is not None and time.monotonic() < deadline:
            time.sleep(0.1)
        require(_pid_command(pid) is None,
                "ambiguous_vmlx_orphan_stop")
    selection.custody_path.unlink()
    report = {"schema": "cohesix-vmlx-custody-recovery/v1",
              "release_graph_sha256": selection.release_graph_sha256,
              "generation": selection.generation,
              "pid": pid, "quiescent": True,
              "observed_unix_ms": int(time.time() * 1000)}
    write(selection.root / "vmlx-recovery.json", encode(report))
    return report
