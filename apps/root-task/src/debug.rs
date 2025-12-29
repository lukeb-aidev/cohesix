// Author: Lukas Bower
// Purpose: Debug helpers for seL4 capability inspection and diagnostic write watching.
#![allow(dead_code)]
#![allow(unsafe_code)]

use core::fmt::Write;
use core::mem;
use core::ops::Range;
use core::panic::Location;
use core::slice;
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

#[derive(Clone)]
struct WatchRange {
    label: &'static str,
    range: Range<usize>,
    last_context: Option<&'static str>,
    last_dst: usize,
    last_dst_len: usize,
    last_src_preview: [u8; SRC_PREVIEW_BYTES],
    last_src_len: usize,
    last_src_ptr: usize,
    last_location_file: Option<&'static str>,
    last_location_line: Option<u32>,
    reported: bool,
}

impl WatchRange {
    const fn new(label: &'static str, range: Range<usize>) -> Self {
        Self {
            label,
            range,
            last_context: None,
            last_dst: 0,
            last_dst_len: 0,
            last_src_preview: [0u8; SRC_PREVIEW_BYTES],
            last_src_len: 0,
            last_src_ptr: 0,
            last_location_file: None,
            last_location_line: None,
            reported: false,
        }
    }
}

static WATCHED: SpinMutex<HeaplessVec<WatchRange, MAX_WATCHED_RANGES>> =
    SpinMutex::new(HeaplessVec::new());
static TRIP_ON_OVERLAP: AtomicBool = AtomicBool::new(true);
static FIRST_OVERLAP_REPORTED: AtomicBool = AtomicBool::new(false);

fn ranges_overlap(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// Register a diagnostic watch over the supplied range.
pub fn watch_range(label: &'static str, ptr: *const u8, len: usize) {
    if !WATCH_ENABLED || len == 0 {
        return;
    }
    let mut guards = WATCHED.lock();
    let range = ptr as usize..ptr as usize + len;
    if let Some(existing) = guards.iter_mut().find(|guard| guard.label == label) {
        existing.range = range;
        existing.reported = false;
        existing.last_context = None;
        existing.last_dst = 0;
        existing.last_dst_len = 0;
        existing.last_src_preview = [0u8; SRC_PREVIEW_BYTES];
        existing.last_src_len = 0;
        existing.last_src_ptr = 0;
        existing.last_location_file = None;
        existing.last_location_line = None;
        return;
    }
    if guards.is_full() {
        return;
    }
    let _ = guards.push(WatchRange::new(label, range));
}

/// Clears any registered watch ranges (intended for tests).
#[cfg(test)]
pub fn clear_watches() {
    let mut guards = WATCHED.lock();
    guards.clear();
}

fn write_log_line(
    label: &'static str,
    context: &'static str,
    location: &'static Location<'static>,
    dst_ptr: usize,
    dst_len: usize,
    src_ptr: usize,
    src_len: usize,
    overlap: &Range<usize>,
    src_prefix: &[u8],
) {
    let mut line = HeaplessString::<MAX_WATCH_LOG>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[write-watch] label={label} context={context} location={file}:{line} dst=[0x{dst:016x}..0x{end:016x}) src=[0x{src:016x} len=0x{src_len:08x}] overlap=[0x{ow_start:016x}..0x{ow_end:016x}) src=",
            dst = dst_ptr,
            end = dst_ptr.saturating_add(dst_len),
            src = src_ptr,
            src_len = src_len,
            file = location.file(),
            line = location.line(),
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
    let guards = WATCHED.lock();
    for guard in guards.iter() {
        if ranges_overlap(&guard.range, &target) {
            if let Some(context) = guard.last_context {
                return Some((guard.label, context));
            }
            return Some((guard.label, "unreported"));
        }
    }
    None
}

fn record_overlap(
    guard: &mut WatchRange,
    context: &'static str,
    dst_ptr: usize,
    dst_len: usize,
    src_ptr: usize,
    src_len: usize,
    src: &[u8],
    location: &'static Location<'static>,
) {
    guard.last_context = Some(context);
    guard.last_dst = dst_ptr;
    guard.last_dst_len = dst_len;
    guard.last_src_len = src_len;
    let preview_len = src.len().min(SRC_PREVIEW_BYTES);
    guard.last_src_preview[..preview_len].copy_from_slice(&src[..preview_len]);
    guard.last_src_ptr = src_ptr;
    guard.last_location_file = Some(location.file());
    guard.last_location_line = Some(location.line());
}

fn capture_preview(
    src_ptr: *const u8,
    src_len: usize,
    preview: Option<&[u8]>,
) -> ([u8; SRC_PREVIEW_BYTES], usize) {
    if let Some(provided) = preview {
        let mut buf = [0u8; SRC_PREVIEW_BYTES];
        let len = provided.len().min(SRC_PREVIEW_BYTES);
        if len > 0 {
            buf[..len].copy_from_slice(&provided[..len]);
        }
        return (buf, len);
    }

    let preview_len = src_len.min(SRC_PREVIEW_BYTES);
    let mut buf = [0u8; SRC_PREVIEW_BYTES];
    if preview_len > 0 {
        let src = unsafe { slice::from_raw_parts(src_ptr, preview_len) };
        buf[..preview_len].copy_from_slice(src);
    }
    (buf, preview_len)
}

fn log_trip_and_maybe_panic(
    guard: &mut WatchRange,
    context: &'static str,
    location: &'static Location<'static>,
) {
    let preview_len = guard.last_src_len.min(SRC_PREVIEW_BYTES);
    let preview = &guard.last_src_preview[..preview_len];
    write_log_line(
        guard.label,
        context,
        location,
        guard.last_dst,
        guard.last_dst_len,
        guard.last_src_ptr,
        guard.last_src_len,
        &guard.range,
        preview,
    );
    if TRIP_ON_OVERLAP.load(Ordering::Acquire)
        && FIRST_OVERLAP_REPORTED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        panic!(
            "write-watch overlap: {} (first corruption site)",
            guard.label
        );
    }
}

