// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Networking subsystem abstractions and configuration for console transports.
// Author: Lukas Bower

//! Networking subsystem abstractions for host and seL4 targets.

#[cfg(all(feature = "kernel", feature = "net-console"))]
use smoltcp::{
    phy::Device,
    wire::{EthernetAddress, Ipv4Address},
};

#[cfg(feature = "net")]
pub mod diag;
#[cfg(feature = "net")]
pub use diag::{NetDiagSnapshot, NET_DIAG, NET_DIAG_FEATURED};

#[cfg(all(feature = "kernel", feature = "net-console"))]
use core::ops::Range;

#[cfg(feature = "net-console")]
use crate::hal::driver_task::{DriverServiceBudget, DriverServiceBudgetError};
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

/// Number of regular outbound console lines retained between pump cycles.
pub const CONSOLE_QUEUE_DEPTH: usize = 16;
/// Number of authenticated inbound console commands retained between pump cycles.
pub const CONSOLE_INGEST_QUEUE_DEPTH: usize = 32;
/// Maximum inbound console commands dispatched from the event pump in one turn.
pub const CONSOLE_DISPATCH_BURST: usize = 8;

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
    /// ARP frames consumed from the NIC by the stack.
    pub arp_rx: u64,
    /// ARP frames submitted by the stack to the NIC.
    pub arp_tx: u64,
    /// Wi-Fi ARP requests normalized from broadcast target hardware to zero.
    pub wifi_arp_target_hw_zeroed: u64,
    /// Post-DHCP CYW43 data-channel RX frames observed.
    pub wifi_post_dhcp_rx_any: u64,
    /// Post-DHCP CYW43 unicast RX frames addressed to the station MAC.
    pub wifi_post_dhcp_rx_unicast: u64,
    /// Post-DHCP CYW43 ARP RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_arp: u64,
    /// Post-DHCP CYW43 IPv4 RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_ipv4: u64,
    /// Post-DHCP CYW43 ICMP RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_icmp: u64,
    /// Post-DHCP CYW43 TCP console or smoke/proof RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_tcp: u64,
    /// Last post-DHCP CYW43 RX frame length.
    pub wifi_post_dhcp_rx_last_len: u64,
    /// Last post-DHCP CYW43 RX Ethernet type.
    pub wifi_post_dhcp_rx_last_ethertype: u64,
    /// Last hardware RX frame length reported by the driver task.
    pub driver_rx_last_len: u64,
    /// Last hardware RX Ethernet type reported by the driver task.
    pub driver_rx_last_ethertype: u64,
    /// GENET linked-runtime RX queue depth from the latest completion.
    pub genet_rx_runtime_queue_count: u64,
    /// GENET linked-runtime RX queue high-water mark.
    pub genet_rx_runtime_queue_high_water: u64,
    /// GENET linked-runtime RX overflow flag, encoded as 0 or 1.
    pub genet_rx_runtime_queue_overflow_seen: u64,
    /// GENET linked-runtime RX drain-budget-hit flag, encoded as 0 or 1.
    pub genet_rx_runtime_drain_budget_hit: u64,
    /// GENET linked-runtime RX byte-budget-hit flag, encoded as 0 or 1.
    pub genet_rx_runtime_byte_budget_hit: u64,
    /// GENET linked-runtime max frames drained during one service turn.
    pub genet_rx_runtime_max_drained_per_turn: u64,
    /// Root preserved Genet RX queue depth.
    pub genet_rx_pending_queue_count: u64,
    /// Root preserved Genet RX queue high-water mark.
    pub genet_rx_pending_queue_high_water: u64,
    /// Root preserved Genet RX queue drops.
    pub genet_rx_pending_drops: u64,
    /// Root preserved CYW43 RX queue depth.
    pub wifi_rx_pending_queue_count: u64,
    /// Root preserved CYW43 RX queue high-water mark.
    pub wifi_rx_pending_queue_high_water: u64,
    /// Root preserved CYW43 RX queue drops.
    pub wifi_rx_pending_drops: u64,
    /// CYW43 linked-runtime RX queue depth from the latest idle trace.
    pub wifi_rx_runtime_queue_count: u64,
    /// CYW43 linked-runtime RX queue high-water mark.
    pub wifi_rx_runtime_queue_high_water: u64,
    /// CYW43 linked-runtime RX overflow flag, encoded as 0 or 1.
    pub wifi_rx_runtime_queue_overflow_seen: u64,
    /// Root-observed CYW43 linked-runtime RX overflow episodes.
    pub wifi_rx_runtime_overflow_episodes: u64,
    /// CYW43 linked-runtime RX drain-budget-hit flag, encoded as 0 or 1.
    pub wifi_rx_runtime_drain_budget_hit: u64,
    /// CYW43 linked-runtime max frames drained during one service turn.
    pub wifi_rx_runtime_max_drained_per_turn: u64,
    /// CYW43 linked-runtime last service operation.
    pub wifi_service_last_op: u64,
    /// CYW43 linked-runtime last service reason.
    pub wifi_service_last_reason: u64,
    /// CYW43 linked-runtime last service progress bits or byte count.
    pub wifi_service_last_progress: u64,
    /// CYW43 linked-runtime last SDPCM sequence/credit window.
    pub wifi_service_last_seq_window: u64,
    /// CYW43 linked-runtime last serviced channel, or 0xffff when none.
    pub wifi_service_last_channel: u64,
    /// CYW43 linked-runtime credit observations at the last service trace.
    pub wifi_service_last_credit_observations: u64,
    /// CYW43 linked-runtime last RFRAME length sampled in the service trace.
    pub wifi_service_last_rframe_len: u64,
    /// CYW43 linked-runtime last RX source flags.
    pub wifi_service_last_source_flags: u64,
    /// CYW43 linked-runtime last pre-service RX source result.
    pub wifi_service_last_pre_source: u64,
    /// CYW43 linked-runtime last post-service RX source result.
    pub wifi_service_last_post_source: u64,
    /// Sampled CYW43 data-path fault trace count.
    pub wifi_data_trace_faults: u64,
    /// Sampled CYW43 data-path TX retry trace count.
    pub wifi_data_trace_tx_retries: u64,
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
    /// Host-EAPOL M1 frames received.
    pub wifi_host_eapol_m1: u64,
    /// Host-EAPOL M2 frames transmitted.
    pub wifi_host_eapol_m2: u64,
    /// Host-EAPOL M3 frames received.
    pub wifi_host_eapol_m3: u64,
    /// Host-EAPOL M4 frames transmitted.
    pub wifi_host_eapol_m4: u64,
    /// Pairwise key install completions.
    pub wifi_host_eapol_ptk: u64,
    /// Group key install completions.
    pub wifi_host_eapol_gtk: u64,
}

