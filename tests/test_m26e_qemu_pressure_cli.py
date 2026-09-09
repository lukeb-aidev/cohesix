# Author: Lukas Bower
# Purpose: Verify release-pressure cleanup ownership and approved operator command setup.
# Copyright 2026 Lukas Bower

"""Check preflight and emitted commands without cleanup, build, or QEMU."""

from pathlib import Path
import shutil
import subprocess
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]


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
