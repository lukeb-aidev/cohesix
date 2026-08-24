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
ABI_LIB = REPO_ROOT / "crates" / "console-network-abi" / "src" / "lib.rs"


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


def test_abi_v4_batches_keep_one_page_and_exact_eight_record_bounds() -> None:
    """Response and command batches remain binary, bounded, and cap-neutral."""

    source = ABI_LIB.read_text(encoding="utf-8")

    assert "pub const ABI_VERSION: u16 = 4;" in source
    assert "SendBatch = 3," in source
    assert "CommandBatch = 27," in source
    assert "pub const SHARED_PAGE_BYTES: usize = 4096;" in source
    assert "pub const CONSOLE_PAYLOAD_BYTES: usize = 2368;" in source
    assert "pub const SEND_BATCH_MAX_RECORDS: usize = 8;" in source
    assert "pub const SEND_BATCH_LINE_BYTES: usize = 256;" in source
    assert "pub struct SendBatchBuilder" in source
    assert "pub struct SendBatchCursor" in source
    assert "SendBatchCursor::validate(payload)" in source
    assert "pub const COMMAND_BATCH_MAX_RECORDS: usize = 8;" in source
    assert "pub struct CommandBatchBuilder" in source
    assert "pub struct CommandBatchCursor" in source
    assert "CommandBatchCursor::validate(payload)" in source


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


