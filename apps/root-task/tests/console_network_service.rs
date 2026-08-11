// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify the generated isolated console-network root boundary and teardown protocol.
// Author: Lukas Bower

use console_network_abi::{
    ExchangeKind, ExchangePage, PacketDirection, PacketPage, EXCHANGE_COMMIT_OFFSET,
    PACKET_COMMIT_OFFSET, SHARED_PAGE_BYTES,
};
use root_task::console_network_service::{
    BoundaryError, ConsoleNetworkBoundary, ConsoleNetworkContainmentProof,
    ConsoleNetworkObjectPlan, ServiceState, READY_IDENTITY,
};

fn event_page(
    generation: u64,
    sequence: u64,
    kind: ExchangeKind,
    connection_id: u64,
    related_sequence: u64,
    payload: &[u8],
) -> [u8; SHARED_PAGE_BYTES] {
    let mut event = ExchangePage::empty(generation);
    event
        .publish_related(
            kind,
            sequence,
            connection_id,
            sequence,
            related_sequence,
            payload,
        )
        .expect("test event must satisfy the sealed ABI");
    let mut page = [0; SHARED_PAGE_BYTES];
    event
        .encode(&mut page)
        .expect("test event must fill one exact shared page");
    page
}

#[test]
fn generated_contract_is_single_listener_active_mcs_authority() {
    let mut boundary = ConsoleNetworkBoundary::new(7).expect("generated contract");
    let contract = boundary.contract();
    let plan = ConsoleNetworkObjectPlan::from_generated().expect("generated object plan");

    assert_eq!(contract.image_id, "console-network-runtime");
    assert_eq!(contract.listener_port, 31_337);
    assert_eq!(plan.revoke_anchor_slot, 16_136);
    assert_eq!(plan.objects.scheduling_contexts, 1);
    assert_eq!(plan.objects.reply_objects, 0);
    assert_eq!(plan.objects.frames, 97);
    assert_eq!(plan.objects.cspace_slots, 121);
    assert_eq!(plan.image_pages, 59);
    assert_eq!(contract.stack_vaddr, 0x7203_0000);
    assert_eq!(contract.stack_pages, 32);
    assert_eq!(
        contract.stack_vaddr + u64::from(contract.stack_pages) * SHARED_PAGE_BYTES as u64,
        0x7205_0000,
    );
    assert!(contract.budget_us > 0);
    assert!(contract.period_us >= contract.budget_us);
    assert!(!boundary.borrows_root_scheduling_context());
    assert!(!boundary.permits_second_listener());

    let descriptor = contract
        .runtime_init(
            boundary.generation(),
            [2, 0, 0, 0, 0, 1],
            [10, 0, 2, 15],
            24,
            [10, 0, 2, 2],
            "test-ticket",
        )
        .expect("bounded runtime init");
    descriptor.validate().expect("sealed descriptor");

    let ready = event_page(
        boundary.generation(),
        1,
        ExchangeKind::Ready,
        0,
        0,
        READY_IDENTITY.as_bytes(),
    );
    boundary.accept_event(&ready).expect("READY");
    assert_eq!(boundary.state(), ServiceState::Listening);
}

#[test]
fn authenticated_commands_and_exact_packet_control_completions_are_bounded() {
    let mut boundary = ConsoleNetworkBoundary::new(9).expect("generated contract");
    let generation = boundary.generation();
    boundary
        .accept_event(&event_page(
            generation,
            1,
            ExchangeKind::Ready,
            0,
            0,
            READY_IDENTITY.as_bytes(),
        ))
        .expect("READY");
    boundary
        .accept_event(&event_page(
            generation,
            2,
            ExchangeKind::Connected,
            42,
            0,
            &[],
        ))
        .expect("TCP accept");
    boundary
        .accept_event(&event_page(
            generation,
            3,
            ExchangeKind::Authenticated,
            42,
            0,
            &[],
        ))
        .expect("transport authentication");

    let mut ingress = [0; SHARED_PAGE_BYTES];
    let packet_sequence = boundary
        .stage_ingress(&[1, 2, 3, 4], &mut ingress)
        .expect("first packet");
    let staged_packet = PacketPage::decode_bounded(&ingress, generation, 0)
        .expect("root packet publisher must preserve compact ABI bytes");
    assert_eq!(staged_packet.direction(), PacketDirection::Ingress);
    assert_eq!(staged_packet.sequence(), packet_sequence);
    assert_eq!(staged_packet.packet(), &[1, 2, 3, 4]);
    assert_eq!(
        boundary.stage_ingress(&[5], &mut ingress),
        Err(BoundaryError::Backpressure)
    );
    boundary
        .accept_event(&event_page(
            generation,
            4,
            ExchangeKind::PacketConsumed,
            0,
            packet_sequence,
            &[],
        ))
        .expect("exact packet completion");
    boundary
        .stage_ingress(&[5], &mut ingress)
        .expect("completion releases one-slot backpressure");

    let mut control = [0; SHARED_PAGE_BYTES];
    let control_sequence = boundary
        .stage_authorized_line("OK CAT bytes=3", 10, &mut control)
        .expect("root-authorized ACK");
    let staged_control = ExchangePage::decode_bounded(&control, generation, 0, true)
        .expect("root control publisher must preserve compact ABI bytes");
    assert_eq!(staged_control.kind(), ExchangeKind::SendLine);
    assert_eq!(staged_control.sequence(), control_sequence);
    assert_eq!(staged_control.payload(), b"OK CAT bytes=3");
    assert_eq!(
        boundary.stage_authorized_line("END CAT", 11, &mut control),
        Err(BoundaryError::Backpressure)
    );
    boundary
        .accept_event(&event_page(
            generation,
            5,
            ExchangeKind::ControlCompleted,
            0,
            control_sequence,
            &[],
        ))
        .expect("exact control completion");
    let final_control_sequence = boundary
        .stage_authorized_line("END CAT", 12, &mut control)
        .expect("completion preserves ACK/ERR/END ordering");
    assert!(!boundary.console_output_drained(42));
    boundary
        .accept_event(&event_page(
            generation,
            6,
            ExchangeKind::ControlCompleted,
            0,
            final_control_sequence,
            &[],
        ))
        .expect("final control accepted by child");
    assert!(!boundary.console_output_drained(42));
    boundary
        .accept_event(&event_page(
            generation,
            7,
            ExchangeKind::OutputDrained,
            42,
            final_control_sequence,
            &[],
        ))
        .expect("final response left the TCP send queue");
    assert!(boundary.console_output_drained(42));

    let command = boundary
        .accept_event(&event_page(
            generation,
            8,
            ExchangeKind::Command,
            42,
            0,
            b"cat /proc/boot",
        ))
        .expect("authenticated child command");
    assert_eq!(command.payload().expect("UTF-8 command"), "cat /proc/boot");

    let disconnect_sequence = boundary
        .stage_disconnect(13, &mut control)
        .expect("root close-after-flush control");
    let staged_disconnect =
        ExchangePage::decode_bounded(&control, generation, final_control_sequence, true)
            .expect("disconnect must preserve compact ABI bytes");
    assert_eq!(staged_disconnect.kind(), ExchangeKind::Disconnect);
    assert_eq!(staged_disconnect.sequence(), disconnect_sequence);
    assert_eq!(staged_disconnect.connection_id(), 42);
    assert!(staged_disconnect.payload().is_empty());
}

