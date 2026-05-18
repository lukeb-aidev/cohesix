// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Networking subsystem abstractions and configuration for console transports.
// Author: Lukas Bower

//! Networking subsystem abstractions for host and seL4 targets.

#[cfg(all(feature = "kernel", feature = "net-console"))]
use smoltcp::{phy::Device, wire::EthernetAddress};

#[cfg(feature = "net")]
pub mod diag;
#[cfg(feature = "net")]
pub use diag::{NetDiagSnapshot, NET_DIAG, NET_DIAG_FEATURED};

#[cfg(all(feature = "kernel", feature = "net-console"))]
use core::ops::Range;

use crate::observe::IngestSnapshot;
use crate::serial::DEFAULT_LINE_CAPACITY;
#[cfg(feature = "kernel")]
use cohesix_ticket::Role;
#[cfg(feature = "net-console")]
pub mod dhcp;
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
/// Unsafe fallback token used only when generated ticket inventory is unavailable.
const AUTH_TOKEN_FALLBACK: &str = "";
/// Insecure placeholder token rejected by configuration validation.
const INSECURE_PLACEHOLDER_TOKEN: &str = concat!("change", "me");
/// Idle timeout applied to authenticated TCP console sessions (milliseconds).
///
/// When the kernel timer cannot use the architected counter (default in dev-virt),
/// the dummy timebase advances once per poll and runs far faster than wall time.
/// Use an extended timeout in that mode to prevent spurious disconnects.
pub const IDLE_TIMEOUT_MS: u64 = if cfg!(feature = "timers-arch-counter") {
    5 * 60 * 1000
} else {
    24 * 60 * 60 * 1000
};
/// Timeout applied to authentication attempts from newly connected clients.
///
/// See `IDLE_TIMEOUT_MS` for notes on the `dev-virt` dummy timebase running far
/// faster than wall time when the architected counter is unavailable.
pub const AUTH_TIMEOUT_MS: u64 = if cfg!(feature = "timers-arch-counter") {
    5 * 1000
} else {
    10 * 60 * 1000
};

/// Number of inbound console command lines retained between pump cycles.
pub const CONSOLE_QUEUE_DEPTH: usize = 32;
/// Number of non-priority outbound console response lines retained while the
/// transport drains. Sized to hold a full `/log/queen.log` snapshot on Pi 4 WiFi.
pub const CONSOLE_OUTBOUND_QUEUE_DEPTH: usize = 512;
/// Number of priority outbound console lines retained for ACK/ERR/END traffic.
pub const CONSOLE_PRIORITY_QUEUE_DEPTH: usize = 128;

pub(crate) fn cyw43_control_plane_bootstrap_replay_reason(_reason: &str) -> bool {
    // The bounded startup-link / sideband recovery ladder already retries
    // the current first-reply family in HAL and the driver. Replaying the full
    // firmware/bootstrap path here only repeats the same slow failure while
    // hiding the preserved blocker, so keep the bootstrap replay gate closed.
    false
}

#[must_use]
pub(crate) const fn wifi_boot_join_should_defer(interface: NetInterfacePolicy) -> bool {
    matches!(
        interface,
        NetInterfacePolicy::Wifi | NetInterfacePolicy::Auto
    )
}

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

/// Effective network acquisition mode for the active control-plane interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetMode {
    Off,
    Static,
    Dhcp,
}

impl NetMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Static => "static",
            Self::Dhcp => "dhcp",
        }
    }

    #[cfg(feature = "kernel")]
    #[must_use]
    pub const fn from_generated(mode: crate::generated::NetworkMode) -> Self {
        match mode {
            crate::generated::NetworkMode::Off => Self::Off,
            crate::generated::NetworkMode::Static => Self::Static,
            crate::generated::NetworkMode::Dhcp => Self::Dhcp,
        }
    }
}

/// Requested runtime interface policy for Pi 4 networking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetInterfacePolicy {
    Wired,
    Wifi,
    Auto,
}