/// NIC and smoltcp counters, either boot-cumulative or generation-projected as documented.
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
    /// TCP console receive-ready turns observed.
    pub tcp_console_recv_ready: u64,
    /// TCP console receive turns that exhausted the bounded receive budget.
    pub tcp_console_recv_budget_hits: u64,
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
    /// ARP frames consumed from the NIC by the stack.
    pub arp_rx: u64,
    /// ARP frames submitted by the stack to the NIC.
    pub arp_tx: u64,
    /// Wi-Fi ARP requests normalized from broadcast target hardware to zero.
    pub wifi_arp_target_hw_zeroed: u64,
    /// Post-DHCP CYW43 data-channel RX frames observed.
    pub wifi_post_dhcp_rx_any: u64,
    /// Post-DHCP CYW43 unicast RX frames addressed to the station MAC.
    pub wifi_post_dhcp_rx_unicast: u64,
    /// Post-DHCP CYW43 ARP RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_arp: u64,
    /// Post-DHCP CYW43 IPv4 RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_ipv4: u64,
    /// Post-DHCP CYW43 ICMP RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_icmp: u64,
    /// Post-DHCP CYW43 TCP console or smoke/proof RX frames for the assigned IPv4 address.
    pub wifi_post_dhcp_rx_tcp: u64,
    /// Last post-DHCP CYW43 RX frame length.
    pub wifi_post_dhcp_rx_last_len: u64,
    /// Last post-DHCP CYW43 RX Ethernet type.
    pub wifi_post_dhcp_rx_last_ethertype: u64,
    /// Last hardware RX frame length reported by the driver task.
    pub driver_rx_last_len: u64,
    /// Last hardware RX Ethernet type reported by the driver task.
    pub driver_rx_last_ethertype: u64,
    /// GENET linked-runtime RX queue depth from the latest completion.
    pub genet_rx_runtime_queue_count: u64,
    /// GENET linked-runtime RX queue high-water mark.
    pub genet_rx_runtime_queue_high_water: u64,
    /// GENET linked-runtime RX overflow flag, encoded as 0 or 1.
    pub genet_rx_runtime_queue_overflow_seen: u64,
    /// GENET linked-runtime RX drain-budget-hit flag, encoded as 0 or 1.
    pub genet_rx_runtime_drain_budget_hit: u64,
    /// GENET linked-runtime RX byte-budget-hit flag, encoded as 0 or 1.
    pub genet_rx_runtime_byte_budget_hit: u64,
    /// GENET linked-runtime max frames drained during one service turn.
    pub genet_rx_runtime_max_drained_per_turn: u64,
    /// Root preserved Genet RX queue depth.
    pub genet_rx_pending_queue_count: u64,
    /// Root preserved Genet RX queue high-water mark.
    pub genet_rx_pending_queue_high_water: u64,
    /// Root preserved Genet RX queue drops.
    pub genet_rx_pending_drops: u64,
    /// Root preserved CYW43 RX queue depth.
    pub wifi_rx_pending_queue_count: u64,
    /// Root preserved CYW43 RX queue high-water mark.
    pub wifi_rx_pending_queue_high_water: u64,
    /// Root preserved CYW43 RX queue drops.
    pub wifi_rx_pending_drops: u64,
    /// Boot-cumulative root preserved CYW43 RX queue drops.
    pub wifi_rx_pending_drops_boot: u64,
    /// CYW43 linked-runtime RX queue depth from the latest idle trace.
    pub wifi_rx_runtime_queue_count: u64,
    /// CYW43 linked-runtime RX queue high-water mark.
    pub wifi_rx_runtime_queue_high_water: u64,
    /// CYW43 linked-runtime RX overflow flag, encoded as 0 or 1.
    pub wifi_rx_runtime_queue_overflow_seen: u64,
    /// Current-generation CYW43 linked-runtime RX overflow episodes.
    pub wifi_rx_runtime_overflow_episodes: u64,
    /// Boot-cumulative CYW43 linked-runtime RX overflow episodes.
    pub wifi_rx_runtime_overflow_episodes_boot: u64,
    /// CYW43 linked-runtime RX drain-budget-hit flag, encoded as 0 or 1.
    pub wifi_rx_runtime_drain_budget_hit: u64,
    /// CYW43 linked-runtime max frames drained during one service turn.
    pub wifi_rx_runtime_max_drained_per_turn: u64,
    /// CYW43 linked-runtime last service operation.
    pub wifi_service_last_op: u64,
    /// CYW43 linked-runtime last service reason.
    pub wifi_service_last_reason: u64,
    /// CYW43 linked-runtime last service progress bits or byte count.
    pub wifi_service_last_progress: u64,
    /// CYW43 linked-runtime last SDPCM sequence/credit window.
    pub wifi_service_last_seq_window: u64,
    /// CYW43 linked-runtime last serviced channel, or 0xffff when none.
    pub wifi_service_last_channel: u64,
    /// CYW43 linked-runtime credit observations at the last service trace.
    pub wifi_service_last_credit_observations: u64,
    /// CYW43 linked-runtime last RFRAME length sampled in the service trace.
    pub wifi_service_last_rframe_len: u64,
    /// CYW43 linked-runtime last RX source flags.
    pub wifi_service_last_source_flags: u64,
    /// CYW43 linked-runtime last pre-service RX source result.
    pub wifi_service_last_pre_source: u64,
    /// CYW43 linked-runtime last post-service RX source result.
    pub wifi_service_last_post_source: u64,
    /// Sampled CYW43 data-path fault trace count.
    pub wifi_data_trace_faults: u64,
    /// Sampled CYW43 data-path TX retry trace count.
    pub wifi_data_trace_tx_retries: u64,
    /// TX publish attempts blocked because the descriptor length was zero.
    pub dropped_zero_len_tx: u64,
    /// Wi-Fi association state, encoded as 0 or 1 for compact diagnostics.
    pub wifi_assoc: u64,
    /// Current CYW43 connection generation.
    pub wifi_connection_generation: u64,
    /// Wi-Fi link state, encoded as 0 or 1 for compact diagnostics.
    pub wifi_link_up: u64,
    /// Host-EAPOL frames received by the Wi-Fi driver.
    pub wifi_host_eapol_rx: u64,
    /// EAPOL-Start frames sent by the Wi-Fi driver.
    pub wifi_host_eapol_start: u64,
    /// Host-EAPOL secure completion state, encoded as 0 or 1.
    pub wifi_host_eapol_secure: u64,
    /// Host-EAPOL M1 frames received.
    pub wifi_host_eapol_m1: u64,
    /// Host-EAPOL M2 frames transmitted.
    pub wifi_host_eapol_m2: u64,
    /// Host-EAPOL M3 frames received.
    pub wifi_host_eapol_m3: u64,
    /// Host-EAPOL M4 frames transmitted.
    pub wifi_host_eapol_m4: u64,
    /// Pairwise key install completions.
    pub wifi_host_eapol_ptk: u64,
    /// Group key install completions.
    pub wifi_host_eapol_gtk: u64,
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

