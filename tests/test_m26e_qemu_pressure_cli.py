# Author: Lukas Bower
# Purpose: Verify release-pressure cleanup ownership and approved operator command setup.
# Copyright 2026 Lukas Bower

"""Check preflight and emitted commands without cleanup, build, or QEMU."""

import ast
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
from types import SimpleNamespace

import pytest


ROOT = Path(__file__).resolve().parents[1]


def test_expired_receipt_keeps_the_admitted_worker_alive() -> None:
    """Expiry requires a live recipient; generation invalidation has its own lane."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    embedded = source.split("drive_receipt_matrix() {", 1)[1].split("<<'PY'\n", 1)[1]
    embedded = embedded.split("\nPY\n", 1)[0]
    submit = next(node for node in ast.parse(embedded).body
                  if isinstance(node, ast.FunctionDef) and node.name == "submit")
    writes = []

    def echo(path: str, line: str) -> SimpleNamespace:
        writes.append((path, json.loads(line)))
        return SimpleNamespace(status="OK")

    scope = {
        "sequence": 0, "json": json,
        "ready": lambda role: SimpleNamespace(
            worker_id="worker7", supervisor_generation=3, cap_generation=4,
        ),
        "client": SimpleNamespace(echo=echo), "run_agent": lambda: None,
        "time": SimpleNamespace(monotonic=lambda: 0),
        "terminal": lambda ticket: "expired", "records": [],
    }
    exec(compile(ast.Module(body=[submit], type_ignores=[]), "receipt-submit", "exec"), scope)
    scope["submit"]("peft.export", "worker-lora", {}, "job", "expired", "operation")
    assert len(writes) == 1
    assert writes[0][0] == "/host/tickets/spec"
    assert writes[0][1]["expires_unix_ms"] == 1
    assert writes[0][1]["receipt_worker_id"] == "worker7"
    assert writes[0][1]["receipt_supervisor_generation"] == 3
    assert writes[0][1]["receipt_cap_generation"] == 4


def test_receipt_fixture_contains_the_exported_adapters_base(tmp_path: Path) -> None:
    """The QEMU export's base_model.ref must resolve after a real PEFT import."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "prepare_host_fixture() {" + source.split(
        "prepare_host_fixture() {", 1,
    )[1].split("\ntrigger_disposable_worker_control() {", 1)[0]
    subprocess.run(
        ["bash", "-eu", "-c", function + '\nprepare_host_fixture "$1"\n',
         "fixture-test", str(tmp_path)],
        check=True, capture_output=True, text=True, timeout=10,
    )
    available = tmp_path / "peft-registry/available"
    exported_base = tomllib.loads((available / "fixture-base-model/manifest.toml").read_text())
    assert exported_base["model"]["id"] == "fixture-base-model"
    assert exported_base["model"]["format"] == "gguf"
    for path in available.glob("*/manifest.toml"):
        model = tomllib.loads(path.read_text())["model"]
        if "base" in model:
            assert (available / model["base"] / "manifest.toml").is_file()


@pytest.fixture
def checkout(tmp_path: Path) -> Path:
    """Create a disposable checkout with output that preflight must preserve."""
    repo = tmp_path.resolve() / "checkout"
    (repo / "scripts").mkdir(parents=True)
    shutil.copy2(ROOT / "scripts/m26e_qemu_pressure.sh", repo / "scripts")
    subprocess.run(["git", "init", "--quiet", str(repo)], check=True)
    for name in ("out", "target"):
        (repo / name).mkdir()
        (repo / name / "retained").write_text("keep\n", encoding="utf-8")
    (repo / "nonexecutable-qemu").write_text("not an executable\n", encoding="utf-8")
    (repo / ".gitignore").write_text("/out/\n/target/\n", encoding="utf-8")
    subprocess.run(["git", "add", "."], cwd=repo, check=True)
    subprocess.run(
        ["git", "-c", "user.name=Test Fixture", "-c",
         "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false",
         "-c", "core.hooksPath=/dev/null", "commit", "--quiet", "-m", "fixture"],
        cwd=repo, check=True,
    )
    subprocess.run(["git", "checkout", "--detach", "--quiet"], cwd=repo, check=True)
    return repo


