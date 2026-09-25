# Author: Lukas Bower
# Purpose: Check pinned Mac HVF process and exact image identity for M28c1 acceptance.
# Copyright 2026 Lukas Bower
"""Focused source and process identity checks for the Mac live case."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import shlex
import subprocess

import pytest

from provider_m28c1_live import _mac_qemu_image_identity, _selected
import provider_m28c1_live as live


def test_mac_qemu_identity_binds_pinned_binary_and_source(
        tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(live, "ROOT", tmp_path)
    image_dir = tmp_path / "out" / "queen"
    image_dir.mkdir(parents=True)
    qemu = tmp_path / "qemu-system-aarch64"
    qemu.write_bytes(b"pinned qemu")
    elfloader = image_dir / "elfloader"
    cpio = image_dir / "rootfs.cpio"
    rootserver = image_dir / "rootserver"
    elfloader.write_bytes(b"elfloader")
    cpio.write_bytes(b"rootfs")
    rootserver.write_bytes(b"[BUILD] abcdef123456 selected profile")
    args = [str(qemu), "-accel", "hvf", "-kernel", str(elfloader),
            "-initrd", str(cpio), "-device",
            f"loader,file={rootserver},addr=0x80000000,force-raw=on"]
    output = shlex.join(args) + "\n"
    monkeypatch.setattr(live.subprocess, "run", lambda *_args, **_kwargs:
                        subprocess.CompletedProcess([], 0, output, ""))
    selected = {"target_qemu_pid": 42, "qemu_binary": str(qemu),
                "qemu_sha256": hashlib.sha256(qemu.read_bytes()).hexdigest()}
    identity = _mac_qemu_image_identity(selected, "abcdef123456" + "0" * 28)
    assert identity["rootserver_sha256"] == hashlib.sha256(
        rootserver.read_bytes()).hexdigest()
    assert identity["qemu_sha256"] == selected["qemu_sha256"]

    monkeypatch.setattr(live.subprocess, "run", lambda *_args, **_kwargs:
                        subprocess.CompletedProcess(
                            [], 0, output.replace("-accel hvf", "-accel tcg"), ""))
    with pytest.raises(ValueError, match="pinned HVF process identity"):
        _mac_qemu_image_identity(selected, "abcdef123456" + "0" * 28)


def test_live_reference_accepts_observed_adapter_only_for_release(
        tmp_path: Path) -> None:
    fields: dict[str, object] = {
        key: ("a" * 64 if key.endswith("_sha256") else "/tmp/selected")
        for key in live.MLX | {"qemu_binary", "qemu_sha256"}
    }
    fields.update(schema=live.SCHEMA, host_profile="mac-apple-m4-macos27",
                  scenario="train", source_commit="a" * 40,
                  target_host="local", target_qemu_pid=42,
                  gateway_url="http://127.0.0.1:8182",
                  request_auth_ref="file:/tmp/auth",
                  delegated_ticket_ref="file:/tmp/ticket",
                  wait_seconds=10, expected_generation=1,
                  expected_adapter_sha256="observed")
    reference = tmp_path / "reference.toml"
    reference.write_text("\n".join(f"{key} = {json.dumps(value)}"
                                   for key, value in fields.items()))
    assert _selected(reference, "mac-apple-m4-macos27",
                     "m28c1-mlx-live")["expected_adapter_sha256"] == "observed"
    fields["expected_adapter_sha256"] = "not-a-hash"
    reference.write_text("\n".join(f"{key} = {json.dumps(value)}"
                                   for key, value in fields.items()))
    with pytest.raises(ValueError, match="expected deployment"):
        _selected(reference, "mac-apple-m4-macos27", "m28c1-mlx-live")
