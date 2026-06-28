// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Drives the no_std GPU receipt worker-loop entrypoint.
// Author: Lukas Bower
#![allow(dead_code)]

use core::panic::PanicInfo;

use worker_gpu::vm::{
    gpu_control_receipt_event, gpu_identity, gpu_receipt_loop, AttachRequest, EndpointCap,
    WorkerEvent,
};

const BOOTSTRAP_ENDPOINT_CPTR: u64 = 1;
const BOOTSTRAP_LEASE_TTL_MS: u64 = 60_000;

/// Entry point for seL4 GPU worker binaries.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let identity = gpu_identity(0, 0, 1);
    let mut loop_state = match gpu_receipt_loop(identity) {
        Ok(loop_state) => loop_state,
        Err(_) => spin_forever(),
    };
    let endpoint = match EndpointCap::new(BOOTSTRAP_ENDPOINT_CPTR, identity.badge()) {
        Ok(endpoint) => endpoint,
        Err(_) => spin_forever(),
    };
    let _ = loop_state.step(WorkerEvent::Attach(AttachRequest {
        endpoint,
        now_ms: 0,
        lease_ttl_ms: BOOTSTRAP_LEASE_TTL_MS,
    }));
    let _ = loop_state.step(gpu_control_receipt_event(0, 0));
    let mut now_ms = 0u64;
    loop {
        let _ = loop_state.step(WorkerEvent::Poll { now_ms });
        now_ms = now_ms.saturating_add(1_000);
        core::hint::spin_loop();
    }
}

/// Panic handler that spins forever so debuggers can attach.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    spin_forever()
}

fn spin_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
