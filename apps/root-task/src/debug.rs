// Author: Lukas Bower
// Purpose: Debug helpers for seL4 capability inspection and diagnostic write watching.
#![allow(dead_code)]
#![allow(unsafe_code)]

use core::fmt::Write;
use core::ops::Range;
use core::sync::atomic::{AtomicBool, Ordering};

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use sel4_sys::{seL4_CPtr, seL4_Word};
use spin::Mutex as SpinMutex;

extern "C" {
    fn seL4_DebugCapIdentify(cap: seL4_CPtr) -> seL4_Word;
}

#[inline(always)]
pub fn debug_identify(cap: seL4_CPtr) -> seL4_Word {
    unsafe { seL4_DebugCapIdentify(cap) }
}

const MAX_WATCHED_RANGES: usize = 8;
const MAX_WATCH_LOG: usize = 256;
const SRC_PREVIEW_BYTES: usize = 32;
const WATCH_ENABLED: bool = cfg!(any(
    debug_assertions,
    feature = "net-console",
    feature = "net-diag",
    test
));

#[derive(Clone, Copy)]
struct WatchRange {
    label: &'static str,
    range: Range<usize>,
    last_context: Option<&'static str>,
    last_dst: usize,
    last_src_preview: [u8; SRC_PREVIEW_BYTES],
    last_src_len: usize,
    reported: bool,
}

impl WatchRange {
    const fn new(label: &'static str, range: Range<usize>) -> Self {
        Self {
            label,
            range,
            last_context: None,
            last_dst: 0,
            last_src_preview: [0u8; SRC_PREVIEW_BYTES],
            last_src_len: 0,
            reported: false,
        }
    }
}

static WATCHED: SpinMutex<HeaplessVec<WatchRange, MAX_WATCHED_RANGES>> =
    SpinMutex::new(HeaplessVec::new());

fn ranges_overlap(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// Register a diagnostic watch over the supplied range.
pub fn watch_range(label: &'static str, ptr: *const u8, len: usize) {
    if !WATCH_ENABLED || len == 0 {
        return;
    }
    if let Ok(mut guards) = WATCHED.lock() {
        let range = ptr as usize..ptr as usize + len;
        if let Some(existing) = guards.iter_mut().find(|guard| guard.label == label) {
            existing.range = range;
            existing.reported = false;
            existing.last_context = None;
            existing.last_dst = 0;
            existing.last_src_preview = [0u8; SRC_PREVIEW_BYTES];
            existing.last_src_len = 0;
            return;
        }
        if guards.is_full() {
            return;
        }
        let _ = guards.push(WatchRange::new(label, range));
    }
}

/// Clears any registered watch ranges (intended for tests).
#[cfg(test)]
pub fn clear_watches() {
    if let Ok(mut guards) = WATCHED.lock() {
        guards.clear();
    }
}

fn write_log_line(
    label: &'static str,
    context: &'static str,
    dst_ptr: usize,
    dst_len: usize,
    overlap: &Range<usize>,
    src_prefix: &[u8],
) {
    let mut line = HeaplessString::<MAX_WATCH_LOG>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[write-watch] label={label} context={context} dst=[0x{dst:016x}..0x{end:016x}) overlap=[0x{ow_start:016x}..0x{ow_end:016x}) src=",
            dst = dst_ptr,
            end = dst_ptr.saturating_add(dst_len),
            ow_start = overlap.start,
            ow_end = overlap.end,
        ),
    );
    for byte in src_prefix {
        if write!(line, "{byte:02x}").is_err() {
            break;
        }
    }
    log::warn!("{}", line.as_str());
}

/// Returns a hint describing the most recent write to the supplied pointer, if any.
pub fn watch_hint_for(ptr: usize, len: usize) -> Option<(&'static str, &'static str)> {
    if !WATCH_ENABLED {
        return None;
    }
    let target = ptr..ptr.saturating_add(len);
    if let Ok(guards) = WATCHED.lock() {
        for guard in guards.iter() {
            if ranges_overlap(&guard.range, &target) {
                if let Some(context) = guard.last_context {
                    return Some((guard.label, context));
                }
                return Some((guard.label, "unreported"));
            }
        }
    }
    None
}

fn record_overlap(
    guard: &mut WatchRange,
    context: &'static str,
    dst_ptr: usize,
    src: &[u8],
) {
    guard.last_context = Some(context);
    guard.last_dst = dst_ptr;
    let preview_len = src.len().min(SRC_PREVIEW_BYTES);
    guard.last_src_preview[..preview_len].copy_from_slice(&src[..preview_len]);
    guard.last_src_len = preview_len;
}

/// Reports an overlapping write to any watched range.
pub fn maybe_report_str_write(
    dst_ptr: *mut u8,
    dst_len: usize,
    src_ptr: *const u8,
    src_len: usize,
    context: &'static str,
) -> bool {
    if !WATCH_ENABLED || dst_len == 0 {
        return false;
    }
    let dst_range = dst_ptr as usize..dst_ptr as usize + dst_len;
    let src = unsafe { core::slice::from_raw_parts(src_ptr, src_len.min(SRC_PREVIEW_BYTES)) };
    if let Ok(mut guards) = WATCHED.lock() {
        for guard in guards.iter_mut() {
            if ranges_overlap(&guard.range, &dst_range) {
                record_overlap(guard, context, dst_ptr as usize, src);
                if !guard.reported {
                    guard.reported = true;
                    write_log_line(
                        guard.label,
                        context,
                        dst_ptr as usize,
                        dst_len,
                        &guard.range,
                        src,
                    );
                }
                return true;
            }
        }
    }
    false
}
