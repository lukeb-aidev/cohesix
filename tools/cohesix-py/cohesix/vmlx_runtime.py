# Author: Lukas Bower
# Purpose: Serve a content-bound disposable MLX model through the installed signed vMLX engine on loopback.
# Copyright 2026 Lukas Bower
"""Optional local serving transport. Cohesix admission and release remain upstream."""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import socket
import stat
import subprocess
import time

from .mlx_native import MAX_MODEL_FILE_BYTES, MlxRefusal, tree_digest
from .vmlx_compat import VmlxClient, VmlxRefusal, VmlxReply


_HASH = re.compile(r"[0-9a-f]{64}\Z")
_ENGINE = Path("Contents/Resources/bundled-python/python/bin/vmlx-serve")
_PROVENANCE = Path("Contents/Resources/bundled-python/vmlx-bundle-provenance.json")
_MODEL_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")


class VmlxRuntimeRefusal(ValueError):
    """The selected local app, model, process or serving bytes changed."""


def _require(condition: bool, reason: str) -> None:
    if not condition:
        raise VmlxRuntimeRefusal(reason)


def _digest_file(path: Path, maximum: int) -> str:
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    except OSError as error:
        raise VmlxRuntimeRefusal("vmlx_file_unavailable") from error
    try:
        info = os.fstat(descriptor)
        _require(stat.S_ISREG(info.st_mode) and info.st_size <= maximum,
                 "vmlx_file_kind_or_size")
        digest = hashlib.sha256()
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            while part := stream.read(1024 * 1024):
                digest.update(part)
        return digest.hexdigest()
    finally:
        os.close(descriptor)


def _private_directory(path: Path) -> None:
    _require(path.is_absolute() and path.is_dir()
             and not any(part.is_symlink() for part in (path, *path.parents))
             and path.stat().st_mode & 0o077 == 0,
             "vmlx_private_staging_required")


def _served_digest(directory: Path) -> str:
    """Hash model bytes after vMLX repair, excluding only its empty lock file."""
    _private_directory(directory)
    files = sorted(directory.iterdir())
    lock = directory / ".vmlx-alignment.lock"
    if lock in files:
        info = lock.stat(follow_symlinks=False)
        _require(stat.S_ISREG(info.st_mode) and info.st_size == 0,
                 "vmlx_alignment_lock_invalid")
        files.remove(lock)
    _require(1 <= len(files) <= 32, "vmlx_served_file_count")
    digest = hashlib.sha256()
    for path in files:
        info = path.stat(follow_symlinks=False)
        _require(_MODEL_NAME.fullmatch(path.name) is not None
                 and stat.S_ISREG(info.st_mode)
                 and info.st_size <= MAX_MODEL_FILE_BYTES,
                 "vmlx_served_file_invalid")
        digest.update(len(path.name).to_bytes(2, "big"))
        digest.update(path.name.encode())
        digest.update(info.st_size.to_bytes(8, "big"))
        digest.update(bytes.fromhex(_digest_file(path, MAX_MODEL_FILE_BYTES)))
    return digest.hexdigest()


