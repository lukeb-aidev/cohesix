// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: DMA pin/unpin seam capturing shared-memory regions for audit.
// Author: Lukas Bower

#![cfg(any(feature = "kernel", feature = "cache-maintenance"))]

#[cfg(feature = "kernel")]
use crate::bootstrap::log as boot_log;
use crate::hal::cache::{CacheError, CacheMaintenance};

#[cfg(all(not(target_os = "none"), any(test, not(feature = "kernel"))))]
use std::{string::String, sync::Mutex, vec::Vec};

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

#[inline]
fn audit_suppressed_for_label(label: &str) -> bool {
    matches!(
        label,
        "xhci-scratchpad-page"
            | "xhci-event-ring-debug-prefix"
            | "xhci-event-ring-prompt-safe"
            | "xhci-event-ring-poll-fast"
    )
}

#[inline]
fn device_publish_uses_clean_invalidate(label: &str) -> bool {
    label.starts_with("xhci-")
}

#[cfg(all(not(target_os = "none"), any(test, not(feature = "kernel"))))]
static DMA_AUDIT_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn emit_audit_line(line: &str) {
    if line.trim().is_empty() {
        return;
    }

    #[cfg(feature = "kernel")]
    {
        boot_log::force_uart_line(line);
    }

    #[cfg(all(not(target_os = "none"), any(test, not(feature = "kernel"))))]
    {
        let mut guard = DMA_AUDIT_LOG.lock().expect("dma audit log");
        guard.push(line.to_string());
    }
}

#[cfg(all(
    not(target_os = "none"),
    any(test, all(feature = "cache-maintenance", not(feature = "kernel")))
))]
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

    let suppress_audit = audit_suppressed_for_label(label);
    if !suppress_audit {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::write(
            &mut line,
            format_args!(
                "[dma][share] prepare label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x}",
                range.label, range.vaddr, range.paddr, range.len,
            ),
        );
        emit_audit_line(line.as_str());
    }

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
            let clean_invalidate =
                policy.dma_invalidate && device_publish_uses_clean_invalidate(label);
            if !suppress_audit {
                let stage = if clean_invalidate {
                    "clean-invalidate-before-share"
                } else {
                    "clean-before-share"
                };
                emit_cache_line(stage, &range);
            }
            let clean_result = if clean_invalidate {
                maintenance.clean_invalidate(range.vaddr, range.len)
            } else {
                maintenance.clean(range.vaddr, range.len)
            };
            if let Err(err) = clean_result {
                let stage = if clean_invalidate {
                    "clean-invalidate-before-share"
                } else {
                    "clean-before-share"
                };
                emit_cache_error(stage, &range, err);
                return Err(PinError::CacheFailure(err));
            }
        }

        if policy.unify_instructions {
            if !suppress_audit {
                emit_cache_line("unify-before-share", &range);
            }
            if let Err(err) = maintenance.unify_instruction(range.vaddr, range.len) {
                emit_cache_error("unify-before-share", &range, err);
                return Err(PinError::CacheFailure(err));
            }
        }
    }

    if !suppress_audit {
        let mut ready = heapless::String::<192>::new();
        let _ = core::fmt::write(
            &mut ready,
            format_args!(
                "[dma][share] ready label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x}",
                range.label, range.vaddr, range.paddr, range.len,
            ),
        );
        emit_audit_line(ready.as_str());
    }

    Ok(range)
}

/// Synchronize a DMA-capable range for CPU reads after device writes.
#[inline(always)]
pub fn sync_for_cpu(
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

    let suppress_audit = audit_suppressed_for_label(label);
    if !suppress_audit {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::write(
            &mut line,
            format_args!(
                "[dma][share] sync-for-cpu label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x}",
                range.label, range.vaddr, range.paddr, range.len,
            ),
        );
        emit_audit_line(line.as_str());
    }

    let policy = cache_policy();
    if cache_ops_requested(policy) && policy.dma_invalidate {
        if !policy.kernel_ops {
            log_pin_error(label, "cache-kernel-ops-disabled");
            return Err(PinError::CacheFailure(CacheError::new(
                sel4_sys::seL4_InvalidArgument,
            )));
        }

        if !suppress_audit {
            emit_cache_line("invalidate-before-cpu-read", &range);
        }
        let maintenance = CacheMaintenance::init_thread();
        if let Err(err) = maintenance.invalidate(range.vaddr, range.len) {
            emit_cache_error("invalidate-before-cpu-read", &range, err);
            return Err(PinError::CacheFailure(err));
        }
    }

    if !suppress_audit {
        let mut ready = heapless::String::<192>::new();
        let _ = core::fmt::write(
            &mut ready,
            format_args!(
                "[dma][share] cpu-ready label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x}",
                range.label, range.vaddr, range.paddr, range.len,
            ),
        );
        emit_audit_line(ready.as_str());
    }

    Ok(range)
}

