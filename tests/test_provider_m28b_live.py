# Author: Lukas Bower
# Purpose: Keep M28b live selection, outcome expectations and credential references fixed before an effect.
# Copyright 2026 Lukas Bower
"""Pure live-runner preflight checks; these do not create native or target evidence."""
from __future__ import annotations

from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts" / "ci"))
import provider_m28b_live as live  # noqa: E402


def reference(path: Path, scenario: str = "train", state: str = "succeeded",
              extra: str = "") -> Path:
    """Supply a syntactically complete private selection without any executable files."""
    hash_a = "a" * 64
    hash_b = "b" * 40
    hash_c = "c" * 64
    lines = [f'{name} = "/private/{name}"' for name in [
        "source_manifest", "coh_binary", "agent_binary", "native_python",
        "native_config", "hf_helper", "deployment", "application_client"]]
    lines += [f'{name} = "{hash_a}"' for name in [
        "target_manifest_sha256", "coh_sha256", "agent_sha256", "native_config_sha256",
        "hf_helper_sha256", "deployment_sha256", "application_client_sha256"]]
    lines += [
        'schema = "cohesix-m28b-live-reference/v1"',
        'host_profile = "jetson-orin-nano-jp7"',
        f'scenario = "{scenario}"',
        f'expected_state = "{state}"',
        f'source_commit = "{hash_b}"',
        'target_qemu_pid = 123',
        'gateway_url = "http://127.0.0.1:8080"',
        'request_auth_ref = "file:/private/auth"',
        'delegated_ticket_ref = "file:/private/ticket"',
        'wait_seconds = 1200',
        'expected_generation = 1',
        f'expected_adapter_sha256 = "{hash_c}"',
        extra,
    ]
    path.write_text("\n".join(lines))
    return path


def test_live_selection_refuses_wrong_scenario_outcome_and_unscoped_inputs(tmp_path: Path) -> None:
    good = reference(tmp_path / "good.toml")
    assert live.load_reference(good, "jetson-orin-nano-jp7", "m28b-peft-live")["scenario"] == "train"
    for scenario, state, case in [
        ("reject", "failed", "m28b-peft-live"),
        ("rollback", "succeeded", "m28b-serving-live"),
    ]:
        with pytest.raises(ValueError):
            live.load_reference(reference(tmp_path / "bad.toml", scenario, state),
                                "jetson-orin-nano-jp7", case)
    with pytest.raises(ValueError):
        live.load_reference(reference(tmp_path / "extra.toml", extra='arbitrary_command = "true"'),
                            "jetson-orin-nano-jp7", "m28b-peft-live")
    changed = good.read_text().replace("file:/private/auth", "plaintext-token")
    good.write_text(changed)
    with pytest.raises(ValueError):
        live.load_reference(good, "jetson-orin-nano-jp7", "m28b-peft-live")