impl NetInterfacePolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wired => "wired",
            Self::Wifi => "wifi",
            Self::Auto => "auto",
        }
    }

    #[cfg(feature = "kernel")]
    #[must_use]
    pub const fn from_generated(policy: crate::generated::NetworkInterfacePolicy) -> Self {
        match policy {
            crate::generated::NetworkInterfacePolicy::Wired => Self::Wired,
            crate::generated::NetworkInterfacePolicy::Wifi => Self::Wifi,
            crate::generated::NetworkInterfacePolicy::Auto => Self::Auto,
        }
    }
}

/// Deterministic DHCP retry and timeout bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetDhcpConfig {
    pub discover_timeout_ms: u32,
    pub request_timeout_ms: u32,
    pub max_retries: u8,
}

impl NetDhcpConfig {
    #[must_use]
    pub const fn dev_virt() -> Self {
        Self {
            discover_timeout_ms: 1_000,
            request_timeout_ms: 1_000,
            max_retries: 4,
        }
    }
}

/// Manifest-authored network policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetPolicyConfig {
    pub mode: NetMode,
    pub interface: NetInterfacePolicy,
    pub dhcp: NetDhcpConfig,
}

impl NetPolicyConfig {
    #[must_use]
    pub const fn dev_virt() -> Self {
        Self {
            mode: NetMode::Static,
            interface: NetInterfacePolicy::Wired,
            dhcp: NetDhcpConfig::dev_virt(),
        }
    }
}

/// Bounded Wi-Fi credentials carried from boot policy into the Pi 4 runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiCredentials {
    pub ssid_len: u8,
    pub ssid: [u8; 32],
    pub psk_len: u8,
    pub psk: [u8; 64],
}

impl WifiCredentials {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ssid_len: 0,
            ssid: [0; 32],
            psk_len: 0,
            psk: [0; 64],
        }
    }

    pub fn new(ssid: &str, psk: &str) -> Result<Self, &'static str> {
        if ssid.is_empty() {
            return Err("wifi-ssid-empty");
        }
        if ssid.len() > 32 {
            return Err("wifi-ssid-too-long");
        }
        if !ssid.as_bytes().iter().copied().all(wifi_text_byte_valid) {
            return Err("wifi-ssid-invalid");
        }
        if psk.len() > 64 {
            return Err("wifi-psk-too-long");
        }
        if !psk.is_empty() && psk.len() < 8 {
            return Err("wifi-psk-too-short");
        }
        if !psk.is_empty() && !wifi_psk_valid(psk.as_bytes()) {
            return Err("wifi-psk-invalid");
        }

        let mut credentials = Self::empty();
        credentials.ssid_len = u8::try_from(ssid.len()).map_err(|_| "wifi-ssid-too-long")?;
        credentials.psk_len = u8::try_from(psk.len()).map_err(|_| "wifi-psk-too-long")?;
        credentials.ssid[..ssid.len()].copy_from_slice(ssid.as_bytes());
        credentials.psk[..psk.len()].copy_from_slice(psk.as_bytes());
        Ok(credentials)
    }

    #[must_use]
    pub const fn has_ssid(self) -> bool {
        self.ssid_len > 0
    }

    #[must_use]
    pub const fn has_psk(self) -> bool {
        self.psk_len > 0
    }

    pub fn ssid(&self) -> Result<&str, &'static str> {
        core::str::from_utf8(&self.ssid[..usize::from(self.ssid_len)]).map_err(|_| "wifi-ssid-utf8")
    }

    pub fn psk(&self) -> Result<&str, &'static str> {
        core::str::from_utf8(&self.psk[..usize::from(self.psk_len)]).map_err(|_| "wifi-psk-utf8")
    }
}

#[must_use]
const fn wifi_text_byte_valid(byte: u8) -> bool {
    matches!(byte, 0x20..=0x7e)
}

