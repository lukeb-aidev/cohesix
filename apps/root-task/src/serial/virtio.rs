// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the serial/virtio module for root-task.
// Author: Lukas Bower

//! Deterministic virtio-console façade used by the root-task event pump.
//!
//! The implementation intentionally models the descriptor rings with
//! `heapless::spsc::Queue` so unit tests can exercise RX/TX paths without
//! accessing MMIO in host builds. Production builds replace the loopback
//! harness with MMIO-backed accessors via `VirtioRegisters`.
//!
//! The driver exposes two-facing APIs:
//! - [`SerialDriver`] implementation consumed by [`SerialPort`].
//! - `device_*` helpers that allow tests (and the eventual MMIO glue) to
//!   push bytes into RX rings and drain TX descriptors.

use heapless::spsc::Queue;

use super::{SerialDriver, SerialError};
use embedded_io::ErrorType;
use nb::Error as NbError;

/// Register-level access used by the virtio console when running on MMIO.
#[allow(dead_code)]
pub trait VirtioRegisters {
    /// Read a single byte from the RX descriptor ring.
    fn pop_rx(&mut self) -> Option<u8>;

    /// Enqueue a byte on the TX descriptor ring, returning `false` on back-pressure.
    fn push_tx(&mut self, byte: u8) -> bool;

    /// Return the number of bytes currently pending in the RX ring.
    fn rx_len(&self) -> usize;

    /// Return the number of bytes currently pending in the TX ring.
    fn tx_len(&self) -> usize;
}

/// Loopback virtio-console implementation backed by heapless queues.
#[derive(Debug)]
pub struct VirtioConsole<const RX: usize, const TX: usize> {
    rx: Queue<u8, RX>,
    tx: Queue<u8, TX>,
}

impl<const RX: usize, const TX: usize> VirtioConsole<RX, TX> {
    /// Create a new virtio-console façade with empty descriptor rings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rx: Queue::new(),
            tx: Queue::new(),
        }
    }

    /// Inject bytes that would have been produced by the device on RX.
    pub fn device_push_rx(&mut self, data: &[u8]) {
        for &byte in data {
            let _ = self.rx.enqueue(byte);
        }
    }

    /// Drain bytes written by the guest onto the TX ring.
    pub fn device_drain_tx<const OUT: usize>(&mut self) -> heapless::Vec<u8, OUT> {
        let mut out = heapless::Vec::new();
        while let Some(byte) = self.tx.dequeue() {
            if out.push(byte).is_err() {
                break;
            }
        }
        out
    }
}

impl<const RX: usize, const TX: usize> Default for VirtioConsole<RX, TX> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const RX: usize, const TX: usize> ErrorType for VirtioConsole<RX, TX> {
    type Error = SerialError;
}

impl<const RX: usize, const TX: usize> SerialDriver for VirtioConsole<RX, TX> {
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
        self.rx.dequeue().ok_or(NbError::WouldBlock)
    }

    fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        self.tx.enqueue(byte).map_err(|_| NbError::WouldBlock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtio_console_round_trips_bytes() {
        let mut console: VirtioConsole<8, 8> = VirtioConsole::new();
        console.device_push_rx(b"hello");
        assert_eq!(console.rx.len(), 5);
        let mut observed = heapless::Vec::<u8, 8>::new();
        for _ in 0..5 {
            observed.push(console.read_byte().unwrap()).unwrap();
        }
        assert_eq!(observed.as_slice(), b"hello");
        for byte in b"world" {
            console.write_byte(*byte).unwrap();
        }
        let drained = console.device_drain_tx::<8>();
        assert_eq!(drained.as_slice(), b"world");
    }

    #[test]
    fn virtio_backpressure_reports_would_block() {
        let mut console: VirtioConsole<2, 2> = VirtioConsole::new();
        console.write_byte(b'a').unwrap();
        assert!(console.write_byte(b'b').is_err());
        console.device_push_rx(b"x");
        assert_eq!(console.read_byte().unwrap(), b'x');
        assert!(console.read_byte().is_err());
    }
}
