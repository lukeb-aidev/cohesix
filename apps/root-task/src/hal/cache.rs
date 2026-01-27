// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Kernel-mediated cache maintenance helpers for DMA buffers with structured logging.
// Author: Lukas Bower
//! Kernel-mediated cache maintenance helpers for DMA buffers.

#![allow(unsafe_code)]

use core::cmp::min;
use core::convert::TryFrom;
use core::fmt;
use core::fmt::Write;
use core::panic::Location;
use core::sync::atomic::{AtomicU64, Ordering};

use heapless::{Deque, Vec};
use log::{info, trace, warn, Level};
use sel4_sys::{
    seL4_CPtr, seL4_Error, seL4_InvalidArgument, seL4_NoError, seL4_RangeError, seL4_Word,
};
use spin::Mutex;

use crate::hal;

#[cfg(all(feature = "kernel", target_os = "none"))]
use sel4_sys::{
    invocation_label_nInvocationLabels, seL4_CallWithMRs, seL4_MessageInfo_get_label,
    seL4_MessageInfo_new, seL4_SetMR,
};

#[cfg(all(feature = "kernel", target_os = "none"))]
const INVOCATION_LABEL_BASE: seL4_Word = invocation_label_nInvocationLabels as seL4_Word;

#[cfg(not(all(feature = "kernel", target_os = "none")))]
const INVOCATION_LABEL_BASE: seL4_Word = 0;

const CACHE_LINE_BYTES: usize = 64;
const ARMVSPACE_CLEAN_LABEL: seL4_Word = INVOCATION_LABEL_BASE;
const ARMVSPACE_INVALIDATE_LABEL: seL4_Word = ARMVSPACE_CLEAN_LABEL + 1;
const ARMVSPACE_CLEAN_INVALIDATE_LABEL: seL4_Word = ARMVSPACE_CLEAN_LABEL + 2;
const ARMVSPACE_UNIFY_LABEL: seL4_Word = ARMVSPACE_CLEAN_LABEL + 3;

// Logging policy: per-op traces are gated to TRACE (or the `cache-trace` feature).
// INFO emits rate-limited summaries with suppression counts; WARN dumps recent ops on errors.
const CACHE_RING_CAPACITY: usize = 2048;
const CACHE_DUMP_CHUNK: usize = 64;
const ERROR_DUMP_RECENT: usize = 64;
const SUMMARY_INTERVAL_MS: u64 = 1_000;
const SUMMARY_OP_THRESHOLD: u64 = 1_024;

#[cfg(feature = "cache-trace")]
const FORCE_CACHE_TRACE: bool = true;
#[cfg(not(feature = "cache-trace"))]
const FORCE_CACHE_TRACE: bool = false;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum CacheOpKind {
    Clean,
    Invalidate,
    CleanInvalidate,
    UnifyInstruction,
}

