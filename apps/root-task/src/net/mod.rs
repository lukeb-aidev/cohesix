// Author: Lukas Bower
// Purpose: Networking subsystem abstractions and configuration for console transports.

//! Networking subsystem abstractions for host and seL4 targets.

#[cfg(all(feature = "kernel", feature = "net-console"))]
use smoltcp::{phy::Device, wire::EthernetAddress};

#[cfg(feature = "net")]
pub mod diag;
#[cfg(feature = "net")]
pub use diag::{NetDiagSnapshot, NET_DIAG, NET_DIAG_FEATURED};

use core::ops::Range;

use crate::serial::DEFAULT_LINE_CAPACITY;
#[cfg(all(feature = "net", feature = "kernel"))]
pub mod outbound;
pub use cohesix_net_constants::{COHESIX_TCP_CONSOLE_PORT, COHSH_TCP_PORT, TCP_CONSOLE_PORT};
use heapless::String as HeaplessString;

pub use crate::net_consts::MAX_FRAME_LEN;

/// Default IP address for the `dev-virt` target.
pub const DEV_VIRT_IP: [u8; 4] = [10, 0, 2, 15];
/// Default gateway for the `dev-virt` target.
pub const DEV_VIRT_GATEWAY: [u8; 4] = [10, 0, 2, 2];
/// Default prefix length for the `dev-virt` target.
pub const DEV_VIRT_PREFIX: u8 = 24;

/// TCP port exposed by the console listener inside the VM.
pub const CONSOLE_TCP_PORT: u16 = COHESIX_TCP_CONSOLE_PORT;
/// Authentication token expected from TCP console clients.
pub const AUTH_TOKEN: &str = "changeme";
/// Idle timeout applied to authenticated TCP console sessions (milliseconds).
pub const IDLE_TIMEOUT_MS: u64 = 5 * 60 * 1000;
/// Timeout applied to authentication attempts from newly connected clients.
pub const AUTH_TIMEOUT_MS: u64 = 5 * 1000;

/// Number of console lines retained between pump cycles.
pub const CONSOLE_QUEUE_DEPTH: usize = 8;

/// Build-time network bring-up stage selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetStage {
    ProbeOnly,
    QueueInitOnly,
    RxOnly,
    TxOnly,
    ArpOnly,
    IcmpOnly,
    TcpHandshakeOnly,
    Full,
}

/// Compile-time staging selector for network bring-up.
pub const NET_STAGE: NetStage = NetStage::Full;

impl NetStage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProbeOnly => "probe_only",
            Self::QueueInitOnly => "queue_init_only",
            Self::RxOnly => "rx_only",
            Self::TxOnly => "tx_only",
            Self::ArpOnly => "arp_only",
            Self::IcmpOnly => "icmp_only",
            Self::TcpHandshakeOnly => "tcp_handshake_only",
            Self::Full => "full",
        }
    }
}

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

    /// Apply development-friendly defaults for the QEMU `virt` target when
    /// configuration inputs (e.g. DTB or bootinfo) are absent or incomplete.
    #[must_use]
    pub fn with_dev_virt_defaults(mut self) -> Self {
        if self.listen_port == 0 {
            self.listen_port = COHESIX_TCP_CONSOLE_PORT;
        }
        if self.address.ip == [0, 0, 0, 0] {
            self.address.ip = DEV_VIRT_IP;
            self.address.prefix_len = DEV_VIRT_PREFIX;
        }
        if self.address.prefix_len == 0 {
            self.address.prefix_len = DEV_VIRT_PREFIX;
        }
        if self.address.gateway.is_none() {
            self.address.gateway = Some(DEV_VIRT_GATEWAY);
        }
        self
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

/// Counters gathered from the NIC driver for diagnostics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetDeviceCounters {
    /// RX packets consumed by smoltcp.
    pub rx_packets: u64,
    /// TX packets submitted to the NIC.
    pub tx_packets: u64,
    /// RX used ring advances observed by the driver.
    pub rx_used_advances: u64,
    /// TX used ring advances observed by the driver.
    pub tx_used_advances: u64,
    /// TX submissions observed by the driver.
    pub tx_submit: u64,
    /// TX completions observed by the driver.
    pub tx_complete: u64,
    /// TX free descriptors available.
    pub tx_free: u64,
    /// TX descriptors currently in flight.
    pub tx_in_flight: u64,
    /// TX double-submit attempts detected.
    pub tx_double_submit: u64,
    /// TX zero-length submit attempts detected.
    pub tx_zero_len_attempt: u64,
    /// TX publish attempts blocked because the descriptor length was zero.
    pub dropped_zero_len_tx: u64,
    /// TX publishes rejected due to duplicate or busy slot state.
    pub tx_dup_publish_blocked: u64,
    /// TX used entries ignored due to duplicate completions.
    pub tx_dup_used_ignored: u64,
    /// TX used entries referencing unexpected heads or generations.
    pub tx_invalid_used_state: u64,
    /// TX allocations blocked while descriptors remain in-flight.
    pub tx_alloc_blocked_inflight: u64,
}

