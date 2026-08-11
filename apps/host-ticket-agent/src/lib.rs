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
/// UTF-8-safe bounded text helpers.
pub mod text;
/// Relay write-ahead log persistence.
pub mod wal;

/// Default resolved manifest path.
pub const DEFAULT_RESOLVED_MANIFEST_PATH: &str = "configs/generated/root_task_resolved.json";
/// Default cursor state file path.
pub const DEFAULT_CURSOR_STATE_PATH: &str = "out/host-ticket-agent/cursor.json";
/// Default relay WAL state file path.
pub const DEFAULT_RELAY_WAL_PATH: &str = "out/host-ticket-agent/relay-wal.json";
/// Default durable version-2 execution journal path.
pub const DEFAULT_EXECUTION_JOURNAL_PATH: &str = "out/host-ticket-agent/execution-journal.json";
/// Default single-agent execution fence path.
pub const DEFAULT_EXECUTION_LOCK_PATH: &str = "out/host-ticket-agent/agent.lock";

/// Compatibility request schema for non-receipt host actions.
pub const HOST_TICKET_V1_SCHEMA: &str = "host-ticket/v1";
/// Root-admitted request schema for Worker receipt actions.
pub const HOST_TICKET_V2_SCHEMA: &str = "host-ticket/v2";
/// Compatibility result schema for non-receipt host actions.
pub const HOST_TICKET_RESULT_V1_SCHEMA: &str = "host-ticket-result/v1";
/// Result schema for Worker receipt actions.
pub const HOST_TICKET_RESULT_V2_SCHEMA: &str = "host-ticket-result/v2";

const REQUIRED_LIFECYCLE_STATES: &[&str] = &[
    "queued",
    "claimed",
    "running",
    "succeeded",
    "failed",
    "expired",
];
const HOST_TICKET_V1_ECHO_COMPAT_MAX_BYTES: u32 = 224;

/// Host ticket spec line (`/host/tickets/spec`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptMode {
    /// Compatibility action with no VM Worker receipt.
    None,
    /// Root-correlated receipt delivered to an already READY Worker.
    Worker,
}

/// Host ticket spec line (`/host/tickets/spec` or root-owned `spec.snapshot`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Receipt policy selected by the generated action/schema matrix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_mode: Option<ReceiptMode>,
    /// Stable provider operation identifier for version-2 requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Immutable provider subject (lease, device, job, or model) reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_ref: Option<String>,
    /// Canonical receipt Worker role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_worker_role: Option<String>,
    /// Public receipt Worker instance id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_worker_id: Option<String>,
    /// Opaque Worker supervisor generation selected by the caller and pinned by root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_supervisor_generation: Option<u64>,
    /// Opaque Worker capability generation selected by the caller and pinned by root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_cap_generation: Option<u64>,
    /// Root-resolved executable slot. Present only in `spec.snapshot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_worker_slot: Option<u16>,
    /// Root-resolved logical lease epoch. Present only in `spec.snapshot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_lease_epoch: Option<u64>,
    /// Root-owned monotonic admission sequence. Present only in `spec.snapshot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission_sequence: Option<u64>,
}

impl Default for HostTicketSpec {
    fn default() -> Self {
        Self {
            schema: HOST_TICKET_V1_SCHEMA.to_owned(),
            id: String::new(),
            idempotency_key: String::new(),
            action: String::new(),
            target: None,
            args: Value::Null,
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
            receipt_mode: None,
            operation_id: None,
            subject_ref: None,
            receipt_worker_role: None,
            receipt_worker_id: None,
            receipt_supervisor_generation: None,
            receipt_cap_generation: None,
            resolved_worker_slot: None,
            resolved_lease_epoch: None,
            admission_sequence: None,
        }
    }
}

/// Host ticket result line (`/host/tickets/status` or `/host/tickets/deadletter`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Receipt policy echoed from an admitted version-2 request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_mode: Option<ReceiptMode>,
    /// Provider operation id echoed from version 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Provider subject reference echoed from version 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_ref: Option<String>,
    /// Receipt Worker role echoed from version 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_worker_role: Option<String>,
    /// Receipt Worker public id echoed from version 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_worker_id: Option<String>,
    /// Receipt Worker supervisor generation echoed from version 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_supervisor_generation: Option<u64>,
    /// Receipt Worker capability generation echoed from version 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_cap_generation: Option<u64>,
    /// Root-resolved executable Worker slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_worker_slot: Option<u16>,
    /// Root-resolved logical Worker lease epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_lease_epoch: Option<u64>,
    /// Root-owned admission sequence for this normalized request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission_sequence: Option<u64>,
    /// SHA-256 of the canonical serialized result fields other than this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
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
    /// Generated accepted request schemas.
    pub accepted_request_schemas: Vec<String>,
    /// Generated accepted result schemas.
    pub accepted_result_schemas: Vec<String>,
    /// Maximum bytes per JSON line.
    pub max_line_bytes: u32,
    /// Allowlisted action strings.
    pub action_allowlist: Vec<String>,
    /// Exact actions allowed to request a Worker receipt.
    pub receipt_action_allowlist: Vec<String>,
    /// Lifecycle states accepted by the namespace.
    pub lifecycle: Vec<String>,
    /// Manifest-driven federation relay policy.
    pub federation: HostFederationManifest,
}

impl Default for HostTicketManifest {
    fn default() -> Self {
        Self {
            enabled: false,
            mount_path: "/host".to_owned(),
            request_schema: HOST_TICKET_V1_SCHEMA.to_owned(),
            result_schema: HOST_TICKET_RESULT_V1_SCHEMA.to_owned(),
            accepted_request_schemas: default_accepted_request_schemas(),
            accepted_result_schemas: default_accepted_result_schemas(),
            max_line_bytes: 2048,
            action_allowlist: Vec::new(),
            receipt_action_allowlist: vec![
                "gpu.lease.grant".to_owned(),
                "gpu.lease.renew".to_owned(),
                "gpu.lease.release".to_owned(),
                "peft.export".to_owned(),
                "peft.import".to_owned(),
                "peft.activate".to_owned(),
                "peft.rollback".to_owned(),
            ],
            lifecycle: REQUIRED_LIFECYCLE_STATES
                .iter()
                .map(|state| (*state).to_owned())
                .collect(),
            federation: HostFederationManifest::default(),
        }
    }
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

impl Default for HostFederationManifest {
    fn default() -> Self {
        Self {
            enabled: false,
            local_hive: "hive-a".to_owned(),
            peers: Vec::new(),
            action_allowlist: Vec::new(),
            relay_queue_max_entries: 256,
            relay_queue_max_bytes: 32 * 1024,
            wal_max_entries: 1024,
            wal_max_bytes: 512 * 1024,
            relay_timeout_ms: 1500,
        }
    }
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

