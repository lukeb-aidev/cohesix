// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Run the isolated console-network service over generated seL4 notifications.
// Author: Lukas Bower

use core::panic::PanicInfo;
use core::ptr::{addr_of, addr_of_mut, copy_nonoverlapping, read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

#[cfg(feature = "direct-genet")]
use console_network_runtime::abi::WAKE_DIRECT_GENET_LINK;
#[cfg(feature = "direct-genet")]
use console_network_runtime::abi::{
    DirectGenetLayout, DIRECT_GENET_LAYOUT_BYTES, DIRECT_GENET_LAYOUT_OFFSET,
};
#[cfg(feature = "direct-virtio")]
use console_network_runtime::abi::{
    DirectVirtioLayout, DIRECT_VIRTIO_LAYOUT_BYTES, DIRECT_VIRTIO_LAYOUT_OFFSET,
};
use console_network_runtime::abi::{
    ExchangeKind, ExchangePage, ExchangePageHeader, PacketDirection, PacketPage, PacketPageHeader,
    RuntimeInitDescriptor, CONSOLE_NETWORK_SERVICE_IDENTITY, CONSOLE_PAYLOAD_BYTES,
    CONTROL_CONSUMED_SEQUENCE_OFFSET, ETHERNET_FRAME_BYTES, INGRESS_CONSUMED_SEQUENCE_OFFSET,
    RUNTIME_INIT_DESCRIPTOR_BYTES, WAKE_CONTROL, WAKE_PACKET_RX, WAKE_PUBLICATION_ACK, WAKE_REVOKE,
    WAKE_SHUTDOWN,
};
#[cfg(feature = "direct-virtio")]
use console_network_runtime::abi::{DIRECT_VIRTIO_IRQ_HANDLER_SLOT, WAKE_DIRECT_VIRTIO_IRQ};
#[cfg(feature = "direct-genet")]
use console_network_runtime::{
    direct_genet_command_control_releases_quiesce, direct_genet_command_publication_quiesces,
};
use console_network_runtime::{
    direct_service_repoll_required, ChildTurnReadiness, ChildTurnScheduler, ChildTurnUnit,
    ConsoleNetworkService, ControlApplyOutcome, RuntimeError, ServicePollOutcome,
};
use heapless::Deque;
use smoltcp::iface::SocketStorage;

#[cfg(feature = "direct-genet")]
use crate::direct_genet::DirectGenetLink;
#[cfg(feature = "direct-virtio")]
use crate::direct_virtio::{DirectVirtioError, DirectVirtioNet};
#[cfg(feature = "direct-genet")]
use console_network_runtime::abi::DirectGenetError;

const TCP_BUFFER_BYTES: usize = 32 * 1024;
const COMPLETION_DEPTH: usize = 3;
#[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
const DIRECT_SERVICE_QUANTUM_UNITS: usize = 64;

static mut TCP_RX: [u8; TCP_BUFFER_BYTES] = [0; TCP_BUFFER_BYTES];
static mut TCP_TX: [u8; TCP_BUFFER_BYTES] = [0; TCP_BUFFER_BYTES];
static mut SOCKET_STORAGE: [SocketStorage<'static>; 1] = [SocketStorage::EMPTY];

/// Stable external-QEMU evidence hook reached on each admitted control turn.
///
/// This hook has no authority and exists only in the explicitly instrumented
/// QEMU child image. GDB may redirect this same child to its existing standard
/// fault path or to the bounded MCS-budget exhaustion target below.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_console_network_qemu_evidence_control_handler() {
    core::hint::black_box(cohesix_console_network_qemu_evidence_standard_fault as *const ());
    core::hint::black_box(cohesix_console_network_qemu_evidence_timeout_spin as *const ());
    core::hint::black_box(());
}

/// Stable external-QEMU target for a console-network standard fault.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_console_network_qemu_evidence_standard_fault() -> ! {
    enter_standard_fault()
}