#[must_use]
const fn wifi_hex_byte(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

#[must_use]
fn wifi_psk_valid(bytes: &[u8]) -> bool {
    if bytes.len() == 64 {
        return bytes.iter().copied().all(wifi_hex_byte);
    }
    bytes.iter().copied().all(wifi_text_byte_valid)
}

impl Default for WifiCredentials {
    fn default() -> Self {
        Self::empty()
    }
}

/// Optional runtime override sourced from bounded boot-policy inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeNetPolicyOverride {
    pub mode: Option<NetMode>,
    pub interface: Option<NetInterfacePolicy>,
    pub static_address: Option<NetAddressConfig>,
    pub wifi_credentials: Option<WifiCredentials>,
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
    /// Active NIC backend selected from profile-generated settings.
    pub backend: NetBackend,
    /// Manifest-authored interface and address acquisition policy.
    pub policy: NetPolicyConfig,
    /// IPv4 configuration for the console interface.
    pub address: NetAddressConfig,
    /// Optional Wi-Fi credentials injected by bounded boot policy.
    pub wifi_credentials: Option<WifiCredentials>,
}

impl ConsoleNetConfig {
    /// Construct a configuration using the default constants.
    pub fn default() -> Self {
        Self {
            auth_token: console_auth_token(),
            idle_timeout_ms: IDLE_TIMEOUT_MS,
            listen_port: COHSH_TCP_PORT,
            backend: DEFAULT_NET_BACKEND,
            policy: NetPolicyConfig::dev_virt(),
            address: NetAddressConfig::dev_virt(),
            wifi_credentials: None,
        }
    }

    /// Apply profile-gated networking defaults.
    ///
    /// QEMU backends keep dev-virt fallbacks while Pi4 backends require
    /// manifest-provided static IPv4 fields.
    #[must_use]
    pub fn with_profile_defaults(mut self) -> Self {
        if self.listen_port == 0 {
            self.listen_port = COHESIX_TCP_CONSOLE_PORT;
        }
        if self.backend.uses_dev_virt_defaults() {
            if self.policy.mode == NetMode::Off {
                self.policy.mode = NetMode::Static;
            }
            if self.policy.interface != NetInterfacePolicy::Wired {
                self.policy.interface = NetInterfacePolicy::Wired;
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
        }
        self
    }

    /// Apply a bounded boot-policy override without changing dev-virt defaults.
    #[must_use]
    pub fn with_runtime_policy(mut self, policy: RuntimeNetPolicyOverride) -> Self {
        if self.backend.uses_dev_virt_defaults() {
            return self;
        }
        if let Some(mode) = policy.mode {
            self.policy.mode = mode;
        }
        if let Some(interface) = policy.interface {
            self.policy.interface = interface;
        }
        if let Some(address) = policy.static_address {
            self.address = address;
        }
        if let Some(credentials) = policy.wifi_credentials {
            self.wifi_credentials = Some(credentials);
        }
        self
    }
}

/// Resolve the active TCP console network configuration from generated manifest
/// artifacts with deterministic profile-gated defaults.
#[must_use]
pub fn console_net_config() -> ConsoleNetConfig {
    #[cfg(feature = "kernel")]
    {
        let mut config = ConsoleNetConfig::default();
        let hardware = crate::generated::hardware_config();
        let network = hardware.network;
        config.backend = NetBackend::from_generated(network.backend);
        config.policy = NetPolicyConfig {
            mode: NetMode::from_generated(network.mode),
            interface: NetInterfacePolicy::from_generated(network.interface),
            dhcp: NetDhcpConfig {
                discover_timeout_ms: network.dhcp.discover_timeout_ms,
                request_timeout_ms: network.dhcp.request_timeout_ms,
                max_retries: network.dhcp.max_retries,
            },
        };
        config.address = NetAddressConfig {
            ip: network.static_ipv4.ip,
            prefix_len: network.static_ipv4.prefix_len,
            gateway: network.static_ipv4.gateway,
        };
        return config.with_profile_defaults();
    }
    #[cfg(not(feature = "kernel"))]
    {
        ConsoleNetConfig::default().with_profile_defaults()
    }
}

/// Resolve the active TCP console config with an optional boot-policy override.
#[must_use]
pub fn console_net_config_with_runtime_policy(
    policy: RuntimeNetPolicyOverride,
) -> ConsoleNetConfig {
    console_net_config().with_runtime_policy(policy)
}

/// Resolve the TCP console authentication token from generated manifest data.
#[must_use]
pub fn console_auth_token() -> &'static str {
    #[cfg(feature = "kernel")]
    {
        for ticket in crate::generated::ticket_inventory() {
            if ticket.role == Role::Queen {
                return ticket.secret;
            }
        }
        AUTH_TOKEN_FALLBACK
    }
    #[cfg(not(feature = "kernel"))]
    {
        "test-console-token"
    }
}