def invoke(checkout: Path, *options: str) -> subprocess.CompletedProcess[str]:
    """Stop in preflight and verify both output sentinels after every case."""
    result = subprocess.run(
        ["bash", str(checkout / "scripts/m26e_qemu_pressure.sh"),
         "--check-only", "--qemu", str(checkout / "nonexecutable-qemu"), *options],
        cwd=checkout, text=True, capture_output=True, check=False, timeout=10,
    )
    for name in ("out", "target"):
        assert (checkout / name / "retained").read_text(encoding="utf-8") == "keep\n"
    return result


def test_unselected_checkout_cannot_enter_cleanup(checkout: Path) -> None:
    result = invoke(checkout)
    assert result.returncode == 2
    assert "refusing to clean an unexpected repository root" in result.stderr


@pytest.mark.parametrize("selection", ["parent", "relative", "alias"])
def test_clean_root_must_be_the_exact_checkout(checkout: Path, selection: str) -> None:
    selected = {"parent": str(checkout.parent), "relative": "."}.get(selection)
    if selection == "alias":
        alias = checkout.parent / "checkout-alias"
        alias.symlink_to(checkout, target_is_directory=True)
        selected = str(alias)
    assert selected is not None
    result = invoke(checkout, "--clean-root", selected)
    assert result.returncode == 2
    assert "--clean-root must equal this exact checkout root" in result.stderr


def test_selected_checkout_advances_only_to_tool_preflight(checkout: Path) -> None:
    result = invoke(checkout, "--clean-root", str(checkout))
    assert result.returncode != 0
    assert f"file is not executable: {checkout / 'nonexecutable-qemu'}" in result.stderr


def test_replay_cannot_authorize_cleanup(checkout: Path) -> None:
    result = invoke(checkout, "--clean-root", str(checkout), "--reuse-artifacts")
    assert result.returncode == 2
    assert "--clean-root cannot be used with --reuse-artifacts" in result.stderr


@pytest.mark.parametrize(("arguments", "expected"), [
    (["--run-dir", "out/../escape"], "may not contain '..'"),
    (["--run-dir", "out/toolchain/sel4-profile-venv/evidence"], "direct child"),
    (["--sel4-source", "/"], "outside its required root"),
    (["--profile-python", "/bin/python"], "canonical repository virtualenv"),
])
def test_selected_checkout_rejects_hostile_path_overrides(
    checkout: Path, arguments: list[str], expected: str,
) -> None:
    """Exercise path admission independently of the caller's checkout or dirt."""
    source = checkout / "out" / "sel4" / "source"
    source.mkdir(parents=True)
    (checkout / "out" / "toolchain" / "sel4-profile-venv").mkdir(parents=True)
    result = invoke(
        checkout, "--clean-root", str(checkout),
        "--qemu", str(Path(sys.executable).resolve()),
        "--sel4-source", str(source), *arguments,
    )
    assert result.returncode != 0
    assert expected in result.stderr


@pytest.mark.parametrize("kind", ["tracked", "untracked"])
def test_selected_checkout_rejects_uncommitted_source(
    checkout: Path, kind: str,
) -> None:
    path = checkout / ("nonexecutable-qemu" if kind == "tracked" else "new-source")
    path.write_text("uncommitted input\n", encoding="utf-8")
    result = invoke(checkout, "--clean-root", str(checkout))
    assert result.returncode == 2
    assert "--clean-root requires an exact clean candidate checkout" in result.stderr


