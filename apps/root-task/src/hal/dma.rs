// Author: Lukas Bower
//! DMA allocation helpers to avoid physical/virtual aliasing mistakes.
#![allow(unsafe_code)]

use core::ptr::NonNull;
use core::slice;

use heapless::Vec;
use log::info;

use super::{dma_rmb, dma_wmb, HalError, Hardware};
use crate::sel4::{RamFrame, PAGE_BITS, PAGE_SIZE};

const LIKELY_PHYS_START: usize = 0x4000_0000;
const LIKELY_PHYS_END: usize = 0x8000_0000;

/// Contiguous DMA mapping exposed to drivers.
#[derive(Debug)]
pub struct DmaBuf {
    paddr: u64,
    vaddr: NonNull<u8>,
    len: usize,
    frames: Vec<RamFrame, 32>,
}

impl DmaBuf {
    /// Physical address visible to the device.
    pub const fn paddr(&self) -> u64 {
        self.paddr
    }

    /// Virtual pointer used by the CPU.
    pub const fn vaddr(&self) -> NonNull<u8> {
        self.vaddr
    }

    /// Total length of the region in bytes (page-aligned).
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Immutable slice over the mapped virtual memory.
    pub fn as_bytes(&self) -> &[u8] {
        self.ensure_mapped();
        unsafe { slice::from_raw_parts(self.vaddr.as_ptr(), self.len) }
    }

    /// Immutable slice alias exposing the mapped region.
    pub fn as_slice(&self) -> &[u8] {
        self.as_bytes()
    }

    /// Mutable slice over the mapped virtual memory.
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        self.ensure_mapped();
        unsafe { slice::from_raw_parts_mut(self.vaddr.as_ptr(), self.len) }
    }

    /// Mutable slice alias exposing the mapped region.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.as_mut_bytes()
    }

    /// Convert the virtual mapping to a typed immutable pointer.
    pub fn as_ptr<T>(&self) -> *const T {
        self.ensure_mapped();
        self.vaddr.cast::<T>().as_ptr()
    }

    /// Convert the virtual mapping to a typed mutable pointer.
    pub fn as_mut_ptr<T>(&mut self) -> *mut T {
        self.ensure_mapped();
        self.vaddr.cast::<T>().as_ptr()
    }

    /// Inserts sentinel values at the end of the region for corruption detection.
    pub fn write_sentinel(&mut self, value: u8) {
        if self.len > 0 {
            let last = self.len - 1;
            unsafe {
                *self.vaddr.as_ptr().add(last) = value;
            }
        }
    }

    /// Validates the sentinel at the end of the region.
    pub fn check_sentinel(&self, value: u8) -> bool {
        if self.len == 0 {
            return true;
        }
        unsafe { *self.vaddr.as_ptr().add(self.len - 1) == value }
    }

    fn ensure_mapped(&self) {
        let vaddr = self.vaddr.as_ptr() as usize;
        if (LIKELY_PHYS_START..LIKELY_PHYS_END).contains(&vaddr) {
            panic!(
                "DMA buffer virtual address 0x{vaddr:x} appears physical; ensure DMA buffers are mapped into VSpace"
            );
        }
    }
}

/// Allocates a contiguous DMA region sized to `len` and aligned to `align`.
pub fn alloc_dma<H>(hal: &mut H, len: usize, align: usize) -> Result<DmaBuf, HalError>
where
    H: Hardware<Error = HalError>,
{
    if len == 0 {
        return Err(HalError::Unsupported("dma len must be non-zero"));
    }

    let align = align.max(16);
    if !align.is_power_of_two() {
        return Err(HalError::Unsupported("dma alignment must be power-of-two"));
    }
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
        .map(|f| f.ptr())
        .ok_or(HalError::Unsupported("dma allocation empty"))?;
    let base_paddr = frames
        .first()
        .map(|f| f.paddr() as u64)
        .ok_or(HalError::Unsupported("dma allocation missing paddr"))?;

    let buf = DmaBuf {
        paddr: base_paddr,
        vaddr: base_vaddr,
        len: page_count * PAGE_SIZE,
        frames,
    };

    info!(
        target: "hal.dma",
        "dma alloc: vaddr=0x{:x} paddr=0x{:x} len={}",
        buf.vaddr.as_ptr() as usize,
        buf.paddr,
        buf.len
    );

    Ok(buf)
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
pub fn dma_sync_for_device(_region: &DmaBuf) {
    dma_wmb();
}

#[inline(always)]
pub fn dma_sync_for_cpu(_region: &DmaBuf) {
    dma_rmb();
}

#[inline(always)]
fn align_up(val: usize, align: usize) -> usize {
    let mask = align - 1;
    (val + mask) & !mask
}