/// Validate that the configured TCP console authentication token is usable.
pub fn validate_console_auth_token(token: &str) -> Result<(), &'static str> {
    if token.trim().is_empty() {
        return Err("console auth token must be configured");
    }
    if token.trim() == INSECURE_PLACEHOLDER_TOKEN {
        return Err("console auth token must not use insecure placeholder");
    }
    Ok(())
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

/// Console line captured from the TCP listener with an ingest timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// Raw line text (without trailing newline).
    pub text: HeaplessString<DEFAULT_LINE_CAPACITY>,
    /// Monotonic ingest timestamp in milliseconds.
    pub ingest_ms: u64,
}

impl ConsoleLine {
    /// Construct a console line with the supplied ingest timestamp.
    #[must_use]
    pub fn new(text: HeaplessString<DEFAULT_LINE_CAPACITY>, ingest_ms: u64) -> Self {
        Self { text, ingest_ms }
    }
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
    /// Wi-Fi association state, encoded as 0 or 1 for compact diagnostics.
    pub wifi_assoc: u64,
    /// Wi-Fi link state, encoded as 0 or 1 for compact diagnostics.
    pub wifi_link_up: u64,
    /// Host-EAPOL frames received by the Wi-Fi driver.
    pub wifi_host_eapol_rx: u64,
    /// EAPOL-Start frames sent by the Wi-Fi driver.
    pub wifi_host_eapol_start: u64,
    /// Host-EAPOL secure completion state, encoded as 0 or 1.
    pub wifi_host_eapol_secure: u64,
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
    /// Authenticated TCP console sessions observed.
    pub tcp_auth_sessions: u64,
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
    /// Wi-Fi association state, encoded as 0 or 1 for compact diagnostics.
    pub wifi_assoc: u64,
    /// Wi-Fi link state, encoded as 0 or 1 for compact diagnostics.
    pub wifi_link_up: u64,
    /// Host-EAPOL frames received by the Wi-Fi driver.
    pub wifi_host_eapol_rx: u64,
    /// EAPOL-Start frames sent by the Wi-Fi driver.
    pub wifi_host_eapol_start: u64,
    /// Host-EAPOL secure completion state, encoded as 0 or 1.
    pub wifi_host_eapol_secure: u64,
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
    /// Whether local driver/DHCP/console proof succeeded while optional peer
    /// echo/smoke helpers were not present.
    pub peer_assisted_ok: bool,
}

/// Outcome when attempting to start a network self-test run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetSelfTestStartResult {
    Started,
    Unsupported,
    PolicyDisabled,
    SelfTestDisabled,
    DhcpPending,
    WifiAssociating,
    WifiHostEapolPending,
    WifiHostEapolRequired,
    WifiAssociationFailed,
    WifiLinkDown,
    NotReadyRootEp,
    NotReadyIpcBuffer,
    NotReadyCspaceWindow,
    NotReadyBootstrapCommit,
}

