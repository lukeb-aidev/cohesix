// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host ticket agent runtime primitives and ticket processing flow.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host ticket agent runtime and manifest-driven processing flow.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use cohsh::{Session, Transport};
use cohsh_core::MAX_ECHO_LEN;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::claim::{terminal_keys, TicketKey};
use crate::executors::ExecutorConfig;

/// Claim/idempotency helpers.
pub mod claim;
/// Ticket action executors.
pub mod executors;
/// Federated cross-hive relay worker.
pub mod relay;
/// Status receipt helpers.
pub mod status;
/// Relay write-ahead log persistence.
pub mod wal;

/// Default resolved manifest path.
pub const DEFAULT_RESOLVED_MANIFEST_PATH: &str = "configs/generated/root_task_resolved.json";
/// Default cursor state file path.
pub const DEFAULT_CURSOR_STATE_PATH: &str = "out/host-ticket-agent/cursor.json";
/// Default relay WAL state file path.
pub const DEFAULT_RELAY_WAL_PATH: &str = "out/host-ticket-agent/relay-wal.json";

const REQUIRED_LIFECYCLE_STATES: &[&str] = &[
    "queued",
    "claimed",
    "running",
    "succeeded",
    "failed",
    "expired",
];

/// Host ticket spec line (`/host/tickets/spec`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostTicketSpec {
    /// Request schema.
    pub schema: String,
    /// Stable ticket id.
    pub id: String,
    /// Idempotency key paired with `id`.
    pub idempotency_key: String,
    /// Action verb from the manifest allowlist.
    pub action: String,
    /// Optional action target path/token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Optional JSON arguments for the action.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub args: Value,
    /// Optional expiration timestamp in unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_unix_ms: Option<u64>,
    /// Optional originating hive identifier for federated relay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hive: Option<String>,
    /// Optional destination hive identifier for federated relay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_hive: Option<String>,
    /// Optional relay hop counter (monotonic across relays).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_hop: Option<u16>,
    /// Optional deterministic relay correlation id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_correlation_id: Option<String>,
}

/// Host ticket result line (`/host/tickets/status` or `/host/tickets/deadletter`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostTicketResult {
    /// Result schema.
    pub schema: String,
    /// Stable ticket id.
    pub id: String,
    /// Idempotency key paired with `id`.
    pub idempotency_key: String,
    /// Action verb from the manifest allowlist.
    pub action: String,
    /// Lifecycle state for this receipt.
    pub state: String,
    /// Optional bounded detail message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Optional originating hive identifier for federated relay receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hive: Option<String>,
    /// Optional destination hive identifier for federated relay receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_hive: Option<String>,
    /// Optional relay hop counter copied from the originating ticket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_hop: Option<u16>,
    /// Optional deterministic relay correlation id copied from the ticket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_correlation_id: Option<String>,
}

/// Manifest-driven ticket configuration for the host agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTicketManifest {
    /// Whether the ticket surface is enabled.
    pub enabled: bool,
    /// Host mount path (for example `/host`).
    pub mount_path: String,
    /// Ticket request schema.
    pub request_schema: String,
    /// Ticket result schema.
    pub result_schema: String,
    /// Maximum bytes per JSON line.
    pub max_line_bytes: u32,
    /// Allowlisted action strings.
    pub action_allowlist: Vec<String>,
    /// Lifecycle states accepted by the namespace.
    pub lifecycle: Vec<String>,
    /// Manifest-driven federation relay policy.
    pub federation: HostFederationManifest,
}

/// Manifest-driven peer descriptor for host ticket federation relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFederationPeer {
    /// Unique peer hive identifier.
    pub name: String,
    /// Peer gateway REST URL.
    pub rest_url: String,
    /// Environment variable key that stores the peer request-auth token.
    pub auth_ref: String,
}

