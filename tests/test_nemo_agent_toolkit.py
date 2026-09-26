"""Check the installable NeMo kit's input and authority boundaries.

Author: Lukas Bower
Purpose: Regress selected config, credential, request and evidence refusal paths.
Copyright 2026 Lukas Bower
"""

from __future__ import annotations

import json
import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace

import pytest


KIT = Path(__file__).resolve().parents[1] / "integrations/nemo-agent-toolkit/src"
sys.path.insert(0, str(KIT))
from cohesix_nemo_kit import cli  # noqa: E402
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/ci"))
from provider_m28f_live import profiler  # noqa: E402

INSTALLER = Path(__file__).resolve().parents[1] / "scripts/install/install_nemo_agent_toolkit.py"
spec = importlib.util.spec_from_file_location("nemo_installer", INSTALLER)
assert spec and spec.loader
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


def private_file(tmp_path: Path, name: str, data: object) -> Path:
    """Write a private fixture using the kit's required file mode."""
    path = tmp_path / name
    path.write_text(json.dumps(data))
    path.chmod(0o600)
    return path


def selected_settings(tmp_path: Path, **updates: str) -> Path:
    """Build a normal loopback client configuration without real credentials."""
    data = {"gateway_url": "http://127.0.0.1:8405", "subject": "alice",
            "request_auth_ref": "env:COH_TEST_AUTH",
            "delegated_ticket_ref": "env:COH_TEST_TICKET"}
    data.update(updates)
    return private_file(tmp_path, "settings.json", data)


def test_settings_require_protected_transport_and_exact_fields(tmp_path: Path) -> None:
    """Reject remote plaintext, URL credentials and implicit authority fields."""
    assert cli.settings(selected_settings(tmp_path))["subject"] == "alice"
    for url in ("http://example.test:8405", "https://user@example.test",
                "https://example.test/path", "file:///tmp/socket"):
        with pytest.raises(ValueError):
            cli.settings(selected_settings(tmp_path, gateway_url=url))
    with pytest.raises(ValueError):
        cli.settings(selected_settings(tmp_path, subject="alice/bob"))
    data = json.loads(selected_settings(tmp_path).read_text())
    data["executor_token"] = "forbidden"
    with pytest.raises(ValueError):
        cli.settings(private_file(tmp_path, "extra.json", data))


