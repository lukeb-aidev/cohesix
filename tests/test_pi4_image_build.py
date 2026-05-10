# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 image build wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4-image-build.sh."""

import pathlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4-image-build.sh"


def test_pi4_image_build_respects_cargo_target_dir_for_root_task() -> None:
    """The flashed root-task must come from the same target dir Cargo built."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'local target_dir="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"' in source
    assert 'root_task_elf="$(root_task_release_elf_path)"' in source
    assert (
        'local root_task_elf="${ROOT_DIR}/target/aarch64-unknown-none/release/root-task"'
        not in source
    )