/// Monotonic counters collected from the NIC driver and smoltcp sockets.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetCounters {
    /// RX packets handed to smoltcp.
    pub rx_packets: u64,
    /// TX packets submitted by smoltcp.
    pub tx_packets: u64,
    /// RX used ring advances observed by the driver.
    pub rx_used_advances: u64,
    /// TX used ring advances observed by the driver.
    pub tx_used_advances: u64,
    /// Total smoltcp poll iterations.
    pub smoltcp_polls: u64,
    /// UDP packets received.
    pub udp_rx: u64,
    /// UDP packets transmitted.
    pub udp_tx: u64,
    /// TCP accepts observed.
    pub tcp_accepts: u64,
    /// TCP RX bytes consumed.
    pub tcp_rx_bytes: u64,
    /// TCP TX bytes submitted.
    pub tcp_tx_bytes: u64,
    /// Successful outbound TCP smoke test completions.
    pub tcp_smoke_outbound: u64,
    /// Failed outbound TCP smoke test attempts.
    pub tcp_smoke_outbound_failures: u64,
    /// TX submissions observed by the driver.
    pub tx_submit: u64,
    /// TX completions observed by the driver.
    pub tx_complete: u64,
    /// TX free descriptors available.
    pub tx_free: u64,
    /// TX descriptors currently in flight.
    pub tx_in_flight: u64,
    /// TX double-submit attempts detected.
    pub tx_double_submit: u64,
    /// TX zero-length submit attempts detected.
    pub tx_zero_len_attempt: u64,
    /// TX publish attempts blocked because the descriptor length was zero.
    pub dropped_zero_len_tx: u64,
}

/// Outcome of the latest network self-test pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetSelfTestResult {
    /// Whether UDP beacons were sent successfully.
    pub tx_ok: bool,
    /// Whether an inbound UDP echo was observed.
    pub udp_echo_ok: bool,
    /// Whether the TCP smoke test completed.
    pub tcp_ok: bool,
    /// Whether the TCP console listener responded and recovered.
    pub console_ok: bool,
}

/// Summary of the self-test subsystem for consoles.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetSelfTestReport {
    /// Indicates whether self-test support is compiled in for the current build.
    pub enabled: bool,
    /// True while a self-test run is active.
    pub running: bool,
    /// Last recorded result, if any.
    pub last_result: Option<NetSelfTestResult>,
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

    /// Construct a device instance for the supplied bring-up stage.
    fn create_with_stage<H>(hal: &mut H, _stage: NetStage) -> Result<Self, Self::Error>
    where
        H: crate::hal::Hardware<Error = crate::hal::HalError>,
        Self: Sized,
    {
        Self::create(hal)
    }

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

    /// Optional debug hook to validate TX avail ring state.
    fn debug_scan_tx_avail_duplicates(&mut self) {}

    /// Counter snapshot for diagnostics.
    fn counters(&self) -> NetDeviceCounters {
        NetDeviceCounters::default()
    }

    /// Returns the primary queue memory bounds, if applicable, for overlap diagnostics.
    fn buffer_bounds(&self) -> Option<Range<usize>> {
        None
    }
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

    /// Retrieve cumulative counters for diagnostics.
    fn stats(&self) -> NetCounters {
        NetCounters::default()
    }

    /// Drain any pending console lines produced by TCP listeners.
    fn drain_console_lines(
        &mut self,
        visitor: &mut dyn FnMut(HeaplessString<DEFAULT_LINE_CAPACITY>),
    );

    /// Queue a console line for transmission to remote clients.
    fn send_console_line(&mut self, line: &str);

    /// Request the active TCP console connection to close after flushing responses.
    fn request_disconnect(&mut self) {}

    /// Drain pending net-console connection events (optional).
    fn drain_console_events(&mut self, _visitor: &mut dyn FnMut(NetConsoleEvent)) {}

    /// Return the active TCP console connection identifier, if any.
    fn active_console_conn_id(&self) -> Option<u64> {
        None
    }

    /// Inject a console line into the network transport (testing hook).
    fn inject_console_line(&mut self, _line: &str) {}

    /// Reset the underlying transport (testing hook).
    fn reset(&mut self) {}

    /// Expose the configured TCP console listen port.
    fn console_listen_port(&self) -> u16 {
        CONSOLE_TCP_PORT
    }

    /// Start a network self-test run if supported.
    fn start_self_test(&mut self, _now_ms: u64) -> bool {
        false
    }

    /// Return the current self-test state for diagnostics.
    fn self_test_report(&self) -> NetSelfTestReport {
        NetSelfTestReport::default()
    }
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
        /// Reason for the disconnect.
        reason: NetConsoleDisconnectReason,
        /// Total bytes read from the client during the session.
        bytes_read: u64,
        /// Total bytes written to the client during the session.
        bytes_written: u64,
    },
}

/// Reason for terminating a TCP console session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetConsoleDisconnectReason {
    Quit,
    Eof,
    Reset,
    Error,
}

impl NetConsoleDisconnectReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Eof => "eof",
            Self::Reset => "reset",
            Self::Error => "error",
        }
    }
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