impl CacheOpKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Invalidate => "invalidate",
            Self::CleanInvalidate => "clean+invalidate",
            Self::UnifyInstruction => "unify-instruction",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CacheErrorKind {
    /// The supplied range overflowed or was otherwise out-of-bounds.
    Range,
    /// The supplied arguments were not valid for the kernel operation.
    InvalidArgument,
    /// The kernel returned a non-zero error code not classified above.
    Kernel,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CacheError {
    code: seL4_Error,
    kind: CacheErrorKind,
}

impl CacheError {
    #[must_use]
    pub const fn new(code: seL4_Error) -> Self {
        let kind = match code {
            seL4_RangeError => CacheErrorKind::Range,
            seL4_InvalidArgument => CacheErrorKind::InvalidArgument,
            _ => CacheErrorKind::Kernel,
        };
        Self { code, kind }
    }

    #[must_use]
    pub const fn code(self) -> seL4_Error {
        self.code
    }

    #[must_use]
    pub const fn kind(self) -> CacheErrorKind {
        self.kind
    }
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cache op error={} kind={:?}", self.code, self.kind)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct ShapeKey {
    op: CacheOpKind,
    vspace: seL4_CPtr,
    len_bucket: usize,
    caller_file: &'static str,
    caller_line: u32,
}

impl ShapeKey {
    fn new(
        op: CacheOpKind,
        vspace: seL4_CPtr,
        aligned_len: usize,
        caller: &'static Location<'static>,
    ) -> Self {
        Self {
            op,
            vspace,
            len_bucket: bucket_len(aligned_len),
            caller_file: caller.file(),
            caller_line: caller.line(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct CacheOpRecord {
    seq: u64,
    timestamp_ms: u64,
    op: CacheOpKind,
    vspace: seL4_CPtr,
    vaddr: usize,
    len: usize,
    aligned_start: usize,
    aligned_len: usize,
    err: seL4_Error,
    caller_file: &'static str,
    caller_line: u32,
}

#[derive(Debug)]
struct SummarySnapshot {
    clean: u64,
    invalidate: u64,
    clean_invalidate: u64,
    unify_instruction: u64,
    requested_bytes: u64,
    aligned_bytes: u64,
    max_aligned_len: usize,
    errors: u64,
    suppressed: u64,
    window_ms: u64,
}

#[derive(Debug)]
struct SummaryCounters {
    clean: u64,
    invalidate: u64,
    clean_invalidate: u64,
    unify_instruction: u64,
    requested_bytes: u64,
    aligned_bytes: u64,
    max_aligned_len: usize,
    errors: u64,
    suppressed: u64,
    window_start_ms: u64,
    last_emit_ms: u64,
    last_emit_seq: u64,
}

impl SummaryCounters {
    const fn new() -> Self {
        Self {
            clean: 0,
            invalidate: 0,
            clean_invalidate: 0,
            unify_instruction: 0,
            requested_bytes: 0,
            aligned_bytes: 0,
            max_aligned_len: 0,
            errors: 0,
            suppressed: 0,
            window_start_ms: 0,
            last_emit_ms: 0,
            last_emit_seq: 0,
        }
    }

    fn record(
        &mut self,
        op: CacheOpKind,
        len: usize,
        aligned_len: usize,
        err: seL4_Error,
        suppressed: bool,
        seq: u64,
        now_ms: u64,
    ) -> Option<SummarySnapshot> {
        if self.window_start_ms == 0 {
            self.window_start_ms = now_ms;
            self.last_emit_ms = now_ms;
            self.last_emit_seq = seq;
        }

        match op {
            CacheOpKind::Clean => self.clean = self.clean.saturating_add(1),
            CacheOpKind::Invalidate => self.invalidate = self.invalidate.saturating_add(1),
            CacheOpKind::CleanInvalidate => {
                self.clean_invalidate = self.clean_invalidate.saturating_add(1)
            }
            CacheOpKind::UnifyInstruction => {
                self.unify_instruction = self.unify_instruction.saturating_add(1)
            }
        }
        self.requested_bytes = self.requested_bytes.saturating_add(len as u64);
        self.aligned_bytes = self.aligned_bytes.saturating_add(aligned_len as u64);
        if aligned_len > self.max_aligned_len {
            self.max_aligned_len = aligned_len;
        }
        if err != 0 {
            self.errors = self.errors.saturating_add(1);
        }
        if suppressed {
            self.suppressed = self.suppressed.saturating_add(1);
        }

        let should_emit_time = now_ms.saturating_sub(self.last_emit_ms) >= SUMMARY_INTERVAL_MS;
        let should_emit_ops = seq.saturating_sub(self.last_emit_seq) >= SUMMARY_OP_THRESHOLD;

        if should_emit_time || should_emit_ops {
            let snapshot = SummarySnapshot {
                clean: self.clean,
                invalidate: self.invalidate,
                clean_invalidate: self.clean_invalidate,
                unify_instruction: self.unify_instruction,
                requested_bytes: self.requested_bytes,
                aligned_bytes: self.aligned_bytes,
                max_aligned_len: self.max_aligned_len,
                errors: self.errors,
                suppressed: self.suppressed,
                window_ms: now_ms.saturating_sub(self.window_start_ms),
            };
            self.clean = 0;
            self.invalidate = 0;
            self.clean_invalidate = 0;
            self.unify_instruction = 0;
            self.requested_bytes = 0;
            self.aligned_bytes = 0;
            self.max_aligned_len = 0;
            self.errors = 0;
            self.suppressed = 0;
            self.window_start_ms = now_ms;
            self.last_emit_ms = now_ms;
            self.last_emit_seq = seq;
            Some(snapshot)
        } else {
            None
        }
    }
}

struct CacheLogState {
    summary: SummaryCounters,
    ring: Deque<CacheOpRecord, CACHE_RING_CAPACITY>,
    last_shape: Option<ShapeKey>,
}

impl CacheLogState {
    const fn new() -> Self {
        Self {
            summary: SummaryCounters::new(),
            ring: Deque::new(),
            last_shape: None,
        }
    }

    fn record(
        &mut self,
        record: CacheOpRecord,
        shape: ShapeKey,
    ) -> (bool, Option<SummarySnapshot>, usize) {
        let suppressed = if let Some(previous) = self.last_shape {
            previous == shape
        } else {
            false
        };

        self.last_shape = Some(shape);

        if self.ring.is_full() {
            let _ = self.ring.pop_front();
        }
        let _ = self.ring.push_back(record);

        let snapshot = self.summary.record(
            record.op,
            record.len,
            record.aligned_len,
            record.err,
            suppressed,
            record.seq,
            record.timestamp_ms,
        );

        (suppressed, snapshot, self.ring.len())
    }
}

static CACHE_LOG: Mutex<CacheLogState> = Mutex::new(CacheLogState::new());
static CACHE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(any(test, feature = "cache-maintenance"))]
static CACHE_TEST_ERROR: Mutex<Option<seL4_Error>> = Mutex::new(None);

/// Cache maintenance wrapper binding operations to a VSpace capability.
#[derive(Copy, Clone, Debug)]
pub struct CacheMaintenance {
    vspace: seL4_CPtr,
}

impl CacheMaintenance {
    /// Construct a cache maintenance helper bound to the supplied VSpace capability.
    #[must_use]
    pub const fn new(vspace: seL4_CPtr) -> Self {
        Self { vspace }
    }

    /// Construct a helper bound to the init thread VSpace.
    #[must_use]
    pub const fn init_thread() -> Self {
        Self::new(sel4_sys::seL4_CapInitThreadVSpace)
    }

    /// Clean the data cache for the supplied range.
    pub fn clean(&self, vaddr: usize, len: usize) -> Result<(), CacheError> {
        call_cache_op(
            CacheOpKind::Clean,
            self.vspace,
            vaddr,
            len,
            ARMVSPACE_CLEAN_LABEL,
        )
    }

    /// Invalidate the data cache for the supplied range.
    pub fn invalidate(&self, vaddr: usize, len: usize) -> Result<(), CacheError> {
        call_cache_op(
            CacheOpKind::Invalidate,
            self.vspace,
            vaddr,
            len,
            ARMVSPACE_INVALIDATE_LABEL,
        )
    }

    /// Clean and invalidate the data cache for the supplied range.
    pub fn clean_invalidate(&self, vaddr: usize, len: usize) -> Result<(), CacheError> {
        call_cache_op(
            CacheOpKind::CleanInvalidate,
            self.vspace,
            vaddr,
            len,
            ARMVSPACE_CLEAN_INVALIDATE_LABEL,
        )
    }

    /// Unify instruction cache lines for the supplied range.
    pub fn unify_instruction(&self, vaddr: usize, len: usize) -> Result<(), CacheError> {
        call_cache_op(
            CacheOpKind::UnifyInstruction,
            self.vspace,
            vaddr,
            len,
            ARMVSPACE_UNIFY_LABEL,
        )
    }
}

/// Inject a deterministic error for the next cache operation (test support).
#[cfg(any(test, feature = "cache-maintenance"))]
pub fn set_test_error(error: Option<seL4_Error>) {
    let mut guard = CACHE_TEST_ERROR.lock();
    *guard = error;
}

fn bucket_len(len: usize) -> usize {
    len.checked_next_power_of_two().unwrap_or(len)
}

fn render_record_line(record: &CacheOpRecord) -> heapless::String<192> {
    let mut line = heapless::String::<192>::new();
    let aligned_end = record.aligned_start.saturating_add(record.aligned_len);
    let _ = write!(
        line,
        "[cache] seq={seq} ts_ms={ts_ms} op={op} vspace=0x{vspace:04x} vaddr=0x{vaddr:016x}..0x{vend:016x} aligned=0x{astart:016x}..0x{aend:016x} len={len} aligned_len={aligned_len} err={err} caller={caller_file}:{caller_line}",
        seq = record.seq,
        ts_ms = record.timestamp_ms,
        op = record.op.as_str(),
        vspace = record.vspace,
        vaddr = record.vaddr,
        vend = record.vaddr.saturating_add(record.len),
        astart = record.aligned_start,
        aend = aligned_end,
        len = record.len,
        aligned_len = record.aligned_len,
        err = record.err,
        caller_file = record.caller_file,
        caller_line = record.caller_line,
    );
    line
}

fn render_summary_line(snapshot: &SummarySnapshot, ring_len: usize) -> heapless::String<256> {
    let mut line = heapless::String::<256>::new();
    let total_ops = snapshot.clean
        + snapshot.invalidate
        + snapshot.clean_invalidate
        + snapshot.unify_instruction;
    let _ = write!(
        line,
        "[cache] summary window_ms={} ops={} clean={} invalidate={} clean_invalidate={} unify_instruction={} requested_bytes={} aligned_bytes={} max_aligned_len={} errors={} suppressed={} ring_size={}",
        snapshot.window_ms,
        total_ops,
        snapshot.clean,
        snapshot.invalidate,
        snapshot.clean_invalidate,
        snapshot.unify_instruction,
        snapshot.requested_bytes,
        snapshot.aligned_bytes,
        snapshot.max_aligned_len,
        snapshot.errors,
        snapshot.suppressed,
        ring_len,
    );
    line
}

fn snapshot_recent(limit: usize, mut visitor: impl FnMut(&[CacheOpRecord])) {
    let mut emitted = 0usize;

    loop {
        if emitted >= limit {
            break;
        }

        let mut chunk: Vec<CacheOpRecord, CACHE_DUMP_CHUNK> = Vec::new();
        {
            let state = CACHE_LOG.lock();
            if emitted >= state.ring.len() {
                break;
            }
            let available = state.ring.len().saturating_sub(emitted);
            let take = min(
                min(CACHE_DUMP_CHUNK, limit.saturating_sub(emitted)),
                available,
            );
            for record in state.ring.iter().rev().skip(emitted).take(take) {
                let _ = chunk.push(*record);
            }
        }

        if chunk.is_empty() {
            break;
        }

        visitor(chunk.as_slice());
        emitted = emitted.saturating_add(chunk.len());

        if chunk.len() < CACHE_DUMP_CHUNK {
            break;
        }
    }
}

fn dump_recent_logs(limit: usize) {
    snapshot_recent(limit, |records| {
        for record in records {
            let line = render_record_line(record);
            warn!(target: "hal-cache", "{line}");
        }
    });
}

fn align_down(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value & !(align - 1)
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value.saturating_add(align - 1) & !(align - 1)
}

fn range_for_cache(vaddr: usize, len: usize) -> Result<(usize, usize), CacheError> {
    if len == 0 {
        return Ok((vaddr, vaddr));
    }
    let end = vaddr
        .checked_add(len)
        .ok_or_else(|| CacheError::new(seL4_RangeError))?;
    let aligned_start = align_down(vaddr, CACHE_LINE_BYTES);
    let aligned_end = align_up(end, CACHE_LINE_BYTES);
    Ok((aligned_start, aligned_end))
}

#[track_caller]
fn call_cache_op(
    op: CacheOpKind,
    vspace: seL4_CPtr,
    vaddr: usize,
    len: usize,
    label: seL4_Word,
) -> Result<(), CacheError> {
    if len == 0 {
        return Ok(());
    }
    let (aligned_start, aligned_end) = range_for_cache(vaddr, len)?;
    let aligned_len = aligned_end.saturating_sub(aligned_start);
    let start_word =
        seL4_Word::try_from(aligned_start).map_err(|_| CacheError::new(seL4_RangeError))?;
    let end_word =
        seL4_Word::try_from(aligned_end).map_err(|_| CacheError::new(seL4_RangeError))?;

    let err = unsafe { call_arm_vspace_op(label, vspace, start_word, end_word) };

    let timestamp_ms = hal::timebase().now_ms();
    let seq = CACHE_SEQUENCE
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    let caller = Location::caller();
    let record = CacheOpRecord {
        seq,
        timestamp_ms,
        op,
        vspace,
        vaddr,
        len,
        aligned_start,
        aligned_len,
        err,
        caller_file: caller.file(),
        caller_line: caller.line(),
    };
    let shape = ShapeKey::new(op, vspace, aligned_len, caller);
    let trace_requested = FORCE_CACHE_TRACE || log::log_enabled!(target: "hal-cache", Level::Trace);

    let (suppressed, summary_snapshot, ring_len) = {
        let mut state = CACHE_LOG.lock();
        state.record(record, shape)
    };

    if trace_requested && !suppressed {
        let line = render_record_line(&record);
        trace!(target: "hal-cache", "{line}");
    }

    if let Some(snapshot) = summary_snapshot {
        let line = render_summary_line(&snapshot, ring_len);
        info!(target: "hal-cache", "{line}");
    }

    if err != 0 {
        let cache_err = CacheError::new(err);
        let line = render_record_line(&record);
        warn!(target: "hal-cache", "{line}");
        dump_recent_logs(ERROR_DUMP_RECENT);
        Err(cache_err)
    } else {
        Ok(())
    }
}

pub fn cache_clean(vspace: seL4_CPtr, vaddr: usize, len: usize) -> Result<(), CacheError> {
    call_cache_op(
        CacheOpKind::Clean,
        vspace,
        vaddr,
        len,
        ARMVSPACE_CLEAN_LABEL,
    )
}

pub fn cache_invalidate(vspace: seL4_CPtr, vaddr: usize, len: usize) -> Result<(), CacheError> {
    call_cache_op(
        CacheOpKind::Invalidate,
        vspace,
        vaddr,
        len,
        ARMVSPACE_INVALIDATE_LABEL,
    )
}

pub fn cache_clean_invalidate(
    vspace: seL4_CPtr,
    vaddr: usize,
    len: usize,
) -> Result<(), CacheError> {
    call_cache_op(
        CacheOpKind::CleanInvalidate,
        vspace,
        vaddr,
        len,
        ARMVSPACE_CLEAN_INVALIDATE_LABEL,
    )
}

pub fn cache_unify_instruction(
    vspace: seL4_CPtr,
    vaddr: usize,
    len: usize,
) -> Result<(), CacheError> {
    call_cache_op(
        CacheOpKind::UnifyInstruction,
        vspace,
        vaddr,
        len,
        ARMVSPACE_UNIFY_LABEL,
    )
}

/// Writes the most recent cache operations to the provided writer.
pub fn write_recent_ops(writer: &mut impl Write, count: usize) {
    snapshot_recent(count, |records| {
        for record in records {
            let line = render_record_line(record);
            let _ = writeln!(writer, "{line}");
        }
    });
}

#[cfg(all(feature = "kernel", target_os = "none"))]
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

#[cfg(not(all(feature = "kernel", target_os = "none")))]
unsafe fn call_arm_vspace_op(
    _label: seL4_Word,
    _vspace: seL4_CPtr,
    _start: seL4_Word,
    _end: seL4_Word,
) -> seL4_Error {
    #[cfg(any(test, feature = "cache-maintenance"))]
    {
        let mut guard = CACHE_TEST_ERROR.lock();
        if let Some(err) = guard.take() {
            return err;
        }
    }
    seL4_NoError
}
