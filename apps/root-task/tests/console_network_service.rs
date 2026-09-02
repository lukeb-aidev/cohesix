// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify the generated isolated console-network root boundary and teardown protocol.
// Author: Lukas Bower

use console_network_abi::{
    CommandBatchBuilder, CommandBatchCursor, ExchangeKind, ExchangePage, PacketDirection,
    PacketPage, SendBatchBuilder, CONSOLE_PAYLOAD_BYTES, CONTROL_CONSUMED_SEQUENCE_OFFSET,
    EXCHANGE_COMMIT_OFFSET, INGRESS_CONSUMED_SEQUENCE_OFFSET, PACKET_COMMIT_OFFSET,
    SHARED_PAGE_BYTES,
};
use root_task::console_network_service::{
    BoundaryError, ConsoleNetworkBoundary, ConsoleNetworkContainmentCursor,
    ConsoleNetworkContainmentProof, ConsoleNetworkContainmentTurn, ConsoleNetworkContainmentUnit,
    ConsoleNetworkControlPublication, ConsoleNetworkObjectPlan, ServiceState, READY_IDENTITY,
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

fn completion_page(
    generation: u64,
    packet_sequence: u64,
    control_sequence: u64,
) -> [u8; SHARED_PAGE_BYTES] {
    let mut page = [0; SHARED_PAGE_BYTES];
    ExchangePage::empty(generation)
        .encode(&mut page)
        .expect("test watermark page must fill one exact shared page");
    page[INGRESS_CONSUMED_SEQUENCE_OFFSET..INGRESS_CONSUMED_SEQUENCE_OFFSET + 8]
        .copy_from_slice(&packet_sequence.to_le_bytes());
    page[CONTROL_CONSUMED_SEQUENCE_OFFSET..CONTROL_CONSUMED_SEQUENCE_OFFSET + 8]
        .copy_from_slice(&control_sequence.to_le_bytes());
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
    assert_eq!(plan.objects.fault_caps, 1);
    assert_eq!(plan.objects.timeout_fault_caps, 1);
    assert!(contract.direct_virtio);
    assert_eq!(plan.objects.frames, 134);
    assert_eq!(plan.objects.cspace_slots, 162);
    assert_eq!(plan.image_pages, 62);
    assert_eq!(contract.stack_vaddr, 0x7203_0000);
    assert_eq!(contract.stack_pages, 32);
    assert_eq!(
        contract.stack_vaddr + u64::from(contract.stack_pages) * SHARED_PAGE_BYTES as u64,
        0x7205_0000,
    );
    assert!(contract.budget_us > 0);
    assert!(contract.period_us >= contract.budget_us);
    assert_eq!(contract.timeout_badge, 0x26ee_0009);
    assert_eq!(contract.standard_fault_badge, 0x26e4_0001);
    assert_eq!(
        contract.timeout_policy,
        root_task::generated::TimeoutPolicy::NaturalPostpone
    );
    assert!(
        !contract.yield_to_child_after_signal,
        "the generated QEMU contract must retain its non-YieldTo path"
    );
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
fn initialized_child_pages_are_idle_before_ready_without_weakening_identity() {
    let mut boundary = ConsoleNetworkBoundary::new(27).expect("generated contract");
    let mut event = [0xa5; SHARED_PAGE_BYTES];
    let mut egress = [0xa5; SHARED_PAGE_BYTES];
    ExchangePage::initialize_into(&mut event, 27).expect("construction-only event init");
    PacketPage::initialize_into(&mut egress, PacketDirection::Egress, 27)
        .expect("construction-only egress init");
    assert_eq!(
        boundary.child_publication_pending(&event, &egress),
        Ok(false)
    );
    assert_eq!(boundary.state(), ServiceState::Constructing);

    let ready = event_page(27, 1, ExchangeKind::Ready, 0, 0, READY_IDENTITY.as_bytes());
    assert_eq!(
        boundary.child_publication_pending(&ready, &egress),
        Ok(true)
    );
    boundary.accept_event(&ready).expect("real child READY");
    assert_eq!(
        boundary.child_publication_pending(&ready, &egress),
        Ok(false)
    );
    assert_eq!(boundary.state(), ServiceState::Listening);

    egress[0] ^= 1;
    assert!(boundary.child_publication_pending(&ready, &egress).is_err());
    PacketPage::initialize_into(&mut egress, PacketDirection::Egress, 28)
        .expect("different generation fixture");
    assert!(boundary.child_publication_pending(&ready, &egress).is_err());
}

#[test]
fn child_publication_frontier_covers_event_egress_and_both_watermarks() {
    let mut boundary = ConsoleNetworkBoundary::new(27).expect("generated contract");
    let generation = boundary.generation();
    let mut egress = [0; SHARED_PAGE_BYTES];
    PacketPage::empty(PacketDirection::Egress, generation)
        .encode(&mut egress)
        .expect("empty egress page");
    let idle = completion_page(generation, 0, 0);
    assert!(!boundary
        .child_publication_pending(&idle, &egress)
        .expect("empty frontier"));

    let ready = event_page(
        generation,
        1,
        ExchangeKind::Ready,
        0,
        0,
        READY_IDENTITY.as_bytes(),
    );
    assert!(boundary
        .child_publication_pending(&ready, &egress)
        .expect("Ready level"));
    boundary.accept_event(&ready).expect("consume Ready");
    assert!(!boundary
        .child_publication_pending(&idle, &egress)
        .expect("accepted Ready frontier"));

    PacketPage::publish_into(
        &mut egress,
        PacketDirection::Egress,
        generation,
        1,
        &[1, 2, 3, 4],
    )
    .expect("egress publication");
    assert!(boundary
        .child_publication_pending(&idle, &egress)
        .expect("egress level"));
    boundary.accept_egress(&egress).expect("consume egress");
    assert!(!boundary
        .child_publication_pending(&idle, &egress)
        .expect("accepted egress frontier"));

    let mut ingress = [0; SHARED_PAGE_BYTES];
    let packet_sequence = boundary
        .stage_ingress(&[5, 6], &mut ingress)
        .expect("root packet publication");
    let packet_complete = completion_page(generation, packet_sequence, 0);
    assert!(boundary
        .child_publication_pending(&packet_complete, &egress)
        .expect("ingress watermark level"));
    boundary
        .accept_completion_watermarks(&packet_complete)
        .expect("consume ingress watermark");
    assert!(!boundary
        .child_publication_pending(&packet_complete, &egress)
        .expect("accepted ingress watermark frontier"));

    for (sequence, kind) in [
        (2, ExchangeKind::Connected),
        (3, ExchangeKind::Authenticated),
    ] {
        boundary
            .accept_event(&event_page(generation, sequence, kind, 42, 0, &[]))
            .expect("authenticated lifecycle");
    }
    let mut control = [0; SHARED_PAGE_BYTES];
    let control_sequence = boundary
        .stage_authorized_line("END", 10, &mut control)
        .expect("root control publication");
    assert_eq!(
        boundary.control_publication_owed(),
        Some(ConsoleNetworkControlPublication {
            generation,
            connection_id: 42,
            sequence: control_sequence,
        }),
        "the exact one-slot control remains causal only until its child watermark",
    );
    let control_complete = completion_page(generation, packet_sequence, control_sequence);
    assert!(boundary
        .child_publication_pending(&control_complete, &egress)
        .expect("control watermark level"));
    boundary
        .accept_completion_watermarks(&control_complete)
        .expect("consume control watermark");
    assert_eq!(
        boundary.control_publication_owed(),
        None,
        "an accepted child watermark revokes wait authority before peer drain",
    );
    assert!(!boundary
        .child_publication_pending(&control_complete, &egress)
        .expect("accepted complete frontier"));
}

#[test]
fn ready_publication_rejects_identity_generation_and_sequence_drift() {
    let mut boundary = ConsoleNetworkBoundary::new(17).expect("generated contract");
    let generation = boundary.generation();

    assert_eq!(
        boundary.accept_event(&event_page(
            generation,
            1,
            ExchangeKind::Ready,
            0,
            0,
            b"wrong-service-identity",
        )),
        Err(BoundaryError::InvalidState),
    );
    assert_eq!(boundary.state(), ServiceState::Constructing);
    assert_eq!(
        boundary.accept_event(&event_page(
            generation + 1,
            1,
            ExchangeKind::Ready,
            0,
            0,
            READY_IDENTITY.as_bytes(),
        )),
        Err(BoundaryError::StaleIdentity),
    );

    let ready = event_page(
        generation,
        1,
        ExchangeKind::Ready,
        0,
        0,
        READY_IDENTITY.as_bytes(),
    );
    let accepted = boundary.accept_event(&ready).expect("exact Ready");
    assert_eq!(accepted.now_ms(), 1);
    assert_eq!(boundary.state(), ServiceState::Listening);
    assert_eq!(
        boundary.accept_event(&ready),
        Err(BoundaryError::StaleIdentity),
        "a retained Ready timestamp cannot be refreshed by replaying its sequence",
    );
}

#[test]
fn authenticated_commands_and_exact_packet_control_watermarks_are_bounded() {
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
        .accept_completion_watermarks(&completion_page(generation, packet_sequence, 0))
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
        .accept_completion_watermarks(&completion_page(
            generation,
            packet_sequence,
            control_sequence,
        ))
        .expect("exact control completion");
    let final_control_sequence = boundary
        .stage_authorized_line("END CAT", 12, &mut control)
        .expect("completion preserves ACK/ERR/END ordering");
    assert!(!boundary.console_output_drained(42));
    boundary
        .accept_completion_watermarks(&completion_page(
            generation,
            packet_sequence,
            final_control_sequence,
        ))
        .expect("final control accepted by child");
    assert!(!boundary.console_output_drained(42));
    boundary
        .accept_event(&event_page(
            generation,
            4,
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
            5,
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
fn authorized_batch_is_one_exact_control_and_one_exact_drain_fence() {
    let mut boundary = ConsoleNetworkBoundary::new(10).expect("generated contract");
    let generation = boundary.generation();
    for (sequence, kind, connection_id, payload) in [
        (1, ExchangeKind::Ready, 0, READY_IDENTITY.as_bytes()),
        (2, ExchangeKind::Connected, 77, &[][..]),
        (3, ExchangeKind::Authenticated, 77, &[][..]),
    ] {
        boundary
            .accept_event(&event_page(
                generation,
                sequence,
                kind,
                connection_id,
                0,
                payload,
            ))
            .expect("ordered authenticated lifecycle");
    }

    let mut batch_storage = [0u8; CONSOLE_PAYLOAD_BYTES];
    let mut builder = SendBatchBuilder::new(&mut batch_storage);
    assert_eq!(builder.try_push_line("OK CAT path=/proc/demo"), Ok(true));
    assert_eq!(builder.try_push_line("record-1"), Ok(true));
    assert_eq!(builder.try_push_line("END"), Ok(true));
    let payload = builder.finish().expect("bounded batch");
    let mut control = [0u8; SHARED_PAGE_BYTES];
    let sequence = boundary
        .stage_authorized_batch(payload, 20, &mut control)
        .expect("one exact SendBatch control");
    let staged = ExchangePage::decode_bounded(&control, generation, 0, true)
        .expect("root batch publication");
    assert_eq!(staged.kind(), ExchangeKind::SendBatch);
    assert_eq!(staged.connection_id(), 77);
    assert_eq!(staged.payload(), payload);
    assert_eq!(
        boundary.stage_authorized_batch(payload, 21, &mut control),
        Err(BoundaryError::Backpressure)
    );

    boundary
        .accept_completion_watermarks(&completion_page(generation, 0, sequence))
        .expect("control page released");
    assert!(!boundary.console_output_drained(77));
    boundary
        .accept_event(&event_page(
            generation,
            4,
            ExchangeKind::OutputDrained,
            77,
            sequence,
            &[],
        ))
        .expect("whole batch left child TCP queue");
    assert!(boundary.console_output_drained(77));
}

#[test]
fn authenticated_command_batch_is_validated_and_copied_atomically() {
    let mut boundary = ConsoleNetworkBoundary::new(12).expect("generated contract");
    let generation = boundary.generation();
    for (sequence, kind, connection_id, payload) in [
        (1, ExchangeKind::Ready, 0, READY_IDENTITY.as_bytes()),
        (2, ExchangeKind::Connected, 88, &[][..]),
        (3, ExchangeKind::Authenticated, 88, &[][..]),
    ] {
        boundary
            .accept_event(&event_page(
                generation,
                sequence,
                kind,
                connection_id,
                0,
                payload,
            ))
            .expect("ordered authenticated lifecycle");
    }

    let mut storage = [0u8; CONSOLE_PAYLOAD_BYTES];
    let payload_len = {
        let mut builder = CommandBatchBuilder::new(&mut storage);
        assert_eq!(builder.try_push_command(101, "help"), Ok(true));
        assert_eq!(builder.try_push_command(102, "smp mcs"), Ok(true));
        assert_eq!(builder.try_push_command(103, "cat /proc/boot"), Ok(true));
        builder.finish().expect("bounded command batch").len()
    };
    let event = boundary
        .accept_event(&event_page(
            generation,
            4,
            ExchangeKind::CommandBatch,
            88,
            0,
            &storage[..payload_len],
        ))
        .expect("validated authenticated command batch");
    assert_eq!(event.kind(), ExchangeKind::CommandBatch);
    let mut cursor = CommandBatchCursor::validate(event.payload_bytes()).unwrap();
    assert_eq!(
        cursor.next_command(event.payload_bytes()).unwrap(),
        Some((101, "help"))
    );
    assert_eq!(
        cursor.next_command(event.payload_bytes()).unwrap(),
        Some((102, "smp mcs"))
    );
    assert_eq!(
        cursor.next_command(event.payload_bytes()).unwrap(),
        Some((103, "cat /proc/boot"))
    );
    assert_eq!(cursor.next_command(event.payload_bytes()), Ok(None));
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
fn containment_completion_carries_only_the_exact_full_proof() {
    let mut cursor = ConsoleNetworkContainmentCursor::new();
    let units = [
        ConsoleNetworkContainmentUnit::SuspendTcb,
        ConsoleNetworkContainmentUnit::UnbindSchedulingContext,
        ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(0),
        ConsoleNetworkContainmentUnit::UnmapSharedFrame(0),
        ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(1),
        ConsoleNetworkContainmentUnit::UnmapSharedFrame(1),
        ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(2),
        ConsoleNetworkContainmentUnit::UnmapSharedFrame(2),
        ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(3),
        ConsoleNetworkContainmentUnit::UnmapSharedFrame(3),
        ConsoleNetworkContainmentUnit::DeleteFaultCap(0),
        ConsoleNetworkContainmentUnit::DeleteFaultCap(1),
        ConsoleNetworkContainmentUnit::RevokeAnchor,
        ConsoleNetworkContainmentUnit::Finalize,
    ];
    for unit in units {
        assert_eq!(cursor.unit(), unit);
        let selected = cursor.select_next();
        assert_eq!(selected, unit);
        cursor.restore_selected(selected);
        assert_eq!(
            cursor.unit(),
            unit,
            "restoring a failed selected unit must undo its committed successor",
        );
        assert_eq!(cursor.select_next(), unit);
    }
    assert_eq!(cursor.unit(), ConsoleNetworkContainmentUnit::Complete);
    assert_eq!(
        cursor.select_next(),
        ConsoleNetworkContainmentUnit::Complete,
        "complete proof remains idempotently reportable",
    );

    let incomplete = ConsoleNetworkContainmentProof::default();
    assert!(!incomplete.complete());

    let complete = ConsoleNetworkContainmentProof {
        tcb_suspended: true,
        scheduling_context_unbound: true,
        mappings_scrubbed: true,
        capabilities_revoked: true,
        objects_deleted: true,
        generation_fenced: true,
    };
    assert!(complete.complete());
    assert_eq!(
        ConsoleNetworkContainmentTurn::Complete(complete),
        ConsoleNetworkContainmentTurn::Complete(complete),
    );
}

#[test]
fn direct_genet_containment_fences_peer_before_external_caps_and_anchor() {
    let mut cursor = ConsoleNetworkContainmentCursor::with_direct_frame_inventories(0, 32);
    let mut units = Vec::new();
    while cursor.unit() != ConsoleNetworkContainmentUnit::Complete {
        units.push(cursor.select_next());
    }

    let fence = units
        .iter()
        .position(|unit| *unit == ConsoleNetworkContainmentUnit::FenceDirectGenetPeer)
        .expect("paired GENET fence");
    let first_unmap = units
        .iter()
        .position(|unit| *unit == ConsoleNetworkContainmentUnit::UnmapDirectGenetFrame(0))
        .expect("first external-frame unmap");
    let anchor = units
        .iter()
        .position(|unit| *unit == ConsoleNetworkContainmentUnit::RevokeAnchor)
        .expect("anchor revoke");
    assert!(fence < first_unmap);
    assert!(first_unmap < anchor);
    for index in 0..32 {
        let unmap = units
            .iter()
            .position(|unit| *unit == ConsoleNetworkContainmentUnit::UnmapDirectGenetFrame(index))
            .expect("indexed direct-frame unmap");
        let delete = units
            .iter()
            .position(|unit| {
                *unit == ConsoleNetworkContainmentUnit::DeleteDirectGenetFrameCap(index)
            })
            .expect("indexed direct-frame cap delete");
        assert!(fence < unmap && unmap < delete && delete < anchor);
    }
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

#[test]
fn accepted_egress_remains_root_owned_after_shared_page_reuse() {
    let mut boundary = ConsoleNetworkBoundary::new(15).expect("generated contract");
    let generation = boundary.generation();
    let first_bytes = [0x11, 0x22, 0x33, 0x44];
    let second_bytes = [0xaa, 0xbb, 0xcc];
    let mut shared_page = [0u8; SHARED_PAGE_BYTES];

    PacketPage::publish_into(
        &mut shared_page,
        PacketDirection::Egress,
        generation,
        1,
        &first_bytes,
    )
    .expect("first bounded egress publication");
    let retained = boundary
        .accept_egress(&shared_page)
        .expect("root copies the first egress record");

    PacketPage::publish_into(
        &mut shared_page,
        PacketDirection::Egress,
        generation,
        2,
        &second_bytes,
    )
    .expect("shared child page may be reused for the next sequence");
    assert_eq!(
        retained.as_slice(),
        &first_bytes,
        "root-owned pending bytes must not alias the reused child page",
    );
    assert_eq!(
        boundary
            .accept_egress(&shared_page)
            .expect("next exact sequence remains admissible")
            .as_slice(),
        &second_bytes,
    );

    PacketPage::publish_into(
        &mut shared_page,
        PacketDirection::Egress,
        generation,
        1,
        &first_bytes,
    )
    .expect("stale replay remains ABI-well-formed");
    assert_eq!(
        boundary.accept_egress(&shared_page),
        Err(BoundaryError::StaleIdentity),
        "shared-page reuse must not make an old sequence admissible again",
    );
}
