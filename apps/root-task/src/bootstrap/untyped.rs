// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/untyped module for root-task.
// Author: Lukas Bower

//! Debug helpers for inspecting untyped capabilities advertised by the kernel.

use crate::sel4::BootInfo;
use heapless::String;
use sel4_sys as sys;

/// Enumerates untyped capabilities and prints their attributes.
#[allow(clippy::cast_lossless)]
pub fn enumerate_and_plan(bootinfo: &'static BootInfo) {
    let total = (bootinfo.untyped.end - bootinfo.untyped.start) as usize;
    let descriptors = &bootinfo.untypedList[..total];
    for (index, desc) in descriptors.iter().enumerate() {
        let cap = bootinfo.untyped.start + index as sys::seL4_CPtr;
        let base = desc.paddr as usize;
        let size_bits = desc.sizeBits as u8;
        let is_device = desc.isDevice != 0;
        let mut line = String::<96>::new();
        use core::fmt::Write as _;
        let _ = write!(
            line,
            "[untyped: cap=0x{cap:04x} size_bits={size_bits} is_device={} paddr=0x{base:016x}]",
            is_device
        );
        log::info!("{line}");
    }
}
