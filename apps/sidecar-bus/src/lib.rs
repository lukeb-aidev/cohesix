// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-side field bus sidecar primitives with bounded spooling.
// Author: Lukas Bower
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-side sidecar framework for MODBUS, DNP3, and LoRa adapters.
//!
//! The core primitives remain `no_std + alloc` friendly so VM builds stay
//! lean. Async runtimes and protocol stacks live behind feature gates.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

/// Link transport kinds for bus adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusLink {
    /// Serial line (RTU) transport.
    Serial,
    /// TCP transport (host-only).
    Tcp,
}

/// Configuration for bounded offline spooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpoolConfig {
    /// Maximum buffered frames.
    pub max_entries: usize,
    /// Maximum buffered bytes across all frames.
    pub max_bytes: usize,
}

impl SpoolConfig {
    /// Build a spool configuration with explicit bounds.
    pub const fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
        }
    }
}

/// Outcome when storing or rejecting spool data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpoolError {
    /// The frame would exceed queue or byte limits.
    Full,
    /// The payload exceeds the configured maximum.
    Oversize {
        /// Payload size in bytes requested by the caller.
        requested: usize,
        /// Maximum payload size permitted by configuration.
        max_bytes: usize,
    },
}

/// Recorded spool entry with deterministic sequencing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolFrame {
    /// Monotonic sequence number assigned on enqueue.
    pub seq: u64,
    /// Raw payload bytes.
    pub payload: Vec<u8>,
}

/// Bounded offline spool with deterministic replay ordering.
#[derive(Debug, Clone)]
pub struct OfflineSpool {
    config: SpoolConfig,
    queue: VecDeque<SpoolFrame>,
    bytes: usize,
    next_seq: u64,
}

impl OfflineSpool {
    /// Create a new spool with the provided bounds.
    pub fn new(config: SpoolConfig) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
            bytes: 0,
            next_seq: 1,
        }
    }

    /// Return the configured spool bounds.
    #[must_use]
    pub fn config(&self) -> SpoolConfig {
        self.config
    }

    /// Return the number of queued frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Return true when no frames are buffered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Return the total buffered bytes.
    #[must_use]
    pub fn buffered_bytes(&self) -> usize {
        self.bytes
    }

    /// Attempt to enqueue a payload into the spool.
    pub fn push(&mut self, payload: &[u8]) -> Result<SpoolFrame, SpoolError> {
        let payload_len = payload.len();
        if payload_len > self.config.max_bytes {
            return Err(SpoolError::Oversize {
                requested: payload_len,
                max_bytes: self.config.max_bytes,
            });
        }
        if self.queue.len() >= self.config.max_entries
            || self.bytes.saturating_add(payload_len) > self.config.max_bytes
        {
            return Err(SpoolError::Full);
        }
        let frame = SpoolFrame {
            seq: self.next_seq,
            payload: payload.to_vec(),
        };
        self.next_seq = self.next_seq.saturating_add(1);
        self.bytes += payload_len;
        self.queue.push_back(frame.clone());
        Ok(frame)
    }

    /// Drain buffered frames in FIFO order and clear the spool.
    pub fn drain(&mut self) -> Vec<SpoolFrame> {
        let mut drained = Vec::with_capacity(self.queue.len());
        while let Some(frame) = self.queue.pop_front() {
            drained.push(frame);
        }
        self.bytes = 0;
        drained
    }

    /// Snapshot buffered frames in FIFO order without clearing the spool.
    #[must_use]
    pub fn snapshot(&self) -> Vec<SpoolFrame> {
        self.queue.iter().cloned().collect()
    }
}

/// Connection state for a bus adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Link is available; payloads can be delivered immediately.
    Online,
    /// Link is unavailable; payloads are spooled.
    Offline,
}

/// Configuration for MODBUS/DNP3 sidecar adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusAdapterConfig {
    /// Stable adapter identifier.
    pub id: String,
    /// Mount label published under `/bus`.
    pub mount: String,
    /// Capability scope required for the adapter.
    pub scope: String,
    /// Link transport kind.
    pub link: BusLink,
    /// Serial baud rate when using a serial link.
    pub baud: u32,
    /// Offline spool bounds.
    pub spool: SpoolConfig,
}

