// Author: Lukas Bower

use core::convert::TryFrom;
use core::fmt::Write;

use heapless::String;
use sel4_sys::{self as sys, seL4_CapTableObject, seL4_EndpointObject};

#[cfg(feature = "canonical_cspace")]
use super::cspace::first_endpoint_retype;
use super::cspace::{slot_in_empty_window, CSpaceCtx, DestCNode};
use super::ffi::raw_untyped_retype;
use crate::bootstrap::log::force_uart_line;
use crate::bootstrap::{boot_tracer, BootPhase, UntypedSelection};
#[cfg(feature = "canonical_cspace")]
use crate::sel4::pick_smallest_non_device_untyped;
use crate::sel4::{error_name, PAGE_BITS, PAGE_TABLE_BITS};
#[cfg(feature = "canonical_cspace")]
use crate::sel4_view;
#[cfg(any(test, feature = "ffi_shim"))]
use spin::Mutex;

const DEFAULT_RETYPE_LIMIT: u32 = 512;
const PROGRESS_INTERVAL: u32 = 64;

fn log_retype_call(
    ut_cap: sys::seL4_Word,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dest: &DestCNode,
    node_index: sys::seL4_Word,
    node_depth: sys::seL4_Word,
    node_offset: sys::seL4_Word,
    num_objects: sys::seL4_Word,
) {
    let word_bits = sys::seL4_WordBits as usize;
    let hex_width = (word_bits + 3) / 4;
    let mut line = String::<128>::new();
    let _ = write!(
        &mut line,
        "[retype:call] ut={:#x} obj={:#x} sz_bits={} root={:#x} idx=0x{idx:0width$x} depth={depth} off=0x{off:0width$x} n={n} window=[0x{start:04x}..0x{end:04x})",
        ut_cap,
        obj_type,
        size_bits,
        dest.root,
        idx = node_index,
        depth = node_depth,
        off = node_offset,
        start = dest.empty_start,
        end = dest.empty_end,
        n = num_objects,
        width = hex_width,
    );
    force_uart_line(line.as_str());
}

#[cfg(any(test, feature = "ffi_shim"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetypeCallRecord {
    pub ut: sys::seL4_Word,
    pub obj: sys::seL4_Word,
    pub size_bits: sys::seL4_Word,
    pub root: sys::seL4_CPtr,
    pub idx: sys::seL4_CPtr,
    pub depth: u8,
    pub off: sys::seL4_Word,
    pub n: sys::seL4_Word,
}

#[cfg(any(test, feature = "ffi_shim"))]
static LAST_RETYPE: spin::Mutex<Option<RetypeCallRecord>> = spin::Mutex::new(None);

#[cfg(any(test, feature = "ffi_shim"))]
fn record_retype_call(record: RetypeCallRecord) {
    *LAST_RETYPE.lock() = Some(record);
}

#[cfg(any(test, feature = "ffi_shim"))]
pub fn last_retype_args() -> RetypeCallRecord {
    LAST_RETYPE
        .lock()
        .copied()
        .expect("no retype calls recorded")
}

#[cfg(any(test, feature = "ffi_shim"))]
fn seL4_Untyped_Retype(
    ut_cap: sys::seL4_Word,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dest_root: sys::seL4_CPtr,
    node_index: sys::seL4_CPtr,
    node_depth: sys::seL4_Word,
    node_offset: sys::seL4_Word,
    num_objects: sys::seL4_Word,
) -> sys::seL4_Error {
    record_retype_call(RetypeCallRecord {
        ut: ut_cap,
        obj: obj_type,
        size_bits,
        root: dest_root,
        idx: node_index,
        depth: node_depth as u8,
        off: node_offset,
        n: num_objects,
    });
    sys::seL4_NoError
}

#[cfg(not(any(test, feature = "ffi_shim")))]
fn seL4_Untyped_Retype(
    ut_cap: sys::seL4_Word,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dest_root: sys::seL4_CPtr,
    node_index: sys::seL4_CPtr,
    node_depth: sys::seL4_Word,
    node_offset: sys::seL4_Word,
    num_objects: sys::seL4_Word,
) -> sys::seL4_Error {
    raw_untyped_retype(
        ut_cap,
        obj_type,
        size_bits,
        dest_root,
        node_index,
        node_depth,
        node_offset,
        num_objects,
    )
}

