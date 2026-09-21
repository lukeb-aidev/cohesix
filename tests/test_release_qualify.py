# Author: Lukas Bower
# Purpose: Verify release archive integrity, SD geometry and complete installation evidence.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import tarfile

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import release_qualify as qualify  # noqa: E402


def sd_fixture(path: Path) -> dict:
    """Encode independent MBR/BPB fields without simulating Pi execution."""
    size = 2 * 1024 * 1024
    payload = bytearray(size)
    payload[450] = 0x0C
    struct.pack_into("<II", payload, 454, 2048, 2048)
    payload[510:512] = b"\x55\xaa"
    payload[1048576 + 82 : 1048576 + 90] = b"FAT32   "
    payload[1048576 + 510 : 1048576 + 512] = b"\x55\xaa"
    path.write_bytes(payload)
    return {
        "schema": "cohesix-pi4-portable-sd-image/v2",
        "minimum_target_bytes": size,
        "image_size_bytes": size,
        "image_sha256": hashlib.sha256(payload).hexdigest(),
        "sector_size_bytes": 512,
        "partition_start_lba": 2048,
        "partition_sector_count": 2048,
    }


def test_sd_layout_rejects_capacity_and_geometry_drift(tmp_path: Path) -> None:
    image = tmp_path / "pi4.img"
    metadata = sd_fixture(image)
    qualify.verify_sd_layout(image, metadata)
    with pytest.raises(ValueError, match="capacity"):
        qualify.verify_sd_layout(
            image, {**metadata, "minimum_target_bytes": image.stat().st_size - 512}
        )
    with image.open("r+b") as handle:
        handle.seek(462)
        handle.write(b"\x01")
    metadata["image_sha256"] = qualify.digest(image)
    with pytest.raises(ValueError, match="exactly the compact"):
        qualify.verify_sd_layout(image, metadata)


def test_readback_rejects_an_ordinary_image_file(tmp_path: Path) -> None:
    image = tmp_path / "image/cohesix-pi4-sd.img"
    image.parent.mkdir()
    sd_fixture(image)
    with pytest.raises(ValueError, match="physical whole-disk"):
        qualify.media_readback(argparse.Namespace(bundle=tmp_path, device=image), {})


def test_pi_boot_requires_the_exact_build_then_physical_console_readiness() -> None:
    marker = b"[BUILD] selected-release image-id=exact-image"
    ready = b"Cohesix console ready\r\n"
    boot = marker + b"\r\nBOOT_TIMING stage=root-console-ready elapsed_us=729875 source=cntvct-el0\r\n" + ready
    qualify.verify_pi_boot(boot, marker)
    for invalid in (
        boot.replace(marker, b"[BUILD] another image"),
        marker + b"\r\n[mark] root-console.start.ok\r\n",
        ready + marker + b"\r\n", boot + ready, boot + boot,
    ):
        with pytest.raises(ValueError, match="exactly one fresh boot"):
            qualify.verify_pi_boot(invalid, marker)