impl NetSelfTestStartResult {
    #[must_use]
    pub const fn refusal_detail(self) -> Option<&'static str> {
        match self {
            Self::Started => None,
            Self::Unsupported => Some("detail=unsupported"),
            Self::PolicyDisabled => Some("detail=policy-disabled"),
            Self::SelfTestDisabled => Some("detail=selftest-disabled"),
            Self::DhcpPending => Some("detail=dhcp-pending"),
            Self::WifiAssociating => Some("detail=wifi-associating"),
            Self::WifiHostEapolPending => Some("detail=wifi-host-eapol-pending"),
            Self::WifiHostEapolRequired => Some("detail=wifi-host-eapol-required"),
            Self::WifiAssociationFailed => Some("detail=wifi-association-failed"),
            Self::WifiLinkDown => Some("detail=wifi-link-down"),
            Self::NotReadyRootEp => Some("detail=not-ready:root-ep"),
            Self::NotReadyIpcBuffer => Some("detail=not-ready:ipc-buffer"),
            Self::NotReadyCspaceWindow => Some("detail=not-ready:cspace-window"),
            Self::NotReadyBootstrapCommit => Some("detail=not-ready:bootstrap-commit"),
        }
    }

    #[must_use]
    pub fn from_readiness_reason(reason: &'static str) -> Self {
        match reason {
            "root-ep" => Self::NotReadyRootEp,
            "ipc-buffer" => Self::NotReadyIpcBuffer,
            "cspace-window" => Self::NotReadyCspaceWindow,
            "bootstrap-commit" => Self::NotReadyBootstrapCommit,
            _ => Self::Unsupported,
        }
    }

    #[must_use]
    pub fn from_bringup_status(status: &'static str) -> Option<Self> {
        match status.as_bytes() {
            b"wifi-associating" => Some(Self::WifiAssociating),
            b"wifi-host-eapol-pending" => Some(Self::WifiHostEapolPending),
            b"wifi-host-eapol-required" => Some(Self::WifiHostEapolRequired),
            b"wifi-association-failed" => Some(Self::WifiAssociationFailed),
            b"wifi-link-down" => Some(Self::WifiLinkDown),
            _ => None,
        }
    }
}

/// Summary of the self-test subsystem for consoles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetSelfTestReport {
    /// Indicates whether self-test support is compiled in for the current build.
    pub enabled: bool,
    /// True while a self-test run is active.
    pub running: bool,
    /// Last recorded result, if any.
    pub last_result: Option<NetSelfTestResult>,
    /// Active backend label.
    pub backend: &'static str,
    /// Primary host-visible UDP test target.
    pub udp_target: HeaplessString<48>,
    /// Primary host-visible TCP test target.
    pub tcp_target: HeaplessString<48>,
}

impl Default for NetSelfTestReport {
    fn default() -> Self {
        Self {
            enabled: false,
            running: false,
            last_result: None,
            backend: "disabled",
            udp_target: HeaplessString::new(),
            tcp_target: HeaplessString::new(),
        }
    }
}

/// Summary of the active network policy and address state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetStatusReport {
    pub backend: &'static str,
    pub mode: &'static str,
    pub interface_policy: &'static str,
    pub active_interface: &'static str,
    pub standby_interface: &'static str,
    pub address_source: &'static str,
    pub ip: HeaplessString<32>,
    pub gateway: HeaplessString<32>,
    pub dhcp_phase: &'static str,
}

impl Default for NetStatusReport {
    fn default() -> Self {
        Self {
            backend: "disabled",
            mode: "off",
            interface_policy: "wired",
            active_interface: "none",
            standby_interface: "none",
            address_source: "disabled",
            ip: HeaplessString::new(),
            gateway: HeaplessString::new(),
            dhcp_phase: "disabled",
        }
    }
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
    fn create_with_stage<H>(
        hal: &mut H,
        _config: &ConsoleNetConfig,
        _stage: NetStage,
    ) -> Result<Self, Self::Error>
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

    /// Active interface label surfaced through diagnostics.
    fn interface_label(&self) -> &'static str {
        "wired"
    }

    /// Optional runtime bring-up status surfaced before the device can carry traffic.
    fn bringup_status_label(&self) -> Option<&'static str> {
        None
    }

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
#[cfg(feature = "net-console")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetBackend {
    /// RTL8139 PCI NIC exposed by QEMU `virt`.
    Rtl8139,
    /// Broadcom GENETv5 backend used on Raspberry Pi 4.
    BcmGenet,
    /// Virtio MMIO NIC (kept for experiments and debugging).
    #[cfg(feature = "net-backend-virtio")]
    Virtio,
}