        let mut accepted_request_schemas = dedupe_tokens(tickets.accepted_request_schemas);
        accepted_request_schemas.sort();
        let mut accepted_result_schemas = dedupe_tokens(tickets.accepted_result_schemas);
        accepted_result_schemas.sort();
        if enabled
            && (!accepted_request_schemas
                .iter()
                .any(|schema| schema == HOST_TICKET_V1_SCHEMA)
                || !accepted_request_schemas
                    .iter()
                    .any(|schema| schema == HOST_TICKET_V2_SCHEMA))
        {
            return Err(anyhow!(
                "ecosystem.host.tickets.accepted_request_schemas must include host-ticket/v1 and host-ticket/v2"
            ));
        }
        if enabled
            && (!accepted_result_schemas
                .iter()
                .any(|schema| schema == HOST_TICKET_RESULT_V1_SCHEMA)
                || !accepted_result_schemas
                    .iter()
                    .any(|schema| schema == HOST_TICKET_RESULT_V2_SCHEMA))
        {
            return Err(anyhow!(
                "ecosystem.host.tickets.accepted_result_schemas must include host-ticket-result/v1 and host-ticket-result/v2"
            ));
        }

        let mut receipt_actions = dedupe_tokens(tickets.receipt_action_allowlist);
        receipt_actions.sort();
        let mut expected_receipt_actions = vec![
            "gpu.lease.grant".to_owned(),
            "gpu.lease.release".to_owned(),
            "gpu.lease.renew".to_owned(),
            "peft.activate".to_owned(),
            "peft.export".to_owned(),
            "peft.import".to_owned(),
            "peft.rollback".to_owned(),
        ];
        expected_receipt_actions.sort();
        if enabled && receipt_actions != expected_receipt_actions {
            return Err(anyhow!(
                "ecosystem.host.tickets.receipt_action_allowlist must contain exactly the three GPU and four PEFT receipt actions"
            ));
        }
        if receipt_actions
            .iter()
            .any(|action| !actions.iter().any(|allowed| allowed == action))
        {
            return Err(anyhow!(
                "ecosystem.host.tickets.receipt_action_allowlist must be a subset of action_allowlist"
            ));
        }

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
            accepted_request_schemas,
            accepted_result_schemas,
            max_line_bytes: tickets.max_line_bytes,
            action_allowlist: actions,
            receipt_action_allowlist: receipt_actions,
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

