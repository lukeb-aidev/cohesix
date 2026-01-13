// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: DMA pin/unpin seam capturing shared-memory regions for audit.
// Author: Lukas Bower

#![cfg(any(feature = "kernel", feature = "cache-maintenance"))]

#[cfg(feature = "kernel")]
use crate::bootstrap::log as boot_log;
use crate::hal::cache::{CacheError, CacheMaintenance};

#[cfg(not(feature = "kernel"))]
use std::sync::Mutex;

/// Error surfaced when pinning a DMA range fails validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinError {
    /// The supplied virtual address was null.
    NullVaddr,
    /// The supplied physical address was null.
    NullPaddr,
    /// The supplied range length was zero.
    EmptyRange,
    /// Cache maintenance failed while preparing the shared range.
    CacheFailure(CacheError),
}

/// Describes a DMA-capable memory span shared with a device or host surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedDmaRange {
    vaddr: usize,
    paddr: usize,
    len: usize,
    label: &'static str,
}

impl PinnedDmaRange {
    /// Virtual base address of the pinned range.
    #[must_use]
    pub const fn vaddr(&self) -> usize {
        self.vaddr
    }

    /// Physical base address of the pinned range.
    #[must_use]
    pub const fn paddr(&self) -> usize {
        self.paddr
    }

    /// Length of the pinned range in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Label associated with the pinned span.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        self.label
    }
}

#[derive(Clone, Copy, Debug)]
struct CachePolicy {
    kernel_ops: bool,
    dma_clean: bool,
    dma_invalidate: bool,
    unify_instructions: bool,
}

fn cache_policy() -> CachePolicy {
    #[cfg(feature = "kernel")]
    {
        let policy = crate::generated::cache_policy();
        CachePolicy {
            kernel_ops: policy.kernel_ops,
            dma_clean: policy.dma_clean,
            dma_invalidate: policy.dma_invalidate,
            unify_instructions: policy.unify_instructions,
        }
    }

    #[cfg(not(feature = "kernel"))]
    {
        CachePolicy {
            kernel_ops: true,
            dma_clean: true,
            dma_invalidate: true,
            unify_instructions: false,
        }
    }
}

fn cache_ops_requested(policy: CachePolicy) -> bool {
    policy.dma_clean || policy.dma_invalidate || policy.unify_instructions
}

#[cfg(not(feature = "kernel"))]
static DMA_AUDIT_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn emit_audit_line(line: &str) {
    if line.trim().is_empty() {
        return;
    }

    #[cfg(feature = "kernel")]
    {
        boot_log::force_uart_line(line);
    }

    #[cfg(not(feature = "kernel"))]
    {
        let mut guard = DMA_AUDIT_LOG.lock().expect("dma audit log");
        guard.push(line.to_string());
    }
}

#[cfg(any(test, all(feature = "cache-maintenance", not(feature = "kernel"))))]
pub fn take_audit_log() -> Vec<String> {
    let mut guard = DMA_AUDIT_LOG.lock().expect("dma audit log");
    let mut out = Vec::new();
    core::mem::swap(&mut *guard, &mut out);
    out
}

/// Validate and record a DMA-capable range.
#[inline(always)]
pub fn pin(
    vaddr: usize,
    paddr: usize,
    len: usize,
    label: &'static str,
) -> Result<PinnedDmaRange, PinError> {
    if vaddr == 0 {
        log_pin_error(label, "null-vaddr");
        return Err(PinError::NullVaddr);
    }
    if paddr == 0 {
        log_pin_error(label, "null-paddr");
        return Err(PinError::NullPaddr);
    }
    if len == 0 {
        log_pin_error(label, "empty-range");
        return Err(PinError::EmptyRange);
    }

    let range = PinnedDmaRange {
        vaddr,
        paddr,
        len,
        label,
    };

    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[dma][share] prepare label={} vaddr=0x{:016x} paddr=0x{:016x} len=0x{:08x}",
            range.label, range.vaddr, range.paddr, range.len,
        ),
    );
    emit_audit_line(line.as_str());

    let policy = cache_policy();
    if cache_ops_requested(policy) {
        if !policy.kernel_ops {
            log_pin_error(label, "cache-kernel-ops-disabled");
            return Err(PinError::CacheFailure(CacheError::new(
                sel4_sys::seL4_InvalidArgument,
            )));
        }

        let maintenance = CacheMaintenance::init_thread();
        if policy.dma_clean {
            emit_cache_line("clean-before-share", &range);
            if let Err(err) = maintenance.clean(range.vaddr, range.len) {
                emit_cache_error("clean-before-share", &range, err);
                return Err(PinError::CacheFailure(err));
            }
        }

        if policy.unify_instructions {
            emit_cache_line("unify-before-share", &range);
            if let Err(err) = maintenance.unify_instruction(range.vaddr, range.len) {
                emit_cache_error("unify-before-share", &range, err);
                return Err(PinError::CacheFailure(err));
            }
        }

    }

    let mut ready = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut ready,
        format_args!(
            "[dma][share] ready label={} vaddr=0x{:016x} paddr=0x{:016x} len=0x{:08x}",
            range.label, range.vaddr, range.paddr, range.len,
        ),
    );
    emit_audit_line(ready.as_str());

    Ok(range)
}

/// Audit the release of a pinned DMA span.
#[inline(always)]
pub fn unpin(range: &PinnedDmaRange) -> Result<(), CacheError> {
    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[dma][share] reclaim label={} vaddr=0x{:016x} paddr=0x{:016x} len=0x{:08x}",
            range.label, range.vaddr, range.paddr, range.len,
        ),
    );
    emit_audit_line(line.as_str());

    let policy = cache_policy();
    if cache_ops_requested(policy) {
        if policy.dma_invalidate {
            emit_cache_line("invalidate-after-reclaim", range);
            let maintenance = CacheMaintenance::init_thread();
            if let Err(err) = maintenance.invalidate(range.vaddr, range.len) {
                emit_cache_error("invalidate-after-reclaim", range, err);
                return Err(err);
            }
        }
    }

    let mut done = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut done,
        format_args!(
            "[dma][share] reclaimed label={} vaddr=0x{:016x} paddr=0x{:016x} len=0x{:08x}",
            range.label, range.vaddr, range.paddr, range.len,
        ),
    );
    emit_audit_line(done.as_str());
    Ok(())
}

fn log_pin_error(label: &'static str, reason: &str) {
    let mut line = heapless::String::<160>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!("[dma] pin validation failed label={label} reason={reason}"),
    );
    emit_audit_line(line.as_str());
}

fn emit_cache_line(stage: &str, range: &PinnedDmaRange) {
    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[dma][cache] {} label={} vaddr=0x{:016x} paddr=0x{:016x} len=0x{:08x}",
            stage, range.label, range.vaddr, range.paddr, range.len,
        ),
    );
    emit_audit_line(line.as_str());
}

fn emit_cache_error(stage: &str, range: &PinnedDmaRange, err: CacheError) {
    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[dma][cache] {} failed label={} vaddr=0x{:016x} paddr=0x{:016x} len=0x{:08x} err={} kind={:?}",
            stage,
            range.label,
            range.vaddr,
            range.paddr,
            range.len,
            err.code(),
            err.kind(),
        ),
    );
    emit_audit_line(line.as_str());
}