impl BusAdapterConfig {
    /// Create a bus adapter configuration.
    pub fn new(
        id: impl Into<String>,
        mount: impl Into<String>,
        scope: impl Into<String>,
        link: BusLink,
        baud: u32,
        spool: SpoolConfig,
    ) -> Self {
        Self {
            id: id.into(),
            mount: mount.into(),
            scope: scope.into(),
            link,
            baud,
            spool,
        }
    }
}

/// Result of enqueuing a payload through the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpoolResult {
    /// Payload should be delivered immediately.
    Delivered,
    /// Payload has been buffered for replay.
    Queued {
        /// Sequence number assigned to the queued payload.
        seq: u64,
    },
}

/// Bus adapter with bounded offline spooling.
#[derive(Debug, Clone)]
pub struct BusAdapter {
    config: BusAdapterConfig,
    spool: OfflineSpool,
    state: LinkState,
}

impl BusAdapter {
    /// Construct a bus adapter in offline mode.
    pub fn new(config: BusAdapterConfig) -> Self {
        let spool = OfflineSpool::new(config.spool);
        Self {
            config,
            spool,
            state: LinkState::Offline,
        }
    }

    /// Return the adapter configuration.
    #[must_use]
    pub fn config(&self) -> &BusAdapterConfig {
        &self.config
    }

    /// Return the current link state.
    #[must_use]
    pub fn state(&self) -> LinkState {
        self.state
    }

    /// Update the link state.
    pub fn set_state(&mut self, state: LinkState) {
        self.state = state;
    }

    /// Enqueue a payload; spools if the link is offline.
    pub fn enqueue(&mut self, payload: &[u8]) -> Result<SpoolResult, SpoolError> {
        match self.state {
            LinkState::Online => Ok(SpoolResult::Delivered),
            LinkState::Offline => {
                let frame = self.spool.push(payload)?;
                Ok(SpoolResult::Queued { seq: frame.seq })
            }
        }
    }

    /// Drain buffered frames in deterministic order.
    pub fn drain_spool(&mut self) -> Vec<SpoolFrame> {
        self.spool.drain()
    }
}

#[cfg(feature = "modbus")]
/// MODBUS adapter alias for the shared bus adapter implementation.
pub type ModbusAdapter = BusAdapter;

#[cfg(feature = "dnp3")]
/// DNP3 adapter alias for the shared bus adapter implementation.
pub type Dnp3Adapter = BusAdapter;

#[cfg(feature = "lora")]
/// LoRa sidecar configuration payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoraConfig {
    /// Adapter identifier.
    pub id: String,
    /// Mount label under `/lora`.
    pub mount: String,
    /// Capability scope required for the adapter.
    pub scope: String,
    /// Region identifier (e.g., us915).
    pub region: String,
    /// Maximum payload size accepted by the bridge.
    pub max_payload_bytes: usize,
    /// Offline spool bounds.
    pub spool: SpoolConfig,
}

#[cfg(feature = "lora")]
impl LoraConfig {
    /// Create a LoRa configuration payload.
    pub fn new(
        id: impl Into<String>,
        mount: impl Into<String>,
        scope: impl Into<String>,
        region: impl Into<String>,
        max_payload_bytes: usize,
        spool: SpoolConfig,
    ) -> Self {
        Self {
            id: id.into(),
            mount: mount.into(),
            scope: scope.into(),
            region: region.into(),
            max_payload_bytes,
            spool,
        }
    }
}

#[cfg(feature = "lora")]
/// LoRa sidecar bridge with bounded offline spooling.
#[derive(Debug, Clone)]
pub struct LoraBridge {
    config: LoraConfig,
    spool: OfflineSpool,
    state: LinkState,
}