/// Stable external-QEMU target that exhausts the service's admitted MCS budget.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_console_network_qemu_evidence_timeout_spin() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Target entry receives the sealed runtime descriptor address in x0.
#[no_mangle]
pub unsafe extern "C" fn _start(descriptor: *const u8) -> ! {
    if descriptor.is_null()
        || descriptor as usize & (core::mem::align_of::<RuntimeInitDescriptor>() - 1) != 0
    {
        enter_standard_fault();
    }
    #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
    let init_page = descriptor;
    let mut descriptor_bytes = [0u8; RUNTIME_INIT_DESCRIPTOR_BYTES];
    let mut index = 0usize;
    while index < descriptor_bytes.len() {
        // SAFETY: The supervisor maps one page-aligned read-only init page
        // before resuming this child. The canonical descriptor is smaller
        // than that page, so every byte read remains inside the mapping.
        descriptor_bytes[index] = unsafe { read_volatile(descriptor.add(index)) };
        index += 1;
    }
    let descriptor = match RuntimeInitDescriptor::decode(&descriptor_bytes) {
        Ok(descriptor) => descriptor,
        Err(_) => enter_standard_fault(),
    };
    if descriptor.validate().is_err() {
        enter_standard_fault();
    }
    #[cfg(feature = "direct-virtio")]
    let mut direct_device = if descriptor.direct_virtio() {
        let mut layout_bytes = [0u8; DIRECT_VIRTIO_LAYOUT_BYTES];
        let mut layout_index = 0usize;
        while layout_index < layout_bytes.len() {
            // SAFETY: Descriptor validation proves the base init page. The
            // direct-layout offset and exact size are compile-time bounded to
            // that same read-only page.
            layout_bytes[layout_index] =
                unsafe { read_volatile(init_page.add(DIRECT_VIRTIO_LAYOUT_OFFSET + layout_index)) };
            layout_index += 1;
        }
        let layout = match DirectVirtioLayout::decode(&layout_bytes) {
            Ok(layout) => layout,
            Err(_) => enter_standard_fault(),
        };
        match DirectVirtioNet::new(layout, descriptor.mac) {
            Ok(device) => Some(device),
            Err(error) => enter_direct_virtio_fault(error),
        }
    } else {
        None
    };
    #[cfg(not(feature = "direct-virtio"))]
    if descriptor.direct_virtio() {
        enter_standard_fault();
    }
    #[cfg(feature = "direct-genet")]
    let mut direct_genet_link = if descriptor.direct_genet() {
        let mut layout_bytes = [0u8; DIRECT_GENET_LAYOUT_BYTES];
        let mut layout_index = 0usize;
        while layout_index < layout_bytes.len() {
            // SAFETY: Descriptor validation proves the base init page. The
            // direct-GENET layout offset and exact size are compile-time
            // bounded to that same read-only page.
            layout_bytes[layout_index] =
                unsafe { read_volatile(init_page.add(DIRECT_GENET_LAYOUT_OFFSET + layout_index)) };
            layout_index += 1;
        }
        let layout = match DirectGenetLayout::decode(&layout_bytes) {
            Ok(layout) => layout,
            Err(_) => enter_standard_fault(),
        };
        if descriptor.validate_direct_genet_layout(layout).is_err() {
            enter_standard_fault();
        }
        match DirectGenetLink::new(layout) {
            Ok(link) => Some(link),
            Err(error) => enter_direct_genet_fault(error),
        }
    } else {
        None
    };
    #[cfg(not(feature = "direct-genet"))]
    if descriptor.direct_genet() {
        enter_standard_fault();
    }
    install_ipc_buffer(descriptor);

    // SAFETY: This target has one entry invocation. The three static regions
    // are disjoint, never aliased outside this service, and remain mapped for
    // the lifetime of the child TCB.
    let (tcp_rx, tcp_tx, socket_storage) = unsafe {
        (
            core::slice::from_raw_parts_mut(addr_of_mut!(TCP_RX).cast::<u8>(), TCP_BUFFER_BYTES),
            core::slice::from_raw_parts_mut(addr_of_mut!(TCP_TX).cast::<u8>(), TCP_BUFFER_BYTES),
            core::slice::from_raw_parts_mut(
                addr_of_mut!(SOCKET_STORAGE).cast::<SocketStorage<'static>>(),
                1,
            ),
        )
    };
    #[cfg(feature = "direct-virtio")]
    let service_mac = direct_device
        .as_ref()
        .map_or(descriptor.mac, DirectVirtioNet::mac);
    #[cfg(not(feature = "direct-virtio"))]
    let service_mac = descriptor.mac;
    let mut service = match ConsoleNetworkService::new_with_mac(
        descriptor,
        service_mac,
        tcp_rx,
        tcp_tx,
        socket_storage,
    ) {
        Ok(service) => service,
        Err(_) => enter_standard_fault(),
    };

    let packet_rx = descriptor.packet_rx_vaddr as *const PacketPage;
    let packet_tx = descriptor.packet_tx_vaddr as *mut PacketPage;
    let command = descriptor.command_vaddr as *const ExchangePage;
    let event = descriptor.event_vaddr as *mut ExchangePage;
    let mut last_packet_sequence = 0u64;
    let mut last_control_sequence = 0u64;
    let mut pending_output_control: Option<(u64, u64)> = None;
    let mut packet_tx_sequence = 0u64;
    let mut event_sequence = 1u64;
    let mut completions: Deque<(ExchangeKind, u64, u64), COMPLETION_DEPTH> = Deque::new();
    let mut turn_scheduler = ChildTurnScheduler::new();
    publish_exchange(
        event,
        descriptor.generation,
        ExchangeKind::Ready,
        event_sequence,
        0,
        now_ms(descriptor.timer_clock_hz),
        0,
        CONSOLE_NETWORK_SERVICE_IDENTITY,
    );
    signal_durable_child_publication(descriptor, descriptor.supervisor_wake_notification_slot);
    // Ready occupies the one-slot event page. Only root's explicit ACK after it
    // has accepted that record grants one publication credit. Internal units
    // preserve the credit, and exactly one later publication consumes it.
    let mut publication_credit_available = false;
    let mut shutdown_pending = false;
    #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
    let mut direct_service_pending = {
        let mut selected = false;
        #[cfg(feature = "direct-virtio")]
        {
            selected |= direct_device.is_some();
        }
        #[cfg(feature = "direct-genet")]
        {
            selected |= direct_genet_link.is_some();
        }
        selected
    };
    #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
    let mut direct_tx_waiting_for_peer = false;
    #[cfg(feature = "direct-genet")]
    let mut awaiting_root_command_control = false;

    loop {
        #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
        let direct_transport = {
            let mut selected = false;
            #[cfg(feature = "direct-virtio")]
            {
                selected |= direct_device.is_some();
            }
            #[cfg(feature = "direct-genet")]
            {
                selected |= direct_genet_link.is_some();
            }
            selected
        };
        #[cfg(not(any(feature = "direct-virtio", feature = "direct-genet")))]
        let direct_transport = false;
        #[cfg(feature = "direct-genet")]
        let direct_genet_command_quiesced = awaiting_root_command_control;
        #[cfg(not(feature = "direct-genet"))]
        let direct_genet_command_quiesced = false;
        let egress_publication_pending = !direct_transport && service.egress_pending();
        let readiness = ChildTurnReadiness::new(
            !completions.is_empty(),
            service.service_event_pending(),
            egress_publication_pending,
        );
        let selected_unit = turn_scheduler.take_next(readiness);
        let local_poll_eligible = if shutdown_pending {
            publication_credit_available
        } else if direct_transport {
            match selected_unit {
                ChildTurnUnit::PublishCompletion
                | ChildTurnUnit::PublishServiceEvent
                | ChildTurnUnit::PublishEgress => publication_credit_available,
                ChildTurnUnit::PollService
                | ChildTurnUnit::IngestPacket
                | ChildTurnUnit::ApplyControl => true,
                #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
                ChildTurnUnit::Idle => direct_service_pending && !direct_genet_command_quiesced,
                #[cfg(not(any(feature = "direct-virtio", feature = "direct-genet")))]
                ChildTurnUnit::Idle => false,
            }
        } else {
            turn_scheduler.local_poll_eligible(publication_credit_available, readiness)
        };
        let badge = wait_for_work(descriptor, local_poll_eligible);
        if badge & !descriptor.root_wake_mask != 0 {
            enter_standard_fault();
        }
        if badge & WAKE_REVOKE != 0 {
            service.revoke();
            // Immediate containment neither needs nor permits reuse of an
            // unacknowledged event page. Root already fenced the generation
            // before signalling revoke and will suspend/revoke this TCB.
            park_for_teardown(descriptor);
        }
        #[cfg(feature = "direct-virtio")]
        if badge & WAKE_DIRECT_VIRTIO_IRQ != 0 {
            let Some(device) = direct_device.as_ref() else {
                enter_standard_fault();
            };
            if let Err(error) = device.acknowledge_interrupt() {
                enter_direct_virtio_fault(error);
            }
            if !acknowledge_direct_irq_handler() {
                enter_standard_fault();
            }
            direct_service_pending = true;
            direct_tx_waiting_for_peer = false;
        }
        #[cfg(feature = "direct-genet")]
        if badge & WAKE_DIRECT_GENET_LINK != 0 {
            if direct_genet_link.is_none() {
                enter_standard_fault();
            }
            direct_service_pending = true;
            direct_tx_waiting_for_peer = false;
        }

        if badge & WAKE_PUBLICATION_ACK != 0 {
            if publication_credit_available {
                // An ACK is causal proof for exactly one newly observed page.
                // A second credit before the prior one is consumed would make
                // notification coalescing ambiguous, so fail closed.
                enter_standard_fault();
            }
            publication_credit_available = true;
        }
        if badge & WAKE_SHUTDOWN != 0 {
            service.revoke();
            shutdown_pending = true;
        }
        if shutdown_pending {
            if !core::mem::take(&mut publication_credit_available) {
                // The prior event page remains root-owned until Observe->ACK.
                // Retain graceful shutdown without reusing that one-slot page.
                continue;
            }
            event_sequence = next_sequence(event_sequence);
            publish_exchange(
                event,
                descriptor.generation,
                ExchangeKind::ShutdownComplete,
                event_sequence,
                0,
                now_ms(descriptor.timer_clock_hz),
                0,
                b"reason=shutdown",
            );
            signal_durable_child_publication(
                descriptor,
                descriptor.supervisor_wake_notification_slot,
            );
            park_for_teardown(descriptor);
        }

        let packet_wake = !direct_transport && badge & WAKE_PACKET_RX != 0;
        let control_wake = badge & WAKE_CONTROL != 0;
        #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
        if direct_transport && control_wake {
            direct_service_pending = true;
        }
        turn_scheduler.retain_notification(packet_wake, control_wake);
        #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
        let mut unit = turn_scheduler.take_next(ChildTurnReadiness::new(
            !completions.is_empty(),
            service.service_event_pending(),
            !direct_transport && service.egress_pending(),
        ));
        #[cfg(not(any(feature = "direct-virtio", feature = "direct-genet")))]
        let unit = turn_scheduler.take_next(ChildTurnReadiness::new(
            !completions.is_empty(),
            service.service_event_pending(),
            !direct_transport && service.egress_pending(),
        ));
        #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
        if direct_transport
            && direct_service_pending
            && !direct_genet_command_quiesced
            && unit == ChildTurnUnit::Idle
        {
            direct_service_pending = false;
            let mut quantum_units = 0usize;
            let mut cycle_progress = false;
            while quantum_units < DIRECT_SERVICE_QUANTUM_UNITS
                && completions.is_empty()
                && !service.service_event_pending()
            {
                let mut tx_blocked_this_unit = false;
                #[cfg(feature = "direct-virtio")]
                if let Some(device) = direct_device.as_mut() {
                    if let Err(error) = device.poll() {
                        enter_direct_virtio_fault(error);
                    }
                }
                #[cfg(feature = "direct-genet")]
                if let Some(link) = direct_genet_link.as_mut() {
                    if let Err(error) = link.poll() {
                        enter_active_direct_genet_fault(link, error);
                    }
                    signal_direct_genet_peer_if_due(link);
                }
                if service.ingress_available() {
                    let mut ingress = [0u8; ETHERNET_FRAME_BYTES];
                    let mut ingress_length = None;
                    #[cfg(feature = "direct-virtio")]
                    if let Some(device) = direct_device.as_mut() {
                        match device.receive(&mut ingress) {
                            Ok(length) => ingress_length = length,
                            Err(error) => enter_direct_virtio_fault(error),
                        }
                    }
                    #[cfg(feature = "direct-genet")]
                    if let Some(link) = direct_genet_link.as_mut() {
                        match link.receive(&mut ingress) {
                            Ok(length) => ingress_length = length,
                            Err(error) => enter_active_direct_genet_fault(link, error),
                        }
                        signal_direct_genet_peer_if_due(link);
                    }
                    if let Some(length) = ingress_length {
                        if service.ingest_packet(&ingress[..length]).is_err() {
                            enter_standard_fault();
                        }
                        cycle_progress = true;
                    }
                }

                let outcome = match service.poll_service_unit(now_ms(descriptor.timer_clock_hz)) {
                    Ok(outcome) => outcome,
                    Err(_) => enter_standard_fault(),
                };
                if service.egress_pending() {
                    let mut can_transmit = false;
                    #[cfg(feature = "direct-virtio")]
                    if let Some(device) = direct_device.as_mut() {
                        match device.can_transmit() {
                            Ok(ready) => can_transmit = ready,
                            Err(error) => enter_direct_virtio_fault(error),
                        }
                    }
                    #[cfg(feature = "direct-genet")]
                    if let Some(link) = direct_genet_link.as_mut() {
                        match link.can_transmit() {
                            Ok(ready) => can_transmit = ready,
                            Err(error) => enter_active_direct_genet_fault(link, error),
                        }
                    }
                    if can_transmit {
                        direct_tx_waiting_for_peer = false;
                        let mut egress = [0u8; ETHERNET_FRAME_BYTES];
                        let length = match service.take_packet(&mut egress) {
                            Ok(Some(length)) => length,
                            Ok(None) | Err(_) => enter_standard_fault(),
                        };
                        #[cfg(feature = "direct-virtio")]
                        if let Some(device) = direct_device.as_mut() {
                            if let Err(error) = device.transmit(&egress[..length]) {
                                enter_direct_virtio_fault(error);
                            }
                        }
                        #[cfg(feature = "direct-genet")]
                        if let Some(link) = direct_genet_link.as_mut() {
                            if let Err(error) = link.transmit(&egress[..length]) {
                                enter_active_direct_genet_fault(link, error);
                            }
                            signal_direct_genet_peer_if_due(link);
                        }
                        cycle_progress = true;
                    } else {
                        direct_tx_waiting_for_peer = true;
                        tx_blocked_this_unit = true;
                    }
                }
                quantum_units += 1;

                if outcome == ServicePollOutcome::Complete {
                    if let Some((control_sequence, control_connection_id)) = pending_output_control
                    {
                        if service.active_connection_id() != Some(control_connection_id) {
                            pending_output_control = None;
                        } else if service.output_drained_connection() == Some(control_connection_id)
                        {
                            if completions
                                .push_back((
                                    ExchangeKind::OutputDrained,
                                    control_sequence,
                                    control_connection_id,
                                ))
                                .is_err()
                            {
                                enter_standard_fault();
                            }
                            pending_output_control = None;
                        }
                    }
                    if !cycle_progress && !service.egress_pending() {
                        break;
                    }
                    cycle_progress = false;
                }
                if tx_blocked_this_unit {
                    break;
                }
            }
            #[cfg(feature = "direct-virtio")]
            if let Some(device) = direct_device.as_mut() {
                device.flush_notifications();
            }
            #[cfg(feature = "direct-genet")]
            let direct_link_work_pending = if let Some(link) = direct_genet_link.as_mut() {
                signal_direct_genet_peer_if_due(link);
                link.actionable_work_pending(service.ingress_available())
            } else {
                false
            };
            #[cfg(not(feature = "direct-genet"))]
            let direct_link_work_pending = false;
            direct_service_pending |= direct_service_repoll_required(
                quantum_units == DIRECT_SERVICE_QUANTUM_UNITS,
                !completions.is_empty(),
                service.service_event_pending(),
                service.egress_pending(),
                direct_tx_waiting_for_peer,
                direct_link_work_pending,
            );
            unit = turn_scheduler.take_next(ChildTurnReadiness::new(
                !completions.is_empty(),
                service.service_event_pending(),
                false,
            ));
        }
        if unit.is_publication() {
            if !publication_credit_available {
                // An ordinary packet/control wake may return from Wait while a
                // previously retained publication still has priority. Retain
                // both and wait for the causal Observe->ACK; never treat that
                // unrelated wake as permission to reuse a one-slot page.
                continue;
            }
            // Consume before touching retained state or the shared page. A
            // fault during publication cannot leave reusable page authority.
            publication_credit_available = false;
        }
        match unit {
            ChildTurnUnit::PublishCompletion => {
                let Some((kind, related_sequence, connection_id)) = completions.pop_front() else {
                    enter_standard_fault();
                };
                event_sequence = next_sequence(event_sequence);
                publish_exchange(
                    event,
                    descriptor.generation,
                    kind,
                    event_sequence,
                    connection_id,
                    now_ms(descriptor.timer_clock_hz),
                    related_sequence,
                    &[],
                );
                signal_durable_child_publication(
                    descriptor,
                    descriptor.supervisor_wake_notification_slot,
                );
            }
            ChildTurnUnit::PublishServiceEvent => {
                let runtime_event =
                    match service.pop_publication_event(descriptor.max_commands_per_wake) {
                        Ok(Some(event)) => event,
                        Ok(None) | Err(_) => enter_standard_fault(),
                    };
                let runtime_event_kind = runtime_event.kind();
                event_sequence = next_sequence(event_sequence);
                publish_exchange(
                    event,
                    descriptor.generation,
                    runtime_event_kind,
                    event_sequence,
                    runtime_event.connection_id(),
                    runtime_event.now_ms(),
                    0,
                    runtime_event.payload_bytes(),
                );
                signal_durable_child_publication(
                    descriptor,
                    descriptor.supervisor_wake_notification_slot,
                );
                #[cfg(feature = "direct-genet")]
                if direct_genet_command_publication_quiesces(
                    direct_genet_link.is_some(),
                    runtime_event_kind,
                ) {
                    // Root must publish the command's bounded response control
                    // before an ACK or link wake may spend this child's SC on
                    // the 64-unit idle NIC loop. Only a newly sequenced control
                    // record clears the latch; an empty or stale control wake
                    // does not. QEMU direct VirtIO never enters it.
                    direct_service_pending = false;
                    awaiting_root_command_control = true;
                }
            }
            ChildTurnUnit::PublishEgress => {
                let mut egress = [0u8; ETHERNET_FRAME_BYTES];
                let length = match service.take_packet(&mut egress) {
                    Ok(Some(length)) => length,
                    Ok(None) | Err(_) => enter_standard_fault(),
                };
                packet_tx_sequence = next_sequence(packet_tx_sequence);
                publish_packet(
                    packet_tx,
                    descriptor.generation,
                    packet_tx_sequence,
                    &egress[..length],
                );
                signal_durable_child_publication(
                    descriptor,
                    descriptor.packet_tx_wake_notification_slot,
                );
            }
            ChildTurnUnit::PollService => {
                let outcome = match service.poll_service_unit(now_ms(descriptor.timer_clock_hz)) {
                    Ok(outcome) => outcome,
                    Err(_) => enter_standard_fault(),
                };
                if outcome == ServicePollOutcome::Complete {
                    if let Some((control_sequence, control_connection_id)) = pending_output_control
                    {
                        if service.active_connection_id() != Some(control_connection_id) {
                            pending_output_control = None;
                        } else if service.output_drained_connection() == Some(control_connection_id)
                        {
                            if completions
                                .push_back((
                                    ExchangeKind::OutputDrained,
                                    control_sequence,
                                    control_connection_id,
                                ))
                                .is_err()
                            {
                                enter_standard_fault();
                            }
                            pending_output_control = None;
                        }
                    }
                    turn_scheduler.complete(ChildTurnUnit::PollService);
                }
            }
            ChildTurnUnit::IngestPacket => {
                let snapshot = read_packet(packet_rx, descriptor.generation, last_packet_sequence);
                match snapshot {
                    Ok((sequence, packet)) => {
                        if service.ingest_packet(packet.as_slice()).is_err() {
                            enter_standard_fault();
                        }
                        last_packet_sequence = sequence;
                        publish_completion_watermark(event, Some(sequence), None);
                        signal_durable_child_publication(
                            descriptor,
                            descriptor.supervisor_wake_notification_slot,
                        );
                        turn_scheduler.complete(ChildTurnUnit::IngestPacket);
                        turn_scheduler.request_service();
                    }
                    Err(RuntimeError::Backpressure) => {
                        // The sequence-last page proved that this coalesced
                        // notification carried no newer packet. Retire only
                        // that hint; a later publish-and-signal remains
                        // independently observable after the local service
                        // cycle completes.
                        turn_scheduler.complete(ChildTurnUnit::IngestPacket);
                        turn_scheduler.request_service();
                    }
                    Err(_) => enter_standard_fault(),
                }
            }
            ChildTurnUnit::ApplyControl => {
                #[cfg(feature = "qemu-evidence")]
                cohesix_console_network_qemu_evidence_control_handler();
                match read_control(command, descriptor.generation, last_control_sequence) {
                    Ok((sequence, connection_id, kind, payload)) => {
                        let outcome =
                            match service.apply_control(connection_id, kind, payload.as_slice()) {
                                Ok(outcome) => outcome,
                                Err(_) => enter_standard_fault(),
                            };
                        #[cfg(feature = "direct-genet")]
                        let release_command_quiesce = direct_genet_command_control_releases_quiesce(
                            direct_genet_link.is_some(),
                            awaiting_root_command_control,
                            sequence > last_control_sequence,
                            Some(outcome),
                        );
                        last_control_sequence = sequence;
                        if outcome == ControlApplyOutcome::Applied {
                            if matches!(kind, ExchangeKind::SendLine | ExchangeKind::SendBatch) {
                                if pending_output_control.is_some() {
                                    // Root's one-slot control contract must not
                                    // replace undrained output evidence.
                                    enter_standard_fault();
                                }
                                pending_output_control = Some((sequence, connection_id));
                            }
                        } else {
                            // The exact stale input is durably consumed below,
                            // but it did not authorize output. Preserve any
                            // independently pending exact-current drain record.
                        }
                        publish_completion_watermark(event, None, Some(sequence));
                        signal_durable_child_publication(
                            descriptor,
                            descriptor.supervisor_wake_notification_slot,
                        );
                        turn_scheduler.complete(ChildTurnUnit::ApplyControl);
                        #[cfg(feature = "direct-genet")]
                        if release_command_quiesce {
                            awaiting_root_command_control = false;
                        }
                        if !direct_transport {
                            turn_scheduler.request_service();
                        } else {
                            #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
                            {
                                direct_service_pending = true;
                            }
                        }
                    }
                    Err(RuntimeError::Backpressure) => {
                        // WAKE_CONTROL also carries root's service tick. Once
                        // the stable page proves that no newer control exists,
                        // consume this empty hint before retaining exactly one
                        // local service cycle. Otherwise local Poll progress
                        // would revisit the same empty control forever.
                        turn_scheduler.complete(ChildTurnUnit::ApplyControl);
                        if !direct_transport {
                            turn_scheduler.request_service();
                        } else {
                            #[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
                            {
                                direct_service_pending = true;
                            }
                        }
                    }
                    Err(_) => enter_standard_fault(),
                }
            }
            ChildTurnUnit::Idle => {}
        }
    }
}

