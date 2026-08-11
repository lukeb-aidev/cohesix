// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Run the isolated NineDoor parser over generated seL4 IPC and shared frames.
// Author: Lukas Bower

#![allow(dead_code)]

use core::panic::PanicInfo;
use core::sync::atomic::{compiler_fence, Ordering};

use nine_door_runtime::{
    NamespaceRuntime, RuntimeInitDescriptor, PREPARED_LABEL, REJECTED_LABEL, REQUEST_LABEL,
};
use secure9p_transport::{NamespaceRequestHeader, TransportError, NAMESPACE_HEADER_BYTES};

/// Stable external-QEMU evidence hook reached on each admitted namespace request.
///
/// This hook has no authority and exists only in the explicitly instrumented
/// QEMU child image. GDB may break here and redirect this same child to its
/// existing standard-fault path.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_ninedoor_qemu_evidence_request_handler() {
    core::hint::black_box(cohesix_ninedoor_qemu_evidence_standard_fault as *const ());
    core::hint::black_box(());
}

/// Stable external-QEMU target for a NineDoor standard fault.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_ninedoor_qemu_evidence_standard_fault() -> ! {
    enter_standard_fault()
}

/// Target entrypoint. The loader passes the sealed descriptor address in x0.
#[no_mangle]
pub unsafe extern "C" fn _start(descriptor: *const RuntimeInitDescriptor) -> ! {
    if descriptor.is_null()
        || descriptor as usize & (core::mem::align_of::<RuntimeInitDescriptor>() - 1) != 0
    {
        enter_standard_fault();
    }
    // SAFETY: The supervisor maps a read-only, aligned descriptor for the
    // child before resuming it. The descriptor validates every address and cap
    // slot before the runtime dereferences a shared frame.
    let descriptor = unsafe { core::ptr::read_volatile(descriptor) };
    if !descriptor.valid() {
        enter_standard_fault();
    }
    let mut runtime = match NamespaceRuntime::new(descriptor.generation) {
        Ok(runtime) => runtime,
        Err(_) => enter_standard_fault(),
    };

    loop {
        let mut badge = 0;
        let tag = receive(descriptor, &mut badge);
        let length = tag.length();
        let label = tag.label();
        let request_sequence = if length >= 1 {
            // SAFETY: The generated request ABI carries the exact sequence in MR0.
            unsafe { sel4_sys::seL4_GetMR(0) as u64 }
        } else {
            0
        };
        let shared_len = if length >= 2 {
            // SAFETY: The generated request ABI carries bounded shared bytes in MR1.
            unsafe { sel4_sys::seL4_GetMR(1) as usize }
        } else {
            0
        };
        let result = if label != REQUEST_LABEL
            || length != 2
            || tag.extra_caps() != 0
            || badge != descriptor.request_badge as u64
            || shared_len < NAMESPACE_HEADER_BYTES
            || shared_len > descriptor.frame_bytes as usize
        {
            Err(TransportError::InvalidAbi)
        } else {
            service_request(&mut runtime, descriptor, request_sequence, shared_len)
        };

        let (reply_label, reply_bytes) = match result {
            Ok(bytes) => (PREPARED_LABEL, bytes),
            Err(error) => (REJECTED_LABEL, error.wire_code() as usize),
        };
        // SAFETY: MR0 and MR1 are the complete fixed reply: exact request
        // sequence and response-byte count or typed rejection code.
        unsafe {
            sel4_sys::seL4_SetMR(0, request_sequence);
            sel4_sys::seL4_SetMR(1, reply_bytes as u64);
        }
        compiler_fence(Ordering::Release);
        reply(descriptor.reply_cptr, reply_label);
    }
}

fn service_request(
    runtime: &mut NamespaceRuntime,
    descriptor: RuntimeInitDescriptor,
    request_sequence: u64,
    shared_len: usize,
) -> Result<usize, TransportError> {
    #[cfg(feature = "qemu-evidence")]
    cohesix_ninedoor_qemu_evidence_request_handler();
    // SAFETY: Descriptor validation proves the request mapping is nonzero and
    // page aligned; the supervisor grants only the declared bounded mapping.
    let request = unsafe {
        core::slice::from_raw_parts(descriptor.request_frame_vaddr as *const u8, shared_len)
    };
    compiler_fence(Ordering::Acquire);
    let header = NamespaceRequestHeader::decode(request)?;
    if header.sequence != request_sequence || header.generation != descriptor.generation {
        return Err(TransportError::StaleIdentity);
    }
    let variable_len = (header.path_len as usize).saturating_add(header.payload_len as usize);
    if NAMESPACE_HEADER_BYTES.saturating_add(variable_len) != request.len() {
        return Err(TransportError::PartialFrame);
    }
    let prepared = runtime.prepare(header, &request[NAMESPACE_HEADER_BYTES..])?;
    // SAFETY: Descriptor validation proves the response mapping is distinct,
    // nonzero, page aligned, and at least `frame_bytes` long. Only this child
    // writes it and the supervisor does not read before the IPC reply.
    let response = unsafe {
        core::slice::from_raw_parts_mut(
            descriptor.response_frame_vaddr as *mut u8,
            descriptor.frame_bytes as usize,
        )
    };
    let encoded = prepared.encode(response)?;
    compiler_fence(Ordering::Release);
    Ok(encoded)
}

fn receive(
    descriptor: RuntimeInitDescriptor,
    badge: &mut sel4_sys::seL4_Word,
) -> sel4_sys::seL4_MessageInfo {
    // SAFETY: The validated descriptor names the fixed Read endpoint and the
    // single-owner Reply object installed by the supervisor for this child.
    unsafe { sel4_sys::seL4_Recv(descriptor.endpoint_cptr, badge, descriptor.reply_cptr) }
}

fn reply(reply_cptr: u64, label: u64) {
    let tag = sel4_sys::seL4_MessageInfo::new(label, 0, 0, 2);
    // SAFETY: The runtime owns this Reply cap for exactly the outstanding
    // receive association and performs one reply before receiving again.
    unsafe { sel4_sys::seL4_MCS_Reply(reply_cptr, tag) }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    enter_standard_fault()
}

fn enter_standard_fault() -> ! {
    // SAFETY: `brk` deliberately transfers control to the generated standard
    // fault endpoint. It performs no memory access; root-fault suspends this
    // passive generation, releases an outstanding donor once through the
    // distinct recovery Reply cap, and publishes containment work.
    unsafe {
        core::arch::asm!("brk #0", options(noreturn, nostack, nomem));
    }
}