#[test]
fn stale_records_and_incomplete_teardown_fail_closed() {
    let mut boundary = ConsoleNetworkBoundary::new(11).expect("generated contract");
    boundary.record_fault();
    assert_eq!(boundary.state(), ServiceState::Faulted);
    assert_eq!(
        boundary.reconstruct(ConsoleNetworkContainmentProof::default()),
        Err(BoundaryError::IncompleteContainment)
    );

    let old_generation = boundary.generation();
    let next_generation = boundary
        .reconstruct(ConsoleNetworkContainmentProof {
            tcb_suspended: true,
            scheduling_context_unbound: true,
            mappings_scrubbed: true,
            capabilities_revoked: true,
            objects_deleted: true,
            generation_fenced: true,
        })
        .expect("complete containment");
    assert_ne!(old_generation, next_generation);
    assert_eq!(boundary.state(), ServiceState::Constructing);

    let stale = event_page(
        old_generation,
        1,
        ExchangeKind::Ready,
        0,
        0,
        READY_IDENTITY.as_bytes(),
    );
    assert_eq!(
        boundary.accept_event(&stale),
        Err(BoundaryError::StaleIdentity)
    );

    let mut egress = PacketPage::empty(PacketDirection::Egress, old_generation);
    egress.publish(1, &[1, 2, 3]).expect("test egress");
    let mut page = [0; SHARED_PAGE_BYTES];
    egress.encode(&mut page).expect("exact test page");
    assert_eq!(
        boundary.accept_egress(&page),
        Err(BoundaryError::StaleIdentity)
    );
}

#[test]
fn mismatched_commits_do_not_advance_root_boundary_state() {
    let mut boundary = ConsoleNetworkBoundary::new(13).expect("generated contract");
    let generation = boundary.generation();
    let mut ready = event_page(
        generation,
        1,
        ExchangeKind::Ready,
        0,
        0,
        READY_IDENTITY.as_bytes(),
    );
    ready[EXCHANGE_COMMIT_OFFSET..EXCHANGE_COMMIT_OFFSET + 8].copy_from_slice(&2u64.to_le_bytes());
    assert_eq!(
        boundary.accept_event(&ready),
        Err(BoundaryError::StaleIdentity)
    );
    assert_eq!(boundary.state(), ServiceState::Constructing);

    let ready = event_page(
        generation,
        1,
        ExchangeKind::Ready,
        0,
        0,
        READY_IDENTITY.as_bytes(),
    );
    boundary
        .accept_event(&ready)
        .expect("same sequence remains admissible after rejection");

    let mut egress = [0u8; SHARED_PAGE_BYTES];
    PacketPage::publish_into(
        &mut egress,
        PacketDirection::Egress,
        generation,
        1,
        &[7, 8, 9],
    )
    .expect("bounded egress publication");
    egress[PACKET_COMMIT_OFFSET..PACKET_COMMIT_OFFSET + 8].copy_from_slice(&2u64.to_le_bytes());
    assert_eq!(
        boundary.accept_egress(&egress),
        Err(BoundaryError::StaleIdentity)
    );
    egress[PACKET_COMMIT_OFFSET..PACKET_COMMIT_OFFSET + 8].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        boundary
            .accept_egress(&egress)
            .expect("same packet remains admissible after rejection")
            .as_slice(),
        &[7, 8, 9]
    );
}