/// Manifest-driven federation relay policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFederationManifest {
    /// Whether relay is enabled.
    pub enabled: bool,
    /// Local hive identifier.
    pub local_hive: String,
    /// Known remote peers.
    pub peers: Vec<HostFederationPeer>,
    /// Relay-allowlisted actions.
    pub action_allowlist: Vec<String>,
    /// Max queued intents allowed before backpressure.
    pub relay_queue_max_entries: u16,
    /// Max queued intent bytes allowed before backpressure.
    pub relay_queue_max_bytes: u32,
    /// Max WAL entries retained.
    pub wal_max_entries: u32,
    /// Max WAL bytes retained.
    pub wal_max_bytes: u32,
    /// Per-peer relay timeout in milliseconds.
    pub relay_timeout_ms: u32,
}

impl HostTicketManifest {
    /// Load ticket configuration from `configs/generated/root_task_resolved.json`.
    pub fn from_resolved_manifest(path: &Path) -> Result<Self> {
        let payload =
            fs::read(path).with_context(|| format!("read resolved manifest {}", path.display()))?;
        let manifest: ResolvedManifest =
            serde_json::from_slice(&payload).context("parse resolved manifest JSON")?;
        let host = manifest.ecosystem.host;
        let tickets = host.tickets;
        let federation = host.federation;

        let mount_path = normalise_absolute_path(host.mount_at.as_str())
            .context("invalid ecosystem.host.mount_at")?;
        let enabled = host.enable && tickets.enable;

        let request_schema = tickets.request_schema.trim().to_owned();
        let result_schema = tickets.result_schema.trim().to_owned();
        if enabled && request_schema.is_empty() {
            return Err(anyhow!(
                "ecosystem.host.tickets.request_schema must not be empty when enabled"
            ));
        }
        if enabled && result_schema.is_empty() {
            return Err(anyhow!(
                "ecosystem.host.tickets.result_schema must not be empty when enabled"
            ));
        }

        if enabled && tickets.max_line_bytes == 0 {
            return Err(anyhow!(
                "ecosystem.host.tickets.max_line_bytes must be >= 1 when enabled"
            ));
        }

        let mut actions = dedupe_tokens(tickets.action_allowlist);
        if enabled && actions.is_empty() {
            return Err(anyhow!(
                "ecosystem.host.tickets.action_allowlist must not be empty when enabled"
            ));
        }
        actions.sort();

        let mut lifecycle = dedupe_tokens(tickets.lifecycle);
        lifecycle.sort();
        if enabled {
            let lifecycle_set: HashSet<&str> = lifecycle.iter().map(String::as_str).collect();
            for required in REQUIRED_LIFECYCLE_STATES {
                if !lifecycle_set.contains(required) {
                    return Err(anyhow!(
                        "ecosystem.host.tickets.lifecycle missing required state '{required}'"
                    ));
                }
            }
        }

        let mut federation_actions = dedupe_tokens(federation.action_allowlist);
        federation_actions.sort();
        let mut federation_peers = Vec::<HostFederationPeer>::new();
        for peer in federation.peers {
            let name =
                normalise_token("ecosystem.host.federation.peers[].name", peer.name.as_str())?;
            let rest_url = normalise_rest_url(peer.rest_url.as_str())?;
            let auth_ref = normalise_token(
                "ecosystem.host.federation.peers[].auth_ref",
                peer.auth_ref.as_str(),
            )?;
            federation_peers.push(HostFederationPeer {
                name,
                rest_url,
                auth_ref,
            });
        }
        federation_peers.sort_by(|left, right| left.name.cmp(&right.name));
        let federation_enabled = enabled && federation.enable;
        if federation_enabled {
            if federation_peers.is_empty() {
                return Err(anyhow!(
                    "ecosystem.host.federation.peers must not be empty when enabled"
                ));
            }
            if federation_actions.is_empty() {
                return Err(anyhow!(
                    "ecosystem.host.federation.action_allowlist must not be empty when enabled"
                ));
            }
            if federation.relay_queue_max_entries == 0 {
                return Err(anyhow!(
                    "ecosystem.host.federation.relay_queue_max_entries must be >= 1 when enabled"
                ));
            }
            if federation.relay_queue_max_bytes == 0 {
                return Err(anyhow!(
                    "ecosystem.host.federation.relay_queue_max_bytes must be >= 1 when enabled"
                ));
            }
            if federation.wal_max_entries == 0 {
                return Err(anyhow!(
                    "ecosystem.host.federation.wal_max_entries must be >= 1 when enabled"
                ));
            }
            if federation.wal_max_bytes == 0 {
                return Err(anyhow!(
                    "ecosystem.host.federation.wal_max_bytes must be >= 1 when enabled"
                ));
            }
            if federation.relay_timeout_ms == 0 {
                return Err(anyhow!(
                    "ecosystem.host.federation.relay_timeout_ms must be >= 1 when enabled"
                ));
            }
        }

        let local_hive = normalise_token(
            "ecosystem.host.federation.local_hive",
            federation.local_hive.as_str(),
        )?;

        Ok(Self {
            enabled,
            mount_path,
            request_schema,
            result_schema,
            max_line_bytes: tickets.max_line_bytes,
            action_allowlist: actions,
            lifecycle,
            federation: HostFederationManifest {
                enabled: federation_enabled,
                local_hive,
                peers: federation_peers,
                action_allowlist: federation_actions,
                relay_queue_max_entries: federation.relay_queue_max_entries,
                relay_queue_max_bytes: federation.relay_queue_max_bytes,
                wal_max_entries: federation.wal_max_entries,
                wal_max_bytes: federation.wal_max_bytes,
                relay_timeout_ms: federation.relay_timeout_ms,
            },
        })
    }

