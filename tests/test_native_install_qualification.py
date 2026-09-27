# Author: Lukas Bower
# Purpose: Test native publisher, package-path and installed-byte refusal boundaries.
# Copyright 2026 Lukas Bower
"""Pure qualification boundaries; live package-manager and GUI checks run on hosts."""

from __future__ import annotations

import hashlib
import io
from pathlib import Path
import sys
import tarfile

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/install"))
import qualify_native_install as native  # noqa: E402


def test_installed_readback_rejects_changed_bytes_and_symlink_parent(tmp_path: Path) -> None:
    root = tmp_path / "root"
    target = root / "usr/lib/cohesix/bin/coh"
    target.parent.mkdir(parents=True)
    target.write_bytes(b"accepted tool")
    row = {"path": "/usr/lib/cohesix/bin/coh", "size": 13,
           "sha256": hashlib.sha256(b"accepted tool").hexdigest()}
    assert native.verify_records(root, [row], ("/usr/lib/cohesix/",)) == 13
    target.write_bytes(b"changed tool!")
    with pytest.raises(ValueError, match="installed payload differs"):
        native.verify_records(root, [row], ("/usr/lib/cohesix/",))
    target.unlink()
    alternate = root / "external/coh"
    alternate.parent.mkdir(parents=True)
    alternate.write_bytes(b"accepted tool")
    target.parent.rename(root / "old-bin")
    (root / "usr/lib/cohesix/bin").symlink_to(alternate.parent, target_is_directory=True)
    with pytest.raises(ValueError, match="symlink component"):
        native.verify_records(root, [row], ("/usr/lib/cohesix/",))


@pytest.mark.parametrize("name", [
    "/usr/lib/cohesix/../private", "/usr/lib/cohesix/./bin/coh",
    "/usr/lib/cohesix/bin/evil\\name", "/tmp/coh",
])
def test_installed_readback_refuses_unsafe_or_unowned_path(
    tmp_path: Path, name: str,
) -> None:
    row = {"path": name, "size": 0, "sha256": "0" * 64}
    with pytest.raises(ValueError, match="unsafe|invalid"):
        native.verify_records(tmp_path, [row], ("/usr/lib/cohesix/",))


def test_installer_manifest_refuses_duplicate_keys_and_path_escape(tmp_path: Path) -> None:
    manifest = tmp_path / "installers.json"
    manifest.write_text('{"host":"macos","host":"linux"}')
    with pytest.raises(ValueError, match="duplicate manifest key"):
        native.read_json(manifest)
    for invalid in ("../outside.pkg", "/tmp/outside.pkg", "other/pkg", "a\n.pkg"):
        with pytest.raises(ValueError, match="invalid installer artifact"):
            native.package_path(manifest, invalid)


def test_reference_fields_rejects_changed_archive_identity() -> None:
    selected = {"host": "linux", "version": "1.2.0",
                "source_commit": "a" * 40, "archive_sha256": "b" * 64}
    manifest = {"schema": "cohesix-m28g-installers/v1", **selected}
    native.reference_fields(selected, manifest)
    with pytest.raises(ValueError, match="independent release reference"):
        native.reference_fields(selected, {**manifest, "archive_sha256": "c" * 64})


def test_selected_install_subset_rechecks_archive_bytes(tmp_path: Path) -> None:
    bundle = tmp_path / "bundle"
    files = {"bin/coh": b"controller", "bin/swarmui": b"desktop"}
    for name, value in files.items():
        path = bundle / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(value)
    (bundle / "MANIFEST.sha256").write_text("".join(
        f"{hashlib.sha256(value).hexdigest()}  {name}\n"
        for name, value in files.items()
    ))
    assert native.selected_subset(bundle, "/usr/lib/cohesix", "controller") == [{
        "path": "/usr/lib/cohesix/bin/coh", "size": len(b"controller"),
        "sha256": hashlib.sha256(b"controller").hexdigest(),
    }]
    (bundle / "bin/coh").write_bytes(b"changed")
    with pytest.raises(ValueError, match="selected archive file changed"):
        native.selected_subset(bundle, "/usr/lib/cohesix", "controller")


class TarProcess:
    """Expose only the bounded payload stream needed by dpkg readback."""

    def __init__(self, payload: bytes):
        self.stdout = io.BytesIO(payload)
        self.code: int | None = None

    def wait(self, timeout: int) -> int:
        self.code = 0
        return 0

    def poll(self) -> int | None:
        return self.code

    def kill(self) -> None:
        self.code = -9


def tar_payload(name: str, contents: bytes, *, symlink: bool = False) -> bytes:
    """Build one independent tar header to probe package boundary checks."""
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w") as handle:
        root = tarfile.TarInfo("./")
        root.type = tarfile.DIRTYPE
        handle.addfile(root)
        member = tarfile.TarInfo(name)
        if symlink:
            member.type = tarfile.SYMTYPE
            member.linkname = "/etc/passwd"
            handle.addfile(member)
        else:
            member.size = len(contents)
            handle.addfile(member, io.BytesIO(contents))
    return output.getvalue()


def test_deb_stream_rejects_traversal_and_links(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    package = tmp_path / "package.deb"
    package.write_bytes(b"package")
    for payload, reason in (
        (tar_payload("./usr/lib/cohesix/bin/coh", b"coh"), None),
        (tar_payload("./usr/lib/cohesix/../../etc/passwd", b"bad"), "unsafe"),
        (tar_payload("./usr/lib/cohesix/bin/coh", b"", symlink=True), "linked"),
    ):
        monkeypatch.setattr(native.subprocess, "Popen", lambda *_args, **_kwargs: TarProcess(payload))
        if reason:
            with pytest.raises(ValueError, match=reason):
                native.deb_contents(package)
        else:
            assert native.deb_contents(package)["/usr/lib/cohesix/bin/coh"]["sha256"] == (
                hashlib.sha256(b"coh").hexdigest()
            )


def test_deb_control_refuses_maintainer_script(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    package = tmp_path / "package.deb"
    package.write_bytes(b"package")
    monkeypatch.setattr(native.subprocess, "Popen", lambda *_args, **_kwargs: TarProcess(
        tar_payload("./control", b"Package: cohesix-controller\n")
    ))
    native.verify_deb_control(package)
    monkeypatch.setattr(native.subprocess, "Popen", lambda *_args, **_kwargs: TarProcess(
        tar_payload("./postinst", b"#!/bin/sh\nexit 0\n")
    ))
    with pytest.raises(ValueError, match="unselected script"):
        native.verify_deb_control(package)