/// Audit the release of a pinned DMA span.
#[inline(always)]
pub fn unpin(range: &PinnedDmaRange) -> Result<(), CacheError> {
    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[dma][share] reclaim label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x}",
            range.label, range.vaddr, range.paddr, range.len,
        ),
    );
    emit_audit_line(line.as_str());

    let policy = cache_policy();
    if cache_ops_requested(policy) {
        if policy.dma_invalidate {
            if !policy.kernel_ops {
                log_pin_error(range.label, "cache-kernel-ops-disabled");
                return Err(CacheError::new(sel4_sys::seL4_InvalidArgument));
            }

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
            "[dma][share] reclaimed label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x}",
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
            "[dma][cache] {} label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x}",
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
            "[dma][cache] {} failed label={} vaddr=0x{:016x} paddr=0x{:016x} addr_domain=cpu-phys len=0x{:08x} err={} kind={:?}",
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

#[cfg(test)]
mod tests {
    use super::{audit_suppressed_for_label, pin, sync_for_cpu, take_audit_log, unpin};
    use std::sync::Mutex as StdMutex;

    static DMA_AUDIT_TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn assert_stage_order(lines: &[String], stages: &[&str]) {
        let mut cursor = 0usize;
        for stage in stages {
            let Some((offset, _)) = lines[cursor..]
                .iter()
                .enumerate()
                .find(|(_, line)| line.contains(stage))
            else {
                panic!("missing DMA audit stage {stage}; lines={lines:?}");
            };
            cursor += offset + 1;
        }
    }

    #[test]
    fn prompt_safe_event_ring_polling_uses_summary_breadcrumbs_not_dma_spam() {
        assert!(audit_suppressed_for_label("xhci-event-ring-prompt-safe"));
        assert!(audit_suppressed_for_label("xhci-event-ring-poll-fast"));
        assert!(!audit_suppressed_for_label("xhci-event-ring-poll"));
        assert!(!audit_suppressed_for_label("xhci-cmd-ring-submit-full"));
    }

    #[test]
    fn dma_pin_audit_orders_prepare_clean_and_ready() {
        let _guard = DMA_AUDIT_TEST_LOCK.lock().expect("dma audit test lock");
        let _ = take_audit_log();

        let range = pin(0x1000, 0x2000, 0x80, "wifi-sdio-audit-order").expect("pin succeeds");

        assert_eq!(range.vaddr(), 0x1000);
        assert_eq!(range.paddr(), 0x2000);
        let lines = take_audit_log();
        assert_stage_order(&lines, &["prepare", "clean-before-share", "ready"]);
    }

    #[test]
    fn xhci_dma_publish_uses_clean_invalidate_like_uboot_flush() {
        let _guard = DMA_AUDIT_TEST_LOCK.lock().expect("dma audit test lock");
        let _ = take_audit_log();

        let range = pin(0x7000, 0x8000, 0x400, "xhci-cmd-ring-submit-full").expect("pin succeeds");

        assert_eq!(range.len(), 0x400);
        let lines = take_audit_log();
        assert_stage_order(
            &lines,
            &["prepare", "clean-invalidate-before-share", "ready"],
        );
        assert!(
            !lines.iter().any(|line| line.contains("clean-before-share")),
            "xHCI publish should use clean+invalidate, not clean-only: {lines:?}"
        );
    }

    #[test]
    fn dma_sync_for_cpu_audit_orders_invalidate_before_cpu_ready() {
        let _guard = DMA_AUDIT_TEST_LOCK.lock().expect("dma audit test lock");
        let _ = take_audit_log();

        let range =
            sync_for_cpu(0x3000, 0x4000, 0x100, "wifi-sdio-cpu-sync").expect("sync succeeds");

        assert_eq!(range.len(), 0x100);
        let lines = take_audit_log();
        assert_stage_order(
            &lines,
            &["sync-for-cpu", "invalidate-before-cpu-read", "cpu-ready"],
        );
    }

    #[test]
    fn dma_unpin_audit_orders_reclaim_invalidate_and_reclaimed() {
        let _guard = DMA_AUDIT_TEST_LOCK.lock().expect("dma audit test lock");
        let _ = take_audit_log();
        let range = pin(0x5000, 0x6000, 0x40, "wifi-sdio-reclaim").expect("pin succeeds");
        let _ = take_audit_log();

        unpin(&range).expect("unpin succeeds");

        let lines = take_audit_log();
        assert_stage_order(
            &lines,
            &["reclaim", "invalidate-after-reclaim", "reclaimed"],
        );
    }
}