#[inline(always)]
pub(crate) fn call_retype(
    ut_cap: sys::seL4_Word,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dest: &DestCNode,
    num_objects: sys::seL4_Word,
) -> sys::seL4_Error {
    dest.assert_sane();
    let node_index: sys::seL4_Word = 0;
    let node_depth: sys::seL4_Word = 0;
    let slot_offset =
        sys::seL4_Word::try_from(dest.slot_offset).expect("slot offset must fit within seL4_Word");
    log_retype_call(
        ut_cap,
        obj_type,
        size_bits,
        dest,
        node_index,
        node_depth,
        slot_offset,
        num_objects,
    );
    let err = seL4_Untyped_Retype(
        ut_cap,
        obj_type,
        size_bits,
        dest.root,
        node_index,
        node_depth,
        slot_offset,
        num_objects,
    );
    let mut line = String::<64>::new();
    if write!(&mut line, "[retype:ret] err={}", err).is_err() {
        // Preserve the best-effort diagnostic even if truncated.
    }
    force_uart_line(line.as_str());
    err
}

/// Convenience wrapper for minting an endpoint object via [`call_retype`].
pub fn retype_endpoint(ut: sys::seL4_Word, dest: &DestCNode) -> sys::seL4_Error {
    call_retype(ut, seL4_EndpointObject as sys::seL4_Word, 0, dest, 1)
}

/// Convenience wrapper for minting a small CNode via [`call_retype`].
pub fn retype_captable(ut: sys::seL4_Word, bits: u8, dest: &DestCNode) -> sys::seL4_Error {
    call_retype(
        ut,
        seL4_CapTableObject as sys::seL4_Word,
        bits as sys::seL4_Word,
        dest,
        1,
    )
}

/// Bumps the destination slot forward by one, panicking on overflow.
pub fn bump_slot(dest: &mut DestCNode) {
    dest.bump_slot();
}

fn boot_retype_limit() -> u32 {
    option_env!("BOOT_RETYPE_MAX")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|limit| *limit > 0)
        .unwrap_or(DEFAULT_RETYPE_LIMIT)
}

fn object_name(obj_type: sys::seL4_ObjectType) -> &'static str {
    match obj_type {
        sys::seL4_ObjectType::seL4_ARM_PageTableObject => "PageTable",
        sys::seL4_ObjectType::seL4_ARM_Page => "Page",
        sys::seL4_ObjectType::seL4_NotificationObject => "Notification",
        _ => "Object",
    }
}

fn log_slot_out_of_range(slot: sys::seL4_CPtr, start: sys::seL4_CPtr, end: sys::seL4_CPtr) {
    let mut line = String::<128>::new();
    let _ = write!(
        line,
        "[retype:err] dest=0x{slot:04x} outside empty window [0x{start:04x}..0x{end:04x})",
    );
    force_uart_line(line.as_str());
}

fn log_retype_error(
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    slot: sys::seL4_CPtr,
    depth: sys::seL4_Word,
    err: sys::seL4_Error,
) {
    let errno = error_name(err);
    let mut line = String::<160>::new();
    let _ = write!(
        line,
        "[retype:err] ut=0x{untyped:03x} obj={kind} dest=0x{slot:04x} depth={depth} errno={errno}",
        kind = object_name(obj_type),
    );
    force_uart_line(line.as_str());
}

fn log_slot_alloc_failure(
    candidate: sys::seL4_CPtr,
    start: sys::seL4_CPtr,
    end: sys::seL4_CPtr,
    err: sys::seL4_Error,
) {
    let mut line = String::<144>::new();
    let _ = write!(
        line,
        "[retype:err] slot alloc failed err={err:?} candidate=0x{candidate:04x} empty=[0x{start:04x}..0x{end:04x})",
    );
    force_uart_line(line.as_str());
}