def test_runtime_polls_retained_work_or_blocks_directly_and_spends_ack() -> None:
    """Each local unit rechecks urgent badges; ineligible work directly Waits."""

    source = RUNTIME_KERNEL.read_text(encoding="utf-8")
    target = source.split("pub unsafe extern \"C\" fn _start", maxsplit=1)[1]
    target = target.split("fn install_ipc_buffer", maxsplit=1)[0]
    ready = target.index("ExchangeKind::Ready,")
    ready_signal = target.index(
        "signal_slot(descriptor.supervisor_wake_notification_slot);", ready
    )
    loop_start = target.index("    loop {", ready_signal)
    readiness = target.index("let readiness = ChildTurnReadiness::new(", loop_start)
    first_wait = target.index(
        "let badge = wait_for_work(descriptor, local_poll_eligible);", loop_start
    )
    badge_validation = target.index(
        "if badge & !descriptor.root_wake_mask != 0", first_wait
    )
    revoke_gate = target.index("if badge & WAKE_REVOKE != 0 {", badge_validation)
    credit_grant = target.index(
        "if badge & WAKE_PUBLICATION_ACK != 0 {", revoke_gate
    )
    shutdown_gate = target.index("if badge & WAKE_SHUTDOWN != 0 {", credit_grant)
    shutdown_pending_gate = target.index("if shutdown_pending {", shutdown_gate)
    retained_badge = target.index(
        "turn_scheduler.retain_notification(packet_wake, control_wake);",
        shutdown_pending_gate,
    )
    publication_gate = target.index("if unit.is_publication() {", retained_badge)
    unit_match = target.index("match unit {", publication_gate)
    loop_end = target.rindex("\n    }")
    assert (
        ready
        < ready_signal
        < loop_start
        < readiness
        < first_wait
        < badge_validation
        < revoke_gate
        < credit_grant
        < shutdown_gate
        < shutdown_pending_gate
        < retained_badge
        < publication_gate
        < unit_match
        < loop_end
    )

    ordinary_loop = target[loop_start:loop_end]
    assert ordinary_loop.count(
        "let badge = wait_for_work(descriptor, local_poll_eligible);"
    ) == 1
    assert ordinary_loop.count("loop {") == 1
    assert ordinary_loop.count("match unit {") == 1
    assert ordinary_loop.rstrip().endswith("ChildTurnUnit::Idle => {}\n        }")
    assert ordinary_loop.count("continue;") == 2
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
    assert "seL4_Yield" not in wait
    assert wait.count("sel4_sys::seL4_Poll(") == 1
    assert wait.count("sel4_sys::seL4_Wait(") == 1
    branch = wait.index("if local_poll_eligible {")
    poll = wait.index("sel4_sys::seL4_Poll(")
    idle = wait.index("} else {")
    blocking_wait = wait.index("sel4_sys::seL4_Wait(")
    assert branch < poll < idle < blocking_wait
    assert "ChildTurnUnit::" not in wait

    assert target.index("let mut publication_credit_available = false;") < loop_start
    predicate = target.index("turn_scheduler.local_poll_eligible(", loop_start)
    predicate_end = target.index(");", predicate)
    predicate_call = target[predicate:predicate_end]
    assert "publication_credit_available" in predicate_call
    assert "readiness" in predicate_call
    badge_retention = target.index(
        "turn_scheduler.retain_notification(packet_wake, control_wake);",
        credit_grant,
    )
    assert credit_grant < badge_retention
    grant = ordinary_loop[credit_grant - loop_start : badge_retention - loop_start]
    assert "if publication_credit_available" in grant
    assert "enter_standard_fault();" in grant
    assert "publication_credit_available = true;" in grant
    gate = target[publication_gate:unit_match]
    assert "if !publication_credit_available" in gate
    assert "continue;" in gate
    assert gate.index("publication_credit_available = false;") < unit_match - publication_gate
    for unit in ("PublishCompletion", "PublishServiceEvent", "PublishEgress"):
        publish = ordinary_loop.split(f"ChildTurnUnit::{unit} => {{", maxsplit=1)[1]
        publish = publish.split("\n            ChildTurnUnit::", maxsplit=1)[0]
        assert "publication_credit_available" not in publish

    scheduler = RUNTIME_LIB.read_text(encoding="utf-8")
    predicate_body = scheduler.split(
        "pub const fn local_poll_eligible(", maxsplit=1
    )[1].split("\n    }", maxsplit=1)[0]
    assert "publication_credit_available" in predicate_body
    assert "self.take_next(readiness)" in predicate_body
    assert "ChildTurnUnit::Idle" in predicate_body
    assert "ChildTurnUnit::PollService" in predicate_body
    assert "ChildTurnUnit::PublishCompletion" in predicate_body
    assert "let control_signal = !this_packet_signal && !local_poll_eligible;" not in scheduler
    assert "const ROOT_LOWER_UNITS_PER_SERVICE_TICK: u64 = 5;" in scheduler
    assert (
        "let control_signal = turn % ROOT_LOWER_UNITS_PER_SERVICE_TICK == 0;"
        in scheduler
    )
    assert "let publication_ack = core::mem::take(&mut pending_publication_ack);" in scheduler
    assert "if publication_ack" in scheduler

    terminal_park = source.split("fn park_for_teardown", maxsplit=1)[1]
    terminal_park = terminal_park.split("#[panic_handler]", maxsplit=1)[0]
    assert "seL4_Yield" not in terminal_park
    assert terminal_park.count("sel4_sys::seL4_Wait(") == 1

    revoke = target.split("if badge & WAKE_REVOKE != 0 {", maxsplit=1)[1]
    revoke = revoke.split("if badge & WAKE_PUBLICATION_ACK != 0", maxsplit=1)[0]
    assert "publish_exchange(" not in revoke
    shutdown = target.split("if shutdown_pending {", maxsplit=1)[1]
    shutdown = shutdown.split("let packet_wake", maxsplit=1)[0]
    assert shutdown.index(
        "if !core::mem::take(&mut publication_credit_available)"
    ) < shutdown.index(
        "ExchangeKind::ShutdownComplete"
    )


def test_empty_notification_hints_retire_before_local_service_progress() -> None:
    """A tick without a newer page cannot self-Poll the same hint forever."""

    source = RUNTIME_KERNEL.read_text(encoding="utf-8")
    packet = source.split("ChildTurnUnit::IngestPacket => {", maxsplit=1)[1]
    packet = packet.split("ChildTurnUnit::ApplyControl => {", maxsplit=1)[0]
    control = source.split("ChildTurnUnit::ApplyControl => {", maxsplit=1)[1]
    control = control.split("ChildTurnUnit::Idle => {}", maxsplit=1)[0]

    for branch, unit in (
        (packet, "IngestPacket"),
        (control, "ApplyControl"),
    ):
        backpressure = branch.index("Err(RuntimeError::Backpressure) => {")
        retire = branch.index(
            f"turn_scheduler.complete(ChildTurnUnit::{unit});", backpressure
        )
        service = branch.index("turn_scheduler.request_service();", backpressure)
        assert backpressure < retire < service

    reader = source.split("fn read_control(", maxsplit=1)[1]
    reader = reader.split("fn publish_packet(", maxsplit=1)[0]
    assert "first == 0 || first <= last_sequence" in reader
    assert "Err(RuntimeError::Backpressure)" in reader