@dataclass(frozen=True)
class VmlxSelection:
    """Predeclared installed engine and exact accepted model generation."""

    app: Path
    version: str
    engine_commit: str
    engine_sha256: str
    source_model: Path
    source_sha256: str
    stage_parent: Path
    generation: int
    port: int

    @property
    def model_id(self) -> str:
        """Use a distinct public model name for each accepted generation."""
        return f"cohesix-g{self.generation}-{self.source_sha256[:16]}"

    def validate(self) -> Path:
        """Verify public app identity, signature, bundled engine and source bytes."""
        _require(all(isinstance(path, Path) for path in (
            self.app, self.source_model, self.stage_parent)),
            "vmlx_selection_paths")
        _require(all(isinstance(value, str) for value in (
            self.version, self.engine_commit, self.engine_sha256,
            self.source_sha256)), "vmlx_selection_identity")
        _require(self.app.is_absolute() and self.app.is_dir()
                 and not any(part.is_symlink() for part in (self.app, *self.app.parents))
                 and self.app.name.endswith(".app"), "vmlx_installed_app_required")
        _require(_HASH.fullmatch(self.engine_sha256) is not None
                 and _HASH.fullmatch(self.source_sha256) is not None
                 and re.fullmatch(r"[0-9a-f]{40}", self.engine_commit) is not None
                 and re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,3}", self.version) is not None
                 and type(self.generation) is int and 0 <= self.generation < 2**32
                 and type(self.port) is int and 1024 <= self.port <= 65535,
                 "vmlx_selection_bounds")
        _private_directory(self.stage_parent)
        try:
            with (self.app / "Contents/Info.plist").open("rb") as stream:
                info = plistlib.load(stream)
            provenance = json.loads((self.app / _PROVENANCE).read_bytes())
        except (OSError, ValueError, plistlib.InvalidFileException) as error:
            raise VmlxRuntimeRefusal("vmlx_installed_metadata") from error
        _require(info.get("CFBundleIdentifier") == "net.vmlx.app"
                 and info.get("CFBundleShortVersionString") == self.version
                 and provenance.get("schema_version") == 1
                 and provenance.get("vmlx") == {
                     "version": self.version, "commit": self.engine_commit},
                 "vmlx_installed_identity_changed")
        try:
            signed = subprocess.run(
                ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(self.app)],
                capture_output=True, check=False, timeout=30,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise VmlxRuntimeRefusal("vmlx_app_signature_unavailable") from error
        _require(signed.returncode == 0, "vmlx_app_signature_invalid")
        engine = self.app / _ENGINE
        _require(not engine.is_symlink()
                 and _digest_file(engine, 16 * 1024 * 1024) == self.engine_sha256,
                 "vmlx_engine_changed")
        try:
            observed = tree_digest(self.source_model, MAX_MODEL_FILE_BYTES)
        except MlxRefusal as error:
            raise VmlxRuntimeRefusal("vmlx_source_model_invalid") from error
        _require(observed == self.source_sha256, "vmlx_source_model_changed")
        return engine


def _stage(selection: VmlxSelection) -> Path:
    """Copy ordinary local model files once; never expose enrolled source to repair."""
    destination = selection.stage_parent / selection.model_id
    destination.mkdir(mode=0o700)
    try:
        for source in sorted(selection.source_model.iterdir()):
            info = source.stat(follow_symlinks=False)
            _require(stat.S_ISREG(info.st_mode)
                     and info.st_size <= MAX_MODEL_FILE_BYTES,
                     "vmlx_source_file_invalid")
            with open(source, "rb", opener=lambda path, flags:
                      os.open(path, flags | os.O_NOFOLLOW)) as input_stream:
                with (destination / source.name).open("xb") as output_stream:
                    os.chmod(destination / source.name, 0o600)
                    shutil.copyfileobj(input_stream, output_stream, 1024 * 1024)
                    output_stream.flush()
                    os.fsync(output_stream.fileno())
        _require(tree_digest(selection.source_model, MAX_MODEL_FILE_BYTES)
                 == selection.source_sha256
                 and tree_digest(destination, MAX_MODEL_FILE_BYTES)
                 == selection.source_sha256,
                 "vmlx_staged_model_changed")
        return destination
    except Exception:
        shutil.rmtree(destination)
        raise