def test_secret_ref_and_file_mode_fail_closed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Refuse public files, symlinks and malformed environment references."""
    secret_file = tmp_path / "private-token"
    secret_file.write_text("abcdefgh\n")
    secret_file.chmod(0o600)
    assert cli.secret(f"file:{secret_file}") == "abcdefgh"
    secret_file.chmod(0o644)
    with pytest.raises(ValueError):
        cli.secret(f"file:{secret_file}")
    secret_file.chmod(0o600)
    link = tmp_path / "link"
    link.symlink_to(secret_file)
    with pytest.raises(OSError):
        cli.secret(f"file:{link}")
    monkeypatch.setenv("COH_TEST_AUTH", "abcdefgh")
    assert cli.secret("env:COH_TEST_AUTH") == "abcdefgh"
    with pytest.raises(ValueError):
        cli.secret("env:bad-name")
    with pytest.raises(ValueError):
        cli.secret("literal-token")


def test_request_identity_and_no_evidence_overwrite(tmp_path: Path) -> None:
    """Require one selected immutable ticket and create-only summary output."""
    ticket = {"schema": "host-ticket/v2", "action": "gpu.workload.submit",
              "id": "cuda-01", "idempotency_key": "same-cuda-01"}
    path = private_file(tmp_path, "request.json",
                        {"scope_id": "cuda-scope", "ticket": ticket})
    assert cli.request(path) == ("cuda-scope", ticket)
    for field, value in (("id", "../other"), ("action", "systemd.restart"),
                         ("idempotency_key", "")):
        wrong = dict(ticket, **{field: value})
        with pytest.raises(ValueError):
            cli.request(private_file(tmp_path, f"wrong-{field}.json",
                                     {"scope_id": "cuda-scope", "ticket": wrong}))
    output = tmp_path / "result.json"
    cli._write_summary(output, {"state": "unknown"})
    with pytest.raises(FileExistsError):
        cli._write_summary(output, {"state": "succeeded"})
    assert json.loads(output.read_text()) == {"state": "unknown"}


def test_recovery_keeps_original_admission_and_native_state() -> None:
    """Observation cannot silently substitute another job or verify a provider."""
    recovered = {"record": {"binding": {"admission_id": "job-1",
                                           "ticket_id": "job-1"},
                            "execution": "confirmed", "delivery": "acknowledged",
                            "result_sha256": "a" * 64},
                 "effect_replay_allowed": False}
    report = cli.recovery_summary("job-1", recovered, "observation_only")
    assert report["state"] == "confirmed"
    assert report["effect_replay_allowed"] is False
    assert report["result_sha256"] == "a" * 64
    recovered["record"]["binding"]["ticket_id"] = "other"
    with pytest.raises(ValueError):
        cli.recovery_summary("job-1", recovered, "observation_only")


def test_installer_hashes_bounded_regular_artifacts(tmp_path: Path) -> None:
    """Refuse mutable links and oversized wheel or lock inputs before installation."""
    artifact = tmp_path / "artifact.whl"
    artifact.write_bytes(b"candidate")
    assert installer.bounded_sha256(artifact, 9) == (
        "dda18a0e21ae47c53b4309434cbc02ae8bf764fa83a6defbb719431242722aa7"
    )
    with pytest.raises(ValueError):
        installer.bounded_sha256(artifact, 8)
    link = tmp_path / "link.whl"
    link.symlink_to(artifact)
    with pytest.raises(ValueError):
        installer.bounded_sha256(link, 9)


def test_cli_suppresses_native_exception_detail(tmp_path: Path,
                                                monkeypatch: pytest.MonkeyPatch,
                                                capsys: pytest.CaptureFixture[str]) -> None:
    """A native transport error cannot print sensitive server or credential detail."""
    config = selected_settings(tmp_path)
    monkeypatch.setattr(sys, "argv", ["cohesix-nemo", "mcp", "--settings", str(config)])
    monkeypatch.setattr(cli, "versions", lambda: {})

    async def failed(*_args: object) -> None:
        raise RuntimeError("private-gateway-token")

    monkeypatch.setattr(cli, "mcp_workflow", failed)
    with pytest.raises(SystemExit) as exited:
        cli.main()
    assert exited.value.code == 2
    captured = capsys.readouterr()
    assert "private-gateway-token" not in captured.err
    assert "RuntimeError" in captured.err


def test_native_evaluation_uses_private_fixed_dataset(tmp_path: Path,
                                                      monkeypatch: pytest.MonkeyPatch) -> None:
    """Run native eval only after validating fixed input and private output paths."""
    data = private_file(tmp_path, "dataset.json", [
        {"id": "case-1", "question": "Inspect original task", "answer": "completed"}
    ])
    directory = tmp_path / "eval"
    calls: list[list[str]] = []
    monkeypatch.setattr(cli, "runtime_env", lambda *_: {})

    def completed(argv: list[str], **_kwargs: object) -> SimpleNamespace:
        calls.append(argv)
        return SimpleNamespace(returncode=0)

    monkeypatch.setattr(cli.subprocess, "run", completed)
    report = cli.run_evaluation("eval-a2a", {"subject": "alice"}, {}, data, directory)
    assert report["state"] == "evaluation_exited"
    assert "a2a-agent.yaml" in calls[0][3]
    assert calls[0][-1] == "alice"
    assert directory.stat().st_mode & 0o077 == 0
    assert (directory / "nat-eval.log").stat().st_mode & 0o077 == 0
    with pytest.raises(ValueError):
        cli.run_evaluation("eval-direct", {"subject": "alice"}, {}, data, directory)
    bad = private_file(tmp_path, "bad.json", [{"id": "../other", "question": "q", "answer": "a"}])
    with pytest.raises(ValueError):
        cli.run_evaluation("eval-mcp", {"subject": "alice"}, {}, bad, tmp_path / "bad-eval")
    assert not (tmp_path / "bad-eval").exists()


def test_profiler_requires_native_completion_and_keeps_tool_identity(tmp_path: Path) -> None:
    """A text trace alone cannot stand in for the Toolkit's completed profile."""
    path = tmp_path / ".tmp/nat/examples/default/standardized_data_all.csv"
    path.parent.mkdir(parents=True)
    path.write_text("event_type,event_timestamp,tool_name,llm_text_output\n"
                    "WORKFLOW_START,1,,\n"
                    "TOOL_START,1.1,cohesix_tasks__get_task,\n"
                    "TOOL_END,1.2,cohesix_tasks__get_task,\n"
                    "WORKFLOW_END,2,,completed\n")
    seconds, tool_seconds, tools, outputs = profiler(tmp_path)
    assert seconds == 1.0
    assert tool_seconds == pytest.approx(0.1)
    assert tools == {"cohesix_tasks__get_task"}
    assert outputs == ["completed"]
    path.write_text("event_type,event_timestamp,tool_name,llm_text_output\n"
                    "WORKFLOW_START,1,,\n")
    with pytest.raises(ValueError):
        profiler(tmp_path)
