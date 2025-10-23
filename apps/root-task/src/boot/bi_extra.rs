// Author: Lukas Bower
#![allow(dead_code)]

use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_UntypedDesc};

use crate::trace;

const BOOTINFO_HEADER_DUMP_LIMIT: usize = 256;

/// Emits a diagnostic dump of the bootinfo header and extra region.
pub fn dump_bootinfo(
    bootinfo: &'static seL4_BootInfo,
    extra_dump_limit: usize,
) -> Option<(&'static [u8], usize)> {
    let header_len = core::mem::size_of::<seL4_BootInfo>();
    let header_bytes =
        unsafe { core::slice::from_raw_parts(bootinfo as *const _ as *const u8, header_len) };
    trace::hex_dump_slice("bootinfo.header", header_bytes, BOOTINFO_HEADER_DUMP_LIMIT);

    let extra_ptr = bootinfo.extra as *const u8;
    if extra_ptr.is_null() {
        return None;
    }

    let word_bytes = core::mem::size_of::<sel4_sys::seL4_Word>();
    let extra_words = bootinfo.extraLen as usize;
    if extra_words == 0 {
        return None;
    }

    let Some(total_bytes) = extra_words.checked_mul(word_bytes) else {
        return None;
    };

    let extra_slice = unsafe { core::slice::from_raw_parts(extra_ptr, total_bytes) };
    trace::hex_dump_slice("bootinfo.extra", extra_slice, extra_dump_limit);
    Some((extra_slice, total_bytes))
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