fn maybe_report_write_internal(
    dst_ptr: *mut u8,
    dst_len: usize,
    src_ptr: *const u8,
    src_len: usize,
    preview: Option<&[u8]>,
    context: &'static str,
    location: &'static Location<'static>,
) -> bool {
    if !WATCH_ENABLED || dst_len == 0 {
        return false;
    }
    let dst_range = dst_ptr as usize..dst_ptr as usize + dst_len;
    let (preview_buf, preview_len) = capture_preview(src_ptr, src_len, preview);
    let mut guards = WATCHED.lock();
    for guard in guards.iter_mut() {
        if ranges_overlap(&guard.range, &dst_range) {
            record_overlap(
                guard,
                context,
                dst_ptr as usize,
                dst_len,
                src_ptr as usize,
                src_len,
                &preview_buf[..preview_len],
                location,
            );
            if !guard.reported {
                guard.reported = true;
                log_trip_and_maybe_panic(guard, context, location);
            } else if TRIP_ON_OVERLAP.load(Ordering::Acquire) {
                log_trip_and_maybe_panic(guard, context, location);
            }
            return true;
        }
    }
    false
}

/// Reports an overlapping write to any watched range.
#[track_caller]
pub fn maybe_report_str_write(
    dst_ptr: *mut u8,
    dst_len: usize,
    src_ptr: *const u8,
    src_len: usize,
    context: &'static str,
) -> bool {
    let location = Location::caller();
    maybe_report_write_internal(dst_ptr, dst_len, src_ptr, src_len, None, context, location)
}

/// Reports an overlapping write using a precomputed preview buffer.
#[track_caller]
pub fn maybe_report_write_with_preview(
    dst_ptr: *mut u8,
    dst_len: usize,
    src_ptr: *const u8,
    src_len: usize,
    preview: &[u8],
    context: &'static str,
) -> bool {
    let location = Location::caller();
    maybe_report_write_internal(
        dst_ptr,
        dst_len,
        src_ptr,
        src_len,
        Some(preview),
        context,
        location,
    )
}

/// Controls whether write-watch overlaps trigger an immediate panic.
pub fn trip_on_overlap(enabled: bool) {
    TRIP_ON_OVERLAP.store(enabled, Ordering::Release);
}

/// Wrapper around `ptr::copy_nonoverlapping` that reports overlaps against watched ranges.
#[track_caller]
pub unsafe fn watched_copy_nonoverlapping<T>(
    src: *const T,
    dst: *mut T,
    count: usize,
    context: &'static str,
) {
    let len_bytes = mem::size_of::<T>().checked_mul(count).unwrap_or(usize::MAX);
    let _ = maybe_report_str_write(
        dst.cast::<u8>(),
        len_bytes,
        src.cast::<u8>(),
        len_bytes,
        context,
    );
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, count);
    }
}

/// Wrapper around `ptr::copy` that reports overlaps against watched ranges.
#[track_caller]
pub unsafe fn watched_copy<T>(src: *const T, dst: *mut T, count: usize, context: &'static str) {
    let len_bytes = mem::size_of::<T>().checked_mul(count).unwrap_or(usize::MAX);
    let _ = maybe_report_str_write(
        dst.cast::<u8>(),
        len_bytes,
        src.cast::<u8>(),
        len_bytes,
        context,
    );
    unsafe {
        core::ptr::copy(src, dst, count);
    }
}

/// Wrapper around `ptr::write_bytes` that reports overlaps against watched ranges.
#[track_caller]
pub unsafe fn watched_write_bytes<T>(dst: *mut T, value: u8, count: usize, context: &'static str) {
    let len_bytes = mem::size_of::<T>().checked_mul(count).unwrap_or(usize::MAX);
    let pattern = [value; SRC_PREVIEW_BYTES];
    let preview_len = len_bytes.min(SRC_PREVIEW_BYTES);
    let _ = maybe_report_write_with_preview(
        dst.cast::<u8>(),
        len_bytes,
        pattern.as_ptr(),
        len_bytes,
        &pattern[..preview_len],
        context,
    );
    unsafe {
        core::ptr::write_bytes(dst, value, count);
    }
}
