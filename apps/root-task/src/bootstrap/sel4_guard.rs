// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/sel4_guard module for root-task.
// Author: Lukas Bower
//! Guard rails for early seL4 invocations to detect null capability usage.
#![allow(dead_code)]

use core::fmt::Write;

use heapless::String;
use sel4_sys::seL4_CPtr;
use spin::Once;

use crate::bootstrap::log::force_uart_line;
use crate::sel4::BootInfoView;

#[derive(Clone, Copy)]
struct GuardBootInfo {
    init_cnode: seL4_CPtr,
    init_bits: u8,
    empty_start: seL4_CPtr,
    empty_end: seL4_CPtr,
}

static GUARD_BOOTINFO: Once<GuardBootInfo> = Once::new();

/// Records bootinfo metadata for panic reporting if a null capability is detected.
pub fn install_bootinfo(view: &BootInfoView) {
    let (empty_start, empty_end) = view.init_cnode_empty_range();
    let info = GuardBootInfo {
        init_cnode: view.root_cnode_cap(),
        init_bits: view.init_cnode_bits() as u8,
        empty_start,
        empty_end,
    };
    let _ = GUARD_BOOTINFO.call_once(|| info);
}

/// Emits a UART breadcrumb describing an imminent seL4 invocation.
pub fn uart_breadcrumb(stage: &str, call: &str, detail: &str) {
    let mut line = String::<192>::new();
    let _ = write!(line, "[guard] stage={stage} call={call} {detail}");
    force_uart_line(line.as_str());
}

/// Guards a capability pointer, panicking via UART if it resolves to `seL4_CapNull`.
pub fn guard_cptr(stage: &str, cap_name: &str, cptr: seL4_CPtr) -> seL4_CPtr {
    if cptr == sel4_sys::seL4_CapNull {
        let mut line = String::<224>::new();
        if let Some(info) = GUARD_BOOTINFO.get() {
            let _ = write!(
                line,
                "[sel4-guard] stage={stage} cap={cap_name} cptr=0x{cptr:04x} init_cnode=0x{root:04x} init_bits={bits} empty=[0x{start:04x}..0x{end:04x})",
                root = info.init_cnode,
                bits = info.init_bits,
                start = info.empty_start,
                end = info.empty_end,
            );
        } else {
            let _ = write!(
                line,
                "[sel4-guard] stage={stage} cap={cap_name} cptr=0x{cptr:04x} bootinfo=unavailable"
            );
        }
        force_uart_line(line.as_str());
        panic!("[sel4-guard] null capability used during {stage} ({cap_name})");
    }
    cptr
}