#[cfg(feature = "direct-genet")]
fn signal_direct_genet_peer_if_due(link: &mut DirectGenetLink) {
    if let Some(slot) = link.take_peer_wake() {
        signal_slot(slot);
    }
}

#[cfg(feature = "direct-genet")]
fn enter_active_direct_genet_fault(link: &mut DirectGenetLink, error: DirectGenetError) -> ! {
    link.fail_closed(error);
    signal_direct_genet_peer_if_due(link);
    enter_direct_genet_fault(error)
}

#[cfg(feature = "direct-virtio")]
fn acknowledge_direct_irq_handler() -> bool {
    let mut mr0 = 0;
    let mut mr1 = 0;
    let mut mr2 = 0;
    let mut mr3 = 0;
    // SAFETY: Descriptor admission and child construction prove that slot 7
    // contains only the IRQHandler for the exclusively admitted QEMU VirtIO
    // device. Device status is cleared before this exact libsel4 Ack shape,
    // so a subsequent edge represents new queue or configuration work.
    unsafe {
        let tag = sel4_sys::seL4_MessageInfo::new(
            sel4_sys::invocation_label_IRQAckIRQ as sel4_sys::seL4_Word,
            0,
            0,
            0,
        );
        let output = sel4_sys::seL4_CallWithMRs(
            DIRECT_VIRTIO_IRQ_HANDLER_SLOT as sel4_sys::seL4_CPtr,
            tag,
            &mut mr0,
            &mut mr1,
            &mut mr2,
            &mut mr3,
        );
        sel4_sys::seL4_MessageInfo_get_label(output) == sel4_sys::seL4_NoError as u64
    }
}