@pytest.fixture
def bundle_fixture(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Create an independently specified minimal host packaging contract."""
    bundle = tmp_path / "Cohesix-1.0.0-beta-MacOS"
    bundle.mkdir()
    (bundle / "VERSION.txt").write_text("1.0.0-beta\n")
    key_paths = [
        "resources/keys/cas_verification_key.hex",
        "configs/generated/cas_verification_key.hex",
    ]
    for relative in key_paths:
        key_path = bundle / relative
        key_path.parent.mkdir(parents=True, exist_ok=True)
        key_path.write_text("11" * 32 + "\n", encoding="ascii")
    (bundle / "BUILD_PROVENANCE.json").write_text(
        json.dumps(
            {
                "schema": "cohesix-release-build-provenance/v1",
                "host": "macos",
                "source_commit": "c" * 40,
                "files": {
                    "resources/keys/cas_verification_key.hex": {
                        "sha256": "sha256:" + qualify.digest(bundle / key_paths[0]),
                        "size": (bundle / key_paths[0]).stat().st_size,
                    },
                },
            }
        )
    )
    names = ["VERSION.txt", "BUILD_PROVENANCE.json", *key_paths]
    (bundle / "MANIFEST.sha256").write_text(
        "".join(f"{qualify.digest(bundle / name)}  {name}\n" for name in names)
    )
    generated = tmp_path / "configs/generated"
    generated.mkdir(parents=True)
    (generated / "implementation_surface_inventory.json").write_text(
        json.dumps(
            {
                "release": {
                    "version": "1.0.0-beta",
                    "expected_bundle_files": names + ["MANIFEST.sha256"],
                    "host_tools": [],
                    "target_images": [],
                }
            }
        )
    )
    monkeypatch.setattr(qualify, "ROOT", tmp_path)
    archive = tmp_path / f"{bundle.name}.tar.gz"
    with tarfile.open(archive, "w:gz") as handle:
        handle.add(bundle, arcname=bundle.name)
    return bundle, archive


def test_extracted_bundle_and_archive_must_match(bundle_fixture) -> None:
    bundle, archive = bundle_fixture
    record = qualify.inspect_bundle(bundle, archive)
    assert record["version"] == "1.0.0-beta"
    (bundle / "VERSION.txt").write_text("0.1.0-alpha1\n")
    with pytest.raises(ValueError, match="VERSION"):
        qualify.inspect_bundle(bundle, archive)


@pytest.mark.parametrize("change", ["missing", "extra", "wrong-hash"])
def test_native_provenance_requires_the_exact_public_key_record(
    bundle_fixture, change: str,
) -> None:
    """A consistent archive manifest cannot conceal missing or false build custody."""
    bundle, archive = bundle_fixture
    path = bundle / "BUILD_PROVENANCE.json"
    record = json.loads(path.read_text())
    key = "resources/keys/cas_verification_key.hex"
    if change == "missing":
        del record["files"][key]
    elif change == "extra":
        record["files"]["undeclared"] = record["files"][key]
    else:
        record["files"][key]["sha256"] = "sha256:" + "0" * 64
    path.write_text(json.dumps(record))
    manifest = bundle / "MANIFEST.sha256"
    names = [line.split("  ", 1)[1] for line in manifest.read_text().splitlines()]
    manifest.write_text("".join(f"{qualify.digest(bundle / name)}  {name}\n" for name in names))
    with tarfile.open(archive, "w:gz") as handle:
        handle.add(bundle, arcname=bundle.name)
    with pytest.raises((ValueError, qualify.evidence.EvidenceError), match="provenance|hash mismatch"):
        qualify.inspect_bundle(bundle, archive)


def test_archive_links_are_rejected_even_with_matching_file_names(
    bundle_fixture,
) -> None:
    bundle, archive = bundle_fixture
    with tarfile.open(archive, "w:gz") as handle:
        member = tarfile.TarInfo(f"{bundle.name}/VERSION.txt")
        member.type = tarfile.SYMTYPE
        member.linkname = "/etc/passwd"
        handle.addfile(member)
    with pytest.raises(ValueError, match="link"):
        qualify.inspect_bundle(bundle, archive)


def test_archive_payload_drift_is_rejected(bundle_fixture) -> None:
    bundle, archive = bundle_fixture
    with tarfile.open(archive, "w:gz") as handle:
        member = tarfile.TarInfo(f"{bundle.name}/VERSION.txt")
        member.size = 5
        handle.addfile(member, io.BytesIO(b"wrong"))
    with pytest.raises(ValueError, match="archive differs"):
        qualify.inspect_bundle(bundle, archive)


def test_qualification_logs_are_content_bound(tmp_path: Path) -> None:
    log = tmp_path / "tcp.log"
    log.write_text("fixture authenticated operation\n")
    record = {
        "schema": qualify.SCHEMA,
        "kind": "macos",
        "status": "pass",
        "logs": [qualify.file_record(log)],
    }
    record["result_sha256"] = hashlib.sha256(
        qualify.evidence.canonical_bytes(record)
    ).hexdigest()
    output = tmp_path / "result.json"
    output.write_text(json.dumps(record))
    assert qualify.read_result(output, "macos")["kind"] == "macos"
    with pytest.raises(ValueError, match="passing pi4"):
        qualify.read_result(output, "pi4")
    log.write_text("changed")
    with pytest.raises(ValueError, match="attachment changed"):
        qualify.read_result(output, "macos")


def test_failed_check_cannot_emit_a_passing_command_log(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="check failed"):
        qualify.run(
            [sys.executable, "-c", "raise SystemExit(7)"],
            tmp_path,
            tmp_path / "command.log",
            {},
            timeout=5,
        )


def test_tcp_smoke_lets_the_packaged_script_attach_once(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    """boot_v0 owns ATTACH; a CLI auto-attach would reject its first operation."""
    commands = []
    monkeypatch.setattr(qualify, "run", lambda command, *_args: commands.append(command))
    qualify.tcp_smoke(tmp_path, "127.0.0.1", 31337, tmp_path / "tcp.log",
                      {"COH_AUTH_TOKEN": "test-only-token"})
    command, = commands
    assert "--role" not in command
    assert command[-2:] == ["--script", str(tmp_path / "scripts/cohsh/boot_v0.coh")]
    assert "test-only-token" not in command


def test_qemu_launcher_receives_graceful_shutdown_before_group_kill(monkeypatch) -> None:
    """Capture descendants must drain rather than receive the first termination signal."""
    from types import SimpleNamespace

    events = []
    process = SimpleNamespace(
        pid=123, poll=lambda: None,
        stdin=SimpleNamespace(write=lambda value: events.append(value),
                              flush=lambda: events.append("flush")),
        wait=lambda **_kwargs: events.append("drained"),
    )
    monkeypatch.setattr(qualify.os, "killpg", lambda *_args: events.append("group-kill"))
    qualify.stop_process(process)
    assert events == [b"\x01x", "flush", "drained"]


def test_failed_qemu_shutdown_cannot_leave_a_passing_qualification(monkeypatch) -> None:
    """Forceful cleanup remains scoped to the owned group and fails the check."""
    from types import SimpleNamespace

    events = []
    waits = iter([qualify.subprocess.TimeoutExpired("qemu", 10), None])

    def wait(**_kwargs):
        error = next(waits)
        if error is not None:
            raise error

    process = SimpleNamespace(pid=123, poll=lambda: None, stdin=io.BytesIO(), wait=wait)
    monkeypatch.setattr(qualify.os, "killpg", lambda pid, sig: events.append((pid, sig)))
    with pytest.raises(ValueError, match="did not shut down cleanly"):
        qualify.stop_process(process)
    assert events == [(123, qualify.signal.SIGKILL)]


def test_installation_checks_cannot_change_qualified_payload(bundle_fixture) -> None:
    bundle, _ = bundle_fixture
    manifest_hash = qualify.digest(bundle / "MANIFEST.sha256")
    qualify.verify_tested_bytes(bundle, manifest_hash)
    (bundle / "VERSION.txt").write_text("different bytes\n")
    with pytest.raises(ValueError, match="payload changed"):
        qualify.verify_tested_bytes(bundle, manifest_hash)


@pytest.fixture
def release_results(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Specify the required three archives and independent passing receipts."""
    version, commit = "1.0.0-beta", "b" * 40
    generated = tmp_path / "configs/generated"
    generated.mkdir(parents=True)
    (generated / "implementation_surface_inventory.json").write_text(
        json.dumps({"release": {"version": version}})
    )
    monkeypatch.setattr(qualify, "ROOT", tmp_path)
    monkeypatch.setattr(qualify.subprocess, "check_output", lambda *a, **k: commit)
    releases = tmp_path / "releases"
    releases.mkdir()
    paths = {}
    for kind, suffix in (("macos", "MacOS"), ("linux", "linux"), ("pi4", "Pi4")):
        archive = releases / f"Cohesix-{version}-{suffix}.tar.gz"
        archive.write_bytes(f"independent {kind} archive bytes".encode())
        result = {
            "schema": qualify.SCHEMA,
            "kind": kind,
            "status": "pass",
            "checks": qualify.PI_CHECKS if kind == "pi4" else qualify.HOST_CHECKS,
            "version": version,
            "source_commit": commit,
            "archive": qualify.file_record(archive),
            "logs": [],
        }
        result["result_sha256"] = hashlib.sha256(
            qualify.evidence.canonical_bytes(result)
        ).hexdigest()
        path = tmp_path / f"{kind}.json"
        path.write_text(json.dumps(result))
        paths[f"{kind}_result"] = path
    return argparse.Namespace(releases_dir=releases, **paths)


def test_final_gate_requires_and_binds_all_three_results(release_results) -> None:
    record = qualify.verify_release(release_results)
    assert record["checks"] == ["macos", "linux", "pi4"]
    assert len(record["qualifications"]) == 3
    assert record["qualifications"][2]["sha256"] == qualify.digest(
        release_results.pi4_result
    )
    release_results.pi4_result.unlink()
    with pytest.raises(qualify.evidence.EvidenceError):
        qualify.verify_release(release_results)


def test_final_gate_rejects_archives_changed_after_testing(release_results) -> None:
    archive = next(release_results.releases_dir.glob("*-linux.tar.gz"))
    archive.write_bytes(b"rebuilt after qualification")
    with pytest.raises(ValueError, match="archive changed"):
        qualify.verify_release(release_results)


def test_final_gate_rejects_stale_source(release_results, monkeypatch) -> None:
    monkeypatch.setattr(qualify.subprocess, "check_output", lambda *a, **k: "f" * 40)
    with pytest.raises(ValueError, match="current selected source"):
        qualify.verify_release(release_results)
