// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enter the isolated LoRA receipt Worker through the sealed Worker ABI.
// Author: Lukas Bower

use core::panic::PanicInfo;

use worker_task_abi::{WorkerImageMetadata, WorkerRole};

/// Retained image-admission identity for the LoRA Worker executable.
// SAFETY: This immutable pointer-free record has fixed ABI alignment and is
// placed in a dedicated read-only ELF section consumed before task admission.
#[allow(unsafe_code)]
#[used]
#[link_section = ".cohesix.worker"]
static COHESIX_WORKER_METADATA: WorkerImageMetadata =
    WorkerImageMetadata::for_role(WorkerRole::Lora);

/// Minimal executable entrypoint receiving the shared init-page address in `x0`.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn _start(runtime_init_address: usize) -> ! {
    let _ = core::hint::black_box(&COHESIX_WORKER_METADATA);
    worker_heart::target_runtime::run(WorkerRole::Lora, runtime_init_address)
}

/// Panic handler that publishes a bounded fault completion before trapping.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    worker_heart::target_runtime::contain_panic()
}