    /// Optional override for the mount path.
    pub fn with_mount_path(mut self, mount_path: &str) -> Result<Self> {
        self.mount_path = normalise_absolute_path(mount_path)?;
        Ok(self)
    }

    /// `/host/tickets/spec` path.
    #[must_use]
    pub fn spec_path(&self) -> String {
        format!("{}{}", self.mount_root(), "/tickets/spec")
    }

    /// `/host/tickets/status` path.
    #[must_use]
    pub fn status_path(&self) -> String {
        format!("{}{}", self.mount_root(), "/tickets/status")
    }

    /// `/host/tickets/deadletter` path.
    #[must_use]
    pub fn deadletter_path(&self) -> String {
        format!("{}{}", self.mount_root(), "/tickets/deadletter")
    }

    fn mount_root(&self) -> &str {
        self.mount_path.trim_end_matches('/')
    }
}

/// Per-run processing counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessSummary {
    /// Total specs evaluated from the current cursor onward.
    pub seen: usize,
    /// Terminal-success specs processed in this pass.
    pub succeeded: usize,
    /// Terminal-failure specs processed in this pass.
    pub failed: usize,
    /// Expired specs written to deadletter in this pass.
    pub expired: usize,
    /// Specs skipped because an existing terminal receipt was found.
    pub skipped_terminal: usize,
    /// Federated specs skipped because they target a different hive.
    pub skipped_remote_target: usize,
}