impl NetSelfTestResult {
    /// Return the stable console verdict for this terminal result.
    #[must_use]
    pub const fn verdict(self) -> &'static str {
        if self.tx_ok && self.udp_echo_ok && self.tcp_ok && self.console_ok {
            "pass"
        } else if self.peer_assisted_ok {
            "peer-assisted-pass"
        } else {
            "fail"
        }
    }
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
    /// Immutable identifier assigned when the current or last run was admitted.
    pub run_generation: u64,
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
            run_generation: 0,
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
    /// Backend selected by the resolved manifest profile.
    pub profile_backend: &'static str,
    /// Compatibility alias for the active physical or virtual driver.
    pub backend: &'static str,
    /// Active physical or virtual driver after interface-policy selection.
    pub active_driver: &'static str,
    pub mode: &'static str,
    pub interface_policy: &'static str,
    pub active_interface: &'static str,
    pub standby_interface: &'static str,
    pub address_source: &'static str,
    pub ip: HeaplessString<32>,
    pub gateway: HeaplessString<32>,
    pub dhcp_phase: &'static str,
    pub tcp_ready: bool,
}

impl Default for NetStatusReport {
    fn default() -> Self {
        Self {
            profile_backend: "disabled",
            backend: "disabled",
            active_driver: "disabled",
            mode: "off",
            interface_policy: "wired",
            active_interface: "none",
            standby_interface: "none",
            address_source: "disabled",
            ip: HeaplessString::new(),
            gateway: HeaplessString::new(),
            dhcp_phase: "disabled",
            tcp_ready: false,
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

    /// Notify the driver client that the stack has a configured IPv4 address.
    fn set_assigned_ipv4(&mut self, _ip: Ipv4Address) {}

    /// Begin one exact smoltcp copied-RX to immediate-egress transaction.
    ///
    /// Most devices need no extra state. A device with a receive-coupled TX
    /// permit may retain it only until the matching end hook.
    fn begin_smoltcp_rx_transaction(&mut self) {}

    /// Revoke and release any receive-coupled TX permit not consumed by the
    /// immediate egress paired with the current ingress packet.
    fn end_smoltcp_rx_transaction(&mut self) {}

    /// Total TX drops recorded by the driver.
    fn tx_drop_count(&self) -> u32;

    /// Human-readable label for diagnostics.
    fn name() -> &'static str
    where
        Self: Sized;

    /// HAL-enforced scheduling contract required before this device is serviced.
    fn driver_task_contract() -> crate::hal::driver_task::DriverTaskContract
    where
        Self: Sized;

    /// Whether this device is only a root-side client for an isolated
    /// driver-task runtime boundary. This is not owner-state proof by itself:
    /// the runtime must return hardware-progress completions and publish
    /// owner-state descriptors before it can be counted as driver-owned.
    fn driver_task_runtime_client() -> bool
    where
        Self: Sized,
    {
        false
    }

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

#[cfg(feature = "net-console")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IcmpEchoRequestError {
    MalformedIpv4,
    NonLocalDestination,
    InvalidSource,
    WrongProtocol,
    MalformedIcmp,
    NotEchoRequest,
    ReplyBufferTooSmall,
}

#[cfg(feature = "net-console")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct IcmpEchoRequest<'a> {
    source: smoltcp::wire::Ipv4Address,
    ident: u16,
    sequence: u16,
    payload: &'a [u8],
}

#[cfg(feature = "net-console")]
impl IcmpEchoRequest<'_> {
    pub(super) fn reply_len(&self) -> usize {
        smoltcp::wire::Ipv4Repr {
            src_addr: smoltcp::wire::Ipv4Address::UNSPECIFIED,
            dst_addr: self.source,
            next_header: smoltcp::wire::IpProtocol::Icmp,
            payload_len: smoltcp::wire::Icmpv4Repr::EchoReply {
                ident: self.ident,
                seq_no: self.sequence,
                data: self.payload,
            }
            .buffer_len(),
            hop_limit: 64,
        }
        .buffer_len()
            + smoltcp::wire::Icmpv4Repr::EchoReply {
                ident: self.ident,
                seq_no: self.sequence,
                data: self.payload,
            }
            .buffer_len()
    }

