// Author: Lukas Bower
// Purpose: Kernel-mediated cache maintenance helpers for DMA buffers.
//! Kernel-mediated cache maintenance helpers for DMA buffers.

#![cfg(all(feature = "kernel", target_os = "none"))]
#![allow(unsafe_code)]

use core::convert::TryFrom;

use log::{debug, trace};
use sel4_sys::{
    invocation_label_nInvocationLabels, seL4_CPtr, seL4_CallWithMRs, seL4_Error,
    seL4_MessageInfo_get_label, seL4_MessageInfo_new, seL4_NoError, seL4_RangeError, seL4_SetMR,
    seL4_Word,
};

const CACHE_LINE_BYTES: usize = 64;
const ARMVSPACE_CLEAN_LABEL: seL4_Word = invocation_label_nInvocationLabels as seL4_Word;
const ARMVSPACE_INVALIDATE_LABEL: seL4_Word = ARMVSPACE_CLEAN_LABEL + 1;
const ARMVSPACE_CLEAN_INVALIDATE_LABEL: seL4_Word = ARMVSPACE_CLEAN_LABEL + 2;

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
    label: seL4_Word,
) -> Result<(), seL4_Error> {
    if len == 0 {
        return Ok(());
    }
    let (aligned_start, aligned_end) = range_for_cache(vaddr, len)?;
    let aligned_len = aligned_end.saturating_sub(aligned_start);
    let start_word = seL4_Word::try_from(aligned_start).map_err(|_| seL4_RangeError)?;
    let end_word = seL4_Word::try_from(aligned_end).map_err(|_| seL4_RangeError)?;

    debug!(
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

    let err = unsafe { call_arm_vspace_op(label, vspace, start_word, end_word) };
    trace!(
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
    call_cache_op("clean", vspace, vaddr, len, ARMVSPACE_CLEAN_LABEL)
}

pub fn cache_invalidate(vspace: seL4_CPtr, vaddr: usize, len: usize) -> Result<(), seL4_Error> {
    call_cache_op("invalidate", vspace, vaddr, len, ARMVSPACE_INVALIDATE_LABEL)
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
        ARMVSPACE_CLEAN_INVALIDATE_LABEL,
    )
}

unsafe fn call_arm_vspace_op(
    label: seL4_Word,
    vspace: seL4_CPtr,
    start: seL4_Word,
    end: seL4_Word,
) -> seL4_Error {
    let mut mr0 = start;
    let mut mr1 = end;
    let mut mr2 = 0;
    let mut mr3 = 0;

    let tag = seL4_MessageInfo_new(label, 0, 0, 2);
    let out_tag = unsafe { seL4_CallWithMRs(vspace, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3) };
    let result_word = seL4_MessageInfo_get_label(out_tag);

    if result_word != seL4_NoError as seL4_Word {
        unsafe {
            seL4_SetMR(0, mr0);
            seL4_SetMR(1, mr1);
            seL4_SetMR(2, mr2);
            seL4_SetMR(3, mr3);
        }
    }

    result_word as seL4_Error
}
