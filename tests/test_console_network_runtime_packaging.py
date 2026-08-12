# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Verify exact target binding and payload placement for the isolated console-network runtime.
# Author: Lukas Bower

"""Milestone 26e console-network image binding and packaging regressions."""

from __future__ import annotations

import pathlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILD_SCRIPT = REPO_ROOT / "scripts" / "cohesix-build-run.sh"
ROOT_BUILD = REPO_ROOT / "apps" / "root-task" / "build.rs"
RUNTIME_MANIFEST = REPO_ROOT / "apps" / "console-network-runtime" / "Cargo.toml"
RUNTIME_MAIN = REPO_ROOT / "apps" / "console-network-runtime" / "src" / "main.rs"
RUNTIME_LIB = REPO_ROOT / "apps" / "console-network-runtime" / "src" / "lib.rs"
RUNTIME_KERNEL = REPO_ROOT / "apps" / "console-network-runtime" / "src" / "kernel.rs"
ROOT_HAL = (
    REPO_ROOT / "apps" / "root-task" / "src" / "hal" / "console_network.rs"
)
ROOT_MANIFEST = REPO_ROOT / "configs" / "root_task.toml"


def test_target_runtime_is_built_bound_then_staged_at_compiler_path() -> None:
    """The one compiled target ELF must be root-bound and packaged once."""

    source = BUILD_SCRIPT.read_text(encoding="utf-8")

    assert "SEL4_COMPONENT_PACKAGES=(nine-door-runtime console-network-runtime " in source
    assert (
        'COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE="$SEL4_ARTIFACT_DIR/'
        'console-network-runtime"' in source
    )
    assert '"$ARTIFACTS_DIR/console-network-runtime"' in source
    rootfs_block = source.split("ROOTFS_COMPONENT_BINS=(", maxsplit=1)[1].split(
        ")", maxsplit=1
    )[0]
    assert "console-network-runtime" not in rootfs_block
    assert 'MANIFEST_INPUTS+=("cohesix/artifacts/console-network-runtime")' in source
    assert '"console_network_runtime": {' in source


def test_root_build_fails_closed_and_binds_exact_elf_identity() -> None:
    """Runtime-eligible root builds require validated AArch64 W^X bytes."""

    source = ROOT_BUILD.read_text(encoding="utf-8")

    assert 'const IMAGE_ENV: &str = "COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE";' in source
    assert "target root-task builds require {IMAGE_ENV}" in source
    assert "validate_console_network_elf(&image)?" in source
    assert "console_network_has_exact_entry_symbol(bytes, entry)?" in source
    assert "console-network ELF load segment violates bounds or W^X" in source
    assert "identity.load_pages != expected_pages" in source
    assert "CONSOLE_NETWORK_RUNTIME_SHA256" in source
    assert "include_bytes!({include_path})" in source


def test_runtime_and_compiler_contract_remain_no_std_and_path_identical() -> None:
    """The target child has no host fallback authority or packaging alias."""

    manifest = RUNTIME_MANIFEST.read_text(encoding="utf-8")
    main = RUNTIME_MAIN.read_text(encoding="utf-8")
    compiler_manifest = ROOT_MANIFEST.read_text(encoding="utf-8")

    assert 'name = "console-network-runtime"' in manifest
    assert 'target_os = "none"' in manifest
    assert "#![cfg_attr(target_os = \"none\", no_std)]" in main
    assert 'image_path = "cohesix/artifacts/console-network-runtime"' in compiler_manifest
    assert 'entry_symbol = "_start"' in compiler_manifest


def test_runtime_qemu_fault_hooks_are_diagnostic_and_control_path_bound() -> None:
    """The GDB hooks are gated and reached only by admitted control turns."""

    manifest = RUNTIME_MANIFEST.read_text(encoding="utf-8")
    source = RUNTIME_KERNEL.read_text(encoding="utf-8")

    assert "qemu-evidence = []" in manifest
    assert "cohesix_console_network_qemu_evidence_control_handler" in source
    assert "cohesix_console_network_qemu_evidence_standard_fault" in source
    assert "cohesix_console_network_qemu_evidence_timeout_spin" in source
    control_branch = source.split("ChildTurnUnit::ApplyControl => {", maxsplit=1)[1]
    control_branch = control_branch.split("ChildTurnUnit::Idle => {}", maxsplit=1)[0]
    control_hook = "cohesix_console_network_qemu_evidence_control_handler();"
    assert control_hook in control_branch
    assert control_branch.index(control_hook) < control_branch.index("match read_control(")
    hook = source.split("/// Stable external-QEMU evidence hook", maxsplit=1)[1]
    hook = hook.split("/// Target entry", maxsplit=1)[0]
    assert "seL4_" not in hook