    pub(super) fn emit_reply(
        &self,
        local_ip: smoltcp::wire::Ipv4Address,
        output: &mut [u8],
    ) -> Result<usize, IcmpEchoRequestError> {
        let icmp_repr = smoltcp::wire::Icmpv4Repr::EchoReply {
            ident: self.ident,
            seq_no: self.sequence,
            data: self.payload,
        };
        let ipv4_repr = smoltcp::wire::Ipv4Repr {
            src_addr: local_ip,
            dst_addr: self.source,
            next_header: smoltcp::wire::IpProtocol::Icmp,
            payload_len: icmp_repr.buffer_len(),
            hop_limit: 64,
        };
        let ipv4_len = ipv4_repr.buffer_len();
        let reply_len = ipv4_len.saturating_add(icmp_repr.buffer_len());
        if output.len() < reply_len {
            return Err(IcmpEchoRequestError::ReplyBufferTooSmall);
        }
        let checksums = smoltcp::phy::ChecksumCapabilities::default();
        ipv4_repr.emit(
            &mut smoltcp::wire::Ipv4Packet::new_unchecked(&mut output[..reply_len]),
            &checksums,
        );
        icmp_repr.emit(
            &mut smoltcp::wire::Icmpv4Packet::new_unchecked(&mut output[ipv4_len..reply_len]),
            &checksums,
        );
        Ok(reply_len)
    }
}