fn install_ipc_buffer(descriptor: RuntimeInitDescriptor) {
    // SAFETY: Descriptor validation proves the ABI-aligned IPC-buffer address;
    // the supervisor maps and binds that frame before resuming this child.
    unsafe {
        sel4_sys::seL4_SetIPCBuffer(descriptor.ipc_buffer_vaddr as *mut sel4_sys::seL4_IPCBuffer);
    }
}

/// Admit one material child unit through the exact notification gate.
///
/// The first call follows the published `Ready` event, and every ordinary
/// child-unit arm falls through to another call on loop re-entry. Retained
/// private work and an explicitly credited publication use one nonblocking
/// Poll, so every iteration still validates newly coalesced revoke, shutdown,
/// ACK, packet, and control bits before selecting one successor. The existing
/// MCS budget remains the cumulative temporal bound for that finite retained
/// burst. Genuine idle and an uncredited publication block directly, so they
/// cannot consume budget while awaiting root authority. Terminal teardown
/// deliberately uses its separate wait-only park loop.
fn wait_for_work(descriptor: RuntimeInitDescriptor, local_poll_eligible: bool) -> u64 {
    let mut badge: sel4_sys::seL4_Word = 0;
    // SAFETY: Validation fixes this CPtr to the child's sole Read notification
    // cap. Poll is used only when retained scheduler state or publication
    // credit already authorizes exactly one successor; the next loop iteration
    // rechecks every notification bit before another unit. Otherwise Wait
    // blocks directly for an idle prompt or the causal ACK after publication.
    // Keeping the syscalls in this existing block preserves the unsafe surface.
    let _ = unsafe {
        if local_poll_eligible {
            sel4_sys::seL4_Poll(
                descriptor.child_wake_notification_slot as sel4_sys::seL4_CPtr,
                &mut badge,
            )
        } else {
            sel4_sys::seL4_Wait(
                descriptor.child_wake_notification_slot as sel4_sys::seL4_CPtr,
                &mut badge,
            )
        }
    };
    badge as u64
}

