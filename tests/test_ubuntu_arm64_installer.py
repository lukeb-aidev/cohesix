# Author: Lukas Bower
# Purpose: Check controller and GNOME package ownership before native Ubuntu lifecycle qualification.
# Copyright 2026 Lukas Bower
"""Focused package-root contract checks with native dpkg reserved for Linux hosts."""

from __future__ import annotations

import hashlib
from pathlib import Path
import subprocess
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/install"))
import build_ubuntu_arm64_deb as deb  # noqa: E402


def test_package_roots_keep_desktop_dependencies_optional(
    tmp_path: Path, monkeypatch,
) -> None:
    bundle = tmp_path / "bundle"
    files = {
        "bin/coh": b"controller",
        "bin/swarmui": b"desktop",
        "ui/swarmui/index.html": b"<html></html>",
        "qemu/run.sh": b"guest",
    }
    manifest = []
    for relative, data in files.items():
        path = bundle / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        manifest.append(f"{hashlib.sha256(data).hexdigest()}  {relative}")
    (bundle / "MANIFEST.sha256").write_text("\n".join(manifest) + "\n")
    reference = {
        "bundle": str(bundle), "version": "1.2.0", "host": "linux",
        "source_commit": "a" * 40, "archive_sha256": "b" * 64,
    }

    def fake_dpkg(command: list[str]) -> None:
        assert command[:3] == ["dpkg-deb", "--build", "--root-owner-group"]
        Path(command[-1]).write_bytes(b"package")

    monkeypatch.setattr(deb, "checked", fake_dpkg)
    output = tmp_path / "output"
    output.mkdir()
    controller_root = tmp_path / "controller"
    desktop_root = tmp_path / "desktop"
    maintainer = "Lukas Bower <release@example.com>"
    deb.make_package(reference, controller_root, "controller", output, maintainer)
    deb.make_package(reference, desktop_root, "desktop", output, maintainer)

    assert (controller_root / "usr/lib/cohesix/bin/coh").is_file()
    assert (controller_root / "usr/lib/cohesix").stat().st_mode & 0o777 == 0o755
    assert not (controller_root / "usr/lib/cohesix/bin/swarmui").exists()
    assert not (controller_root / "usr/share/applications").exists()
    assert not (controller_root / "usr/lib/cohesix/qemu").exists()
    controller_control = (controller_root / "DEBIAN/control").read_text()
    desktop_control = (desktop_root / "DEBIAN/control").read_text()
    assert "libwebkit" not in controller_control
    assert "libc6 (>= 2.39)" in controller_control
    assert f"Maintainer: {maintainer}" in controller_control
    assert "cohesix-controller (= 1.2.0)" in desktop_control
    assert "libc6 (>= 2.39)" in desktop_control
    assert "libgtk-3-0t64" in desktop_control
    assert "libgtk-3-0 |" not in desktop_control
    assert "libwebkit2gtk-4.1-0" in desktop_control
    desktop_entry = desktop_root / "usr/share/applications/com.cohesix.swarmui.desktop"
    assert "Exec=/usr/lib/cohesix/bin/swarmui" in desktop_entry.read_text()
    assert "Terminal=false" in desktop_entry.read_text()
    assert (desktop_root / "usr/share/icons/hicolor/scalable/apps/com.cohesix.swarmui.svg").is_file()


def test_signing_failure_leaves_no_publishable_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(deb.platform, "system", lambda: "Linux")
    monkeypatch.setattr(deb.platform, "machine", lambda: "aarch64")
    monkeypatch.setattr(deb.shutil, "which", lambda *_: "/usr/bin/tool")
    monkeypatch.setattr(deb, "load_reference", lambda *_: {
        "version": "1.2.0", "source_commit": "a" * 40,
        "archive_sha256": "b" * 64,
    })

    def fake_package(_reference, _root, subset: str, output: Path,
                     _maintainer: str) -> dict:
        package = output / f"{subset}.deb"
        package.write_bytes(b"package")
        return {"package": subset, "path": package.name}

    monkeypatch.setattr(deb, "make_package", fake_package)
    monkeypatch.setattr(
        deb, "checked", lambda _command: (_ for _ in ()).throw(
            subprocess.CalledProcessError(1, "gpg")
        ),
    )
    output = tmp_path / "installers"
    with pytest.raises(subprocess.CalledProcessError):
        deb.build(
            tmp_path / "reference.json", output, "PUBLISHER",
            "Lukas Bower <release@example.com>",
        )
    assert not output.exists()


def test_builder_refuses_unselected_public_maintainer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(deb.platform, "system", lambda: "Linux")
    monkeypatch.setattr(deb.platform, "machine", lambda: "aarch64")
    monkeypatch.setattr(deb.shutil, "which", lambda *_: "/usr/bin/tool")
    for invalid in (None, "Lukas Bower", "Lukas <bad\nInjected: yes>"):
        with pytest.raises(ValueError, match="public Debian maintainer"):
            deb.build(tmp_path / "reference.json", tmp_path / "out", "KEY", invalid)


def test_signing_passphrase_comes_from_inherited_stdin(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Allow a remote native builder to sign without a secret in argv or files."""
    monkeypatch.setattr(deb.platform, "system", lambda: "Linux")
    monkeypatch.setattr(deb.platform, "machine", lambda: "aarch64")
    monkeypatch.setattr(deb.shutil, "which", lambda *_: "/usr/bin/tool")
    monkeypatch.setattr(deb, "load_reference", lambda *_: {
        "version": "1.2.0", "source_commit": "a" * 40,
        "archive_sha256": "b" * 64,
    })
    monkeypatch.setattr(deb, "make_package", lambda _r, _p, subset, _o, _m: {
        "package": subset, "path": f"{subset}.deb",
    })
    monkeypatch.setenv("COHESIX_DEB_SIGNING_PASSPHRASE_STDIN", "1")
    observed: list[str] = []

    def fake_sign(command: list[str]) -> None:
        observed.extend(command)
        Path(command[command.index("--output") + 1]).write_text("signature")

    monkeypatch.setattr(deb, "checked", fake_sign)
    result = deb.build(tmp_path / "reference.json", tmp_path / "packages",
                       "PUBLISHER", "Lukas Bower <release@example.com>")
    assert result["version"] == "1.2.0"
    assert observed[:7] == ["gpg", "--batch", "--yes", "--pinentry-mode",
                            "loopback", "--passphrase-fd", "0"]
    assert (tmp_path / "packages/installers.json.asc").is_file()
