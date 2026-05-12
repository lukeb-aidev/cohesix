// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Runtime UART backend selection for kernel serial console support.
// Author: Lukas Bower
//! Runtime UART backend selection for kernel serial transport.

#![cfg(feature = "kernel")]

use core::ptr::NonNull;

use embedded_io::ErrorType;

use crate::hal::uart::{PI4_MINI_UART_PADDR, PI4_PL011_PADDR, QEMU_PL011_PADDR};

use super::bcm2711_mini_uart::{Bcm2711MiniUart, Bcm2711MiniUartMmio};
use super::pl011::{Pl011, Pl011Mmio};
use super::{SerialDriver, SerialError};
use sel4_sys::seL4_CPtr;

/// Ordered UART candidates for runtime probing.
pub const UART_CANDIDATES: [KernelUartCandidate; 3] = [
    KernelUartCandidate::QemuPl011,
    KernelUartCandidate::Pi4MiniUart,
    KernelUartCandidate::Pi4Pl011,
];

/// Runtime-selected kernel UART backend kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelUartKind {
    /// ARM PL011-compatible UART.
    Pl011,
    /// BCM2711 auxiliary mini-UART.
    Bcm2711MiniUart,
}

/// Physical UART candidate probed during boot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelUartCandidate {
    /// QEMU `virt` PL011.
    QemuPl011,
    /// Pi 4 mini-UART.
    Pi4MiniUart,
    /// Pi 4 PL011 UART0 fallback.
    Pi4Pl011,
}

impl KernelUartCandidate {
    /// Candidate label used in boot diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::QemuPl011 => "qemu-pl011",
            Self::Pi4MiniUart => "pi4-mini-uart",
            Self::Pi4Pl011 => "pi4-pl011",
        }
    }

    /// Candidate physical MMIO base.
    #[must_use]
    pub const fn paddr(self) -> usize {
        match self {
            Self::QemuPl011 => QEMU_PL011_PADDR,
            Self::Pi4MiniUart => PI4_MINI_UART_PADDR,
            Self::Pi4Pl011 => PI4_PL011_PADDR,
        }
    }

    /// UART backend kind for the candidate.
    #[must_use]
    pub const fn kind(self) -> KernelUartKind {
        match self {
            Self::QemuPl011 | Self::Pi4Pl011 => KernelUartKind::Pl011,
            Self::Pi4MiniUart => KernelUartKind::Bcm2711MiniUart,
        }
    }
}

/// MMIO mapping metadata for the runtime-selected kernel UART.
#[derive(Clone, Copy, Debug)]
pub enum KernelUartMmio {
    /// PL011 mapping.
    Pl011(Pl011Mmio),
    /// BCM2711 mini-UART mapping.
    Bcm2711MiniUart(Bcm2711MiniUartMmio),
}

impl KernelUartMmio {
    /// Build mapping metadata from a candidate, capability slot, and virtual base pointer.
    #[must_use]
    pub fn mapped(candidate: KernelUartCandidate, cap: seL4_CPtr, vaddr: NonNull<u8>) -> Self {
        match candidate.kind() {
            KernelUartKind::Pl011 => Self::Pl011(Pl011Mmio::mapped(candidate.paddr(), cap, vaddr)),
            KernelUartKind::Bcm2711MiniUart => {
                Self::Bcm2711MiniUart(Bcm2711MiniUartMmio::mapped(candidate.paddr(), cap, vaddr))
            }
        }
    }

    /// Returns the backend kind.
    #[must_use]
    pub const fn kind(self) -> KernelUartKind {
        match self {
            Self::Pl011(_) => KernelUartKind::Pl011,
            Self::Bcm2711MiniUart(_) => KernelUartKind::Bcm2711MiniUart,
        }
    }

    /// Returns a stable backend label for logs.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pl011(_) => "pl011",
            Self::Bcm2711MiniUart(_) => "bcm2711-mini-uart",
        }
    }

    /// Physical address backing the mapping.
    #[must_use]
    pub fn paddr(self) -> usize {
        match self {
            Self::Pl011(mmio) => mmio.paddr(),
            Self::Bcm2711MiniUart(mmio) => mmio.paddr(),
        }
    }

    /// Virtual address backing the mapping.
    #[must_use]
    pub fn vaddr(self) -> NonNull<u8> {
        match self {
            Self::Pl011(mmio) => mmio.vaddr(),
            Self::Bcm2711MiniUart(mmio) => mmio.vaddr(),
        }
    }

    /// Capability slot used to map the UART, if available.
    #[must_use]
    pub fn cap(self) -> Option<seL4_CPtr> {
        match self {
            Self::Pl011(mmio) => mmio.cap(),
            Self::Bcm2711MiniUart(mmio) => mmio.cap(),
        }
    }

    /// Whether the UART mapping is live.
    #[must_use]
    pub fn is_mapped(self) -> bool {
        match self {
            Self::Pl011(mmio) => mmio.is_mapped(),
            Self::Bcm2711MiniUart(mmio) => mmio.is_mapped(),
        }
    }

    /// Validate alignment and span coverage for the mapped UART page.
    pub fn assert_page_coverage(self, page_size: usize) {
        match self {
            Self::Pl011(mmio) => mmio.assert_page_coverage(page_size, 0x0ff),
            Self::Bcm2711MiniUart(mmio) => mmio.assert_page_coverage(page_size, 0x068),
        }
    }
}