fn signal_slot(slot: u32) {
    fence(Ordering::Release);
    // SAFETY: Descriptor validation fixes every passed slot to one of the
    // child's write-only publication caps. Their badges are minted by root and
    // are never child-selected.
    unsafe {
        sel4_sys::seL4_Signal(slot as sel4_sys::seL4_CPtr);
    }
}

/// Preserve the existing publication notification before waking root-control.
///
/// Root consumes the original packet/event badge to select and acknowledge the
/// sequence-last record. Physical WiFi/direct-GENET profiles then signal the
/// Pi root-control fan-in after that semantic edge. Direct-VirtIO QEMU retains
/// its qualified signal-only path without leaving an unconsumed Pi fan-in
/// latched; neither edge is publication authority without the durable page
/// transition.
fn signal_durable_child_publication(descriptor: RuntimeInitDescriptor, publication_slot: u32) {
    signal_slot(publication_slot);
    if !descriptor.direct_virtio() {
        signal_slot(descriptor.root_control_wake_notification_slot);
    }
}

/// Publish exact input ownership retirement without consuming the semantic
/// event slot or its global publication credit.
#[inline(never)]
fn publish_completion_watermark(
    event_page: *mut ExchangePage,
    packet_sequence: Option<u64>,
    control_sequence: Option<u64>,
) {
    if packet_sequence.is_none() && control_sequence.is_none() {
        enter_standard_fault();
    }
    // SAFETY: Descriptor validation fixes this page-aligned child-produced
    // mapping. The two aligned trailer words lie inside that page and are
    // disjoint from the sequence-last event body. Root never writes them.
    unsafe {
        let page = event_page.cast::<u8>();
        if let Some(sequence) = packet_sequence {
            write_volatile(
                page.add(INGRESS_CONSUMED_SEQUENCE_OFFSET).cast::<u64>(),
                sequence.to_le(),
            );
        }
        if let Some(sequence) = control_sequence {
            write_volatile(
                page.add(CONTROL_CONSUMED_SEQUENCE_OFFSET).cast::<u64>(),
                sequence.to_le(),
            );
        }
        fence(Ordering::Release);
    }
}