@pytest.mark.parametrize("command", ["spawn heartbeat ticks=100", "kill worker1", "ls /"])
def test_control_script_approves_only_mutations(tmp_path: Path, command: str) -> None:
    """The generated script follows the documented single-use approval order."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text(encoding="utf-8")
    function = "run_cohsh_command() {" + source.split(
        "run_cohsh_command() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    host_tools = tmp_path / "host-tools"
    host_tools.mkdir()
    cohsh = host_tools / "cohsh"
    cohsh.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    cohsh.chmod(0o700)
    subprocess.run(
        ["bash", "-eu", "-c", function + '\nGATEWAY_PID=\n'
         'HOST_TOOLS="$1"\nM26E_CONSOLE_AUTH_TOKEN=fixture\n'
         'run_cohsh_command "$2" "$3" 17\n',
         "qualification-test", str(host_tools), str(tmp_path), command],
        check=True, timeout=10, capture_output=True, text=True,
    )
    lines = (tmp_path / "cohsh-command-17.coh").read_text(encoding="utf-8").splitlines()
    commands = [line for line in lines if not line.startswith("#")]
    approval = (
        'echo \'{"id":"m26e-control-17","target":"/queen/ctl",'
        '"decision":"approve"}\' > /actions/queue'
    )
    prefix = ["attach queen", "EXPECT OK"]
    if command.startswith(("spawn ", "kill ")):
        prefix += [approval, "EXPECT SUBSTR path=/actions/queue"]
    assert commands == prefix + [command, "EXPECT OK", "quit"]


@pytest.mark.parametrize("role", ["worker-heartbeat", "worker-gpu", "worker-lora"])
def test_fault_plan_drives_two_passive_lifecycle_calls(tmp_path: Path, role: str) -> None:
    """Each role needs received IPC between READY and the two execution faults."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text(encoding="utf-8")
    function = "drive_worker_fault_plan() {" + source.split(
        "drive_worker_fault_plan() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    host_tools = tmp_path / "host-tools"
    host_tools.mkdir()
    fixture = host_tools / "gpu-bridge-host"
    fixture.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    fixture.chmod(0o700)
    (tmp_path / "uart.live.log").write_text("", encoding="utf-8")
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
HARNESS_PYTHON=true
GDB_BIN=fixture
TARGET_SESSION=fixture
GENERATED_INVENTORY=fixture
WORKER_MANIFEST=fixture
WORKER_HEART_ELF=fixture
WORKER_GPU_ELF=fixture
WORKER_LORA_ELF=fixture
M26E_CONSOLE_AUTH_TOKEN=fixture
HOST_TOOLS="$1/host-tools"
GDB_RUNNER_PID=
sleep() { :; }
wait_for_marker_count() { :; }
capture_worker_log() { :; }
worker_marker_count() { printf '0\n'; }
spawn_command_for_role() { printf 'spawn %s\n' "$1"; }
run_cohsh_command() { printf 'operator %s\n' "$2"; }
trigger_disposable_worker_control() { printf 'shutdown %s %s\n' "$2" "$3"; }
drive_worker_fault_plan "$1" "$2" 100
''', "fault-plan-test", str(tmp_path), role],
        check=True, capture_output=True, text=True, timeout=10,
    )
    assert result.stdout.splitlines() == [
        f"operator spawn {role}", f"operator spawn {role}",
        f"shutdown {role} 110", f"operator spawn {role}",
        f"shutdown {role} 111", f"operator spawn {role}",
    ]


def test_fault_control_refuses_an_existing_gateway_owner(tmp_path: Path) -> None:
    """A phase error must stop before attempting direct TCP authentication."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text(encoding="utf-8")
    function = "trigger_disposable_worker_control() {" + source.split(
        "trigger_disposable_worker_control() {", 1,
    )[1].split("\ndrive_worker_fault_plan() {", 1)[0]
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
die() { printf '%s\n' "$*" >&2; exit 2; }
GATEWAY_PID=123
trigger_disposable_worker_control "$1" worker-heartbeat 1
''', "fault-owner-test", str(tmp_path)],
        check=False, capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == 2
    assert "requires the pre-gateway phase" in result.stderr