def _port_available(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as connection:
        connection.settimeout(0.25)
        return connection.connect_ex(("127.0.0.1", port)) != 0


class VmlxSession:
    """Own one disposable process, model copy and exact advertised generation."""

    def __init__(self, selection: VmlxSelection) -> None:
        self.selection = selection
        self.stage: Path | None = None
        self.process: subprocess.Popen[bytes] | None = None
        self.loaded_sha256: str | None = None
        self.client: VmlxClient | None = None

    def start(self, timeout_seconds: float = 60) -> VmlxSession:
        """Start only the selected signed engine on an otherwise free loopback port."""
        _require(self.process is None and 1 <= timeout_seconds <= 120,
                 "vmlx_session_state_or_timeout")
        engine = self.selection.validate()
        _require(_port_available(self.selection.port), "vmlx_port_in_use")
        self.stage = _stage(self.selection)
        args = [str(engine), "serve", str(self.stage),
                "--host", "127.0.0.1", "--port", str(self.selection.port),
                "--served-model-name", self.selection.model_id,
                "--max-num-seqs", "1", "--max-tokens", "64",
                "--max-prompt-tokens", "4096", "--text-only",
                "--tool-call-parser", "none", "--reasoning-parser", "none",
                "--log-level", "ERROR"]
        # The optional server receives no Cohesix token, provider credential,
        # user Python path or proxy setting from the parent process.
        environment = {
            "HOME": str(self.selection.stage_parent),
            "TMPDIR": str(self.selection.stage_parent),
            "XDG_CACHE_HOME": str(self.selection.stage_parent / "cache"),
            "HF_HOME": str(self.selection.stage_parent / "hf-cache"),
            "HF_HUB_OFFLINE": "1",
            "TRANSFORMERS_OFFLINE": "1",
            "PYTHONNOUSERSITE": "1",
            "PATH": "/usr/bin:/bin",
            "LANG": "en_US.UTF-8",
            "NO_PROXY": "127.0.0.1,localhost",
        }
        self.process = subprocess.Popen(
            args, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, cwd=self.selection.stage_parent,
            env=environment, close_fds=True, start_new_session=True,
        )
        self.client = VmlxClient(
            f"http://127.0.0.1:{self.selection.port}", self.selection.model_id)
        deadline = time.monotonic() + timeout_seconds
        try:
            while time.monotonic() < deadline:
                _require(self.process.poll() is None, "vmlx_engine_exited")
                try:
                    self.client.ready()
                    self.loaded_sha256 = _served_digest(self.stage)
                    return self
                except VmlxRefusal:
                    time.sleep(0.25)
            raise VmlxRuntimeRefusal("vmlx_start_timeout")
        except Exception:
            self.close()
            raise

    def generate(self, prompt: str, max_tokens: int = 32) -> VmlxReply:
        """Reject a changed process or staged model across one bounded request."""
        _require(self.process is not None and self.process.poll() is None
                 and self.stage is not None and self.client is not None
                 and self.loaded_sha256 is not None,
                 "vmlx_session_not_running")
        _require(_served_digest(self.stage)
                 == self.loaded_sha256, "vmlx_loaded_model_changed")
        reply = self.client.generate(prompt, max_tokens)
        _require(self.process.poll() is None
                 and _served_digest(self.stage)
                 == self.loaded_sha256,
                 "vmlx_loaded_model_changed")
        return reply

    def evidence(self) -> dict[str, str | int]:
        """Expose process and byte identity, never prompt or output content."""
        _require(self.process is not None and self.process.poll() is None
                 and self.loaded_sha256 is not None and self.stage is not None,
                 "vmlx_session_not_running")
        _require(_served_digest(self.stage) == self.loaded_sha256,
                 "vmlx_loaded_model_changed")
        return {"pid": self.process.pid,
                "app_version": self.selection.version,
                "engine_commit": self.selection.engine_commit,
                "engine_sha256": self.selection.engine_sha256,
                "generation": self.selection.generation,
                "model_id": self.selection.model_id,
                "source_sha256": self.selection.source_sha256,
                "loaded_sha256": self.loaded_sha256}

    def close(self) -> None:
        """Stop this engine; retain the staged copy for mutation inspection."""
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)

    def __enter__(self) -> VmlxSession:
        return self.start()

    def __exit__(self, *_args: object) -> None:
        self.close()