#[cfg(feature = "lora")]
impl LoraBridge {
    /// Construct a LoRa bridge in offline mode.
    pub fn new(config: LoraConfig) -> Self {
        let spool = OfflineSpool::new(config.spool);
        Self {
            config,
            spool,
            state: LinkState::Offline,
        }
    }

    /// Return the bridge configuration.
    #[must_use]
    pub fn config(&self) -> &LoraConfig {
        &self.config
    }

    /// Update the link state.
    pub fn set_state(&mut self, state: LinkState) {
        self.state = state;
    }

    /// Enqueue a payload; rejects oversize payloads deterministically.
    pub fn enqueue(&mut self, payload: &[u8]) -> Result<SpoolResult, SpoolError> {
        if payload.len() > self.config.max_payload_bytes {
            return Err(SpoolError::Oversize {
                requested: payload.len(),
                max_bytes: self.config.max_payload_bytes,
            });
        }
        match self.state {
            LinkState::Online => Ok(SpoolResult::Delivered),
            LinkState::Offline => {
                let frame = self.spool.push(payload)?;
                Ok(SpoolResult::Queued { seq: frame.seq })
            }
        }
    }

    /// Drain buffered frames in deterministic order.
    pub fn drain_spool(&mut self) -> Vec<SpoolFrame> {
        self.spool.drain()
    }
}

#[cfg(feature = "tokio")]
/// Tokio runtime helpers for async sidecar loops.
pub mod tokio_runtime {
    use core::future::Future;
    use std::time::Duration;

    /// Spawn async sidecar work onto a Tokio executor.
    pub fn spawn<F>(future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(future)
    }

    /// Sleep for the requested duration in milliseconds.
    pub async fn sleep_ms(ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spool_rejects_overflow() {
        let config = SpoolConfig::new(2, 5);
        let mut spool = OfflineSpool::new(config);
        spool.push(b"abc").expect("first ok");
        spool.push(b"de").expect("second ok");
        assert_eq!(spool.push(b"f"), Err(SpoolError::Full));
    }

    #[test]
    fn spool_replay_is_fifo() {
        let config = SpoolConfig::new(3, 16);
        let mut spool = OfflineSpool::new(config);
        let a = spool.push(b"a").expect("a");
        let b = spool.push(b"bb").expect("b");
        let c = spool.push(b"ccc").expect("c");
        let drained = spool.drain();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0], a);
        assert_eq!(drained[1], b);
        assert_eq!(drained[2], c);
        assert!(spool.is_empty());
    }

    #[cfg(feature = "modbus")]
    #[test]
    fn modbus_offline_spool_replays_in_order() {
        let config = BusAdapterConfig::new(
            "modbus-1",
            "modbus-1",
            "modbus-1",
            BusLink::Serial,
            19200,
            SpoolConfig::new(2, 8),
        );
        let mut adapter = BusAdapter::new(config);
        assert_eq!(
            adapter.enqueue(b"hi"),
            Ok(SpoolResult::Queued { seq: 1 })
        );
        assert_eq!(
            adapter.enqueue(b"ok"),
            Ok(SpoolResult::Queued { seq: 2 })
        );
        assert_eq!(adapter.enqueue(b"!"), Err(SpoolError::Full));
        let drained = adapter.drain_spool();
        let payloads: Vec<&[u8]> = drained.iter().map(|frame| frame.payload.as_slice()).collect();
        assert_eq!(payloads, vec![b"hi".as_slice(), b"ok".as_slice()]);
    }

    #[cfg(feature = "dnp3")]
    #[test]
    fn dnp3_offline_spool_respects_bounds() {
        let config = BusAdapterConfig::new(
            "dnp3-1",
            "dnp3-1",
            "dnp3-1",
            BusLink::Serial,
            9600,
            SpoolConfig::new(1, 4),
        );
        let mut adapter = BusAdapter::new(config);
        assert!(adapter.enqueue(b"1234").is_ok());
        assert_eq!(adapter.enqueue(b"9"), Err(SpoolError::Full));
        let drained = adapter.drain_spool();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].payload, b"1234");
    }
}