def test_runtime_crosses_one_mcs_refill_before_each_ordinary_wait() -> None:
    """Ready and every completed material unit re-enter Yield then Wait."""

    source = RUNTIME_KERNEL.read_text(encoding="utf-8")
    target = source.split("pub unsafe extern \"C\" fn _start", maxsplit=1)[1]
    target = target.split("fn install_ipc_buffer", maxsplit=1)[0]
    ready = target.index("ExchangeKind::Ready,")
    ready_signal = target.index(
        "signal_slot(descriptor.supervisor_wake_notification_slot);", ready
    )
    loop_start = target.index("    loop {", ready_signal)
    first_wait = target.index("let badge = wait_for_work(descriptor);", loop_start)
    unit_match = target.index("match unit {", first_wait)
    loop_end = target.rindex("\n    }")
    assert ready < ready_signal < loop_start < first_wait < unit_match < loop_end

    ordinary_loop = target[loop_start:loop_end]
    assert ordinary_loop.count("let badge = wait_for_work(descriptor);") == 1
    assert ordinary_loop.rstrip().endswith("ChildTurnUnit::Idle => {}\n        }")
    assert "continue;" not in ordinary_loop
    for unit in (
        "PublishCompletion",
        "PublishServiceEvent",
        "PublishEgress",
        "PollService",
        "IngestPacket",
        "ApplyControl",
        "Idle",
    ):
        assert f"ChildTurnUnit::{unit}" in ordinary_loop

    wait = source.split("fn wait_for_work", maxsplit=1)[1]
    wait = wait.split("fn signal_slot", maxsplit=1)[0]
    assert wait.count("sel4_sys::seL4_Yield();") == 1
    assert wait.count("sel4_sys::seL4_Wait(") == 1
    assert wait.index("sel4_sys::seL4_Yield();") < wait.index("sel4_sys::seL4_Wait(")

    terminal_park = source.split("fn park_for_teardown", maxsplit=1)[1]
    terminal_park = terminal_park.split("#[panic_handler]", maxsplit=1)[0]
    assert "seL4_Yield" not in terminal_park
    assert terminal_park.count("sel4_sys::seL4_Wait(") == 1


def test_service_poll_continuation_is_retained_until_session_complete() -> None:
    """Ingress, egress, and session work occupy distinct PollService turns."""

    library = RUNTIME_LIB.read_text(encoding="utf-8")
    kernel = RUNTIME_KERNEL.read_text(encoding="utf-8")
    poll = library.split("pub fn poll_service_unit", maxsplit=1)[1]
    poll = poll.split("fn poll_session_unit", maxsplit=1)[0]
    dispatch_body = poll.split("fn poll_stack_ingress_unit", maxsplit=1)[0]
    selected = poll.index("let unit = self.poll_unit;")
    successor = poll.index("self.poll_unit = unit.successor();")
    dispatch = poll.index("match unit {")
    assert selected < successor < dispatch
    assert "ServicePollUnit::StackIngress => {" in dispatch_body
    assert "self.poll_stack_ingress_unit(timestamp);" in dispatch_body
    assert "ServicePollUnit::StackEgress => {" in dispatch_body
    assert "self.poll_stack_egress_unit(timestamp);" in dispatch_body
    assert "ServicePollUnit::Session => {" in dispatch_body
    assert "self.poll_session_unit(now_ms)?;" in dispatch_body
    assert dispatch_body.count("Ok(ServicePollOutcome::Continuation)") == 2
    assert dispatch_body.count("Ok(ServicePollOutcome::Complete)") == 1
    ingress = poll.split("fn poll_stack_ingress_unit", maxsplit=1)[1]
    ingress = ingress.split("fn poll_stack_egress_unit", maxsplit=1)[0]
    egress = poll.split("fn poll_stack_egress_unit", maxsplit=1)[1]
    assert ".poll_ingress_single(" in ingress
    assert ".poll_egress(" not in ingress
    assert ".poll_egress(" in egress
    assert ".poll_ingress_single(" not in egress
    assert poll.count("#[inline(never)]") == 3
    assert ".poll(" not in poll

    target = kernel.split("ChildTurnUnit::PollService => {", maxsplit=1)[1]
    target = target.split("ChildTurnUnit::IngestPacket => {", maxsplit=1)[0]
    call = target.index("service.poll_service_unit")
    complete_guard = target.index("if outcome == ServicePollOutcome::Complete {")
    clear = target.index("turn_scheduler.complete(ChildTurnUnit::PollService);")
    assert call < complete_guard < clear
    assert target.count("turn_scheduler.complete(ChildTurnUnit::PollService);") == 1


def test_runtime_shared_pages_use_bounded_sequence_last_io() -> None:
    """The target must never materialize a volatile 4-KiB page value."""

    source = RUNTIME_KERNEL.read_text(encoding="utf-8")

    assert "read_volatile(page)" not in source
    assert "write_volatile(page, staged)" not in source
    assert "#[inline(never)]\nfn read_packet" in source
    assert "#[inline(never)]\nfn read_control" in source
    assert "#[inline(never)]\nfn publish_packet" in source
    assert "#[inline(never)]\nfn publish_exchange" in source
    assert "PacketPageHeader" in source
    assert "ExchangePageHeader" in source
    assert "copy_nonoverlapping" in source
    assert source.count("unsafe {") == 12


def test_shared_page_caps_are_directionally_minimal() -> None:
    """The child can write only its egress packet and event pages."""

    source = ROOT_HAL.read_text(encoding="utf-8")
    mapping = source.split("fn map_shared_frames(", maxsplit=1)[1]
    mapping = mapping.split("fn install_caps_and_mcs(", maxsplit=1)[0]

    assert "let child_rights = if matches!(index, 0 | 2)" in mapping
    assert "seL4_CapRights::new(0, 0, 1, 0)" in mapping
    assert "seL4_CapRights_ReadWrite" in mapping
    assert "child_rights," in mapping
