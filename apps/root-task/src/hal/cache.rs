// Author: Lukas Bower
//! Kernel-mediated cache maintenance helpers for DMA buffers.

#![cfg(all(feature = "kernel", target_os = "none"))]
#![allow(unsafe_code)]

use core::convert::TryFrom;

use log::info;
use sel4_sys::{
    seL4_ARM_VSpace_CleanInvalidate_Data, seL4_ARM_VSpace_Clean_Data,
    seL4_ARM_VSpace_Invalidate_Data, seL4_CPtr, seL4_Error, seL4_RangeError, seL4_Word,
};

const CACHE_LINE_BYTES: usize = 64;

fn align_down(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value & !(align - 1)
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value.saturating_add(align - 1) & !(align - 1)
}

fn range_for_cache(vaddr: usize, len: usize) -> Result<(usize, usize), seL4_Error> {
    if len == 0 {
        return Ok((vaddr, vaddr));
    }
    let end = vaddr.checked_add(len).ok_or(seL4_RangeError)?;
    let aligned_start = align_down(vaddr, CACHE_LINE_BYTES);
    let aligned_end = align_up(end, CACHE_LINE_BYTES);
    Ok((aligned_start, aligned_end))
}

fn call_cache_op(
    op: &str,
    vspace: seL4_CPtr,
    vaddr: usize,
    len: usize,
    f: unsafe fn(seL4_CPtr, seL4_Word, seL4_Word) -> seL4_Error,
) -> Result<(), seL4_Error> {
    if len == 0 {
        return Ok(());
    }
    let (aligned_start, aligned_end) = range_for_cache(vaddr, len)?;
    let aligned_len = aligned_end.saturating_sub(aligned_start);
    let start_word = seL4_Word::try_from(aligned_start).map_err(|_| seL4_RangeError)?;
    let end_word = seL4_Word::try_from(aligned_end).map_err(|_| seL4_RangeError)?;

    info!(
        target: "hal-cache",
        "[cache] CACHE_OP enter op={op} vspace=0x{vspace:04x} vaddr=0x{vaddr:016x}..0x{vend:016x} aligned=0x{astart:016x}..0x{aend:016x} len={len} aligned_len={aligned_len}",
        op = op,
        vspace = vspace,
        vaddr = vaddr,
        vend = vaddr.saturating_add(len),
        astart = aligned_start,
        aend = aligned_end,
        len = len,
        aligned_len = aligned_len,
    );

    let err = unsafe { f(vspace, start_word, end_word) };
    info!(
        target: "hal-cache",
        "[cache] CACHE_OP exit op={} err={}",
        op,
        err,
    );
    if err == 0 {
        Ok(())
    } else {
        Err(err)
    }
}

pub fn cache_clean(vspace: seL4_CPtr, vaddr: usize, len: usize) -> Result<(), seL4_Error> {
    call_cache_op("clean", vspace, vaddr, len, seL4_ARM_VSpace_Clean_Data)
}

pub fn cache_invalidate(vspace: seL4_CPtr, vaddr: usize, len: usize) -> Result<(), seL4_Error> {
    call_cache_op(
        "invalidate",
        vspace,
        vaddr,
        len,
        seL4_ARM_VSpace_Invalidate_Data,
    )
}

pub fn cache_clean_invalidate(
    vspace: seL4_CPtr,
    vaddr: usize,
    len: usize,
) -> Result<(), seL4_Error> {
    call_cache_op(
        "clean+invalidate",
        vspace,
        vaddr,
        len,
        seL4_ARM_VSpace_CleanInvalidate_Data,
    )
}
