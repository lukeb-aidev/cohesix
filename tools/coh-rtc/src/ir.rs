// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define and validate the root-task manifest IR.
// Author: Lukas Bower

use anyhow::{bail, Context, Result};
use cohsh_core::MAX_LINE_LEN;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: &str = "1.6";
const PI4_PROFILE_NAME: &str = "pi4-uboot-aarch64";
const PI4_PROFILE_LEGACY_ALIAS: &str = "uefi-aarch64";
const MAX_WALK_DEPTH: usize = 8;
const MAX_MSIZE: u32 = 8192;
const MAX_SHARD_BITS: u8 = 8;
const SHARDED_WORKER_PATH_DEPTH: usize = 5;
const LEGACY_WORKER_PATH_DEPTH: usize = 3;
const EVENT_PUMP_TELEMETRY_BUDGET_BYTES: u32 = 32 * 1024;
const EVENT_PUMP_MAX_TELEMETRY_WORKERS: u32 = 8;
const EVENT_PUMP_CAS_BUDGET_BYTES: u32 = 32 * 1024;
const EVENT_PUMP_SIDECAR_BUDGET_BYTES: u32 = 16 * 1024;
const CAS_MAX_CHUNKS: u32 = 8;
const MAX_POLICY_QUEUE_ENTRIES: u16 = 256;
const MAX_POLICY_RULE_ID_LEN: usize = 64;
const MAX_REPLAY_ENTRIES: u16 = 256;
const MAX_OBSERVE_LATENCY_SAMPLES: u16 = 64;
const MAX_OBSERVE_WATCH_ENTRIES: u16 = 64;
const MAX_SIDECAR_SCOPE_LEN: usize = 64;
const MAX_SIDECAR_ID_LEN: usize = 64;
const MAX_SIDECAR_MOUNT_LEN: usize = 64;
const MAX_SPOOL_ENTRIES: u16 = 256;
const MAX_ROOT_CUT_REASON_LEN: usize = "network_unreachable".len();
const MAX_SESSION_STATE_LEN: usize = "DRAINING".len();
const MAX_SESSION_OWNER_LEN: usize = 64;
const MAX_U64_DIGITS: usize = 20;
const MAX_U32_DIGITS: usize = 10;
const MAX_U8_DIGITS: usize = 3;
const SHARD_LABEL_BYTES: usize = 2;
const SHARD_COUNT_DIGITS: usize = 3;
const MAX_TICKET_SCOPES: u16 = 16;
const MAX_TICKET_SCOPE_PATH_LEN: usize = 255;
const MAX_COH_ALLOWLIST: usize = 16;
const MAX_COH_TELEMETRY_DEVICES: u32 = 256;
const MAX_COH_SCHEMA_LEN: usize = 64;
const MAX_COH_LEASE_STATE_LEN: usize = 16;
const MAX_COH_PEFT_ID_LEN: u32 = 256;
const MAX_HOST_TICKET_ACTIONS: usize = 32;
const MAX_HOST_FEDERATION_PEERS: usize = 32;
const MAX_HOST_FEDERATION_HIVE_LEN: usize = 64;
const MAX_HOST_FEDERATION_AUTH_REF_LEN: usize = 128;
const MAX_HOST_FEDERATION_URL_LEN: usize = 256;
const MAX_HOST_FEDERATION_QUEUE_ENTRIES: u16 = 4096;
const MAX_HOST_FEDERATION_WAL_ENTRIES: u32 = 16_384;
const MAX_HOST_FEDERATION_QUEUE_BYTES: u32 = 1024 * 1024;
const MAX_HOST_FEDERATION_WAL_BYTES: u32 = 8 * 1024 * 1024;
const MAX_HW_DEVICES: usize = 32;
const MAX_HW_DEVICE_ID_LEN: usize = 64;
const MAX_LOCAL_SEAT_DEVICE_ID_LEN: usize = 64;
const MAX_LOCAL_SEAT_BUFFER_LINES: u16 = 1024;
const MAX_HW_NETWORK_IP_LITERAL_LEN: usize = 15;
const MAX_HW_NETWORK_DHCP_TIMEOUT_MS: u32 = 60_000;
const MAX_HW_NETWORK_DHCP_RETRIES: u8 = 16;
const MAX_LIFECYCLE_AUTO_TRANSITIONS: usize = 8;
const MAX_SCHEDULE_ID_LEN: usize = 64;
const MAX_SCHEDULE_ROLE_LEN: usize = 16;
const MAX_SCHEDULE_QUEUE_ENTRIES: u32 = 256;
const MAX_LEASE_ID_LEN: usize = 32;
const MAX_LEASE_SUBJECT_LEN: usize = 32;
const MAX_LEASE_RESOURCE_LEN: usize = 48;
const MAX_LEASE_REASON_LEN: usize = 24;
const MAX_LEASE_ACTIVE_ENTRIES: u32 = 256;
const MAX_LEASE_PREEMPTION_ENTRIES: u32 = 256;
const MAX_AFFINITY_CORES: u8 = 64;
const MAX_TELEMETRY_REFERENCE_ENTRIES_PER_SEGMENT: u32 = 16_384;
const MAX_DRIVER_RUNTIME_IMAGES: usize = 16;
const MAX_DRIVER_RUNTIME_IMAGE_ID_LEN: usize = 64;
const MAX_DRIVER_RUNTIME_IMAGE_PATH_LEN: usize = 160;
const MAX_DRIVER_RUNTIME_ENTRY_SYMBOL_LEN: usize = 96;
const MAX_DRIVER_RUNTIME_REGION_PAGES: u16 = 1024;
const MAX_DRIVER_RUNTIME_IRQS: usize = 16;
const MAX_DRIVER_RUNTIME_BUS_LINKS: usize = 8;
const DRIVER_RUNTIME_CHILD_CSPACE_SLOTS: u8 = 16;
const DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT: u8 = 3;
const DRIVER_RUNTIME_CYW43_SDIO_IRQ: u32 = 158;
const DRIVER_RUNTIME_CYW43_SDIO_BADGE: u32 = 159;
const DRIVER_RUNTIME_CYW43_SDIO_CLIENT_TO_OWNER_SLOT: u8 = 8;
const DRIVER_RUNTIME_CYW43_SDIO_OWNER_TO_CLIENT_SLOT: u8 = 10;
const DRIVER_RUNTIME_CYW43_SDIO_SHARED_OFFSET: u32 = 4096;
const DRIVER_RUNTIME_CYW43_SDIO_SHARED_LEN: u32 = 8192;
const DRIVER_RUNTIME_CYW43_SDIO_LINK_EPOCH: u32 = 0x4359_5301;
const DRIVER_RUNTIME_RING_FRAME_OFFSET: u16 = 256;
const DRIVER_RUNTIME_DPC_EVENT_OFFSET: u16 = 160;
const DRIVER_RUNTIME_DPC_EVENT_LEN: u16 = 96;
const DRIVER_RUNTIME_DPC_EVENT_DEPTH: u16 = 4;
const MAX_WORKER_RUNTIME_ROLES: usize = 8;
const MAX_WORKER_RUNTIME_TEXT_LEN: usize = 96;
const REQUIRED_WORKER_ROLE_RECORDS: [Role; 3] =
    [Role::WorkerHeartbeat, Role::WorkerGpu, Role::WorkerLora];
