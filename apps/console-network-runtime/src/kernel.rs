// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Run the isolated console-network service over generated seL4 notifications.
// Author: Lukas Bower

use core::panic::PanicInfo;
use core::ptr::{addr_of, addr_of_mut, copy_nonoverlapping, read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use console_network_runtime::abi::{
    ExchangeKind, ExchangePage, ExchangePageHeader, PacketDirection, PacketPage, PacketPageHeader,
    RuntimeInitDescriptor, CONSOLE_PAYLOAD_BYTES, ETHERNET_FRAME_BYTES,
    RUNTIME_INIT_DESCRIPTOR_BYTES, WAKE_CONTROL, WAKE_PACKET_RX, WAKE_REVOKE, WAKE_SHUTDOWN,
};
use console_network_runtime::{ConsoleNetworkService, RuntimeError};
use heapless::Deque;
use smoltcp::iface::SocketStorage;

const TCP_BUFFER_BYTES: usize = 32 * 1024;
const COMPLETION_DEPTH: usize = 3;

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
    let mut service = match ConsoleNetworkService::new(descriptor, tcp_rx, tcp_tx, socket_storage) {
        Ok(service) => service,
        Err(_) => enter_standard_fault(),
    };

    let packet_rx = descriptor.packet_rx_vaddr as *const PacketPage;
    let packet_tx = descriptor.packet_tx_vaddr as *mut PacketPage;
    let command = descriptor.command_vaddr as *const ExchangePage;
    let event = descriptor.event_vaddr as *mut ExchangePage;
    let mut last_packet_sequence = 0u64;
    let mut last_control_sequence = 0u64;
    let mut last_output_drained_sequence = 0u64;
    let mut packet_tx_sequence = 0u64;
    let mut event_sequence = 1u64;
    let mut completions: Deque<(ExchangeKind, u64, u64), COMPLETION_DEPTH> = Deque::new();
    publish_exchange(
        event,
        descriptor.generation,
        ExchangeKind::Ready,
        event_sequence,
        0,
        now_ms(descriptor.timer_clock_hz),
        0,
        b"console-network-service/v1",
    );
    signal_slot(descriptor.supervisor_wake_notification_slot);

    loop {
        let badge = wait_for_work(descriptor);
        if badge & !descriptor.root_wake_mask != 0 {
            enter_standard_fault();
        }
        if badge & WAKE_REVOKE != 0 {
            service.revoke();
            event_sequence = next_sequence(event_sequence);
            publish_exchange(
                event,
                descriptor.generation,
                ExchangeKind::ShutdownComplete,
                event_sequence,
                0,
                now_ms(descriptor.timer_clock_hz),
                0,
                b"reason=revoked",
            );
            signal_slot(descriptor.supervisor_wake_notification_slot);
            park_for_teardown(descriptor);
        }
        if badge & WAKE_PACKET_RX != 0 {
            let snapshot = read_packet(packet_rx, descriptor.generation, last_packet_sequence);
            match snapshot {
                Ok((sequence, packet)) => {
                    last_packet_sequence = sequence;
                    if service.ingest_packet(packet.as_slice()).is_err() {
                        enter_standard_fault();
                    }
                    if completions
                        .push_back((ExchangeKind::PacketConsumed, sequence, 0))
                        .is_err()
                    {
                        enter_standard_fault();
                    }
                }
                Err(RuntimeError::Backpressure) => {}
                Err(_) => enter_standard_fault(),
            }
        }
        if badge & WAKE_CONTROL != 0 {
            #[cfg(feature = "qemu-evidence")]
            cohesix_console_network_qemu_evidence_control_handler();
            match read_control(command, descriptor.generation, last_control_sequence) {
                Ok((sequence, kind, payload)) => {
                    last_control_sequence = sequence;
                    let payload = match core::str::from_utf8(payload.as_slice()) {
                        Ok(payload) => payload,
                        Err(_) => enter_standard_fault(),
                    };
                    if service.apply_control(kind, payload).is_err() {
                        enter_standard_fault();
                    }
                    if completions
                        .push_back((ExchangeKind::ControlCompleted, sequence, 0))
                        .is_err()
                    {
                        enter_standard_fault();
                    }
                }
                Err(RuntimeError::Backpressure) => {}
                Err(_) => enter_standard_fault(),
            }
        }

        if badge & WAKE_SHUTDOWN != 0 {
            service.revoke();
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
            signal_slot(descriptor.supervisor_wake_notification_slot);
            park_for_teardown(descriptor);
        }

        if service.poll(now_ms(descriptor.timer_clock_hz)).is_err() {
            enter_standard_fault();
        }
        if last_control_sequence > last_output_drained_sequence {
            if let Some(connection_id) = service.output_drained_connection() {
                if completions
                    .push_back((
                        ExchangeKind::OutputDrained,
                        last_control_sequence,
                        connection_id,
                    ))
                    .is_err()
                {
                    enter_standard_fault();
                }
                last_output_drained_sequence = last_control_sequence;
            }
        }
        if let Some((kind, related_sequence, connection_id)) = completions.pop_front() {
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
            signal_slot(descriptor.supervisor_wake_notification_slot);
        } else if let Some(runtime_event) = service.pop_event() {
            let payload = match runtime_event.payload() {
                Ok(payload) => payload,
                Err(_) => enter_standard_fault(),
            };
            event_sequence = next_sequence(event_sequence);
            publish_exchange(
                event,
                descriptor.generation,
                runtime_event.kind(),
                event_sequence,
                runtime_event.connection_id(),
                runtime_event.now_ms(),
                0,
                payload.as_bytes(),
            );
            signal_slot(descriptor.supervisor_wake_notification_slot);
        }
        let mut egress = [0u8; ETHERNET_FRAME_BYTES];
        match service.take_packet(&mut egress) {
            Ok(Some(length)) => {
                packet_tx_sequence = next_sequence(packet_tx_sequence);
                publish_packet(
                    packet_tx,
                    descriptor.generation,
                    packet_tx_sequence,
                    &egress[..length],
                );
                signal_slot(descriptor.packet_tx_wake_notification_slot);
            }
            Ok(None) => {}
            Err(_) => enter_standard_fault(),
        }
    }
}

fn install_ipc_buffer(descriptor: RuntimeInitDescriptor) {
    // SAFETY: Descriptor validation proves the ABI-aligned IPC-buffer address;
    // the supervisor maps and binds that frame before resuming this child.
    unsafe {
        sel4_sys::seL4_SetIPCBuffer(descriptor.ipc_buffer_vaddr as *mut sel4_sys::seL4_IPCBuffer);
    }
}

fn wait_for_work(descriptor: RuntimeInitDescriptor) -> u64 {
    let mut badge: sel4_sys::seL4_Word = 0;
    // SAFETY: Validation fixes this CPtr to the child's sole Read notification
    // cap. Waiting blocks its active MCS scheduling context when idle.
    let _ = unsafe {
        sel4_sys::seL4_Wait(
            descriptor.child_wake_notification_slot as sel4_sys::seL4_CPtr,
            &mut badge,
        )
    };
    badge as u64
}

fn signal_slot(slot: u32) {
    fence(Ordering::Release);
    // SAFETY: The descriptor fixes slots 3 and 4 to separate Write-only caps;
    // their one-hot badges are minted by the supervisor and not child-selected.
    unsafe {
        sel4_sys::seL4_Signal(slot as sel4_sys::seL4_CPtr);
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
        Ok((header.sequence, kind, payload))
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