#[cfg(feature = "net-console")]
pub(super) fn parse_icmp_echo_request(
    packet: &[u8],
    local_ip: smoltcp::wire::Ipv4Address,
) -> Result<IcmpEchoRequest<'_>, IcmpEchoRequestError> {
    let checksums = smoltcp::phy::ChecksumCapabilities::default();
    let ipv4_packet = smoltcp::wire::Ipv4Packet::new_checked(packet)
        .map_err(|_| IcmpEchoRequestError::MalformedIpv4)?;
    let ipv4 = smoltcp::wire::Ipv4Repr::parse(&ipv4_packet, &checksums)
        .map_err(|_| IcmpEchoRequestError::MalformedIpv4)?;
    if local_ip.is_unspecified() || ipv4.dst_addr != local_ip {
        return Err(IcmpEchoRequestError::NonLocalDestination);
    }
    if ipv4.src_addr.is_unspecified()
        || ipv4.src_addr.is_multicast()
        || ipv4.src_addr.is_broadcast()
    {
        return Err(IcmpEchoRequestError::InvalidSource);
    }
    if ipv4.next_header != smoltcp::wire::IpProtocol::Icmp {
        return Err(IcmpEchoRequestError::WrongProtocol);
    }
    let icmp_packet = smoltcp::wire::Icmpv4Packet::new_checked(ipv4_packet.payload())
        .map_err(|_| IcmpEchoRequestError::MalformedIcmp)?;
    match smoltcp::wire::Icmpv4Repr::parse(&icmp_packet, &checksums)
        .map_err(|_| IcmpEchoRequestError::MalformedIcmp)?
    {
        smoltcp::wire::Icmpv4Repr::EchoRequest {
            ident,
            seq_no,
            data,
        } => Ok(IcmpEchoRequest {
            source: ipv4.src_addr,
            ident,
            sequence: seq_no,
            payload: data,
        }),
        _ => Err(IcmpEchoRequestError::NotEchoRequest),
    }
}

