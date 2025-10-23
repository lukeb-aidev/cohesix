// Author: Lukas Bower
#![allow(dead_code)]

use core::mem::size_of;
use sel4_sys::*;

/// Minimal mirror of seL4_BootInfoHeader (matches kernel ABI)
#[repr(C)]
#[derive(Clone, Copy)]
struct BootInfoHeader {
    id: u32,
    len: u32,
}

// Kernel tag for the untyped caps block (stable ABI value):
const SEL4_BOOTINFO_HEADER_UNTYPED_CAPS: u32 = 6;

// Minimal mirror of seL4_UntypedDesc (subset we need; packed to match ABI)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UntypedDesc {
    pub paddr: u64,
    pub size_bits: u8,
    pub is_device: u8,
    pub padding0: u16,
    pub padding1: u32,
}

/// Iterate BootInfo extra region and return (cap_index, desc) for the first regular untyped.
pub fn first_regular_untyped_from_extra(bi: &seL4_BootInfo) -> Option<(seL4_CPtr, UntypedDesc)> {
    // extra region starts right after the BootInfo header
    let extra_ptr = (bi as *const _ as usize + bi.extraBIPages.start as usize) as *const u8;
    let extra_bytes = (bi.extraBIPages.end - bi.extraBIPages.start) as usize;

    // Fallback for older ABI (your earlier prints show header=64, extraLen words):
    let (extra_ptr, extra_bytes) = if extra_bytes == 0 {
        let ptr = (bi as *const _ as usize + 64) as *const u8;
        let bytes = (bi.extraLen as usize) * 4;
        (ptr, bytes)
    } else {
        (extra_ptr, extra_bytes)
    };

    if extra_bytes < size_of::<BootInfoHeader>() {
        return None;
    }

    // Walk blocks
    let mut off = 0usize;
    unsafe {
        while off + size_of::<BootInfoHeader>() <= extra_bytes {
            let hdr = *(extra_ptr.add(off) as *const BootInfoHeader);
            if hdr.len == 0 {
                break;
            }
            let next = off + hdr.len as usize;

            if hdr.id == SEL4_BOOTINFO_HEADER_UNTYPED_CAPS {
                if off + 8 > extra_bytes {
                    break;
                }
                let count = *(extra_ptr.add(off + 8) as *const u32) as usize;
                let mut rec_off = off + 16;
                for i in 0..count {
                    if rec_off + size_of::<UntypedDesc>() > extra_bytes {
                        break;
                    }
                    let desc = *(extra_ptr.add(rec_off) as *const UntypedDesc);
                    let cap = (bi.untyped.start as usize + i) as seL4_CPtr;
                    if desc.is_device == 0 {
                        return Some((cap, desc));
                    }
                    rec_off += size_of::<UntypedDesc>();
                }
            }

            off = next;
        }
    }
    None
}