/// Retypes a single kernel object from an untyped capability into the init CSpace.
pub fn retype_one(
    ctx: &mut CSpaceCtx,
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_bits: u8,
) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
    let slot = ctx.alloc_slot_checked()?;
    let (start, end) = ctx.empty_bounds();
    if !slot_in_empty_window(slot, start, end) {
        log_slot_out_of_range(slot, start, end);
        return Err(sys::seL4_RangeError);
    }
    boot_tracer().record_slot(slot as u32);

    let err = ctx.retype_to_slot(
        untyped,
        obj_type as sys::seL4_Word,
        obj_bits as sys::seL4_Word,
        slot,
    );
    if err != sys::seL4_NoError {
        return Err(err);
    }

    if ctx.root_cnode_copy_slot == sys::seL4_CapNull {
        ctx.mint_root_cnode_copy()?;
    }

    Ok(slot)
}

/// Retypes a batch of objects according to the supplied untyped selection plan.
pub fn retype_selection<F>(
    ctx: &mut CSpaceCtx,
    selection: &UntypedSelection,
    mut watchdog: F,
) -> Result<u32, sys::seL4_Error>
where
    F: FnMut(),
{
    let tracer = boot_tracer();
    let limit = boot_retype_limit();
    let mut remaining_total = selection.plan.total.min(limit);
    if remaining_total == 0 {
        return Ok(0);
    }

    tracer.advance(BootPhase::RetypeBegin);

    let tables = selection.plan.page_tables.min(remaining_total);
    remaining_total = remaining_total.saturating_sub(tables);
    let pages = selection.plan.small_pages.min(remaining_total);
    let total_target = tables + pages;
    if total_target == 0 {
        tracer.advance(BootPhase::RetypeDone);
        return Ok(0);
    }

    let (start, end) = ctx.empty_bounds();
    let log_node_depth = sys::seL4_Word::from(ctx.cnode_bits());

    let categories: [(u32, sys::seL4_ObjectType, u8); 2] = [
        (
            tables,
            sys::seL4_ObjectType::seL4_ARM_PageTableObject,
            PAGE_TABLE_BITS as u8,
        ),
        (pages, sys::seL4_ObjectType::seL4_ARM_Page, PAGE_BITS as u8),
    ];

    let mut done = 0u32;
    for (count, obj_type, obj_bits) in categories {
        for _ in 0..count {
            watchdog();
            let slot = match ctx.alloc_slot_checked() {
                Ok(slot) => slot,
                Err(err) => {
                    let candidate = ctx.next_candidate_slot();
                    log_slot_alloc_failure(candidate, start, end, err);
                    tracer.advance(BootPhase::RetypeDone);
                    return Err(err);
                }
            };
            if !slot_in_empty_window(slot, start, end) {
                log_slot_out_of_range(slot, start, end);
                tracer.advance(BootPhase::RetypeDone);
                return Ok(done);
            }
            tracer.record_slot(slot as u32);
            let result = ctx.retype_to_slot(
                selection.cap,
                obj_type as sys::seL4_Word,
                obj_bits as sys::seL4_Word,
                slot,
            );
            if result != sys::seL4_NoError {
                log_retype_error(selection.cap, obj_type, slot, log_node_depth, result);
                tracer.advance(BootPhase::RetypeDone);
                return Err(result);
            }
            done += 1;
            if done % PROGRESS_INTERVAL == 0 || done == total_target {
                tracer.advance(BootPhase::RetypeProgress {
                    done,
                    total: total_target,
                });
            }
        }
    }

    tracer.advance(BootPhase::RetypeDone);
    Ok(done)
}

#[cfg(feature = "canonical_cspace")]
pub fn canonical_cspace_console(bi: &sel4_sys::seL4_BootInfo) -> ! {
    let (start, _end) = sel4_view::empty_window(bi);
    let dst = u32::try_from(start).expect("bootinfo empty window start must fit within u32");

    crate::bootstrap::cspace::cnode_copy_selftest(bi).expect("CNode copy selftest failed");

    let ut = pick_smallest_non_device_untyped(bi);
    first_endpoint_retype(bi, ut, dst).expect("endpoint retype failed");

    log::info!("[retype:ok] endpoint @ slot=0x{:04x}", dst);

    crate::console::start(dst, bi);
    loop {
        core::hint::spin_loop();
    }
}
