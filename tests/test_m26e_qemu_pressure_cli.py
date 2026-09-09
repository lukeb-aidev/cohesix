# Author: Lukas Bower
# Purpose: Keep release pressure cleanup confined to an explicitly selected Git checkout.
# Copyright 2026 Lukas Bower

"""Black-box preflight checks; no test reaches cleanup, build, or QEMU."""

from pathlib import Path
import shutil
import subprocess

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


@pytest.mark.parametrize("kind", ["tracked", "untracked"])
def test_selected_checkout_rejects_uncommitted_source(
    checkout: Path, kind: str,
) -> None:
    path = checkout / ("nonexecutable-qemu" if kind == "tracked" else "new-source")
    path.write_text("uncommitted input\n", encoding="utf-8")
    result = invoke(checkout, "--clean-root", str(checkout))
    assert result.returncode == 2
    assert "--clean-root requires an exact clean candidate checkout" in result.stderr
