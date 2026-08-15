// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Host-only REST gateway projecting Cohesix console/file semantics.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-only REST gateway projecting Cohesix console/file semantics.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use cohesix_net_constants::{
    COHESIX_TCP_CONSOLE_PORT, HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS,
    HIVE_GATEWAY_DEFAULT_BROKER_RESPONSE_TIMEOUT_MS,
};
use cohesix_ticket::Role;
use cohesix_worker_evidence::{
    parse_evidence, parse_target_session, ArtifactState, ExecutionProof, KernelObjectInventory,
    LifecycleState, ReceiptState, Sha256Hex, TargetClass, TargetSession, ValidatedEvidence,
    Verdict, WorkerRole,
};
use cohsh::policy::PolicyOverrides;
use cohsh::{
    CohshPolicy, PoolKind, Session, SessionPool, TransportFactory, CLIENT_LOG_PATH,
    CLIENT_POLICY_CTL_PATH, CLIENT_QUEEN_CTL_PATH, CLIENT_QUEEN_EXPORT_CTL_PATH,
    CLIENT_QUEEN_LEASE_CTL_PATH, CLIENT_QUEEN_LIFECYCLE_CTL_PATH, CLIENT_QUEEN_SCHEDULE_CTL_PATH,
    CONTROL_EXPORT_CTL_MAX_BYTES, CONTROL_EXPORT_ENABLED, CONTROL_LEASE_ACTIVE_MAX_ENTRIES,
    CONTROL_LEASE_CTL_MAX_BYTES, CONTROL_LEASE_ENABLED, CONTROL_LEASE_PREEMPTIONS_MAX_ENTRIES,
    CONTROL_SCHEDULE_CTL_MAX_BYTES, CONTROL_SCHEDULE_ENABLED, CONTROL_SCHEDULE_QUEUE_MAX_ENTRIES,
    POLICY_CTL_MAX_BYTES, POLICY_ENABLED, POLICY_QUEUE_MAX_BYTES, POLICY_QUEUE_MAX_ENTRIES,
    PROC_LEASE_ACTIVE_BYTES, PROC_LEASE_ACTIVE_ENABLED, PROC_LEASE_PREEMPTIONS_BYTES,
    PROC_LEASE_PREEMPTIONS_ENABLED, PROC_LEASE_SUMMARY_BYTES, PROC_LEASE_SUMMARY_ENABLED,
    PROC_SCHEDULE_QUEUE_BYTES, PROC_SCHEDULE_QUEUE_ENABLED, PROC_SCHEDULE_SUMMARY_BYTES,
    PROC_SCHEDULE_SUMMARY_ENABLED, SECURE9P_MSIZE, SECURE9P_WALK_DEPTH,
};
use cohsh::{NineDoorTransport, PooledTcpTransport, TcpTransport};
use cohsh_core::{
    parse_ack, parse_role, AckStatus, RoleParseMode, MAX_ECHO_LEN, MAX_ID_LEN, MAX_JSON_LEN,
    MAX_LINE_LEN, MAX_PATH_LEN, MAX_TAIL_LINES, MAX_TICKET_LEN,
};
use nine_door::{NineDoor, ShardLayout};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

type SharedPool = SessionPool;

const PROC_SCHEDULE_SUMMARY_PATH: &str = "/proc/schedule/summary";
const PROC_SCHEDULE_QUEUE_PATH: &str = "/proc/schedule/queue";
const PROC_LEASE_SUMMARY_PATH: &str = "/proc/lease/summary";
const PROC_LEASE_ACTIVE_PATH: &str = "/proc/lease/active";
const PROC_LEASE_PREEMPTIONS_PATH: &str = "/proc/lease/preemptions";
const REQUEST_AUTH_HEADER: &str = "x-cohesix-auth";
const AUTHORIZATION_BEARER_PREFIX: &str = "bearer ";
const INSECURE_PLACEHOLDER_TOKEN: &str = concat!("change", "me");
const DEFAULT_PROC_CACHE_TTL_MS: u64 = 2_000;
const DEFAULT_PROC_CACHE_MAX_ENTRIES: usize = 64;
const BROKER_QUEUE_WAIT_LIMIT_MS: u64 = HIVE_GATEWAY_BROKER_QUEUE_WAIT_LIMIT_MS;
const DEFAULT_BROKER_CONTROL_RESPONSE_TIMEOUT_MS: u64 =
    HIVE_GATEWAY_DEFAULT_BROKER_RESPONSE_TIMEOUT_MS;
const DEFAULT_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS: u64 =
    HIVE_GATEWAY_DEFAULT_BROKER_RESPONSE_TIMEOUT_MS;
const BROKER_ENQUEUE_RETRY_SLEEP_MS: u64 = 5;
const BROKER_CONTROL_QUEUE_CAPACITY: usize = 256;
const BROKER_TELEMETRY_QUEUE_CAPACITY: usize = 1024;
const BROKER_CONTROL_BURST: usize = 6;
const TELEMETRY_WRITE_BATCH_MAX: usize = 4;
const BROKER_IDLE_WAIT_MS: u64 = 20;
const DEFAULT_CONTROL_WRITE_RETRY_WINDOW_MS: u64 = 1_200;
const MAX_WORKER_ACCEPTANCE_EVIDENCE_BYTES: u64 = 256 * 1024;
const MAX_CONTROL_WRITE_RETRY_WINDOW_MS: u64 = 60_000;
const CONTROL_WRITE_RETRY_SLEEP_MS: u64 = 15;
const CONTROL_WRITE_RETRY_MAX_SLEEP_MS: u64 = 120;
const CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS: u64 = 250;
const CACHE_INVALIDATE_CONTROL_NAMESPACES: &[&str] =
    &["/proc", "/queen", "/shard", "/worker", "/gpu"];
const CACHE_INVALIDATE_HOST_NAMESPACES: &[&str] = &["/host"];
const CACHE_INVALIDATE_GPU_NAMESPACES: &[&str] = &["/gpu"];
const CACHE_INVALIDATE_SCHEDULE_NAMESPACES: &[&str] = &["/proc/schedule"];
const CACHE_INVALIDATE_LEASE_NAMESPACES: &[&str] = &["/proc/lease"];
const CACHE_INVALIDATE_LEASE_GRANT_PATHS: &[&str] =
    &[PROC_LEASE_SUMMARY_PATH, PROC_LEASE_ACTIVE_PATH];
const CACHE_INVALIDATE_LEASE_RENEW_PATHS: &[&str] = &[PROC_LEASE_ACTIVE_PATH];
const CACHE_INVALIDATE_LEASE_PREEMPT_PATHS: &[&str] = &[
    PROC_LEASE_SUMMARY_PATH,
    PROC_LEASE_ACTIVE_PATH,
    PROC_LEASE_PREEMPTIONS_PATH,
];
const CACHE_INVALIDATE_LEASE_QUOTA_PATHS: &[&str] = &[PROC_LEASE_SUMMARY_PATH];
const CACHE_INVALIDATE_TELEMETRY_NAMESPACES: &[&str] = &["/queen/telemetry"];
const CACHE_INVALIDATE_POLICY_NAMESPACES: &[&str] = &["/proc/pressure/policy"];

const OPENAPI_YAML: &str = include_str!("../../../resources/openapi/hive-gateway.yaml");

const SWAGGER_UI_HTML: &str = r#"<!doctype html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\" />
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
  <title>Hive Gateway API</title>
  <link rel=\"stylesheet\" href=\"https://unpkg.com/swagger-ui-dist@5/swagger-ui.css\" />
  <style>
    body { margin: 0; background: #0c121c; }
    #swagger-ui { min-height: 100vh; }
  </style>
</head>
<body>
  <div id=\"swagger-ui\"></div>
  <script src=\"https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js\"></script>
  <script>
    window.onload = () => {
      window.ui = SwaggerUIBundle({
        url: '/v1/openapi.yaml',
        dom_id: '#swagger-ui',
        presets: [SwaggerUIBundle.presets.apis],
        layout: 'BaseLayout'
      });
    };
  </script>
</body>
</html>"#;

#[derive(Debug, Parser)]
#[command(author = "Lukas Bower", version, about = "Cohesix host REST gateway")]
struct Cli {
    /// Bind address for the REST gateway.
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: String,
    /// TCP console host.
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,
    /// TCP console port.
    #[arg(long, default_value_t = COHESIX_TCP_CONSOLE_PORT)]
    tcp_port: u16,
    /// TCP console auth token (or set COH_AUTH_TOKEN / COHSH_AUTH_TOKEN).
    #[arg(long)]
    auth_token: Option<String>,
    /// Per-request REST auth token for mutating paths (`Authorization: Bearer` or `x-cohesix-auth`).
    #[arg(long)]
    request_auth_token: Option<String>,
    /// Allow non-loopback bind addresses (risk: exposes write-capable gateway over network).
    #[arg(long, default_value_t = false)]
    allow_non_loopback_bind: bool,
    /// Role to attach with (queen by default).
    #[arg(long, default_value = "queen")]
    role: String,
    /// Optional capability ticket payload.
    #[arg(long)]
    ticket: Option<String>,
    /// Override pooled control session capacity for this gateway process.
    #[arg(long)]
    pool_control_sessions: Option<u16>,
    /// Override pooled telemetry session capacity for this gateway process.
    #[arg(long)]
    pool_telemetry_sessions: Option<u16>,
    /// Max milliseconds to wait for a control broker response after enqueue.
    #[arg(
        long = "broker-control-response-timeout-ms",
        alias = "broker-control-timeout-ms"
    )]
    broker_control_response_timeout_ms: Option<u64>,
    /// Max milliseconds to wait for a telemetry broker response after enqueue.
    #[arg(
        long = "broker-telemetry-response-timeout-ms",
        alias = "broker-telemetry-timeout-ms"
    )]
    broker_telemetry_response_timeout_ms: Option<u64>,
    /// Max milliseconds to retry retryable control writes before surfacing bounded backpressure.
    #[arg(long)]
    control_write_retry_window_ms: Option<u64>,
    /// Use the in-process mock NineDoor backend.
    #[arg(long, default_value_t = false)]
    mock: bool,
    /// Import one bounded Worker acceptance record for read-only status projection.
    #[arg(long)]
    worker_acceptance_evidence: Option<PathBuf>,
    /// Explicit operator or release root containing the Worker acceptance record.
    #[arg(long)]
    worker_acceptance_root: Option<PathBuf>,
    /// Exact target-session identity for the console target currently behind this gateway.
    #[arg(long)]
    target_session: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<GatewayInner>,
}

struct GatewayInner {
    pool: SharedPool,
    broker_client: GatewayBrokerClient,
    role: Role,
    ticket: Option<String>,
    request_auth_token: String,
    status: Mutex<GatewayStatus>,
    shutdown: Arc<AtomicBool>,
    broker_timeouts: BrokerTimeouts,
    control_write_retry_window_ms: u64,
    bounds: BoundsResponse,
    backend_class: BackendClass,
    worker_acceptance: WorkerAcceptanceImport,
    policy: CohshPolicy,
    broker: Arc<BrokerMetrics>,
    proc_cache: Mutex<ProcReadCache>,
    control_write_backpressure: Mutex<ControlWriteBackpressure>,
}