impl ProcessSummary {
    /// Total terminal updates generated in this pass.
    #[must_use]
    pub fn terminal_updates(self) -> usize {
        self.succeeded
            .saturating_add(self.failed)
            .saturating_add(self.expired)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CursorState {
    next_spec_index: usize,
}

/// Process one polling pass using the built-in executor dispatcher.
pub fn process_tickets_once(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    cursor_path: &Path,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
) -> Result<ProcessSummary> {
    process_tickets_once_with_executor(
        transport,
        session,
        manifest,
        cursor_path,
        executor_config,
        now_unix_ms,
        |inner_transport, inner_session, spec, config| {
            executors::execute_action(inner_transport, inner_session, spec, config)
        },
    )
}

/// Process one polling pass with a caller-supplied executor callback (tests/hooks).
pub fn process_tickets_once_with_executor<F>(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    cursor_path: &Path,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
    mut executor: F,
) -> Result<ProcessSummary>
where
    F: FnMut(&mut dyn Transport, &Session, &HostTicketSpec, &ExecutorConfig) -> Result<String>,
{
    if !manifest.enabled {
        return Ok(ProcessSummary::default());
    }

    let spec_path = manifest.spec_path();
    let status_path = manifest.status_path();
    let deadletter_path = manifest.deadletter_path();

    let spec_lines = transport
        .read(session, spec_path.as_str())
        .with_context(|| format!("read {}", spec_path))?;
    let status_lines = transport
        .read(session, status_path.as_str())
        .with_context(|| format!("read {}", status_path))?;
    let deadletter_lines = transport
        .read(session, deadletter_path.as_str())
        .with_context(|| format!("read {}", deadletter_path))?;

    let specs = claim::parse_spec_lines(
        &spec_lines,
        manifest.request_schema.as_str(),
        manifest.max_line_bytes,
    )?;
    let mut results = claim::parse_result_lines(
        &status_lines,
        manifest.result_schema.as_str(),
        manifest.max_line_bytes,
    )?;
    let mut deadletters = claim::parse_result_lines(
        &deadletter_lines,
        manifest.result_schema.as_str(),
        manifest.max_line_bytes,
    )?;
    results.append(&mut deadletters);
    let mut terminal = terminal_keys(&results);

    let mut cursor = load_cursor_state(cursor_path)?;
    if cursor.next_spec_index > specs.len() {
        cursor.next_spec_index = specs.len();
        save_cursor_state(cursor_path, &cursor)?;
    }

    let mut summary = ProcessSummary::default();

    for spec in specs.iter().skip(cursor.next_spec_index) {
        summary.seen = summary.seen.saturating_add(1);

        if let Some(target_hive) = spec.target_hive.as_deref() {
            if target_hive != manifest.federation.local_hive {
                summary.skipped_remote_target = summary.skipped_remote_target.saturating_add(1);
                cursor.next_spec_index = cursor.next_spec_index.saturating_add(1);
                save_cursor_state(cursor_path, &cursor)?;
                continue;
            }
        }

        let key = TicketKey::new(spec.id.as_str(), spec.idempotency_key.as_str());
        if terminal.contains(&key) {
            summary.skipped_terminal = summary.skipped_terminal.saturating_add(1);
            cursor.next_spec_index = cursor.next_spec_index.saturating_add(1);
            save_cursor_state(cursor_path, &cursor)?;
            continue;
        }

        if let Some(expires_unix_ms) = spec.expires_unix_ms {
            if now_unix_ms >= expires_unix_ms {
                append_result(
                    transport,
                    session,
                    manifest,
                    spec,
                    "expired",
                    Some("ticket expired before execution"),
                    deadletter_path.as_str(),
                )?;
                summary.expired = summary.expired.saturating_add(1);
                terminal.insert(key);
                cursor.next_spec_index = cursor.next_spec_index.saturating_add(1);
                save_cursor_state(cursor_path, &cursor)?;
                continue;
            }
        }

        append_result(
            transport,
            session,
            manifest,
            spec,
            "claimed",
            Some("claimed by host-ticket-agent"),
            status_path.as_str(),
        )?;
        append_result(
            transport,
            session,
            manifest,
            spec,
            "running",
            Some("executor started"),
            status_path.as_str(),
        )?;

        match executor(transport, session, spec, executor_config) {
            Ok(message) => {
                append_result(
                    transport,
                    session,
                    manifest,
                    spec,
                    "succeeded",
                    Some(message.as_str()),
                    status_path.as_str(),
                )?;
                summary.succeeded = summary.succeeded.saturating_add(1);
                terminal.insert(key);
            }
            Err(err) => {
                let detail = bounded_detail(err.as_ref(), 192);
                append_result(
                    transport,
                    session,
                    manifest,
                    spec,
                    "failed",
                    Some(detail.as_str()),
                    deadletter_path.as_str(),
                )?;
                summary.failed = summary.failed.saturating_add(1);
                terminal.insert(key);
            }
        }

        cursor.next_spec_index = cursor.next_spec_index.saturating_add(1);
        save_cursor_state(cursor_path, &cursor)?;
    }

    Ok(summary)
}

/// Current unix timestamp in milliseconds.
#[must_use]
pub fn unix_time_ms_now() -> u64 {
    let now = SystemTime::now();
    now.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn append_result(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    spec: &HostTicketSpec,
    state: &str,
    message: Option<&str>,
    path: &str,
) -> Result<()> {
    let line_limit = effective_result_line_limit(manifest.max_line_bytes);
    let line = status::build_result_line(
        spec,
        manifest.result_schema.as_str(),
        state,
        message,
        line_limit,
    )?;
    status::append_result_line(transport, session, path, line.as_str())
}

fn effective_result_line_limit(max_line_bytes: u32) -> u32 {
    max_line_bytes.min(MAX_ECHO_LEN as u32)
}

fn load_cursor_state(path: &Path) -> Result<CursorState> {
    let payload = match fs::read(path) {
        Ok(payload) => payload,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(CursorState::default()),
        Err(err) => {
            return Err(err).with_context(|| format!("read cursor state {}", path.display()))
        }
    };
    let state: CursorState = serde_json::from_slice(&payload)
        .with_context(|| format!("parse cursor state {}", path.display()))?;
    Ok(state)
}

fn save_cursor_state(path: &Path, state: &CursorState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create cursor state dir {}", parent.display()))?;
    }
    let payload = serde_json::to_vec_pretty(state).context("serialize cursor state")?;
    let tmp = path.with_extension("partial");
    fs::write(&tmp, &payload).with_context(|| format!("write cursor state {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("commit cursor state {}", path.display()))?;
    Ok(())
}

fn dedupe_tokens(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_owned()) {
            out.push(trimmed.to_owned());
        }
    }
    out
}

fn normalise_token(label: &str, token: &str) -> Result<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("{label} must not be empty"));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':'))
    {
        return Err(anyhow!("{label} contains invalid characters"));
    }
    Ok(trimmed.to_owned())
}

