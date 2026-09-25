# Author: Lukas Bower
# Purpose: Check signed-engine selection, disposable staging and mutation refusal for optional vMLX serving.
# Copyright 2026 Lukas Bower
"""Focused host contract checks; the installed engine is verified separately."""

from __future__ import annotations

import json
from pathlib import Path
import plistlib
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cohesix.mlx_native import MAX_MODEL_FILE_BYTES, tree_digest  # noqa: E402
from cohesix.vmlx_runtime import (  # noqa: E402
    VmlxRuntimeRefusal, VmlxSelection, VmlxSession, _served_digest, _stage,
)


def selection(tmp_path: Path) -> VmlxSelection:
    model = tmp_path / "source"
    model.mkdir()
    (model / "config.json").write_text('{"model_type":"probe"}')
    (model / "model.safetensors").write_bytes(b"selected model")
    stage = tmp_path / "stage"
    stage.mkdir(mode=0o700)
    stage.chmod(0o700)
    return VmlxSelection(
        app=tmp_path / "vMLX.app", version="1.6.65",
        engine_commit="a" * 40, engine_sha256="b" * 64,
        source_model=model,
        source_sha256=tree_digest(model, MAX_MODEL_FILE_BYTES),
        stage_parent=stage, generation=4, port=18084,
    )


def test_selection_refuses_changed_app_identity_before_process(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    chosen = selection(tmp_path)
    resources = chosen.app / "Contents/Resources/bundled-python"
    resources.mkdir(parents=True)
    (chosen.app / "Contents/Info.plist").write_bytes(plistlib.dumps({
        "CFBundleIdentifier": "net.vmlx.app",
        "CFBundleShortVersionString": "changed-version",
    }))
    (resources / "vmlx-bundle-provenance.json").write_text(json.dumps({
        "schema_version": 1,
        "vmlx": {"version": "1.6.65", "commit": "a" * 40},
    }))
    monkeypatch.setattr("cohesix.vmlx_runtime.subprocess.run",
                        lambda *_args, **_kwargs:
                        pytest.fail("signature check should not run"))
    with pytest.raises(VmlxRuntimeRefusal, match="identity_changed"):
        chosen.validate()


def test_disposable_copy_preserves_selected_source(tmp_path: Path) -> None:
    chosen = selection(tmp_path)
    staged = _stage(chosen)
    assert staged != chosen.source_model
    assert tree_digest(staged, MAX_MODEL_FILE_BYTES) == chosen.source_sha256
    (staged / "model.safetensors").write_bytes(b"aligned copy")
    assert tree_digest(chosen.source_model, MAX_MODEL_FILE_BYTES) == chosen.source_sha256
    with pytest.raises(FileExistsError):
        _stage(chosen)
    (chosen.source_model / "model.safetensors").write_bytes(b"changed source")
    with pytest.raises(VmlxRuntimeRefusal, match="staged_model_changed"):
        _stage(VmlxSelection(**{
            **chosen.__dict__, "generation": 5,
        }))


def test_served_digest_only_excludes_empty_vmlx_alignment_lock(tmp_path: Path) -> None:
    chosen = selection(tmp_path)
    staged = _stage(chosen)
    assert _served_digest(staged) == chosen.source_sha256
    lock = staged / ".vmlx-alignment.lock"
    lock.touch(mode=0o600)
    assert _served_digest(staged) == chosen.source_sha256
    lock.write_bytes(b"unexpected")
    with pytest.raises(VmlxRuntimeRefusal, match="alignment_lock_invalid"):
        _served_digest(staged)
    lock.unlink()
    (staged / ".unknown").touch()
    with pytest.raises(VmlxRuntimeRefusal, match="served_file_invalid"):
        _served_digest(staged)


def test_session_refuses_model_mutation_during_request(tmp_path: Path) -> None:
    chosen = selection(tmp_path)
    session = VmlxSession(chosen)
    session.stage = _stage(chosen)
    session.loaded_sha256 = _served_digest(session.stage)

    class Running:
        def poll(self) -> None:
            return None

    class MutatingClient:
        def generate(self, _prompt: str, _tokens: int) -> object:
            assert session.stage is not None
            (session.stage / "model.safetensors").write_bytes(b"replaced")
            return object()

    session.process = Running()  # type: ignore[assignment]
    session.client = MutatingClient()  # type: ignore[assignment]
    with pytest.raises(VmlxRuntimeRefusal, match="loaded_model_changed"):
        session.generate("private prompt")


def test_server_receives_no_parent_credentials_or_proxy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    chosen = selection(tmp_path)
    monkeypatch.setenv("COHESIX_SECRET", "must-not-leak")
    monkeypatch.setenv("HTTPS_PROXY", "https://other.example")
    monkeypatch.setattr(VmlxSelection, "validate", lambda _self: Path("/bin/echo"))
    monkeypatch.setattr("cohesix.vmlx_runtime._port_available", lambda _port: True)
    captured: dict[str, object] = {}

    class Process:
        pid = 123

        def poll(self) -> None:
            return None

    class Client:
        def __init__(self, *_args: object) -> None:
            pass

        def ready(self) -> None:
            pass

    def spawn(_args: list[str], **kwargs: object) -> Process:
        captured.update(kwargs)
        return Process()

    monkeypatch.setattr("cohesix.vmlx_runtime.subprocess.Popen", spawn)
    monkeypatch.setattr("cohesix.vmlx_runtime.VmlxClient", Client)
    server = VmlxSession(chosen).start()
    environment = captured["env"]
    assert isinstance(environment, dict)
    assert "COHESIX_SECRET" not in environment
    assert "HTTPS_PROXY" not in environment
    assert environment["HF_HUB_OFFLINE"] == "1"
    assert server.evidence()["model_id"] == chosen.model_id