/// Networking integration exposed to the pump when the `net` feature is enabled.
pub trait NetPoller {
    /// Poll the network subsystem and return whether new work occurred.
    fn poll(&mut self, now_ms: u64) -> bool;

    /// Poll the network subsystem through a HAL driver-task service budget.
    fn poll_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        budget.charge_ops(1)?;
        budget.charge_frames(1)?;
        Ok(self.poll(now_ms))
    }

    /// Flush TCP console work through a HAL driver-task service budget.
    fn flush_tcp_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        self.poll_with_budget(now_ms, budget)
    }

    /// Return the active network driver scheduling contract.
    fn driver_task_contract(&self) -> crate::hal::driver_task::DriverTaskContract;

    /// Obtain telemetry for diagnostics.
    fn telemetry(&self) -> NetTelemetry;

    /// Retrieve cumulative counters for diagnostics.
    fn stats(&self) -> NetCounters {
        NetCounters::default()
    }

    /// Drain any pending console lines produced by TCP listeners.
    fn drain_console_lines(&mut self, now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine));

    /// Drain a bounded number of pending console lines produced by TCP listeners.
    fn drain_console_lines_bounded(
        &mut self,
        now_ms: u64,
        max_lines: usize,
        visitor: &mut dyn FnMut(ConsoleLine),
    ) -> usize;

    /// Queue a console line for transmission to remote clients.
    fn send_console_line(&mut self, line: &str) -> bool;

    /// Request the active TCP console connection to close after flushing responses.
    fn request_disconnect(&mut self) {}

    /// Return true only after every byte queued for the named console
    /// connection has left both the console/coalescer queues and the TCP send
    /// queue. Reboot uses this as an exact ACK barrier.
    fn console_output_drained(&self, _conn_id: u64) -> bool {
        false
    }

    /// Drain pending net-console connection events (optional).
    fn drain_console_events(&mut self, _visitor: &mut dyn FnMut(NetConsoleEvent)) {}

    /// Snapshot ingest metrics for observability providers.
    fn ingest_snapshot(&self) -> IngestSnapshot {
        IngestSnapshot::default()
    }

    /// Return whether a complete TCP console line is buffered for root
    /// dispatch.
    ///
    /// This predicate is deliberately separate from socket service state:
    /// once ingest has retained a complete command, the linked-runtime
    /// scheduler must leave a CYW43 Network burst and route through the fixed
    /// physical-operator phases to Dispatch before admitting another NIC
    /// operation.
    fn buffered_console_lines_pending(&self) -> bool {
        self.ingest_snapshot().queued != 0
    }

    /// Return the active TCP console connection identifier, if any.
    fn active_console_conn_id(&self) -> Option<u64> {
        None
    }

    /// Return the active TCP console connection identifier only when its
    /// current session has completed authentication.
    fn authenticated_console_conn_id(&self) -> Option<u64> {
        None
    }

    /// Return whether the TCP console has exact socket/parser work that needs
    /// another network service turn. An idle authenticated connection must
    /// return `false`.
    fn console_service_pending(&self) -> bool {
        false
    }

    /// Return whether retained ICMP echo work is due for another network turn.
    ///
    /// This is separate from TCP console demand so a cold-neighbor Echo Reply
    /// can survive ARP resolution without manufacturing NIC-driver work.
    fn icmp_echo_service_due(&self, _now_ms: u64) -> bool {
        false
    }

    /// Inject a console line into the network transport (testing hook).
    fn inject_console_line(&mut self, _line: &str) {}

    /// Reset the underlying transport (testing hook).
    fn reset(&mut self) {}

    /// Expose the configured TCP console listen port.
    fn console_listen_port(&self) -> u16 {
        CONSOLE_TCP_PORT
    }

    /// Return whether the TCP console listener is bound and accepting peers.
    ///
    /// This is intentionally weaker than [`NetStatusReport::tcp_ready`] on
    /// physical drivers, where readiness also requires an accepted or
    /// authenticated session as end-to-end data-path proof.
    fn console_listener_ready(&self) -> bool {
        self.status_report().tcp_ready
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

    #[cfg(feature = "net-console")]
    use crate::hal::driver_task::{
        CYW43_WIFI_DRIVER_TASK_CONTRACT, GENET_DRIVER_TASK_CONTRACT, RTL8139_DRIVER_TASK_CONTRACT,
    };

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
    fn network_driver_task_contracts_match_backend_labels() {
        assert_eq!(
            RTL8139_DRIVER_TASK_CONTRACT.name,
            NetBackend::Rtl8139.label()
        );
        assert_eq!(
            GENET_DRIVER_TASK_CONTRACT.name,
            NetBackend::BcmGenet.label()
        );
        assert_eq!(CYW43_WIFI_DRIVER_TASK_CONTRACT.name, "cyw43455");
        assert_eq!(RTL8139_DRIVER_TASK_CONTRACT.validate(), Ok(()));
        assert_eq!(GENET_DRIVER_TASK_CONTRACT.validate(), Ok(()));
        assert_eq!(CYW43_WIFI_DRIVER_TASK_CONTRACT.validate(), Ok(()));
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

    #[test]
    fn net_self_test_verdict_is_stable_and_fail_closed() {
        assert_eq!(NetSelfTestResult::default().verdict(), "fail");
        assert_eq!(
            NetSelfTestResult {
                tx_ok: true,
                udp_echo_ok: true,
                tcp_ok: true,
                console_ok: true,
                peer_assisted_ok: false,
            }
            .verdict(),
            "pass"
        );
        assert_eq!(
            NetSelfTestResult {
                peer_assisted_ok: true,
                ..NetSelfTestResult::default()
            }
            .verdict(),
            "peer-assisted-pass"
        );
    }
}