fn normalise_rest_url(url: &str) -> Result<String> {
    let mut trimmed = url.trim().to_owned();
    if trimmed.is_empty() {
        return Err(anyhow!(
            "ecosystem.host.federation.peers[].rest_url must not be empty"
        ));
    }
    while trimmed.ends_with('/') {
        trimmed.pop();
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(anyhow!(
            "ecosystem.host.federation.peers[].rest_url must start with http:// or https://"
        ));
    }
    Ok(trimmed)
}

fn normalise_absolute_path(path: &str) -> Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("path must not be empty"));
    }
    if !trimmed.starts_with('/') {
        return Err(anyhow!("path must be absolute"));
    }
    let mut out = trimmed.trim_end_matches('/').to_owned();
    if out.is_empty() {
        out.push('/');
    }
    for component in out.split('/').filter(|segment| !segment.is_empty()) {
        if component == "." || component == ".." {
            return Err(anyhow!("path contains invalid component '{component}'"));
        }
    }
    if out == "/" {
        return Err(anyhow!("path must not be '/'"));
    }
    Ok(out)
}

fn bounded_detail(err: &dyn std::error::Error, max_chars: usize) -> String {
    let text = err.to_string();
    if text.len() <= max_chars {
        return text;
    }
    text[..max_chars].to_owned()
}

#[derive(Debug, Deserialize, Default)]
struct ResolvedManifest {
    #[serde(default)]
    ecosystem: ResolvedEcosystem,
}

#[derive(Debug, Deserialize, Default)]
struct ResolvedEcosystem {
    #[serde(default)]
    host: ResolvedHost,
}

#[derive(Debug, Deserialize)]
struct ResolvedHost {
    #[serde(default)]
    enable: bool,
    #[serde(default = "default_host_mount")]
    mount_at: String,
    #[serde(default)]
    tickets: ResolvedHostTickets,
    #[serde(default)]
    federation: ResolvedHostFederation,
}

