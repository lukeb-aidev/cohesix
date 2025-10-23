// Author: Lukas Bower
#![allow(dead_code)]

use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_UntypedDesc};

use crate::sel4::BootInfoExt;
use crate::trace;

const BOOTINFO_HEADER_DUMP_LIMIT: usize = 256;

/// Emits a diagnostic dump of the bootinfo header and extra region.
pub fn dump_bootinfo(
    bootinfo: &'static seL4_BootInfo,
    extra_dump_limit: usize,
) -> Option<(&'static [u8], usize)> {
    let header_bytes = bootinfo.header_bytes();
    trace::hex_dump_slice("bootinfo.header", header_bytes, BOOTINFO_HEADER_DUMP_LIMIT);
    let extra_slice = bootinfo.extra_bytes();
    if extra_slice.is_empty() {
        return None;
    }

    trace::hex_dump_slice("bootinfo.extra", extra_slice, extra_dump_limit);
    Some((extra_slice, extra_slice.len()))
}

/// Minimal mirror of [`seL4_UntypedDesc`] with idiomatic field names for the root task.
#[derive(Clone, Copy)]
pub struct UntypedDesc {
    pub paddr: u64,
    pub size_bits: u8,
    pub is_device: u8,
}

impl From<seL4_UntypedDesc> for UntypedDesc {
    fn from(value: seL4_UntypedDesc) -> Self {
        Self {
            paddr: value.paddr as u64,
            size_bits: value.sizeBits,
            is_device: value.isDevice,
        }
    }
}

/// Returns the first RAM-backed untyped descriptor advertised by the kernel.
pub fn first_regular_untyped_from_extra(bi: &seL4_BootInfo) -> Option<(seL4_CPtr, UntypedDesc)> {
    let count = (bi.untyped.end - bi.untyped.start) as usize;
    let descriptors = &bi.untypedList[..count];

    descriptors.iter().enumerate().find_map(|(index, desc)| {
        if desc.isDevice == 0 {
            let cap = bi.untyped.start + index as seL4_CPtr;
            Some((cap, (*desc).into()))
        } else {
            None
        }
    })
}
