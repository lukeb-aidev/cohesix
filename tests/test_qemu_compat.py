# Author: Lukas Bower
# Purpose: Verify optional QEMU startup checks distinguish a live accelerator from version output.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import hashlib
from pathlib import Path
import subprocess
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/install"))
import qemu_compat  # noqa: E402
import setup_qemu_macos  # noqa: E402


def fake_qemu(tmp_path: Path, *, start: bool) -> Path:
    """Provide a controllable executable with real subprocess lifetime."""
    binary = tmp_path / "qemu-system-aarch64"
    binary.write_text(
        "#!/bin/sh\n"
        "if [ \"$1\" = --version ]; then echo 'QEMU emulator version 10.1.0'; exit; fi\n"
        "if [ \"$1\" = -accel ] && [ \"$2\" = help ]; then\n"
        "  printf 'hvf\\nkvm\\ntcg\\n'; exit\n"
        "fi\n"
        + ("exec sleep 5\n" if start else "echo 'accelerator aborted' >&2; exit 6\n")
    )
    binary.chmod(0o755)
    return binary


@pytest.mark.parametrize("profile", ("macos-hvf", "linux-kvm"))
def test_compatible_qemu_must_stay_alive_for_four_core_startup(
    tmp_path: Path, profile: str,
) -> None:
    binary = fake_qemu(tmp_path, start=True)
    result = qemu_compat.inspect(binary, profile, hold_seconds=0.1)
    assert result["result"] == "startup-pass-guest-unverified"
    assert result["sha256"] == hashlib.sha256(binary.read_bytes()).hexdigest()


def test_version_and_accelerator_list_do_not_hide_startup_abort(tmp_path: Path) -> None:
    binary = fake_qemu(tmp_path, start=False)
    with pytest.raises(ValueError, match="accelerator aborted"):
        qemu_compat.inspect(binary, "macos-hvf", hold_seconds=0.1)


def test_missing_version_is_a_diagnostic_error(tmp_path: Path) -> None:
    binary = tmp_path / "qemu-system-aarch64"
    binary.write_text("#!/bin/sh\nexit 0\n")
    binary.chmod(0o755)
    with pytest.raises(ValueError, match="did not report a version"):
        qemu_compat.inspect(binary, "macos-hvf", hold_seconds=0.1)


def test_pinned_macos_source_and_patch_are_explicit() -> None:
    """The optional builder must not drift to a moving upstream QEMU release."""
    assert setup_qemu_macos.ARCHIVE_URL.endswith("/qemu-10.1.0.tar.xz")
    assert hashlib.sha256(setup_qemu_macos.PATCH.encode()).hexdigest() == (
        setup_qemu_macos.PATCH_SHA256
    )
    result = subprocess.run(
        [sys.executable, str(ROOT / "scripts/install/setup_qemu_macos.py"), "--help"],
        capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == 0, result.stderr


def test_builder_reuses_a_valid_repair_prefix(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str],
) -> None:
    base = tmp_path / "10.1.0-hvf-gic-sync"
    repair = tmp_path / "10.1.0-hvf-gic-sync-repair-20260927"
    (base / "bin").mkdir(parents=True)
    (repair / "bin").mkdir(parents=True)
    fake_qemu(base / "bin", start=False)
    working = fake_qemu(repair / "bin", start=True)
    monkeypatch.setattr(sys, "argv", ["setup_qemu_macos.py", "--prefix", str(base)])

    setup_qemu_macos.main()

    assert capsys.readouterr().out.strip() == str(working)


def test_builder_preserves_an_incompatible_prefix_before_repair(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str],
) -> None:
    base = tmp_path / "10.1.0-hvf-gic-sync"
    (base / "bin").mkdir(parents=True)
    fake_qemu(base / "bin", start=False)
    selected = []

    def record_install(prefix: Path, archive: Path | None) -> Path:
        selected.append(prefix)
        assert archive is None
        return prefix / "bin/qemu-system-aarch64"

    monkeypatch.setattr(setup_qemu_macos, "install", record_install)
    monkeypatch.setattr(sys, "argv", ["setup_qemu_macos.py", "--prefix", str(base)])

    setup_qemu_macos.main()

    assert len(selected) == 1
    assert selected[0].name.startswith(base.name + "-repair-")
    assert selected[0] != base
    assert capsys.readouterr().out.strip() == str(selected[0] / "bin/qemu-system-aarch64")