impl Default for ResolvedHost {
    fn default() -> Self {
        Self {
            enable: false,
            mount_at: default_host_mount(),
            tickets: ResolvedHostTickets::default(),
            federation: ResolvedHostFederation::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResolvedHostTickets {
    #[serde(default)]
    enable: bool,
    #[serde(default = "default_request_schema")]
    request_schema: String,
    #[serde(default = "default_result_schema")]
    result_schema: String,
    #[serde(default = "default_max_line_bytes")]
    max_line_bytes: u32,
    #[serde(default)]
    action_allowlist: Vec<String>,
    #[serde(default)]
    lifecycle: Vec<String>,
}

impl Default for ResolvedHostTickets {
    fn default() -> Self {
        Self {
            enable: false,
            request_schema: default_request_schema(),
            result_schema: default_result_schema(),
            max_line_bytes: default_max_line_bytes(),
            action_allowlist: Vec::new(),
            lifecycle: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResolvedHostFederation {
    #[serde(default)]
    enable: bool,
    #[serde(default = "default_local_hive")]
    local_hive: String,
    #[serde(default)]
    peers: Vec<ResolvedHostFederationPeer>,
    #[serde(default)]
    action_allowlist: Vec<String>,
    #[serde(default = "default_relay_queue_max_entries")]
    relay_queue_max_entries: u16,
    #[serde(default = "default_relay_queue_max_bytes")]
    relay_queue_max_bytes: u32,
    #[serde(default = "default_wal_max_entries")]
    wal_max_entries: u32,
    #[serde(default = "default_wal_max_bytes")]
    wal_max_bytes: u32,
    #[serde(default = "default_relay_timeout_ms")]
    relay_timeout_ms: u32,
}

impl Default for ResolvedHostFederation {
    fn default() -> Self {
        Self {
            enable: false,
            local_hive: default_local_hive(),
            peers: Vec::new(),
            action_allowlist: Vec::new(),
            relay_queue_max_entries: default_relay_queue_max_entries(),
            relay_queue_max_bytes: default_relay_queue_max_bytes(),
            wal_max_entries: default_wal_max_entries(),
            wal_max_bytes: default_wal_max_bytes(),
            relay_timeout_ms: default_relay_timeout_ms(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResolvedHostFederationPeer {
    #[serde(default)]
    name: String,
    #[serde(default)]
    rest_url: String,
    #[serde(default)]
    auth_ref: String,
}

fn default_host_mount() -> String {
    "/host".to_owned()
}

fn default_request_schema() -> String {
    "host-ticket/v1".to_owned()
}

fn default_result_schema() -> String {
    "host-ticket-result/v1".to_owned()
}

fn default_max_line_bytes() -> u32 {
    2048
}

fn default_local_hive() -> String {
    "hive-a".to_owned()
}

fn default_relay_queue_max_entries() -> u16 {
    256
}

fn default_relay_queue_max_bytes() -> u32 {
    32 * 1024
}

fn default_wal_max_entries() -> u32 {
    1024
}

fn default_wal_max_bytes() -> u32 {
    512 * 1024
}

fn default_relay_timeout_ms() -> u32 {
    1500
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use cohesix_ticket::Role;

    use super::*;

    #[test]
    fn manifest_loader_reads_ticket_config() {
        let temp = tempfile::TempDir::new().unwrap_or_else(|err| unreachable!("temp dir: {err}"));
        let path = temp.path().join("resolved.json");
        let payload = r#"{
          "ecosystem": {
            "host": {
              "enable": true,
              "mount_at": "/host",
              "tickets": {
                "enable": true,
                "request_schema": "host-ticket/v1",
                "result_schema": "host-ticket-result/v1",
                "max_line_bytes": 2048,
                "action_allowlist": ["systemd.restart"],
                "lifecycle": ["queued","claimed","running","succeeded","failed","expired"]
              },
              "federation": {
                "enable": true,
                "local_hive": "hive-a",
                "action_allowlist": ["systemd.restart"],
                "relay_queue_max_entries": 128,
                "relay_queue_max_bytes": 16384,
                "wal_max_entries": 512,
                "wal_max_bytes": 262144,
                "relay_timeout_ms": 2500,
                "peers": [
                  {
                    "name": "hive-b",
                    "rest_url": "http://127.0.0.1:8081",
                    "auth_ref": "COHESIX_RELAY_HIVE_B_TOKEN"
                  }
                ]
              }
            }
          }
        }"#;
        fs::write(&path, payload).unwrap_or_else(|err| unreachable!("write manifest: {err}"));
        let config = HostTicketManifest::from_resolved_manifest(&path)
            .unwrap_or_else(|err| unreachable!("load manifest: {err}"));
        assert!(config.enabled);
        assert_eq!(config.mount_path, "/host");
        assert_eq!(config.spec_path(), "/host/tickets/spec");
        assert!(config.federation.enabled);
        assert_eq!(config.federation.local_hive, "hive-a");
        assert_eq!(config.federation.peers.len(), 1);
    }

    #[test]
    fn process_once_is_cursor_and_terminal_safe() {
        let temp = tempfile::TempDir::new().unwrap_or_else(|err| unreachable!("temp dir: {err}"));
        let cursor = temp.path().join("cursor.json");
        let manifest = HostTicketManifest {
            enabled: true,
            mount_path: "/host".to_owned(),
            request_schema: "host-ticket/v1".to_owned(),
            result_schema: "host-ticket-result/v1".to_owned(),
            max_line_bytes: 2048,
            action_allowlist: vec!["systemd.restart".to_owned()],
            lifecycle: REQUIRED_LIFECYCLE_STATES
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            federation: HostFederationManifest {
                enabled: false,
                local_hive: "hive-a".to_owned(),
                peers: Vec::new(),
                action_allowlist: Vec::new(),
                relay_queue_max_entries: 256,
                relay_queue_max_bytes: 32 * 1024,
                wal_max_entries: 1024,
                wal_max_bytes: 512 * 1024,
                relay_timeout_ms: 1500,
            },
        };
        let mut files = BTreeMap::<String, Vec<String>>::new();
        files.insert(
            manifest.spec_path(),
            vec![
                "{\"schema\":\"host-ticket/v1\",\"id\":\"t1\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\"}".to_owned(),
                "{\"schema\":\"host-ticket/v1\",\"id\":\"t1\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\"}".to_owned(),
            ],
        );
        files.insert(manifest.status_path(), Vec::new());
        files.insert(manifest.deadletter_path(), Vec::new());
        let mut transport = FakeTransport {
            files,
            max_write_line_len: None,
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig {
            mount: "/host".to_owned(),
            registry_root: PathBuf::from("out/model_registry"),
        };

        let mut executions = 0usize;
        let summary = process_tickets_once_with_executor(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &config,
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                executions = executions.saturating_add(1);
                Ok("ok".to_owned())
            },
        )
        .unwrap_or_else(|err| unreachable!("process pass: {err}"));
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.skipped_terminal, 1);
        assert_eq!(executions, 1);

        let second = process_tickets_once_with_executor(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &config,
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                unreachable!("second pass should not execute");
            },
        )
        .unwrap_or_else(|err| unreachable!("second pass: {err}"));
        assert_eq!(second.seen, 0);
    }

    #[test]
    fn process_once_skips_remote_federated_targets() {
        let temp = tempfile::TempDir::new().unwrap_or_else(|err| unreachable!("temp dir: {err}"));
        let cursor = temp.path().join("cursor.json");
        let manifest = HostTicketManifest {
            enabled: true,
            mount_path: "/host".to_owned(),
            request_schema: "host-ticket/v1".to_owned(),
            result_schema: "host-ticket-result/v1".to_owned(),
            max_line_bytes: 2048,
            action_allowlist: vec!["systemd.restart".to_owned()],
            lifecycle: REQUIRED_LIFECYCLE_STATES
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            federation: HostFederationManifest {
                enabled: true,
                local_hive: "hive-a".to_owned(),
                peers: Vec::new(),
                action_allowlist: vec!["systemd.restart".to_owned()],
                relay_queue_max_entries: 256,
                relay_queue_max_bytes: 32 * 1024,
                wal_max_entries: 1024,
                wal_max_bytes: 512 * 1024,
                relay_timeout_ms: 1500,
            },
        };

        let mut files = BTreeMap::<String, Vec<String>>::new();
        files.insert(
            manifest.spec_path(),
            vec![
                "{\"schema\":\"host-ticket/v1\",\"id\":\"t-remote\",\"idempotency_key\":\"k-remote\",\"action\":\"systemd.restart\",\"source_hive\":\"hive-a\",\"target_hive\":\"hive-b\"}".to_owned(),
            ],
        );
        files.insert(manifest.status_path(), Vec::new());
        files.insert(manifest.deadletter_path(), Vec::new());

        let mut transport = FakeTransport {
            files,
            max_write_line_len: None,
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig {
            mount: "/host".to_owned(),
            registry_root: PathBuf::from("out/model_registry"),
        };

        let mut executions = 0usize;
        let summary = process_tickets_once_with_executor(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &config,
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                executions = executions.saturating_add(1);
                Ok("ok".to_owned())
            },
        )
        .unwrap_or_else(|err| unreachable!("process pass: {err}"));

        assert_eq!(summary.seen, 1);
        assert_eq!(summary.skipped_remote_target, 1);
        assert_eq!(summary.terminal_updates(), 0);
        assert_eq!(executions, 0);
    }

    #[test]
    fn process_once_compacts_federated_results_for_echo_limit() {
        let temp = tempfile::TempDir::new().unwrap_or_else(|err| unreachable!("temp dir: {err}"));
        let cursor = temp.path().join("cursor.json");
        let manifest = HostTicketManifest {
            enabled: true,
            mount_path: "/host".to_owned(),
            request_schema: "host-ticket/v1".to_owned(),
            result_schema: "host-ticket-result/v1".to_owned(),
            max_line_bytes: 2048,
            action_allowlist: vec!["systemd.stop".to_owned()],
            lifecycle: REQUIRED_LIFECYCLE_STATES
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            federation: HostFederationManifest {
                enabled: true,
                local_hive: "hive-b".to_owned(),
                peers: Vec::new(),
                action_allowlist: vec!["systemd.stop".to_owned()],
                relay_queue_max_entries: 256,
                relay_queue_max_bytes: 32 * 1024,
                wal_max_entries: 1024,
                wal_max_bytes: 512 * 1024,
                relay_timeout_ms: 1500,
            },
        };

        let mut files = BTreeMap::<String, Vec<String>>::new();
        files.insert(
            manifest.spec_path(),
            vec![
                "{\"schema\":\"host-ticket/v1\",\"id\":\"fed-ticket-1\",\"idempotency_key\":\"idem-1\",\"action\":\"systemd.stop\",\"source_hive\":\"hive-a\",\"target_hive\":\"hive-b\",\"relay_hop\":2,\"relay_correlation_id\":\"fed-ticket-1:idem-1:hive-a:hive-b\"}".to_owned(),
            ],
        );
        files.insert(manifest.status_path(), Vec::new());
        files.insert(manifest.deadletter_path(), Vec::new());

        let mut transport = FakeTransport {
            files,
            max_write_line_len: Some(224),
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig {
            mount: "/host".to_owned(),
            registry_root: PathBuf::from("out/model_registry"),
        };

        let summary = process_tickets_once_with_executor(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &config,
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| Ok("ok".to_owned()),
        )
        .unwrap_or_else(|err| unreachable!("process pass: {err}"));

        assert_eq!(summary.succeeded, 1);
    }

    #[derive(Debug)]
    struct FakeTransport {
        files: BTreeMap<String, Vec<String>>,
        max_write_line_len: Option<usize>,
    }

    impl Transport for FakeTransport {
        fn attach(&mut self, _role: Role, _ticket: Option<&str>) -> Result<Session> {
            Ok(Session::new(1.into(), Role::Queen))
        }

        fn ping(&mut self, _session: &Session) -> Result<String> {
            Ok("pong".to_owned())
        }

        fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
            self.read(_session, path)
        }

        fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
            Ok(self.files.get(path).cloned().unwrap_or_default())
        }

        fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
            Ok(self.files.get(path).cloned().unwrap_or_default())
        }

        fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
            let text = std::str::from_utf8(payload).context("fake payload utf8")?;
            let entry = self.files.entry(path.to_owned()).or_default();
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Some(limit) = self.max_write_line_len {
                        if trimmed.len() > limit {
                            return Err(anyhow!("payload exceeds max_echo_len {limit}"));
                        }
                    }
                    entry.push(trimmed.to_owned());
                }
            }
            Ok(())
        }
    }
}
