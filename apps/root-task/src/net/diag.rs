// Author: Lukas Bower
// Purpose: Lightweight networking diagnostics for smoltcp and virtio surfaces.

//! Minimal, copyable diagnostics for the networking stack.
//! Counters are intentionally monotonic and safe to snapshot without locks.

use portable_atomic::{AtomicBool, AtomicU64, Ordering};
use crate::profile;

/// Monotonic snapshot of networking choke points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetDiagSnapshot {
    pub rx_irq_count: u64,
    pub rx_kicks: u64,
    pub rx_desc_posted: u64,
    pub rx_used_seen: u64,
    pub rx_frames_to_stack: u64,
    pub tx_submits: u64,
    pub tx_kicks: u64,
    pub tx_used_seen: u64,
    pub tx_completions: u64,
    pub poll_calls: u64,
    pub rx_frames_into_smoltcp: u64,
    pub tx_frames_from_smoltcp: u64,
    pub listener_bound: u64,
    pub accept_attempts: u64,
    pub accept_success: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub rx_cache_clean: u64,
    pub rx_cache_invalidate: u64,
}

/// Global diagnostics backing the NETDIAG line.
pub struct NetDiag {
    rx_irq_count: AtomicU64,
    rx_kicks: AtomicU64,
    rx_desc_posted: AtomicU64,
    rx_used_seen: AtomicU64,
    rx_frames_to_stack: AtomicU64,
    tx_submits: AtomicU64,
    tx_kicks: AtomicU64,
    tx_used_seen: AtomicU64,
    tx_completions: AtomicU64,
    poll_calls: AtomicU64,
    rx_frames_into_smoltcp: AtomicU64,
    tx_frames_from_smoltcp: AtomicU64,
    listener_bound: AtomicU64,
    accept_attempts: AtomicU64,
    accept_success: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    rx_cache_clean: AtomicU64,
    rx_cache_invalidate: AtomicU64,
    last_rx_used_change_ms: AtomicU64,
    stuck_warned: AtomicBool,
}

impl NetDiag {
    pub const fn new() -> Self {
        Self {
            rx_irq_count: AtomicU64::new(0),
            rx_kicks: AtomicU64::new(0),
            rx_desc_posted: AtomicU64::new(0),
            rx_used_seen: AtomicU64::new(0),
            rx_frames_to_stack: AtomicU64::new(0),
            tx_submits: AtomicU64::new(0),
            tx_kicks: AtomicU64::new(0),
            tx_used_seen: AtomicU64::new(0),
            tx_completions: AtomicU64::new(0),
            poll_calls: AtomicU64::new(0),
            rx_frames_into_smoltcp: AtomicU64::new(0),
            tx_frames_from_smoltcp: AtomicU64::new(0),
            listener_bound: AtomicU64::new(0),
            accept_attempts: AtomicU64::new(0),
            accept_success: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            rx_cache_clean: AtomicU64::new(0),
            rx_cache_invalidate: AtomicU64::new(0),
            last_rx_used_change_ms: AtomicU64::new(0),
            stuck_warned: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn snapshot(&self) -> NetDiagSnapshot {
        NetDiagSnapshot {
            rx_irq_count: self.rx_irq_count.load(Ordering::Relaxed),
            rx_kicks: self.rx_kicks.load(Ordering::Relaxed),
            rx_desc_posted: self.rx_desc_posted.load(Ordering::Relaxed),
            rx_used_seen: self.rx_used_seen.load(Ordering::Relaxed),
            rx_frames_to_stack: self.rx_frames_to_stack.load(Ordering::Relaxed),
            tx_submits: self.tx_submits.load(Ordering::Relaxed),
            tx_kicks: self.tx_kicks.load(Ordering::Relaxed),
            tx_used_seen: self.tx_used_seen.load(Ordering::Relaxed),
            tx_completions: self.tx_completions.load(Ordering::Relaxed),
            poll_calls: self.poll_calls.load(Ordering::Relaxed),
            rx_frames_into_smoltcp: self.rx_frames_into_smoltcp.load(Ordering::Relaxed),
            tx_frames_from_smoltcp: self.tx_frames_from_smoltcp.load(Ordering::Relaxed),
            listener_bound: self.listener_bound.load(Ordering::Relaxed),
            accept_attempts: self.accept_attempts.load(Ordering::Relaxed),
            accept_success: self.accept_success.load(Ordering::Relaxed),
            bytes_read: self.bytes_read.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            rx_cache_clean: self.rx_cache_clean.load(Ordering::Relaxed),
            rx_cache_invalidate: self.rx_cache_invalidate.load(Ordering::Relaxed),
        }
    }

    #[inline]
    pub fn record_rx_irq(&self) {
        let _ = self.rx_irq_count.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_rx_kick(&self) {
        let _ = self.rx_kicks.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_rx_desc_posted(&self) {
        let _ = self.rx_desc_posted.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_rx_used_seen(&self, now_ms: u64) {
        let _ = self.rx_used_seen.fetch_add(1, Ordering::Relaxed);
        self.last_rx_used_change_ms.store(now_ms, Ordering::Relaxed);
        self.stuck_warned.store(false, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_rx_frame_to_stack(&self) {
        let _ = self.rx_frames_to_stack.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_tx_submit(&self) {
        let _ = self.tx_submits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_tx_kick(&self) {
        let _ = self.tx_kicks.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_tx_used_seen(&self) {
        let _ = self.tx_used_seen.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_tx_completion(&self) {
        let _ = self.tx_completions.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_poll_call(&self) {
        let _ = self.poll_calls.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_smoltcp_rx(&self) {
        let _ = self.rx_frames_into_smoltcp.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_smoltcp_tx(&self) {
        let _ = self.tx_frames_from_smoltcp.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_listener_bound(&self) {
        let _ = self.listener_bound.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_accept_attempt(&self) {
        let _ = self.accept_attempts.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_accept_success(&self) {
        let _ = self.accept_success.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn add_bytes_read(&self, bytes: u64) {
        let _ = self.bytes_read.fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn add_bytes_written(&self, bytes: u64) {
        let _ = self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_rx_cache_clean(&self) {
        let _ = self.rx_cache_clean.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_rx_cache_invalidate(&self) {
        let _ = self.rx_cache_invalidate.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn last_rx_used_change_ms(&self) -> u64 {
        self.last_rx_used_change_ms.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn mark_stuck_warned(&self) {
        self.stuck_warned.store(true, Ordering::Relaxed);
    }

    #[inline]
    pub fn stuck_warned(&self) -> bool {
        self.stuck_warned.load(Ordering::Relaxed)
    }
}

/// Compile-time toggle for NETDIAG emissions without enabling the TCP console.
pub const NET_DIAG_FEATURED: bool = profile::NET_DIAG_FEATURED;

/// Global diagnostics instance.
pub static NET_DIAG: NetDiag = NetDiag::new();
