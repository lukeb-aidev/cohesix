// Author: Lukas Bower
//! Architecture-neutral DMA barrier helpers.
#![allow(unsafe_code)]

use core::sync::atomic::{fence, Ordering};

#[inline(always)]
pub fn dma_wmb() {
    fence(Ordering::Release);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dmb ishst", options(nostack, preserves_flags));
    }
}

#[inline(always)]
pub fn dma_rmb() {
    fence(Ordering::Acquire);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dmb ish", options(nostack, preserves_flags));
    }
}
