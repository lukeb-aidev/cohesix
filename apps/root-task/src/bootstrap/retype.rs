// Author: Lukas Bower

use core::fmt::Write;

use heapless::String;
use sel4_sys as sys;

use super::cspace::{slot_in_empty_window, CSpaceCtx, DestCNode};
use super::cspace_sys;
use crate::bootstrap::log::force_uart_line;
use crate::bootstrap::{boot_tracer, BootPhase, UntypedSelection};
use crate::sel4::{error_name, PAGE_BITS, PAGE_TABLE_BITS};

const DEFAULT_RETYPE_LIMIT: u32 = 512;
const PROGRESS_INTERVAL: u32 = 64;

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
    let log_node_depth = match ctx.dest {
        DestCNode::Init => cspace_sys::encode_cnode_depth(cspace_sys::INIT_CNODE_RETYPE_DEPTH_BITS),
        DestCNode::Other { bits, .. } => cspace_sys::encode_cnode_depth(bits),
    };

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