#[inline(never)]
fn read_packet(
    page: *const PacketPage,
    generation: u64,
    last_sequence: u64,
) -> Result<(u64, heapless::Vec<u8, ETHERNET_FRAME_BYTES>), RuntimeError> {
    // SAFETY: Init validation proves this page-aligned mapping. Root is the
    // sole producer and keeps the one-slot record stable until the child
    // publishes its exact completion. Primitive volatile header reads prevent
    // the compiler from materializing or caching the 4-KiB aggregate.
    unsafe {
        let commit = addr_of!((*page).committed_sequence);
        let first = read_volatile(commit);
        if first == 0 || first <= last_sequence {
            return Err(RuntimeError::Backpressure);
        }
        fence(Ordering::Acquire);
        let header = PacketPageHeader {
            magic: read_volatile(addr_of!((*page).magic)),
            abi_version: read_volatile(addr_of!((*page).abi_version)),
            direction: read_volatile(addr_of!((*page).direction)),
            generation: read_volatile(addr_of!((*page).generation)),
            sequence: read_volatile(addr_of!((*page).sequence)),
            committed_sequence: read_volatile(commit),
            packet_len: read_volatile(addr_of!((*page).packet_len)),
            flags: read_volatile(addr_of!((*page).flags)),
            reserved0: read_volatile(addr_of!((*page).reserved0)),
        };
        let (direction, packet_len) = header
            .validate(generation, last_sequence)
            .map_err(|_| RuntimeError::PacketBound)?;
        if direction != PacketDirection::Ingress || first != header.sequence {
            return Err(RuntimeError::PacketBound);
        }
        let mut bytes = [0u8; ETHERNET_FRAME_BYTES];
        copy_nonoverlapping(
            addr_of!((*page).packet).cast::<u8>(),
            bytes.as_mut_ptr(),
            packet_len,
        );
        fence(Ordering::Acquire);
        let second = read_volatile(commit);
        if first != second {
            return Err(RuntimeError::PacketBound);
        }
        let mut packet = heapless::Vec::new();
        packet
            .extend_from_slice(&bytes[..packet_len])
            .map_err(|_| RuntimeError::PacketBound)?;
        Ok((header.sequence, packet))
    }
}