#[derive(Debug, Clone, Serialize)]
struct BoundsResponse {
    manifest_sha256: &'static str,
    secure9p: Secure9pBounds,
    console: ConsoleBounds,
    paths: PathBounds,
    control_plane: ControlPlaneBounds,
    policy: PolicyBounds,
    observability: ObservabilityBounds,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_runtime: Option<WorkerRuntimeBounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum WorkerDeclaration {
    Executable,
    ModelOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkerRoleBounds {
    role: String,
    declaration: WorkerDeclaration,
    executable_slots: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WorkerRuntimeBounds {
    roles: Vec<WorkerRoleBounds>,
    task_abi_schema: String,
    task_abi_version: u16,
    worker_observation_schema: String,
    worker_integration_evidence_schema: String,
    maximum_live_tasks: u16,
    canonical_telemetry_template: String,
    shard_bits: u8,
    legacy_worker_alias: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
enum BackendClass {
    HostModel,
    ConsoleProjection,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct WorkerAcceptanceRoleSummary {
    role: &'static str,
    lifecycle: &'static str,
    artifact: &'static str,
    receipt: &'static str,
    execution_proof: &'static str,
    slot: u16,
    lease_epoch: u64,
    supervisor_generation: u64,
    cap_generation: u64,
    image_sha256: String,
    ready_sequence: u64,
    completion_sequence: u64,
    core: u8,
    scheduling_context: WorkerSchedulingContextSummary,
    object_inventory: KernelObjectInventorySummary,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct WorkerSchedulingContextSummary {
    budget_us: u32,
    period_us: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct KernelObjectInventorySummary {
    tcbs: u32,
    scheduling_contexts: u32,
    reply_objects: u32,
    vspaces: u32,
    cnodes: u32,
    page_tables: u32,
    asids: u32,
    frames: u32,
    endpoints: u32,
    notifications: u32,
    fault_caps: u32,
    timeout_fault_caps: u32,
    cspace_slots: u32,
    untyped_bytes: u64,
}

impl From<&KernelObjectInventory> for KernelObjectInventorySummary {
    fn from(value: &KernelObjectInventory) -> Self {
        Self {
            tcbs: value.tcbs,
            scheduling_contexts: value.scheduling_contexts,
            reply_objects: value.reply_objects,
            vspaces: value.vspaces,
            cnodes: value.cnodes,
            page_tables: value.page_tables,
            asids: value.asids,
            frames: value.frames,
            endpoints: value.endpoints,
            notifications: value.notifications,
            fault_caps: value.fault_caps,
            timeout_fault_caps: value.timeout_fault_caps,
            cspace_slots: value.cspace_slots,
            untyped_bytes: value.untyped_bytes,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct TargetSessionSummary {
    target_session_sha256: String,
    manifest_sha256: String,
    root_image_sha256: String,
    worker_archive_sha256: String,
    worker_image_manifest_sha256: String,
    worker_abi_sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct WorkerAcceptanceSummary {
    schema: String,
    record_kind: &'static str,
    evidence_sha256: String,
    verdict: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'static str>,
    execution_proof: &'static str,
    target_session: TargetSessionSummary,
    topology_sha256: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    workers: Vec<WorkerAcceptanceRoleSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum WorkerAcceptanceDiagnosticCode {
    NotConfigured,
    IncompleteConfiguration,
    UnsafeRoot,
    OutsideRoot,
    SymlinkTraversal,
    NotRegularFile,
    RecordTooLarge,
    ReadFailed,
    InvalidEvidence,
    InvalidTargetSession,
    TargetSessionMismatch,
    ManifestMismatch,
    UnsupportedRecordKind,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct WorkerAcceptanceDiagnostic {
    code: WorkerAcceptanceDiagnosticCode,
}

#[derive(Debug, Clone)]
struct WorkerAcceptanceImport {
    summary: Option<WorkerAcceptanceSummary>,
    diagnostic: Option<WorkerAcceptanceDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
struct Secure9pBounds {
    msize: u32,
    walk_depth: u8,
}

#[derive(Debug, Clone, Serialize)]
struct ConsoleBounds {
    max_line_len: usize,
    max_path_len: usize,
    max_json_len: usize,
    max_id_len: usize,
    max_echo_len: usize,
    max_ticket_len: usize,
}

#[derive(Debug, Clone, Serialize)]
struct PathBounds {
    queen_ctl: &'static str,
    queen_lifecycle_ctl: &'static str,
    queen_schedule_ctl: &'static str,
    queen_lease_ctl: &'static str,
    queen_export_ctl: &'static str,
    policy_ctl: &'static str,
    log: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ControlPlaneBounds {
    schedule: ScheduleBounds,
    lease: LeaseBounds,
    export: ExportBounds,
}

#[derive(Debug, Clone, Serialize)]
struct ScheduleBounds {
    enable: bool,
    queue_max_entries: u32,
    ctl_max_bytes: u32,
}

#[derive(Debug, Clone, Serialize)]
struct LeaseBounds {
    enable: bool,
    active_max_entries: u32,
    preemptions_max_entries: u32,
    ctl_max_bytes: u32,
}

#[derive(Debug, Clone, Serialize)]
struct ExportBounds {
    enable: bool,
    ctl_max_bytes: u32,
}

#[derive(Debug, Clone, Serialize)]
struct PolicyBounds {
    enable: bool,
    queue_max_entries: u32,
    queue_max_bytes: u32,
    ctl_max_bytes: u32,
}

#[derive(Debug, Clone, Serialize)]
struct ObservabilityBounds {
    proc_schedule: ProcScheduleBounds,
    proc_lease: ProcLeaseBounds,
}

#[derive(Debug, Clone, Serialize)]
struct ProcScheduleBounds {
    summary: bool,
    queue: bool,
    summary_bytes: u32,
    queue_bytes: u32,
}

#[derive(Debug, Clone, Serialize)]
struct ProcLeaseBounds {
    summary: bool,
    active: bool,
    preemptions: bool,
    summary_bytes: u32,
    active_bytes: u32,
    preemptions_bytes: u32,
}

#[derive(Debug, Default, Clone)]
struct GatewayStatus {
    connected: bool,
    last_error: Option<String>,
    last_change: Option<SystemTime>,
    reconnects: u64,
    connects: u64,
}

#[derive(Default)]
struct BrokerMetrics {
    control_waiters: AtomicU64,
    telemetry_waiters: AtomicU64,
    control_waiters_high_water: AtomicU64,
    telemetry_waiters_high_water: AtomicU64,
    control_checkouts: AtomicU64,
    telemetry_checkouts: AtomicU64,
    pool_exhausted: AtomicU64,
    checkout_retries: AtomicU64,
    timeout_rejections: AtomicU64,
    telemetry_yields: AtomicU64,
    proc_cache_hits: AtomicU64,
    proc_cache_misses: AtomicU64,
    proc_cache_evictions: AtomicU64,
    control_write_retryable_errors: AtomicU64,
    control_write_retries: AtomicU64,
    control_write_retry_sleep_ms: AtomicU64,
    control_write_retry_exhaustions: AtomicU64,
    control_write_success_after_retry: AtomicU64,
    relay_queue_depth: AtomicU64,
    relay_deduped: AtomicU64,
    relay_remote_write_failures: AtomicU64,
}

impl BrokerMetrics {
    fn wait_counter(&self, kind: PoolKind) -> &AtomicU64 {
        match kind {
            PoolKind::Control => &self.control_waiters,
            PoolKind::Telemetry => &self.telemetry_waiters,
        }
    }

    fn wait_high_water_counter(&self, kind: PoolKind) -> &AtomicU64 {
        match kind {
            PoolKind::Control => &self.control_waiters_high_water,
            PoolKind::Telemetry => &self.telemetry_waiters_high_water,
        }
    }

    fn checkout_counter(&self, kind: PoolKind) -> &AtomicU64 {
        match kind {
            PoolKind::Control => &self.control_checkouts,
            PoolKind::Telemetry => &self.telemetry_checkouts,
        }
    }

    fn snapshot(&self) -> BrokerStatusResponse {
        BrokerStatusResponse {
            control_waiters: self.control_waiters.load(Ordering::Relaxed),
            telemetry_waiters: self.telemetry_waiters.load(Ordering::Relaxed),
            control_waiters_high_water: self.control_waiters_high_water.load(Ordering::Relaxed),
            telemetry_waiters_high_water: self.telemetry_waiters_high_water.load(Ordering::Relaxed),
            control_checkouts: self.control_checkouts.load(Ordering::Relaxed),
            telemetry_checkouts: self.telemetry_checkouts.load(Ordering::Relaxed),
            pool_exhausted: self.pool_exhausted.load(Ordering::Relaxed),
            checkout_retries: self.checkout_retries.load(Ordering::Relaxed),
            timeout_rejections: self.timeout_rejections.load(Ordering::Relaxed),
            telemetry_yields: self.telemetry_yields.load(Ordering::Relaxed),
            proc_cache_hits: self.proc_cache_hits.load(Ordering::Relaxed),
            proc_cache_misses: self.proc_cache_misses.load(Ordering::Relaxed),
            proc_cache_evictions: self.proc_cache_evictions.load(Ordering::Relaxed),
            control_write_retryable_errors: self
                .control_write_retryable_errors
                .load(Ordering::Relaxed),
            control_write_retries: self.control_write_retries.load(Ordering::Relaxed),
            control_write_retry_sleep_ms: self.control_write_retry_sleep_ms.load(Ordering::Relaxed),
            control_write_retry_exhaustions: self
                .control_write_retry_exhaustions
                .load(Ordering::Relaxed),
            control_write_success_after_retry: self
                .control_write_success_after_retry
                .load(Ordering::Relaxed),
            relay_queue_depth: self.relay_queue_depth.load(Ordering::Relaxed),
            relay_deduped: self.relay_deduped.load(Ordering::Relaxed),
            relay_remote_write_failures: self.relay_remote_write_failures.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrokerTimeouts {
    control_response_ms: u64,
    telemetry_response_ms: u64,
}

impl BrokerTimeouts {
    fn response_ms(self, kind: PoolKind) -> u64 {
        match kind {
            PoolKind::Control => self.control_response_ms,
            PoolKind::Telemetry => self.telemetry_response_ms,
        }
    }
}

#[derive(Default)]
struct ProcReadCache {
    entries: HashMap<String, ProcReadCacheEntry>,
    order: VecDeque<String>,
    in_flight: HashMap<String, Arc<ProcReadFill>>,
}

type SharedLines = Arc<[String]>;

#[derive(Default)]
struct ProcReadFill {
    result: Mutex<Option<std::result::Result<SharedLines, String>>>,
    ready: Condvar,
    #[cfg(test)]
    waiters: std::sync::atomic::AtomicUsize,
}

enum ProcReadCacheClaim {
    Hit(SharedLines),
    Leader(Arc<ProcReadFill>),
    Follower(Arc<ProcReadFill>),
    Bypass,
}

impl ProcReadFill {
    fn wait(&self) -> Result<SharedLines> {
        let mut result = match self.result.lock() {
            Ok(result) => result,
            Err(poisoned) => poisoned.into_inner(),
        };
        while result.is_none() {
            #[cfg(test)]
            self.waiters.fetch_add(1, Ordering::Relaxed);
            result = match self.ready.wait(result) {
                Ok(result) => result,
                Err(poisoned) => poisoned.into_inner(),
            };
            #[cfg(test)]
            self.waiters.fetch_sub(1, Ordering::Relaxed);
        }
        let Some(result) = result.as_ref() else {
            return Err(anyhow::anyhow!("gateway cache fill ended without a result"));
        };
        match result {
            Ok(lines) => Ok(Arc::clone(lines)),
            Err(message) => Err(anyhow::anyhow!(message.clone())),
        }
    }

    fn publish(&self, result: std::result::Result<SharedLines, String>) {
        let mut published = match self.result.lock() {
            Ok(published) => published,
            Err(poisoned) => poisoned.into_inner(),
        };
        if published.is_none() {
            *published = Some(result);
        }
        drop(published);
        self.ready.notify_all();
    }
}

struct ProcReadFillLeader<'a> {
    state: &'a AppState,
    path: &'a str,
    fill: Arc<ProcReadFill>,
    completed: bool,
}

impl<'a> ProcReadFillLeader<'a> {
    fn new(state: &'a AppState, path: &'a str, fill: Arc<ProcReadFill>) -> Self {
        Self {
            state,
            path,
            fill,
            completed: false,
        }
    }

    fn complete(mut self, result: std::result::Result<SharedLines, String>) {
        self.state.read_cache_finish(self.path, &self.fill, result);
        self.completed = true;
    }
}

impl Drop for ProcReadFillLeader<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.state.read_cache_finish(
                self.path,
                &self.fill,
                Err("gateway cache fill cancelled".to_owned()),
            );
        }
    }
}

#[derive(Default)]
struct ControlWriteBackpressure {
    entries: HashMap<ControlWriteBackpressureKey, ControlWriteBackpressureEntry>,
    grant_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ControlWriteBackpressureKey {
    Schedule,
    LeaseGrant,
    LeasePreempt,
}

#[derive(Debug, Deserialize)]
struct LeaseControlOperation {
    op: LeaseControlOperationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LeaseControlOperationKind {
    Grant,
    Renew,
    Preempt,
    Quota,
}

struct ControlWriteBackpressureEntry {
    until: Instant,
    last_error: String,
}

impl ControlWriteBackpressure {
    fn record(
        &mut self,
        key: ControlWriteBackpressureKey,
        grant_generation: u64,
        now: Instant,
        cooldown: Duration,
        last_error: &str,
    ) -> bool {
        if key == ControlWriteBackpressureKey::LeaseGrant
            && grant_generation != self.grant_generation
        {
            return false;
        }
        self.entries.insert(
            key,
            ControlWriteBackpressureEntry {
                until: now + cooldown,
                last_error: last_error.to_owned(),
            },
        );
        true
    }

    fn refusal(&mut self, key: ControlWriteBackpressureKey, now: Instant) -> Option<String> {
        let entry = self.entries.get(&key)?;
        if now >= entry.until {
            self.entries.remove(&key);
            return None;
        }
        Some(entry.last_error.clone())
    }

    fn clear_after_success(&mut self, key: ControlWriteBackpressureKey) {
        self.entries.remove(&key);
        if key == ControlWriteBackpressureKey::LeasePreempt {
            self.grant_generation = self.grant_generation.saturating_add(1);
            self.entries
                .remove(&ControlWriteBackpressureKey::LeaseGrant);
        }
    }
}

struct ProcReadCacheEntry {
    inserted_at: Instant,
    lines: SharedLines,
}

fn read_cache_valid_entry(cache: &mut ProcReadCache, path: &str) -> Option<SharedLines> {
    let expired = cache.entries.get(path)?.inserted_at.elapsed()
        > Duration::from_millis(DEFAULT_PROC_CACHE_TTL_MS);
    if expired {
        cache.entries.remove(path);
        cache.order.retain(|value| value != path);
        return None;
    }
    cache
        .entries
        .get(path)
        .map(|entry| Arc::clone(&entry.lines))
}

#[derive(Debug, Clone, Serialize)]
struct GatewayStatusResponse {
    connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    backend_class: Option<BackendClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_acceptance: Option<WorkerAcceptanceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_acceptance_diagnostic: Option<WorkerAcceptanceDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_change_unix_ms: Option<u128>,
    reconnects: u64,
    connects: u64,
    broker: BrokerStatusResponse,
}

#[derive(Debug, Clone, Serialize)]
struct BrokerStatusResponse {
    control_waiters: u64,
    telemetry_waiters: u64,
    control_waiters_high_water: u64,
    telemetry_waiters_high_water: u64,
    control_checkouts: u64,
    telemetry_checkouts: u64,
    pool_exhausted: u64,
    checkout_retries: u64,
    timeout_rejections: u64,
    telemetry_yields: u64,
    proc_cache_hits: u64,
    proc_cache_misses: u64,
    proc_cache_evictions: u64,
    control_write_retryable_errors: u64,
    control_write_retries: u64,
    control_write_retry_sleep_ms: u64,
    control_write_retry_exhaustions: u64,
    control_write_success_after_retry: u64,
    relay_queue_depth: u64,
    relay_deduped: u64,
    relay_remote_write_failures: u64,
}

#[derive(Clone)]
struct GatewayBrokerClient {
    control_tx: SyncSender<BrokerCommand>,
    telemetry_tx: SyncSender<BrokerCommand>,
}

struct BrokerCommand {
    kind: PoolKind,
    request: BrokerRequest,
    response_tx: mpsc::Sender<Result<BrokerResponse>>,
}

enum BrokerRequest {
    Attach { role: Role, ticket: Option<String> },
    Ping,
    List { path: String },
    Read { path: String },
    Tail { path: String, lines: Option<u16> },
    Write { path: String, payload: Vec<u8> },
}

enum BrokerResponse {
    Unit,
    Lines(Vec<String>),
}

struct TelemetryWriteBatch {
    path: String,
    payloads: Vec<Vec<u8>>,
    response_txs: Vec<mpsc::Sender<Result<BrokerResponse>>>,
}

impl TelemetryWriteBatch {
    fn from_command(command: BrokerCommand) -> std::result::Result<Self, BrokerCommand> {
        if command.kind != PoolKind::Telemetry {
            return Err(command);
        }
        let response_tx = command.response_tx;
        match command.request {
            BrokerRequest::Write { path, payload } => {
                if is_batchable_telemetry_write_path(path.as_str()) {
                    Ok(Self {
                        path,
                        payloads: vec![payload],
                        response_txs: vec![response_tx],
                    })
                } else {
                    Err(BrokerCommand {
                        kind: command.kind,
                        request: BrokerRequest::Write { path, payload },
                        response_tx,
                    })
                }
            }
            request => Err(BrokerCommand {
                kind: command.kind,
                request,
                response_tx,
            }),
        }
    }

    fn len(&self) -> usize {
        self.payloads.len()
    }

    fn try_push(&mut self, command: BrokerCommand) -> std::result::Result<(), BrokerCommand> {
        if self.len() >= TELEMETRY_WRITE_BATCH_MAX || command.kind != PoolKind::Telemetry {
            return Err(command);
        }
        let response_tx = command.response_tx;
        match command.request {
            BrokerRequest::Write { path, payload } if path == self.path => {
                self.payloads.push(payload);
                self.response_txs.push(response_tx);
                Ok(())
            }
            request => Err(BrokerCommand {
                kind: command.kind,
                request,
                response_tx,
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PathQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
struct CatQuery {
    path: String,
    max_bytes: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TailQuery {
    path: String,
    max_bytes: Option<u32>,
    lines: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct EchoRequest {
    path: String,
    line: Option<String>,
}

#[derive(Debug, Serialize)]
struct GatewayResponse {
    status: &'static str,
    verb: &'static str,
    path: String,
    end: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    lines: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let config = GatewayConfig::from_cli(cli)?;
    if config.mock {
        info!("hive-gateway mock transport enabled");
    }

    let policy = apply_policy_overrides(CohshPolicy::from_generated(), &config)?;
    info!(
        "hive-gateway session pool control={} telemetry={}",
        policy.pool.control_sessions, policy.pool.telemetry_sessions
    );
    let pool = build_session_pool(&config, policy)?;
    let bounds = build_bounds().context("load generated Worker runtime bounds")?;
    let worker_acceptance = load_worker_acceptance(
        config.worker_acceptance_root.as_deref(),
        config.worker_acceptance_evidence.as_deref(),
        config.target_session.as_deref(),
        CohshPolicy::manifest_hash(),
    );
    if let Some(diagnostic) = worker_acceptance.diagnostic.as_ref() {
        warn!(
            "Worker acceptance evidence unavailable: {:?}",
            diagnostic.code
        );
    }
    let shutdown = Arc::new(AtomicBool::new(false));
    let broker_metrics = Arc::new(BrokerMetrics::default());
    seed_relay_metrics_from_env(&broker_metrics);
    let broker_client =
        build_gateway_broker(pool.clone(), broker_metrics.clone(), shutdown.clone());

    let state = AppState {
        inner: Arc::new(GatewayInner {
            pool: pool.clone(),
            broker_client,
            role: config.role,
            ticket: config.ticket.clone(),
            request_auth_token: config.request_auth_token.clone(),
            status: Mutex::new(GatewayStatus::default()),
            shutdown,
            broker_timeouts: config.broker_timeouts,
            control_write_retry_window_ms: config.control_write_retry_window_ms,
            bounds,
            backend_class: if config.mock {
                BackendClass::HostModel
            } else {
                BackendClass::ConsoleProjection
            },
            worker_acceptance,
            policy,
            broker: broker_metrics,
            proc_cache: Mutex::new(ProcReadCache::default()),
            control_write_backpressure: Mutex::new(ControlWriteBackpressure::default()),
        }),
    };

    let (log_host, log_port) = if config.mock {
        ("mock".to_owned(), 0)
    } else {
        (config.tcp_host.clone(), config.tcp_port)
    };
    spawn_connection_manager(state.clone(), log_host, log_port);

    let app = Router::new()
        .route("/v1/meta/bounds", get(meta_bounds))
        .route("/v1/meta/status", get(meta_status))
        .route("/v1/fs/ls", get(fs_ls))
        .route("/v1/fs/cat", get(fs_cat))
        .route("/v1/fs/tail", get(fs_tail))
        .route("/v1/fs/echo", post(fs_echo))
        .route("/v1/openapi.yaml", get(openapi_yaml))
        .route("/docs", get(swagger_ui))
        .with_state(state.clone());

    let addr: SocketAddr = config
        .bind
        .parse()
        .with_context(|| format!("invalid bind address {}", config.bind))?;
    enforce_bind_exposure(addr, config.allow_non_loopback_bind)?;

    info!("hive-gateway listening on {}", addr);

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {}", addr))?;

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal(state))
        .await
        .context("server shutdown")?;

    Ok(())
}

#[derive(Debug, Clone)]
struct GatewayConfig {
    bind: String,
    tcp_host: String,
    tcp_port: u16,
    auth_token: String,
    request_auth_token: String,
    role: Role,
    ticket: Option<String>,
    pool_control_sessions: Option<u16>,
    pool_telemetry_sessions: Option<u16>,
    broker_timeouts: BrokerTimeouts,
    control_write_retry_window_ms: u64,
    mock: bool,
    allow_non_loopback_bind: bool,
    worker_acceptance_evidence: Option<PathBuf>,
    worker_acceptance_root: Option<PathBuf>,
    target_session: Option<PathBuf>,
}

impl GatewayConfig {
    fn from_cli(cli: Cli) -> Result<Self> {
        let mut mock = cli.mock;
        if !mock {
            if let Ok(value) = env::var("HIVE_GATEWAY_MOCK") {
                let trimmed = value.trim();
                if !trimmed.is_empty() && !matches!(trimmed, "0" | "false" | "off" | "no") {
                    mock = true;
                }
            }
        }
        let bind = env_override(cli.bind, "127.0.0.1:8080", "HIVE_GATEWAY_BIND");
        let tcp_host = env_override(cli.tcp_host, "127.0.0.1", "COH_TCP_HOST");
        let tcp_port = env_override_u16(cli.tcp_port, COHESIX_TCP_CONSOLE_PORT, "COH_TCP_PORT");
        let auth_token = resolve_secret(
            cli.auth_token.as_deref(),
            &["COH_AUTH_TOKEN", "COHSH_AUTH_TOKEN"],
        );
        let request_auth_token = resolve_secret(
            cli.request_auth_token.as_deref(),
            &[
                "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
                "COH_REST_AUTH_TOKEN",
                "COHSH_REST_AUTH_TOKEN",
            ],
        );
        let role_value = env_override(cli.role, "queen", "COH_ROLE");
        let role = parse_role(&role_value, RoleParseMode::AllowWorkerAlias)
            .ok_or_else(|| anyhow::anyhow!("unsupported role '{role_value}'"))?;
        let worker_acceptance_evidence = optional_path_override(
            cli.worker_acceptance_evidence,
            "HIVE_GATEWAY_WORKER_ACCEPTANCE_EVIDENCE",
        );
        let worker_acceptance_root = optional_path_override(
            cli.worker_acceptance_root,
            "HIVE_GATEWAY_WORKER_ACCEPTANCE_ROOT",
        );
        let target_session =
            optional_path_override(cli.target_session, "HIVE_GATEWAY_TARGET_SESSION");
        let mut ticket = cli.ticket;
        if ticket.is_none() {
            if let Ok(value) = env::var("COH_TICKET") {
                let trimmed = value.trim().to_owned();
                if !trimmed.is_empty() {
                    ticket = Some(trimmed);
                }
            }
        }
        let pool_control_sessions = env_override_opt_u16(
            cli.pool_control_sessions,
            "HIVE_GATEWAY_POOL_CONTROL_SESSIONS",
        );
        let pool_telemetry_sessions = env_override_opt_u16(
            cli.pool_telemetry_sessions,
            "HIVE_GATEWAY_POOL_TELEMETRY_SESSIONS",
        );
        let broker_control_response_timeout_ms = broker_response_timeout_ms(
            env_override_opt_u64(
                cli.broker_control_response_timeout_ms,
                &[
                    "HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS",
                    "HIVE_GATEWAY_BROKER_CONTROL_TIMEOUT_MS",
                ],
            ),
            DEFAULT_BROKER_CONTROL_RESPONSE_TIMEOUT_MS,
            "broker control response timeout",
        )?;
        let broker_telemetry_response_timeout_ms = broker_response_timeout_ms(
            env_override_opt_u64(
                cli.broker_telemetry_response_timeout_ms,
                &[
                    "HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS",
                    "HIVE_GATEWAY_BROKER_TELEMETRY_TIMEOUT_MS",
                ],
            ),
            DEFAULT_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS,
            "broker telemetry response timeout",
        )?;
        let control_write_retry_window_ms = env_override_opt_u64(
            cli.control_write_retry_window_ms,
            &["HIVE_GATEWAY_CONTROL_WRITE_RETRY_WINDOW_MS"],
        )
        .unwrap_or(DEFAULT_CONTROL_WRITE_RETRY_WINDOW_MS);
        let control_write_retry_window_ms =
            validate_control_write_retry_window_ms(control_write_retry_window_ms)?;
        let allow_non_loopback_bind =
            cli.allow_non_loopback_bind || env_flag("HIVE_GATEWAY_ALLOW_NON_LOOPBACK_BIND");
        let auth_token = normalize_required_secret("tcp auth token", auth_token, mock)?;
        let request_auth_token =
            normalize_required_secret("request auth token", request_auth_token, mock)?;
        Ok(Self {
            bind,
            tcp_host,
            tcp_port,
            auth_token,
            request_auth_token,
            role,
            ticket,
            pool_control_sessions,
            pool_telemetry_sessions,
            broker_timeouts: BrokerTimeouts {
                control_response_ms: broker_control_response_timeout_ms,
                telemetry_response_ms: broker_telemetry_response_timeout_ms,
            },
            control_write_retry_window_ms,
            mock,
            allow_non_loopback_bind,
            worker_acceptance_evidence,
            worker_acceptance_root,
            target_session,
        })
    }
}

fn env_flag(key: &str) -> bool {
    let Ok(value) = env::var(key) else {
        return false;
    };
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "off" | "no"
    )
}

fn env_override(value: String, default_value: &str, key: &str) -> String {
    if value == default_value {
        if let Ok(env_value) = env::var(key) {
            if !env_value.trim().is_empty() {
                return env_value;
            }
        }
    }
    value
}

fn env_override_u16(value: u16, default_value: u16, key: &str) -> u16 {
    if value == default_value {
        if let Ok(env_value) = env::var(key) {
            if let Ok(parsed) = env_value.parse::<u16>() {
                return parsed;
            }
        }
    }
    value
}

fn env_override_opt_u16(value: Option<u16>, key: &str) -> Option<u16> {
    if value.is_some() {
        return value;
    }
    let Ok(env_value) = env::var(key) else {
        return None;
    };
    env_value.trim().parse::<u16>().ok().filter(|v| *v > 0)
}

fn env_override_opt_u64(value: Option<u64>, keys: &[&str]) -> Option<u64> {
    if value.is_some() {
        return value;
    }
    for key in keys {
        let Ok(env_value) = env::var(key) else {
            continue;
        };
        let trimmed = env_value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(parsed) = trimmed.parse::<u64>() {
            return Some(parsed);
        }
    }
    None
}

fn optional_path_override(value: Option<PathBuf>, key: &str) -> Option<PathBuf> {
    if value.is_some() {
        return value;
    }
    let value = env::var_os(key)?;
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn acceptance_diagnostic(code: WorkerAcceptanceDiagnosticCode) -> WorkerAcceptanceImport {
    WorkerAcceptanceImport {
        summary: None,
        diagnostic: Some(WorkerAcceptanceDiagnostic { code }),
    }
}

fn normalize_path_without_parent(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => return None,
            other => normalized.push(other.as_os_str()),
        }
    }
    Some(normalized)
}

fn read_bounded_trusted_file(
    root: &Path,
    selected: &Path,
) -> Result<Vec<u8>, WorkerAcceptanceDiagnosticCode> {
    let current_dir = env::current_dir().map_err(|_| WorkerAcceptanceDiagnosticCode::UnsafeRoot)?;
    let root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        current_dir.join(root)
    };
    let selected = if selected.is_absolute() {
        selected.to_path_buf()
    } else {
        current_dir.join(selected)
    };
    let root =
        normalize_path_without_parent(&root).ok_or(WorkerAcceptanceDiagnosticCode::UnsafeRoot)?;
    let selected = normalize_path_without_parent(&selected)
        .ok_or(WorkerAcceptanceDiagnosticCode::OutsideRoot)?;
    let root_metadata =
        fs::symlink_metadata(&root).map_err(|_| WorkerAcceptanceDiagnosticCode::UnsafeRoot)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(WorkerAcceptanceDiagnosticCode::UnsafeRoot);
    }
    let relative = selected
        .strip_prefix(&root)
        .map_err(|_| WorkerAcceptanceDiagnosticCode::OutsideRoot)?;
    if relative.as_os_str().is_empty() {
        return Err(WorkerAcceptanceDiagnosticCode::NotRegularFile);
    }
    let mut cursor = root.clone();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&cursor)
            .map_err(|_| WorkerAcceptanceDiagnosticCode::ReadFailed)?;
        if metadata.file_type().is_symlink() {
            return Err(WorkerAcceptanceDiagnosticCode::SymlinkTraversal);
        }
    }
    let canonical_root =
        fs::canonicalize(&root).map_err(|_| WorkerAcceptanceDiagnosticCode::UnsafeRoot)?;
    let canonical_selected =
        fs::canonicalize(&selected).map_err(|_| WorkerAcceptanceDiagnosticCode::ReadFailed)?;
    if !canonical_selected.starts_with(&canonical_root) {
        return Err(WorkerAcceptanceDiagnosticCode::OutsideRoot);
    }
    let metadata = fs::metadata(&canonical_selected)
        .map_err(|_| WorkerAcceptanceDiagnosticCode::ReadFailed)?;
    if !metadata.is_file() {
        return Err(WorkerAcceptanceDiagnosticCode::NotRegularFile);
    }
    if metadata.len() > MAX_WORKER_ACCEPTANCE_EVIDENCE_BYTES {
        return Err(WorkerAcceptanceDiagnosticCode::RecordTooLarge);
    }
    let bytes =
        fs::read(&canonical_selected).map_err(|_| WorkerAcceptanceDiagnosticCode::ReadFailed)?;
    if bytes.len() as u64 > MAX_WORKER_ACCEPTANCE_EVIDENCE_BYTES {
        return Err(WorkerAcceptanceDiagnosticCode::RecordTooLarge);
    }
    Ok(bytes)
}

fn load_worker_acceptance(
    root: Option<&Path>,
    evidence: Option<&Path>,
    target_session: Option<&Path>,
    expected_manifest_sha256: &str,
) -> WorkerAcceptanceImport {
    let (Some(root), Some(evidence), Some(target_session)) = (root, evidence, target_session)
    else {
        return if root.is_none() && evidence.is_none() && target_session.is_none() {
            acceptance_diagnostic(WorkerAcceptanceDiagnosticCode::NotConfigured)
        } else {
            acceptance_diagnostic(WorkerAcceptanceDiagnosticCode::IncompleteConfiguration)
        };
    };
    let bytes = match read_bounded_trusted_file(root, evidence) {
        Ok(bytes) => bytes,
        Err(code) => return acceptance_diagnostic(code),
    };
    let target_session_bytes = match read_bounded_trusted_file(root, target_session) {
        Ok(bytes) => bytes,
        Err(code) => return acceptance_diagnostic(code),
    };
    let current_target_session = match parse_target_session(&target_session_bytes) {
        Ok(session) => session,
        Err(_) => {
            return acceptance_diagnostic(WorkerAcceptanceDiagnosticCode::InvalidTargetSession)
        }
    };
    if current_target_session.manifest_sha256.0 != expected_manifest_sha256 {
        return acceptance_diagnostic(WorkerAcceptanceDiagnosticCode::ManifestMismatch);
    }
    let evidence_sha256 = Sha256Hex::digest(&bytes).0;
    let target_session_sha256 = Sha256Hex::digest(&target_session_bytes).0;
    match parse_evidence(&bytes) {
        Ok(ValidatedEvidence::Component(record)) => {
            if record.target_session != current_target_session {
                return acceptance_diagnostic(
                    WorkerAcceptanceDiagnosticCode::TargetSessionMismatch,
                );
            }
            let summary = acceptance_summary(&record, evidence_sha256, target_session_sha256);
            WorkerAcceptanceImport {
                summary: Some(summary),
                diagnostic: None,
            }
        }
        Ok(_) => acceptance_diagnostic(WorkerAcceptanceDiagnosticCode::UnsupportedRecordKind),
        Err(_) => acceptance_diagnostic(WorkerAcceptanceDiagnosticCode::InvalidEvidence),
    }
}

fn acceptance_summary(
    record: &cohesix_worker_evidence::WorkerComponentEvidence,
    evidence_sha256: String,
    target_session_sha256: String,
) -> WorkerAcceptanceSummary {
    let target = target_label(record.target);
    let verdict = verdict_label(record.verdict);
    let execution_proof = if record.verdict == Verdict::Pass {
        target_proof_label(record.target)
    } else {
        "none"
    };
    let workers = record
        .workers
        .iter()
        .map(|worker| WorkerAcceptanceRoleSummary {
            role: worker_role_label(worker.identity.role),
            lifecycle: lifecycle_label(worker.state.lifecycle),
            artifact: artifact_label(worker.state.artifact),
            receipt: receipt_label(worker.state.receipt),
            execution_proof: proof_label(worker.state.execution_proof),
            slot: worker.identity.slot,
            lease_epoch: worker.identity.lease_epoch,
            supervisor_generation: worker.identity.supervisor_generation,
            cap_generation: worker.identity.cap_generation,
            image_sha256: worker.image_sha256.0.clone(),
            ready_sequence: worker.ready_sequence,
            completion_sequence: worker.completion_sequence,
            core: worker.core,
            scheduling_context: WorkerSchedulingContextSummary {
                budget_us: worker.scheduling_context.budget_us,
                period_us: worker.scheduling_context.period_us,
            },
            object_inventory: KernelObjectInventorySummary::from(&worker.object_inventory),
        })
        .collect();
    WorkerAcceptanceSummary {
        schema: record.schema.clone(),
        record_kind: "target-component",
        evidence_sha256,
        verdict,
        target: Some(target),
        execution_proof,
        target_session: target_session_summary(&record.target_session, target_session_sha256),
        topology_sha256: record.topology_sha256.0.clone(),
        workers,
    }
}

fn target_session_summary(
    session: &TargetSession,
    target_session_sha256: String,
) -> TargetSessionSummary {
    TargetSessionSummary {
        target_session_sha256,
        manifest_sha256: session.manifest_sha256.0.clone(),
        root_image_sha256: session.root_image_sha256.0.clone(),
        worker_archive_sha256: session.worker_archive_sha256.0.clone(),
        worker_image_manifest_sha256: session.worker_image_manifest_sha256.0.clone(),
        worker_abi_sha256: session.worker_abi_sha256.0.clone(),
    }
}

const fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "PASS",
        Verdict::Fail => "FAIL",
    }
}

const fn target_label(target: TargetClass) -> &'static str {
    match target {
        TargetClass::Qemu => "qemu",
        TargetClass::Pi4 => "pi4",
    }
}

const fn target_proof_label(target: TargetClass) -> &'static str {
    match target {
        TargetClass::Qemu => "qemu",
        TargetClass::Pi4 => "fresh-pi",
    }
}

const fn worker_role_label(role: WorkerRole) -> &'static str {
    match role {
        WorkerRole::WorkerHeartbeat => "worker-heartbeat",
        WorkerRole::WorkerGpu => "worker-gpu",
        WorkerRole::WorkerLora => "worker-lora",
        WorkerRole::WorkerBus => "worker-bus",
    }
}

const fn lifecycle_label(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::Absent => "absent",
        LifecycleState::Queued => "queued",
        LifecycleState::Starting => "starting",
        LifecycleState::Ready => "ready",
        LifecycleState::Closing => "closing",
        LifecycleState::Faulted => "faulted",
        LifecycleState::Terminal => "terminal",
    }
}

const fn artifact_label(state: ArtifactState) -> &'static str {
    match state {
        ArtifactState::Missing => "missing",
        ArtifactState::Verified => "verified",
        ArtifactState::Mismatch => "mismatch",
    }
}

const fn receipt_label(state: ReceiptState) -> &'static str {
    match state {
        ReceiptState::None => "none",
        ReceiptState::Pending => "pending",
        ReceiptState::Confirmed => "confirmed",
        ReceiptState::Rejected => "rejected",
        ReceiptState::Stale => "stale",
    }
}

const fn proof_label(proof: ExecutionProof) -> &'static str {
    match proof {
        ExecutionProof::None => "none",
        ExecutionProof::HostModel => "host-model",
        ExecutionProof::Qemu => "qemu",
        ExecutionProof::FreshPi => "fresh-pi",
    }
}

fn broker_response_timeout_ms(value: Option<u64>, default_ms: u64, label: &str) -> Result<u64> {
    let timeout_ms = value.unwrap_or(default_ms);
    if timeout_ms < BROKER_QUEUE_WAIT_LIMIT_MS {
        anyhow::bail!("{label} must be >= broker queue wait limit {BROKER_QUEUE_WAIT_LIMIT_MS}ms");
    }
    Ok(timeout_ms)
}

fn validate_control_write_retry_window_ms(value: u64) -> Result<u64> {
    if value > MAX_CONTROL_WRITE_RETRY_WINDOW_MS {
        anyhow::bail!(
            "control write retry window must be <= {MAX_CONTROL_WRITE_RETRY_WINDOW_MS}ms"
        );
    }
    Ok(value)
}

fn seed_relay_metrics_from_env(metrics: &BrokerMetrics) {
    if let Some(value) = env_u64("HIVE_GATEWAY_RELAY_QUEUE_DEPTH") {
        metrics.relay_queue_depth.store(value, Ordering::Relaxed);
    }
    if let Some(value) = env_u64("HIVE_GATEWAY_RELAY_DEDUPED") {
        metrics.relay_deduped.store(value, Ordering::Relaxed);
    }
    if let Some(value) = env_u64("HIVE_GATEWAY_RELAY_REMOTE_WRITE_FAILURES") {
        metrics
            .relay_remote_write_failures
            .store(value, Ordering::Relaxed);
    }
}

fn env_u64(key: &str) -> Option<u64> {
    env::var(key).ok()?.trim().parse::<u64>().ok()
}

fn resolve_secret(cli_value: Option<&str>, env_keys: &[&str]) -> Option<String> {
    if let Some(value) = cli_value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    for key in env_keys {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn allow_insecure_console_auth() -> bool {
    env_flag("HIVE_GATEWAY_ALLOW_INSECURE_CONSOLE_AUTH")
        || env_flag("COHESIX_ALLOW_INSECURE_CONSOLE_AUTH")
}

fn normalize_required_secret(label: &str, value: Option<String>, mock: bool) -> Result<String> {
    let secret = value.unwrap_or_default();
    let trimmed = secret.trim();
    if mock {
        if trimmed.is_empty() {
            return Ok("mock-only-token".to_owned());
        }
        return Ok(trimmed.to_owned());
    }
    if trimmed.is_empty() {
        anyhow::bail!("{label} must be configured in non-mock mode");
    }
    if trimmed == INSECURE_PLACEHOLDER_TOKEN {
        if label == "tcp auth token" && allow_insecure_console_auth() {
            warn!(
                "allowing insecure TCP auth token placeholder because HIVE_GATEWAY_ALLOW_INSECURE_CONSOLE_AUTH is set"
            );
            return Ok(trimmed.to_owned());
        }
        anyhow::bail!("{label} uses insecure placeholder token; set a real secret");
    }
    Ok(trimmed.to_owned())
}

fn enforce_bind_exposure(addr: SocketAddr, allow_non_loopback: bool) -> Result<()> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if allow_non_loopback {
        warn!(
            "hive-gateway binding to non-loopback address {}; exposure override enabled",
            addr
        );
        return Ok(());
    }
    anyhow::bail!(
        "refusing non-loopback bind {}; set --allow-non-loopback-bind or HIVE_GATEWAY_ALLOW_NON_LOOPBACK_BIND=1",
        addr
    );
}

fn apply_policy_overrides(policy: CohshPolicy, config: &GatewayConfig) -> Result<CohshPolicy> {
    let overrides = PolicyOverrides {
        pool_control_sessions: config.pool_control_sessions,
        pool_telemetry_sessions: config.pool_telemetry_sessions,
        ..PolicyOverrides::default()
    };
    if overrides == PolicyOverrides::default() {
        return Ok(policy);
    }
    policy.with_overrides(&overrides).context(
        "failed to apply gateway pool overrides; check pool_control_sessions/pool_telemetry_sessions",
    )
}

fn build_session_pool(config: &GatewayConfig, policy: CohshPolicy) -> Result<SharedPool> {
    if config.mock {
        let server = NineDoor::new_with_shard_layout(ShardLayout::enabled(8, true));
        let factory: Arc<dyn TransportFactory> = Arc::new(move || {
            Ok(Box::new(NineDoorTransport::new(server.clone()))
                as Box<dyn cohsh::Transport + Send>)
        });
        return Ok(SessionPool::new(
            policy.pool.control_sessions,
            policy.pool.telemetry_sessions,
            factory,
        ));
    }
    let tcp = TcpTransport::new(&config.tcp_host, config.tcp_port)
        .with_auth_token(&config.auth_token)
        .with_retry_policy(policy.retry)
        .with_heartbeat_interval(Duration::from_millis(policy.heartbeat.interval_ms));
    let inner = Arc::new(Mutex::new(tcp));
    let factory: Arc<dyn TransportFactory> = Arc::new(move || {
        Ok(Box::new(PooledTcpTransport::new(inner.clone())) as Box<dyn cohsh::Transport + Send>)
    });
    Ok(SessionPool::new(
        policy.pool.control_sessions,
        policy.pool.telemetry_sessions,
        factory,
    ))
}

#[derive(Debug, Deserialize)]
struct GeneratedGatewayWorkerProfile {
    namespace: GeneratedGatewayNamespace,
    schemas: GeneratedGatewaySchemas,
    worker: GeneratedGatewayWorker,
}

#[derive(Debug, Deserialize)]
struct GeneratedGatewayNamespace {
    legacy_worker_alias: bool,
    shard_bits: u8,
    telemetry_path_template: String,
}

#[derive(Debug, Deserialize)]
struct GeneratedGatewaySchemas {
    worker_observation: String,
    worker_integration_evidence: String,
}

#[derive(Debug, Deserialize)]
struct GeneratedGatewayWorker {
    maximum_live_tasks: u16,
    roles: Vec<WorkerRoleBounds>,
    task_abi_schema: String,
    task_abi_version: u16,
}

fn build_worker_runtime_bounds() -> Result<WorkerRuntimeBounds> {
    const GENERATED_PROFILE: &str =
        include_str!("../../../configs/generated/cohesix_python_qemu_smp_production.json");
    let profile: GeneratedGatewayWorkerProfile = serde_json::from_str(GENERATED_PROFILE)
        .context("parse compiler-generated qemu_smp_production Worker profile")?;
    if profile.worker.roles.is_empty()
        || profile.worker.roles.len() > 16
        || profile.worker.maximum_live_tasks == 0
        || profile.worker.task_abi_version == 0
        || profile.namespace.shard_bits == 0
        || profile.namespace.shard_bits > 8
        || profile.worker.task_abi_schema.is_empty()
        || profile.schemas.worker_observation.is_empty()
        || profile.schemas.worker_integration_evidence.is_empty()
        || profile.namespace.telemetry_path_template != "/shard/<label>/worker/<id>/telemetry"
    {
        anyhow::bail!("invalid compiler-generated Worker runtime bounds");
    }
    let executable_slots = profile
        .worker
        .roles
        .iter()
        .try_fold(0u16, |total, role| total.checked_add(role.executable_slots))
        .context("generated Worker slot total overflow")?;
    let mut role_names = profile
        .worker
        .roles
        .iter()
        .map(|role| role.role.as_str())
        .collect::<Vec<_>>();
    role_names.sort_unstable();
    if role_names.windows(2).any(|pair| pair[0] == pair[1])
        || executable_slots != profile.worker.maximum_live_tasks
        || profile.worker.roles.iter().any(|role| {
            role.role.is_empty()
                || role.role.len() > MAX_ID_LEN
                || (role.declaration == WorkerDeclaration::ModelOnly && role.executable_slots != 0)
        })
    {
        anyhow::bail!("inconsistent compiler-generated Worker role matrix");
    }
    Ok(WorkerRuntimeBounds {
        roles: profile.worker.roles,
        task_abi_schema: profile.worker.task_abi_schema,
        task_abi_version: profile.worker.task_abi_version,
        worker_observation_schema: profile.schemas.worker_observation,
        worker_integration_evidence_schema: profile.schemas.worker_integration_evidence,
        maximum_live_tasks: profile.worker.maximum_live_tasks,
        canonical_telemetry_template: profile.namespace.telemetry_path_template,
        shard_bits: profile.namespace.shard_bits,
        legacy_worker_alias: profile.namespace.legacy_worker_alias,
    })
}

fn build_bounds() -> Result<BoundsResponse> {
    Ok(BoundsResponse {
        manifest_sha256: CohshPolicy::manifest_hash(),
        secure9p: Secure9pBounds {
            msize: SECURE9P_MSIZE,
            walk_depth: SECURE9P_WALK_DEPTH,
        },
        console: ConsoleBounds {
            max_line_len: MAX_LINE_LEN,
            max_path_len: MAX_PATH_LEN,
            max_json_len: MAX_JSON_LEN,
            max_id_len: MAX_ID_LEN,
            max_echo_len: MAX_ECHO_LEN,
            max_ticket_len: MAX_TICKET_LEN,
        },
        paths: PathBounds {
            queen_ctl: CLIENT_QUEEN_CTL_PATH,
            queen_lifecycle_ctl: CLIENT_QUEEN_LIFECYCLE_CTL_PATH,
            queen_schedule_ctl: CLIENT_QUEEN_SCHEDULE_CTL_PATH,
            queen_lease_ctl: CLIENT_QUEEN_LEASE_CTL_PATH,
            queen_export_ctl: CLIENT_QUEEN_EXPORT_CTL_PATH,
            policy_ctl: CLIENT_POLICY_CTL_PATH,
            log: CLIENT_LOG_PATH,
        },
        control_plane: ControlPlaneBounds {
            schedule: ScheduleBounds {
                enable: CONTROL_SCHEDULE_ENABLED,
                queue_max_entries: CONTROL_SCHEDULE_QUEUE_MAX_ENTRIES,
                ctl_max_bytes: CONTROL_SCHEDULE_CTL_MAX_BYTES,
            },
            lease: LeaseBounds {
                enable: CONTROL_LEASE_ENABLED,
                active_max_entries: CONTROL_LEASE_ACTIVE_MAX_ENTRIES,
                preemptions_max_entries: CONTROL_LEASE_PREEMPTIONS_MAX_ENTRIES,
                ctl_max_bytes: CONTROL_LEASE_CTL_MAX_BYTES,
            },
            export: ExportBounds {
                enable: CONTROL_EXPORT_ENABLED,
                ctl_max_bytes: CONTROL_EXPORT_CTL_MAX_BYTES,
            },
        },
        policy: PolicyBounds {
            enable: POLICY_ENABLED,
            queue_max_entries: POLICY_QUEUE_MAX_ENTRIES,
            queue_max_bytes: POLICY_QUEUE_MAX_BYTES,
            ctl_max_bytes: POLICY_CTL_MAX_BYTES,
        },
        observability: ObservabilityBounds {
            proc_schedule: ProcScheduleBounds {
                summary: PROC_SCHEDULE_SUMMARY_ENABLED,
                queue: PROC_SCHEDULE_QUEUE_ENABLED,
                summary_bytes: PROC_SCHEDULE_SUMMARY_BYTES,
                queue_bytes: PROC_SCHEDULE_QUEUE_BYTES,
            },
            proc_lease: ProcLeaseBounds {
                summary: PROC_LEASE_SUMMARY_ENABLED,
                active: PROC_LEASE_ACTIVE_ENABLED,
                preemptions: PROC_LEASE_PREEMPTIONS_ENABLED,
                summary_bytes: PROC_LEASE_SUMMARY_BYTES,
                active_bytes: PROC_LEASE_ACTIVE_BYTES,
                preemptions_bytes: PROC_LEASE_PREEMPTIONS_BYTES,
            },
        },
        worker_runtime: Some(build_worker_runtime_bounds()?),
    })
}

fn build_gateway_broker(
    pool: SharedPool,
    metrics: Arc<BrokerMetrics>,
    shutdown: Arc<AtomicBool>,
) -> GatewayBrokerClient {
    let (control_tx, control_rx) = mpsc::sync_channel(BROKER_CONTROL_QUEUE_CAPACITY);
    let (telemetry_tx, telemetry_rx) = mpsc::sync_channel(BROKER_TELEMETRY_QUEUE_CAPACITY);
    thread::spawn(move || {
        run_broker_dispatcher(pool, metrics, shutdown, control_rx, telemetry_rx);
    });
    GatewayBrokerClient {
        control_tx,
        telemetry_tx,
    }
}

fn run_broker_dispatcher(
    pool: SharedPool,
    metrics: Arc<BrokerMetrics>,
    shutdown: Arc<AtomicBool>,
    control_rx: Receiver<BrokerCommand>,
    telemetry_rx: Receiver<BrokerCommand>,
) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let mut dispatched = false;
        for _ in 0..BROKER_CONTROL_BURST {
            match control_rx.try_recv() {
                Ok(command) => {
                    dispatched = true;
                    dispatch_broker_command(&pool, &metrics, command);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        match telemetry_rx.try_recv() {
            Ok(command) => {
                dispatched = true;
                dispatch_telemetry_command(&pool, &metrics, command, &telemetry_rx);
            }
            Err(TryRecvError::Empty) => {
                if dispatched {
                    metrics.telemetry_yields.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(TryRecvError::Disconnected) => {}
        }

        if dispatched {
            continue;
        }

        match control_rx.recv_timeout(Duration::from_millis(BROKER_IDLE_WAIT_MS)) {
            Ok(command) => {
                dispatch_broker_command(&pool, &metrics, command);
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {}
        }

        match telemetry_rx.recv_timeout(Duration::from_millis(BROKER_IDLE_WAIT_MS)) {
            Ok(command) => dispatch_telemetry_command(&pool, &metrics, command, &telemetry_rx),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {}
        }
    }
    pool.shutdown();
}

fn dispatch_telemetry_command(
    pool: &SharedPool,
    metrics: &BrokerMetrics,
    command: BrokerCommand,
    telemetry_rx: &Receiver<BrokerCommand>,
) {
    let mut batch = match TelemetryWriteBatch::from_command(command) {
        Ok(batch) => batch,
        Err(command) => {
            dispatch_broker_command(pool, metrics, command);
            return;
        }
    };
    let mut deferred = None;
    while batch.len() < TELEMETRY_WRITE_BATCH_MAX {
        match telemetry_rx.try_recv() {
            Ok(command) => match batch.try_push(command) {
                Ok(()) => {}
                Err(command) => {
                    deferred = Some(command);
                    break;
                }
            },
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }
    dispatch_telemetry_write_batch(pool, metrics, batch);
    if let Some(command) = deferred {
        dispatch_broker_command(pool, metrics, command);
    }
}

fn dispatch_telemetry_write_batch(
    pool: &SharedPool,
    metrics: &BrokerMetrics,
    batch: TelemetryWriteBatch,
) {
    let TelemetryWriteBatch {
        path,
        payloads,
        response_txs,
    } = batch;
    let payload_count = response_txs.len();
    for _ in 0..payload_count {
        decrement_counter(metrics.wait_counter(PoolKind::Telemetry));
    }
    metrics
        .checkout_counter(PoolKind::Telemetry)
        .fetch_add(1, Ordering::Relaxed);
    let result = execute_broker_write_batch(pool, PoolKind::Telemetry, path, payloads)
        .map_err(|err| map_broker_error(metrics, PoolKind::Telemetry, err));
    match result {
        Ok(written) => {
            for (index, response_tx) in response_txs.into_iter().enumerate() {
                if index < written {
                    let _ = response_tx.send(Ok(BrokerResponse::Unit));
                } else {
                    let _ = response_tx.send(Err(anyhow::anyhow!(
                        "telemetry write batch acknowledged {written}/{payload_count} payloads"
                    )));
                }
            }
        }
        Err(err) => {
            let message = err.to_string();
            for response_tx in response_txs {
                let _ = response_tx.send(Err(anyhow::anyhow!(message.clone())));
            }
        }
    }
}

fn dispatch_broker_command(pool: &SharedPool, metrics: &BrokerMetrics, command: BrokerCommand) {
    decrement_counter(metrics.wait_counter(command.kind));
    metrics
        .checkout_counter(command.kind)
        .fetch_add(1, Ordering::Relaxed);
    let result = execute_broker_request(pool, command.kind, command.request)
        .map_err(|err| map_broker_error(metrics, command.kind, err));
    let _ = command.response_tx.send(result);
}

fn map_broker_error(metrics: &BrokerMetrics, kind: PoolKind, err: anyhow::Error) -> anyhow::Error {
    if is_pool_exhausted(&err) {
        metrics.pool_exhausted.fetch_add(1, Ordering::Relaxed);
        anyhow::anyhow!(
            "gateway backpressure: broker checkout exhausted for {:?}",
            kind
        )
    } else {
        err
    }
}

fn execute_broker_request(
    pool: &SharedPool,
    kind: PoolKind,
    request: BrokerRequest,
) -> Result<BrokerResponse> {
    match request {
        BrokerRequest::Attach { role, ticket } => {
            attach_and_prime_pool(pool, role, ticket.as_deref())?;
            Ok(BrokerResponse::Unit)
        }
        BrokerRequest::Ping => with_pool_once(pool, kind, |transport, session| {
            transport.ping(session).context("ping failed")?;
            Ok(BrokerResponse::Unit)
        }),
        BrokerRequest::List { path } => with_pool_once(pool, kind, move |transport, session| {
            transport.list(session, &path).map(BrokerResponse::Lines)
        }),
        BrokerRequest::Read { path } => with_pool_once(pool, kind, move |transport, session| {
            transport.read(session, &path).map(BrokerResponse::Lines)
        }),
        BrokerRequest::Tail { path, lines } => {
            with_pool_once(pool, kind, move |transport, session| {
                transport
                    .tail(session, &path, lines)
                    .map(BrokerResponse::Lines)
            })
        }
        BrokerRequest::Write { path, payload } => {
            with_pool_once(pool, kind, move |transport, session| {
                if is_telemetry_control_path(path.as_str()) {
                    let _ = transport.drain_acknowledgements();
                    let result = transport.write(session, path.as_str(), payload.as_slice());
                    let acknowledgements = transport.drain_acknowledgements();
                    result?;
                    let lines = telemetry_segment_id_from_ack_lines(
                        path.as_str(),
                        acknowledgements.as_slice(),
                    )
                    .into_iter()
                    .collect();
                    Ok(BrokerResponse::Lines(lines))
                } else {
                    transport.write(session, path.as_str(), payload.as_slice())?;
                    Ok(BrokerResponse::Unit)
                }
            })
        }
    }
}

fn attach_and_prime_pool(pool: &SharedPool, role: Role, ticket: Option<&str>) -> Result<()> {
    let result = (|| {
        pool.attach(role, ticket)?;

        // Readiness covers both logical request lanes; remaining configured capacity stays lazy.
        let control = pool
            .checkout(PoolKind::Control)
            .context("prime gateway control session")?;
        drop(control);

        let telemetry = pool
            .checkout(PoolKind::Telemetry)
            .context("prime gateway telemetry session")?;
        drop(telemetry);
        Ok(())
    })();

    if result.is_err() {
        pool.shutdown();
    }
    result
}

fn execute_broker_write_batch(
    pool: &SharedPool,
    kind: PoolKind,
    path: String,
    payloads: Vec<Vec<u8>>,
) -> Result<usize> {
    let mut lease = pool.checkout(kind)?;
    let session = lease.session().clone();
    lease
        .transport_mut()
        .write_batch(&session, path.as_str(), payloads.as_slice())
}

fn with_pool_once<F>(pool: &SharedPool, kind: PoolKind, action: F) -> Result<BrokerResponse>
where
    F: FnOnce(&mut dyn cohsh::Transport, &Session) -> Result<BrokerResponse>,
{
    let mut lease = pool.checkout(kind)?;
    let session = lease.session().clone();
    action(lease.transport_mut(), &session)
}

fn decrement_counter(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_sub(1))
    });
}

fn spawn_connection_manager(state: AppState, host: String, port: u16) {
    thread::spawn(move || {
        let mut backoff = Duration::from_millis(state.inner.policy.retry.backoff_ms);
        let ceiling = Duration::from_millis(state.inner.policy.retry.ceiling_ms).max(backoff);
        let heartbeat = Duration::from_millis(state.inner.policy.heartbeat.interval_ms)
            .max(Duration::from_millis(250));
        let mut attempt = 0u64;
        loop {
            if state.inner.shutdown.load(Ordering::SeqCst) {
                break;
            }
            if state.is_connected() {
                if let Err(err) = state.ping() {
                    state.mark_disconnected(err);
                    state.inner.pool.shutdown();
                    attempt = 0;
                    backoff = Duration::from_millis(state.inner.policy.retry.backoff_ms);
                    warn!("hive-gateway disconnected");
                    continue;
                }
                thread::sleep(heartbeat);
                continue;
            }
            attempt = attempt.saturating_add(1);
            info!(
                "hive-gateway reconnect attempt #{} to {}:{}",
                attempt, host, port
            );
            match state.attach() {
                Ok(()) => {
                    state.mark_connected();
                    attempt = 0;
                    backoff = Duration::from_millis(state.inner.policy.retry.backoff_ms);
                    info!("hive-gateway connected");
                    // AUTH plus the shared wire ATTACH establish initial readiness for both
                    // logical lanes. Keep the first steady-state PING behind one normal interval
                    // so it cannot block first use.
                    thread::sleep(heartbeat);
                }
                Err(err) => {
                    state.inner.pool.shutdown();
                    state.mark_disconnected(err);
                    thread::sleep(backoff);
                    backoff = next_delay(backoff, ceiling);
                }
            }
        }
    });
}

fn next_delay(current: Duration, ceiling: Duration) -> Duration {
    let doubled = current + current;
    if doubled > ceiling {
        ceiling
    } else {
        doubled
    }
}

fn duration_millis_u64(duration: Duration) -> u64 {
    let millis = duration.as_millis();
    if millis > u128::from(u64::MAX) {
        u64::MAX
    } else {
        millis as u64
    }
}

async fn shutdown_signal(state: AppState) {
    let _ = signal::ctrl_c().await;
    state.inner.shutdown.store(true, Ordering::SeqCst);
}

impl AppState {
    fn submit_broker(&self, kind: PoolKind, request: BrokerRequest) -> Result<BrokerResponse> {
        let queue_deadline = Instant::now() + Duration::from_millis(BROKER_QUEUE_WAIT_LIMIT_MS);
        let tx = match kind {
            PoolKind::Control => &self.inner.broker_client.control_tx,
            PoolKind::Telemetry => &self.inner.broker_client.telemetry_tx,
        };
        let (response_tx, response_rx) = mpsc::channel();
        let mut command = BrokerCommand {
            kind,
            request,
            response_tx,
        };
        loop {
            match tx.try_send(command) {
                Ok(()) => {
                    let waiters = self
                        .inner
                        .broker
                        .wait_counter(kind)
                        .fetch_add(1, Ordering::Relaxed)
                        .saturating_add(1);
                    self.inner
                        .broker
                        .wait_high_water_counter(kind)
                        .fetch_max(waiters, Ordering::Relaxed);
                    break;
                }
                Err(TrySendError::Full(returned)) => {
                    command = returned;
                    self.inner
                        .broker
                        .pool_exhausted
                        .fetch_add(1, Ordering::Relaxed);
                    if self.inner.shutdown.load(Ordering::SeqCst) {
                        return Err(anyhow::anyhow!("gateway broker is shutting down"));
                    }
                    if Instant::now() >= queue_deadline {
                        self.inner
                            .broker
                            .timeout_rejections
                            .fetch_add(1, Ordering::Relaxed);
                        return Err(anyhow::anyhow!(
                            "gateway backpressure: broker queue timed out for {kind:?}"
                        ));
                    }
                    self.inner
                        .broker
                        .checkout_retries
                        .fetch_add(1, Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(BROKER_ENQUEUE_RETRY_SLEEP_MS));
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(anyhow::anyhow!("gateway broker is unavailable"));
                }
            }
        }
        let response_timeout_ms = self.inner.broker_timeouts.response_ms(kind);
        let response_deadline = Instant::now() + Duration::from_millis(response_timeout_ms);
        let remaining = response_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            self.inner
                .broker
                .timeout_rejections
                .fetch_add(1, Ordering::Relaxed);
            return Err(anyhow::anyhow!(
                "gateway timeout: broker response timed out for {kind:?} after {response_timeout_ms}ms"
            ));
        }
        match response_rx.recv_timeout(remaining) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.inner
                    .broker
                    .timeout_rejections
                    .fetch_add(1, Ordering::Relaxed);
                Err(anyhow::anyhow!(
                    "gateway timeout: broker response timed out for {kind:?} after {response_timeout_ms}ms"
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(anyhow::anyhow!("gateway broker disconnected"))
            }
        }
    }

    fn attach(&self) -> Result<()> {
        self.submit_broker(
            PoolKind::Control,
            BrokerRequest::Attach {
                role: self.inner.role,
                ticket: self.inner.ticket.clone(),
            },
        )?;
        Ok(())
    }

    fn ping(&self) -> Result<()> {
        self.submit_broker(PoolKind::Control, BrokerRequest::Ping)?;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        let status = self.inner.status.lock().expect("status lock poisoned");
        status.connected
    }

    fn mark_connected(&self) {
        let mut status = self.inner.status.lock().expect("status lock poisoned");
        status.connected = true;
        status.last_error = None;
        status.last_change = Some(SystemTime::now());
        status.connects = status.connects.saturating_add(1);
    }

    fn mark_disconnected(&self, err: anyhow::Error) {
        let mut status = self.inner.status.lock().expect("status lock poisoned");
        status.connected = false;
        status.last_error = Some(err.to_string());
        status.last_change = Some(SystemTime::now());
        status.reconnects = status.reconnects.saturating_add(1);
    }

    fn ensure_connected(&self) -> Result<()> {
        if self.is_connected() {
            return Ok(());
        }
        anyhow::bail!("gateway transport unavailable: backend is not connected")
    }

    fn list(&self, path: &str) -> Result<Vec<String>> {
        if is_cacheable_list_path(path) {
            return self.read_through_cache(path, || self.list_uncached(path));
        }
        self.list_uncached(path)
    }

    fn list_uncached(&self, path: &str) -> Result<Vec<String>> {
        self.ensure_connected()?;
        match self.submit_broker(
            PoolKind::Telemetry,
            BrokerRequest::List {
                path: path.to_owned(),
            },
        )? {
            BrokerResponse::Lines(lines) => Ok(lines),
            BrokerResponse::Unit => Ok(Vec::new()),
        }
    }

    fn read(&self, path: &str) -> Result<Vec<String>> {
        if is_cacheable_read_path(path) {
            return self.read_through_cache(path, || self.read_uncached(path));
        }
        self.read_uncached(path)
    }

    fn read_uncached(&self, path: &str) -> Result<Vec<String>> {
        self.ensure_connected()?;
        match self.submit_broker(
            PoolKind::Telemetry,
            BrokerRequest::Read {
                path: path.to_owned(),
            },
        )? {
            BrokerResponse::Lines(lines) => Ok(lines),
            BrokerResponse::Unit => Ok(Vec::new()),
        }
    }

    fn tail(&self, path: &str, lines: Option<u16>) -> Result<Vec<String>> {
        self.ensure_connected()?;
        match self.submit_broker(
            PoolKind::Telemetry,
            BrokerRequest::Tail {
                path: path.to_owned(),
                lines,
            },
        )? {
            BrokerResponse::Lines(lines) => Ok(lines),
            BrokerResponse::Unit => Ok(Vec::new()),
        }
    }

    fn write(&self, path: &str, payload: &[u8]) -> Result<Vec<String>> {
        self.ensure_connected()?;
        let write_path = path.to_owned();
        let backpressure_key = control_write_backpressure_key(write_path.as_str(), payload);
        let grant_generation = if backpressure_key.is_some() {
            self.control_write_backpressure_grant_generation()
        } else {
            0
        };
        if let Some(key) = backpressure_key {
            if let Some(error) = self.control_write_backpressure_refusal(key) {
                self.inner
                    .broker
                    .control_write_retryable_errors
                    .fetch_add(1, Ordering::Relaxed);
                self.inner
                    .broker
                    .control_write_retry_exhaustions
                    .fetch_add(1, Ordering::Relaxed);
                return Err(anyhow::anyhow!(error));
            }
        }
        let payload = payload.to_vec();
        let retry_window = Duration::from_millis(self.inner.control_write_retry_window_ms);
        let deadline = Instant::now() + retry_window;
        let retry_deadline_enabled = is_retryable_control_write_path(write_path.as_str());
        let write_kind = write_pool_kind(write_path.as_str());
        let mut first_retryable_error: Option<String> = None;
        let mut retry_delay = Duration::from_millis(CONTROL_WRITE_RETRY_SLEEP_MS);
        loop {
            let result = self.submit_broker(
                write_kind,
                BrokerRequest::Write {
                    path: write_path.clone(),
                    payload: payload.clone(),
                },
            );
            match result {
                Ok(response) => {
                    if first_retryable_error.is_some() {
                        self.inner
                            .broker
                            .control_write_success_after_retry
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    if let Some(key) = backpressure_key {
                        self.control_write_backpressure_clear(key);
                    }
                    self.read_cache_invalidate_for_write(write_path.as_str(), payload.as_slice());
                    return match response {
                        BrokerResponse::Lines(lines) => Ok(lines),
                        BrokerResponse::Unit => Ok(Vec::new()),
                    };
                }
                Err(err) => {
                    if !retry_deadline_enabled {
                        return Err(err);
                    }
                    let message = err.to_string();
                    if !is_retryable_control_write_error(message.as_str()) {
                        if first_retryable_error.is_some() {
                            return Err(anyhow::anyhow!(final_control_write_error(
                                &first_retryable_error,
                                message.as_str()
                            )));
                        }
                        return Err(err);
                    }
                    self.inner
                        .broker
                        .control_write_retryable_errors
                        .fetch_add(1, Ordering::Relaxed);
                    if first_retryable_error.is_none() {
                        first_retryable_error = Some(message.clone());
                    }
                    if is_vm_control_queue_full_error(message.as_str()) {
                        if let Some(key) = backpressure_key {
                            self.control_write_backpressure_record(
                                key,
                                grant_generation,
                                message.as_str(),
                            );
                            self.inner
                                .broker
                                .control_write_retry_exhaustions
                                .fetch_add(1, Ordering::Relaxed);
                            return Err(anyhow::anyhow!(message));
                        }
                    }
                    let now = Instant::now();
                    if retry_window.is_zero() || now >= deadline {
                        self.inner
                            .broker
                            .control_write_retry_exhaustions
                            .fetch_add(1, Ordering::Relaxed);
                        return Err(anyhow::anyhow!(final_control_write_error(
                            &first_retryable_error,
                            message.as_str()
                        )));
                    }
                    let remaining = deadline.saturating_duration_since(now);
                    let bounded_delay = retry_delay.min(remaining);
                    self.inner
                        .broker
                        .control_write_retries
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner
                        .broker
                        .control_write_retry_sleep_ms
                        .fetch_add(duration_millis_u64(bounded_delay), Ordering::Relaxed);
                    thread::sleep(bounded_delay);
                    retry_delay = next_delay(
                        retry_delay,
                        Duration::from_millis(CONTROL_WRITE_RETRY_MAX_SLEEP_MS),
                    );
                }
            }
        }
    }

    fn bounds(&self) -> BoundsResponse {
        self.inner.bounds.clone()
    }

    fn status(&self) -> GatewayStatusResponse {
        let status = self
            .inner
            .status
            .lock()
            .expect("status lock poisoned")
            .clone();
        let last_change_unix_ms = status.last_change.and_then(|value| {
            value
                .duration_since(SystemTime::UNIX_EPOCH)
                .ok()
                .map(|duration| duration.as_millis())
        });
        GatewayStatusResponse {
            connected: status.connected,
            backend_class: Some(self.inner.backend_class),
            worker_acceptance: self.inner.worker_acceptance.summary.clone(),
            worker_acceptance_diagnostic: self.inner.worker_acceptance.diagnostic.clone(),
            last_error: status.last_error,
            last_change_unix_ms,
            reconnects: status.reconnects,
            connects: status.connects,
            broker: self.inner.broker.snapshot(),
        }
    }

    fn request_auth_token(&self) -> &str {
        self.inner.request_auth_token.as_str()
    }

    fn read_through_cache<F>(&self, path: &str, fetch: F) -> Result<Vec<String>>
    where
        F: FnOnce() -> Result<Vec<String>>,
    {
        match self.read_cache_claim(path) {
            ProcReadCacheClaim::Hit(lines) => Ok(lines.as_ref().to_vec()),
            ProcReadCacheClaim::Follower(fill) => {
                let lines = fill.wait()?;
                Ok(lines.as_ref().to_vec())
            }
            ProcReadCacheClaim::Bypass => fetch(),
            ProcReadCacheClaim::Leader(fill) => {
                let leader = ProcReadFillLeader::new(self, path, fill);
                match fetch() {
                    Ok(lines) => {
                        let shared: SharedLines = lines.into();
                        leader.complete(Ok(Arc::clone(&shared)));
                        Ok(shared.as_ref().to_vec())
                    }
                    Err(err) => {
                        leader.complete(Err(err.to_string()));
                        Err(err)
                    }
                }
            }
        }
    }

    fn read_cache_claim(&self, path: &str) -> ProcReadCacheClaim {
        let mut cache = self.proc_cache_guard();
        if let Some(lines) = read_cache_valid_entry(&mut cache, path) {
            self.inner
                .broker
                .proc_cache_hits
                .fetch_add(1, Ordering::Relaxed);
            return ProcReadCacheClaim::Hit(lines);
        }
        self.inner
            .broker
            .proc_cache_misses
            .fetch_add(1, Ordering::Relaxed);
        if let Some(fill) = cache.in_flight.get(path) {
            return ProcReadCacheClaim::Follower(fill.clone());
        }
        if cache.in_flight.len() >= DEFAULT_PROC_CACHE_MAX_ENTRIES {
            return ProcReadCacheClaim::Bypass;
        }
        let fill = Arc::new(ProcReadFill::default());
        cache.in_flight.insert(path.to_owned(), fill.clone());
        ProcReadCacheClaim::Leader(fill)
    }

    fn read_cache_try_get(&self, path: &str) -> Option<Vec<String>> {
        let mut cache = match self.inner.proc_cache.try_lock() {
            Ok(cache) => cache,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        let lines = read_cache_valid_entry(&mut cache, path)?;
        self.inner
            .broker
            .proc_cache_hits
            .fetch_add(1, Ordering::Relaxed);
        drop(cache);
        Some(lines.as_ref().to_vec())
    }

    #[cfg(test)]
    fn read_cache_get(&self, path: &str) -> Option<Vec<String>> {
        let mut cache = self.proc_cache_guard();
        let lines = read_cache_valid_entry(&mut cache, path)?;
        self.inner
            .broker
            .proc_cache_hits
            .fetch_add(1, Ordering::Relaxed);
        drop(cache);
        Some(lines.as_ref().to_vec())
    }

    #[cfg(test)]
    fn read_cache_insert(&self, path: &str, lines: Vec<String>) {
        let mut cache = self.proc_cache_guard();
        self.read_cache_insert_locked(&mut cache, path, lines.into());
    }

    fn read_cache_insert_locked(&self, cache: &mut ProcReadCache, path: &str, lines: SharedLines) {
        if !cache.entries.contains_key(path)
            && cache.entries.len() >= DEFAULT_PROC_CACHE_MAX_ENTRIES
        {
            if let Some(evicted) = cache.order.pop_front() {
                cache.entries.remove(evicted.as_str());
                self.inner
                    .broker
                    .proc_cache_evictions
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        cache.order.retain(|value| value != path);
        cache.order.push_back(path.to_owned());
        cache.entries.insert(
            path.to_owned(),
            ProcReadCacheEntry {
                inserted_at: Instant::now(),
                lines,
            },
        );
    }

    fn read_cache_finish(
        &self,
        path: &str,
        fill: &Arc<ProcReadFill>,
        result: std::result::Result<SharedLines, String>,
    ) {
        let mut cache = self.proc_cache_guard();
        let fill_is_current = cache
            .in_flight
            .get(path)
            .is_some_and(|current| Arc::ptr_eq(current, fill));
        if fill_is_current {
            cache.in_flight.remove(path);
            if let Ok(lines) = &result {
                self.read_cache_insert_locked(&mut cache, path, Arc::clone(lines));
            }
        }
        drop(cache);
        fill.publish(result);
    }

    fn read_cache_invalidate_for_write(&self, write_path: &str, payload: &[u8]) {
        let mut cache = self.proc_cache_guard();
        let Some(namespaces) = cache_invalidation_namespaces(write_path, payload) else {
            cache.entries.clear();
            cache.order.clear();
            cache.in_flight.clear();
            return;
        };
        cache
            .entries
            .retain(|key, _| !namespaces.iter().any(|ns| cache_key_in_namespace(key, ns)));
        cache
            .in_flight
            .retain(|key, _| !namespaces.iter().any(|ns| cache_key_in_namespace(key, ns)));
        let ProcReadCache { entries, order, .. } = &mut *cache;
        order.retain(|key| entries.contains_key(key.as_str()));
    }

    fn proc_cache_guard(&self) -> MutexGuard<'_, ProcReadCache> {
        match self.inner.proc_cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn control_write_backpressure_refusal(
        &self,
        key: ControlWriteBackpressureKey,
    ) -> Option<String> {
        self.control_write_backpressure_guard()
            .refusal(key, Instant::now())
    }

    fn control_write_backpressure_record(
        &self,
        key: ControlWriteBackpressureKey,
        grant_generation: u64,
        message: &str,
    ) {
        let _ = self.control_write_backpressure_guard().record(
            key,
            grant_generation,
            Instant::now(),
            Duration::from_millis(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS),
            message,
        );
    }

    fn control_write_backpressure_clear(&self, key: ControlWriteBackpressureKey) {
        self.control_write_backpressure_guard()
            .clear_after_success(key);
    }

    fn control_write_backpressure_grant_generation(&self) -> u64 {
        self.control_write_backpressure_guard().grant_generation
    }

    fn control_write_backpressure_guard(&self) -> MutexGuard<'_, ControlWriteBackpressure> {
        match self.inner.control_write_backpressure.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

fn is_pool_exhausted(err: &anyhow::Error) -> bool {
    err.to_string().contains("session pool exhausted")
}

async fn meta_bounds(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    Json(state.bounds())
}

async fn meta_status(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    Json(state.status())
}

async fn openapi_yaml() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/yaml")],
        OPENAPI_YAML,
    )
}

async fn swagger_ui() -> impl axum::response::IntoResponse {
    Html(SWAGGER_UI_HTML)
}

async fn fs_ls(
    State(state): State<AppState>,
    Query(query): Query<PathQuery>,
) -> impl axum::response::IntoResponse {
    handle_list(state, query.path).await
}

async fn fs_cat(
    State(state): State<AppState>,
    Query(query): Query<CatQuery>,
) -> impl axum::response::IntoResponse {
    handle_cat(state, query).await
}

async fn fs_tail(
    State(state): State<AppState>,
    Query(query): Query<TailQuery>,
) -> impl axum::response::IntoResponse {
    handle_tail(state, query).await
}

async fn fs_echo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<EchoRequest>,
) -> axum::response::Response {
    if let Err(err) = validate_request_auth(&headers, state.request_auth_token()) {
        return response_err("ECHO", payload.path.as_str(), err, StatusCode::UNAUTHORIZED)
            .into_response();
    }
    handle_echo(state, payload).await.into_response()
}

async fn handle_list(state: AppState, path: String) -> impl axum::response::IntoResponse {
    let verb = "LS";
    if let Err(err) = validate_path(&path) {
        return response_err(verb, &path, err, StatusCode::BAD_REQUEST);
    }
    if is_cacheable_list_path(&path) {
        if let Some(lines) = state.read_cache_try_get(&path) {
            return response_ok(verb, path, lines, None);
        }
    }
    let path_clone = path.clone();
    let result = tokio::task::spawn_blocking(move || state.list(&path_clone)).await;
    match result {
        Ok(Ok(lines)) => response_ok(verb, path, lines, None),
        Ok(Err(err)) => response_transport_err(verb, &path, err),
        Err(err) => response_err(
            verb,
            &path,
            err.to_string(),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

async fn handle_cat(state: AppState, query: CatQuery) -> impl axum::response::IntoResponse {
    let verb = "CAT";
    if let Err(err) = validate_path(&query.path) {
        return response_err(verb, &query.path, err, StatusCode::BAD_REQUEST);
    }
    if let Err(err) = validate_proc_enabled(&query.path, &state.bounds()) {
        return response_err(verb, &query.path, err, StatusCode::BAD_REQUEST);
    }
    let max_bytes = match query.max_bytes {
        Some(value) if value > 0 => value as usize,
        _ => {
            return response_err(
                verb,
                &query.path,
                "max_bytes is required",
                StatusCode::BAD_REQUEST,
            )
        }
    };
    if let Some(limit) = max_proc_bytes(&query.path, &state.bounds()) {
        if max_bytes > limit {
            return response_err(
                verb,
                &query.path,
                format!("max_bytes {max_bytes} exceeds bound {limit}"),
                StatusCode::BAD_REQUEST,
            );
        }
    }
    if is_cacheable_read_path(&query.path) {
        if let Some(lines) = state.read_cache_try_get(&query.path) {
            return response_cat_lines(query.path, lines, max_bytes);
        }
    }
    let path = query.path.clone();
    let result = tokio::task::spawn_blocking(move || state.read(&path)).await;
    match result {
        Ok(Ok(lines)) => response_cat_lines(query.path, lines, max_bytes),
        Ok(Err(err)) => response_transport_err(verb, &query.path, err),
        Err(err) => response_err(
            verb,
            &query.path,
            err.to_string(),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

fn response_cat_lines(
    path: String,
    lines: Vec<String>,
    max_bytes: usize,
) -> (StatusCode, Json<GatewayResponse>) {
    let Some(bytes) = bounded_joined_line_bytes(&lines, max_bytes) else {
        return response_err(
            "CAT",
            &path,
            format!("read exceeded max_bytes {max_bytes}"),
            StatusCode::BAD_REQUEST,
        );
    };
    response_ok("CAT", path, lines, Some(bytes))
}

async fn handle_tail(state: AppState, query: TailQuery) -> impl axum::response::IntoResponse {
    let verb = "TAIL";
    if let Err(err) = validate_path(&query.path) {
        return response_err(verb, &query.path, err, StatusCode::BAD_REQUEST);
    }
    if let Err(err) = validate_proc_enabled(&query.path, &state.bounds()) {
        return response_err(verb, &query.path, err, StatusCode::BAD_REQUEST);
    }
    let max_bytes = match query.max_bytes {
        Some(value) if value > 0 => value as usize,
        _ => {
            return response_err(
                verb,
                &query.path,
                "max_bytes is required",
                StatusCode::BAD_REQUEST,
            )
        }
    };
    if let Some(limit) = max_proc_bytes(&query.path, &state.bounds()) {
        if max_bytes > limit {
            return response_err(
                verb,
                &query.path,
                format!("max_bytes {max_bytes} exceeds bound {limit}"),
                StatusCode::BAD_REQUEST,
            );
        }
    }
    let lines = match validate_tail_lines(query.lines) {
        Ok(lines) => lines,
        Err(err) => return response_err(verb, &query.path, err, StatusCode::BAD_REQUEST),
    };
    let path = query.path.clone();
    let result = tokio::task::spawn_blocking(move || state.tail(&path, lines)).await;
    match result {
        Ok(Ok(lines)) => {
            let Some(bytes) = bounded_joined_line_bytes(&lines, max_bytes) else {
                return response_err(
                    verb,
                    &query.path,
                    format!("tail exceeded max_bytes {max_bytes}"),
                    StatusCode::BAD_REQUEST,
                );
            };
            response_ok(verb, query.path, lines, Some(bytes))
        }
        Ok(Err(err)) => response_transport_err(verb, &query.path, err),
        Err(err) => response_err(
            verb,
            &query.path,
            err.to_string(),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

fn validate_tail_lines(lines: Option<u16>) -> Result<Option<u16>, String> {
    match lines {
        Some(0) => Err("lines must be >= 1".to_owned()),
        Some(count) if count > MAX_TAIL_LINES => Err(format!(
            "lines {count} exceeds max_tail_lines {MAX_TAIL_LINES}"
        )),
        _ => Ok(lines),
    }
}

fn bounded_joined_line_bytes(lines: &[String], max_bytes: usize) -> Option<usize> {
    let mut bytes = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            bytes = bytes.checked_add(1)?;
        }
        bytes = bytes.checked_add(line.len())?;
        if bytes > max_bytes {
            return None;
        }
    }
    Some(bytes)
}

async fn handle_echo(state: AppState, payload: EchoRequest) -> impl axum::response::IntoResponse {
    let verb = "ECHO";
    if let Err(err) = validate_path(&payload.path) {
        return response_err(verb, &payload.path, err, StatusCode::BAD_REQUEST);
    }
    if let Err(err) = validate_control_enabled(&payload.path, &state.bounds()) {
        return response_err(verb, &payload.path, err, StatusCode::BAD_REQUEST);
    }
    let raw_line = payload.line.unwrap_or_default();
    let raw_len = raw_line.len();
    let trimmed = match normalise_payload(&raw_line, &payload.path) {
        Ok(value) => value,
        Err(err) => return response_err(verb, &payload.path, err, StatusCode::BAD_REQUEST),
    };
    if let Some(limit) = max_ctl_bytes(&payload.path, &state.bounds()) {
        if trimmed.len() > limit {
            return response_err(
                verb,
                &payload.path,
                format!("payload exceeds ctl_max_bytes {limit}"),
                StatusCode::BAD_REQUEST,
            );
        }
    }
    let path = payload.path.clone();
    let payload_bytes = trimmed.as_bytes().to_vec();
    let result = tokio::task::spawn_blocking(move || state.write(&path, &payload_bytes)).await;
    match result {
        Ok(Ok(lines)) => response_ok(verb, payload.path, lines, Some(raw_len)),
        Ok(Err(err)) => response_transport_err(verb, &payload.path, err),
        Err(err) => response_err(
            verb,
            &payload.path,
            err.to_string(),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

fn is_cacheable_read_path(path: &str) -> bool {
    path.starts_with("/proc/") || path.starts_with("/host/") || path.starts_with("/gpu/")
}

fn is_cacheable_list_path(path: &str) -> bool {
    matches!(
        path,
        "/" | "/proc" | "/queen" | "/shard" | "/worker" | "/gpu" | "/host"
    ) || path.starts_with("/shard/")
}

fn cache_key_in_namespace(key: &str, namespace: &str) -> bool {
    key == namespace
        || key
            .strip_prefix(namespace)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn cache_invalidation_namespaces(
    write_path: &str,
    payload: &[u8],
) -> Option<&'static [&'static str]> {
    if write_path == CLIENT_QUEEN_SCHEDULE_CTL_PATH {
        return Some(CACHE_INVALIDATE_SCHEDULE_NAMESPACES);
    }
    if write_path == CLIENT_QUEEN_LEASE_CTL_PATH {
        return Some(match lease_control_operation_kind(payload) {
            Some(LeaseControlOperationKind::Grant) => CACHE_INVALIDATE_LEASE_GRANT_PATHS,
            Some(LeaseControlOperationKind::Renew) => CACHE_INVALIDATE_LEASE_RENEW_PATHS,
            Some(LeaseControlOperationKind::Preempt) => CACHE_INVALIDATE_LEASE_PREEMPT_PATHS,
            Some(LeaseControlOperationKind::Quota) => CACHE_INVALIDATE_LEASE_QUOTA_PATHS,
            None => CACHE_INVALIDATE_LEASE_NAMESPACES,
        });
    }
    if write_path.starts_with("/queen/telemetry/") {
        return Some(CACHE_INVALIDATE_TELEMETRY_NAMESPACES);
    }
    if write_path == CLIENT_POLICY_CTL_PATH {
        return Some(CACHE_INVALIDATE_POLICY_NAMESPACES);
    }
    if write_path.starts_with("/queen/") || write_path.starts_with("/actions/") {
        return Some(CACHE_INVALIDATE_CONTROL_NAMESPACES);
    }
    if write_path.starts_with("/host/") {
        return Some(CACHE_INVALIDATE_HOST_NAMESPACES);
    }
    if write_path.starts_with("/gpu/") {
        return Some(CACHE_INVALIDATE_GPU_NAMESPACES);
    }
    None
}

fn is_retryable_control_write_path(path: &str) -> bool {
    path.starts_with("/queen/") || path == CLIENT_POLICY_CTL_PATH || path.starts_with("/actions/")
}

fn write_pool_kind(path: &str) -> PoolKind {
    if is_batchable_telemetry_write_path(path) {
        PoolKind::Telemetry
    } else {
        PoolKind::Control
    }
}

fn is_batchable_telemetry_write_path(path: &str) -> bool {
    path.starts_with("/queen/telemetry/") && path.contains("/seg/")
}

fn is_telemetry_control_path(path: &str) -> bool {
    let mut segments = path.split('/');
    matches!(
        (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
        ),
        (Some(""), Some("queen"), Some("telemetry"), Some(device_id), Some("ctl"), None)
            if !device_id.is_empty()
    )
}

fn valid_telemetry_segment_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn telemetry_segment_id_from_ack_lines(path: &str, lines: &[String]) -> Option<String> {
    if !is_telemetry_control_path(path) {
        return None;
    }
    for line in lines {
        let Some(ack) = parse_ack(line) else {
            continue;
        };
        if ack.status != AckStatus::Ok || ack.verb != "ECHO" {
            continue;
        }
        let Some(detail) = ack.detail else {
            continue;
        };
        let mut ack_path = None;
        let mut segment_id = None;
        for token in detail.split_whitespace() {
            if let Some(value) = token.strip_prefix("path=") {
                if ack_path.replace(value).is_some() {
                    return None;
                }
            }
            if let Some(value) = token.strip_prefix("seg_id=") {
                if segment_id.replace(value).is_some() {
                    return None;
                }
            }
        }
        if ack_path == Some(path) {
            if let Some(value) = segment_id.filter(|value| valid_telemetry_segment_id(value)) {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn control_write_backpressure_key(
    path: &str,
    payload: &[u8],
) -> Option<ControlWriteBackpressureKey> {
    if path == CLIENT_QUEEN_SCHEDULE_CTL_PATH {
        return Some(ControlWriteBackpressureKey::Schedule);
    }
    if path != CLIENT_QUEEN_LEASE_CTL_PATH {
        return None;
    }
    match lease_control_operation_kind(payload)? {
        LeaseControlOperationKind::Grant => Some(ControlWriteBackpressureKey::LeaseGrant),
        LeaseControlOperationKind::Preempt => Some(ControlWriteBackpressureKey::LeasePreempt),
        LeaseControlOperationKind::Renew | LeaseControlOperationKind::Quota => None,
    }
}

fn lease_control_operation_kind(payload: &[u8]) -> Option<LeaseControlOperationKind> {
    match serde_json::from_slice::<LeaseControlOperation>(payload) {
        Ok(operation) => Some(operation.op),
        // This parser only classifies cache/cooldown scope. The target remains
        // authoritative for full validation; unknown input gets no cached
        // refusal and uses conservative whole-lease cache invalidation.
        Err(_classification_error) => None,
    }
}

fn is_retryable_control_write_error(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    lowered.contains("buffer-full")
        || lowered.contains("buffer full")
        || lowered.contains("session pool exhausted")
        || lowered.contains("gateway backpressure")
}

fn is_vm_control_queue_full_error(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    lowered.contains("buffer-full") || lowered.contains("buffer full")
}

fn final_control_write_error(
    first_retryable_error: &Option<String>,
    current_error: &str,
) -> String {
    first_retryable_error
        .as_deref()
        .unwrap_or(current_error)
        .to_owned()
}

fn extract_request_auth(headers: &HeaderMap) -> Option<String> {
    if let Some(header) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(value) = header.to_str() {
            let lowered = value.to_ascii_lowercase();
            if lowered.starts_with(AUTHORIZATION_BEARER_PREFIX) {
                let token = value[AUTHORIZATION_BEARER_PREFIX.len()..].trim();
                if !token.is_empty() {
                    return Some(token.to_owned());
                }
            }
        }
    }
    if let Some(header) = headers.get(REQUEST_AUTH_HEADER) {
        if let Ok(value) = header.to_str() {
            let token = value.trim();
            if !token.is_empty() {
                return Some(token.to_owned());
            }
        }
    }
    None
}

fn validate_request_auth(headers: &HeaderMap, expected: &str) -> Result<(), String> {
    let Some(observed) = extract_request_auth(headers) else {
        return Err(format!(
            "missing request auth token; provide Authorization: Bearer <token> or {REQUEST_AUTH_HEADER}"
        ));
    };
    if observed == expected {
        return Ok(());
    }
    Err("invalid request auth token".to_owned())
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path is required".to_owned());
    }
    if !path.starts_with('/') {
        return Err("path must be absolute".to_owned());
    }
    if path.len() > MAX_PATH_LEN {
        return Err(format!("path exceeds max length {MAX_PATH_LEN}"));
    }
    if path.as_bytes().contains(&0) {
        return Err("path contains NUL byte".to_owned());
    }
    let mut depth = 0usize;
    for segment in path.split('/').skip(1) {
        if segment.is_empty() {
            continue;
        }
        if segment == "." || segment == ".." {
            return Err("path segments '.' and '..' are not permitted".to_owned());
        }
        depth = depth.saturating_add(1);
    }
    if depth > SECURE9P_WALK_DEPTH as usize {
        return Err(format!("path exceeds max depth {}", SECURE9P_WALK_DEPTH));
    }
    Ok(())
}

fn normalise_payload(raw: &str, path: &str) -> Result<String, String> {
    let trimmed = raw.trim_end_matches('\n');
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("payload must be a single line".to_owned());
    }
    let len = trimmed.len();
    if len > MAX_ECHO_LEN {
        return Err(format!("payload exceeds max_echo_len {MAX_ECHO_LEN}"));
    }
    let overhead = "ECHO ".len() + path.len();
    let total = if trimmed.is_empty() {
        overhead
    } else {
        overhead + 1 + len
    };
    if total > MAX_LINE_LEN {
        return Err(format!("echo line exceeds max_line_len {MAX_LINE_LEN}"));
    }
    Ok(trimmed.to_owned())
}

fn max_ctl_bytes(path: &str, bounds: &BoundsResponse) -> Option<usize> {
    if path == CLIENT_QUEEN_SCHEDULE_CTL_PATH {
        return Some(bounds.control_plane.schedule.ctl_max_bytes as usize);
    }
    if path == CLIENT_QUEEN_LEASE_CTL_PATH {
        return Some(bounds.control_plane.lease.ctl_max_bytes as usize);
    }
    if path == CLIENT_QUEEN_EXPORT_CTL_PATH {
        return Some(bounds.control_plane.export.ctl_max_bytes as usize);
    }
    if path == CLIENT_POLICY_CTL_PATH {
        return Some(bounds.policy.ctl_max_bytes as usize);
    }
    None
}

fn max_proc_bytes(path: &str, bounds: &BoundsResponse) -> Option<usize> {
    match path {
        PROC_SCHEDULE_SUMMARY_PATH => {
            Some(bounds.observability.proc_schedule.summary_bytes as usize)
        }
        PROC_SCHEDULE_QUEUE_PATH => Some(bounds.observability.proc_schedule.queue_bytes as usize),
        PROC_LEASE_SUMMARY_PATH => Some(bounds.observability.proc_lease.summary_bytes as usize),
        PROC_LEASE_ACTIVE_PATH => Some(bounds.observability.proc_lease.active_bytes as usize),
        PROC_LEASE_PREEMPTIONS_PATH => {
            Some(bounds.observability.proc_lease.preemptions_bytes as usize)
        }
        _ => None,
    }
}

fn validate_proc_enabled(path: &str, bounds: &BoundsResponse) -> Result<(), String> {
    if path == PROC_SCHEDULE_SUMMARY_PATH && !bounds.observability.proc_schedule.summary {
        return Err("proc schedule summary is disabled".to_owned());
    }
    if path == PROC_SCHEDULE_QUEUE_PATH && !bounds.observability.proc_schedule.queue {
        return Err("proc schedule queue is disabled".to_owned());
    }
    if path == PROC_LEASE_SUMMARY_PATH && !bounds.observability.proc_lease.summary {
        return Err("proc lease summary is disabled".to_owned());
    }
    if path == PROC_LEASE_ACTIVE_PATH && !bounds.observability.proc_lease.active {
        return Err("proc lease active is disabled".to_owned());
    }
    if path == PROC_LEASE_PREEMPTIONS_PATH && !bounds.observability.proc_lease.preemptions {
        return Err("proc lease preemptions is disabled".to_owned());
    }
    Ok(())
}

fn validate_control_enabled(path: &str, bounds: &BoundsResponse) -> Result<(), String> {
    if path == CLIENT_QUEEN_SCHEDULE_CTL_PATH && !bounds.control_plane.schedule.enable {
        return Err("schedule control is disabled".to_owned());
    }
    if path == CLIENT_QUEEN_LEASE_CTL_PATH && !bounds.control_plane.lease.enable {
        return Err("lease control is disabled".to_owned());
    }
    if path == CLIENT_QUEEN_EXPORT_CTL_PATH && !bounds.control_plane.export.enable {
        return Err("export control is disabled".to_owned());
    }
    if path == CLIENT_POLICY_CTL_PATH && !bounds.policy.enable {
        return Err("policy control is disabled".to_owned());
    }
    Ok(())
}

fn response_ok(
    verb: &'static str,
    path: String,
    lines: Vec<String>,
    bytes: Option<usize>,
) -> (StatusCode, Json<GatewayResponse>) {
    (
        StatusCode::OK,
        Json(GatewayResponse {
            status: "OK",
            verb,
            path,
            end: true,
            lines,
            bytes,
            error: None,
        }),
    )
}

fn response_err(
    verb: &'static str,
    path: &str,
    error: impl ToString,
    status: StatusCode,
) -> (StatusCode, Json<GatewayResponse>) {
    let message = error.to_string();
    if status.is_server_error() {
        error!("hive-gateway error: {}", message);
    } else {
        warn!("hive-gateway request rejected: {}", message);
    }
    (
        status,
        Json(GatewayResponse {
            status: "ERR",
            verb,
            path: path.to_owned(),
            end: true,
            lines: Vec::new(),
            bytes: None,
            error: Some(message),
        }),
    )
}

fn extract_ack_error(message: &str) -> Option<&str> {
    let offset = message.find("ERR ")?;
    Some(message[offset..].trim())
}

fn response_transport_err(
    verb: &'static str,
    path: &str,
    err: anyhow::Error,
) -> (StatusCode, Json<GatewayResponse>) {
    let message = err.to_string();
    if message.contains("gateway backpressure") {
        return response_err(verb, path, message, StatusCode::TOO_MANY_REQUESTS);
    }
    if message.contains("gateway timeout") {
        return response_err(verb, path, message, StatusCode::GATEWAY_TIMEOUT);
    }
    if let Some(ack) = extract_ack_error(&message) {
        return response_err(verb, path, ack, StatusCode::OK);
    }
    response_err(verb, path, message, StatusCode::SERVICE_UNAVAILABLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use axum::http::header::AUTHORIZATION;
    use axum::http::{HeaderMap, HeaderValue};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Barrier;

    const COMPONENT_OUTCOMES: [&str; 25] = [
        "bounded-control-path",
        "bounded-receipt-path",
        "budget-exhaustion-attributed",
        "combined-notification",
        "driver-liveness",
        "durable-completion-order",
        "fault-before-ready",
        "fault-during-ipc",
        "forbidden-blocking-send-refused",
        "fresh-supervisor-generation",
        "gpu-grant-confirmed-rejected-stale",
        "gpu-release-confirmed-rejected-stale",
        "gpu-renew-confirmed-rejected-stale",
        "heartbeat-progress",
        "lora-activate-confirmed-rejected-stale",
        "lora-export-confirmed-rejected-stale",
        "lora-import-confirmed-rejected-stale",
        "lora-rollback-confirmed-rejected-stale",
        "maximum-slot-refused",
        "no-post-revoke-activity",
        "operator-liveness",
        "same-role-sequential-instances",
        "stale-record-revoked",
        "teardown-zero-leak",
        "timeout-attributed",
    ];

    fn valid_target_component(manifest_sha256: &str) -> (Vec<u8>, Vec<u8>) {
        let hash = "0".repeat(64);
        let target_session = serde_json::json!({
            "target": "qemu",
            "source_sha256": hash,
            "manifest_sha256": manifest_sha256,
            "kernel_sha256": "1".repeat(64),
            "root_image_sha256": "2".repeat(64),
            "driver_archive_sha256": "3".repeat(64),
            "driver_manifest_sha256": "4".repeat(64),
            "cyw43_coexistence_record_sha256": "5".repeat(64),
            "worker_archive_sha256": "6".repeat(64),
            "worker_image_manifest_sha256": "7".repeat(64),
            "worker_abi_sha256": "8".repeat(64),
        });
        let inventory = serde_json::json!({
            "tcbs": 1,
            "scheduling_contexts": 1,
            "reply_objects": 0,
            "vspaces": 1,
            "cnodes": 1,
            "page_tables": 8,
            "asids": 1,
            "frames": 16,
            "endpoints": 0,
            "notifications": 1,
            "fault_caps": 1,
            "timeout_fault_caps": 1,
            "cspace_slots": 64,
            "untyped_bytes": 1_048_576,
        });
        let roles = [
            ("worker-heartbeat", "none", 0_u16),
            ("worker-gpu", "confirmed", 1_u16),
            ("worker-lora", "confirmed", 2_u16),
        ];
        let workers = roles
            .into_iter()
            .enumerate()
            .map(|(index, (role, receipt, slot))| {
                serde_json::json!({
                    "identity": {
                        "role": role,
                        "slot": slot,
                        "lease_epoch": 1,
                        "supervisor_generation": index + 1,
                        "cap_generation": 1,
                    },
                    "state": {
                        "declaration": "executable",
                        "lifecycle": "ready",
                        "artifact": "verified",
                        "receipt": receipt,
                        "execution_proof": "qemu",
                    },
                    "image_sha256": format!("{}", index + 1).repeat(64),
                    "ready_sequence": 1,
                    "completion_sequence": 2,
                    "endpoint_badge": 1_u64 << (index + 1),
                    "fault_badge": 1_u64 << (index + 9),
                    "core": index,
                    "scheduling_context": {"budget_us": 100, "period_us": 1_000},
                    "object_inventory": inventory.clone(),
                })
            })
            .collect::<Vec<_>>();
        let evidence = serde_json::json!({
            "schema": "cohesix-worker-task-evidence/v1",
            "record_kind": "target-component",
            "target": "qemu",
            "target_session": target_session.clone(),
            "topology_sha256": "9".repeat(64),
            "workers": workers,
            "integration_evidence": [
                {"id":"gpu-receipt-path","record_kind":"worker-integration","sha256":"a".repeat(64)},
                {"id":"peft-receipt-path","record_kind":"worker-integration","sha256":"b".repeat(64)},
                {"id":"worker-control","record_kind":"worker-integration","sha256":"c".repeat(64)},
            ],
            "outcomes": COMPONENT_OUTCOMES.map(|id| serde_json::json!({
                "id": id,
                "class": "observation",
                "result": "pass",
            })),
            "raw_evidence": [{
                "id":"qemu-worker-transcript",
                "sha256":"d".repeat(64),
                "bytes":128,
            }],
            "verdict":"PASS",
            "blockers":[],
        });
        (
            serde_json::to_vec(&target_session).expect("serialize target session"),
            serde_json::to_vec(&evidence).expect("serialize component evidence"),
        )
    }

    #[test]
    fn generated_worker_bounds_are_exact_and_bounded() {
        let bounds = build_worker_runtime_bounds().expect("generated Worker profile parses");
        assert_eq!(bounds.maximum_live_tasks, 3);
        assert_eq!(bounds.task_abi_schema, "worker-task-abi/v1");
        assert_eq!(bounds.task_abi_version, 1);
        assert_eq!(
            bounds.canonical_telemetry_template,
            "/shard/<label>/worker/<id>/telemetry"
        );
        assert_eq!(
            bounds
                .roles
                .iter()
                .filter(|role| role.declaration == WorkerDeclaration::Executable)
                .map(|role| role.executable_slots)
                .sum::<u16>(),
            bounds.maximum_live_tasks
        );
        assert_eq!(
            bounds
                .roles
                .iter()
                .find(|role| role.role == "worker-bus")
                .map(|role| (&role.declaration, role.executable_slots)),
            Some((&WorkerDeclaration::ModelOnly, 0))
        );
    }

    #[test]
    fn worker_acceptance_import_requires_all_explicit_paths() {
        let manifest = CohshPolicy::manifest_hash();
        let absent = load_worker_acceptance(None, None, None, manifest);
        assert_eq!(
            absent.diagnostic.map(|diagnostic| diagnostic.code),
            Some(WorkerAcceptanceDiagnosticCode::NotConfigured)
        );
        let incomplete = load_worker_acceptance(Some(Path::new(".")), None, None, manifest);
        assert_eq!(
            incomplete.diagnostic.map(|diagnostic| diagnostic.code),
            Some(WorkerAcceptanceDiagnosticCode::IncompleteConfiguration)
        );
    }

    #[test]
    fn worker_acceptance_import_binds_current_component_and_redacts_capabilities() {
        let root = tempfile::tempdir().expect("acceptance root");
        let target_session = root.path().join("target-session.json");
        let evidence = root.path().join("worker-component.json");
        let manifest = CohshPolicy::manifest_hash();
        let (session_bytes, evidence_bytes) = valid_target_component(manifest);
        fs::write(&target_session, session_bytes).expect("write target session");
        fs::write(&evidence, evidence_bytes).expect("write component evidence");
        let imported = load_worker_acceptance(
            Some(root.path()),
            Some(&evidence),
            Some(&target_session),
            manifest,
        );
        assert!(imported.diagnostic.is_none());
        let summary = imported.summary.expect("validated summary");
        assert_eq!(summary.record_kind, "target-component");
        assert_eq!(summary.verdict, "PASS");
        assert_eq!(summary.target, Some("qemu"));
        assert_eq!(summary.execution_proof, "qemu");
        assert_eq!(summary.evidence_sha256.len(), 64);
        assert_eq!(summary.target_session.manifest_sha256, manifest);
        assert_eq!(summary.workers.len(), 3);
        let serialized = serde_json::to_string(&summary).expect("serialize summary");
        assert!(!serialized.contains(root.path().to_string_lossy().as_ref()));
        assert!(!serialized.contains("endpoint_badge"));
        assert!(!serialized.contains("fault_badge"));
    }

    #[test]
    fn worker_acceptance_import_rejects_outside_and_oversized_files() {
        let root = tempfile::tempdir().expect("acceptance root");
        let target_session = root.path().join("target-session.json");
        let (session_bytes, _) = valid_target_component(CohshPolicy::manifest_hash());
        fs::write(&target_session, session_bytes).expect("write target session");
        let outside = tempfile::NamedTempFile::new().expect("outside evidence");
        let imported = load_worker_acceptance(
            Some(root.path()),
            Some(outside.path()),
            Some(&target_session),
            CohshPolicy::manifest_hash(),
        );
        assert_eq!(
            imported.diagnostic.map(|diagnostic| diagnostic.code),
            Some(WorkerAcceptanceDiagnosticCode::OutsideRoot)
        );

        let oversized = root.path().join("oversized.json");
        fs::write(
            &oversized,
            vec![b'x'; MAX_WORKER_ACCEPTANCE_EVIDENCE_BYTES as usize + 1],
        )
        .expect("write oversized evidence");
        let imported = load_worker_acceptance(
            Some(root.path()),
            Some(&oversized),
            Some(&target_session),
            CohshPolicy::manifest_hash(),
        );
        assert_eq!(
            imported.diagnostic.map(|diagnostic| diagnostic.code),
            Some(WorkerAcceptanceDiagnosticCode::RecordTooLarge)
        );
    }

    #[test]
    fn worker_acceptance_import_rejects_manifest_and_target_session_drift() {
        let root = tempfile::tempdir().expect("acceptance root");
        let target_session = root.path().join("target-session.json");
        let evidence = root.path().join("worker-component.json");
        let manifest = CohshPolicy::manifest_hash();
        let (session_bytes, evidence_bytes) = valid_target_component(manifest);
        fs::write(&target_session, &session_bytes).expect("write target session");
        fs::write(&evidence, evidence_bytes).expect("write component evidence");

        let wrong_manifest = load_worker_acceptance(
            Some(root.path()),
            Some(&evidence),
            Some(&target_session),
            "f000000000000000000000000000000000000000000000000000000000000000",
        );
        assert_eq!(
            wrong_manifest.diagnostic.map(|diagnostic| diagnostic.code),
            Some(WorkerAcceptanceDiagnosticCode::ManifestMismatch)
        );

        let mut changed_session: serde_json::Value =
            serde_json::from_slice(&session_bytes).expect("parse target session");
        changed_session["root_image_sha256"] = serde_json::Value::String("e".repeat(64));
        fs::write(
            &target_session,
            serde_json::to_vec(&changed_session).expect("serialize changed target session"),
        )
        .expect("replace target session");
        let mismatch = load_worker_acceptance(
            Some(root.path()),
            Some(&evidence),
            Some(&target_session),
            manifest,
        );
        assert_eq!(
            mismatch.diagnostic.map(|diagnostic| diagnostic.code),
            Some(WorkerAcceptanceDiagnosticCode::TargetSessionMismatch)
        );
    }

    #[cfg(unix)]
    #[test]
    fn worker_acceptance_import_rejects_symlink_traversal() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("acceptance root");
        let real = root.path().join("real.json");
        let link = root.path().join("linked.json");
        let target_session = root.path().join("target-session.json");
        let (session_bytes, evidence_bytes) = valid_target_component(CohshPolicy::manifest_hash());
        fs::write(&target_session, session_bytes).expect("write target session");
        fs::write(&real, evidence_bytes).expect("write component evidence");
        symlink(&real, &link).expect("create evidence symlink");
        let imported = load_worker_acceptance(
            Some(root.path()),
            Some(&link),
            Some(&target_session),
            CohshPolicy::manifest_hash(),
        );
        assert_eq!(
            imported.diagnostic.map(|diagnostic| diagnostic.code),
            Some(WorkerAcceptanceDiagnosticCode::SymlinkTraversal)
        );
    }

    fn test_write_command(
        kind: PoolKind,
        path: &str,
        payload: &[u8],
    ) -> (BrokerCommand, mpsc::Receiver<Result<BrokerResponse>>) {
        let (response_tx, response_rx) = mpsc::channel();
        (
            BrokerCommand {
                kind,
                request: BrokerRequest::Write {
                    path: path.to_owned(),
                    payload: payload.to_vec(),
                },
                response_tx,
            },
            response_rx,
        )
    }

    #[test]
    fn extract_ack_error_returns_err_line() {
        let message =
            "echo failed: ERR ECHO reason=policy detail=denied path=/queen/ctl error=EPERM";
        let ack = extract_ack_error(message).expect("ack");
        assert!(ack.starts_with("ERR ECHO"));
        assert!(ack.contains("reason=policy"));
    }

    #[test]
    fn response_transport_err_maps_ack_to_ok() {
        let err = anyhow!(
            "echo failed: ERR ECHO reason=policy detail=denied path=/queen/ctl error=EPERM"
        );
        let (status, body) = response_transport_err("ECHO", "/queen/ctl", err);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.0.status, "ERR");
        assert!(body
            .0
            .error
            .as_ref()
            .expect("error")
            .contains("reason=policy"));
    }

    #[test]
    fn in_process_schedule_capacity_surfaces_one_typed_buffer_full_refusal() {
        let server = NineDoor::new_with_shard_layout(ShardLayout::enabled(8, true));
        let factory: Arc<dyn TransportFactory> = Arc::new(move || {
            Ok(Box::new(NineDoorTransport::new(server.clone()))
                as Box<dyn cohsh::Transport + Send>)
        });
        let pool = SessionPool::new(3, 5, factory);
        execute_broker_request(
            &pool,
            PoolKind::Control,
            BrokerRequest::Attach {
                role: Role::Queen,
                ticket: None,
            },
        )
        .expect("in-process broker attach");

        for index in 0..CONTROL_SCHEDULE_QUEUE_MAX_ENTRIES {
            let payload = format!(
                "{{\"id\":\"sched-{index}\",\"role\":\"worker-heartbeat\",\"priority\":1,\"ticks\":1,\"budget_ms\":1}}"
            )
            .into_bytes();
            execute_broker_request(
                &pool,
                PoolKind::Control,
                BrokerRequest::Write {
                    path: CLIENT_QUEEN_SCHEDULE_CTL_PATH.to_owned(),
                    payload,
                },
            )
            .expect("admitted schedule entry");
        }

        let refused_payload = format!(
            "{{\"id\":\"sched-{CONTROL_SCHEDULE_QUEUE_MAX_ENTRIES}\",\"role\":\"worker-heartbeat\",\"priority\":1,\"ticks\":1,\"budget_ms\":1}}"
        )
        .into_bytes();
        let err = match execute_broker_request(
            &pool,
            PoolKind::Control,
            BrokerRequest::Write {
                path: CLIENT_QUEEN_SCHEDULE_CTL_PATH.to_owned(),
                payload: refused_payload,
            },
        ) {
            Ok(_) => panic!("first over-capacity schedule entry must be refused"),
            Err(err) => err,
        };

        assert_eq!(
            err.to_string(),
            "ERR ECHO reason=quota detail=buffer-full path=/queen/schedule/ctl error=buffer full"
        );
        assert!(is_retryable_control_write_error(err.to_string().as_str()));
        assert!(is_vm_control_queue_full_error(err.to_string().as_str()));
        let (status, body) = response_transport_err("ECHO", CLIENT_QUEEN_SCHEDULE_CTL_PATH, err);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.0.status, "ERR");
        assert_eq!(
            body.0.error.as_deref(),
            Some(
                "ERR ECHO reason=quota detail=buffer-full path=/queen/schedule/ctl error=buffer full"
            )
        );
    }

    #[test]
    fn validate_tail_lines_enforces_cli_bound() {
        assert_eq!(validate_tail_lines(None).unwrap(), None);
        assert_eq!(validate_tail_lines(Some(1)).unwrap(), Some(1));
        assert_eq!(
            validate_tail_lines(Some(MAX_TAIL_LINES)).unwrap(),
            Some(MAX_TAIL_LINES)
        );
        assert!(validate_tail_lines(Some(0)).is_err());
        assert!(validate_tail_lines(Some(MAX_TAIL_LINES + 1)).is_err());
    }

    #[test]
    fn enforce_bind_exposure_rejects_non_loopback_without_override() {
        let addr: SocketAddr = "0.0.0.0:8080".parse().expect("parse");
        let err = enforce_bind_exposure(addr, false).expect_err("must reject");
        assert!(err.to_string().contains("refusing non-loopback bind"));
    }

    #[test]
    fn normalize_required_secret_rejects_placeholder() {
        let err = normalize_required_secret(
            "request auth token",
            Some(INSECURE_PLACEHOLDER_TOKEN.to_owned()),
            false,
        )
        .expect_err("placeholder must be rejected");
        assert!(err.to_string().contains("insecure placeholder"));
    }

    #[test]
    fn extract_request_auth_reads_bearer_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str("Bearer secret-token").expect("header value"),
        );
        let token = extract_request_auth(&headers).expect("token");
        assert_eq!(token, "secret-token");
    }

    #[test]
    fn extract_request_auth_reads_alt_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            REQUEST_AUTH_HEADER,
            HeaderValue::from_str("alt-token").expect("header value"),
        );
        let token = extract_request_auth(&headers).expect("token");
        assert_eq!(token, "alt-token");
    }

    #[test]
    fn validate_request_auth_rejects_missing_token() {
        let headers = HeaderMap::new();
        let err = validate_request_auth(&headers, "expected").expect_err("missing token");
        assert!(err.contains("missing request auth token"));
    }

    #[test]
    fn validate_request_auth_accepts_matching_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str("Bearer expected").expect("header value"),
        );
        validate_request_auth(&headers, "expected").expect("auth accepted");
    }

    #[test]
    fn response_transport_err_maps_backpressure_to_too_many_requests() {
        let err = anyhow!("gateway backpressure: session pool checkout timed out for Telemetry");
        let (status, body) = response_transport_err("CAT", "/proc/root/reachable", err);
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body.0.status, "ERR");
    }

    #[test]
    fn response_transport_err_maps_broker_response_timeout_to_gateway_timeout() {
        let err =
            anyhow!("gateway timeout: broker response timed out for Telemetry after 120000ms");
        let (status, body) = response_transport_err("LS", "/", err);
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body.0.status, "ERR");
    }

    #[test]
    fn broker_timeout_defaults_cover_slow_pi_console_responses() {
        let control = broker_response_timeout_ms(
            None,
            DEFAULT_BROKER_CONTROL_RESPONSE_TIMEOUT_MS,
            "broker control response timeout",
        )
        .expect("default control timeout");
        let telemetry = broker_response_timeout_ms(
            None,
            DEFAULT_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS,
            "broker telemetry response timeout",
        )
        .expect("default telemetry timeout");
        assert!(control >= 120_000);
        assert!(telemetry >= 120_000);
        assert!(control > BROKER_QUEUE_WAIT_LIMIT_MS);
        assert!(telemetry > BROKER_QUEUE_WAIT_LIMIT_MS);
    }

    #[test]
    fn broker_response_timeout_rejects_values_below_queue_wait() {
        let err = broker_response_timeout_ms(
            Some(BROKER_QUEUE_WAIT_LIMIT_MS - 1),
            DEFAULT_BROKER_CONTROL_RESPONSE_TIMEOUT_MS,
            "broker control response timeout",
        )
        .expect_err("timeout below queue wait should fail");
        assert!(err.to_string().contains("broker queue wait limit"));
    }

    #[test]
    fn broker_timeouts_select_response_timeout_by_pool_kind() {
        let timeouts = BrokerTimeouts {
            control_response_ms: 11_000,
            telemetry_response_ms: 22_000,
        };
        assert_eq!(timeouts.response_ms(PoolKind::Control), 11_000);
        assert_eq!(timeouts.response_ms(PoolKind::Telemetry), 22_000);
    }

    #[test]
    fn cacheable_read_path_includes_proc_host_and_gpu() {
        assert!(is_cacheable_read_path("/proc/root/reachable"));
        assert!(is_cacheable_read_path("/host/systemd/ssh.service/status"));
        assert!(is_cacheable_read_path("/gpu/bridge/status"));
        assert!(!is_cacheable_read_path("/queen/ctl"));
    }

    #[test]
    fn cacheable_list_path_covers_hot_roots() {
        for path in [
            "/",
            "/proc",
            "/queen",
            "/shard",
            "/shard/0a/worker",
            "/worker",
            "/gpu",
            "/host",
        ] {
            assert!(
                is_cacheable_list_path(path),
                "expected cacheable path: {path}"
            );
        }
        assert!(!is_cacheable_list_path("/log"));
    }

    #[test]
    fn cache_key_in_namespace_matches_exact_or_child() {
        assert!(cache_key_in_namespace("/proc/schedule/summary", "/proc"));
        assert!(cache_key_in_namespace("/proc", "/proc"));
        assert!(!cache_key_in_namespace("/process", "/proc"));
    }

    #[test]
    fn cache_invalidation_namespaces_follow_write_scope() {
        assert_eq!(
            cache_invalidation_namespaces("/queen/schedule/ctl", br#"{}"#),
            Some(CACHE_INVALIDATE_SCHEDULE_NAMESPACES)
        );
        assert!(CACHE_INVALIDATE_CONTROL_NAMESPACES.contains(&"/shard"));
        assert_eq!(
            cache_invalidation_namespaces("/queen/lease/ctl", br#"{"op":"grant"}"#),
            Some(CACHE_INVALIDATE_LEASE_GRANT_PATHS)
        );
        assert_eq!(
            cache_invalidation_namespaces("/queen/lease/ctl", br#"{"op":"renew"}"#),
            Some(CACHE_INVALIDATE_LEASE_RENEW_PATHS)
        );
        assert_eq!(
            cache_invalidation_namespaces("/queen/lease/ctl", br#"{"op":"preempt"}"#),
            Some(CACHE_INVALIDATE_LEASE_PREEMPT_PATHS)
        );
        assert_eq!(
            cache_invalidation_namespaces("/queen/lease/ctl", br#"{"op":"quota"}"#),
            Some(CACHE_INVALIDATE_LEASE_QUOTA_PATHS)
        );
        assert_eq!(
            cache_invalidation_namespaces("/queen/lease/ctl", b"not-json"),
            Some(CACHE_INVALIDATE_LEASE_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/queen/telemetry/gpu0/segment", br#"{"new":"segment"}"#,),
            Some(CACHE_INVALIDATE_TELEMETRY_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/policy/ctl", br#"{}"#),
            Some(CACHE_INVALIDATE_POLICY_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/host/docker/status", br#"{}"#),
            Some(CACHE_INVALIDATE_HOST_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/gpu/bridge/status", br#"{}"#),
            Some(CACHE_INVALIDATE_GPU_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/log/queen.log", br#"{}"#),
            None
        );
    }

    #[test]
    fn retryable_control_write_helpers_match_expected_paths_and_errors() {
        assert!(is_retryable_control_write_path("/queen/lease/ctl"));
        assert!(is_retryable_control_write_path("/policy/ctl"));
        assert!(is_retryable_control_write_path("/actions/queue"));
        assert!(!is_retryable_control_write_path("/host/docker/status"));
        assert_eq!(
            control_write_backpressure_key(CLIENT_QUEEN_SCHEDULE_CTL_PATH, br#"{}"#),
            Some(ControlWriteBackpressureKey::Schedule)
        );
        assert_eq!(
            control_write_backpressure_key(
                CLIENT_QUEEN_LEASE_CTL_PATH,
                br#"{"op":"grant","id":"lease-1"}"#,
            ),
            Some(ControlWriteBackpressureKey::LeaseGrant)
        );
        assert_eq!(
            control_write_backpressure_key(
                CLIENT_QUEEN_LEASE_CTL_PATH,
                br#"{"op":"renew","id":"lease-1"}"#,
            ),
            None
        );
        assert_eq!(
            control_write_backpressure_key(
                CLIENT_QUEEN_LEASE_CTL_PATH,
                br#"{"op":"preempt","id":"lease-1"}"#,
            ),
            Some(ControlWriteBackpressureKey::LeasePreempt)
        );
        assert_eq!(
            control_write_backpressure_key(
                CLIENT_QUEEN_LEASE_CTL_PATH,
                br#"{"op":"quota","subject":"queen"}"#,
            ),
            None
        );
        assert_eq!(
            control_write_backpressure_key(CLIENT_QUEEN_LEASE_CTL_PATH, br#"{"op":"unknown"}"#),
            None
        );
        assert_eq!(
            control_write_backpressure_key(CLIENT_QUEEN_LEASE_CTL_PATH, b"not-json"),
            None
        );
        assert_eq!(
            control_write_backpressure_key(CLIENT_POLICY_CTL_PATH, br#"{}"#),
            None
        );

        assert!(is_retryable_control_write_error(
            "ERR ECHO reason=quota detail=buffer-full path=/queen/schedule/ctl error=buffer full"
        ));
        assert!(is_retryable_control_write_error(
            "gateway backpressure: session pool checkout timed out for Control"
        ));
        assert!(!is_retryable_control_write_error(
            "ERR ECHO reason=policy detail=invalid-payload path=/queen/lease/ctl error=invalid payload"
        ));
        assert!(is_vm_control_queue_full_error(
            "ERR ECHO reason=quota detail=buffer-full path=/queen/schedule/ctl error=buffer full"
        ));
        assert!(!is_vm_control_queue_full_error(
            "ERR ECHO reason=policy detail=invalid-payload path=/queen/lease/ctl error=invalid payload"
        ));
    }

    #[test]
    fn telemetry_write_routing_batches_only_segment_appends() {
        assert_eq!(
            write_pool_kind("/queen/telemetry/bench/seg/current"),
            PoolKind::Telemetry
        );
        assert!(is_batchable_telemetry_write_path(
            "/queen/telemetry/bench/seg/current"
        ));

        for path in [
            "/queen/telemetry/bench/ctl",
            "/queen/telemetry/bench/segment",
            CLIENT_QUEEN_SCHEDULE_CTL_PATH,
            CLIENT_POLICY_CTL_PATH,
        ] {
            assert_eq!(write_pool_kind(path), PoolKind::Control);
            assert!(
                !is_batchable_telemetry_write_path(path),
                "unexpected batchable path: {path}"
            );
        }
    }

    #[test]
    fn telemetry_segment_receipt_accepts_only_matching_bounded_ack() {
        let path = "/queen/telemetry/bench/ctl";
        let valid =
            vec!["OK ECHO path=/queen/telemetry/bench/ctl bytes=41 seg_id=seg-000001".to_owned()];
        assert_eq!(
            telemetry_segment_id_from_ack_lines(path, valid.as_slice()).as_deref(),
            Some("seg-000001")
        );

        for line in [
            "ERR ECHO path=/queen/telemetry/bench/ctl seg_id=seg-000001",
            "OK CAT path=/queen/telemetry/bench/ctl seg_id=seg-000001",
            "OK ECHO path=/queen/telemetry/other/ctl seg_id=seg-000001",
            "OK ECHO path=/queen/telemetry/bench/ctl",
            "OK ECHO path=/queen/telemetry/bench/ctl seg_id=..",
            "OK ECHO path=/queen/telemetry/bench/ctl seg_id=bad/segment",
            "OK ECHO path=/queen/telemetry/bench/ctl seg_id=bad\0segment",
        ] {
            assert_eq!(
                telemetry_segment_id_from_ack_lines(path, &[line.to_owned()]),
                None,
                "unexpectedly accepted receipt: {line}"
            );
        }

        let oversized = format!("OK ECHO path={path} seg_id={}", "a".repeat(MAX_ID_LEN + 1));
        assert_eq!(
            telemetry_segment_id_from_ack_lines(path, &[oversized]),
            None
        );
        assert_eq!(
            telemetry_segment_id_from_ack_lines("/queen/telemetry/bench/latest", valid.as_slice()),
            None
        );
    }

    #[test]
    fn broker_attach_primes_exactly_one_control_and_one_telemetry_session() {
        let creates = Arc::new(AtomicUsize::new(0));
        let factory_creates = creates.clone();
        let server = NineDoor::new_with_shard_layout(ShardLayout::enabled(8, true));
        let factory: Arc<dyn TransportFactory> = Arc::new(move || {
            factory_creates.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(NineDoorTransport::new(server.clone()))
                as Box<dyn cohsh::Transport + Send>)
        });
        let pool = SessionPool::new(3, 5, factory);

        let response = execute_broker_request(
            &pool,
            PoolKind::Control,
            BrokerRequest::Attach {
                role: Role::Queen,
                ticket: None,
            },
        )
        .expect("broker attach must prime both gateway lanes");

        assert!(matches!(response, BrokerResponse::Unit));
        assert_eq!(creates.load(Ordering::Relaxed), 2);

        let control = pool
            .checkout(PoolKind::Control)
            .expect("primed control session must be idle");
        let telemetry = pool
            .checkout(PoolKind::Telemetry)
            .expect("primed telemetry session must be idle");
        assert_eq!(creates.load(Ordering::Relaxed), 2);
        drop(control);
        drop(telemetry);
    }

    #[test]
    fn broker_attach_failure_closes_partially_primed_pool() {
        let creates = Arc::new(AtomicUsize::new(0));
        let factory_creates = creates.clone();
        let server = NineDoor::new_with_shard_layout(ShardLayout::enabled(8, true));
        let factory: Arc<dyn TransportFactory> = Arc::new(move || {
            let create_index = factory_creates.fetch_add(1, Ordering::Relaxed);
            if create_index == 1 {
                return Err(anyhow!("injected telemetry prime failure"));
            }
            Ok(Box::new(NineDoorTransport::new(server.clone()))
                as Box<dyn cohsh::Transport + Send>)
        });
        let pool = SessionPool::new(2, 2, factory);

        let err = match execute_broker_request(
            &pool,
            PoolKind::Control,
            BrokerRequest::Attach {
                role: Role::Queen,
                ticket: None,
            },
        ) {
            Ok(_) => panic!("telemetry prime failure must reject broker attach"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("prime gateway telemetry session"));
        assert_eq!(creates.load(Ordering::Relaxed), 2);
        match pool.checkout(PoolKind::Control) {
            Ok(_) => panic!("failed attach must leave the pool closed"),
            Err(err) => assert_eq!(err.to_string(), "session pool is closed"),
        }
    }

    #[test]
    fn telemetry_broker_discards_stale_ack_and_returns_current_receipt() {
        struct ReceiptTransport {
            attach_transport: NineDoorTransport,
            acknowledgements: Vec<String>,
        }

        impl ReceiptTransport {
            fn new() -> Self {
                Self {
                    attach_transport: NineDoorTransport::new(NineDoor::new_with_shard_layout(
                        ShardLayout::enabled(8, true),
                    )),
                    acknowledgements: Vec::new(),
                }
            }
        }

        impl cohsh::Transport for ReceiptTransport {
            fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
                cohsh::Transport::attach(&mut self.attach_transport, role, ticket)
            }

            fn ping(&mut self, _session: &Session) -> Result<String> {
                Ok("pong".to_owned())
            }

            fn tail(
                &mut self,
                _session: &Session,
                _path: &str,
                _lines: Option<u16>,
            ) -> Result<Vec<String>> {
                Err(anyhow!("tail is not used by this test"))
            }

            fn read(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
                Err(anyhow!("read is not used by this test"))
            }

            fn list(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
                Err(anyhow!("list is not used by this test"))
            }

            fn write(&mut self, _session: &Session, path: &str, _payload: &[u8]) -> Result<()> {
                let segment = if is_telemetry_control_path(path) {
                    "seg-current"
                } else {
                    "seg-stale"
                };
                self.acknowledgements.push(format!(
                    "OK ECHO path=/queen/telemetry/bench/ctl seg_id={segment}"
                ));
                Ok(())
            }

            fn drain_acknowledgements(&mut self) -> Vec<String> {
                std::mem::take(&mut self.acknowledgements)
            }
        }

        let factory: Arc<dyn TransportFactory> =
            Arc::new(|| Ok(Box::new(ReceiptTransport::new()) as Box<dyn cohsh::Transport + Send>));
        let pool = SessionPool::new(1, 1, factory);
        pool.attach(Role::Queen, None)
            .expect("receipt transport must attach");

        let ordinary = execute_broker_request(
            &pool,
            PoolKind::Control,
            BrokerRequest::Write {
                path: "/queen/ctl".to_owned(),
                payload: b"{}".to_vec(),
            },
        )
        .expect("ordinary write must succeed");
        assert!(matches!(ordinary, BrokerResponse::Unit));

        let receipt = execute_broker_request(
            &pool,
            PoolKind::Control,
            BrokerRequest::Write {
                path: "/queen/telemetry/bench/ctl".to_owned(),
                payload: br#"{"new":"segment","mime":"text/plain"}"#.to_vec(),
            },
        )
        .expect("telemetry control write must succeed");
        let BrokerResponse::Lines(lines) = receipt else {
            panic!("telemetry control write must return receipt lines");
        };
        assert_eq!(lines, ["seg-current"]);

        let (_, response) = response_ok(
            "ECHO",
            "/queen/telemetry/bench/ctl".to_owned(),
            lines,
            Some(41),
        );
        assert_eq!(response.0.lines, ["seg-current"]);
        assert!(response.0.end);
    }

    #[test]
    fn telemetry_write_batch_accepts_only_same_segment_path() {
        let segment_path = "/queen/telemetry/bench/seg/current";
        let other_segment_path = "/queen/telemetry/bench/seg/next";
        let (command, _response_rx) = test_write_command(PoolKind::Telemetry, segment_path, b"one");
        let batch_result = TelemetryWriteBatch::from_command(command);
        assert!(batch_result.is_ok(), "expected batchable command");
        let Ok(mut batch) = batch_result else {
            return;
        };
        assert_eq!(batch.len(), 1);

        let (same_path_command, _response_rx) =
            test_write_command(PoolKind::Telemetry, segment_path, b"two");
        assert!(batch.try_push(same_path_command).is_ok());
        assert_eq!(batch.len(), 2);

        let (other_path_command, _response_rx) =
            test_write_command(PoolKind::Telemetry, other_segment_path, b"three");
        assert!(batch.try_push(other_path_command).is_err());
        assert_eq!(batch.len(), 2);

        let (control_command, _response_rx) =
            test_write_command(PoolKind::Control, segment_path, b"four");
        assert!(batch.try_push(control_command).is_err());
        assert_eq!(batch.len(), 2);

        for index in batch.len()..TELEMETRY_WRITE_BATCH_MAX {
            let payload = format!("payload-{index}");
            let (command, _response_rx) =
                test_write_command(PoolKind::Telemetry, segment_path, payload.as_bytes());
            assert!(batch.try_push(command).is_ok());
        }
        let (overflow_command, _response_rx) =
            test_write_command(PoolKind::Telemetry, segment_path, b"overflow");
        assert!(batch.try_push(overflow_command).is_err());
        assert_eq!(batch.len(), TELEMETRY_WRITE_BATCH_MAX);
    }

    #[test]
    fn control_write_backpressure_tracks_and_expires_queue_full_paths() {
        let mut backpressure = ControlWriteBackpressure::default();
        let now = Instant::now();
        let last_error =
            "ERR ECHO reason=quota detail=buffer-full path=/queen/schedule/ctl error=buffer full";

        let recorded = backpressure.record(
            ControlWriteBackpressureKey::Schedule,
            0,
            now,
            Duration::from_millis(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS),
            last_error,
        );

        assert!(recorded);

        let refusal = backpressure.refusal(
            ControlWriteBackpressureKey::Schedule,
            now + Duration::from_millis(100),
        );
        assert_eq!(refusal.as_deref(), Some(last_error));

        assert!(backpressure
            .refusal(
                ControlWriteBackpressureKey::Schedule,
                now + Duration::from_millis(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS + 1),
            )
            .is_none());
    }

    #[test]
    fn control_write_backpressure_isolates_lease_operation_classes() {
        let mut backpressure = ControlWriteBackpressure::default();
        let now = Instant::now();
        let grant_error =
            "ERR ECHO reason=quota detail=buffer-full path=/queen/lease/ctl error=buffer full";
        backpressure.record(
            ControlWriteBackpressureKey::LeaseGrant,
            0,
            now,
            Duration::from_millis(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS),
            grant_error,
        );

        assert_eq!(
            backpressure
                .refusal(ControlWriteBackpressureKey::LeaseGrant, now)
                .as_deref(),
            Some(grant_error)
        );
        assert!(backpressure
            .refusal(ControlWriteBackpressureKey::LeasePreempt, now)
            .is_none());

        backpressure.record(
            ControlWriteBackpressureKey::LeasePreempt,
            0,
            now,
            Duration::from_millis(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS),
            grant_error,
        );
        backpressure.clear_after_success(ControlWriteBackpressureKey::LeaseGrant);
        assert!(backpressure
            .refusal(ControlWriteBackpressureKey::LeaseGrant, now)
            .is_none());
        assert!(backpressure
            .refusal(ControlWriteBackpressureKey::LeasePreempt, now)
            .is_some());
    }

    #[test]
    fn final_control_write_error_prefers_initial_retryable_error() {
        let first = Some(
            "ERR ECHO reason=quota detail=buffer-full path=/queen/ctl error=buffer full".to_owned(),
        );
        let later = "ERR ECHO reason=policy detail=denied path=/queen/ctl error=EPERM";
        assert_eq!(
            final_control_write_error(&first, later),
            "ERR ECHO reason=quota detail=buffer-full path=/queen/ctl error=buffer full"
        );
        assert_eq!(final_control_write_error(&None, later), later);
    }

    #[test]
    fn retry_budget_defaults_preserve_existing_control_write_window() {
        assert_eq!(DEFAULT_CONTROL_WRITE_RETRY_WINDOW_MS, 1_200);
        const {
            assert!(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS < DEFAULT_CONTROL_WRITE_RETRY_WINDOW_MS);
        }
        assert_eq!(validate_control_write_retry_window_ms(0).unwrap(), 0);
        assert!(
            validate_control_write_retry_window_ms(MAX_CONTROL_WRITE_RETRY_WINDOW_MS + 1).is_err()
        );
        assert_eq!(duration_millis_u64(Duration::from_millis(17)), 17);
    }

    #[test]
    fn broker_status_snapshot_includes_control_write_retry_counters() {
        let metrics = BrokerMetrics::default();
        metrics
            .control_write_retryable_errors
            .store(3, Ordering::Relaxed);
        metrics.control_write_retries.store(4, Ordering::Relaxed);
        metrics
            .control_write_retry_sleep_ms
            .store(75, Ordering::Relaxed);
        metrics
            .control_write_retry_exhaustions
            .store(2, Ordering::Relaxed);
        metrics
            .control_write_success_after_retry
            .store(1, Ordering::Relaxed);

        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.control_write_retryable_errors, 3);
        assert_eq!(snapshot.control_write_retries, 4);
        assert_eq!(snapshot.control_write_retry_sleep_ms, 75);
        assert_eq!(snapshot.control_write_retry_exhaustions, 2);
        assert_eq!(snapshot.control_write_success_after_retry, 1);
    }

    #[test]
    fn apply_policy_overrides_updates_pool_sizes_when_requested() {
        let mut config = GatewayConfig {
            bind: "127.0.0.1:8080".to_owned(),
            tcp_host: "127.0.0.1".to_owned(),
            tcp_port: 31337,
            auth_token: "token".to_owned(),
            request_auth_token: "request-token".to_owned(),
            role: Role::Queen,
            ticket: None,
            pool_control_sessions: Some(3),
            pool_telemetry_sessions: Some(12),
            broker_timeouts: BrokerTimeouts {
                control_response_ms: DEFAULT_BROKER_CONTROL_RESPONSE_TIMEOUT_MS,
                telemetry_response_ms: DEFAULT_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS,
            },
            control_write_retry_window_ms: DEFAULT_CONTROL_WRITE_RETRY_WINDOW_MS,
            mock: true,
            allow_non_loopback_bind: false,
            worker_acceptance_evidence: None,
            worker_acceptance_root: None,
            target_session: None,
        };
        let policy = CohshPolicy::from_generated();
        let updated = apply_policy_overrides(policy, &config).expect("apply overrides");
        assert_eq!(updated.pool.control_sessions, 3);
        assert_eq!(updated.pool.telemetry_sessions, 12);

        config.pool_control_sessions = None;
        config.pool_telemetry_sessions = None;
        let unchanged =
            apply_policy_overrides(CohshPolicy::from_generated(), &config).expect("no override");
        assert_eq!(
            unchanged.pool.control_sessions,
            CohshPolicy::from_generated().pool.control_sessions
        );
        assert_eq!(
            unchanged.pool.telemetry_sessions,
            CohshPolicy::from_generated().pool.telemetry_sessions
        );
    }

    fn disconnected_cached_state() -> AppState {
        let (control_tx, _control_rx) = mpsc::sync_channel(0);
        let (telemetry_tx, _telemetry_rx) = mpsc::sync_channel(0);
        let factory: Arc<dyn TransportFactory> = Arc::new(|| {
            Err(anyhow!(
                "cached read/list regression test should not construct transport"
            ))
        });
        AppState {
            inner: Arc::new(GatewayInner {
                pool: SessionPool::new(1, 1, factory),
                broker_client: GatewayBrokerClient {
                    control_tx,
                    telemetry_tx,
                },
                role: Role::Queen,
                ticket: None,
                request_auth_token: "request-token".to_owned(),
                status: Mutex::new(GatewayStatus {
                    connected: false,
                    last_error: Some("offline".to_owned()),
                    last_change: None,
                    reconnects: 1,
                    connects: 0,
                }),
                shutdown: Arc::new(AtomicBool::new(false)),
                broker_timeouts: BrokerTimeouts {
                    control_response_ms: DEFAULT_BROKER_CONTROL_RESPONSE_TIMEOUT_MS,
                    telemetry_response_ms: DEFAULT_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS,
                },
                control_write_retry_window_ms: DEFAULT_CONTROL_WRITE_RETRY_WINDOW_MS,
                bounds: build_bounds().expect("compiled generated Worker bounds must parse"),
                backend_class: BackendClass::Unknown,
                worker_acceptance: acceptance_diagnostic(
                    WorkerAcceptanceDiagnosticCode::NotConfigured,
                ),
                policy: CohshPolicy::from_generated(),
                broker: Arc::new(BrokerMetrics::default()),
                proc_cache: Mutex::new(ProcReadCache::default()),
                control_write_backpressure: Mutex::new(ControlWriteBackpressure::default()),
            }),
        }
    }

    #[test]
    fn cached_read_hit_does_not_require_connected_gateway() {
        let state = disconnected_cached_state();
        state.read_cache_insert("/proc/root/reachable", vec!["reachable=yes".to_owned()]);

        let lines = state
            .read("/proc/root/reachable")
            .expect("cached read should bypass reconnect");

        assert_eq!(lines, vec!["reachable=yes"]);
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            state
                .inner
                .broker
                .timeout_rejections
                .load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn cached_list_hit_does_not_require_connected_gateway() {
        let state = disconnected_cached_state();
        state.read_cache_insert("/proc", vec!["root".to_owned(), "lease".to_owned()]);

        let lines = state
            .list("/proc")
            .expect("cached list should bypass reconnect");

        assert_eq!(lines, vec!["root", "lease"]);
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            state
                .inner
                .broker
                .timeout_rejections
                .load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn uncached_request_fails_fast_while_gateway_is_disconnected() {
        let state = disconnected_cached_state();

        let err = state
            .read_uncached("/proc/root/reachable")
            .expect_err("uncached request must not attach on the request path");

        assert_eq!(
            err.to_string(),
            "gateway transport unavailable: backend is not connected"
        );
        assert!(!state.is_connected());
        let (status, body) = response_transport_err("CAT", "/proc/root/reachable", err);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.0.status, "ERR");
        assert_eq!(
            body.0.error.as_deref(),
            Some("gateway transport unavailable: backend is not connected")
        );
    }

    #[test]
    fn nonblocking_cache_probe_returns_independent_hit_without_a_miss() {
        let state = disconnected_cached_state();
        let path = "/proc/root/reachable";
        state.read_cache_insert(path, vec!["reachable=yes".to_owned()]);

        let mut lines = state
            .read_cache_try_get(path)
            .expect("nonblocking cache probe must hit");
        lines[0].push_str("-mutated");

        let cache = state.proc_cache_guard();
        assert_eq!(
            cache
                .entries
                .get(path)
                .expect("cached entry must remain present")
                .lines
                .as_ref(),
            ["reachable=yes"]
        );
        drop(cache);
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn nonblocking_cache_probe_miss_defers_accounting_to_fallback() {
        let state = disconnected_cached_state();
        let path = "/proc/root/reachable";

        assert!(state.read_cache_try_get(path).is_none());
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            0
        );
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            0
        );

        let lines = state
            .read_through_cache(path, || Ok(vec!["reachable=yes".to_owned()]))
            .expect("blocking fallback must fill the cache");
        assert_eq!(lines, ["reachable=yes"]);
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn nonblocking_cache_probe_preserves_existing_fill_for_fallback() {
        let state = disconnected_cached_state();
        let path = "/proc/root/reachable";
        let fill = Arc::new(ProcReadFill::default());
        state
            .proc_cache_guard()
            .in_flight
            .insert(path.to_owned(), Arc::clone(&fill));

        assert!(state.read_cache_try_get(path).is_none());
        let cache = state.proc_cache_guard();
        assert!(cache
            .in_flight
            .get(path)
            .is_some_and(|current| Arc::ptr_eq(current, &fill)));
        drop(cache);
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            0
        );
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn nonblocking_cache_probe_expires_then_refills_once() {
        let state = disconnected_cached_state();
        let path = "/proc/root/reachable";
        state.read_cache_insert(path, vec!["reachable=stale".to_owned()]);
        {
            let mut cache = state.proc_cache_guard();
            cache
                .entries
                .get_mut(path)
                .expect("cached entry must exist")
                .inserted_at =
                Instant::now() - Duration::from_millis(DEFAULT_PROC_CACHE_TTL_MS + 1);
        }

        assert!(state.read_cache_try_get(path).is_none());
        {
            let cache = state.proc_cache_guard();
            assert!(!cache.entries.contains_key(path));
            assert!(!cache.order.iter().any(|entry| entry == path));
        }
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            0
        );

        let lines = state
            .read_through_cache(path, || Ok(vec!["reachable=fresh".to_owned()]))
            .expect("expired cache entry must refill");
        assert_eq!(lines, ["reachable=fresh"]);
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn nonblocking_cache_probe_falls_back_without_waiting_on_contention() {
        let state = disconnected_cached_state();
        let path = "/proc/root/reachable";
        state.read_cache_insert(path, vec!["reachable=yes".to_owned()]);
        let cache = state.proc_cache_guard();
        let probe_state = state.clone();
        let probe = thread::spawn(move || probe_state.read_cache_try_get(path));

        assert!(probe
            .join()
            .expect("nonblocking cache probe thread must join")
            .is_none());
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            0
        );
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            0
        );

        drop(cache);
        assert_eq!(
            state
                .read_cache_try_get(path)
                .expect("probe must hit after contention clears"),
            ["reachable=yes"]
        );
    }

    #[test]
    fn nonblocking_cache_probe_recovers_poisoned_cache() {
        let state = disconnected_cached_state();
        let path = "/proc/root/reachable";
        state.read_cache_insert(path, vec!["reachable=yes".to_owned()]);
        let poison_state = state.clone();
        let poison = thread::spawn(move || {
            let _cache = poison_state
                .inner
                .proc_cache
                .lock()
                .expect("cache must start unpoisoned");
            panic!("poison cache mutex for recovery test");
        });
        assert!(poison.join().is_err());

        assert_eq!(
            state
                .read_cache_try_get(path)
                .expect("nonblocking probe must recover poisoned cache"),
            ["reachable=yes"]
        );
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            1
        );
    }

    #[tokio::test]
    async fn cached_list_and_cat_handlers_preserve_wire_response_bounds() {
        let state = disconnected_cached_state();
        state.read_cache_insert("/proc", vec!["root".to_owned(), "lease".to_owned()]);
        state.read_cache_insert("/proc/root/reachable", vec!["reachable=yes".to_owned()]);

        let list_response = handle_list(state.clone(), "/proc".to_owned())
            .await
            .into_response();
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body = axum::body::to_bytes(list_response.into_body(), 1024)
            .await
            .expect("cached list response body");
        let list_json: serde_json::Value =
            serde_json::from_slice(&list_body).expect("cached list response JSON");
        assert_eq!(list_json["status"], "OK");
        assert_eq!(list_json["verb"], "LS");
        assert_eq!(list_json["path"], "/proc");
        assert_eq!(list_json["end"], true);
        assert_eq!(list_json["lines"], serde_json::json!(["root", "lease"]));

        let cat_response = handle_cat(
            state.clone(),
            CatQuery {
                path: "/proc/root/reachable".to_owned(),
                max_bytes: Some(64),
            },
        )
        .await
        .into_response();
        assert_eq!(cat_response.status(), StatusCode::OK);
        let cat_body = axum::body::to_bytes(cat_response.into_body(), 1024)
            .await
            .expect("cached CAT response body");
        let cat_json: serde_json::Value =
            serde_json::from_slice(&cat_body).expect("cached CAT response JSON");
        assert_eq!(cat_json["status"], "OK");
        assert_eq!(cat_json["verb"], "CAT");
        assert_eq!(cat_json["path"], "/proc/root/reachable");
        assert_eq!(cat_json["end"], true);
        assert_eq!(cat_json["lines"], serde_json::json!(["reachable=yes"]));
        assert_eq!(cat_json["bytes"], "reachable=yes".len());
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            2
        );
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            0
        );
        assert_eq!(
            state
                .inner
                .broker
                .telemetry_checkouts
                .load(Ordering::Relaxed),
            0
        );

        let too_small = handle_cat(
            state,
            CatQuery {
                path: "/proc/root/reachable".to_owned(),
                max_bytes: Some(4),
            },
        )
        .await
        .into_response();
        assert_eq!(too_small.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn invalidation_removes_entry_from_nonblocking_cache_probe() {
        let state = disconnected_cached_state();
        state.read_cache_insert(PROC_SCHEDULE_SUMMARY_PATH, vec!["queue=stale".to_owned()]);

        state.read_cache_invalidate_for_write(CLIENT_QUEEN_SCHEDULE_CTL_PATH, br#"{}"#);

        assert!(state
            .read_cache_try_get(PROC_SCHEDULE_SUMMARY_PATH)
            .is_none());
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn cache_invalidation_preserves_static_and_unrelated_namespaces() {
        let state = disconnected_cached_state();
        state.read_cache_insert("/queen", vec!["ctl".to_owned(), "telemetry".to_owned()]);
        state.read_cache_insert("/proc/pressure/policy", vec!["state=high-load".to_owned()]);

        state.read_cache_invalidate_for_write(CLIENT_POLICY_CTL_PATH, br#"{}"#);

        assert_eq!(
            state.read_cache_get("/queen").as_deref(),
            Some(["ctl".to_owned(), "telemetry".to_owned()].as_slice())
        );
        assert!(state.read_cache_get("/proc/pressure/policy").is_none());

        state.read_cache_insert(
            "/queen/telemetry/bench/latest",
            vec!["seg-000001".to_owned()],
        );
        state.read_cache_insert("/gpu/bridge/status", vec!["ready=yes".to_owned()]);
        state
            .read_cache_invalidate_for_write("/queen/telemetry/bench/ctl", br#"{"new":"segment"}"#);

        assert!(state
            .read_cache_get("/queen/telemetry/bench/latest")
            .is_none());
        assert_eq!(
            state.read_cache_get("/gpu/bridge/status").as_deref(),
            Some(["ready=yes".to_owned()].as_slice())
        );
    }

    #[test]
    fn lease_quota_invalidation_preserves_unchanged_lease_views() {
        let state = disconnected_cached_state();
        state.read_cache_insert(PROC_LEASE_SUMMARY_PATH, vec!["quotas=1".to_owned()]);
        state.read_cache_insert(PROC_LEASE_ACTIVE_PATH, vec!["id=lease-1".to_owned()]);
        state.read_cache_insert(PROC_LEASE_PREEMPTIONS_PATH, vec!["id=lease-old".to_owned()]);

        state.read_cache_invalidate_for_write(
            CLIENT_QUEEN_LEASE_CTL_PATH,
            br#"{"op":"quota","subject":"queen","resource":"gpu0","max_active":2,"max_preemptions":2}"#,
        );

        assert!(state.read_cache_get(PROC_LEASE_SUMMARY_PATH).is_none());
        assert_eq!(
            state.read_cache_get(PROC_LEASE_ACTIVE_PATH),
            Some(vec!["id=lease-1".to_owned()])
        );
        assert_eq!(
            state.read_cache_get(PROC_LEASE_PREEMPTIONS_PATH),
            Some(vec!["id=lease-old".to_owned()])
        );
    }

    #[test]
    fn cache_fill_shares_immutable_lines_and_returns_independent_vectors() {
        let state = disconnected_cached_state();
        let path = "/proc/root/reachable";
        let fill = Arc::new(ProcReadFill::default());
        let shared: SharedLines = vec!["reachable=yes".to_owned()].into();
        state
            .proc_cache_guard()
            .in_flight
            .insert(path.to_owned(), Arc::clone(&fill));

        state.read_cache_finish(path, &fill, Ok(Arc::clone(&shared)));

        {
            let cache = state.proc_cache_guard();
            let cached = cache.entries.get(path).expect("completed fill must cache");
            assert!(Arc::ptr_eq(&cached.lines, &shared));
        }
        {
            let published = fill.result.lock().expect("fill result lock");
            let published = published
                .as_ref()
                .expect("fill result must publish")
                .as_ref()
                .expect("fill result must succeed");
            assert!(Arc::ptr_eq(published, &shared));
        }

        let mut returned = state.read_cache_get(path).expect("cache hit");
        returned[0].push_str("-mutated");
        assert_eq!(
            state.read_cache_get(path).expect("second cache hit"),
            ["reachable=yes"]
        );
    }

    #[test]
    fn successful_preempt_clears_grant_and_rejects_stale_grant_recording() {
        let state = disconnected_cached_state();
        let now = Instant::now();
        let error =
            "ERR ECHO reason=quota detail=buffer-full path=/queen/lease/ctl error=buffer full";
        {
            let mut backpressure = state.control_write_backpressure_guard();
            for key in [
                ControlWriteBackpressureKey::LeaseGrant,
                ControlWriteBackpressureKey::LeasePreempt,
            ] {
                backpressure.record(
                    key,
                    0,
                    now,
                    Duration::from_millis(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS),
                    error,
                );
            }
        }

        state.control_write_backpressure_clear(ControlWriteBackpressureKey::LeasePreempt);

        let mut backpressure = state.control_write_backpressure_guard();
        assert!(backpressure
            .refusal(ControlWriteBackpressureKey::LeaseGrant, now)
            .is_none());
        assert!(backpressure
            .refusal(ControlWriteBackpressureKey::LeasePreempt, now)
            .is_none());
        assert_eq!(backpressure.grant_generation, 1);
        assert!(!backpressure.record(
            ControlWriteBackpressureKey::LeaseGrant,
            0,
            now,
            Duration::from_millis(CONTROL_WRITE_BACKPRESSURE_COOLDOWN_MS),
            error,
        ));
        assert!(backpressure
            .refusal(ControlWriteBackpressureKey::LeaseGrant, now)
            .is_none());
    }

    #[test]
    fn concurrent_cache_misses_share_one_fill() {
        const CALLERS: usize = 32;
        let state = Arc::new(disconnected_cached_state());
        let start = Arc::new(Barrier::new(CALLERS + 1));
        let release_leader = Arc::new(AtomicBool::new(false));
        let fetches = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::with_capacity(CALLERS);

        for _ in 0..CALLERS {
            let state = state.clone();
            let start = start.clone();
            let release_leader = release_leader.clone();
            let fetches = fetches.clone();
            handles.push(thread::spawn(move || {
                start.wait();
                state.read_through_cache("/proc/root/reachable", || {
                    fetches.fetch_add(1, Ordering::Relaxed);
                    while !release_leader.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                    Ok(vec!["reachable=yes".to_owned()])
                })
            }));
        }

        start.wait();
        let deadline = Instant::now() + Duration::from_secs(10);
        let all_waiting = loop {
            let misses = state.inner.broker.proc_cache_misses.load(Ordering::Relaxed);
            let waiters = state
                .proc_cache_guard()
                .in_flight
                .get("/proc/root/reachable")
                .map_or(0, |fill| fill.waiters.load(Ordering::Relaxed));
            if misses == CALLERS as u64 && waiters == CALLERS - 1 {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::yield_now();
        };
        release_leader.store(true, Ordering::Release);

        for handle in handles {
            let lines = handle.join().expect("cache request thread must join");
            assert_eq!(
                lines.expect("cache request must succeed"),
                ["reachable=yes"]
            );
        }
        assert!(all_waiting, "all cache followers must wait before release");
        assert_eq!(fetches.load(Ordering::Relaxed), 1);
        assert_eq!(
            state.inner.broker.proc_cache_misses.load(Ordering::Relaxed),
            CALLERS as u64
        );
        assert_eq!(
            state.inner.broker.proc_cache_hits.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn cache_fill_error_fans_out_and_next_request_retries() {
        const CALLERS: usize = 16;
        let state = Arc::new(disconnected_cached_state());
        let start = Arc::new(Barrier::new(CALLERS + 1));
        let release_leader = Arc::new(AtomicBool::new(false));
        let fetches = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::with_capacity(CALLERS);

        for _ in 0..CALLERS {
            let state = state.clone();
            let start = start.clone();
            let release_leader = release_leader.clone();
            let fetches = fetches.clone();
            handles.push(thread::spawn(move || {
                start.wait();
                state.read_through_cache("/proc/root/reachable", || {
                    fetches.fetch_add(1, Ordering::Relaxed);
                    while !release_leader.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                    Err(anyhow!("cold read failed"))
                })
            }));
        }

        start.wait();
        let deadline = Instant::now() + Duration::from_secs(10);
        let all_waiting = loop {
            let misses = state.inner.broker.proc_cache_misses.load(Ordering::Relaxed);
            let waiters = state
                .proc_cache_guard()
                .in_flight
                .get("/proc/root/reachable")
                .map_or(0, |fill| fill.waiters.load(Ordering::Relaxed));
            if misses == CALLERS as u64 && waiters == CALLERS - 1 {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::yield_now();
        };
        release_leader.store(true, Ordering::Release);

        for handle in handles {
            let result = handle.join().expect("cache request thread must join");
            assert_eq!(
                result.expect_err("cache fill must fail").to_string(),
                "cold read failed"
            );
        }
        assert!(all_waiting, "all cache followers must wait before release");
        assert_eq!(fetches.load(Ordering::Relaxed), 1);

        let retried = state
            .read_through_cache("/proc/root/reachable", || {
                fetches.fetch_add(1, Ordering::Relaxed);
                Ok(vec!["reachable=yes".to_owned()])
            })
            .expect("failed fills must remain retryable");
        assert_eq!(retried, ["reachable=yes"]);
        assert_eq!(fetches.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn invalidation_prevents_stale_fill_reinsertion() {
        let state = Arc::new(disconnected_cached_state());
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let leader_state = state.clone();
        let leader = thread::spawn(move || {
            let result = leader_state.read_through_cache("/proc/schedule/summary", || {
                started_tx
                    .send(())
                    .expect("test must observe the first cache fill");
                release_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("test must release the first cache fill");
                Ok(vec!["queue=stale".to_owned()])
            });
            result_tx
                .send(result)
                .expect("test must observe the first cache result");
        });

        started_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("first cache fill must reach the fetch boundary");
        state.read_cache_invalidate_for_write(CLIENT_QUEEN_SCHEDULE_CTL_PATH, br#"{}"#);
        let fresh = state
            .read_through_cache("/proc/schedule/summary", || {
                Ok(vec!["queue=fresh".to_owned()])
            })
            .expect("post-write cache fill must succeed");
        assert_eq!(fresh, ["queue=fresh"]);

        release_tx
            .send(())
            .expect("first cache fill must be releasable");
        let stale = result_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("first cache fill must complete within the test bound")
            .expect("already-started cache fill may complete");
        leader.join().expect("first cache fill thread must join");
        assert_eq!(stale, ["queue=stale"]);
        assert_eq!(
            state
                .read_cache_get("/proc/schedule/summary")
                .expect("fresh result must remain cached"),
            ["queue=fresh"]
        );
    }

    #[test]
    fn cache_invalidation_preserves_unrelated_fills_and_bounds_tracking() {
        let state = disconnected_cached_state();
        state.read_cache_insert("/proc/root/reachable", vec!["reachable=yes".to_owned()]);
        state.read_cache_insert("/host/docker/status", vec!["active=yes".to_owned()]);
        state.read_cache_invalidate_for_write("/host/docker/status", br#"{}"#);
        {
            let cache = state.proc_cache_guard();
            assert!(cache.entries.contains_key("/proc/root/reachable"));
            assert!(!cache.entries.contains_key("/host/docker/status"));
            assert_eq!(
                cache.order.iter().map(String::as_str).collect::<Vec<_>>(),
                ["/proc/root/reachable"]
            );
        }

        let proc_fill = Arc::new(ProcReadFill::default());
        let host_fill = Arc::new(ProcReadFill::default());
        {
            let mut cache = state.proc_cache_guard();
            cache
                .in_flight
                .insert("/proc/root/reachable".to_owned(), proc_fill.clone());
            cache
                .in_flight
                .insert("/host/docker/status".to_owned(), host_fill);
        }

        state.read_cache_invalidate_for_write("/host/docker/status", br#"{}"#);
        {
            let cache = state.proc_cache_guard();
            assert_eq!(cache.in_flight.len(), 1);
            assert!(cache
                .in_flight
                .get("/proc/root/reachable")
                .is_some_and(|fill| Arc::ptr_eq(fill, &proc_fill)));
        }

        state.read_cache_invalidate_for_write("/log/queen.log", br#"{}"#);
        assert!(state.proc_cache_guard().in_flight.is_empty());

        {
            let mut cache = state.proc_cache_guard();
            for index in 0..DEFAULT_PROC_CACHE_MAX_ENTRIES {
                cache.in_flight.insert(
                    format!("/proc/bounded/{index}"),
                    Arc::new(ProcReadFill::default()),
                );
            }
        }
        let bypass_fetches = AtomicUsize::new(0);
        let lines = state
            .read_through_cache("/proc/bounded/overflow", || {
                bypass_fetches.fetch_add(1, Ordering::Relaxed);
                Ok(vec!["bounded=yes".to_owned()])
            })
            .expect("bounded tracking must bypass coalescing without refusing the read");
        assert_eq!(lines, ["bounded=yes"]);
        assert_eq!(bypass_fetches.load(Ordering::Relaxed), 1);
        assert!(state.read_cache_get("/proc/bounded/overflow").is_none());
    }

    #[test]
    fn cancelled_cache_leader_does_not_strand_or_poison_future_reads() {
        let state = Arc::new(disconnected_cached_state());
        let fill = Arc::new(ProcReadFill::default());
        state
            .proc_cache_guard()
            .in_flight
            .insert("/proc/root/reachable".to_owned(), fill.clone());
        let leader = ProcReadFillLeader::new(&state, "/proc/root/reachable", fill.clone());

        let follower_state = state.clone();
        let follower_fetches = Arc::new(AtomicUsize::new(0));
        let follower_fetches_for_thread = follower_fetches.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let follower = thread::spawn(move || {
            let result = follower_state.read_through_cache("/proc/root/reachable", || {
                follower_fetches_for_thread.fetch_add(1, Ordering::Relaxed);
                Ok(vec!["unexpected-fetch=yes".to_owned()])
            });
            result_tx
                .send(result)
                .expect("test must observe the follower result");
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        while fill.waiters.load(Ordering::Relaxed) != 1 {
            assert!(
                Instant::now() < deadline,
                "cache follower must reach the condition wait"
            );
            thread::yield_now();
        }
        drop(leader);

        let error = result_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("cancelled follower must complete within the test bound")
            .expect_err("cancelled follower must receive an error");
        assert_eq!(error.to_string(), "gateway cache fill cancelled");
        follower.join().expect("cache follower thread must join");
        assert_eq!(follower_fetches.load(Ordering::Relaxed), 0);
        assert!(state.proc_cache_guard().in_flight.is_empty());

        let lines = state
            .read_through_cache("/proc/root/reachable", || {
                Ok(vec!["reachable=yes".to_owned()])
            })
            .expect("a cancelled leader must not block a later fill");
        assert_eq!(lines, ["reachable=yes"]);
    }

    #[test]
    fn bounded_joined_line_bytes_matches_wire_join_semantics() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(bounded_joined_line_bytes(&empty, 0), Some(0));

        let lines = vec!["alpha".to_owned(), "βeta".to_owned(), "".to_owned()];
        let expected = lines.join("\n").len();
        assert_eq!(bounded_joined_line_bytes(&lines, expected), Some(expected));
        assert_eq!(bounded_joined_line_bytes(&lines, expected - 1), None);
    }
}
