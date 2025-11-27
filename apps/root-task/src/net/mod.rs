// Author: Lukas Bower

//! Networking subsystem abstractions for host and seL4 targets.

use crate::serial::DEFAULT_LINE_CAPACITY;
use heapless::String as HeaplessString;

pub use crate::net_consts::MAX_FRAME_LEN;

/// TCP port exposed by the console listener inside the VM.
pub const CONSOLE_TCP_PORT: u16 = 31337;
/// Authentication token expected from TCP console clients.
pub const AUTH_TOKEN: &str = "changeme";
/// Idle timeout applied to authenticated TCP console sessions (milliseconds).
pub const IDLE_TIMEOUT_MS: u64 = 5 * 60 * 1000;

/// Number of console lines retained between pump cycles.
pub const CONSOLE_QUEUE_DEPTH: usize = 8;

/// Configuration for console networking transports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleNetConfig {
    /// Authentication token expected from TCP console clients.
    pub auth_token: &'static str,
    /// Idle timeout applied to authenticated sessions (milliseconds).
    pub idle_timeout_ms: u64,
}

impl ConsoleNetConfig {
    /// Construct a configuration using the default constants.
    pub const fn default() -> Self {
        Self {
            auth_token: AUTH_TOKEN,
            idle_timeout_ms: IDLE_TIMEOUT_MS,
        }
    }
}

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

    /// Queue a console line for transmission to remote clients.
    fn send_console_line(&mut self, line: &str);

    /// Inject a console line into the network transport (testing hook).
    fn inject_console_line(&mut self, _line: &str) {}

    /// Reset the underlying transport (testing hook).
    fn reset(&mut self) {}
}

mod console_srv;

#[cfg(feature = "kernel")]
mod stack;
#[cfg(feature = "kernel")]
pub use stack::*;

#[cfg(not(feature = "kernel"))]
mod queue;
#[cfg(not(feature = "kernel"))]
pub use queue::*;