#[inline(never)]
fn read_control(
    page: *const ExchangePage,
    generation: u64,
    last_sequence: u64,
) -> Result<
    (
        u64,
        u64,
        ExchangeKind,
        heapless::Vec<u8, { console_network_runtime::abi::CONSOLE_PAYLOAD_BYTES }>,
    ),
    RuntimeError,
> {
    // SAFETY: Descriptor validation fixes this page mapping and root is its
    // sole producer. Only primitive header fields are volatile-read; the
    // declared payload prefix is copied after its bound is validated.
    unsafe {
        let commit = addr_of!((*page).committed_sequence);
        let first = read_volatile(commit);
        if first == 0 || first <= last_sequence {
            return Err(RuntimeError::Backpressure);
        }
        fence(Ordering::Acquire);
        let header = ExchangePageHeader {
            magic: read_volatile(addr_of!((*page).magic)),
            abi_version: read_volatile(addr_of!((*page).abi_version)),
            kind: read_volatile(addr_of!((*page).kind)),
            record_bytes: read_volatile(addr_of!((*page).record_bytes)),
            payload_len: read_volatile(addr_of!((*page).payload_len)),
            reserved0: read_volatile(addr_of!((*page).reserved0)),
            generation: read_volatile(addr_of!((*page).generation)),
            sequence: read_volatile(addr_of!((*page).sequence)),
            connection_id: read_volatile(addr_of!((*page).connection_id)),
            now_ms: read_volatile(addr_of!((*page).now_ms)),
            related_sequence: read_volatile(addr_of!((*page).related_sequence)),
            committed_sequence: read_volatile(commit),
        };
        let (kind, payload_len) = header
            .validate(generation, last_sequence, true)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        if first != header.sequence {
            return Err(RuntimeError::ConsoleFrame);
        }
        let mut bytes = [0u8; CONSOLE_PAYLOAD_BYTES];
        copy_nonoverlapping(
            addr_of!((*page).payload).cast::<u8>(),
            bytes.as_mut_ptr(),
            payload_len,
        );
        fence(Ordering::Acquire);
        let second = read_volatile(commit);
        if first != second {
            return Err(RuntimeError::ConsoleFrame);
        }
        let mut payload = heapless::Vec::new();
        payload
            .extend_from_slice(&bytes[..payload_len])
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        Ok((header.sequence, header.connection_id, kind, payload))
    }
}