#[cfg(feature = "net-console")]
impl NetBackend {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rtl8139 => "rtl8139",
            Self::BcmGenet => "bcmgenet-v5",
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio => "virtio-net",
        }
    }

    #[must_use]
    pub const fn uses_dev_virt_defaults(self) -> bool {
        match self {
            Self::Rtl8139 => true,
            Self::BcmGenet => false,
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio => true,
        }
    }

    #[must_use]
    pub const fn supports_interface_policy(self, policy: NetInterfacePolicy) -> bool {
        match self {
            Self::Rtl8139 => matches!(policy, NetInterfacePolicy::Wired),
            Self::BcmGenet => matches!(
                policy,
                NetInterfacePolicy::Wired | NetInterfacePolicy::Wifi | NetInterfacePolicy::Auto
            ),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio => matches!(policy, NetInterfacePolicy::Wired),
        }
    }

    #[cfg(feature = "kernel")]
    #[must_use]
    pub fn from_generated(backend: crate::generated::NetworkBackendKind) -> Self {
        match backend {
            crate::generated::NetworkBackendKind::Auto => DEFAULT_NET_BACKEND,
            crate::generated::NetworkBackendKind::Rtl8139 => Self::Rtl8139,
            crate::generated::NetworkBackendKind::VirtioNet => {
                #[cfg(feature = "net-backend-virtio")]
                {
                    Self::Virtio
                }
                #[cfg(not(feature = "net-backend-virtio"))]
                {
                    Self::Rtl8139
                }
            }
            crate::generated::NetworkBackendKind::BcmGenetV5 => Self::BcmGenet,
        }
    }
}

/// Default NIC backend used for developer QEMU runs.
#[cfg(all(feature = "net-console", not(feature = "net-backend-virtio")))]
pub const DEFAULT_NET_BACKEND: NetBackend = NetBackend::Rtl8139;

/// Experimental virtio-net backend used only when explicitly selected.
#[cfg(all(feature = "net-console", feature = "net-backend-virtio"))]
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
    fn drain_console_lines(&mut self, now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine));

    /// Queue a console line for transmission to remote clients.
    fn send_console_line(&mut self, line: &str) -> bool;

    /// Request the active TCP console connection to close after flushing responses.
    fn request_disconnect(&mut self) {}

    /// Drain pending net-console connection events (optional).
    fn drain_console_events(&mut self, _visitor: &mut dyn FnMut(NetConsoleEvent)) {}

    /// Snapshot ingest metrics for observability providers.
    fn ingest_snapshot(&self) -> IngestSnapshot {
        IngestSnapshot::default()
    }

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
    fn start_self_test(&mut self, _now_ms: u64) -> NetSelfTestStartResult {
        NetSelfTestStartResult::Unsupported
    }

    /// Return the current self-test state for diagnostics.
    fn self_test_report(&self) -> NetSelfTestReport {
        NetSelfTestReport::default()
    }

    /// Return the active network policy and address state for diagnostics.
    fn status_report(&self) -> NetStatusReport {
        NetStatusReport::default()
    }
}

