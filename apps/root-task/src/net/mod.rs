// Author: Lukas Bower

//! Networking subsystem abstractions for host and seL4 targets.

#[cfg(all(feature = "kernel", feature = "net-console"))]
use smoltcp::{phy::Device, wire::EthernetAddress};

use crate::serial::DEFAULT_LINE_CAPACITY;
pub use cohesix_net_constants::COHSH_TCP_PORT;
use cohesix_net_constants::TCP_CONSOLE_PORT;
use heapless::String as HeaplessString;

pub use crate::net_consts::MAX_FRAME_LEN;

/// Default IP address for the `dev-virt` target.
pub const DEV_VIRT_IP: [u8; 4] = [10, 0, 2, 15];
/// Default gateway for the `dev-virt` target.
pub const DEV_VIRT_GATEWAY: [u8; 4] = [10, 0, 2, 2];
/// Default prefix length for the `dev-virt` target.
pub const DEV_VIRT_PREFIX: u8 = 24;

/// TCP port exposed by the console listener inside the VM.
pub const CONSOLE_TCP_PORT: u16 = TCP_CONSOLE_PORT;
/// Authentication token expected from TCP console clients.
pub const AUTH_TOKEN: &str = "changeme";
/// Idle timeout applied to authenticated TCP console sessions (milliseconds).
pub const IDLE_TIMEOUT_MS: u64 = 5 * 60 * 1000;
/// Timeout applied to authentication attempts from newly connected clients.
pub const AUTH_TIMEOUT_MS: u64 = 5 * 1000;

/// Number of console lines retained between pump cycles.
pub const CONSOLE_QUEUE_DEPTH: usize = 8;

/// Static IPv4 configuration for the TCP console listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetAddressConfig {
    /// Local interface address.
    pub ip: [u8; 4],
    /// Prefix length applied to the interface.
    pub prefix_len: u8,
    /// Default gateway, if any.
    pub gateway: Option<[u8; 4]>,
}

impl NetAddressConfig {
    /// Development defaults for the QEMU `virt` target.
    #[must_use]
    pub const fn dev_virt() -> Self {
        Self {
            ip: DEV_VIRT_IP,
            prefix_len: DEV_VIRT_PREFIX,
            gateway: Some(DEV_VIRT_GATEWAY),
        }
    }
}

/// Configuration for console networking transports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleNetConfig {
    /// Authentication token expected from TCP console clients.
    pub auth_token: &'static str,
    /// Idle timeout applied to authenticated sessions (milliseconds).
    pub idle_timeout_ms: u64,
    /// TCP port exposed by the console listener inside the VM.
    pub listen_port: u16,
    /// IPv4 configuration for the console interface.
    pub address: NetAddressConfig,
}

impl ConsoleNetConfig {
    /// Construct a configuration using the default constants.
    pub const fn default() -> Self {
        Self {
            auth_token: AUTH_TOKEN,
            idle_timeout_ms: IDLE_TIMEOUT_MS,
            listen_port: COHSH_TCP_PORT,
            address: NetAddressConfig::dev_virt(),
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

/// Driver-facing abstraction that all NIC backends must implement in order to
/// plug into the TCP console stack.
#[cfg(all(feature = "kernel", feature = "net-console"))]
pub trait NetDevice: Device {
    /// Driver-specific error type surfaced during device bring-up.
    type Error: NetDriverError;

    /// Construct a device instance using the supplied HAL.
    fn create<H>(hal: &mut H) -> Result<Self, Self::Error>
    where
        H: crate::hal::Hardware<Error = crate::hal::HalError>,
        Self: Sized;

    /// Return the Ethernet MAC address for the device.
    fn mac(&self) -> EthernetAddress;

    /// Total TX drops recorded by the driver.
    fn tx_drop_count(&self) -> u32;

    /// Human-readable label for diagnostics.
    fn name() -> &'static str
    where
        Self: Sized;

    /// Optional debug snapshot hook surfaced to stack callers.
    fn debug_snapshot(&mut self);
}

/// Helper trait used to normalise driver error handling across NIC backends.
#[cfg(all(feature = "kernel", feature = "net-console"))]
pub trait NetDriverError: core::fmt::Display + core::fmt::Debug {
    /// Indicates whether the backing device was absent during discovery.
    fn is_absent(&self) -> bool;
}

/// Supported NIC backends for the root-task TCP console.
#[cfg(all(feature = "kernel", feature = "net-console"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetBackend {
    /// RTL8139 PCI NIC exposed by QEMU `virt`.
    Rtl8139,
    /// Virtio MMIO NIC (kept for experiments and debugging).
    #[cfg(feature = "net-backend-virtio")]
    Virtio,
}

#[cfg(all(feature = "kernel", feature = "net-console"))]
impl NetBackend {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rtl8139 => "rtl8139",
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio => "virtio-net",
        }
    }
}

/// Default NIC backend used for developer QEMU runs.
#[cfg(all(
    feature = "kernel",
    feature = "net-console",
    not(feature = "net-backend-virtio")
))]
pub const DEFAULT_NET_BACKEND: NetBackend = NetBackend::Rtl8139;

/// Experimental virtio-net backend used only when explicitly selected.
#[cfg(all(
    feature = "kernel",
    feature = "net-console",
    feature = "net-backend-virtio"
))]
pub const DEFAULT_NET_BACKEND: NetBackend = NetBackend::Virtio;

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

    /// Drain pending net-console connection events (optional).
    fn drain_console_events(&mut self, _visitor: &mut dyn FnMut(NetConsoleEvent)) {}

    /// Inject a console line into the network transport (testing hook).
    fn inject_console_line(&mut self, _line: &str) {}

    /// Reset the underlying transport (testing hook).
    fn reset(&mut self) {}
}

/// Connection lifecycle notifications surfaced by TCP console transports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetConsoleEvent {
    /// A TCP console client has successfully connected.
    Connected {
        /// Unique connection identifier assigned by the stack.
        conn_id: u64,
        /// Peer address (if known).
        peer: Option<heapless::String<32>>,
    },
    /// A TCP console client disconnected or was closed by the server.
    Disconnected {
        /// Unique connection identifier assigned by the stack.
        conn_id: u64,
        /// Total bytes read from the client during the session.
        bytes_read: u64,
        /// Total bytes written to the client during the session.
        bytes_written: u64,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_net_config_uses_console_port() {
        let config = ConsoleNetConfig::default();

        assert_eq!(config.listen_port, COHSH_TCP_PORT);
        assert_ne!(config.listen_port, 0);
        assert_eq!(config.address.ip, DEV_VIRT_IP);
        assert_eq!(config.address.prefix_len, DEV_VIRT_PREFIX);
        assert_eq!(config.address.gateway, Some(DEV_VIRT_GATEWAY));
    }
}