def test_service_poll_continuation_is_retained_until_session_complete() -> None:
    """A wire commit or bounded receive retains one follow-up service cycle."""

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
    assert "if self.poll_session_unit(now_ms)? {" in dispatch_body
    assert dispatch_body.count("Ok(ServicePollOutcome::Continuation)") == 3
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

    session = library.split("fn poll_session_unit", maxsplit=1)[1]
    session = session.split("pub fn pop_event", maxsplit=1)[0]
    assert "-> Result<bool, RuntimeError>" in session
    assert session.count("let mut committed_wire_frame = false;") == 1
    received = session.index("let received = {")
    ingest = session.index("if received != 0 {")
    full_frame = session.index("if sent != length {")
    commit = session.index("self.session.commit_wire_output()?;")
    retain = session.index("committed_wire_frame = true;")
    result = session.index("Ok(committed_wire_frame || received != 0)")
    assert received < ingest < full_frame < commit < retain < result
    assert session.count("committed_wire_frame = true;") == 1
    assert session.count("Ok(committed_wire_frame || received != 0)") == 1

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
    assert "fn publish_completion_watermark(" in source
    assert "INGRESS_CONSUMED_SEQUENCE_OFFSET" in source
    assert "CONTROL_CONSUMED_SEQUENCE_OFFSET" in source
    assert source.count("unsafe {") == 13


def test_control_bytes_are_kind_validated_and_drain_tracks_exact_output() -> None:
    """Binary batches bypass blanket UTF-8 and retain exact drain identity."""

    source = RUNTIME_KERNEL.read_text(encoding="utf-8")
    reader = source.split("fn read_control(", maxsplit=1)[1]
    reader = reader.split("fn publish_packet(", maxsplit=1)[0]
    assert "connection_id: read_volatile(addr_of!((*page).connection_id))" in reader
    assert "Ok((header.sequence, header.connection_id, kind, payload))" in reader

    apply_control = source.split("ChildTurnUnit::ApplyControl => {", maxsplit=1)[1]
    apply_control = apply_control.split("ChildTurnUnit::Idle => {}", maxsplit=1)[0]
    read = apply_control.index("Ok((sequence, connection_id, kind, payload))")
    apply = apply_control.index("match service.apply_control(connection_id, kind")
    payload_bytes = apply_control.index("payload.as_slice()", apply)
    applied = apply_control.index("outcome == ControlApplyOutcome::Applied")
    output_kind = apply_control.index(
        "matches!(kind, ExchangeKind::SendLine | ExchangeKind::SendBatch)"
    )
    pending = apply_control.index(
        "pending_output_control = Some((sequence, connection_id));"
    )
    completion = apply_control.index(
        "publish_completion_watermark(event, None, Some(sequence));"
    )
    completion_signal = apply_control.index(
        "signal_slot(descriptor.supervisor_wake_notification_slot);",
        completion,
    )
    assert (
        read
        < apply
        < payload_bytes
        < applied
        < output_kind
        < pending
        < completion
        < completion_signal
    )
    assert apply_control.count("publish_completion_watermark(") == 1
    assert "ExchangeKind::ControlCompleted" not in apply_control
    assert "core::str::from_utf8(payload.as_slice())" not in apply_control
    assert "Err(_) => enter_standard_fault()" in apply_control

    drain = source.split("ChildTurnUnit::PollService => {", maxsplit=1)[1]
    drain = drain.split("ChildTurnUnit::IngestPacket => {", maxsplit=1)[0]
    assert (
        "if let Some((control_sequence, control_connection_id)) = "
        "pending_output_control" in drain
    )
    assert "service.active_connection_id() != Some(control_connection_id)" in drain
    assert "service.output_drained_connection()" in drain
    assert "ExchangeKind::OutputDrained" in drain
    assert "control_sequence" in drain
    assert "pending_output_control = None;" in drain


def test_shared_page_caps_are_directionally_minimal() -> None:
    """The child can write only its egress packet and event pages."""

    source = ROOT_HAL.read_text(encoding="utf-8")
    mapping = source.split("fn map_shared_frames(", maxsplit=1)[1]
    mapping = mapping.split("fn install_caps_and_mcs(", maxsplit=1)[0]

    assert "let child_rights = if matches!(index, 0 | 2)" in mapping
    assert "seL4_CapRights::new(0, 0, 1, 0)" in mapping
    assert "seL4_CapRights_ReadWrite" in mapping
    assert "child_rights," in mapping
