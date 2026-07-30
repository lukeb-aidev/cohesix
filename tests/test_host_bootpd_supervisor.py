# Author: Lukas Bower
# Purpose: Guard host bootpd supervisor recovery after disposable output cleanup.
# Copyright 2026 Lukas Bower

"""Tests for the Pi 4 direct-link bootpd supervisor."""

import pathlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "tools" / "host-bootpd" / "start-en8-bootpd.zsh"


def test_supervisor_recreates_runtime_dir_before_each_service_turn() -> None:
    """Cleaning out must not strand a live supervisor without bootpd."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    ensure_runtime_dir = source[
        source.index("ensure_runtime_dir() {") : source.index("\nlog() {")
    ]
    loop = source[source.index("while true; do") :]
    loop_lines = loop.splitlines()

    assert 'mkdir -p "${runtime_dir}"' in ensure_runtime_dir
    assert 'print "$$" > "${pid_file}"' in ensure_runtime_dir
    assert loop_lines[1].strip() == "ensure_runtime_dir"
    assert loop.index("ensure_runtime_dir") < loop.index(
        '/usr/libexec/bootpd'
    )
