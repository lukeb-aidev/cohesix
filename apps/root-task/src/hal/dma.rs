// Author: Lukas Bower
//! DMA allocation helpers to avoid physical/virtual aliasing mistakes.
#![allow(unsafe_code)]

use core::slice;
use heapless::Vec;

use super::{dma_rmb, dma_wmb, HalError, Hardware};
use crate::sel4::{RamFrame, PAGE_BITS, PAGE_SIZE};

/// Contiguous DMA mapping exposed to drivers.
#[derive(Debug)]
pub struct DmaRegion {
    paddr: usize,
    vaddr: *mut u8,
    len: usize,
    frames: Vec<RamFrame, 32>,
}

impl DmaRegion {
    /// Physical address visible to the device.
    pub const fn paddr(&self) -> usize {
        self.paddr
    }

    /// Virtual pointer used by the CPU.
    pub const fn as_ptr(&self) -> *mut u8 {
        self.vaddr
    }

    /// Total length of the region in bytes.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Immutable slice over the mapped virtual memory.
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.vaddr, self.len) }
    }

    /// Mutable slice over the mapped virtual memory.
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.vaddr, self.len) }
    }

    /// Inserts sentinel values at the end of the region for corruption detection.
    pub fn write_sentinel(&mut self, value: u8) {
        if self.len > 0 {
            let last = self.len - 1;
            unsafe {
                *self.vaddr.add(last) = value;
            }
        }
    }

    /// Validates the sentinel at the end of the region.
    pub fn check_sentinel(&self, value: u8) -> bool {
        if self.len == 0 {
            return true;
        }
        unsafe { *self.vaddr.add(self.len - 1) == value }
    }
}

/// Allocates a contiguous DMA region sized to `len` and aligned to `align`.
pub fn alloc_dma<H>(hal: &mut H, len: usize, align: usize) -> Result<DmaRegion, HalError>
where
    H: Hardware<Error = HalError>,
{
    if len == 0 {
        return Err(HalError::Unsupported("dma len must be non-zero"));
    }

    let align = align.max(1);
    let page_align = 1usize << PAGE_BITS;
    let effective_align = align.max(page_align);
    let total_len = align_up(len, PAGE_SIZE);
    let page_count = (total_len + PAGE_SIZE - 1) / PAGE_SIZE;

    let mut frames: Vec<RamFrame, 32> = Vec::new();
    for _ in 0..page_count {
        let frame = hal.alloc_dma_frame()?;
        frames
            .push(frame)
            .map_err(|_| HalError::Unsupported("dma allocation exceeds vec capacity"))?;
    }

    validate_contiguous(&frames, PAGE_SIZE, effective_align)?;

    let base_vaddr = frames
        .first()
        .map(|f| f.ptr().as_ptr())
        .ok_or(HalError::Unsupported("dma allocation empty"))?;
    let base_paddr = frames
        .first()
        .map(|f| f.paddr())
        .ok_or(HalError::Unsupported("dma allocation missing paddr"))?;

    Ok(DmaRegion {
        paddr: base_paddr,
        vaddr: base_vaddr,
        len: page_count * PAGE_SIZE,
        frames,
    })
}

fn validate_contiguous(frames: &[RamFrame], stride: usize, align: usize) -> Result<(), HalError> {
    for window in frames.windows(2) {
        let a = window[0].paddr();
        let b = window[1].paddr();
        if b != a + stride {
            return Err(HalError::Unsupported("dma allocation not contiguous"));
        }
    }

    let base = frames
        .first()
        .map(|f| f.paddr())
        .ok_or(HalError::Unsupported("dma allocation empty"))?;
    if base % align != 0 {
        return Err(HalError::Unsupported("dma allocation alignment failure"));
    }
    Ok(())
}

#[inline(always)]
pub fn dma_sync_for_device(_region: &DmaRegion) {
    dma_wmb();
}

#[inline(always)]
pub fn dma_sync_for_cpu(_region: &DmaRegion) {
    dma_rmb();
}

#[inline(always)]
fn align_up(val: usize, align: usize) -> usize {
    let mask = align - 1;
    (val + mask) & !mask
}