#[inline(never)]
fn publish_packet(page: *mut PacketPage, generation: u64, sequence: u64, packet: &[u8]) {
    let header =
        match PacketPageHeader::staged(PacketDirection::Egress, generation, sequence, packet.len())
        {
            Ok(header) => header,
            Err(_) => enter_standard_fault(),
        };
    // SAFETY: The descriptor fixes this page-aligned child-produced mapping.
    // Each scalar and the declared payload prefix remain in that page. Root
    // does not mutate it before consuming the sequence-last publication.
    unsafe {
        write_volatile(addr_of_mut!((*page).committed_sequence), 0);
        fence(Ordering::Release);
        write_volatile(addr_of_mut!((*page).magic), header.magic);
        write_volatile(addr_of_mut!((*page).abi_version), header.abi_version);
        write_volatile(addr_of_mut!((*page).direction), header.direction);
        write_volatile(addr_of_mut!((*page).generation), header.generation);
        write_volatile(addr_of_mut!((*page).sequence), header.sequence);
        write_volatile(addr_of_mut!((*page).packet_len), header.packet_len);
        write_volatile(addr_of_mut!((*page).flags), header.flags);
        write_volatile(addr_of_mut!((*page).reserved0), header.reserved0);
        copy_nonoverlapping(
            packet.as_ptr(),
            addr_of_mut!((*page).packet).cast::<u8>(),
            packet.len(),
        );
        fence(Ordering::Release);
        write_volatile(addr_of_mut!((*page).committed_sequence), sequence);
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn publish_exchange(
    page: *mut ExchangePage,
    generation: u64,
    kind: ExchangeKind,
    sequence: u64,
    connection_id: u64,
    observation_ms: u64,
    related_sequence: u64,
    payload: &[u8],
) {
    let header = match ExchangePageHeader::staged(
        kind,
        generation,
        sequence,
        connection_id,
        observation_ms,
        related_sequence,
        payload.len(),
    ) {
        Ok(header) => header,
        Err(_) => enter_standard_fault(),
    };
    // SAFETY: The descriptor fixes this page-aligned child-produced mapping.
    // The bounded body is stable until root consumes this one-slot record.
    unsafe {
        write_volatile(addr_of_mut!((*page).committed_sequence), 0);
        fence(Ordering::Release);
        write_volatile(addr_of_mut!((*page).magic), header.magic);
        write_volatile(addr_of_mut!((*page).abi_version), header.abi_version);
        write_volatile(addr_of_mut!((*page).kind), header.kind);
        write_volatile(addr_of_mut!((*page).record_bytes), header.record_bytes);
        write_volatile(addr_of_mut!((*page).payload_len), header.payload_len);
        write_volatile(addr_of_mut!((*page).reserved0), header.reserved0);
        write_volatile(addr_of_mut!((*page).generation), header.generation);
        write_volatile(addr_of_mut!((*page).sequence), header.sequence);
        write_volatile(addr_of_mut!((*page).connection_id), header.connection_id);
        write_volatile(addr_of_mut!((*page).now_ms), header.now_ms);
        write_volatile(
            addr_of_mut!((*page).related_sequence),
            header.related_sequence,
        );
        copy_nonoverlapping(
            payload.as_ptr(),
            addr_of_mut!((*page).payload).cast::<u8>(),
            payload.len(),
        );
        fence(Ordering::Release);
        write_volatile(addr_of_mut!((*page).committed_sequence), sequence);
    }
}

fn now_ms(timer_clock_hz: u64) -> u64 {
    let counter: u64;
    // SAFETY: The selected seL4 profile exports CNTVCT_EL0 to userspace and
    // the sealed descriptor carries that generated profile's TIMER_CLOCK_HZ.
    unsafe {
        core::arch::asm!("mrs {value}, cntvct_el0", value = out(reg) counter, options(nostack, nomem));
    }
    let seconds = counter / timer_clock_hz;
    let remainder = counter % timer_clock_hz;
    seconds
        .saturating_mul(1000)
        .saturating_add(remainder.saturating_mul(1000) / timer_clock_hz)
}

fn next_sequence(sequence: u64) -> u64 {
    sequence.saturating_add(1).max(1)
}

fn park_for_teardown(descriptor: RuntimeInitDescriptor) -> ! {
    loop {
        let mut ignored_badge = 0;
        // SAFETY: The supervisor keeps this receive-only notification until it
        // suspends and deletes the child. Re-waiting consumes no service work.
        let _ = unsafe {
            sel4_sys::seL4_Wait(
                descriptor.child_wake_notification_slot as sel4_sys::seL4_CPtr,
                &mut ignored_badge,
            )
        };
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    enter_standard_fault()
}

fn enter_standard_fault() -> ! {
    // SAFETY: `brk` deliberately transfers control to the supervisor-installed
    // standard fault endpoint and performs no memory access.
    unsafe {
        core::arch::asm!("brk #0", options(noreturn, nostack, nomem));
    }
}

#[cfg(feature = "direct-virtio")]
fn enter_direct_virtio_fault(error: DirectVirtioError) -> ! {
    match error {
        DirectVirtioError::InvalidDevice => enter_direct_fault_code::<1>(),
        DirectVirtioError::FeatureNegotiation => enter_direct_fault_code::<2>(),
        DirectVirtioError::QueueUnavailable => enter_direct_fault_code::<3>(),
        DirectVirtioError::QueueCorrupt => enter_direct_fault_code::<4>(),
        DirectVirtioError::MacMismatch => enter_direct_fault_code::<5>(),
        DirectVirtioError::FrameBound => enter_direct_fault_code::<6>(),
        DirectVirtioError::TxBackpressure => enter_direct_fault_code::<7>(),
        DirectVirtioError::RxDescriptorCorrupt => enter_direct_fault_code::<8>(),
        DirectVirtioError::RxLengthZero => enter_direct_fault_code::<9>(),
        DirectVirtioError::RxLengthHeaderOnly => enter_direct_fault_code::<10>(),
        DirectVirtioError::RxLengthTooLong => enter_direct_fault_code::<11>(),
        DirectVirtioError::RxBufferCountCorrupt => enter_direct_fault_code::<12>(),
    }
}

#[cfg(feature = "direct-genet")]
fn enter_direct_genet_fault(error: DirectGenetError) -> ! {
    match error {
        DirectGenetError::InvalidIdentity => enter_direct_fault_code::<32>(),
        DirectGenetError::InvalidLayout => enter_direct_fault_code::<33>(),
        DirectGenetError::StaleGeneration => enter_direct_fault_code::<34>(),
        DirectGenetError::InvalidCursor => enter_direct_fault_code::<35>(),
        DirectGenetError::InvalidSequence => enter_direct_fault_code::<36>(),
        DirectGenetError::InvalidBound => enter_direct_fault_code::<37>(),
        DirectGenetError::Empty => enter_direct_fault_code::<38>(),
        DirectGenetError::Backpressure => enter_direct_fault_code::<39>(),
        DirectGenetError::StateChanged => enter_direct_fault_code::<40>(),
        DirectGenetError::Poisoned(_) => enter_direct_fault_code::<41>(),
    }
}

#[cfg(any(feature = "direct-virtio", feature = "direct-genet"))]
#[cold]
#[inline(never)]
fn enter_direct_fault_code<const CODE: u16>() -> ! {
    // SAFETY: The immediate is a typed, bounded direct-transport fault reason.
    // `brk` transfers control to the supervisor-installed standard fault
    // endpoint and performs no memory access.
    unsafe {
        core::arch::asm!("brk #{code}", code = const CODE, options(noreturn, nostack, nomem));
    }
}