/// Connection lifecycle notifications surfaced by TCP console transports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetConsoleEvent {
    /// A TCP console client reached TCP Established and is waiting for auth.
    Connected {
        /// Unique connection identifier assigned by the stack.
        conn_id: u64,
        /// Peer address (if known).
        peer: Option<heapless::String<32>>,
    },
    /// A TCP console client completed Cohesix authentication.
    Authenticated {
        /// Unique connection identifier assigned by the stack.
        conn_id: u64,
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
    fn auth_timeout_scales_with_timebase() {
        if cfg!(feature = "timers-arch-counter") {
            assert_eq!(AUTH_TIMEOUT_MS, 5_000);
        } else {
            // In dev-virt the dummy timer advances once per poll; enforce a large enough
            // auth deadline to avoid spurious WAN/tunnel authentication failures.
            assert!(AUTH_TIMEOUT_MS >= 10 * 60 * 1000);
        }
    }

    #[test]
    fn default_net_config_uses_console_port() {
        let config = ConsoleNetConfig::default();

        assert_eq!(config.listen_port, COHSH_TCP_PORT);
        assert_ne!(config.listen_port, 0);
        assert_eq!(config.policy.mode, NetMode::Static);
        assert_eq!(config.policy.interface, NetInterfacePolicy::Wired);
        assert_eq!(config.address.ip, DEV_VIRT_IP);
        assert_eq!(config.address.prefix_len, DEV_VIRT_PREFIX);
        assert_eq!(config.address.gateway, Some(DEV_VIRT_GATEWAY));
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn bcmgenet_profile_does_not_force_dev_virt_defaults() {
        let config = ConsoleNetConfig {
            auth_token: "token",
            idle_timeout_ms: IDLE_TIMEOUT_MS,
            listen_port: COHSH_TCP_PORT,
            backend: NetBackend::BcmGenet,
            policy: NetPolicyConfig {
                mode: NetMode::Dhcp,
                interface: NetInterfacePolicy::Wired,
                dhcp: NetDhcpConfig::dev_virt(),
            },
            address: NetAddressConfig {
                ip: [192, 168, 10, 42],
                prefix_len: 24,
                gateway: Some([192, 168, 10, 1]),
            },
            wifi_credentials: None,
        };
        let resolved = config.with_profile_defaults();
        assert_eq!(resolved.address.ip, [192, 168, 10, 42]);
        assert_eq!(resolved.address.prefix_len, 24);
        assert_eq!(resolved.address.gateway, Some([192, 168, 10, 1]));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn backend_supports_pi4_wifi_policies() {
        assert!(NetBackend::Rtl8139.supports_interface_policy(NetInterfacePolicy::Wired));
        assert!(!NetBackend::Rtl8139.supports_interface_policy(NetInterfacePolicy::Wifi));
        assert!(NetBackend::BcmGenet.supports_interface_policy(NetInterfacePolicy::Wifi));
        assert!(NetBackend::BcmGenet.supports_interface_policy(NetInterfacePolicy::Auto));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn cyw43_control_plane_bootstrap_replay_reason_stays_closed_for_first_reply_failures() {
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-control-plane-pure-f2-startup-link-no-reply"
        ));
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-control-plane-sideband-read-stall-no-buffer-ready"
        ));
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
        ));
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-control-plane-no-reply-linux-f2-armed"
        ));
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-control-plane-hintless-firstread-no-irq"
        ));
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-control-plane-startup-link-reply-timeout"
        ));
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-control-plane-passive-startup-link-timeout"
        ));
        assert!(!cyw43_control_plane_bootstrap_replay_reason(
            "cyw43-control-plane-startup-link-rescue-budget-exhausted"
        ));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn runtime_policy_override_changes_pi4_mode_and_interface() {
        let config = ConsoleNetConfig {
            auth_token: "token",
            idle_timeout_ms: IDLE_TIMEOUT_MS,
            listen_port: COHSH_TCP_PORT,
            backend: NetBackend::BcmGenet,
            policy: NetPolicyConfig {
                mode: NetMode::Static,
                interface: NetInterfacePolicy::Wired,
                dhcp: NetDhcpConfig::dev_virt(),
            },
            address: NetAddressConfig {
                ip: [192, 168, 10, 42],
                prefix_len: 24,
                gateway: Some([192, 168, 10, 1]),
            },
            wifi_credentials: None,
        };
        let resolved = config.with_runtime_policy(RuntimeNetPolicyOverride {
            mode: Some(NetMode::Dhcp),
            interface: Some(NetInterfacePolicy::Auto),
            static_address: None,
            wifi_credentials: Some(
                WifiCredentials::new("cohesix", "passphrase").expect("valid wifi credentials"),
            ),
        });
        assert_eq!(resolved.policy.mode, NetMode::Dhcp);
        assert_eq!(resolved.policy.interface, NetInterfacePolicy::Auto);
        assert_eq!(resolved.address.ip, [192, 168, 10, 42]);
        assert_eq!(
            resolved
                .wifi_credentials
                .expect("runtime credentials present")
                .ssid()
                .expect("ssid utf8"),
            "cohesix"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn runtime_policy_override_does_not_change_dev_virt_defaults() {
        let resolved = ConsoleNetConfig::default().with_runtime_policy(RuntimeNetPolicyOverride {
            mode: Some(NetMode::Off),
            interface: Some(NetInterfacePolicy::Wifi),
            static_address: Some(NetAddressConfig {
                ip: [10, 1, 2, 3],
                prefix_len: 24,
                gateway: Some([10, 1, 2, 1]),
            }),
            wifi_credentials: Some(
                WifiCredentials::new("cohesix", "passphrase").expect("valid wifi credentials"),
            ),
        });
        assert_eq!(resolved.policy.mode, NetMode::Static);
        assert_eq!(resolved.policy.interface, NetInterfacePolicy::Wired);
        assert_eq!(resolved.address.ip, DEV_VIRT_IP);
        assert!(resolved.wifi_credentials.is_none());
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn runtime_policy_override_changes_pi4_static_address() {
        let config = ConsoleNetConfig {
            auth_token: "token",
            idle_timeout_ms: IDLE_TIMEOUT_MS,
            listen_port: COHSH_TCP_PORT,
            backend: NetBackend::BcmGenet,
            policy: NetPolicyConfig {
                mode: NetMode::Static,
                interface: NetInterfacePolicy::Wired,
                dhcp: NetDhcpConfig::dev_virt(),
            },
            address: NetAddressConfig {
                ip: [192, 168, 10, 42],
                prefix_len: 24,
                gateway: Some([192, 168, 10, 1]),
            },
            wifi_credentials: None,
        };
        let resolved = config.with_runtime_policy(RuntimeNetPolicyOverride {
            mode: Some(NetMode::Static),
            interface: Some(NetInterfacePolicy::Wifi),
            static_address: Some(NetAddressConfig {
                ip: [10, 20, 30, 40],
                prefix_len: 25,
                gateway: Some([10, 20, 30, 1]),
            }),
            wifi_credentials: Some(
                WifiCredentials::new("cohesix", "passphrase").expect("valid wifi credentials"),
            ),
        });
        assert_eq!(resolved.policy.mode, NetMode::Static);
        assert_eq!(resolved.policy.interface, NetInterfacePolicy::Wifi);
        assert_eq!(resolved.address.ip, [10, 20, 30, 40]);
        assert_eq!(resolved.address.prefix_len, 25);
        assert_eq!(resolved.address.gateway, Some([10, 20, 30, 1]));
    }

    #[test]
    fn wifi_credentials_enforce_bounds() {
        assert!(WifiCredentials::new("", "").is_err());
        assert!(WifiCredentials::new("ssid", "short").is_err());
        assert!(WifiCredentials::new("ssid", "12345678").is_ok());
        assert!(WifiCredentials::new("open-network", "").is_ok());
        assert!(WifiCredentials::new("ssid with spaces", "printable passphrase").is_ok());
        assert!(WifiCredentials::new("ssid\n", "12345678").is_err());
        assert!(WifiCredentials::new("ssid", "pass\nword").is_err());
        assert!(WifiCredentials::new(
            "ssid",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        )
        .is_ok());
        assert!(WifiCredentials::new(
            "ssid",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        )
        .is_err());
    }

    #[test]
    fn wifi_join_deferral_applies_to_boot_wifi_paths() {
        assert!(wifi_boot_join_should_defer(NetInterfacePolicy::Wifi));
        assert!(wifi_boot_join_should_defer(NetInterfacePolicy::Auto));
        assert!(!wifi_boot_join_should_defer(NetInterfacePolicy::Wired));
    }
}
