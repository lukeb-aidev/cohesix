// Author: Lukas Bower
// Purpose: DMA pin/unpin seam capturing shared-memory regions for audit.

#![cfg(feature = "kernel")]

use crate::bootstrap::log as boot_log;

/// Error surfaced when pinning a DMA range fails validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinError {
    /// The supplied virtual address was null.
    NullVaddr,
    /// The supplied physical address was null.
    NullPaddr,
    /// The supplied range length was zero.
    EmptyRange,
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

/// Validate and record a DMA-capable range.
#[inline(always)]
pub fn pin(vaddr: usize, paddr: usize, len: usize, label: &'static str) -> Result<PinnedDmaRange, PinError> {
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
    Ok(PinnedDmaRange {
        vaddr,
        paddr,
        len,
        label,
    })
}

/// Audit the release of a pinned DMA span.
#[inline(always)]
pub fn unpin(range: &PinnedDmaRange) {
    let mut line = heapless::String::<160>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[dma] unpin label={} vaddr=0x{:016x} paddr=0x{:016x} len=0x{:08x}",
            range.label,
            range.vaddr,
            range.paddr,
            range.len,
        ),
    );
    boot_log::force_uart_line(line.as_str());
}

fn log_pin_error(label: &'static str, reason: &str) {
    let mut line = heapless::String::<160>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!("[dma] pin validation failed label={label} reason={reason}"),
    );
    boot_log::force_uart_line(line.as_str());
}
