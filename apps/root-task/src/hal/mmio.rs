// Author: Lukas Bower
//! MMIO mapping and access helpers used by drivers.
#![allow(unsafe_code)]

use core::{ptr, slice};
use heapless::Vec;
use sel4_sys::seL4_PageBits;

use super::{HalError, Hardware, MapPerms, MappedRegion};

/// Describes a mapped MMIO window backed by device frames.
#[derive(Clone, Debug)]
pub struct MmioRegion {
    paddr: usize,
    vaddr: *mut u8,
    len: usize,
    frames: Vec<MappedRegion, 16>,
}

impl MmioRegion {
    /// Returns the physical base address of the mapping.
    pub const fn paddr(&self) -> usize {
        self.paddr
    }

    /// Returns the virtual base pointer of the mapping.
    pub const fn as_ptr(&self) -> *mut u8 {
        self.vaddr
    }

    /// Returns the length of the mapping in bytes.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns the mapping as a mutable byte slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.vaddr, self.len) }
    }
}

/// Maps the requested MMIO window as device memory using the provided HAL.
pub fn map_mmio<H>(hal: &mut H, paddr: usize, len: usize) -> Result<MmioRegion, HalError>
where
    H: Hardware<Error = HalError>,
{
    if len == 0 {
        return Err(HalError::Unsupported("mmio len must be non-zero"));
    }

    let page_size = 1usize << (seL4_PageBits as usize);
    let page_mask = page_size - 1;
    let aligned_base = paddr & !page_mask;
    let end = paddr
        .checked_add(len)
        .ok_or(HalError::Unsupported("mmio len overflow"))?;
    let aligned_end = (end + page_mask) & !page_mask;
    let page_count = (aligned_end - aligned_base) / page_size;

    let mut frames: Vec<MappedRegion, 16> = Vec::new();
    for idx in 0..page_count {
        let page_paddr = aligned_base + idx * page_size;
        let frame = hal.map_device(page_paddr)?;
        let mapped = MappedRegion::new(frame, page_size, MapPerms::RW);
        frames
            .push(mapped)
            .map_err(|_| HalError::Unsupported("mmio too large for vec"))?;
    }

    let vaddr = frames
        .first()
        .map(|frame| frame.ptr().as_ptr())
        .ok_or(HalError::Unsupported("mmio mapping failed"))?;

    Ok(MmioRegion {
        paddr: aligned_base,
        vaddr,
        len: aligned_end - aligned_base,
        frames,
    })
}

#[inline(always)]
pub unsafe fn write32(base: &mut MmioRegion, offset: usize, value: u32) {
    unsafe {
        let ptr = base.vaddr.add(offset) as *mut u32;
        ptr::write_volatile(ptr, value);
    }
}

#[inline(always)]
pub unsafe fn write64(base: &mut MmioRegion, offset: usize, value: u64) {
    unsafe {
        let ptr = base.vaddr.add(offset) as *mut u64;
        ptr::write_volatile(ptr, value);
    }
}

#[inline(always)]
pub unsafe fn read32(base: &MmioRegion, offset: usize) -> u32 {
    unsafe {
        let ptr = base.vaddr.add(offset) as *const u32;
        ptr::read_volatile(ptr)
    }
}

#[inline(always)]
pub unsafe fn read64(base: &MmioRegion, offset: usize) -> u64 {
    unsafe {
        let ptr = base.vaddr.add(offset) as *const u64;
        ptr::read_volatile(ptr)
    }
}
