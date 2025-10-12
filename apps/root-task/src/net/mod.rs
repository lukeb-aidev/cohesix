//! Networking subsystem abstractions for host and seL4 targets.

use crate::serial::DEFAULT_LINE_CAPACITY;
use heapless::String as HeaplessString;

/// Maximum frame length supported by the networking stack (bytes).
pub const MAX_FRAME_LEN: usize = 1536;

/// Number of console lines retained between pump cycles.
pub const CONSOLE_QUEUE_DEPTH: usize = 8;

/// Networking telemetry reported by the event pump.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetTelemetry {
    /// Indicates whether the link is currently up.
    pub link_up: bool,
    /// Total TX drops recorded by the PHY.
    pub tx_drops: u32,
    /// Millisecond timestamp of the most recent poll.
    pub last_poll_ms: u64,
}

/// Networking integration exposed to the pump when the `net` feature is enabled.
pub trait NetPoller {
    /// Poll the network subsystem and return whether new work occurred.
    fn poll(&mut self, now_ms: u64) -> bool;

    /// Obtain telemetry for diagnostics.
    fn telemetry(&self) -> NetTelemetry;

    /// Drain any pending console lines produced by TCP listeners.
    fn drain_console_lines(
        &mut self,
        visitor: &mut dyn FnMut(HeaplessString<DEFAULT_LINE_CAPACITY>),
    );
}

#[cfg(target_os = "none")]
mod virtio;
#[cfg(target_os = "none")]
pub use virtio::*;

#[cfg(not(target_os = "none"))]
mod queue;
#[cfg(not(target_os = "none"))]
pub use queue::*;