/// Runtime-selected serial driver used by the event pump in kernel builds.
pub enum KernelSerialDriver {
    /// PL011 backend.
    Pl011(Pl011),
    /// BCM2711 mini-UART backend.
    Bcm2711MiniUart(Bcm2711MiniUart),
    /// No physical UART is mapped; reads/writes stay non-blocking no-op.
    Null,
}

impl KernelSerialDriver {
    /// Build a serial driver for the supplied MMIO mapping.
    #[must_use]
    pub fn from_mmio(mmio: KernelUartMmio) -> Self {
        match mmio {
            KernelUartMmio::Pl011(mapping) => Self::Pl011(Pl011::new(mapping.vaddr())),
            KernelUartMmio::Bcm2711MiniUart(mapping) => {
                Self::Bcm2711MiniUart(Bcm2711MiniUart::new(mapping.vaddr()))
            }
        }
    }

    /// Construct a no-op serial backend used when no UART mapping is available.
    #[must_use]
    pub const fn null() -> Self {
        Self::Null
    }

    /// Backend kind.
    #[must_use]
    pub const fn kind(&self) -> KernelUartKind {
        match self {
            Self::Pl011(_) => KernelUartKind::Pl011,
            Self::Bcm2711MiniUart(_) => KernelUartKind::Bcm2711MiniUart,
            Self::Null => KernelUartKind::Pl011,
        }
    }

    /// Stable backend label for diagnostics.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Pl011(_) => "pl011",
            Self::Bcm2711MiniUart(_) => "bcm2711-mini-uart",
            Self::Null => "none",
        }
    }

    /// Returns the backend virtual address.
    #[must_use]
    pub fn vaddr(&self) -> usize {
        match self {
            Self::Pl011(driver) => driver.vaddr(),
            Self::Bcm2711MiniUart(driver) => driver.vaddr(),
            Self::Null => 0,
        }
    }

    /// Initialise the selected UART backend.
    pub fn init(&mut self) {
        match self {
            Self::Pl011(driver) => driver.init(),
            Self::Bcm2711MiniUart(driver) => driver.init(),
            Self::Null => {}
        }
    }

    /// Write a string using backend-specific blocking helpers.
    pub fn write_str(&mut self, text: &str) {
        match self {
            Self::Pl011(driver) => driver.write_str(text),
            Self::Bcm2711MiniUart(driver) => driver.write_str(text),
            Self::Null => {
                let _ = text;
            }
        }
    }

    /// Consume the driver and return the PL011 backend when selected.
    #[must_use]
    pub fn into_pl011(self) -> Option<Pl011> {
        match self {
            Self::Pl011(driver) => Some(driver),
            Self::Bcm2711MiniUart(_) | Self::Null => None,
        }
    }
}

impl ErrorType for KernelSerialDriver {
    type Error = SerialError;
}

impl SerialDriver for KernelSerialDriver {
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
        match self {
            Self::Pl011(driver) => driver.read_byte(),
            Self::Bcm2711MiniUart(driver) => driver.read_byte(),
            Self::Null => Err(nb::Error::WouldBlock),
        }
    }

    fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        match self {
            Self::Pl011(driver) => driver.write_byte(byte),
            Self::Bcm2711MiniUart(driver) => driver.write_byte(byte),
            Self::Null => {
                let _ = byte;
                Err(nb::Error::WouldBlock)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi4_runtime_uart_prefers_mini_uart_before_pl011() {
        assert_eq!(
            UART_CANDIDATES,
            [
                KernelUartCandidate::QemuPl011,
                KernelUartCandidate::Pi4MiniUart,
                KernelUartCandidate::Pi4Pl011,
            ]
        );
        assert_eq!(KernelUartCandidate::Pi4Pl011.kind(), KernelUartKind::Pl011);
        assert_eq!(
            KernelUartCandidate::Pi4MiniUart.kind(),
            KernelUartKind::Bcm2711MiniUart
        );
    }
}