const REQUIRED_PI4_DRIVER_RUNTIME_HOT_PATHS: [&str; 7] = [
    "serial-console",
    "usb-keyboard",
    "hdmi-text",
    "genet-nic",
    "cyw43-wifi",
    "sdio-host",
    "pcie-root",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub meta: ManifestMeta,
    pub root_task: RootTaskSection,
    pub profile: Profile,
    pub event_pump: EventPump,
    pub secure9p: Secure9pLimits,
    pub features: FeatureToggles,
    #[serde(default)]
    pub hw: HardwareConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub dma: DmaConfig,
    pub tickets: Vec<TicketSpec>,
    #[serde(default)]
    pub ticket_limits: TicketLimits,
    #[serde(default)]
    pub namespaces: Namespaces,
    #[serde(default)]
    pub sharding: Sharding,
    #[serde(default)]
    pub ecosystem: Ecosystem,
    #[serde(default)]
    pub sidecars: Sidecars,
    #[serde(default)]
    pub telemetry: Telemetry,
    #[serde(default)]
    pub telemetry_ingest: TelemetryIngest,
    #[serde(default)]
    pub lifecycle: LifecycleConfig,
    #[serde(default)]
    pub worker_runtime: WorkerRuntimeConfig,
    #[serde(default)]
    pub control_plane: ControlPlaneConfig,
    #[serde(default)]
    pub observability: Observability,
    #[serde(default)]
    pub ui_providers: UiProviders,
    #[serde(default)]
    pub client_policies: ClientPolicies,
    #[serde(default)]
    pub client_paths: ClientPaths,
    #[serde(default)]
    pub swarmui: SwarmUiConfig,
    #[serde(default)]
    pub cas: CasConfig,
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        self.validate_with_base(None)
    }

    pub fn validate_with_base(&self, base_dir: Option<&Path>) -> Result<()> {
        if self.root_task.schema != SCHEMA_VERSION {
            bail!(
                "unsupported root_task.schema {} (expected {})",
                self.root_task.schema,
                SCHEMA_VERSION
            );
        }
        if self.secure9p.msize > MAX_MSIZE {
            bail!(
                "secure9p.msize {} exceeds maximum {}",
                self.secure9p.msize,
                MAX_MSIZE
            );
        }
        if self.secure9p.walk_depth as usize > MAX_WALK_DEPTH {
            bail!(
                "secure9p.walk_depth {} exceeds maximum {}",
                self.secure9p.walk_depth,
                MAX_WALK_DEPTH
            );
        }
        if self.secure9p.tags_per_session < 1 {
            bail!("secure9p.tags_per_session must be >= 1");
        }
        if self.secure9p.batch_frames < 1 {
            bail!("secure9p.batch_frames must be >= 1");
        }
        if self.profile.kernel {
            if self.features.std_console {
                bail!("std_console requires profile.kernel = false");
            }
            if self.features.std_host_tools {
                bail!("std_host_tools requires profile.kernel = false");
            }
        }
        self.validate_hw()?;
        self.validate_cache()?;
        self.validate_dma()?;
        self.validate_namespace_mounts()?;
        self.validate_sharding()?;
        self.validate_tickets()?;
        self.validate_ticket_limits()?;
        self.validate_ecosystem()?;
        self.validate_sidecars()?;
        self.validate_telemetry()?;
        self.validate_lifecycle()?;
        self.validate_worker_runtime()?;
        self.validate_control_plane()?;
        self.validate_observability()?;
        self.validate_ui_providers()?;
        self.validate_client_policies()?;
        self.validate_client_paths()?;
        self.validate_swarmui()?;
        self.validate_cas(base_dir)?;
        self.validate_affinity()?;
        self.root_task.driver_images.validate()?;
        Ok(())
    }

    fn validate_namespace_mounts(&self) -> Result<()> {
        for mount in &self.namespaces.mounts {
            if mount.target.len() > MAX_WALK_DEPTH {
                bail!(
                    "namespace mount {} exceeds walk depth {}",
                    mount.service,
                    MAX_WALK_DEPTH
                );
            }
            for component in &mount.target {
                if component == ".." {
                    bail!("namespace mount {} contains disallowed '..'", mount.service);
                }
                if component.is_empty() {
                    bail!(
                        "namespace mount {} contains empty path component",
                        mount.service
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_tickets(&self) -> Result<()> {
        let mut seen = BTreeSet::new();
        for ticket in &self.tickets {
            let key = (ticket.role.as_str(), ticket.secret.as_str());
            if !seen.insert(key) {
                bail!("duplicate ticket entry for role {}", ticket.role.as_str());
            }
        }
        Ok(())
    }

    fn validate_affinity(&self) -> Result<()> {
        let affinity = &self.root_task.affinity;
        if affinity.max_cores == 0 {
            bail!("root_task.affinity.max_cores must be >= 1");
        }
        if affinity.max_cores > MAX_AFFINITY_CORES {
            bail!(
                "root_task.affinity.max_cores {} exceeds max {}",
                affinity.max_cores,
                MAX_AFFINITY_CORES
            );
        }
        let max_core = affinity.max_cores;
        if let Some(core) = affinity.authority_core {
            if core >= max_core {
                bail!(
                    "root_task.affinity.authority_core {} exceeds max_core {}",
                    core,
                    max_core
                );
            }
        }
        for &core in affinity.ninedoor_cores.iter() {
            if core >= max_core {
                bail!(
                    "root_task.affinity.ninedoor_cores contains {} which exceeds max_core {}",
                    core,
                    max_core
                );
            }
        }
        for &core in affinity.provider_cores.iter() {
            if core >= max_core {
                bail!(
                    "root_task.affinity.provider_cores contains {} which exceeds max_core {}",
                    core,
                    max_core
                );
            }
        }
        for &core in affinity.worker_cores.iter() {
            if core >= max_core {
                bail!(
                    "root_task.affinity.worker_cores contains {} which exceeds max_core {}",
                    core,
                    max_core
                );
            }
        }
        affinity
            .drivers
            .validate(affinity.authority_core.unwrap_or(0), max_core)?;
        Ok(())
    }

    fn validate_hw(&self) -> Result<()> {
        if self.hw.devices.len() > MAX_HW_DEVICES {
            bail!("hw.devices exceeds max {}", MAX_HW_DEVICES);
        }
        let mut seen_ids = BTreeSet::new();
        for device in &self.hw.devices {
            let id = device.id.trim();
            if id.is_empty() {
                bail!("hw.devices[].id must not be empty");
            }
            if id.len() > MAX_HW_DEVICE_ID_LEN {
                bail!(
                    "hw.devices[].id '{}' exceeds max length {}",
                    id,
                    MAX_HW_DEVICE_ID_LEN
                );
            }
            if id.contains('/') || id.contains("..") {
                bail!("hw.devices[].id '{}' contains invalid path characters", id);
            }
            if !seen_ids.insert(id.to_owned()) {
                bail!("hw.devices[].id '{}' is duplicated", id);
            }
        }

        let local_seat = &self.hw.local_seat;
        if local_seat.required && !local_seat.enabled {
            bail!("hw.local_seat.required=true requires hw.local_seat.enabled=true");
        }
        if local_seat.keyboard_device.trim().is_empty() {
            bail!("hw.local_seat.keyboard_device must not be empty");
        }
        if local_seat.display_device.trim().is_empty() {
            bail!("hw.local_seat.display_device must not be empty");
        }
        if local_seat.keyboard_device.len() > MAX_LOCAL_SEAT_DEVICE_ID_LEN {
            bail!(
                "hw.local_seat.keyboard_device exceeds max length {}",
                MAX_LOCAL_SEAT_DEVICE_ID_LEN
            );
        }
        if local_seat.display_device.len() > MAX_LOCAL_SEAT_DEVICE_ID_LEN {
            bail!(
                "hw.local_seat.display_device exceeds max length {}",
                MAX_LOCAL_SEAT_DEVICE_ID_LEN
            );
        }
        if local_seat.line_bytes == 0 {
            bail!("hw.local_seat.line_bytes must be >= 1");
        }
        if u32::from(local_seat.line_bytes) > self.secure9p.msize {
            bail!(
                "hw.local_seat.line_bytes {} exceeds secure9p.msize {}",
                local_seat.line_bytes,
                self.secure9p.msize
            );
        }
        if local_seat.buffer_lines == 0 {
            bail!("hw.local_seat.buffer_lines must be >= 1");
        }
        if local_seat.buffer_lines > MAX_LOCAL_SEAT_BUFFER_LINES {
            bail!(
                "hw.local_seat.buffer_lines {} exceeds max {}",
                local_seat.buffer_lines,
                MAX_LOCAL_SEAT_BUFFER_LINES
            );
        }

        let attest = &self.hw.attestation;
        if attest.evidence_max_bytes == 0 {
            bail!("hw.attestation.evidence_max_bytes must be >= 1");
        }
        if u32::from(attest.evidence_max_bytes) > self.secure9p.msize {
            bail!(
                "hw.attestation.evidence_max_bytes {} exceeds secure9p.msize {}",
                attest.evidence_max_bytes,
                self.secure9p.msize
            );
        }

        let has_device =
            |kind: HardwareDeviceKind| self.hw.devices.iter().any(|device| device.kind == kind);
        let find_device = |kind: HardwareDeviceKind, id: &str| {
            self.hw
                .devices
                .iter()
                .find(|device| device.kind == kind && device.id == id)
        };
        let has_tpm = has_device(HardwareDeviceKind::Tpm);
        let has_net = has_device(HardwareDeviceKind::Net);
        let has_wifi = has_device(HardwareDeviceKind::Wifi);
        if attest.enabled {
            match attest.policy {
                AttestationPolicy::TpmOnly if !has_tpm => {
                    bail!("hw.attestation.policy=tpm-only requires hw.devices[] kind=tpm");
                }
                AttestationPolicy::TpmOrDice
                | AttestationPolicy::DiceOnly
                | AttestationPolicy::TpmOnly => {}
            }
        }

        let profile_name = self.profile.name.as_str();
        if self.hw.network.static_ipv4.ip.len() > MAX_HW_NETWORK_IP_LITERAL_LEN {
            bail!(
                "hw.network.static_ipv4.ip exceeds max length {}",
                MAX_HW_NETWORK_IP_LITERAL_LEN
            );
        }
        if let Some(gateway) = self.hw.network.static_ipv4.gateway.as_ref() {
            if gateway.len() > MAX_HW_NETWORK_IP_LITERAL_LEN {
                bail!(
                    "hw.network.static_ipv4.gateway exceeds max length {}",
                    MAX_HW_NETWORK_IP_LITERAL_LEN
                );
            }
        }
        if self.hw.network.dhcp.discover_timeout_ms == 0
            || self.hw.network.dhcp.discover_timeout_ms > MAX_HW_NETWORK_DHCP_TIMEOUT_MS
        {
            bail!(
                "hw.network.dhcp.discover_timeout_ms {} must be in 1..={}",
                self.hw.network.dhcp.discover_timeout_ms,
                MAX_HW_NETWORK_DHCP_TIMEOUT_MS
            );
        }
        if self.hw.network.dhcp.request_timeout_ms == 0
            || self.hw.network.dhcp.request_timeout_ms > MAX_HW_NETWORK_DHCP_TIMEOUT_MS
        {
            bail!(
                "hw.network.dhcp.request_timeout_ms {} must be in 1..={}",
                self.hw.network.dhcp.request_timeout_ms,
                MAX_HW_NETWORK_DHCP_TIMEOUT_MS
            );
        }
        if self.hw.network.dhcp.max_retries == 0
            || self.hw.network.dhcp.max_retries > MAX_HW_NETWORK_DHCP_RETRIES
        {
            bail!(
                "hw.network.dhcp.max_retries {} must be in 1..={}",
                self.hw.network.dhcp.max_retries,
                MAX_HW_NETWORK_DHCP_RETRIES
            );
        }
        if self.hw.network.backend == NetworkBackendKind::BcmGenetV5
            && !self.profile_is_pi4_family()
        {
            bail!(
                "hw.network.backend=bcmgenet-v5 is only valid for profile.name={} (or legacy alias {})",
                PI4_PROFILE_NAME,
                PI4_PROFILE_LEGACY_ALIAS
            );
        }

        if self.profile_is_pi4_family() {
            if !has_device(HardwareDeviceKind::Uart) {
                bail!("profile.name={profile_name} requires hw.devices[] kind=uart");
            }
            if !has_device(HardwareDeviceKind::Rtc) {
                bail!("profile.name={profile_name} requires hw.devices[] kind=rtc");
            }
            if attest.enabled
                && matches!(
                    attest.policy,
                    AttestationPolicy::TpmOnly | AttestationPolicy::TpmOrDice
                )
                && !has_tpm
            {
                bail!(
                    "profile.name={profile_name} with attestation enabled requires TPM device declaration"
                );
            }
            if local_seat.enabled {
                let keyboard =
                    find_device(HardwareDeviceKind::Keyboard, &local_seat.keyboard_device)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "hw.local_seat.keyboard_device={} requires matching hw.devices[] kind=keyboard",
                                local_seat.keyboard_device
                            )
                        })?;
                let display = find_device(HardwareDeviceKind::Display, &local_seat.display_device)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "hw.local_seat.display_device={} requires matching hw.devices[] kind=display",
                            local_seat.display_device
                        )
                    })?;
                if local_seat.required && !keyboard.required {
                    bail!(
                        "hw.local_seat.required=true requires hw.devices[] kind=keyboard id={} required=true",
                        local_seat.keyboard_device
                    );
                }
                if local_seat.required && !display.required {
                    bail!(
                        "hw.local_seat.required=true requires hw.devices[] kind=display id={} required=true",
                        local_seat.display_device
                    );
                }
            }
            if self.hw.no_nic {
                if self.features.net_console {
                    bail!(
                        "profile.name={profile_name} with hw.no_nic=true requires features.net_console=false"
                    );
                }
                if self.hw.network.enabled {
                    bail!(
                        "profile.name={profile_name} with hw.no_nic=true requires hw.network.enabled=false"
                    );
                }
            } else if self.features.net_console {
                self.validate_pi4_network(profile_name, has_net, has_wifi)?;
            } else if self.hw.network.enabled {
                bail!(
                    "profile.name={profile_name} with hw.network.enabled=true requires features.net_console=true"
                );
            }
        } else {
            self.validate_optional_static_ipv4_fields()?;
            if self.hw.network.interface != NetworkInterfacePolicy::Wired {
                bail!(
                    "hw.network.interface={} is only valid for profile.name={} (or legacy alias {})",
                    self.hw.network.interface.as_str(),
                    PI4_PROFILE_NAME,
                    PI4_PROFILE_LEGACY_ALIAS
                );
            }
        }

        Ok(())
    }

    fn profile_is_pi4_family(&self) -> bool {
        matches!(
            self.profile.name.as_str(),
            PI4_PROFILE_NAME | PI4_PROFILE_LEGACY_ALIAS
        )
    }

    fn parse_ipv4_literal(label: &str, value: &str) -> Result<Ipv4Addr> {
        value
            .parse::<Ipv4Addr>()
            .with_context(|| format!("{label} must be a valid IPv4 literal"))
    }

    fn validate_optional_static_ipv4_fields(&self) -> Result<()> {
        if !self.hw.network.static_ipv4.ip.trim().is_empty() {
            let _ = Self::parse_ipv4_literal(
                "hw.network.static_ipv4.ip",
                self.hw.network.static_ipv4.ip.trim(),
            )?;
        }
        if let Some(gateway) = self.hw.network.static_ipv4.gateway.as_ref() {
            if !gateway.trim().is_empty() {
                let _ = Self::parse_ipv4_literal("hw.network.static_ipv4.gateway", gateway.trim())?;
            }
        }
        if self.hw.network.static_ipv4.prefix_len != 0
            && !(1..=32).contains(&self.hw.network.static_ipv4.prefix_len)
        {
            bail!(
                "hw.network.static_ipv4.prefix_len {} must be in 1..=32",
                self.hw.network.static_ipv4.prefix_len
            );
        }
        Ok(())
    }

    fn validate_pi4_network(
        &self,
        profile_name: &str,
        has_net_device: bool,
        has_wifi_device: bool,
    ) -> Result<()> {
        if !self.hw.network.enabled {
            bail!(
                "profile.name={profile_name} with features.net_console=true requires hw.network.enabled=true"
            );
        }
        if self.hw.network.mode == NetworkMode::Off {
            bail!("hw.network.mode=off requires hw.network.enabled=false");
        }
        self.validate_optional_static_ipv4_fields()?;
        match self.hw.network.interface {
            NetworkInterfacePolicy::Wired => {
                if !has_net_device {
                    bail!(
                        "profile.name={profile_name} network-enabled mode requires hw.devices[] kind=net"
                    );
                }
                if self.hw.network.backend != NetworkBackendKind::BcmGenetV5 {
                    bail!(
                        "profile.name={profile_name} network-enabled wired mode requires hw.network.backend=bcmgenet-v5"
                    );
                }
            }
            NetworkInterfacePolicy::Wifi => {
                if !has_wifi_device {
                    bail!("profile.name={profile_name} wifi mode requires hw.devices[] kind=wifi");
                }
                if self.hw.network.backend != NetworkBackendKind::BcmGenetV5 {
                    bail!(
                        "profile.name={profile_name} wifi mode requires hw.network.backend=bcmgenet-v5"
                    );
                }
                if !matches!(
                    self.hw.network.mode,
                    NetworkMode::Dhcp | NetworkMode::Static
                ) {
                    bail!(
                        "profile.name={profile_name} wifi mode requires hw.network.mode=dhcp|static"
                    );
                }
            }
            NetworkInterfacePolicy::Auto => {
                if !has_net_device {
                    bail!("profile.name={profile_name} auto mode requires hw.devices[] kind=net");
                }
                if !has_wifi_device {
                    bail!("profile.name={profile_name} auto mode requires hw.devices[] kind=wifi");
                }
                if self.hw.network.backend != NetworkBackendKind::BcmGenetV5 {
                    bail!(
                        "profile.name={profile_name} auto mode requires hw.network.backend=bcmgenet-v5"
                    );
                }
                if self.hw.network.mode != NetworkMode::Dhcp {
                    bail!("profile.name={profile_name} auto mode requires hw.network.mode=dhcp");
                }
            }
        }
        if self.hw.network.mode == NetworkMode::Static {
            let ip_literal = self.hw.network.static_ipv4.ip.trim();
            if ip_literal.is_empty() {
                bail!("hw.network.static_ipv4.ip must be set for profile.name={profile_name}");
            }
            let ip = Self::parse_ipv4_literal("hw.network.static_ipv4.ip", ip_literal)?;
            if ip.is_unspecified() {
                bail!("hw.network.static_ipv4.ip must not be 0.0.0.0");
            }
            let prefix = self.hw.network.static_ipv4.prefix_len;
            if !(1..=32).contains(&prefix) {
                bail!(
                    "hw.network.static_ipv4.prefix_len {} must be in 1..=32",
                    prefix
                );
            }
            if let Some(gateway) = self.hw.network.static_ipv4.gateway.as_ref() {
                let gateway_literal = gateway.trim();
                if !gateway_literal.is_empty() {
                    let parsed = Self::parse_ipv4_literal(
                        "hw.network.static_ipv4.gateway",
                        gateway_literal,
                    )?;
                    if parsed.is_unspecified() {
                        bail!("hw.network.static_ipv4.gateway must not be 0.0.0.0");
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_ticket_limits(&self) -> Result<()> {
        if self.ticket_limits.max_scopes > MAX_TICKET_SCOPES {
            bail!(
                "ticket_limits.max_scopes {} exceeds maximum {}",
                self.ticket_limits.max_scopes,
                MAX_TICKET_SCOPES
            );
        }
        if self.ticket_limits.max_scope_path_len == 0 {
            bail!("ticket_limits.max_scope_path_len must be >= 1");
        }
        if self.ticket_limits.max_scope_path_len as usize > MAX_TICKET_SCOPE_PATH_LEN {
            bail!(
                "ticket_limits.max_scope_path_len {} exceeds maximum {}",
                self.ticket_limits.max_scope_path_len,
                MAX_TICKET_SCOPE_PATH_LEN
            );
        }
        Ok(())
    }

    fn validate_sharding(&self) -> Result<()> {
        if self.sharding.shard_bits > MAX_SHARD_BITS {
            bail!(
                "sharding.shard_bits {} exceeds max {}",
                self.sharding.shard_bits,
                MAX_SHARD_BITS
            );
        }
        if self.sharding.enabled {
            if (self.secure9p.walk_depth as usize) < SHARDED_WORKER_PATH_DEPTH {
                bail!(
                    "sharding.enabled requires secure9p.walk_depth >= {}",
                    SHARDED_WORKER_PATH_DEPTH
                );
            }
            if self.sharding.legacy_worker_alias
                && (self.secure9p.walk_depth as usize) < LEGACY_WORKER_PATH_DEPTH
            {
                bail!(
                    "sharding.legacy_worker_alias requires secure9p.walk_depth >= {}",
                    LEGACY_WORKER_PATH_DEPTH
                );
            }
            if !self.sharding.legacy_worker_alias {
                self.reject_legacy_worker_paths()?;
            }
        }
        Ok(())
    }

    fn reject_legacy_worker_paths(&self) -> Result<()> {
        for mount in &self.namespaces.mounts {
            if matches!(mount.target.first(), Some(component) if component == "worker") {
                bail!(
                    "namespace mount {} references legacy /worker paths while sharding.legacy_worker_alias is false",
                    mount.service
                );
            }
        }
        for rule in &self.ecosystem.policy.rules {
            let target = rule.target.trim();
            let components: Vec<&str> = target.split('/').filter(|seg| !seg.is_empty()).collect();
            if matches!(components.first(), Some(component) if *component == "worker") {
                bail!(
                    "ecosystem.policy.rules[].target references legacy /worker paths while sharding.legacy_worker_alias is false"
                );
            }
        }
        Ok(())
    }

    fn validate_ecosystem(&self) -> Result<()> {
        self.validate_policy()?;
        self.validate_audit()?;
        let host = &self.ecosystem.host;
        if !host.enable {
            if host.tickets.enable {
                bail!("ecosystem.host.tickets.enable requires ecosystem.host.enable = true");
            }
            if host.federation.enable {
                bail!("ecosystem.host.federation.enable requires ecosystem.host.enable = true");
            }
            return Ok(());
        }
        self.validate_host_mount()?;
        self.validate_host_tickets()?;
        self.validate_host_federation()?;
        if self.secure9p.msize > MAX_MSIZE {
            bail!("ecosystem.host.enable requires secure9p.msize <= {MAX_MSIZE}");
        }
        if self.secure9p.walk_depth as usize > MAX_WALK_DEPTH {
            bail!("ecosystem.host.enable requires secure9p.walk_depth <= {MAX_WALK_DEPTH}");
        }
        if !self.namespaces.role_isolation {
            bail!("ecosystem.host.enable requires namespaces.role_isolation = true");
        }
        Ok(())
    }

    fn validate_sidecars(&self) -> Result<()> {
        self.validate_sidecar_bus("sidecars.modbus", &self.sidecars.modbus)?;
        self.validate_sidecar_bus("sidecars.dnp3", &self.sidecars.dnp3)?;
        self.validate_sidecar_scopes()?;
        self.validate_sidecar_budget()?;
        Ok(())
    }

    fn validate_sidecar_bus(&self, label: &str, config: &SidecarBusConfig) -> Result<()> {
        if !config.enable {
            return Ok(());
        }
        self.validate_sidecar_mount_at(&format!("{label}.mount_at"), &config.mount_at)?;
        if config.adapters.is_empty() {
            bail!("{label}.enable requires at least one adapter");
        }
        let mut scopes = BTreeSet::new();
        for adapter in &config.adapters {
            self.validate_sidecar_adapter(label, adapter)?;
            if !scopes.insert(adapter.scope.as_str()) {
                bail!("{label}.adapters scope '{}' is duplicated", adapter.scope);
            }
        }
        Ok(())
    }

    fn validate_sidecar_adapter(&self, label: &str, adapter: &SidecarBusAdapter) -> Result<()> {
        self.validate_sidecar_id(&format!("{label}.adapters[].id"), &adapter.id)?;
        self.validate_sidecar_mount(&format!("{label}.adapters[].mount"), &adapter.mount)?;
        self.validate_sidecar_scope(&format!("{label}.adapters[].scope"), &adapter.scope)?;
        match adapter.link {
            SidecarLink::Serial => {
                if adapter.baud == 0 {
                    bail!("{label}.adapters[].baud must be >= 1 for serial links");
                }
            }
            SidecarLink::Tcp => {}
        }
        self.validate_spool(&format!("{label}.adapters[].spool"), &adapter.spool)?;
        Ok(())
    }

    fn validate_spool(&self, label: &str, spool: &SpoolConfig) -> Result<()> {
        if spool.max_entries == 0 {
            bail!("{label}.max_entries must be >= 1");
        }
        if spool.max_entries > MAX_SPOOL_ENTRIES {
            bail!(
                "{label}.max_entries {} exceeds max {}",
                spool.max_entries,
                MAX_SPOOL_ENTRIES
            );
        }
        if spool.max_bytes == 0 {
            bail!("{label}.max_bytes must be >= 1");
        }
        if spool.max_bytes > self.secure9p.msize {
            bail!(
                "{label}.max_bytes {} exceeds secure9p.msize {}",
                spool.max_bytes,
                self.secure9p.msize
            );
        }
        Ok(())
    }

    fn validate_sidecar_mount_at(&self, label: &str, mount_at: &str) -> Result<()> {
        let trimmed = mount_at.trim();
        if !trimmed.starts_with('/') {
            bail!("{label} must be an absolute path");
        }
        let components: Vec<&str> = trimmed.split('/').filter(|seg| !seg.is_empty()).collect();
        if components.is_empty() {
            bail!("{label} must not be root");
        }
        if components.len() > self.secure9p.walk_depth as usize {
            bail!(
                "{label} exceeds secure9p.walk_depth {}",
                self.secure9p.walk_depth
            );
        }
        if components.len() + 1 > self.secure9p.walk_depth as usize {
            bail!(
                "{label} requires secure9p.walk_depth >= {}",
                components.len() + 1
            );
        }
        for component in components {
            if component == ".." {
                bail!("{label} contains disallowed '..'");
            }
            if component.is_empty() {
                bail!("{label} contains empty path component");
            }
        }
        Ok(())
    }

    fn validate_sidecar_id(&self, label: &str, id: &str) -> Result<()> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            bail!("{label} must not be empty");
        }
        if trimmed.len() > MAX_SIDECAR_ID_LEN {
            bail!(
                "{label} '{}' exceeds max length {}",
                trimmed,
                MAX_SIDECAR_ID_LEN
            );
        }
        if trimmed.contains('/') {
            bail!("{label} '{}' must not include '/'", trimmed);
        }
        if trimmed == ".." {
            bail!("{label} must not be '..'");
        }
        Ok(())
    }

    fn validate_sidecar_mount(&self, label: &str, mount: &str) -> Result<()> {
        let trimmed = mount.trim();
        if trimmed.is_empty() {
            bail!("{label} must not be empty");
        }
        if trimmed.len() > MAX_SIDECAR_MOUNT_LEN {
            bail!(
                "{label} '{}' exceeds max length {}",
                trimmed,
                MAX_SIDECAR_MOUNT_LEN
            );
        }
        if trimmed.contains('/') {
            bail!("{label} '{}' must not include '/'", trimmed);
        }
        if trimmed == ".." {
            bail!("{label} must not be '..'");
        }
        Ok(())
    }

    fn validate_sidecar_scope(&self, label: &str, scope: &str) -> Result<()> {
        let trimmed = scope.trim();
        if trimmed.is_empty() {
            bail!("{label} must not be empty");
        }
        if trimmed.len() > MAX_SIDECAR_SCOPE_LEN {
            bail!(
                "{label} '{}' exceeds max length {}",
                trimmed,
                MAX_SIDECAR_SCOPE_LEN
            );
        }
        if trimmed.contains('/') {
            bail!("{label} '{}' must not include '/'", trimmed);
        }
        if trimmed == ".." {
            bail!("{label} must not be '..'");
        }
        Ok(())
    }

    fn validate_sidecar_budget(&self) -> Result<()> {
        let mut bytes = 0u32;
        if self.sidecars.modbus.enable {
            for adapter in &self.sidecars.modbus.adapters {
                bytes = bytes.saturating_add(adapter.spool.max_bytes);
            }
        }
        if self.sidecars.dnp3.enable {
            for adapter in &self.sidecars.dnp3.adapters {
                bytes = bytes.saturating_add(adapter.spool.max_bytes);
            }
        }
        if bytes > EVENT_PUMP_SIDECAR_BUDGET_BYTES {
            bail!(
                "sidecar budgets {} bytes exceed event-pump budget {} bytes",
                bytes,
                EVENT_PUMP_SIDECAR_BUDGET_BYTES
            );
        }
        Ok(())
    }

    fn validate_sidecar_scopes(&self) -> Result<()> {
        let mut scopes = BTreeSet::new();
        if self.sidecars.modbus.enable {
            for adapter in &self.sidecars.modbus.adapters {
                if !scopes.insert(adapter.scope.as_str()) {
                    bail!("sidecar scope '{}' is duplicated", adapter.scope);
                }
            }
        }
        if self.sidecars.dnp3.enable {
            for adapter in &self.sidecars.dnp3.adapters {
                if !scopes.insert(adapter.scope.as_str()) {
                    bail!("sidecar scope '{}' is duplicated", adapter.scope);
                }
            }
        }
        Ok(())
    }

    fn validate_policy(&self) -> Result<()> {
        let policy = &self.ecosystem.policy;
        if policy.queue_max_entries == 0 {
            bail!("ecosystem.policy.queue_max_entries must be >= 1");
        }
        if policy.queue_max_entries > MAX_POLICY_QUEUE_ENTRIES {
            bail!(
                "ecosystem.policy.queue_max_entries {} exceeds max {}",
                policy.queue_max_entries,
                MAX_POLICY_QUEUE_ENTRIES
            );
        }
        let msize = self.secure9p.msize;
        if policy.queue_max_bytes == 0 {
            bail!("ecosystem.policy.queue_max_bytes must be >= 1");
        }
        if policy.queue_max_bytes > msize {
            bail!(
                "ecosystem.policy.queue_max_bytes {} exceeds secure9p.msize {}",
                policy.queue_max_bytes,
                msize
            );
        }
        if policy.ctl_max_bytes == 0 {
            bail!("ecosystem.policy.ctl_max_bytes must be >= 1");
        }
        if policy.ctl_max_bytes > msize {
            bail!(
                "ecosystem.policy.ctl_max_bytes {} exceeds secure9p.msize {}",
                policy.ctl_max_bytes,
                msize
            );
        }
        if policy.status_max_bytes == 0 {
            bail!("ecosystem.policy.status_max_bytes must be >= 1");
        }
        if policy.status_max_bytes > msize {
            bail!(
                "ecosystem.policy.status_max_bytes {} exceeds secure9p.msize {}",
                policy.status_max_bytes,
                msize
            );
        }
        for rule in &policy.rules {
            validate_policy_rule(rule)?;
        }
        Ok(())
    }

    fn validate_audit(&self) -> Result<()> {
        let audit = &self.ecosystem.audit;
        let msize = self.secure9p.msize;
        if audit.journal_max_bytes == 0 {
            bail!("ecosystem.audit.journal_max_bytes must be >= 1");
        }
        if audit.journal_max_bytes > msize {
            bail!(
                "ecosystem.audit.journal_max_bytes {} exceeds secure9p.msize {}",
                audit.journal_max_bytes,
                msize
            );
        }
        if audit.decisions_max_bytes == 0 {
            bail!("ecosystem.audit.decisions_max_bytes must be >= 1");
        }
        if audit.decisions_max_bytes > msize {
            bail!(
                "ecosystem.audit.decisions_max_bytes {} exceeds secure9p.msize {}",
                audit.decisions_max_bytes,
                msize
            );
        }
        if audit.replay_ctl_max_bytes == 0 {
            bail!("ecosystem.audit.replay_ctl_max_bytes must be >= 1");
        }
        if audit.replay_ctl_max_bytes > msize {
            bail!(
                "ecosystem.audit.replay_ctl_max_bytes {} exceeds secure9p.msize {}",
                audit.replay_ctl_max_bytes,
                msize
            );
        }
        if audit.replay_status_max_bytes == 0 {
            bail!("ecosystem.audit.replay_status_max_bytes must be >= 1");
        }
        if audit.replay_status_max_bytes > msize {
            bail!(
                "ecosystem.audit.replay_status_max_bytes {} exceeds secure9p.msize {}",
                audit.replay_status_max_bytes,
                msize
            );
        }
        if audit.replay_max_entries == 0 {
            bail!("ecosystem.audit.replay_max_entries must be >= 1");
        }
        if audit.replay_max_entries > MAX_REPLAY_ENTRIES {
            bail!(
                "ecosystem.audit.replay_max_entries {} exceeds max {}",
                audit.replay_max_entries,
                MAX_REPLAY_ENTRIES
            );
        }
        if audit.replay_enable && !audit.enable {
            bail!("ecosystem.audit.replay_enable requires ecosystem.audit.enable = true");
        }
        Ok(())
    }

    fn validate_host_mount(&self) -> Result<()> {
        let mount_at = self.ecosystem.host.mount_at.trim();
        if !mount_at.starts_with('/') {
            bail!("ecosystem.host.mount_at must be an absolute path");
        }
        let components: Vec<&str> = mount_at.split('/').filter(|seg| !seg.is_empty()).collect();
        if components.is_empty() {
            bail!("ecosystem.host.mount_at must not be root");
        }
        if components.len() > MAX_WALK_DEPTH {
            bail!(
                "ecosystem.host.mount_at exceeds walk depth {}",
                MAX_WALK_DEPTH
            );
        }
        if self.ecosystem.host.tickets.enable
            && components.len().saturating_add(2) > self.secure9p.walk_depth as usize
        {
            bail!(
                "ecosystem.host.mount_at requires secure9p.walk_depth >= {} for /tickets paths",
                components.len() + 2
            );
        }
        for component in components {
            if component == ".." {
                bail!("ecosystem.host.mount_at contains disallowed '..'");
            }
            if component.is_empty() {
                bail!("ecosystem.host.mount_at contains empty path component");
            }
        }
        Ok(())
    }

    fn validate_host_tickets(&self) -> Result<()> {
        let tickets = &self.ecosystem.host.tickets;
        if !tickets.enable {
            return Ok(());
        }
        let request_schema = tickets.request_schema.trim();
        if request_schema.is_empty() {
            bail!("ecosystem.host.tickets.request_schema must not be empty");
        }
        if request_schema.len() > MAX_COH_SCHEMA_LEN {
            bail!(
                "ecosystem.host.tickets.request_schema exceeds max length {}",
                MAX_COH_SCHEMA_LEN
            );
        }
        let result_schema = tickets.result_schema.trim();
        if result_schema.is_empty() {
            bail!("ecosystem.host.tickets.result_schema must not be empty");
        }
        if result_schema.len() > MAX_COH_SCHEMA_LEN {
            bail!(
                "ecosystem.host.tickets.result_schema exceeds max length {}",
                MAX_COH_SCHEMA_LEN
            );
        }
        if tickets.max_line_bytes == 0 {
            bail!("ecosystem.host.tickets.max_line_bytes must be >= 1");
        }
        if tickets.max_line_bytes > self.secure9p.msize {
            bail!(
                "ecosystem.host.tickets.max_line_bytes {} exceeds secure9p.msize {}",
                tickets.max_line_bytes,
                self.secure9p.msize
            );
        }
        if tickets.action_allowlist.is_empty() {
            bail!("ecosystem.host.tickets.action_allowlist must not be empty");
        }
        if tickets.action_allowlist.len() > MAX_HOST_TICKET_ACTIONS {
            bail!(
                "ecosystem.host.tickets.action_allowlist exceeds max {}",
                MAX_HOST_TICKET_ACTIONS
            );
        }
        let mut seen_actions = BTreeSet::new();
        for action in &tickets.action_allowlist {
            if !seen_actions.insert(*action) {
                bail!("ecosystem.host.tickets.action_allowlist has duplicates");
            }
        }
        if tickets.lifecycle.is_empty() {
            bail!("ecosystem.host.tickets.lifecycle must not be empty");
        }
        let mut seen_states = BTreeSet::new();
        for state in &tickets.lifecycle {
            if !seen_states.insert(*state) {
                bail!("ecosystem.host.tickets.lifecycle has duplicates");
            }
        }
        for required in [
            HostTicketLifecycleState::Queued,
            HostTicketLifecycleState::Claimed,
            HostTicketLifecycleState::Running,
            HostTicketLifecycleState::Succeeded,
            HostTicketLifecycleState::Failed,
            HostTicketLifecycleState::Expired,
        ] {
            if !seen_states.contains(&required) {
                bail!(
                    "ecosystem.host.tickets.lifecycle must include state '{}'",
                    required.as_str()
                );
            }
        }
        Ok(())
    }

    fn validate_host_federation(&self) -> Result<()> {
        let federation = &self.ecosystem.host.federation;
        if !federation.enable {
            return Ok(());
        }
        if !self.ecosystem.host.tickets.enable {
            bail!("ecosystem.host.federation.enable requires ecosystem.host.tickets.enable = true");
        }
        validate_host_federation_token(
            "ecosystem.host.federation.local_hive",
            federation.local_hive.as_str(),
            MAX_HOST_FEDERATION_HIVE_LEN,
        )?;
        if federation.peers.is_empty() {
            bail!("ecosystem.host.federation.peers must not be empty");
        }
        if federation.peers.len() > MAX_HOST_FEDERATION_PEERS {
            bail!(
                "ecosystem.host.federation.peers exceeds max {}",
                MAX_HOST_FEDERATION_PEERS
            );
        }
        if federation.relay_queue_max_entries == 0 {
            bail!("ecosystem.host.federation.relay_queue_max_entries must be >= 1");
        }
        if federation.relay_queue_max_entries > MAX_HOST_FEDERATION_QUEUE_ENTRIES {
            bail!(
                "ecosystem.host.federation.relay_queue_max_entries {} exceeds max {}",
                federation.relay_queue_max_entries,
                MAX_HOST_FEDERATION_QUEUE_ENTRIES
            );
        }
        if federation.relay_queue_max_bytes == 0 {
            bail!("ecosystem.host.federation.relay_queue_max_bytes must be >= 1");
        }
        if federation.relay_queue_max_bytes > MAX_HOST_FEDERATION_QUEUE_BYTES {
            bail!(
                "ecosystem.host.federation.relay_queue_max_bytes {} exceeds max {}",
                federation.relay_queue_max_bytes,
                MAX_HOST_FEDERATION_QUEUE_BYTES
            );
        }
        if federation.relay_queue_max_bytes < self.ecosystem.host.tickets.max_line_bytes {
            bail!(
                "ecosystem.host.federation.relay_queue_max_bytes {} must be >= ecosystem.host.tickets.max_line_bytes {}",
                federation.relay_queue_max_bytes,
                self.ecosystem.host.tickets.max_line_bytes
            );
        }
        if federation.wal_max_entries == 0 {
            bail!("ecosystem.host.federation.wal_max_entries must be >= 1");
        }
        if federation.wal_max_entries > MAX_HOST_FEDERATION_WAL_ENTRIES {
            bail!(
                "ecosystem.host.federation.wal_max_entries {} exceeds max {}",
                federation.wal_max_entries,
                MAX_HOST_FEDERATION_WAL_ENTRIES
            );
        }
        if federation.wal_max_entries < u32::from(federation.relay_queue_max_entries) {
            bail!(
                "ecosystem.host.federation.wal_max_entries {} must be >= relay_queue_max_entries {}",
                federation.wal_max_entries,
                federation.relay_queue_max_entries
            );
        }
        if federation.wal_max_bytes == 0 {
            bail!("ecosystem.host.federation.wal_max_bytes must be >= 1");
        }
        if federation.wal_max_bytes > MAX_HOST_FEDERATION_WAL_BYTES {
            bail!(
                "ecosystem.host.federation.wal_max_bytes {} exceeds max {}",
                federation.wal_max_bytes,
                MAX_HOST_FEDERATION_WAL_BYTES
            );
        }
        if federation.wal_max_bytes < federation.relay_queue_max_bytes {
            bail!(
                "ecosystem.host.federation.wal_max_bytes {} must be >= relay_queue_max_bytes {}",
                federation.wal_max_bytes,
                federation.relay_queue_max_bytes
            );
        }
        if federation.relay_timeout_ms < 100 {
            bail!("ecosystem.host.federation.relay_timeout_ms must be >= 100");
        }
        if federation.relay_timeout_ms > 60_000 {
            bail!("ecosystem.host.federation.relay_timeout_ms must be <= 60000");
        }
        if federation.action_allowlist.is_empty() {
            bail!("ecosystem.host.federation.action_allowlist must not be empty");
        }
        if federation.action_allowlist.len() > MAX_HOST_TICKET_ACTIONS {
            bail!(
                "ecosystem.host.federation.action_allowlist exceeds max {}",
                MAX_HOST_TICKET_ACTIONS
            );
        }
        let mut seen_relay_actions = BTreeSet::new();
        for action in &federation.action_allowlist {
            if !seen_relay_actions.insert(*action) {
                bail!("ecosystem.host.federation.action_allowlist has duplicates");
            }
            if !self
                .ecosystem
                .host
                .tickets
                .action_allowlist
                .contains(action)
            {
                bail!(
                    "ecosystem.host.federation.action_allowlist action '{}' must also be listed in ecosystem.host.tickets.action_allowlist",
                    action.as_str()
                );
            }
        }
        let mut seen_peers = BTreeSet::new();
        for peer in &federation.peers {
            validate_host_federation_token(
                "ecosystem.host.federation.peers[].name",
                peer.name.as_str(),
                MAX_HOST_FEDERATION_HIVE_LEN,
            )?;
            if peer.name == federation.local_hive {
                bail!(
                    "ecosystem.host.federation.peers[].name '{}' must differ from local_hive",
                    peer.name
                );
            }
            if !seen_peers.insert(peer.name.as_str()) {
                bail!(
                    "ecosystem.host.federation.peers contains duplicate name '{}'",
                    peer.name
                );
            }
            let rest_url = peer.rest_url.trim();
            if rest_url.is_empty() {
                bail!("ecosystem.host.federation.peers[].rest_url must not be empty");
            }
            if rest_url.len() > MAX_HOST_FEDERATION_URL_LEN {
                bail!(
                    "ecosystem.host.federation.peers[].rest_url exceeds max length {}",
                    MAX_HOST_FEDERATION_URL_LEN
                );
            }
            if !(rest_url.starts_with("http://") || rest_url.starts_with("https://")) {
                bail!(
                    "ecosystem.host.federation.peers[].rest_url '{}' must start with http:// or https://",
                    peer.rest_url
                );
            }
            validate_host_federation_token(
                "ecosystem.host.federation.peers[].auth_ref",
                peer.auth_ref.as_str(),
                MAX_HOST_FEDERATION_AUTH_REF_LEN,
            )?;
        }
        Ok(())
    }

    fn validate_cache(&self) -> Result<()> {
        let requested =
            self.cache.dma_clean || self.cache.dma_invalidate || self.cache.unify_instructions;
        if requested && !self.cache.kernel_ops {
            bail!("cache.kernel_ops must be true when cache maintenance is requested");
        }
        Ok(())
    }

    fn validate_dma(&self) -> Result<()> {
        if self.profile_is_pi4_family()
            && self.dma.protection_profile != DmaProtectionProfile::BoundedNoIommu
        {
            bail!(
                "profile.name={} requires dma.protection_profile=bounded-no-iommu",
                self.profile.name
            );
        }

        match self.dma.protection_profile {
            DmaProtectionProfile::None | DmaProtectionProfile::BoundedNoIommu => Ok(()),
            DmaProtectionProfile::SmmuV2 | DmaProtectionProfile::SmmuV3 => {
                bail!(
                    "dma.protection_profile={} requires generated per-device DMA-domain state before isolation can be claimed",
                    self.dma.protection_profile.as_str()
                );
            }
        }
    }

    fn validate_worker_runtime(&self) -> Result<()> {
        self.worker_runtime.validate()?;
        for role in REQUIRED_WORKER_ROLE_RECORDS {
            if self.worker_runtime.role(role).is_none() {
                bail!(
                    "worker_runtime.roles missing required role record {}",
                    role.as_str()
                );
            }
        }
        if self.worker_runtime.has_implemented_roles() {
            bail!(
                "worker_runtime executable roles are unsupported until the generated contract includes image, TCB, CSpace, VSpace, IPC-buffer, stack, fault, and revocation state"
            );
        }
        Ok(())
    }

    fn validate_telemetry(&self) -> Result<()> {
        if self.telemetry.ring_bytes_per_worker == 0 {
            bail!("telemetry.ring_bytes_per_worker must be > 0");
        }
        let aggregate = self
            .telemetry
            .ring_bytes_per_worker
            .saturating_mul(EVENT_PUMP_MAX_TELEMETRY_WORKERS);
        if aggregate > EVENT_PUMP_TELEMETRY_BUDGET_BYTES {
            bail!(
                "telemetry rings {} bytes exceed event-pump budget {} bytes",
                aggregate,
                EVENT_PUMP_TELEMETRY_BUDGET_BYTES
            );
        }
        self.validate_telemetry_ingest()?;
        Ok(())
    }

    fn validate_telemetry_ingest(&self) -> Result<()> {
        let ingest = &self.telemetry_ingest;
        let zero_segments = ingest.max_segments_per_device == 0;
        let zero_segment_bytes = ingest.max_bytes_per_segment == 0;
        let zero_total_bytes = ingest.max_total_bytes_per_device == 0;
        let zero_reference_entries = ingest.max_reference_entries_per_segment == 0;
        let zero_reference_manifest_bytes = ingest.max_reference_manifest_bytes_per_segment == 0;
        let zero_reference_bytes = ingest.max_reference_bytes_per_segment == 0;
        if zero_segments
            || zero_segment_bytes
            || zero_total_bytes
            || zero_reference_entries
            || zero_reference_manifest_bytes
            || zero_reference_bytes
        {
            if zero_segments
                && zero_segment_bytes
                && zero_total_bytes
                && zero_reference_entries
                && zero_reference_manifest_bytes
                && zero_reference_bytes
            {
                return Ok(());
            }
            bail!("telemetry_ingest.* must be all zero (disabled) or all non-zero (enabled)");
        }
        if ingest.max_total_bytes_per_device < ingest.max_bytes_per_segment {
            bail!(
                "telemetry_ingest.max_total_bytes_per_device {} must be >= max_bytes_per_segment {}",
                ingest.max_total_bytes_per_device,
                ingest.max_bytes_per_segment
            );
        }
        if ingest.max_reference_entries_per_segment > MAX_TELEMETRY_REFERENCE_ENTRIES_PER_SEGMENT {
            bail!(
                "telemetry_ingest.max_reference_entries_per_segment {} exceeds max {}",
                ingest.max_reference_entries_per_segment,
                MAX_TELEMETRY_REFERENCE_ENTRIES_PER_SEGMENT
            );
        }
        if ingest.max_reference_manifest_bytes_per_segment > ingest.max_bytes_per_segment {
            bail!(
                "telemetry_ingest.max_reference_manifest_bytes_per_segment {} must be <= max_bytes_per_segment {}",
                ingest.max_reference_manifest_bytes_per_segment,
                ingest.max_bytes_per_segment
            );
        }
        if ingest.max_reference_bytes_per_segment < ingest.max_bytes_per_segment as u64 {
            bail!(
                "telemetry_ingest.max_reference_bytes_per_segment {} must be >= max_bytes_per_segment {}",
                ingest.max_reference_bytes_per_segment,
                ingest.max_bytes_per_segment
            );
        }
        Ok(())
    }

    fn validate_lifecycle(&self) -> Result<()> {
        let lifecycle = &self.lifecycle;
        if lifecycle.auto_transitions.len() > MAX_LIFECYCLE_AUTO_TRANSITIONS {
            bail!(
                "lifecycle.auto_transitions exceeds max entries {}",
                MAX_LIFECYCLE_AUTO_TRANSITIONS
            );
        }
        let mut seen = BTreeSet::new();
        for transition in &lifecycle.auto_transitions {
            if transition.from == transition.to {
                bail!(
                    "lifecycle.auto_transitions contains no-op {} -> {}",
                    transition.from.as_str(),
                    transition.to.as_str()
                );
            }
            if !matches!(
                (transition.from, transition.to),
                (LifecycleState::Booting, LifecycleState::Online)
                    | (LifecycleState::Booting, LifecycleState::Degraded)
            ) {
                bail!(
                    "lifecycle.auto_transitions contains unsupported {} -> {} (only BOOTING -> ONLINE/DEGRADED allowed)",
                    transition.from.as_str(),
                    transition.to.as_str()
                );
            }
            if !seen.insert((transition.from, transition.to)) {
                bail!(
                    "lifecycle.auto_transitions contains duplicate {} -> {}",
                    transition.from.as_str(),
                    transition.to.as_str()
                );
            }
        }
        Ok(())
    }

    fn validate_control_plane(&self) -> Result<()> {
        let msize = self.secure9p.msize;
        let schedule = &self.control_plane.schedule;
        if schedule.enable {
            if schedule.queue_max_entries == 0 {
                bail!("control_plane.schedule.queue_max_entries must be >= 1");
            }
            if schedule.queue_max_entries > MAX_SCHEDULE_QUEUE_ENTRIES {
                bail!(
                    "control_plane.schedule.queue_max_entries {} exceeds max {}",
                    schedule.queue_max_entries,
                    MAX_SCHEDULE_QUEUE_ENTRIES
                );
            }
            if schedule.ctl_max_bytes == 0 {
                bail!("control_plane.schedule.ctl_max_bytes must be >= 1");
            }
            if schedule.ctl_max_bytes > msize {
                bail!(
                    "control_plane.schedule.ctl_max_bytes {} exceeds secure9p.msize {}",
                    schedule.ctl_max_bytes,
                    msize
                );
            }
        } else if schedule.queue_max_entries != 0 || schedule.ctl_max_bytes != 0 {
            bail!(
                "control_plane.schedule must set queue_max_entries and ctl_max_bytes to 0 when disabled"
            );
        }

        let lease = &self.control_plane.lease;
        if lease.enable {
            if lease.active_max_entries == 0 {
                bail!("control_plane.lease.active_max_entries must be >= 1");
            }
            if lease.active_max_entries > MAX_LEASE_ACTIVE_ENTRIES {
                bail!(
                    "control_plane.lease.active_max_entries {} exceeds max {}",
                    lease.active_max_entries,
                    MAX_LEASE_ACTIVE_ENTRIES
                );
            }
            if lease.preemptions_max_entries == 0 {
                bail!("control_plane.lease.preemptions_max_entries must be >= 1");
            }
            if lease.preemptions_max_entries > MAX_LEASE_PREEMPTION_ENTRIES {
                bail!(
                    "control_plane.lease.preemptions_max_entries {} exceeds max {}",
                    lease.preemptions_max_entries,
                    MAX_LEASE_PREEMPTION_ENTRIES
                );
            }
            if lease.ctl_max_bytes == 0 {
                bail!("control_plane.lease.ctl_max_bytes must be >= 1");
            }
            if lease.ctl_max_bytes > msize {
                bail!(
                    "control_plane.lease.ctl_max_bytes {} exceeds secure9p.msize {}",
                    lease.ctl_max_bytes,
                    msize
                );
            }
        } else if lease.active_max_entries != 0
            || lease.preemptions_max_entries != 0
            || lease.ctl_max_bytes != 0
        {
            bail!(
                "control_plane.lease must set active_max_entries, preemptions_max_entries, and ctl_max_bytes to 0 when disabled"
            );
        }

        let export = &self.control_plane.export;
        if export.enable {
            if export.ctl_max_bytes == 0 {
                bail!("control_plane.export.ctl_max_bytes must be >= 1");
            }
            if export.ctl_max_bytes > msize {
                bail!(
                    "control_plane.export.ctl_max_bytes {} exceeds secure9p.msize {}",
                    export.ctl_max_bytes,
                    msize
                );
            }
        } else if export.ctl_max_bytes != 0 {
            bail!("control_plane.export must set ctl_max_bytes to 0 when disabled");
        }

        Ok(())
    }

    fn validate_observability(&self) -> Result<()> {
        let proc_9p = &self.observability.proc_9p;
        let shard_count = self.proc_9p_shard_count();
        if proc_9p.sessions {
            let required = required_proc_9p_sessions_bytes(shard_count);
            ensure_buffer_bytes(
                "observability.proc_9p.sessions_bytes",
                proc_9p.sessions_bytes,
                required,
            )?;
        }
        if proc_9p.outstanding {
            let required = required_proc_9p_outstanding_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p.outstanding_bytes",
                proc_9p.outstanding_bytes,
                required,
            )?;
        }
        if proc_9p.short_writes {
            let required = required_proc_9p_short_writes_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p.short_writes_bytes",
                proc_9p.short_writes_bytes,
                required,
            )?;
        }

        let proc_9p_session = &self.observability.proc_9p_session;
        if proc_9p_session.active {
            let required = required_proc_9p_session_active_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p_session.active_bytes",
                proc_9p_session.active_bytes,
                required,
            )?;
        }
        if proc_9p_session.state {
            let required = required_proc_9p_session_state_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p_session.state_bytes",
                proc_9p_session.state_bytes,
                required,
            )?;
        }
        if proc_9p_session.since_ms {
            let required = required_proc_9p_session_since_ms_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p_session.since_ms_bytes",
                proc_9p_session.since_ms_bytes,
                required,
            )?;
        }
        if proc_9p_session.owner {
            let required = required_proc_9p_session_owner_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p_session.owner_bytes",
                proc_9p_session.owner_bytes,
                required,
            )?;
        }

        let proc_ingest = &self.observability.proc_ingest;
        let ingest_enabled = proc_ingest.p50_ms
            || proc_ingest.p95_ms
            || proc_ingest.backpressure
            || proc_ingest.dropped
            || proc_ingest.queued
            || proc_ingest.watch;

        if ingest_enabled {
            if proc_ingest.latency_samples == 0 {
                bail!("observability.proc_ingest.latency_samples must be >= 1");
            }
            if proc_ingest.latency_samples > MAX_OBSERVE_LATENCY_SAMPLES {
                bail!(
                    "observability.proc_ingest.latency_samples {} exceeds max {}",
                    proc_ingest.latency_samples,
                    MAX_OBSERVE_LATENCY_SAMPLES
                );
            }
        }

        if proc_ingest.p50_ms {
            let required = required_proc_ingest_p50_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.p50_ms_bytes",
                proc_ingest.p50_ms_bytes,
                required,
            )?;
        }
        if proc_ingest.p95_ms {
            let required = required_proc_ingest_p95_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.p95_ms_bytes",
                proc_ingest.p95_ms_bytes,
                required,
            )?;
        }
        if proc_ingest.backpressure {
            let required = required_proc_ingest_backpressure_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.backpressure_bytes",
                proc_ingest.backpressure_bytes,
                required,
            )?;
        }
        if proc_ingest.dropped {
            let required = required_proc_ingest_dropped_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.dropped_bytes",
                proc_ingest.dropped_bytes,
                required,
            )?;
        }
        if proc_ingest.queued {
            let required = required_proc_ingest_queued_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.queued_bytes",
                proc_ingest.queued_bytes,
                required,
            )?;
        }
        if proc_ingest.watch {
            if !proc_ingest.p50_ms
                || !proc_ingest.p95_ms
                || !proc_ingest.backpressure
                || !proc_ingest.dropped
                || !proc_ingest.queued
            {
                bail!("observability.proc_ingest.watch requires p50_ms, p95_ms, backpressure, dropped, and queued to be enabled");
            }
            if proc_ingest.watch_max_entries == 0 {
                bail!("observability.proc_ingest.watch_max_entries must be >= 1");
            }
            if proc_ingest.watch_max_entries > MAX_OBSERVE_WATCH_ENTRIES {
                bail!(
                    "observability.proc_ingest.watch_max_entries {} exceeds max {}",
                    proc_ingest.watch_max_entries,
                    MAX_OBSERVE_WATCH_ENTRIES
                );
            }
            if proc_ingest.watch_min_interval_ms == 0 {
                bail!("observability.proc_ingest.watch_min_interval_ms must be >= 1");
            }
            let required = required_proc_ingest_watch_line_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.watch_line_bytes",
                proc_ingest.watch_line_bytes,
                required,
            )?;
        }

        let proc_root = &self.observability.proc_root;
        if proc_root.reachable {
            let required = required_proc_root_reachable_bytes();
            ensure_buffer_bytes(
                "observability.proc_root.reachable_bytes",
                proc_root.reachable_bytes,
                required,
            )?;
        }
        if proc_root.last_seen_ms {
            let required = required_proc_root_last_seen_ms_bytes();
            ensure_buffer_bytes(
                "observability.proc_root.last_seen_ms_bytes",
                proc_root.last_seen_ms_bytes,
                required,
            )?;
        }
        if proc_root.cut_reason {
            let required = required_proc_root_cut_reason_bytes();
            ensure_buffer_bytes(
                "observability.proc_root.cut_reason_bytes",
                proc_root.cut_reason_bytes,
                required,
            )?;
        }

        let proc_pressure = &self.observability.proc_pressure;
        if proc_pressure.busy {
            let required = required_proc_pressure_busy_bytes();
            ensure_buffer_bytes(
                "observability.proc_pressure.busy_bytes",
                proc_pressure.busy_bytes,
                required,
            )?;
        }
        if proc_pressure.quota {
            let required = required_proc_pressure_quota_bytes();
            ensure_buffer_bytes(
                "observability.proc_pressure.quota_bytes",
                proc_pressure.quota_bytes,
                required,
            )?;
        }
        if proc_pressure.cut {
            let required = required_proc_pressure_cut_bytes();
            ensure_buffer_bytes(
                "observability.proc_pressure.cut_bytes",
                proc_pressure.cut_bytes,
                required,
            )?;
        }
        if proc_pressure.policy {
            let required = required_proc_pressure_policy_bytes();
            ensure_buffer_bytes(
                "observability.proc_pressure.policy_bytes",
                proc_pressure.policy_bytes,
                required,
            )?;
        }

        let proc_schedule = &self.observability.proc_schedule;
        if proc_schedule.summary {
            let required = required_proc_schedule_summary_bytes();
            ensure_buffer_bytes(
                "observability.proc_schedule.summary_bytes",
                proc_schedule.summary_bytes,
                required,
            )?;
        }
        if proc_schedule.queue {
            let required = required_proc_schedule_queue_bytes();
            ensure_buffer_bytes(
                "observability.proc_schedule.queue_bytes",
                proc_schedule.queue_bytes,
                required,
            )?;
        }

        let proc_lease = &self.observability.proc_lease;
        if proc_lease.summary {
            let required = required_proc_lease_summary_bytes();
            ensure_buffer_bytes(
                "observability.proc_lease.summary_bytes",
                proc_lease.summary_bytes,
                required,
            )?;
        }
        if proc_lease.active {
            let required = required_proc_lease_active_bytes();
            ensure_buffer_bytes(
                "observability.proc_lease.active_bytes",
                proc_lease.active_bytes,
                required,
            )?;
        }
        if proc_lease.preemptions {
            let required = required_proc_lease_preemptions_bytes();
            ensure_buffer_bytes(
                "observability.proc_lease.preemptions_bytes",
                proc_lease.preemptions_bytes,
                required,
            )?;
        }
        Ok(())
    }

    fn validate_ui_providers(&self) -> Result<()> {
        let ui = &self.ui_providers;
        let proc_9p = &self.observability.proc_9p;
        let proc_ingest = &self.observability.proc_ingest;
        if ui.proc_9p.sessions && !proc_9p.sessions {
            bail!("ui_providers.proc_9p.sessions requires observability.proc_9p.sessions = true");
        }
        if ui.proc_9p.outstanding && !proc_9p.outstanding {
            bail!(
                "ui_providers.proc_9p.outstanding requires observability.proc_9p.outstanding = true"
            );
        }
        if ui.proc_9p.short_writes && !proc_9p.short_writes {
            bail!(
                "ui_providers.proc_9p.short_writes requires observability.proc_9p.short_writes = true"
            );
        }
        if ui.proc_ingest.p50_ms && !proc_ingest.p50_ms {
            bail!(
                "ui_providers.proc_ingest.p50_ms requires observability.proc_ingest.p50_ms = true"
            );
        }
        if ui.proc_ingest.p95_ms && !proc_ingest.p95_ms {
            bail!(
                "ui_providers.proc_ingest.p95_ms requires observability.proc_ingest.p95_ms = true"
            );
        }
        if ui.proc_ingest.backpressure && !proc_ingest.backpressure {
            bail!(
                "ui_providers.proc_ingest.backpressure requires observability.proc_ingest.backpressure = true"
            );
        }
        if (ui.policy_preflight.req || ui.policy_preflight.diff) && !self.ecosystem.policy.enable {
            bail!("ui_providers.policy_preflight requires ecosystem.policy.enable = true");
        }
        if (ui.updates.manifest || ui.updates.status) && !self.cas.enable {
            bail!("ui_providers.updates requires cas.enable = true");
        }
        if self.cas.enable && !ui.updates.manifest {
            bail!("ui_providers.updates.manifest must be true when cas.enable = true");
        }
        Ok(())
    }

    fn proc_9p_shard_count(&self) -> usize {
        if self.sharding.enabled {
            1usize << self.sharding.shard_bits
        } else {
            1
        }
    }

    fn validate_client_policies(&self) -> Result<()> {
        let pool = &self.client_policies.cohsh.pool;
        if pool.control_sessions == 0 {
            bail!("client_policies.cohsh.pool.control_sessions must be >= 1");
        }
        if pool.telemetry_sessions == 0 {
            bail!("client_policies.cohsh.pool.telemetry_sessions must be >= 1");
        }
        let tail = &self.client_policies.cohsh.tail;
        if tail.poll_ms_min == 0 {
            bail!("client_policies.cohsh.tail.poll_ms_min must be >= 1");
        }
        if tail.poll_ms_max < tail.poll_ms_min {
            bail!(
                "client_policies.cohsh.tail.poll_ms_max {} must be >= poll_ms_min {}",
                tail.poll_ms_max,
                tail.poll_ms_min
            );
        }
        if tail.poll_ms_default < tail.poll_ms_min || tail.poll_ms_default > tail.poll_ms_max {
            bail!(
                "client_policies.cohsh.tail.poll_ms_default {} must be within {}..={}",
                tail.poll_ms_default,
                tail.poll_ms_min,
                tail.poll_ms_max
            );
        }
        let host_telemetry = &self.client_policies.cohsh.host_telemetry;
        if host_telemetry.nvidia_poll_ms == 0 {
            bail!("client_policies.cohsh.host_telemetry.nvidia_poll_ms must be >= 1");
        }
        if host_telemetry.systemd_poll_ms == 0 {
            bail!("client_policies.cohsh.host_telemetry.systemd_poll_ms must be >= 1");
        }
        if host_telemetry.docker_poll_ms == 0 {
            bail!("client_policies.cohsh.host_telemetry.docker_poll_ms must be >= 1");
        }
        if host_telemetry.k8s_poll_ms == 0 {
            bail!("client_policies.cohsh.host_telemetry.k8s_poll_ms must be >= 1");
        }
        let retry = &self.client_policies.retry;
        if retry.max_attempts == 0 {
            bail!("client_policies.retry.max_attempts must be >= 1");
        }
        if retry.backoff_ms == 0 {
            bail!("client_policies.retry.backoff_ms must be >= 1");
        }
        if retry.ceiling_ms < retry.backoff_ms {
            bail!(
                "client_policies.retry.ceiling_ms {} must be >= backoff_ms {}",
                retry.ceiling_ms,
                retry.backoff_ms
            );
        }
        if retry.timeout_ms == 0 {
            bail!("client_policies.retry.timeout_ms must be >= 1");
        }
        let heartbeat = &self.client_policies.heartbeat;
        if heartbeat.interval_ms == 0 {
            bail!("client_policies.heartbeat.interval_ms must be >= 1");
        }
        let trace = &self.client_policies.trace;
        if trace.max_bytes == 0 {
            bail!("client_policies.trace.max_bytes must be > 0");
        }
        self.validate_coh_policy()?;
        Ok(())
    }

    fn validate_client_paths(&self) -> Result<()> {
        self.validate_client_path("client_paths.queen_ctl", &self.client_paths.queen_ctl)?;
        self.validate_client_path(
            "client_paths.queen_lifecycle_ctl",
            &self.client_paths.queen_lifecycle_ctl,
        )?;
        self.validate_client_path(
            "client_paths.queen_schedule_ctl",
            &self.client_paths.queen_schedule_ctl,
        )?;
        self.validate_client_path(
            "client_paths.queen_lease_ctl",
            &self.client_paths.queen_lease_ctl,
        )?;
        self.validate_client_path(
            "client_paths.queen_export_ctl",
            &self.client_paths.queen_export_ctl,
        )?;
        self.validate_client_path("client_paths.policy_ctl", &self.client_paths.policy_ctl)?;
        self.validate_client_path("client_paths.log", &self.client_paths.log)?;
        Ok(())
    }

    fn validate_coh_policy(&self) -> Result<()> {
        let mount = &self.client_policies.coh.mount;
        self.validate_coh_path("client_policies.coh.mount.root", &mount.root, true)?;
        if mount.allowlist.is_empty() {
            bail!("client_policies.coh.mount.allowlist must not be empty");
        }
        if mount.allowlist.len() > MAX_COH_ALLOWLIST {
            bail!(
                "client_policies.coh.mount.allowlist exceeds max entries {}",
                MAX_COH_ALLOWLIST
            );
        }
        for path in &mount.allowlist {
            self.validate_coh_path("client_policies.coh.mount.allowlist", path, false)?;
        }
        let telemetry = &self.client_policies.coh.telemetry;
        self.validate_coh_path("client_policies.coh.telemetry.root", &telemetry.root, false)?;
        if telemetry.max_devices == 0 {
            bail!("client_policies.coh.telemetry.max_devices must be >= 1");
        }
        if telemetry.max_devices > MAX_COH_TELEMETRY_DEVICES {
            bail!(
                "client_policies.coh.telemetry.max_devices {} exceeds max {}",
                telemetry.max_devices,
                MAX_COH_TELEMETRY_DEVICES
            );
        }
        if telemetry.max_segments_per_device == 0
            || telemetry.max_bytes_per_segment == 0
            || telemetry.max_total_bytes_per_device == 0
        {
            bail!(
                "client_policies.coh.telemetry.* must be >= 1 (segments, bytes per segment, total bytes)"
            );
        }
        if telemetry.max_segments_per_device > self.telemetry_ingest.max_segments_per_device {
            bail!(
                "client_policies.coh.telemetry.max_segments_per_device {} exceeds telemetry_ingest.max_segments_per_device {}",
                telemetry.max_segments_per_device,
                self.telemetry_ingest.max_segments_per_device
            );
        }
        if telemetry.max_bytes_per_segment > self.telemetry_ingest.max_bytes_per_segment {
            bail!(
                "client_policies.coh.telemetry.max_bytes_per_segment {} exceeds telemetry_ingest.max_bytes_per_segment {}",
                telemetry.max_bytes_per_segment,
                self.telemetry_ingest.max_bytes_per_segment
            );
        }
        if telemetry.max_total_bytes_per_device > self.telemetry_ingest.max_total_bytes_per_device {
            bail!(
                "client_policies.coh.telemetry.max_total_bytes_per_device {} exceeds telemetry_ingest.max_total_bytes_per_device {}",
                telemetry.max_total_bytes_per_device,
                self.telemetry_ingest.max_total_bytes_per_device
            );
        }
        let run = &self.client_policies.coh.run;
        let lease = &run.lease;
        if lease.schema.trim().is_empty() {
            bail!("client_policies.coh.run.lease.schema must not be empty");
        }
        if lease.schema.len() > MAX_COH_SCHEMA_LEN {
            bail!(
                "client_policies.coh.run.lease.schema exceeds max len {}",
                MAX_COH_SCHEMA_LEN
            );
        }
        if lease.active_state.trim().is_empty() {
            bail!("client_policies.coh.run.lease.active_state must not be empty");
        }
        if lease.active_state.len() > MAX_COH_LEASE_STATE_LEN {
            bail!(
                "client_policies.coh.run.lease.active_state exceeds max len {}",
                MAX_COH_LEASE_STATE_LEN
            );
        }
        if lease.max_bytes == 0 {
            bail!("client_policies.coh.run.lease.max_bytes must be >= 1");
        }
        if lease.max_bytes > self.secure9p.msize {
            bail!(
                "client_policies.coh.run.lease.max_bytes {} exceeds secure9p.msize {}",
                lease.max_bytes,
                self.secure9p.msize
            );
        }
        let breadcrumb = &run.breadcrumb;
        if breadcrumb.schema.trim().is_empty() {
            bail!("client_policies.coh.run.breadcrumb.schema must not be empty");
        }
        if breadcrumb.schema.len() > MAX_COH_SCHEMA_LEN {
            bail!(
                "client_policies.coh.run.breadcrumb.schema exceeds max len {}",
                MAX_COH_SCHEMA_LEN
            );
        }
        if breadcrumb.max_line_bytes == 0 {
            bail!("client_policies.coh.run.breadcrumb.max_line_bytes must be >= 1");
        }
        if breadcrumb.max_line_bytes > self.secure9p.msize {
            bail!(
                "client_policies.coh.run.breadcrumb.max_line_bytes {} exceeds secure9p.msize {}",
                breadcrumb.max_line_bytes,
                self.secure9p.msize
            );
        }
        if breadcrumb.max_command_bytes == 0 {
            bail!("client_policies.coh.run.breadcrumb.max_command_bytes must be >= 1");
        }
        if breadcrumb.max_command_bytes > breadcrumb.max_line_bytes {
            bail!(
                "client_policies.coh.run.breadcrumb.max_command_bytes {} exceeds max_line_bytes {}",
                breadcrumb.max_command_bytes,
                breadcrumb.max_line_bytes
            );
        }
        let peft = &self.client_policies.coh.peft;
        self.validate_coh_path(
            "client_policies.coh.peft.export.root",
            &peft.export.root,
            false,
        )?;
        if peft.export.max_telemetry_bytes == 0 {
            bail!("client_policies.coh.peft.export.max_telemetry_bytes must be >= 1");
        }
        if peft.export.max_telemetry_bytes > self.telemetry_ingest.max_total_bytes_per_device {
            bail!(
                "client_policies.coh.peft.export.max_telemetry_bytes {} exceeds telemetry_ingest.max_total_bytes_per_device {}",
                peft.export.max_telemetry_bytes,
                self.telemetry_ingest.max_total_bytes_per_device
            );
        }
        if peft.export.max_policy_bytes == 0 {
            bail!("client_policies.coh.peft.export.max_policy_bytes must be >= 1");
        }
        if peft.export.max_base_model_bytes == 0 {
            bail!("client_policies.coh.peft.export.max_base_model_bytes must be >= 1");
        }
        self.validate_host_path(
            "client_policies.coh.peft.import.registry_root",
            &peft.import.registry_root,
        )?;
        if peft.import.max_adapter_bytes == 0 {
            bail!("client_policies.coh.peft.import.max_adapter_bytes must be >= 1");
        }
        if peft.import.max_lora_bytes == 0 {
            bail!("client_policies.coh.peft.import.max_lora_bytes must be >= 1");
        }
        if peft.import.max_metrics_bytes == 0 {
            bail!("client_policies.coh.peft.import.max_metrics_bytes must be >= 1");
        }
        if peft.import.max_manifest_bytes == 0 {
            bail!("client_policies.coh.peft.import.max_manifest_bytes must be >= 1");
        }
        if peft.activate.max_model_id_bytes == 0 {
            bail!("client_policies.coh.peft.activate.max_model_id_bytes must be >= 1");
        }
        if peft.activate.max_model_id_bytes > MAX_COH_PEFT_ID_LEN {
            bail!(
                "client_policies.coh.peft.activate.max_model_id_bytes {} exceeds max {}",
                peft.activate.max_model_id_bytes,
                MAX_COH_PEFT_ID_LEN
            );
        }
        if peft.activate.max_model_id_bytes.saturating_add(1) > self.secure9p.msize {
            bail!(
                "client_policies.coh.peft.activate.max_model_id_bytes {} exceeds secure9p.msize {}",
                peft.activate.max_model_id_bytes,
                self.secure9p.msize
            );
        }
        if peft.activate.max_state_bytes == 0 {
            bail!("client_policies.coh.peft.activate.max_state_bytes must be >= 1");
        }
        Ok(())
    }

    fn validate_coh_path(&self, label: &str, path: &str, allow_root: bool) -> Result<()> {
        let trimmed = path.trim();
        if !trimmed.starts_with('/') {
            bail!("{label} must be an absolute path");
        }
        let components: Vec<&str> = trimmed.split('/').filter(|seg| !seg.is_empty()).collect();
        if components.is_empty() && !allow_root {
            bail!("{label} must not be root");
        }
        if components.len() > MAX_WALK_DEPTH {
            bail!("{label} exceeds walk depth {}", MAX_WALK_DEPTH);
        }
        for component in components {
            if component == ".." {
                bail!("{label} contains disallowed '..'");
            }
            if component.is_empty() {
                bail!("{label} contains empty path component");
            }
        }
        Ok(())
    }

    fn validate_host_path(&self, label: &str, path: &str) -> Result<()> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            bail!("{label} must not be empty");
        }
        if trimmed.as_bytes().contains(&0) {
            bail!("{label} contains NUL byte");
        }
        let components: Vec<&str> = trimmed.split('/').filter(|seg| !seg.is_empty()).collect();
        if components.len() > MAX_WALK_DEPTH {
            bail!("{label} exceeds max depth {}", MAX_WALK_DEPTH);
        }
        for component in components {
            if component == "." || component == ".." {
                bail!("{label} contains disallowed component '{component}'");
            }
        }
        Ok(())
    }

    fn validate_swarmui(&self) -> Result<()> {
        let swarmui = &self.swarmui;
        if swarmui.cache.max_bytes == 0 {
            bail!("swarmui.cache.max_bytes must be > 0");
        }
        if swarmui.cache.ttl_s == 0 {
            bail!("swarmui.cache.ttl_s must be > 0");
        }
        let hive = &swarmui.hive;
        if hive.frame_cap_fps < 30 || hive.frame_cap_fps > 60 {
            bail!("swarmui.hive.frame_cap_fps must be within 30..=60");
        }
        if hive.step_ms == 0 {
            bail!("swarmui.hive.step_ms must be > 0");
        }
        if hive.lod_zoom_out <= 0.0 || hive.lod_zoom_out >= 1.0 {
            bail!("swarmui.hive.lod_zoom_out must be within (0.0, 1.0)");
        }
        if hive.lod_zoom_in <= hive.lod_zoom_out {
            bail!("swarmui.hive.lod_zoom_in must be > lod_zoom_out");
        }
        if hive.lod_event_budget == 0 {
            bail!("swarmui.hive.lod_event_budget must be > 0");
        }
        if hive.snapshot_max_events == 0 {
            bail!("swarmui.hive.snapshot_max_events must be > 0");
        }
        if hive.overlay_lines == 0 {
            bail!("swarmui.hive.overlay_lines must be > 0");
        }
        if hive.detail_lines == 0 {
            bail!("swarmui.hive.detail_lines must be > 0");
        }
        if hive.detail_lines < hive.overlay_lines {
            bail!("swarmui.hive.detail_lines must be >= overlay_lines");
        }
        if hive.line_cap_bytes == 0 {
            bail!("swarmui.hive.line_cap_bytes must be > 0");
        }
        if hive.line_cap_bytes as usize > MAX_LINE_LEN {
            bail!(
                "swarmui.hive.line_cap_bytes {} exceeds max {}",
                hive.line_cap_bytes,
                MAX_LINE_LEN
            );
        }
        if hive.per_worker_bytes == 0 {
            bail!("swarmui.hive.per_worker_bytes must be > 0");
        }
        if hive.per_worker_bytes < hive.line_cap_bytes {
            bail!("swarmui.hive.per_worker_bytes must be >= line_cap_bytes");
        }
        if hive.pending_lines_per_worker == 0 {
            bail!("swarmui.hive.pending_lines_per_worker must be > 0");
        }
        if hive.pending_event_cap == 0 {
            bail!("swarmui.hive.pending_event_cap must be > 0");
        }
        if hive.pending_event_cap < hive.lod_event_budget {
            bail!("swarmui.hive.pending_event_cap must be >= lod_event_budget");
        }
        if hive.poll_workers_per_tick == 0 {
            bail!("swarmui.hive.poll_workers_per_tick must be > 0");
        }
        if hive.status_poll_ms == 0 {
            bail!("swarmui.hive.status_poll_ms must be > 0");
        }
        if hive.degrade_pressure <= 0.0 {
            bail!("swarmui.hive.degrade_pressure must be > 0");
        }
        self.validate_client_path(
            "swarmui.paths.telemetry_root",
            &swarmui.paths.telemetry_root,
        )?;
        self.validate_client_path(
            "swarmui.paths.proc_ingest_root",
            &swarmui.paths.proc_ingest_root,
        )?;
        self.validate_client_path("swarmui.paths.worker_root", &swarmui.paths.worker_root)?;
        if swarmui.paths.namespace_roots.is_empty() {
            bail!("swarmui.paths.namespace_roots must not be empty");
        }
        for (idx, path) in swarmui.paths.namespace_roots.iter().enumerate() {
            let label = format!("swarmui.paths.namespace_roots[{idx}]");
            self.validate_client_path(&label, path)?;
        }
        Ok(())
    }

    fn validate_client_path(&self, label: &str, path: &str) -> Result<()> {
        if !path.starts_with('/') {
            bail!("{label} must be an absolute path");
        }
        let mut depth = 0usize;
        for component in path.split('/').skip(1) {
            if component.is_empty() {
                continue;
            }
            if component == "." || component == ".." {
                bail!("{label} contains disallowed path component '{component}'");
            }
            if component.as_bytes().contains(&0) {
                bail!("{label} contains NUL byte");
            }
            depth = depth.saturating_add(1);
            if depth > self.secure9p.walk_depth as usize {
                bail!(
                    "{label} exceeds secure9p.walk_depth {}",
                    self.secure9p.walk_depth
                );
            }
        }
        if depth == 0 {
            bail!("{label} must not be empty");
        }
        Ok(())
    }

    fn validate_cas(&self, base_dir: Option<&Path>) -> Result<()> {
        if self.ecosystem.models.enable && !self.cas.enable {
            bail!("ecosystem.models.enable requires cas.enable = true");
        }
        if !self.cas.enable {
            return Ok(());
        }
        if self.cas.store.chunk_bytes == 0 {
            bail!("cas.store.chunk_bytes must be > 0");
        }
        if self.cas.store.chunk_bytes > self.secure9p.msize {
            bail!(
                "cas.store.chunk_bytes {} exceeds secure9p.msize {}",
                self.cas.store.chunk_bytes,
                self.secure9p.msize
            );
        }
        let required = self.cas.store.chunk_bytes.saturating_mul(CAS_MAX_CHUNKS);
        if required > EVENT_PUMP_CAS_BUDGET_BYTES {
            bail!(
                "cas.store.chunk_bytes {} with max_chunks {} exceeds event-pump budget {}",
                self.cas.store.chunk_bytes,
                CAS_MAX_CHUNKS,
                EVENT_PUMP_CAS_BUDGET_BYTES
            );
        }
        if self.cas.delta.enable && !self.cas.enable {
            bail!("cas.delta.enable requires cas.enable = true");
        }
        let signing = self.cas.signing.as_ref().ok_or_else(|| {
            anyhow::anyhow!("cas.signing section required when cas.enable = true")
        })?;
        if signing.required {
            let key_path = signing
                .key_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("cas.signing.key_path required when signing.required = true")
                })?;
            let resolved = resolve_manifest_relative_path(base_dir, key_path);
            let key_bytes = fs::read(&resolved).with_context(|| {
                format!("failed to read cas signing key {}", resolved.display())
            })?;
            let key_text = std::str::from_utf8(&key_bytes).with_context(|| {
                format!("cas signing key {} is not valid UTF-8", resolved.display())
            })?;
            let key_text = key_text.trim();
            if key_text.is_empty() {
                bail!("cas signing key {} is empty", resolved.display());
            }
            let raw = hex::decode(key_text).map_err(|err| {
                anyhow::anyhow!("cas signing key {} must be hex: {err}", resolved.display())
            })?;
            if raw.len() != 32 {
                bail!(
                    "cas signing key {} must be 32 bytes (got {})",
                    resolved.display(),
                    raw.len()
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootTaskSection {
    pub schema: String,
    #[serde(default)]
    pub affinity: AffinityPolicy,
    #[serde(default)]
    pub driver_images: DriverRuntimeImagePolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DriverRuntimeImagePolicy {
    pub required: bool,
    pub images: Vec<DriverRuntimeImageSpec>,
    pub irqs: Vec<DriverRuntimeIrqSpec>,
    pub bus_links: Vec<DriverRuntimeBusLinkSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default, rename_all = "kebab-case")]
pub struct DriverRuntimeImageSpec {
    pub id: String,
    pub contract: String,
    pub hot_path: String,
    pub artifact: String,
    pub entry_symbol: String,
    pub code_pages: u16,
    pub stack_pages: u16,
    pub ipc_pages: u16,
    pub ring_pages: u16,
    pub mmio_pages: u16,
    pub dma_pages: u16,
    pub shared_buffer_pages: u16,
    pub root_context_required: bool,
    pub hardware_state_migrated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum DriverRuntimeIrqTrigger {
    Level,
    Edge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DriverRuntimeIrqSpec {
    pub hot_path: String,
    pub irq: u32,
    pub badge: u32,
    pub handler_slot: u8,
    pub notification_slot: u8,
    pub trigger: DriverRuntimeIrqTrigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DriverRuntimeBusLinkSpec {
    pub channel: String,
    pub client_hot_path: String,
    pub owner_hot_path: String,
    pub client_notification_slot: u8,
    pub owner_notification_slot: u8,
    pub client_to_owner_slot: u8,
    pub owner_to_client_slot: u8,
    pub shared_offset: u32,
    pub shared_len: u32,
    pub link_epoch: u32,
    pub event_offset: u16,
    pub event_len: u16,
    pub event_depth: u16,
}

impl DriverRuntimeImagePolicy {
    fn validate(&self) -> Result<()> {
        if self.images.len() > MAX_DRIVER_RUNTIME_IMAGES {
            bail!(
                "root_task.driver_images.images contains {} entries, max {}",
                self.images.len(),
                MAX_DRIVER_RUNTIME_IMAGES
            );
        }
        let mut ids = BTreeSet::new();
        let mut contracts = BTreeSet::new();
        let mut hot_paths = BTreeSet::new();
        for image in &self.images {
            image.validate()?;
            if !ids.insert(image.id.as_str()) {
                bail!("duplicate driver runtime image id {}", image.id);
            }
            if !contracts.insert(image.contract.as_str()) {
                bail!("duplicate driver runtime image contract {}", image.contract);
            }
            if !hot_paths.insert(image.hot_path.as_str()) {
                bail!("duplicate driver runtime image hot path {}", image.hot_path);
            }
        }
        if self.required {
            for required in REQUIRED_PI4_DRIVER_RUNTIME_HOT_PATHS {
                if !hot_paths.contains(required) {
                    bail!(
                        "root_task.driver_images.required missing hot path {}",
                        required
                    );
                }
            }
        }
        if self.irqs.len() > MAX_DRIVER_RUNTIME_IRQS {
            bail!(
                "root_task.driver_images.irqs contains {} entries, max {}",
                self.irqs.len(),
                MAX_DRIVER_RUNTIME_IRQS
            );
        }
        let mut irq_sources = BTreeSet::new();
        for irq in &self.irqs {
            irq.validate(&hot_paths)?;
            if !irq_sources.insert((irq.hot_path.as_str(), irq.irq)) {
                bail!(
                    "duplicate driver runtime IRQ {} for hot path {}",
                    irq.irq,
                    irq.hot_path
                );
            }
        }
        if self.bus_links.len() > MAX_DRIVER_RUNTIME_BUS_LINKS {
            bail!(
                "root_task.driver_images.bus_links contains {} entries, max {}",
                self.bus_links.len(),
                MAX_DRIVER_RUNTIME_BUS_LINKS
            );
        }
        let mut bus_link_channels = BTreeSet::new();
        for link in &self.bus_links {
            link.validate(&hot_paths, &self.irqs)?;
            if !bus_link_channels.insert(link.channel.as_str()) {
                bail!("duplicate driver runtime bus-link channel {}", link.channel);
            }
        }
        if self.required {
            let has_sdio_irq = self
                .irqs
                .iter()
                .any(DriverRuntimeIrqSpec::is_cyw43_sdio_irq);
            if !has_sdio_irq {
                bail!("root_task.driver_images.required missing SDIO IRQ 158 topology");
            }
            let has_cyw43_sdio_link = self
                .bus_links
                .iter()
                .any(DriverRuntimeBusLinkSpec::is_cyw43_sdio_dpc_link);
            if !has_cyw43_sdio_link {
                bail!(
                    "root_task.driver_images.required missing reciprocal cyw43-sdio notification topology"
                );
            }
        }
        Ok(())
    }
}

impl DriverRuntimeIrqSpec {
    fn validate(&self, hot_paths: &BTreeSet<&str>) -> Result<()> {
        validate_driver_runtime_text(
            "irqs.hot-path",
            &self.hot_path,
            MAX_DRIVER_RUNTIME_IMAGE_ID_LEN,
        )?;
        if !hot_paths.contains(self.hot_path.as_str()) {
            bail!(
                "driver runtime IRQ references unknown hot path {}",
                self.hot_path
            );
        }
        if self.irq == 0 || self.badge == 0 {
            bail!(
                "driver runtime IRQ for {} must use nonzero irq and badge",
                self.hot_path
            );
        }
        for (field, slot) in [
            ("handler-slot", self.handler_slot),
            ("notification-slot", self.notification_slot),
        ] {
            if slot == 0 || slot >= DRIVER_RUNTIME_CHILD_CSPACE_SLOTS {
                bail!(
                    "driver runtime IRQ {} {} must be within child CSpace slots 1..{}",
                    self.hot_path,
                    field,
                    DRIVER_RUNTIME_CHILD_CSPACE_SLOTS - 1
                );
            }
        }
        if self.handler_slot == self.notification_slot {
            bail!(
                "driver runtime IRQ {} handler and notification slots must differ",
                self.hot_path
            );
        }
        Ok(())
    }

    fn is_cyw43_sdio_irq(&self) -> bool {
        self.hot_path == "sdio-host"
            && self.irq == DRIVER_RUNTIME_CYW43_SDIO_IRQ
            && self.badge == DRIVER_RUNTIME_CYW43_SDIO_BADGE
            && self.notification_slot == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
            && self.trigger == DriverRuntimeIrqTrigger::Level
    }
}

impl DriverRuntimeBusLinkSpec {
    fn validate(&self, hot_paths: &BTreeSet<&str>, irqs: &[DriverRuntimeIrqSpec]) -> Result<()> {
        for (field, value) in [
            ("bus-links.channel", self.channel.as_str()),
            ("bus-links.client-hot-path", self.client_hot_path.as_str()),
            ("bus-links.owner-hot-path", self.owner_hot_path.as_str()),
        ] {
            validate_driver_runtime_text(field, value, MAX_DRIVER_RUNTIME_IMAGE_ID_LEN)?;
        }
        if !hot_paths.contains(self.client_hot_path.as_str())
            || !hot_paths.contains(self.owner_hot_path.as_str())
        {
            bail!(
                "driver runtime bus link {} references unknown client or owner hot path",
                self.channel
            );
        }
        if self.client_hot_path == self.owner_hot_path {
            bail!(
                "driver runtime bus link {} client and owner must differ",
                self.channel
            );
        }
        if self.link_epoch == 0 {
            bail!(
                "driver runtime bus link {} shared epoch must be nonzero",
                self.channel
            );
        }
        for (field, slot) in [
            ("client-notification-slot", self.client_notification_slot),
            ("owner-notification-slot", self.owner_notification_slot),
            ("client-to-owner-slot", self.client_to_owner_slot),
            ("owner-to-client-slot", self.owner_to_client_slot),
        ] {
            if slot == 0 || slot >= DRIVER_RUNTIME_CHILD_CSPACE_SLOTS {
                bail!(
                    "driver runtime bus link {} {} must be within child CSpace slots 1..{}",
                    self.channel,
                    field,
                    DRIVER_RUNTIME_CHILD_CSPACE_SLOTS - 1
                );
            }
        }
        if self.client_notification_slot == self.client_to_owner_slot
            || self.owner_notification_slot == self.owner_to_client_slot
        {
            bail!(
                "driver runtime bus link {} local and peer notification slots must differ",
                self.channel
            );
        }
        let event_end = u32::from(self.event_offset).saturating_add(u32::from(self.event_len));
        if self.event_len == 0
            || self.event_depth == 0
            || event_end > u32::from(DRIVER_RUNTIME_RING_FRAME_OFFSET)
        {
            bail!(
                "driver runtime bus link {} DPC event ring must be nonzero and end before offset {}",
                self.channel,
                DRIVER_RUNTIME_RING_FRAME_OFFSET
            );
        }
        if self.channel == "cyw43-sdio" {
            if !self.is_cyw43_sdio_dpc_link() {
                bail!("cyw43-sdio bus link does not match the bounded reciprocal DPC contract");
            }
            if !irqs.iter().any(DriverRuntimeIrqSpec::is_cyw43_sdio_irq) {
                bail!("cyw43-sdio bus link requires the generated level-triggered SDIO IRQ 158");
            }
        } else {
            bail!("unknown driver runtime bus-link channel {}", self.channel);
        }
        Ok(())
    }

    fn is_cyw43_sdio_dpc_link(&self) -> bool {
        self.channel == "cyw43-sdio"
            && self.client_hot_path == "cyw43-wifi"
            && self.owner_hot_path == "sdio-host"
            && self.client_notification_slot == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
            && self.owner_notification_slot == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
            && self.client_to_owner_slot == DRIVER_RUNTIME_CYW43_SDIO_CLIENT_TO_OWNER_SLOT
            && self.owner_to_client_slot == DRIVER_RUNTIME_CYW43_SDIO_OWNER_TO_CLIENT_SLOT
            && self.shared_offset == DRIVER_RUNTIME_CYW43_SDIO_SHARED_OFFSET
            && self.shared_len == DRIVER_RUNTIME_CYW43_SDIO_SHARED_LEN
            && self.link_epoch == DRIVER_RUNTIME_CYW43_SDIO_LINK_EPOCH
            && self.event_offset == DRIVER_RUNTIME_DPC_EVENT_OFFSET
            && self.event_len == DRIVER_RUNTIME_DPC_EVENT_LEN
            && self.event_depth == DRIVER_RUNTIME_DPC_EVENT_DEPTH
    }
}

impl DriverRuntimeImageSpec {
    fn validate(&self) -> Result<()> {
        validate_driver_runtime_text("id", &self.id, MAX_DRIVER_RUNTIME_IMAGE_ID_LEN)?;
        validate_driver_runtime_text("contract", &self.contract, MAX_DRIVER_RUNTIME_IMAGE_ID_LEN)?;
        validate_driver_runtime_text("hot_path", &self.hot_path, MAX_DRIVER_RUNTIME_IMAGE_ID_LEN)?;
        validate_driver_runtime_text(
            "artifact",
            &self.artifact,
            MAX_DRIVER_RUNTIME_IMAGE_PATH_LEN,
        )?;
        validate_driver_runtime_text(
            "entry_symbol",
            &self.entry_symbol,
            MAX_DRIVER_RUNTIME_ENTRY_SYMBOL_LEN,
        )?;
        if !REQUIRED_PI4_DRIVER_RUNTIME_HOT_PATHS.contains(&self.hot_path.as_str()) {
            bail!("unknown driver runtime hot path {}", self.hot_path);
        }
        for (name, pages) in [
            ("code_pages", self.code_pages),
            ("stack_pages", self.stack_pages),
            ("ipc_pages", self.ipc_pages),
            ("ring_pages", self.ring_pages),
            ("mmio_pages", self.mmio_pages),
            ("dma_pages", self.dma_pages),
            ("shared_buffer_pages", self.shared_buffer_pages),
        ] {
            if pages > MAX_DRIVER_RUNTIME_REGION_PAGES {
                bail!(
                    "root_task.driver_images.images.{} for {} exceeds max {}",
                    name,
                    self.id,
                    MAX_DRIVER_RUNTIME_REGION_PAGES
                );
            }
        }
        if self.code_pages == 0
            || self.stack_pages == 0
            || self.ipc_pages == 0
            || self.ring_pages == 0
            || self.shared_buffer_pages == 0
        {
            bail!(
                "driver runtime image {} must declare nonzero code, stack, ipc, ring, and shared-buffer pages",
                self.id
            );
        }
        if self.hot_path == "sdio-host" && (self.mmio_pages != 2 || self.dma_pages != 1) {
            bail!(
                "driver runtime image {} for sdio-host must declare exactly 2 mmio pages and 1 dma page for SDHCI plus WiFi pwrseq",
                self.id
            );
        }
        Ok(())
    }
}

impl Default for DriverRuntimeImageSpec {
    fn default() -> Self {
        Self {
            id: String::new(),
            contract: String::new(),
            hot_path: String::new(),
            artifact: String::new(),
            entry_symbol: String::new(),
            code_pages: 1,
            stack_pages: 1,
            ipc_pages: 1,
            ring_pages: 1,
            mmio_pages: 0,
            dma_pages: 0,
            shared_buffer_pages: 1,
            root_context_required: true,
            hardware_state_migrated: false,
        }
    }
}

fn validate_driver_runtime_text(field: &str, value: &str, max_len: usize) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("root_task.driver_images.images.{field} must not be empty");
    }
    if trimmed.len() > max_len {
        bail!(
            "root_task.driver_images.images.{field} exceeds max length {}",
            max_len
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AffinityPolicy {
    pub enabled: bool,
    pub max_cores: u8,
    pub authority_core: Option<u8>,
    pub ninedoor_cores: Vec<u8>,
    pub provider_cores: Vec<u8>,
    pub worker_cores: Vec<u8>,
    pub drivers: DriverAffinityPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default, rename_all = "kebab-case")]
pub struct DriverAffinityPolicy {
    pub serial: Option<u8>,
    pub usb_local_seat: Option<u8>,
    pub hdmi_text: Option<u8>,
    pub bcmgenet_v5: Option<u8>,
    pub cyw43455: Option<u8>,
    pub rtl8139: Option<u8>,
    pub virtio_net: Option<u8>,
    pub sdio_host: Option<u8>,
    pub pcie_root: Option<u8>,
}

impl DriverAffinityPolicy {
    fn validate(&self, root_core: u8, max_core: u8) -> Result<()> {
        for (name, core) in self.entries() {
            let Some(core) = core else {
                continue;
            };
            if core >= max_core {
                bail!(
                    "root_task.affinity.drivers.{} contains {} which exceeds max_core {}",
                    name,
                    core,
                    max_core
                );
            }
            if core == root_core {
                bail!(
                    "root_task.affinity.drivers.{} contains root core {}; driver TCBs must use non-root cores",
                    name,
                    root_core
                );
            }
        }
        Ok(())
    }

    fn entries(&self) -> [(&'static str, Option<u8>); 9] {
        [
            ("serial", self.serial),
            ("usb-local-seat", self.usb_local_seat),
            ("hdmi-text", self.hdmi_text),
            ("bcmgenet-v5", self.bcmgenet_v5),
            ("cyw43455", self.cyw43455),
            ("rtl8139", self.rtl8139),
            ("virtio-net", self.virtio_net),
            ("sdio-host", self.sdio_host),
            ("pcie-root", self.pcie_root),
        ]
    }
}

impl Default for DriverAffinityPolicy {
    fn default() -> Self {
        Self {
            serial: Some(1),
            usb_local_seat: Some(1),
            hdmi_text: Some(2),
            bcmgenet_v5: Some(3),
            cyw43455: Some(3),
            rtl8139: Some(2),
            virtio_net: Some(3),
            sdio_host: Some(3),
            pcie_root: Some(2),
        }
    }
}

impl Default for AffinityPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_cores: 4,
            authority_core: Some(0),
            ninedoor_cores: vec![1],
            provider_cores: vec![2, 3],
            worker_cores: vec![2, 3],
            drivers: DriverAffinityPolicy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        load_manifest, AffinityPolicy, AttestationPolicy, DmaProtectionProfile,
        DriverAffinityPolicy, DriverRuntimeBusLinkSpec, DriverRuntimeImagePolicy,
        DriverRuntimeImageSpec, DriverRuntimeIrqSpec, DriverRuntimeIrqTrigger, HardwareDevice,
        HardwareDeviceKind, NetworkBackendKind, NetworkInterfacePolicy, NetworkMode,
        WorkerSchedulingProfile,
    };
    use std::path::PathBuf;

    #[test]
    fn affinity_defaults_enabled_with_expected_cores() {
        let policy = AffinityPolicy::default();
        assert!(policy.enabled);
        assert_eq!(policy.max_cores, 4);
        assert_eq!(policy.authority_core, Some(0));
        assert_eq!(policy.ninedoor_cores, vec![1]);
        assert_eq!(policy.provider_cores, vec![2, 3]);
        assert_eq!(policy.worker_cores, vec![2, 3]);
        assert_eq!(policy.drivers.serial, Some(1));
        assert_eq!(policy.drivers.usb_local_seat, Some(1));
        assert_eq!(policy.drivers.hdmi_text, Some(2));
        assert_eq!(policy.drivers.bcmgenet_v5, Some(3));
        assert_eq!(policy.drivers.cyw43455, Some(3));
        assert_eq!(policy.drivers.rtl8139, Some(2));
        assert_eq!(policy.drivers.virtio_net, Some(3));
        assert_eq!(policy.drivers.sdio_host, Some(3));
        assert_eq!(policy.drivers.pcie_root, Some(2));
    }

    fn driver_runtime_image(hot_path: &str) -> DriverRuntimeImageSpec {
        DriverRuntimeImageSpec {
            id: format!("pi4-{hot_path}"),
            contract: hot_path.to_owned(),
            hot_path: hot_path.to_owned(),
            artifact: format!("cohesix/bin/pi4-driver-{hot_path}"),
            entry_symbol: "cohesix_pi4_driver_runtime_entry".to_owned(),
            code_pages: 1,
            stack_pages: 1,
            ipc_pages: 1,
            ring_pages: 1,
            mmio_pages: if hot_path == "sdio-host" { 2 } else { 1 },
            dma_pages: 1,
            shared_buffer_pages: 1,
            root_context_required: true,
            hardware_state_migrated: false,
        }
    }

    fn sdio_irq() -> DriverRuntimeIrqSpec {
        DriverRuntimeIrqSpec {
            hot_path: "sdio-host".to_owned(),
            irq: super::DRIVER_RUNTIME_CYW43_SDIO_IRQ,
            badge: super::DRIVER_RUNTIME_CYW43_SDIO_BADGE,
            handler_slot: 4,
            notification_slot: super::DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
            trigger: DriverRuntimeIrqTrigger::Level,
        }
    }

    fn cyw43_sdio_link() -> DriverRuntimeBusLinkSpec {
        DriverRuntimeBusLinkSpec {
            channel: "cyw43-sdio".to_owned(),
            client_hot_path: "cyw43-wifi".to_owned(),
            owner_hot_path: "sdio-host".to_owned(),
            client_notification_slot: super::DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
            owner_notification_slot: super::DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
            client_to_owner_slot: super::DRIVER_RUNTIME_CYW43_SDIO_CLIENT_TO_OWNER_SLOT,
            owner_to_client_slot: super::DRIVER_RUNTIME_CYW43_SDIO_OWNER_TO_CLIENT_SLOT,
            shared_offset: super::DRIVER_RUNTIME_CYW43_SDIO_SHARED_OFFSET,
            shared_len: super::DRIVER_RUNTIME_CYW43_SDIO_SHARED_LEN,
            link_epoch: super::DRIVER_RUNTIME_CYW43_SDIO_LINK_EPOCH,
            event_offset: super::DRIVER_RUNTIME_DPC_EVENT_OFFSET,
            event_len: super::DRIVER_RUNTIME_DPC_EVENT_LEN,
            event_depth: super::DRIVER_RUNTIME_DPC_EVENT_DEPTH,
        }
    }

    #[test]
    fn driver_runtime_policy_required_covers_all_pi4_hot_paths() {
        let policy = DriverRuntimeImagePolicy {
            required: true,
            images: super::REQUIRED_PI4_DRIVER_RUNTIME_HOT_PATHS
                .iter()
                .copied()
                .map(driver_runtime_image)
                .collect(),
            irqs: vec![sdio_irq()],
            bus_links: vec![cyw43_sdio_link()],
        };
        policy.validate().expect("complete driver runtime table");
    }

    #[test]
    fn sdio_runtime_requires_exact_power_sequence_resources() {
        let mut image = driver_runtime_image("sdio-host");
        image.mmio_pages = 1;
        let err = image.validate().expect_err("missing mailbox page rejected");
        assert!(err
            .to_string()
            .contains("exactly 2 mmio pages and 1 dma page"));

        let mut image = driver_runtime_image("sdio-host");
        image.dma_pages = 0;
        let err = image.validate().expect_err("missing request page rejected");
        assert!(err
            .to_string()
            .contains("exactly 2 mmio pages and 1 dma page"));
    }

    #[test]
    fn driver_runtime_policy_rejects_missing_required_hot_path() {
        let policy = DriverRuntimeImagePolicy {
            required: true,
            images: super::REQUIRED_PI4_DRIVER_RUNTIME_HOT_PATHS
                .iter()
                .copied()
                .filter(|hot_path| *hot_path != "pcie-root")
                .map(driver_runtime_image)
                .collect(),
            irqs: vec![sdio_irq()],
            bus_links: vec![cyw43_sdio_link()],
        };
        let err = policy.validate().expect_err("missing pcie-root rejected");
        assert!(err.to_string().contains("missing hot path pcie-root"));
    }

    #[test]
    fn driver_runtime_policy_rejects_incomplete_cyw43_sdio_dpc_topology() {
        let images = super::REQUIRED_PI4_DRIVER_RUNTIME_HOT_PATHS
            .iter()
            .copied()
            .map(driver_runtime_image)
            .collect();
        let missing_irq = DriverRuntimeImagePolicy {
            required: true,
            images,
            irqs: Vec::new(),
            bus_links: vec![cyw43_sdio_link()],
        };
        let err = missing_irq
            .validate()
            .expect_err("missing generated SDIO IRQ rejected");
        assert!(err
            .to_string()
            .contains("requires the generated level-triggered SDIO IRQ 158"));

        let mut invalid_link = cyw43_sdio_link();
        invalid_link.event_depth = 8;
        let invalid_dpc = DriverRuntimeImagePolicy {
            required: true,
            images: super::REQUIRED_PI4_DRIVER_RUNTIME_HOT_PATHS
                .iter()
                .copied()
                .map(driver_runtime_image)
                .collect(),
            irqs: vec![sdio_irq()],
            bus_links: vec![invalid_link],
        };
        let err = invalid_dpc
            .validate()
            .expect_err("unbounded generated DPC shape rejected");
        assert!(err.to_string().contains("bounded reciprocal DPC contract"));
    }

    #[test]
    fn pi4_manifest_places_genet_and_wifi_on_fourth_core() {
        let manifest_path = repo_root()
            .join("configs/root_task_pi4_uboot_aarch64.toml")
            .canonicalize()
            .expect("Pi 4 manifest path");
        let manifest = load_manifest(&manifest_path).expect("load Pi 4 manifest");
        assert_eq!(manifest.root_task.affinity.max_cores, 4);
        assert_eq!(manifest.root_task.affinity.drivers.bcmgenet_v5, Some(3));
        assert_eq!(manifest.root_task.affinity.drivers.cyw43455, Some(3));
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("Pi 4 driver affinity must validate");
    }

    #[test]
    fn affinity_rejects_driver_root_core() {
        let mut manifest = fixture_manifest();
        manifest.root_task.affinity.drivers = DriverAffinityPolicy {
            serial: Some(0),
            ..DriverAffinityPolicy::default()
        };
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("driver affinity must avoid the root core");
        assert!(
            err.to_string()
                .contains("root_task.affinity.drivers.serial contains root core 0"),
            "unexpected error: {err}"
        );
    }

    fn fixture_manifest() -> super::Manifest {
        let manifest_path = repo_root()
            .join("configs/root_task.toml")
            .canonicalize()
            .expect("fixture manifest path");
        load_manifest(&manifest_path).expect("load fixture manifest")
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root path")
    }

    fn base_pi4_manifest(profile_name: &str) -> super::Manifest {
        let mut manifest = fixture_manifest();
        manifest.profile.name = profile_name.to_owned();
        manifest.features.net_console = false;
        manifest.hw.no_nic = true;
        manifest.hw.network.enabled = false;
        manifest.hw.network.backend = NetworkBackendKind::Auto;
        manifest.hw.network.mode = NetworkMode::Off;
        manifest.hw.network.interface = NetworkInterfacePolicy::Wired;
        manifest.hw.network.static_ipv4.ip.clear();
        manifest.hw.network.static_ipv4.prefix_len = 0;
        manifest.hw.network.static_ipv4.gateway = None;
        manifest.dma.protection_profile = DmaProtectionProfile::BoundedNoIommu;
        manifest.hw.devices = vec![
            HardwareDevice {
                kind: HardwareDeviceKind::Uart,
                id: "uart0".to_owned(),
                required: true,
            },
            HardwareDevice {
                kind: HardwareDeviceKind::Rtc,
                id: "rtc0".to_owned(),
                required: true,
            },
            HardwareDevice {
                kind: HardwareDeviceKind::Net,
                id: "bcmgenet0".to_owned(),
                required: true,
            },
        ];
        manifest
    }

    #[test]
    fn virt_profile_accepts_default_dma_profile() {
        let manifest = fixture_manifest();
        assert_eq!(manifest.dma.protection_profile, DmaProtectionProfile::None);
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("virt profile accepts no DMA protection profile claim");
    }

    #[test]
    fn pi4_profile_requires_bounded_no_iommu_dma_profile() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.dma.protection_profile = DmaProtectionProfile::None;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("Pi 4 DMA profile must be declared");
        assert!(
            err.to_string()
                .contains("requires dma.protection_profile=bounded-no-iommu"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn smmu_dma_profile_requires_generated_domain_state() {
        let mut manifest = fixture_manifest();
        manifest.dma.protection_profile = DmaProtectionProfile::SmmuV3;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("SMMU isolation needs generated domain state");
        assert!(
            err.to_string()
                .contains("requires generated per-device DMA-domain state"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn worker_runtime_accepts_modeled_non_executable_roles() {
        let manifest = fixture_manifest();
        assert!(manifest
            .worker_runtime
            .roles
            .iter()
            .all(|entry| !entry.implemented));
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("modeled role records do not claim target execution");
    }

    #[test]
    fn worker_runtime_requires_role_records() {
        let mut manifest = fixture_manifest();
        manifest
            .worker_runtime
            .roles
            .retain(|entry| entry.role != super::Role::WorkerHeartbeat);
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("heartbeat role record is required");
        assert!(
            err.to_string()
                .contains("missing required role record worker-heartbeat"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn worker_runtime_rejects_metadata_only_authority() {
        let mut manifest = fixture_manifest();
        let heartbeat = manifest
            .worker_runtime
            .roles
            .iter_mut()
            .find(|entry| entry.role == super::Role::WorkerHeartbeat)
            .expect("heartbeat role");
        heartbeat.implemented = true;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("metadata-only implementation must not become executable authority");
        assert!(
            err.to_string()
                .contains("executable roles are unsupported until the generated contract"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn worker_runtime_rejects_executable_roles_without_task_object_contract() {
        let mut manifest = fixture_manifest();
        manifest.worker_runtime.roles[0].implemented = true;
        manifest.worker_runtime.cap_backed_authority = true;
        manifest.worker_runtime.endpoint_caps.required = true;
        manifest.worker_runtime.notification_lifecycle = true;
        manifest.worker_runtime.notifications.enabled = true;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("executable workers require a generated task-object contract");
        assert!(
            err.to_string().contains(
                "executable roles are unsupported until the generated contract includes image"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn worker_runtime_rejects_capability_claims_without_executable_roles() {
        let mut manifest = fixture_manifest();
        manifest.worker_runtime.cap_backed_authority = true;
        manifest.worker_runtime.endpoint_caps.required = true;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("disabled worker roles must not claim endpoint authority");
        assert!(
            err.to_string()
                .contains("endpoint authority must be false when no roles are implemented"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn worker_runtime_rejects_notification_claims_without_executable_roles() {
        let mut manifest = fixture_manifest();
        manifest.worker_runtime.notification_lifecycle = true;
        manifest.worker_runtime.notifications.enabled = true;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("disabled worker roles must not claim notification lifecycle");
        assert!(
            err.to_string()
                .contains("notification lifecycle must be false when no roles are implemented"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn worker_runtime_rejects_duplicate_worker_badges() {
        let mut manifest = fixture_manifest();
        manifest.worker_runtime.roles[0].implemented = true;
        manifest.worker_runtime.cap_backed_authority = true;
        manifest.worker_runtime.endpoint_caps.required = true;
        manifest.worker_runtime.notification_lifecycle = true;
        manifest.worker_runtime.notifications.enabled = true;
        manifest.worker_runtime.notifications.revoke_badge =
            manifest.worker_runtime.endpoint_caps.attach_badge_base;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("badge overlap must fail");
        assert!(
            err.to_string().contains("overlaps another badge"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn non_mcs_worker_runtime_must_not_claim_mcs_evidence() {
        let mut manifest = fixture_manifest();
        manifest.worker_runtime.scheduling.profile = WorkerSchedulingProfile::NonMcs;
        manifest.worker_runtime.scheduling.consumed_budget_evidence = true;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("non-MCS evidence claim must fail");
        assert!(
            err.to_string().contains("non-mcs profile must not claim"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn mcs_worker_runtime_requires_budget_and_timeout_evidence() {
        let mut manifest = fixture_manifest();
        manifest.worker_runtime.scheduling.profile = WorkerSchedulingProfile::Mcs;
        let err = manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect_err("MCS budget fields are required");
        assert!(
            err.to_string().contains("MCS profile requires"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn uefi_alias_accepts_no_nic_baseline() {
        let manifest = base_pi4_manifest("uefi-aarch64");
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("legacy alias should validate in no-nic baseline mode");
    }

    #[test]
    fn pi4_profile_network_enabled_requires_backend_and_static_ipv4() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Static;
        manifest.hw.network.static_ipv4.prefix_len = 24;
        let err = manifest
            .validate()
            .expect_err("missing static IPv4 must fail");
        assert!(
            err.to_string()
                .contains("hw.network.static_ipv4.ip must be set"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pi4_profile_network_enabled_rejects_invalid_prefix() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Static;
        manifest.hw.network.static_ipv4.ip = "192.168.2.20".to_owned();
        manifest.hw.network.static_ipv4.prefix_len = 0;
        let err = manifest.validate().expect_err("invalid prefix must fail");
        assert!(
            err.to_string()
                .contains("hw.network.static_ipv4.prefix_len 0 must be in 1..=32"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pi4_profile_network_enabled_rejects_non_bcmgenet_backend() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::Rtl8139;
        manifest.hw.network.mode = NetworkMode::Static;
        manifest.hw.network.static_ipv4.ip = "192.168.2.20".to_owned();
        manifest.hw.network.static_ipv4.prefix_len = 24;
        let err = manifest
            .validate()
            .expect_err("non-bcmgenet backend should fail for pi4 profile");
        assert!(
            err.to_string()
                .contains("requires hw.network.backend=bcmgenet-v5"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pi4_profile_accepts_static_ipv4_network_configuration() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Static;
        manifest.hw.network.static_ipv4.ip = "192.168.2.20".to_owned();
        manifest.hw.network.static_ipv4.prefix_len = 24;
        manifest.hw.network.static_ipv4.gateway = Some("192.168.2.1".to_owned());
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("valid pi4 static IPv4 manifest should validate");
    }

    #[test]
    fn uefi_alias_accepts_static_ipv4_migration_path() {
        let mut manifest = base_pi4_manifest("uefi-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Static;
        manifest.hw.network.static_ipv4.ip = "10.42.0.9".to_owned();
        manifest.hw.network.static_ipv4.prefix_len = 24;
        manifest.hw.network.static_ipv4.gateway = Some("10.42.0.1".to_owned());
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("legacy alias should accept 26a static IPv4 migration path");
    }

    #[test]
    fn pi4_profile_wifi_policy_requires_wifi_device() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Dhcp;
        manifest.hw.network.interface = NetworkInterfacePolicy::Wifi;
        let err = manifest
            .validate()
            .expect_err("wifi policy without wifi device must fail");
        assert!(
            err.to_string().contains("hw.devices[] kind=wifi"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pi4_profile_auto_policy_requires_wifi_device() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Dhcp;
        manifest.hw.network.interface = NetworkInterfacePolicy::Auto;
        let err = manifest
            .validate()
            .expect_err("auto policy without wifi device must fail");
        assert!(
            err.to_string().contains("hw.devices[] kind=wifi"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pi4_profile_accepts_wifi_dhcp_configuration() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Dhcp;
        manifest.hw.network.interface = NetworkInterfacePolicy::Wifi;
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Wifi,
            id: "cyw43xx0".to_owned(),
            required: false,
        });
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("valid pi4 wifi DHCP manifest should validate");
    }

    #[test]
    fn pi4_profile_accepts_wifi_static_configuration() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Static;
        manifest.hw.network.interface = NetworkInterfacePolicy::Wifi;
        manifest.hw.network.static_ipv4.ip = "192.168.20.42".to_owned();
        manifest.hw.network.static_ipv4.prefix_len = 24;
        manifest.hw.network.static_ipv4.gateway = Some("192.168.20.1".to_owned());
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Wifi,
            id: "cyw43xx0".to_owned(),
            required: false,
        });
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("valid pi4 wifi static manifest should validate");
    }

    #[test]
    fn pi4_profile_accepts_auto_dhcp_configuration() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.no_nic = false;
        manifest.features.net_console = true;
        manifest.hw.network.enabled = true;
        manifest.hw.network.backend = NetworkBackendKind::BcmGenetV5;
        manifest.hw.network.mode = NetworkMode::Dhcp;
        manifest.hw.network.interface = NetworkInterfacePolicy::Auto;
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Wifi,
            id: "cyw43xx0".to_owned(),
            required: false,
        });
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("valid pi4 auto DHCP manifest should validate");
    }

    #[test]
    fn uefi_alias_accepts_minimal_no_nic_hardware_bindings() {
        let manifest = base_pi4_manifest("uefi-aarch64");
        manifest
            .validate_with_base(Some(repo_root().as_path()))
            .expect("minimal uefi no-nic manifest must validate");
    }

    #[test]
    fn local_seat_required_demands_enabled() {
        let mut manifest = fixture_manifest();
        manifest.hw.local_seat.required = true;
        manifest.hw.local_seat.enabled = false;
        let err = manifest
            .validate()
            .expect_err("required local seat without enable should fail");
        assert!(
            err.to_string()
                .contains("hw.local_seat.required=true requires hw.local_seat.enabled=true"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pi4_local_seat_requires_matching_keyboard_and_display_ids() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.local_seat.enabled = true;
        manifest.hw.local_seat.required = false;
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Keyboard,
            id: "other-kbd".to_owned(),
            required: false,
        });
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Display,
            id: "hdmi0".to_owned(),
            required: false,
        });
        let err = manifest
            .validate()
            .expect_err("local-seat keyboard id mismatch should fail");
        assert!(
            err.to_string().contains(
                "hw.local_seat.keyboard_device=usb-kbd0 requires matching hw.devices[] kind=keyboard"
            ),
            "unexpected error: {err}"
        );

        manifest.hw.devices.clear();
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Uart,
            id: "uart0".to_owned(),
            required: true,
        });
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Rtc,
            id: "rtc0".to_owned(),
            required: true,
        });
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Net,
            id: "bcmgenet0".to_owned(),
            required: true,
        });
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Keyboard,
            id: "usb-kbd0".to_owned(),
            required: false,
        });
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Display,
            id: "other-display".to_owned(),
            required: false,
        });
        let err = manifest
            .validate()
            .expect_err("local-seat display id mismatch should fail");
        assert!(
            err.to_string().contains(
                "hw.local_seat.display_device=hdmi0 requires matching hw.devices[] kind=display"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pi4_required_local_seat_requires_required_devices() {
        let mut manifest = base_pi4_manifest("pi4-uboot-aarch64");
        manifest.hw.local_seat.enabled = true;
        manifest.hw.local_seat.required = true;
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Keyboard,
            id: "usb-kbd0".to_owned(),
            required: false,
        });
        manifest.hw.devices.push(HardwareDevice {
            kind: HardwareDeviceKind::Display,
            id: "hdmi0".to_owned(),
            required: false,
        });
        let err = manifest
            .validate()
            .expect_err("required local-seat keyboard must be required");
        assert!(
            err.to_string().contains(
                "hw.local_seat.required=true requires hw.devices[] kind=keyboard id=usb-kbd0 required=true"
            ),
            "unexpected error: {err}"
        );

        let keyboard = manifest
            .hw
            .devices
            .iter_mut()
            .find(|device| device.kind == HardwareDeviceKind::Keyboard)
            .expect("keyboard device");
        keyboard.required = true;
        let err = manifest
            .validate()
            .expect_err("required local-seat display must be required");
        assert!(
            err.to_string().contains(
                "hw.local_seat.required=true requires hw.devices[] kind=display id=hdmi0 required=true"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tpm_only_attestation_requires_tpm_device() {
        let mut manifest = fixture_manifest();
        manifest.hw.attestation.enabled = true;
        manifest.hw.attestation.policy = AttestationPolicy::TpmOnly;
        manifest.hw.devices.clear();
        let err = manifest
            .validate()
            .expect_err("tpm-only policy without tpm device must fail");
        assert!(
            err.to_string()
                .contains("hw.attestation.policy=tpm-only requires hw.devices[] kind=tpm"),
            "unexpected error: {err}"
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ManifestMeta {
    pub author: String,
    pub purpose: String,
}

impl Default for ManifestMeta {
    fn default() -> Self {
        Self {
            author: "Lukas Bower".to_owned(),
            purpose: "Resolved root-task manifest.".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub kernel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventPump {
    pub tick_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secure9pLimits {
    pub msize: u32,
    pub walk_depth: u8,
    pub tags_per_session: u16,
    pub batch_frames: u16,
    pub short_write: ShortWriteConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortWriteConfig {
    pub policy: ShortWritePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShortWritePolicy {
    Reject,
    Retry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureToggles {
    pub net_console: bool,
    #[serde(default)]
    pub serial_console: bool,
    #[serde(default)]
    pub std_console: bool,
    #[serde(default)]
    pub std_host_tools: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HardwareConfig {
    pub secure_boot: bool,
    pub no_nic: bool,
    pub network: HardwareNetworkConfig,
    pub attestation: AttestationConfig,
    pub local_seat: LocalSeatConfig,
    pub devices: Vec<HardwareDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareDevice {
    pub kind: HardwareDeviceKind,
    pub id: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HardwareDeviceKind {
    Uart,
    Net,
    Wifi,
    Tpm,
    Rtc,
    Keyboard,
    Display,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AttestationConfig {
    pub enabled: bool,
    pub policy: AttestationPolicy,
    pub evidence_max_bytes: u16,
}

impl Default for AttestationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            policy: AttestationPolicy::TpmOrDice,
            evidence_max_bytes: 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttestationPolicy {
    TpmOnly,
    TpmOrDice,
    DiceOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LocalSeatConfig {
    pub enabled: bool,
    pub required: bool,
    pub keyboard_device: String,
    pub display_device: String,
    pub line_bytes: u16,
    pub buffer_lines: u16,
}

impl Default for LocalSeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            required: false,
            keyboard_device: "usb-kbd0".to_owned(),
            display_device: "hdmi0".to_owned(),
            line_bytes: 160,
            buffer_lines: 128,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HardwareNetworkConfig {
    pub enabled: bool,
    pub backend: NetworkBackendKind,
    pub mode: NetworkMode,
    pub interface: NetworkInterfacePolicy,
    pub static_ipv4: StaticIpv4Config,
    pub dhcp: DhcpPolicyConfig,
}

impl Default for HardwareNetworkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: NetworkBackendKind::Auto,
            mode: NetworkMode::Off,
            interface: NetworkInterfacePolicy::Wired,
            static_ipv4: StaticIpv4Config::default(),
            dhcp: DhcpPolicyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    #[default]
    Off,
    Static,
    Dhcp,
}

impl NetworkMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Static => "static",
            Self::Dhcp => "dhcp",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkInterfacePolicy {
    #[default]
    Wired,
    Wifi,
    Auto,
}

impl NetworkInterfacePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wired => "wired",
            Self::Wifi => "wifi",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DhcpPolicyConfig {
    pub discover_timeout_ms: u32,
    pub request_timeout_ms: u32,
    pub max_retries: u8,
}

impl Default for DhcpPolicyConfig {
    fn default() -> Self {
        Self {
            discover_timeout_ms: 1_000,
            request_timeout_ms: 1_000,
            max_retries: 4,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StaticIpv4Config {
    pub ip: String,
    pub prefix_len: u8,
    pub gateway: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkBackendKind {
    #[default]
    Auto,
    Rtl8139,
    VirtioNet,
    #[serde(rename = "bcmgenet-v5", alias = "bcm-genet-v5")]
    BcmGenetV5,
}

impl NetworkBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Rtl8139 => "rtl8139",
            Self::VirtioNet => "virtio-net",
            Self::BcmGenetV5 => "bcmgenet-v5",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CacheConfig {
    pub kernel_ops: bool,
    pub dma_clean: bool,
    pub dma_invalidate: bool,
    pub unify_instructions: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DmaProtectionProfile {
    #[default]
    None,
    BoundedNoIommu,
    SmmuV2,
    SmmuV3,
}

impl DmaProtectionProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::BoundedNoIommu => "bounded-no-iommu",
            Self::SmmuV2 => "smmu-v2",
            Self::SmmuV3 => "smmu-v3",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DmaConfig {
    pub protection_profile: DmaProtectionProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerRuntimeConfig {
    pub implementation_epoch: u32,
    pub max_workers: u16,
    pub ticket_subject_required: bool,
    pub cap_backed_authority: bool,
    pub notification_lifecycle: bool,
    pub roles: Vec<WorkerRoleRuntime>,
    pub endpoint_caps: WorkerEndpointCapConfig,
    pub notifications: WorkerNotificationConfig,
    pub scheduling: WorkerSchedulingConfig,
}

impl WorkerRuntimeConfig {
    fn validate(&self) -> Result<()> {
        if self.implementation_epoch == 0 {
            bail!("worker_runtime.implementation_epoch must be > 0");
        }
        if self.max_workers == 0 {
            bail!("worker_runtime.max_workers must be > 0");
        }
        if self.roles.is_empty() {
            bail!("worker_runtime.roles must not be empty");
        }
        if self.roles.len() > MAX_WORKER_RUNTIME_ROLES {
            bail!(
                "worker_runtime.roles contains {} entries, max {}",
                self.roles.len(),
                MAX_WORKER_RUNTIME_ROLES
            );
        }
        let mut seen = BTreeSet::new();
        for role in &self.roles {
            role.validate()?;
            if !seen.insert(role.role.as_str()) {
                bail!(
                    "worker_runtime.roles contains duplicate role {}",
                    role.role.as_str()
                );
            }
        }
        self.scheduling.validate()?;
        if self.cap_backed_authority != self.endpoint_caps.required {
            bail!("worker_runtime.cap_backed_authority must match worker_runtime.endpoint_caps.required");
        }
        if self.notification_lifecycle != self.notifications.enabled {
            bail!("worker_runtime.notification_lifecycle must match worker_runtime.notifications.enabled");
        }
        if !self.has_implemented_roles() {
            if self.cap_backed_authority || self.endpoint_caps.required {
                bail!(
                    "worker_runtime endpoint authority must be false when no roles are implemented"
                );
            }
            if self.notification_lifecycle || self.notifications.enabled {
                bail!("worker_runtime notification lifecycle must be false when no roles are implemented");
            }
            return Ok(());
        }
        self.endpoint_caps.validate()?;
        self.notifications.validate()?;
        let mut badges = BTreeSet::new();
        for (name, badge) in self.endpoint_caps.badge_entries() {
            if badge == 0 {
                bail!("worker_runtime.endpoint_caps.{name} must be non-zero");
            }
            if !badges.insert(badge) {
                bail!("worker_runtime endpoint badge 0x{badge:x} is duplicated");
            }
        }
        let badge_span = self.endpoint_caps.badge_span()?;
        let mut ranges = Vec::<(&'static str, u64, u64)>::new();
        for (name, base) in self.endpoint_caps.badge_entries() {
            let end = base
                .checked_add(badge_span.saturating_sub(1))
                .ok_or_else(|| {
                    anyhow::anyhow!("worker_runtime.endpoint_caps.{name} range overflows")
                })?;
            for (seen_name, seen_start, seen_end) in &ranges {
                if base <= *seen_end && end >= *seen_start {
                    bail!(
                        "worker_runtime.endpoint_caps.{} range overlaps {}",
                        name,
                        seen_name
                    );
                }
            }
            ranges.push((name, base, end));
        }
        for (name, badge) in self.notifications.badge_entries() {
            if badge == 0 {
                bail!("worker_runtime.notifications.{name} must be non-zero");
            }
            if !badges.insert(badge) {
                bail!("worker_runtime notification badge 0x{badge:x} overlaps another badge");
            }
            for (range_name, start, end) in &ranges {
                if badge >= *start && badge <= *end {
                    bail!(
                        "worker_runtime.notifications.{} overlaps endpoint range {}",
                        name,
                        range_name
                    );
                }
            }
        }
        Ok(())
    }

    fn role(&self, role: Role) -> Option<&WorkerRoleRuntime> {
        self.roles.iter().find(|entry| entry.role == role)
    }

    fn has_implemented_roles(&self) -> bool {
        self.roles.iter().any(|role| role.implemented)
    }
}

impl Default for WorkerRuntimeConfig {
    fn default() -> Self {
        Self {
            implementation_epoch: 26,
            max_workers: 8,
            ticket_subject_required: true,
            cap_backed_authority: false,
            notification_lifecycle: false,
            roles: default_worker_roles(),
            endpoint_caps: WorkerEndpointCapConfig::default(),
            notifications: WorkerNotificationConfig::default(),
            scheduling: WorkerSchedulingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerRoleRuntime {
    pub role: Role,
    pub implemented: bool,
    pub ticket_scope: String,
    pub telemetry_path_template: String,
    pub lease_path_template: String,
    pub shutdown_policy: String,
}

impl WorkerRoleRuntime {
    fn validate(&self) -> Result<()> {
        validate_worker_runtime_text("ticket_scope", &self.ticket_scope)?;
        validate_worker_runtime_text("telemetry_path_template", &self.telemetry_path_template)?;
        validate_worker_runtime_text("lease_path_template", &self.lease_path_template)?;
        validate_worker_runtime_text("shutdown_policy", &self.shutdown_policy)?;
        if self.implemented && self.ticket_scope.is_empty() {
            bail!(
                "worker_runtime.roles role={} requires non-empty ticket_scope",
                self.role.as_str()
            );
        }
        if self.implemented && self.telemetry_path_template.is_empty() {
            bail!(
                "worker_runtime.roles role={} requires non-empty telemetry_path_template",
                self.role.as_str()
            );
        }
        Ok(())
    }
}

impl Default for WorkerRoleRuntime {
    fn default() -> Self {
        Self {
            role: Role::WorkerHeartbeat,
            implemented: false,
            ticket_scope: String::new(),
            telemetry_path_template: String::new(),
            lease_path_template: String::new(),
            shutdown_policy: "deferred".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerEndpointCapConfig {
    pub required: bool,
    pub attach_badge_base: u64,
    pub telemetry_badge_base: u64,
    pub lease_badge_base: u64,
    pub receipt_badge_base: u64,
    pub revoke_badge_base: u64,
    pub epoch_bits: u8,
    pub role_bits: u8,
}

impl WorkerEndpointCapConfig {
    fn validate(&self) -> Result<()> {
        if self.required && self.epoch_bits == 0 {
            bail!("worker_runtime.endpoint_caps.epoch_bits must be > 0 when endpoint caps are required");
        }
        if self.required && self.role_bits == 0 {
            bail!("worker_runtime.endpoint_caps.role_bits must be > 0 when endpoint caps are required");
        }
        Ok(())
    }

    fn badge_entries(&self) -> [(&'static str, u64); 5] {
        [
            ("attach_badge_base", self.attach_badge_base),
            ("telemetry_badge_base", self.telemetry_badge_base),
            ("lease_badge_base", self.lease_badge_base),
            ("receipt_badge_base", self.receipt_badge_base),
            ("revoke_badge_base", self.revoke_badge_base),
        ]
    }

    fn badge_span(&self) -> Result<u64> {
        let total_bits = u16::from(self.epoch_bits) + u16::from(self.role_bits);
        if total_bits == 0 || total_bits >= 63 {
            bail!("worker_runtime.endpoint_caps epoch_bits + role_bits must be in 1..63");
        }
        Ok(1u64 << total_bits)
    }
}

impl Default for WorkerEndpointCapConfig {
    fn default() -> Self {
        Self {
            required: false,
            attach_badge_base: 0x260c_1000,
            telemetry_badge_base: 0x260c_2000,
            lease_badge_base: 0x260c_3000,
            receipt_badge_base: 0x260c_4000,
            revoke_badge_base: 0x260c_5000,
            epoch_bits: 8,
            role_bits: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerNotificationConfig {
    pub enabled: bool,
    pub revoke_badge: u64,
    pub shutdown_badge: u64,
    pub lease_expiry_badge: u64,
    pub telemetry_pressure_badge: u64,
    pub irq_badge: u64,
}

impl WorkerNotificationConfig {
    fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        Ok(())
    }

    fn badge_entries(&self) -> [(&'static str, u64); 5] {
        [
            ("revoke_badge", self.revoke_badge),
            ("shutdown_badge", self.shutdown_badge),
            ("lease_expiry_badge", self.lease_expiry_badge),
            ("telemetry_pressure_badge", self.telemetry_pressure_badge),
            ("irq_badge", self.irq_badge),
        ]
    }
}

impl Default for WorkerNotificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            revoke_badge: 0x260c_6000,
            shutdown_badge: 0x260c_7000,
            lease_expiry_badge: 0x260c_8000,
            telemetry_pressure_badge: 0x260c_9000,
            irq_badge: 0x260c_a000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerSchedulingProfile {
    #[default]
    NonMcs,
    Mcs,
}

impl WorkerSchedulingProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonMcs => "non-mcs",
            Self::Mcs => "mcs",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerSchedulingConfig {
    pub profile: WorkerSchedulingProfile,
    pub priority: u8,
    pub domain: u8,
    pub service_turn_budget: u16,
    pub mcs_budget_us: u32,
    pub mcs_period_us: u32,
    pub timeout_endpoint_badge: u64,
    pub consumed_budget_evidence: bool,
}

impl WorkerSchedulingConfig {
    fn validate(&self) -> Result<()> {
        match self.profile {
            WorkerSchedulingProfile::NonMcs => {
                if self.service_turn_budget == 0 {
                    bail!("worker_runtime.scheduling.service_turn_budget must be > 0 for non-mcs profiles");
                }
                if self.mcs_budget_us != 0
                    || self.mcs_period_us != 0
                    || self.timeout_endpoint_badge != 0
                    || self.consumed_budget_evidence
                {
                    bail!("worker_runtime.scheduling non-mcs profile must not claim MCS budget, timeout endpoint, or consumed-budget evidence");
                }
            }
            WorkerSchedulingProfile::Mcs => {
                if self.mcs_budget_us == 0 || self.mcs_period_us == 0 {
                    bail!("worker_runtime.scheduling MCS profile requires mcs_budget_us and mcs_period_us");
                }
                if self.timeout_endpoint_badge == 0 || !self.consumed_budget_evidence {
                    bail!("worker_runtime.scheduling MCS profile requires timeout endpoint badge and consumed-budget evidence");
                }
            }
        }
        Ok(())
    }
}

impl Default for WorkerSchedulingConfig {
    fn default() -> Self {
        Self {
            profile: WorkerSchedulingProfile::NonMcs,
            priority: 96,
            domain: 0,
            service_turn_budget: 64,
            mcs_budget_us: 0,
            mcs_period_us: 0,
            timeout_endpoint_badge: 0,
            consumed_budget_evidence: false,
        }
    }
}

fn default_worker_roles() -> Vec<WorkerRoleRuntime> {
    vec![
        WorkerRoleRuntime {
            role: Role::WorkerHeartbeat,
            implemented: false,
            ticket_scope: "/worker".to_owned(),
            telemetry_path_template: "/shard/<label>/worker/<id>/telemetry".to_owned(),
            lease_path_template: String::new(),
            shutdown_policy: "deferred".to_owned(),
        },
        WorkerRoleRuntime {
            role: Role::WorkerGpu,
            implemented: false,
            ticket_scope: "/gpu".to_owned(),
            telemetry_path_template: "/shard/<label>/worker/<id>/telemetry".to_owned(),
            lease_path_template: "/gpu/<id>/lease".to_owned(),
            shutdown_policy: "deferred".to_owned(),
        },
        WorkerRoleRuntime {
            role: Role::WorkerBus,
            implemented: false,
            ticket_scope: "/bus".to_owned(),
            telemetry_path_template: "/shard/<label>/worker/<id>/telemetry".to_owned(),
            lease_path_template: String::new(),
            shutdown_policy: "deferred".to_owned(),
        },
        WorkerRoleRuntime {
            role: Role::WorkerLora,
            implemented: false,
            ticket_scope: "/worker".to_owned(),
            telemetry_path_template: "/shard/<label>/worker/<id>/telemetry".to_owned(),
            lease_path_template: String::new(),
            shutdown_policy: "deferred".to_owned(),
        },
    ]
}

fn validate_worker_runtime_text(name: &str, value: &str) -> Result<()> {
    if value.len() > MAX_WORKER_RUNTIME_TEXT_LEN {
        bail!(
            "worker_runtime.{} exceeds max length {}",
            name,
            MAX_WORKER_RUNTIME_TEXT_LEN
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketSpec {
    pub role: Role,
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TicketLimits {
    pub max_scopes: u16,
    pub max_scope_path_len: u16,
    pub max_scope_rate_per_s: u32,
    pub bandwidth_bytes: u64,
    pub cursor_resumes: u32,
    pub cursor_advances: u32,
}

impl Default for TicketLimits {
    fn default() -> Self {
        Self {
            max_scopes: 8,
            max_scope_path_len: 128,
            max_scope_rate_per_s: 64,
            bandwidth_bytes: 131_072,
            cursor_resumes: 16,
            cursor_advances: 256,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Namespaces {
    pub role_isolation: bool,
    pub mounts: Vec<NamespaceMount>,
}

impl Default for Namespaces {
    fn default() -> Self {
        Self {
            role_isolation: true,
            mounts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sharding {
    pub enabled: bool,
    pub shard_bits: u8,
    pub legacy_worker_alias: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespaceMount {
    pub service: String,
    pub target: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ecosystem {
    pub host: EcosystemHost,
    pub audit: AuditConfig,
    pub policy: PolicyConfig,
    pub models: FeatureFlag,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sidecars {
    pub modbus: SidecarBusConfig,
    pub dnp3: SidecarBusConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SidecarBusConfig {
    pub enable: bool,
    #[serde(default = "default_bus_mount")]
    pub mount_at: String,
    #[serde(default)]
    pub adapters: Vec<SidecarBusAdapter>,
}

impl Default for SidecarBusConfig {
    fn default() -> Self {
        Self {
            enable: false,
            mount_at: default_bus_mount(),
            adapters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarBusAdapter {
    pub id: String,
    pub mount: String,
    pub scope: String,
    pub link: SidecarLink,
    pub baud: u32,
    #[serde(default)]
    pub spool: SpoolConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SidecarLink {
    Serial,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SpoolConfig {
    pub max_entries: u16,
    pub max_bytes: u32,
}

impl Default for SpoolConfig {
    fn default() -> Self {
        Self {
            max_entries: 32,
            max_bytes: 4096,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Telemetry {
    pub ring_bytes_per_worker: u32,
    pub frame_schema: TelemetryFrameSchema,
    pub cursor: TelemetryCursor,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            ring_bytes_per_worker: 1024,
            frame_schema: TelemetryFrameSchema::LegacyPlaintext,
            cursor: TelemetryCursor::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TelemetryIngest {
    pub max_segments_per_device: u32,
    pub max_bytes_per_segment: u32,
    pub max_total_bytes_per_device: u32,
    pub max_reference_entries_per_segment: u32,
    pub max_reference_manifest_bytes_per_segment: u32,
    pub max_reference_bytes_per_segment: u64,
    pub eviction_policy: TelemetryIngestEvictionPolicy,
}

impl Default for TelemetryIngest {
    fn default() -> Self {
        Self {
            max_segments_per_device: 4,
            max_bytes_per_segment: 32 * 1024,
            max_total_bytes_per_device: 128 * 1024,
            max_reference_entries_per_segment: 1024,
            max_reference_manifest_bytes_per_segment: 32 * 1024,
            max_reference_bytes_per_segment: 1_073_741_824,
            eviction_policy: TelemetryIngestEvictionPolicy::EvictOldest,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LifecycleState {
    Booting,
    Degraded,
    Online,
    Draining,
    Quiesced,
    Offline,
}

impl LifecycleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            LifecycleState::Booting => "BOOTING",
            LifecycleState::Degraded => "DEGRADED",
            LifecycleState::Online => "ONLINE",
            LifecycleState::Draining => "DRAINING",
            LifecycleState::Quiesced => "QUIESCED",
            LifecycleState::Offline => "OFFLINE",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleAutoTransition {
    pub from: LifecycleState,
    pub to: LifecycleState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LifecycleConfig {
    pub initial_state: LifecycleState,
    #[serde(default)]
    pub auto_transitions: Vec<LifecycleAutoTransition>,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            initial_state: LifecycleState::Booting,
            auto_transitions: vec![LifecycleAutoTransition {
                from: LifecycleState::Booting,
                to: LifecycleState::Online,
            }],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ControlPlaneConfig {
    pub schedule: ScheduleControlConfig,
    pub lease: LeaseControlConfig,
    pub export: ExportControlConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScheduleControlConfig {
    pub enable: bool,
    pub queue_max_entries: u32,
    pub ctl_max_bytes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LeaseControlConfig {
    pub enable: bool,
    pub active_max_entries: u32,
    pub preemptions_max_entries: u32,
    pub ctl_max_bytes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExportControlConfig {
    pub enable: bool,
    pub ctl_max_bytes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Observability {
    pub proc_9p: Proc9pObservability,
    pub proc_9p_session: Proc9pSessionObservability,
    pub proc_ingest: ProcIngestObservability,
    pub proc_root: ProcRootObservability,
    pub proc_pressure: ProcPressureObservability,
    pub proc_schedule: ProcScheduleObservability,
    pub proc_lease: ProcLeaseObservability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Proc9pObservability {
    pub sessions: bool,
    pub outstanding: bool,
    pub short_writes: bool,
    pub sessions_bytes: u32,
    pub outstanding_bytes: u32,
    pub short_writes_bytes: u32,
}

impl Default for Proc9pObservability {
    fn default() -> Self {
        Self {
            sessions: false,
            outstanding: false,
            short_writes: false,
            sessions_bytes: 1024,
            outstanding_bytes: 128,
            short_writes_bytes: 128,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Proc9pSessionObservability {
    pub active: bool,
    pub state: bool,
    pub since_ms: bool,
    pub owner: bool,
    pub active_bytes: u32,
    pub state_bytes: u32,
    pub since_ms_bytes: u32,
    pub owner_bytes: u32,
}

impl Default for Proc9pSessionObservability {
    fn default() -> Self {
        Self {
            active: false,
            state: false,
            since_ms: false,
            owner: false,
            active_bytes: 128,
            state_bytes: 64,
            since_ms_bytes: 64,
            owner_bytes: 96,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProcIngestObservability {
    pub p50_ms: bool,
    pub p95_ms: bool,
    pub backpressure: bool,
    pub dropped: bool,
    pub queued: bool,
    pub watch: bool,
    pub p50_ms_bytes: u32,
    pub p95_ms_bytes: u32,
    pub backpressure_bytes: u32,
    pub dropped_bytes: u32,
    pub queued_bytes: u32,
    pub watch_max_entries: u16,
    pub watch_line_bytes: u32,
    pub watch_min_interval_ms: u64,
    pub latency_samples: u16,
    pub latency_tolerance_ms: u32,
    pub counter_tolerance: u32,
}

impl Default for ProcIngestObservability {
    fn default() -> Self {
        Self {
            p50_ms: false,
            p95_ms: false,
            backpressure: false,
            dropped: false,
            queued: false,
            watch: false,
            p50_ms_bytes: 64,
            p95_ms_bytes: 64,
            backpressure_bytes: 64,
            dropped_bytes: 64,
            queued_bytes: 64,
            watch_max_entries: 16,
            watch_line_bytes: 160,
            watch_min_interval_ms: 50,
            latency_samples: 16,
            latency_tolerance_ms: 5,
            counter_tolerance: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProcRootObservability {
    pub reachable: bool,
    pub last_seen_ms: bool,
    pub cut_reason: bool,
    pub reachable_bytes: u32,
    pub last_seen_ms_bytes: u32,
    pub cut_reason_bytes: u32,
}

impl Default for ProcRootObservability {
    fn default() -> Self {
        Self {
            reachable: false,
            last_seen_ms: false,
            cut_reason: false,
            reachable_bytes: 32,
            last_seen_ms_bytes: 64,
            cut_reason_bytes: 64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProcPressureObservability {
    pub busy: bool,
    pub quota: bool,
    pub cut: bool,
    pub policy: bool,
    pub busy_bytes: u32,
    pub quota_bytes: u32,
    pub cut_bytes: u32,
    pub policy_bytes: u32,
}

impl Default for ProcPressureObservability {
    fn default() -> Self {
        Self {
            busy: false,
            quota: false,
            cut: false,
            policy: false,
            busy_bytes: 64,
            quota_bytes: 64,
            cut_bytes: 64,
            policy_bytes: 64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProcScheduleObservability {
    pub summary: bool,
    pub queue: bool,
    pub summary_bytes: u32,
    pub queue_bytes: u32,
}

impl Default for ProcScheduleObservability {
    fn default() -> Self {
        Self {
            summary: false,
            queue: false,
            summary_bytes: 128,
            queue_bytes: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProcLeaseObservability {
    pub summary: bool,
    pub active: bool,
    pub preemptions: bool,
    pub summary_bytes: u32,
    pub active_bytes: u32,
    pub preemptions_bytes: u32,
}

impl Default for ProcLeaseObservability {
    fn default() -> Self {
        Self {
            summary: false,
            active: false,
            preemptions: false,
            summary_bytes: 128,
            active_bytes: 1024,
            preemptions_bytes: 1024,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiProviders {
    pub proc_9p: UiProc9p,
    pub proc_ingest: UiProcIngest,
    pub policy_preflight: UiPolicyPreflight,
    pub updates: UiUpdates,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiProc9p {
    pub sessions: bool,
    pub outstanding: bool,
    pub short_writes: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiProcIngest {
    pub p50_ms: bool,
    pub p95_ms: bool,
    pub backpressure: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiPolicyPreflight {
    pub req: bool,
    pub diff: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiUpdates {
    pub manifest: bool,
    pub status: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientPolicies {
    pub coh: CohClientPolicy,
    pub cohsh: CohshClientPolicy,
    pub retry: ClientRetryPolicy,
    pub heartbeat: ClientHeartbeatPolicy,
    pub trace: ClientTracePolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CohClientPolicy {
    pub mount: CohMountPolicy,
    pub telemetry: CohTelemetryPolicy,
    pub run: CohRunPolicy,
    pub peft: CohPeftPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohMountPolicy {
    pub root: String,
    pub allowlist: Vec<String>,
}

impl Default for CohMountPolicy {
    fn default() -> Self {
        Self {
            root: default_coh_mount_root(),
            allowlist: default_coh_allowlist(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohTelemetryPolicy {
    pub root: String,
    pub max_devices: u32,
    pub max_segments_per_device: u32,
    pub max_bytes_per_segment: u32,
    pub max_total_bytes_per_device: u32,
}

impl Default for CohTelemetryPolicy {
    fn default() -> Self {
        Self {
            root: default_coh_telemetry_root(),
            max_devices: 32,
            max_segments_per_device: 4,
            max_bytes_per_segment: 32 * 1024,
            max_total_bytes_per_device: 128 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CohRunPolicy {
    pub lease: CohLeasePolicy,
    pub breadcrumb: CohBreadcrumbPolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CohPeftPolicy {
    pub export: CohPeftExportPolicy,
    pub import: CohPeftImportPolicy,
    pub activate: CohPeftActivatePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohPeftExportPolicy {
    pub root: String,
    pub max_telemetry_bytes: u32,
    pub max_policy_bytes: u32,
    pub max_base_model_bytes: u32,
}

impl Default for CohPeftExportPolicy {
    fn default() -> Self {
        Self {
            root: default_coh_peft_export_root(),
            max_telemetry_bytes: 128 * 1024,
            max_policy_bytes: 8 * 1024,
            max_base_model_bytes: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohPeftImportPolicy {
    pub registry_root: String,
    pub max_adapter_bytes: u64,
    pub max_lora_bytes: u32,
    pub max_metrics_bytes: u32,
    pub max_manifest_bytes: u32,
}

impl Default for CohPeftImportPolicy {
    fn default() -> Self {
        Self {
            registry_root: default_coh_peft_registry_root(),
            max_adapter_bytes: 64 * 1024 * 1024,
            max_lora_bytes: 64 * 1024,
            max_metrics_bytes: 64 * 1024,
            max_manifest_bytes: 8 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohPeftActivatePolicy {
    pub max_model_id_bytes: u32,
    pub max_state_bytes: u32,
}

impl Default for CohPeftActivatePolicy {
    fn default() -> Self {
        Self {
            max_model_id_bytes: 128,
            max_state_bytes: 4096,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohLeasePolicy {
    pub schema: String,
    pub active_state: String,
    pub max_bytes: u32,
}

impl Default for CohLeasePolicy {
    fn default() -> Self {
        Self {
            schema: default_coh_lease_schema(),
            active_state: default_coh_lease_active_state(),
            max_bytes: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohBreadcrumbPolicy {
    pub schema: String,
    pub max_line_bytes: u32,
    pub max_command_bytes: u32,
}

impl Default for CohBreadcrumbPolicy {
    fn default() -> Self {
        Self {
            schema: default_coh_breadcrumb_schema(),
            max_line_bytes: 512,
            max_command_bytes: 256,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientPaths {
    pub queen_ctl: String,
    pub queen_lifecycle_ctl: String,
    pub queen_schedule_ctl: String,
    pub queen_lease_ctl: String,
    pub queen_export_ctl: String,
    pub policy_ctl: String,
    pub log: String,
}

impl Default for ClientPaths {
    fn default() -> Self {
        Self {
            queen_ctl: "/queen/ctl".to_owned(),
            queen_lifecycle_ctl: "/queen/lifecycle/ctl".to_owned(),
            queen_schedule_ctl: "/queen/schedule/ctl".to_owned(),
            queen_lease_ctl: "/queen/lease/ctl".to_owned(),
            queen_export_ctl: "/queen/export/ctl".to_owned(),
            policy_ctl: "/policy/ctl".to_owned(),
            log: "/log/queen.log".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SwarmUiTicketScope {
    PerTicket,
    PerRole,
}

impl SwarmUiTicketScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            SwarmUiTicketScope::PerTicket => "per-ticket",
            SwarmUiTicketScope::PerRole => "per-role",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiConfig {
    pub ticket_scope: SwarmUiTicketScope,
    pub cache: SwarmUiCacheConfig,
    pub hive: SwarmUiHiveConfig,
    pub paths: SwarmUiPathsConfig,
}

impl Default for SwarmUiConfig {
    fn default() -> Self {
        Self {
            ticket_scope: SwarmUiTicketScope::PerTicket,
            cache: SwarmUiCacheConfig::default(),
            hive: SwarmUiHiveConfig::default(),
            paths: SwarmUiPathsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiCacheConfig {
    pub enabled: bool,
    pub max_bytes: u32,
    pub ttl_s: u64,
}

impl Default for SwarmUiCacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_bytes: 262_144,
            ttl_s: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiHiveConfig {
    pub frame_cap_fps: u16,
    pub step_ms: u16,
    pub lod_zoom_out: f32,
    pub lod_zoom_in: f32,
    pub lod_event_budget: u32,
    pub snapshot_max_events: u32,
    pub overlay_lines: u16,
    pub detail_lines: u16,
    pub line_cap_bytes: u32,
    pub per_worker_bytes: u32,
    pub pending_lines_per_worker: u16,
    pub pending_event_cap: u32,
    pub poll_workers_per_tick: u16,
    pub status_poll_ms: u32,
    pub degrade_pressure: f32,
}

impl Default for SwarmUiHiveConfig {
    fn default() -> Self {
        Self {
            frame_cap_fps: 60,
            step_ms: 16,
            lod_zoom_out: 0.7,
            lod_zoom_in: 1.25,
            lod_event_budget: 512,
            snapshot_max_events: 4096,
            overlay_lines: 3,
            detail_lines: 50,
            line_cap_bytes: 160,
            per_worker_bytes: 2048,
            pending_lines_per_worker: 64,
            pending_event_cap: 4096,
            poll_workers_per_tick: 32,
            status_poll_ms: 500,
            degrade_pressure: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiPathsConfig {
    pub telemetry_root: String,
    pub proc_ingest_root: String,
    pub worker_root: String,
    pub namespace_roots: Vec<String>,
}

impl Default for SwarmUiPathsConfig {
    fn default() -> Self {
        Self {
            telemetry_root: "/worker".to_owned(),
            proc_ingest_root: "/proc/ingest".to_owned(),
            worker_root: "/worker".to_owned(),
            namespace_roots: vec![
                "/proc".to_owned(),
                "/queen".to_owned(),
                "/worker".to_owned(),
                "/log".to_owned(),
                "/gpu".to_owned(),
            ],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CohshClientPolicy {
    pub pool: CohshPoolPolicy,
    pub tail: CohshTailPolicy,
    pub host_telemetry: CohshHostTelemetryPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohshTailPolicy {
    pub poll_ms_default: u64,
    pub poll_ms_min: u64,
    pub poll_ms_max: u64,
}

impl Default for CohshTailPolicy {
    fn default() -> Self {
        Self {
            poll_ms_default: 1000,
            poll_ms_min: 250,
            poll_ms_max: 10_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohshHostTelemetryPolicy {
    pub nvidia_poll_ms: u64,
    pub systemd_poll_ms: u64,
    pub docker_poll_ms: u64,
    pub k8s_poll_ms: u64,
}

impl Default for CohshHostTelemetryPolicy {
    fn default() -> Self {
        Self {
            nvidia_poll_ms: 1000,
            systemd_poll_ms: 2000,
            docker_poll_ms: 2000,
            k8s_poll_ms: 5000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CohshPoolPolicy {
    pub control_sessions: u16,
    pub telemetry_sessions: u16,
}

impl Default for CohshPoolPolicy {
    fn default() -> Self {
        Self {
            control_sessions: 2,
            telemetry_sessions: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientRetryPolicy {
    pub max_attempts: u8,
    pub backoff_ms: u64,
    pub ceiling_ms: u64,
    pub timeout_ms: u64,
}

impl Default for ClientRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: 200,
            ceiling_ms: 2000,
            timeout_ms: 5000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientHeartbeatPolicy {
    pub interval_ms: u64,
}

impl Default for ClientHeartbeatPolicy {
    fn default() -> Self {
        Self { interval_ms: 15000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientTracePolicy {
    pub max_bytes: u32,
}

impl Default for ClientTracePolicy {
    fn default() -> Self {
        Self {
            max_bytes: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CasConfig {
    pub enable: bool,
    pub store: CasStoreConfig,
    pub delta: CasDeltaConfig,
    pub signing: Option<CasSigningConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CasStoreConfig {
    pub chunk_bytes: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CasDeltaConfig {
    pub enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CasSigningConfig {
    pub required: bool,
    pub key_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TelemetryCursor {
    pub retain_on_boot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TelemetryFrameSchema {
    LegacyPlaintext,
    CborV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TelemetryIngestEvictionPolicy {
    Refuse,
    EvictOldest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcosystemHost {
    pub enable: bool,
    #[serde(default)]
    pub providers: Vec<HostProvider>,
    #[serde(default = "default_host_mount")]
    pub mount_at: String,
    #[serde(default)]
    pub tickets: HostTicketConfig,
    #[serde(default)]
    pub federation: HostFederationConfig,
}

impl Default for EcosystemHost {
    fn default() -> Self {
        Self {
            enable: false,
            providers: Vec::new(),
            mount_at: default_host_mount(),
            tickets: HostTicketConfig::default(),
            federation: HostFederationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostProvider {
    Systemd,
    K8s,
    Docker,
    Nvidia,
    Jetson,
    Net,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HostTicketConfig {
    pub enable: bool,
    #[serde(default = "default_host_ticket_request_schema")]
    pub request_schema: String,
    #[serde(default = "default_host_ticket_result_schema")]
    pub result_schema: String,
    pub max_line_bytes: u32,
    #[serde(default = "default_host_ticket_action_allowlist")]
    pub action_allowlist: Vec<HostTicketAction>,
    #[serde(default = "default_host_ticket_lifecycle")]
    pub lifecycle: Vec<HostTicketLifecycleState>,
}

impl Default for HostTicketConfig {
    fn default() -> Self {
        Self {
            enable: false,
            request_schema: default_host_ticket_request_schema(),
            result_schema: default_host_ticket_result_schema(),
            max_line_bytes: 2048,
            action_allowlist: default_host_ticket_action_allowlist(),
            lifecycle: default_host_ticket_lifecycle(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HostFederationConfig {
    pub enable: bool,
    #[serde(default = "default_host_federation_local_hive")]
    pub local_hive: String,
    #[serde(default)]
    pub peers: Vec<HostFederationPeer>,
    #[serde(default = "default_host_ticket_action_allowlist")]
    pub action_allowlist: Vec<HostTicketAction>,
    pub relay_queue_max_entries: u16,
    pub relay_queue_max_bytes: u32,
    pub wal_max_entries: u32,
    pub wal_max_bytes: u32,
    pub relay_timeout_ms: u32,
}

impl Default for HostFederationConfig {
    fn default() -> Self {
        Self {
            enable: false,
            local_hive: default_host_federation_local_hive(),
            peers: Vec::new(),
            action_allowlist: default_host_ticket_action_allowlist(),
            relay_queue_max_entries: 256,
            relay_queue_max_bytes: 32 * 1024,
            wal_max_entries: 1024,
            wal_max_bytes: 512 * 1024,
            relay_timeout_ms: 1500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostFederationPeer {
    pub name: String,
    pub rest_url: String,
    pub auth_ref: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum HostTicketAction {
    #[serde(rename = "gpu.lease.grant")]
    GpuLeaseGrant,
    #[serde(rename = "gpu.lease.renew")]
    GpuLeaseRenew,
    #[serde(rename = "gpu.lease.release")]
    GpuLeaseRelease,
    #[serde(rename = "peft.import")]
    PeftImport,
    #[serde(rename = "peft.activate")]
    PeftActivate,
    #[serde(rename = "peft.rollback")]
    PeftRollback,
    #[serde(rename = "systemd.start")]
    SystemdStart,
    #[serde(rename = "systemd.stop")]
    SystemdStop,
    #[serde(rename = "systemd.restart")]
    SystemdRestart,
    #[serde(rename = "systemd.status-check")]
    SystemdStatusCheck,
    #[serde(rename = "docker.restart")]
    DockerRestart,
    #[serde(rename = "docker.stop")]
    DockerStop,
    #[serde(rename = "docker.status-check")]
    DockerStatusCheck,
    #[serde(rename = "k8s.cordon")]
    K8sCordon,
    #[serde(rename = "k8s.drain")]
    K8sDrain,
    #[serde(rename = "k8s.lease.sync")]
    K8sLeaseSync,
}

impl HostTicketAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GpuLeaseGrant => "gpu.lease.grant",
            Self::GpuLeaseRenew => "gpu.lease.renew",
            Self::GpuLeaseRelease => "gpu.lease.release",
            Self::PeftImport => "peft.import",
            Self::PeftActivate => "peft.activate",
            Self::PeftRollback => "peft.rollback",
            Self::SystemdStart => "systemd.start",
            Self::SystemdStop => "systemd.stop",
            Self::SystemdRestart => "systemd.restart",
            Self::SystemdStatusCheck => "systemd.status-check",
            Self::DockerRestart => "docker.restart",
            Self::DockerStop => "docker.stop",
            Self::DockerStatusCheck => "docker.status-check",
            Self::K8sCordon => "k8s.cordon",
            Self::K8sDrain => "k8s.drain",
            Self::K8sLeaseSync => "k8s.lease.sync",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum HostTicketLifecycleState {
    Queued,
    Claimed,
    Running,
    Succeeded,
    Failed,
    Expired,
}

impl HostTicketLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FeatureFlag {
    pub enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AuditConfig {
    pub enable: bool,
    pub journal_max_bytes: u32,
    pub decisions_max_bytes: u32,
    pub replay_enable: bool,
    pub replay_max_entries: u16,
    pub replay_ctl_max_bytes: u32,
    pub replay_status_max_bytes: u32,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enable: false,
            journal_max_bytes: 8192,
            decisions_max_bytes: 4096,
            replay_enable: false,
            replay_max_entries: 64,
            replay_ctl_max_bytes: 1024,
            replay_status_max_bytes: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PolicyConfig {
    pub enable: bool,
    pub queue_max_entries: u16,
    pub queue_max_bytes: u32,
    pub ctl_max_bytes: u32,
    pub status_max_bytes: u32,
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            enable: false,
            queue_max_entries: 32,
            queue_max_bytes: 4096,
            ctl_max_bytes: 2048,
            status_max_bytes: 512,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRule {
    pub id: String,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Queen,
    WorkerHeartbeat,
    WorkerGpu,
    WorkerBus,
    WorkerLora,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queen => "queen",
            Self::WorkerHeartbeat => "worker-heartbeat",
            Self::WorkerGpu => "worker-gpu",
            Self::WorkerBus => "worker-bus",
            Self::WorkerLora => "worker-lora",
        }
    }
}

fn default_host_mount() -> String {
    "/host".to_owned()
}

fn default_host_federation_local_hive() -> String {
    "hive-a".to_owned()
}

fn default_host_ticket_request_schema() -> String {
    "host-ticket/v1".to_owned()
}

fn default_host_ticket_result_schema() -> String {
    "host-ticket-result/v1".to_owned()
}

fn default_host_ticket_action_allowlist() -> Vec<HostTicketAction> {
    vec![
        HostTicketAction::GpuLeaseGrant,
        HostTicketAction::GpuLeaseRenew,
        HostTicketAction::GpuLeaseRelease,
        HostTicketAction::PeftImport,
        HostTicketAction::PeftActivate,
        HostTicketAction::PeftRollback,
        HostTicketAction::SystemdStart,
        HostTicketAction::SystemdStop,
        HostTicketAction::SystemdRestart,
        HostTicketAction::SystemdStatusCheck,
        HostTicketAction::DockerRestart,
        HostTicketAction::DockerStop,
        HostTicketAction::DockerStatusCheck,
        HostTicketAction::K8sCordon,
        HostTicketAction::K8sDrain,
        HostTicketAction::K8sLeaseSync,
    ]
}

fn default_host_ticket_lifecycle() -> Vec<HostTicketLifecycleState> {
    vec![
        HostTicketLifecycleState::Queued,
        HostTicketLifecycleState::Claimed,
        HostTicketLifecycleState::Running,
        HostTicketLifecycleState::Succeeded,
        HostTicketLifecycleState::Failed,
        HostTicketLifecycleState::Expired,
    ]
}

fn default_coh_mount_root() -> String {
    "/".to_owned()
}

fn default_coh_telemetry_root() -> String {
    "/queen/telemetry".to_owned()
}

fn default_coh_allowlist() -> Vec<String> {
    vec![
        "/proc".to_owned(),
        "/queen".to_owned(),
        "/worker".to_owned(),
        "/log".to_owned(),
        "/gpu".to_owned(),
        "/host".to_owned(),
    ]
}

fn default_coh_lease_schema() -> String {
    "gpu-lease/v1".to_owned()
}

fn default_coh_lease_active_state() -> String {
    "ACTIVE".to_owned()
}

fn default_coh_breadcrumb_schema() -> String {
    "gpu-breadcrumb/v1".to_owned()
}

fn default_bus_mount() -> String {
    "/bus".to_owned()
}

fn default_coh_peft_export_root() -> String {
    "/queen/export/lora_jobs".to_owned()
}

fn default_coh_peft_registry_root() -> String {
    "out/model_registry".to_owned()
}

fn ensure_buffer_bytes(label: &str, value: u32, required: usize) -> Result<()> {
    if required > MAX_MSIZE as usize {
        bail!("{label} requires at least {required} bytes which exceeds max {MAX_MSIZE}");
    }
    if value < required as u32 {
        bail!("{label} {value} is below required minimum {required}");
    }
    if value > MAX_MSIZE {
        bail!("{label} {value} exceeds max {MAX_MSIZE}");
    }
    Ok(())
}

fn required_proc_9p_sessions_bytes(shard_count: usize) -> usize {
    let header = "sessions total=".len()
        + MAX_U64_DIGITS
        + " worker=".len()
        + MAX_U64_DIGITS
        + " shard_bits=".len()
        + MAX_U8_DIGITS
        + " shard_count=".len()
        + SHARD_COUNT_DIGITS
        + 1;
    let shard_line = "shard ".len() + SHARD_LABEL_BYTES + 1 + MAX_U64_DIGITS + 1;
    header + shard_count.saturating_mul(shard_line)
}

fn required_proc_9p_outstanding_bytes() -> usize {
    "outstanding current=".len() + MAX_U64_DIGITS + " limit=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_9p_short_writes_bytes() -> usize {
    "short_writes total=".len() + MAX_U64_DIGITS + " retries=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_9p_session_active_bytes() -> usize {
    "active=".len() + MAX_U64_DIGITS + " draining=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_9p_session_state_bytes() -> usize {
    "state=".len() + MAX_SESSION_STATE_LEN + 1
}

fn required_proc_9p_session_since_ms_bytes() -> usize {
    "since_ms=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_9p_session_owner_bytes() -> usize {
    "owner=".len() + MAX_SESSION_OWNER_LEN + 1
}

fn required_proc_ingest_p50_bytes() -> usize {
    "p50_ms=".len() + MAX_U32_DIGITS + 1
}

fn required_proc_ingest_p95_bytes() -> usize {
    "p95_ms=".len() + MAX_U32_DIGITS + 1
}

fn required_proc_ingest_backpressure_bytes() -> usize {
    "backpressure=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_ingest_dropped_bytes() -> usize {
    "dropped=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_ingest_queued_bytes() -> usize {
    "queued=".len() + MAX_U32_DIGITS + 1
}

fn required_proc_ingest_watch_line_bytes() -> usize {
    "watch ts_ms=".len()
        + MAX_U64_DIGITS
        + " p50_ms=".len()
        + MAX_U32_DIGITS
        + " p95_ms=".len()
        + MAX_U32_DIGITS
        + " queued=".len()
        + MAX_U32_DIGITS
        + " backpressure=".len()
        + MAX_U64_DIGITS
        + " dropped=".len()
        + MAX_U64_DIGITS
        + 1
}

fn required_proc_root_reachable_bytes() -> usize {
    "reachable=".len() + "yes".len() + 1
}

fn required_proc_root_last_seen_ms_bytes() -> usize {
    "last_seen_ms=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_root_cut_reason_bytes() -> usize {
    "cut_reason=".len() + MAX_ROOT_CUT_REASON_LEN + 1
}

fn required_proc_pressure_busy_bytes() -> usize {
    "busy=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_pressure_quota_bytes() -> usize {
    "quota=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_pressure_cut_bytes() -> usize {
    "cut=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_pressure_policy_bytes() -> usize {
    "policy=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_schedule_summary_bytes() -> usize {
    "queue=".len()
        + MAX_U64_DIGITS
        + " dequeued=".len()
        + MAX_U64_DIGITS
        + " dropped=".len()
        + MAX_U64_DIGITS
        + " max_entries=".len()
        + MAX_U32_DIGITS
        + 1
}

fn required_proc_schedule_queue_bytes() -> usize {
    "id=".len()
        + MAX_SCHEDULE_ID_LEN
        + " role=".len()
        + MAX_SCHEDULE_ROLE_LEN
        + " priority=".len()
        + MAX_U32_DIGITS
        + " ticks=".len()
        + MAX_U32_DIGITS
        + " budget_ms=".len()
        + MAX_U32_DIGITS
        + " seq=".len()
        + MAX_U64_DIGITS
        + 1
}

fn required_proc_lease_summary_bytes() -> usize {
    "active=".len()
        + MAX_U64_DIGITS
        + " preemptions=".len()
        + MAX_U64_DIGITS
        + " quotas=".len()
        + MAX_U64_DIGITS
        + " max_active=".len()
        + MAX_U32_DIGITS
        + " max_preemptions=".len()
        + MAX_U32_DIGITS
        + 1
}

fn required_proc_lease_active_bytes() -> usize {
    "id=".len()
        + MAX_LEASE_ID_LEN
        + " subject=".len()
        + MAX_LEASE_SUBJECT_LEN
        + " resource=".len()
        + MAX_LEASE_RESOURCE_LEN
        + " ttl_s=".len()
        + MAX_U32_DIGITS
        + " priority=".len()
        + MAX_U32_DIGITS
        + " state=".len()
        + MAX_COH_LEASE_STATE_LEN
        + " seq=".len()
        + MAX_U64_DIGITS
        + 1
}

fn required_proc_lease_preemptions_bytes() -> usize {
    "id=".len()
        + MAX_LEASE_ID_LEN
        + " subject=".len()
        + MAX_LEASE_SUBJECT_LEN
        + " resource=".len()
        + MAX_LEASE_RESOURCE_LEN
        + " reason=".len()
        + MAX_LEASE_REASON_LEN
        + " seq=".len()
        + MAX_U64_DIGITS
        + 1
}

fn validate_policy_rule(rule: &PolicyRule) -> Result<()> {
    let id = rule.id.trim();
    if id.is_empty() {
        bail!("ecosystem.policy.rules[].id must not be empty");
    }
    if id.len() > MAX_POLICY_RULE_ID_LEN {
        bail!(
            "ecosystem.policy.rules[].id '{}' exceeds max length {}",
            id,
            MAX_POLICY_RULE_ID_LEN
        );
    }
    let target = rule.target.trim();
    if !target.starts_with('/') {
        bail!("ecosystem.policy.rules[].target must be absolute");
    }
    let components: Vec<&str> = target.split('/').filter(|seg| !seg.is_empty()).collect();
    if components.is_empty() {
        bail!("ecosystem.policy.rules[].target must not be root");
    }
    if components.len() > MAX_WALK_DEPTH {
        bail!(
            "ecosystem.policy.rules[].target exceeds walk depth {}",
            MAX_WALK_DEPTH
        );
    }
    for component in components {
        if component == ".." {
            bail!("ecosystem.policy.rules[].target contains disallowed '..'");
        }
        if component.is_empty() {
            bail!("ecosystem.policy.rules[].target contains empty component");
        }
        if component.contains('*') && component != "*" {
            bail!("ecosystem.policy.rules[].target wildcard must be '*'");
        }
    }
    Ok(())
}

fn validate_host_federation_token(label: &str, value: &str, max_len: usize) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{label} must not be empty");
    }
    if trimmed.len() > max_len {
        bail!("{label} exceeds max length {}", max_len);
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':'))
    {
        bail!("{label} contains invalid characters");
    }
    Ok(())
}

pub fn load_manifest(path: &Path) -> Result<Manifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let manifest: Manifest = toml::from_str(&contents)
        .with_context(|| format!("invalid manifest TOML in {}", path.display()))?;
    Ok(manifest)
}

pub(crate) fn resolve_manifest_relative_path(base_dir: Option<&Path>, value: &str) -> PathBuf {
    let trimmed = value.trim();
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() || base_dir.is_none() {
        return candidate.to_path_buf();
    }
    let base = base_dir.unwrap_or_else(|| Path::new("."));
    let primary = base.join(candidate);
    if primary.exists() {
        return primary;
    }
    if let Some(parent) = base.parent() {
        let secondary = parent.join(candidate);
        if secondary.exists() {
            return secondary;
        }
    }
    primary
}

pub fn serialize_manifest(manifest: &Manifest) -> Result<Vec<u8>> {
    let json = serde_json::to_vec_pretty(manifest)?;
    Ok(json)
}

pub fn schema_version() -> &'static str {
    SCHEMA_VERSION
}