    /// Root-owned canonical `/host/tickets/spec.snapshot` path.
    #[must_use]
    pub fn spec_snapshot_path(&self) -> String {
        format!("{}{}", self.mount_root(), "/tickets/spec.snapshot")
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

    /// Return the generated result schema corresponding to a request schema.
    pub fn result_schema_for(&self, request_schema: &str) -> Result<&str> {
        let required = match request_schema {
            HOST_TICKET_V1_SCHEMA => HOST_TICKET_RESULT_V1_SCHEMA,
            HOST_TICKET_V2_SCHEMA => HOST_TICKET_RESULT_V2_SCHEMA,
            other => return Err(anyhow!("unsupported host ticket request schema {other}")),
        };
        self.accepted_result_schemas
            .iter()
            .find(|schema| schema.as_str() == required)
            .map(String::as_str)
            .ok_or_else(|| anyhow!("generated accepted result schemas omit {required}"))
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
    #[serde(default, alias = "next_spec_index")]
    raw_next_spec_index: usize,
    #[serde(default)]
    snapshot_next_spec_index: usize,
}

/// Process one polling pass using the built-in executor and action observers.
pub fn process_tickets_once(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    cursor_path: &Path,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
) -> Result<ProcessSummary> {
    let journal_path = execution_journal_path(cursor_path);
    process_tickets_once_with_journal(
        transport,
        session,
        manifest,
        cursor_path,
        journal_path.as_path(),
        executor_config,
        now_unix_ms,
    )
}

/// Process one polling pass using an explicit durable execution journal path.
#[allow(clippy::too_many_arguments)]
pub fn process_tickets_once_with_journal(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    cursor_path: &Path,
    journal_path: &Path,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
) -> Result<ProcessSummary> {
    process_tickets_once_with_hooks(
        transport,
        session,
        manifest,
        cursor_path,
        journal_path,
        executor_config,
        now_unix_ms,
        |inner_transport, inner_session, spec, config| {
            executors::execute_action(inner_transport, inner_session, spec, config)
        },
        |inner_transport, inner_session, spec, config| {
            executors::reconcile_action(inner_transport, inner_session, spec, config)
        },
    )
}

/// Process one polling pass with a caller-supplied executor callback.
///
/// This compatibility hook deliberately returns an ambiguous recovery outcome;
/// tests that exercise version-2 crash recovery should use
/// [`process_tickets_once_with_hooks`] and provide an observer.
pub fn process_tickets_once_with_executor<F>(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    cursor_path: &Path,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
    executor: F,
) -> Result<ProcessSummary>
where
    F: FnMut(&mut dyn Transport, &Session, &HostTicketSpec, &ExecutorConfig) -> Result<String>,
{
    let journal_path = execution_journal_path(cursor_path);
    process_tickets_once_with_hooks(
        transport,
        session,
        manifest,
        cursor_path,
        journal_path.as_path(),
        executor_config,
        now_unix_ms,
        executor,
        |_transport, _session, _spec, _config| Ok(executors::ReconcileOutcome::Ambiguous),
    )
}

/// Process one pass with explicit provider execution and recovery hooks.
#[allow(clippy::too_many_arguments)]
pub fn process_tickets_once_with_hooks<F, R>(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    cursor_path: &Path,
    journal_path: &Path,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
    mut executor: F,
    mut reconciler: R,
) -> Result<ProcessSummary>
where
    F: FnMut(&mut dyn Transport, &Session, &HostTicketSpec, &ExecutorConfig) -> Result<String>,
    R: FnMut(
        &mut dyn Transport,
        &Session,
        &HostTicketSpec,
        &ExecutorConfig,
    ) -> Result<executors::ReconcileOutcome>,
{
    if !manifest.enabled {
        return Ok(ProcessSummary::default());
    }

    let spec_path = manifest.spec_path();
    let snapshot_path = manifest.spec_snapshot_path();
    let status_path = manifest.status_path();
    let deadletter_path = manifest.deadletter_path();
    let spec_lines = transport
        .read(session, spec_path.as_str())
        .with_context(|| format!("read {spec_path}"))?;
    let snapshot_lines = transport
        .read(session, snapshot_path.as_str())
        .with_context(|| format!("read {snapshot_path}"))?;
    let status_lines = transport
        .read(session, status_path.as_str())
        .with_context(|| format!("read {status_path}"))?;
    let deadletter_lines = transport
        .read(session, deadletter_path.as_str())
        .with_context(|| format!("read {deadletter_path}"))?;

    let raw_specs = claim::parse_spec_lines_from(
        &spec_lines,
        &manifest.accepted_request_schemas,
        manifest.max_line_bytes,
        claim::SpecSource::RawRequest,
    )?;
    let admitted_specs = claim::parse_spec_lines_from(
        &snapshot_lines,
        &manifest.accepted_request_schemas,
        manifest.max_line_bytes,
        claim::SpecSource::AdmittedSnapshot,
    )?;
    let mut results = claim::parse_result_lines_from(
        &status_lines,
        &manifest.accepted_result_schemas,
        manifest.max_line_bytes,
    )?;
    let mut deadletters = claim::parse_result_lines_from(
        &deadletter_lines,
        &manifest.accepted_result_schemas,
        manifest.max_line_bytes,
    )?;
    results.append(&mut deadletters);
    validate_manifest_results(manifest, &results)?;
    validate_snapshot_admission_order(&admitted_specs)?;
    let mut terminal = terminal_keys(&results);

    let mut cursor = load_cursor_state(cursor_path)?;
    cursor.raw_next_spec_index = cursor.raw_next_spec_index.min(raw_specs.len());
    cursor.snapshot_next_spec_index = cursor.snapshot_next_spec_index.min(admitted_specs.len());
    save_cursor_state(cursor_path, &cursor)?;
    let mut journal = wal::ExecutionJournal::load(journal_path)?;
    let mut summary = ProcessSummary::default();

    for spec in raw_specs.iter().skip(cursor.raw_next_spec_index) {
        if spec.schema == HOST_TICKET_V2_SCHEMA {
            // Caller bytes are intentionally never executable. Root must emit the
            // corresponding enriched entry on spec.snapshot before it is claimable.
            cursor.raw_next_spec_index = cursor.raw_next_spec_index.saturating_add(1);
            save_cursor_state(cursor_path, &cursor)?;
            continue;
        }
        process_v1_spec(
            transport,
            session,
            manifest,
            spec,
            executor_config,
            now_unix_ms,
            &mut terminal,
            &mut summary,
            &mut executor,
        )?;
        cursor.raw_next_spec_index = cursor.raw_next_spec_index.saturating_add(1);
        save_cursor_state(cursor_path, &cursor)?;
    }

    for spec in admitted_specs.iter().skip(cursor.snapshot_next_spec_index) {
        if spec.schema == HOST_TICKET_V1_SCHEMA {
            // Version 1 retains its raw compatibility path and cannot be
            // mistaken for root-admitted Worker receipt work.
            cursor.snapshot_next_spec_index = cursor.snapshot_next_spec_index.saturating_add(1);
            save_cursor_state(cursor_path, &cursor)?;
            continue;
        }
        process_v2_spec(
            transport,
            session,
            manifest,
            spec,
            executor_config,
            now_unix_ms,
            &mut terminal,
            &results,
            &mut summary,
            journal_path,
            &mut journal,
            &mut executor,
            &mut reconciler,
        )?;
        cursor.snapshot_next_spec_index = cursor.snapshot_next_spec_index.saturating_add(1);
        save_cursor_state(cursor_path, &cursor)?;
        let key = TicketKey::new(&spec.id, &spec.idempotency_key);
        if journal
            .get(&key)
            .is_some_and(|entry| entry.state == wal::ExecutionJournalState::ResultPublished)
        {
            journal.mark_terminal(&key)?;
            journal.save(journal_path)?;
        }
    }

    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
fn process_v1_spec<F>(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    spec: &HostTicketSpec,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
    terminal: &mut HashSet<TicketKey>,
    summary: &mut ProcessSummary,
    executor: &mut F,
) -> Result<()>
where
    F: FnMut(&mut dyn Transport, &Session, &HostTicketSpec, &ExecutorConfig) -> Result<String>,
{
    summary.seen = summary.seen.saturating_add(1);
    if let Some(target_hive) = spec.target_hive.as_deref() {
        if target_hive != manifest.federation.local_hive {
            summary.skipped_remote_target = summary.skipped_remote_target.saturating_add(1);
            return Ok(());
        }
    }
    let key = TicketKey::new(&spec.id, &spec.idempotency_key);
    if terminal.contains(&key) {
        summary.skipped_terminal = summary.skipped_terminal.saturating_add(1);
        return Ok(());
    }
    if is_expired(spec, now_unix_ms) {
        append_result(
            transport,
            session,
            manifest,
            spec,
            "expired",
            Some("ticket expired before execution"),
            manifest.deadletter_path().as_str(),
        )?;
        summary.expired = summary.expired.saturating_add(1);
        terminal.insert(key);
        return Ok(());
    }
    if !manifest.action_allowlist.contains(&spec.action) {
        append_result(
            transport,
            session,
            manifest,
            spec,
            "failed",
            Some("ticket action is not in generated allowlist"),
            manifest.deadletter_path().as_str(),
        )?;
        summary.failed = summary.failed.saturating_add(1);
        terminal.insert(key);
        return Ok(());
    }
    append_result(
        transport,
        session,
        manifest,
        spec,
        "claimed",
        Some("claimed by host-ticket-agent compatibility executor"),
        manifest.status_path().as_str(),
    )?;
    append_result(
        transport,
        session,
        manifest,
        spec,
        "running",
        Some("compatibility executor started"),
        manifest.status_path().as_str(),
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
                manifest.status_path().as_str(),
            )?;
            summary.succeeded = summary.succeeded.saturating_add(1);
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
                manifest.deadletter_path().as_str(),
            )?;
            summary.failed = summary.failed.saturating_add(1);
        }
    }
    terminal.insert(key);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_v2_spec<F, R>(
    transport: &mut dyn Transport,
    session: &Session,
    manifest: &HostTicketManifest,
    spec: &HostTicketSpec,
    executor_config: &ExecutorConfig,
    now_unix_ms: u64,
    terminal: &mut HashSet<TicketKey>,
    parsed_results: &[HostTicketResult],
    summary: &mut ProcessSummary,
    journal_path: &Path,
    journal: &mut wal::ExecutionJournal,
    executor: &mut F,
    reconciler: &mut R,
) -> Result<()>
where
    F: FnMut(&mut dyn Transport, &Session, &HostTicketSpec, &ExecutorConfig) -> Result<String>,
    R: FnMut(
        &mut dyn Transport,
        &Session,
        &HostTicketSpec,
        &ExecutorConfig,
    ) -> Result<executors::ReconcileOutcome>,
{
    summary.seen = summary.seen.saturating_add(1);
    if !manifest.receipt_action_allowlist.contains(&spec.action) {
        return Err(anyhow!(
            "root snapshot contains version-2 action {} outside generated receipt allowlist",
            spec.action
        ));
    }
    let key = TicketKey::new(&spec.id, &spec.idempotency_key);
    journal.prepare(spec)?;
    journal.save(journal_path)?;
    let mut state = journal
        .get(&key)
        .map(|entry| entry.state)
        .ok_or_else(|| anyhow!("prepared execution journal entry disappeared"))?;
    if terminal.contains(&key) {
        let visible_terminal = parsed_results
            .iter()
            .filter(|result| {
                result.id == spec.id
                    && result.idempotency_key == spec.idempotency_key
                    && matches!(result.state.as_str(), "succeeded" | "failed" | "expired")
            })
            .collect::<Vec<_>>();
        if visible_terminal.is_empty() {
            return Err(anyhow!(
                "terminal key exists without a parsed terminal version-2 result"
            ));
        }
        for result in &visible_terminal {
            validate_result_binding(spec, result)?;
        }
        if state >= wal::ExecutionJournalState::ProviderResultPersisted {
            let staged_line = journal
                .get(&key)
                .and_then(|entry| entry.result_line.as_deref())
                .ok_or_else(|| {
                    anyhow!("terminal VM result exists without staged journal result")
                })?;
            let exact_visible = visible_terminal.iter().any(|result| {
                serde_json::to_string(result).is_ok_and(|encoded| encoded == staged_line)
            });
            if !exact_visible {
                return Err(anyhow!(
                    "visible terminal VM result differs from the journal-staged result"
                ));
            }
        }
        if state == wal::ExecutionJournalState::ProviderResultPersisted {
            journal.mark_result_published(&key)?;
            journal.save(journal_path)?;
            state = wal::ExecutionJournalState::ResultPublished;
        }
        if matches!(
            state,
            wal::ExecutionJournalState::ResultPublished | wal::ExecutionJournalState::Terminal
        ) {
            summary.skipped_terminal = summary.skipped_terminal.saturating_add(1);
            return Ok(());
        }
        return Err(anyhow!(
            "terminal VM result conflicts with journal state {state:?}"
        ));
    }
    if state == wal::ExecutionJournalState::Terminal {
        let entry = journal
            .get(&key)
            .ok_or_else(|| anyhow!("terminal execution journal entry disappeared"))?;
        let path = entry
            .result_path
            .as_deref()
            .ok_or_else(|| anyhow!("terminal execution journal entry lacks result path"))?;
        let line = entry
            .result_line
            .as_deref()
            .ok_or_else(|| anyhow!("terminal execution journal entry lacks result line"))?;
        status::append_result_line(transport, session, path, line)?;
        summary.skipped_terminal = summary.skipped_terminal.saturating_add(1);
        terminal.insert(key);
        return Ok(());
    }
    if state == wal::ExecutionJournalState::ResultPublished {
        let entry = journal
            .get(&key)
            .ok_or_else(|| anyhow!("published execution journal entry disappeared"))?;
        let path = entry
            .result_path
            .as_deref()
            .ok_or_else(|| anyhow!("published execution journal entry lacks result path"))?;
        let line = entry
            .result_line
            .as_deref()
            .ok_or_else(|| anyhow!("published execution journal entry lacks result line"))?;
        status::append_result_line(transport, session, path, line)?;
        terminal.insert(key);
        return Ok(());
    }

    if state == wal::ExecutionJournalState::Prepared {
        validate_ready_worker_binding(transport, session, spec)?;
        append_result(
            transport,
            session,
            manifest,
            spec,
            "claimed",
            Some("root-admitted version-2 ticket claimed"),
            manifest.status_path().as_str(),
        )?;
        journal.mark_executing(&key)?;
        journal.save(journal_path)?;
        append_result(
            transport,
            session,
            manifest,
            spec,
            "running",
            Some("version-2 executor started"),
            manifest.status_path().as_str(),
        )?;
        let result = if is_expired(spec, now_unix_ms) {
            wal::JournalProviderResult {
                outcome: wal::JournalProviderOutcome::Stale,
                message: "ticket expired before provider execution".to_owned(),
                reconciled: false,
            }
        } else {
            match executor(transport, session, spec, executor_config) {
                Ok(message) => wal::JournalProviderResult {
                    outcome: wal::JournalProviderOutcome::Confirmed,
                    message: bounded_provider_message(message.as_str()),
                    reconciled: false,
                },
                Err(err) if executors::is_provider_pending(&err) => {
                    return Err(err).context(
                        "provider outcome remains pending; journal stays executing for observation",
                    );
                }
                Err(err) => wal::JournalProviderResult {
                    outcome: wal::JournalProviderOutcome::Rejected,
                    message: bounded_detail(err.as_ref(), 192),
                    reconciled: false,
                },
            }
        };
        journal.persist_provider_result(&key, result)?;
        journal.save(journal_path)?;
    } else if state == wal::ExecutionJournalState::Executing {
        let observed = reconciler(transport, session, spec, executor_config)?;
        let result = match observed {
            executors::ReconcileOutcome::Committed(message) => wal::JournalProviderResult {
                outcome: wal::JournalProviderOutcome::Confirmed,
                message: bounded_provider_message(message.as_str()),
                reconciled: true,
            },
            executors::ReconcileOutcome::Rejected(message) => wal::JournalProviderResult {
                outcome: wal::JournalProviderOutcome::Rejected,
                message: bounded_provider_message(message.as_str()),
                reconciled: true,
            },
            executors::ReconcileOutcome::Ambiguous if is_expired(spec, now_unix_ms) => {
                wal::JournalProviderResult {
                    outcome: wal::JournalProviderOutcome::Stale,
                    message:
                        "ticket expired while provider outcome remained ambiguous; execution was not repeated"
                            .to_owned(),
                    reconciled: true,
                }
            }
            executors::ReconcileOutcome::Ambiguous => {
                return Err(anyhow!(
                    "provider outcome remains ambiguous; execution was not repeated"
                ));
            }
        };
        journal.persist_provider_result(&key, result)?;
        journal.save(journal_path)?;
    }

    let entry = journal
        .get(&key)
        .cloned()
        .ok_or_else(|| anyhow!("execution journal entry disappeared"))?;
    if entry.state == wal::ExecutionJournalState::ProviderResultPersisted {
        let provider_result = entry
            .provider_result
            .as_ref()
            .ok_or_else(|| anyhow!("provider-result-persisted entry lacks result"))?;
        let (state, path) = match provider_result.outcome {
            wal::JournalProviderOutcome::Confirmed => ("succeeded", manifest.status_path()),
            wal::JournalProviderOutcome::Rejected => ("failed", manifest.deadletter_path()),
            wal::JournalProviderOutcome::Stale => ("expired", manifest.deadletter_path()),
        };
        let line = status::build_result_line(
            spec,
            manifest.result_schema_for(spec.schema.as_str())?,
            state,
            Some(provider_result.message.as_str()),
            effective_result_line_limit(spec.schema.as_str(), manifest.max_line_bytes),
        )?;
        journal.stage_result(&key, path.as_str(), line.as_str())?;
        journal.save(journal_path)?;

        let already_visible = terminal.contains(&key);
        if !already_visible {
            status::append_result_line(transport, session, path.as_str(), line.as_str())?;
        }
        journal.mark_result_published(&key)?;
        journal.save(journal_path)?;
        match provider_result.outcome {
            wal::JournalProviderOutcome::Confirmed => {
                summary.succeeded = summary.succeeded.saturating_add(1);
            }
            wal::JournalProviderOutcome::Rejected => {
                summary.failed = summary.failed.saturating_add(1);
            }
            wal::JournalProviderOutcome::Stale => {
                summary.expired = summary.expired.saturating_add(1);
            }
        }
        terminal.insert(key);
    }
    Ok(())
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
    let line_limit = effective_result_line_limit(spec.schema.as_str(), manifest.max_line_bytes);
    let line = status::build_result_line(
        spec,
        manifest.result_schema_for(spec.schema.as_str())?,
        state,
        message,
        line_limit,
    )?;
    status::append_result_line(transport, session, path, line.as_str())
}

fn effective_result_line_limit(request_schema: &str, max_line_bytes: u32) -> u32 {
    if request_schema == HOST_TICKET_V2_SCHEMA {
        max_line_bytes.min(MAX_ECHO_LEN as u32)
    } else {
        max_line_bytes.min(HOST_TICKET_V1_ECHO_COMPAT_MAX_BYTES)
    }
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
    wal::save_cursor_durable(path, &payload)
}

fn execution_journal_path(cursor_path: &Path) -> std::path::PathBuf {
    cursor_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("execution-journal.json")
}

fn is_expired(spec: &HostTicketSpec, now_unix_ms: u64) -> bool {
    spec.expires_unix_ms
        .is_some_and(|expires_unix_ms| now_unix_ms >= expires_unix_ms)
}

fn validate_manifest_results(
    manifest: &HostTicketManifest,
    results: &[HostTicketResult],
) -> Result<()> {
    for result in results {
        if !manifest.action_allowlist.contains(&result.action) {
            return Err(anyhow!(
                "ticket result action {} is outside generated allowlist",
                result.action
            ));
        }
        if !manifest.lifecycle.contains(&result.state) {
            return Err(anyhow!(
                "ticket result state {} is outside generated lifecycle",
                result.state
            ));
        }
        if result.schema == HOST_TICKET_RESULT_V2_SCHEMA
            && !manifest.receipt_action_allowlist.contains(&result.action)
        {
            return Err(anyhow!(
                "version-2 result action {} is outside generated receipt allowlist",
                result.action
            ));
        }
    }
    Ok(())
}

fn validate_result_binding(spec: &HostTicketSpec, result: &HostTicketResult) -> Result<()> {
    if result.schema != HOST_TICKET_RESULT_V2_SCHEMA
        || result.action != spec.action
        || result.receipt_mode != spec.receipt_mode
        || result.operation_id != spec.operation_id
        || result.subject_ref != spec.subject_ref
        || result.receipt_worker_role != spec.receipt_worker_role
        || result.receipt_worker_id != spec.receipt_worker_id
        || result.receipt_supervisor_generation != spec.receipt_supervisor_generation
        || result.receipt_cap_generation != spec.receipt_cap_generation
        || result.resolved_worker_slot != spec.resolved_worker_slot
        || result.resolved_lease_epoch != spec.resolved_lease_epoch
        || result.admission_sequence != spec.admission_sequence
    {
        return Err(anyhow!(
            "terminal version-2 result does not echo the root-admitted Worker binding"
        ));
    }
    Ok(())
}

fn validate_snapshot_admission_order(specs: &[HostTicketSpec]) -> Result<()> {
    let mut last = 0u64;
    for spec in specs {
        if spec.schema != HOST_TICKET_V2_SCHEMA {
            continue;
        }
        let sequence = spec
            .admission_sequence
            .ok_or_else(|| anyhow!("version-2 snapshot lacks admission_sequence"))?;
        if sequence <= last {
            return Err(anyhow!(
                "version-2 snapshot admission_sequence must be strictly increasing"
            ));
        }
        last = sequence;
    }
    Ok(())
}

fn validate_ready_worker_binding(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<()> {
    let worker_id = spec
        .receipt_worker_id
        .as_deref()
        .ok_or_else(|| anyhow!("host-ticket/v2 lacks receipt_worker_id"))?;
    let expected_role = spec
        .receipt_worker_role
        .as_deref()
        .ok_or_else(|| anyhow!("host-ticket/v2 lacks receipt_worker_role"))?;
    let expected_slot = spec
        .resolved_worker_slot
        .ok_or_else(|| anyhow!("host-ticket/v2 lacks resolved_worker_slot"))?;
    let expected_lease_epoch = spec
        .resolved_lease_epoch
        .ok_or_else(|| anyhow!("host-ticket/v2 lacks resolved_lease_epoch"))?;
    let expected_supervisor_generation = spec
        .receipt_supervisor_generation
        .ok_or_else(|| anyhow!("host-ticket/v2 lacks receipt_supervisor_generation"))?;
    let expected_cap_generation = spec
        .receipt_cap_generation
        .ok_or_else(|| anyhow!("host-ticket/v2 lacks receipt_cap_generation"))?;

    let shards = transport
        .list(session, "/shard")
        .context("list canonical Worker shard root")?;
    for shard in shards.into_iter().take(64) {
        if normalise_token("shard label", shard.as_str()).is_err() {
            continue;
        }
        let worker_root = format!("/shard/{shard}/worker");
        let workers = transport.list(session, worker_root.as_str())?;
        if !workers.iter().any(|candidate| candidate == worker_id) {
            continue;
        }
        let telemetry = format!("{worker_root}/{worker_id}/telemetry");
        let lines = transport.tail(session, telemetry.as_str(), None)?;
        for line in lines.iter().rev().take(64) {
            let Ok(snapshot) = serde_json::from_str::<WorkerRuntimeStateLine>(line.trim()) else {
                continue;
            };
            if snapshot.schema != "worker-runtime-state/v1" || snapshot.worker_id != worker_id {
                continue;
            }
            if snapshot.state != "ready"
                || snapshot.role != expected_role
                || snapshot.slot != expected_slot
                || snapshot.lease_epoch != expected_lease_epoch
                || snapshot.supervisor_generation != expected_supervisor_generation
                || snapshot.cap_generation != expected_cap_generation
                || snapshot.ready_sequence == 0
            {
                return Err(anyhow!(
                    "receipt Worker {worker_id} is not READY at the exact root-pinned identity"
                ));
            }
            return Ok(());
        }
        return Err(anyhow!(
            "receipt Worker {worker_id} has no authoritative runtime-state projection"
        ));
    }
    Err(anyhow!(
        "receipt Worker {worker_id} is absent from canonical /shard projections"
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerRuntimeStateLine {
    schema: String,
    worker_id: String,
    role: String,
    state: String,
    slot: u16,
    lease_epoch: u64,
    supervisor_generation: u64,
    cap_generation: u64,
    ready_sequence: u64,
    #[serde(rename = "control_sequence")]
    _control_sequence: u64,
    #[serde(rename = "receipt_sequence")]
    _receipt_sequence: u64,
    #[serde(rename = "completion_sequence")]
    _completion_sequence: u64,
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
    text::bounded_single_line(text.as_str(), max_chars)
}

fn bounded_provider_message(message: &str) -> String {
    let bounded = text::bounded_single_line(message, 192);
    if bounded.is_empty() {
        "provider completed without detail".to_owned()
    } else {
        bounded
    }
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
    #[serde(default = "default_accepted_request_schemas")]
    accepted_request_schemas: Vec<String>,
    #[serde(default = "default_accepted_result_schemas")]
    accepted_result_schemas: Vec<String>,
    #[serde(default = "default_max_line_bytes")]
    max_line_bytes: u32,
    #[serde(default)]
    action_allowlist: Vec<String>,
    #[serde(default)]
    receipt_action_allowlist: Vec<String>,
    #[serde(default)]
    lifecycle: Vec<String>,
}

impl Default for ResolvedHostTickets {
    fn default() -> Self {
        Self {
            enable: false,
            request_schema: default_request_schema(),
            result_schema: default_result_schema(),
            accepted_request_schemas: default_accepted_request_schemas(),
            accepted_result_schemas: default_accepted_result_schemas(),
            max_line_bytes: default_max_line_bytes(),
            action_allowlist: Vec::new(),
            receipt_action_allowlist: Vec::new(),
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

fn default_accepted_request_schemas() -> Vec<String> {
    vec![
        HOST_TICKET_V1_SCHEMA.to_owned(),
        HOST_TICKET_V2_SCHEMA.to_owned(),
    ]
}

fn default_accepted_result_schemas() -> Vec<String> {
    vec![
        HOST_TICKET_RESULT_V1_SCHEMA.to_owned(),
        HOST_TICKET_RESULT_V2_SCHEMA.to_owned(),
    ]
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
                "accepted_request_schemas": ["host-ticket/v1", "host-ticket/v2"],
                "accepted_result_schemas": ["host-ticket-result/v1", "host-ticket-result/v2"],
                "max_line_bytes": 2048,
                "action_allowlist": ["systemd.restart","gpu.lease.grant","gpu.lease.renew","gpu.lease.release","peft.export","peft.import","peft.activate","peft.rollback"],
                "receipt_action_allowlist": ["gpu.lease.grant","gpu.lease.renew","gpu.lease.release","peft.export","peft.import","peft.activate","peft.rollback"],
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
            ..HostTicketManifest::default()
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
            fail_after_terminal_write_once: false,
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig {
            mount: "/host".to_owned(),
            registry_root: PathBuf::from("out/model_registry"),
            ..ExecutorConfig::default()
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
            ..HostTicketManifest::default()
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
            fail_after_terminal_write_once: false,
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig {
            mount: "/host".to_owned(),
            registry_root: PathBuf::from("out/model_registry"),
            ..ExecutorConfig::default()
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
            ..HostTicketManifest::default()
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
            fail_after_terminal_write_once: false,
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig {
            mount: "/host".to_owned(),
            registry_root: PathBuf::from("out/model_registry"),
            ..ExecutorConfig::default()
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

    fn v2_manifest() -> HostTicketManifest {
        HostTicketManifest {
            enabled: true,
            action_allowlist: vec!["gpu.lease.grant".to_owned()],
            receipt_action_allowlist: vec!["gpu.lease.grant".to_owned()],
            ..HostTicketManifest::default()
        }
    }

    fn v2_specs() -> (HostTicketSpec, HostTicketSpec) {
        let raw = HostTicketSpec {
            schema: HOST_TICKET_V2_SCHEMA.to_owned(),
            id: "ticket-v2".to_owned(),
            idempotency_key: "idem-v2".to_owned(),
            action: "gpu.lease.grant".to_owned(),
            args: serde_json::json!({"ttl_s": 30}),
            receipt_mode: Some(ReceiptMode::Worker),
            operation_id: Some("lease-1".to_owned()),
            subject_ref: Some("GPU-0".to_owned()),
            receipt_worker_role: Some("worker-gpu".to_owned()),
            receipt_worker_id: Some("gpu-worker-1".to_owned()),
            receipt_supervisor_generation: Some(2),
            receipt_cap_generation: Some(3),
            ..HostTicketSpec::default()
        };
        let mut admitted = raw.clone();
        admitted.resolved_worker_slot = Some(0);
        admitted.resolved_lease_epoch = Some(4);
        admitted.admission_sequence = Some(5);
        (raw, admitted)
    }

    fn v2_files(manifest: &HostTicketManifest) -> BTreeMap<String, Vec<String>> {
        let (raw, admitted) = v2_specs();
        let mut files = BTreeMap::new();
        files.insert(
            manifest.spec_path(),
            vec![serde_json::to_string(&raw).expect("raw v2")],
        );
        files.insert(
            manifest.spec_snapshot_path(),
            vec![serde_json::to_string(&admitted).expect("admitted v2")],
        );
        files.insert(manifest.status_path(), Vec::new());
        files.insert(manifest.deadletter_path(), Vec::new());
        files.insert("/shard".to_owned(), vec!["s0".to_owned()]);
        files.insert(
            "/shard/s0/worker".to_owned(),
            vec!["gpu-worker-1".to_owned()],
        );
        files.insert(
            "/shard/s0/worker/gpu-worker-1/telemetry".to_owned(),
            vec!["{\"schema\":\"worker-runtime-state/v1\",\"worker_id\":\"gpu-worker-1\",\"role\":\"worker-gpu\",\"state\":\"ready\",\"slot\":0,\"lease_epoch\":4,\"supervisor_generation\":2,\"cap_generation\":3,\"ready_sequence\":1,\"control_sequence\":0,\"receipt_sequence\":0,\"completion_sequence\":0}".to_owned()],
        );
        files
    }

    #[test]
    fn v2_executes_only_root_snapshot_and_emits_canonical_digest() {
        let temp = tempfile::TempDir::new().expect("temp");
        let cursor = temp.path().join("cursor.json");
        let journal = temp.path().join("journal.json");
        let manifest = v2_manifest();
        let mut transport = FakeTransport {
            files: v2_files(&manifest),
            max_write_line_len: None,
            fail_after_terminal_write_once: false,
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig::default();
        let mut executions = 0usize;
        let summary = process_tickets_once_with_hooks(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &journal,
            &config,
            unix_time_ms_now(),
            |_transport, _session, spec, _config| {
                executions = executions.saturating_add(1);
                assert_eq!(spec.admission_sequence, Some(5));
                Ok("🔥 provider committed".to_owned())
            },
            |_transport, _session, _spec, _config| {
                unreachable!("fresh execution must not reconcile")
            },
        )
        .expect("process v2");
        assert_eq!(summary.succeeded, 1);
        assert_eq!(executions, 1, "raw v2 must never execute separately");

        let results = claim::parse_result_lines_from(
            transport
                .files
                .get(&manifest.status_path())
                .expect("status"),
            &manifest.accepted_result_schemas,
            manifest.max_line_bytes,
        )
        .expect("parse status");
        let terminal = results
            .iter()
            .find(|result| result.state == "succeeded")
            .expect("terminal result");
        status::validate_result_digest(terminal).expect("digest");
        assert_eq!(terminal.resolved_worker_slot, Some(0));
        assert_eq!(terminal.resolved_lease_epoch, Some(4));
        assert_eq!(terminal.admission_sequence, Some(5));
    }

    #[test]
    fn v2_refuses_not_ready_or_unpinned_worker_before_execution() {
        let temp = tempfile::TempDir::new().expect("temp");
        let manifest = v2_manifest();
        let mut files = v2_files(&manifest);
        let telemetry = files
            .get_mut("/shard/s0/worker/gpu-worker-1/telemetry")
            .expect("telemetry");
        telemetry[0] = telemetry[0].replace("\"state\":\"ready\"", "\"state\":\"starting\"");
        let mut transport = FakeTransport {
            files,
            max_write_line_len: None,
            fail_after_terminal_write_once: false,
        };
        let err = process_tickets_once_with_hooks(
            &mut transport,
            &Session::new(1.into(), Role::Queen),
            &manifest,
            &temp.path().join("cursor.json"),
            &temp.path().join("journal.json"),
            &ExecutorConfig::default(),
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                unreachable!("not-ready Worker must prevent execution")
            },
            |_transport, _session, _spec, _config| {
                unreachable!("fresh not-ready request must not reconcile")
            },
        )
        .expect_err("not-ready binding must fail");
        assert!(format!("{err:#}").contains("not READY"));
    }

    #[test]
    fn v2_partial_vm_publish_recovers_without_provider_reexecution() {
        let temp = tempfile::TempDir::new().expect("temp");
        let cursor = temp.path().join("cursor.json");
        let journal = temp.path().join("journal.json");
        let manifest = v2_manifest();
        let mut transport = FakeTransport {
            files: v2_files(&manifest),
            max_write_line_len: None,
            fail_after_terminal_write_once: true,
        };
        let session = Session::new(1.into(), Role::Queen);
        let config = ExecutorConfig::default();
        let mut executions = 0usize;
        let first = process_tickets_once_with_hooks(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &journal,
            &config,
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                executions = executions.saturating_add(1);
                Ok("provider committed".to_owned())
            },
            |_transport, _session, _spec, _config| Ok(executors::ReconcileOutcome::Ambiguous),
        )
        .expect_err("injected post-commit write failure");
        assert!(first.to_string().contains("injected failure"));
        assert_eq!(executions, 1);

        let second = process_tickets_once_with_hooks(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &journal,
            &config,
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| unreachable!("provider must not execute twice"),
            |_transport, _session, _spec, _config| {
                unreachable!("committed result is already visible")
            },
        )
        .expect("recover partial publish");
        assert_eq!(second.skipped_terminal, 1);
        let status = transport
            .files
            .get(&manifest.status_path())
            .expect("status");
        assert_eq!(
            status
                .iter()
                .filter(|line| line.contains("\"state\":\"succeeded\""))
                .count(),
            1
        );
        let state = wal::ExecutionJournal::load(&journal).expect("journal");
        let (_, admitted) = v2_specs();
        let key = TicketKey::new(&admitted.id, &admitted.idempotency_key);
        assert_eq!(
            state.get(&key).map(|entry| entry.state),
            Some(wal::ExecutionJournalState::Terminal)
        );
    }

    #[test]
    fn v2_executing_recovery_observes_and_never_replays_provider() {
        let temp = tempfile::TempDir::new().expect("temp");
        let cursor = temp.path().join("cursor.json");
        let journal_path = temp.path().join("journal.json");
        let manifest = v2_manifest();
        let mut transport = FakeTransport {
            files: v2_files(&manifest),
            max_write_line_len: None,
            fail_after_terminal_write_once: false,
        };
        let (_raw, admitted) = v2_specs();
        let key = TicketKey::new(&admitted.id, &admitted.idempotency_key);
        let mut journal = wal::ExecutionJournal::default();
        journal.prepare(&admitted).expect("prepare");
        journal.mark_executing(&key).expect("executing");
        journal.save(&journal_path).expect("save executing");

        let session = Session::new(1.into(), Role::Queen);
        let summary = process_tickets_once_with_hooks(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &journal_path,
            &ExecutorConfig::default(),
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                unreachable!("executing recovery must not replay provider")
            },
            |_transport, _session, _spec, _config| {
                Ok(executors::ReconcileOutcome::Committed(
                    "exact operation observed committed".to_owned(),
                ))
            },
        )
        .expect("reconcile");
        assert_eq!(summary.succeeded, 1);
        let loaded = wal::ExecutionJournal::load(&journal_path).expect("journal");
        assert_eq!(
            loaded.get(&key).map(|entry| entry.state),
            Some(wal::ExecutionJournalState::Terminal)
        );
        assert!(loaded
            .get(&key)
            .and_then(|entry| entry.provider_result.as_ref())
            .is_some_and(|result| result.reconciled));
    }

    #[test]
    fn v2_pending_provider_stays_executing_until_observed() {
        let temp = tempfile::TempDir::new().expect("temp");
        let cursor = temp.path().join("cursor.json");
        let journal_path = temp.path().join("journal.json");
        let manifest = v2_manifest();
        let mut transport = FakeTransport {
            files: v2_files(&manifest),
            max_write_line_len: None,
            fail_after_terminal_write_once: false,
        };
        let session = Session::new(1.into(), Role::Queen);
        let first = process_tickets_once_with_hooks(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &journal_path,
            &ExecutorConfig::default(),
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                Err(executors::provider_pending("provider admitted operation"))
            },
            |_transport, _session, _spec, _config| {
                unreachable!("fresh dispatch must not reconcile")
            },
        )
        .expect_err("pending outcome keeps pass open");
        assert!(first.to_string().contains("journal stays executing"));
        let (_, admitted) = v2_specs();
        let key = TicketKey::new(&admitted.id, &admitted.idempotency_key);
        assert_eq!(
            wal::ExecutionJournal::load(&journal_path)
                .expect("journal")
                .get(&key)
                .map(|entry| entry.state),
            Some(wal::ExecutionJournalState::Executing)
        );

        let second = process_tickets_once_with_hooks(
            &mut transport,
            &session,
            &manifest,
            &cursor,
            &journal_path,
            &ExecutorConfig::default(),
            unix_time_ms_now(),
            |_transport, _session, _spec, _config| {
                unreachable!("pending provider must not be replayed")
            },
            |_transport, _session, _spec, _config| {
                Ok(executors::ReconcileOutcome::Committed(
                    "provider operation observed".to_owned(),
                ))
            },
        )
        .expect("observed provider");
        assert_eq!(second.succeeded, 1);
    }

    #[derive(Debug)]
    struct FakeTransport {
        files: BTreeMap<String, Vec<String>>,
        max_write_line_len: Option<usize>,
        fail_after_terminal_write_once: bool,
    }

    impl Transport for FakeTransport {
        fn attach(&mut self, _role: Role, _ticket: Option<&str>) -> Result<Session> {
            Ok(Session::new(1.into(), Role::Queen))
        }

        fn ping(&mut self, _session: &Session) -> Result<String> {
            Ok("pong".to_owned())
        }

        fn tail(
            &mut self,
            _session: &Session,
            path: &str,
            _lines: Option<u16>,
        ) -> Result<Vec<String>> {
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
            let fail_after_commit = self.fail_after_terminal_write_once
                && (text.contains("\"state\":\"succeeded\"")
                    || text.contains("\"state\":\"failed\"")
                    || text.contains("\"state\":\"expired\""));
            let entry = self.files.entry(path.to_owned()).or_default();
            for line in text.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Some(limit) = self.max_write_line_len {
                        if trimmed.len() > limit {
                            return Err(anyhow!(
                                "payload length {} exceeds max_echo_len {limit}: {trimmed}",
                                trimmed.len()
                            ));
                        }
                    }
                    entry.push(trimmed.to_owned());
                }
            }
            if fail_after_commit {
                self.fail_after_terminal_write_once = false;
                return Err(anyhow!("injected failure after terminal write commit"));
            }
            Ok(())
        }
    }
}
