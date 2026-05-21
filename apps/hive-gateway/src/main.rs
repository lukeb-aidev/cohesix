// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Host-only REST gateway projecting Cohesix console/file semantics.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-only REST gateway projecting Cohesix console/file semantics.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use cohesix_net_constants::COHESIX_TCP_CONSOLE_PORT;
use cohesix_ticket::Role;
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
    parse_role, RoleParseMode, MAX_ECHO_LEN, MAX_ID_LEN, MAX_JSON_LEN, MAX_LINE_LEN, MAX_PATH_LEN,
    MAX_TICKET_LEN,
};
use nine_door::NineDoor;
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
const DEFAULT_PROC_CACHE_TTL_MS: u64 = 500;
const DEFAULT_PROC_CACHE_MAX_ENTRIES: usize = 64;
const BROKER_QUEUE_WAIT_LIMIT_MS: u64 = 5_000;
const DEFAULT_BROKER_CONTROL_RESPONSE_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS: u64 = 120_000;
const BROKER_ENQUEUE_RETRY_SLEEP_MS: u64 = 5;
const BROKER_CONTROL_QUEUE_CAPACITY: usize = 256;
const BROKER_TELEMETRY_QUEUE_CAPACITY: usize = 1024;
const BROKER_CONTROL_BURST: usize = 6;
const BROKER_IDLE_WAIT_MS: u64 = 20;
const CONTROL_WRITE_RETRY_WINDOW_MS: u64 = 1_200;
const CONTROL_WRITE_RETRY_SLEEP_MS: u64 = 15;
const CONTROL_WRITE_RETRY_MAX_SLEEP_MS: u64 = 120;
const CACHE_INVALIDATE_CONTROL_NAMESPACES: &[&str] = &["/proc", "/queen", "/worker", "/gpu"];
const CACHE_INVALIDATE_HOST_NAMESPACES: &[&str] = &["/host"];
const CACHE_INVALIDATE_GPU_NAMESPACES: &[&str] = &["/gpu"];

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
    /// Use the in-process mock NineDoor backend.
    #[arg(long, default_value_t = false)]
    mock: bool,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<GatewayInner>,
}

struct GatewayInner {
    broker_client: GatewayBrokerClient,
    role: Role,
    ticket: Option<String>,
    request_auth_token: String,
    status: Mutex<GatewayStatus>,
    shutdown: Arc<AtomicBool>,
    broker_timeouts: BrokerTimeouts,
    bounds: BoundsResponse,
    policy: CohshPolicy,
    broker: Arc<BrokerMetrics>,
    proc_cache: Mutex<ProcReadCache>,
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
    control_checkouts: AtomicU64,
    telemetry_checkouts: AtomicU64,
    pool_exhausted: AtomicU64,
    checkout_retries: AtomicU64,
    timeout_rejections: AtomicU64,
    telemetry_yields: AtomicU64,
    proc_cache_hits: AtomicU64,
    proc_cache_misses: AtomicU64,
    proc_cache_evictions: AtomicU64,
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
            control_checkouts: self.control_checkouts.load(Ordering::Relaxed),
            telemetry_checkouts: self.telemetry_checkouts.load(Ordering::Relaxed),
            pool_exhausted: self.pool_exhausted.load(Ordering::Relaxed),
            checkout_retries: self.checkout_retries.load(Ordering::Relaxed),
            timeout_rejections: self.timeout_rejections.load(Ordering::Relaxed),
            telemetry_yields: self.telemetry_yields.load(Ordering::Relaxed),
            proc_cache_hits: self.proc_cache_hits.load(Ordering::Relaxed),
            proc_cache_misses: self.proc_cache_misses.load(Ordering::Relaxed),
            proc_cache_evictions: self.proc_cache_evictions.load(Ordering::Relaxed),
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
}

#[derive(Clone)]
struct ProcReadCacheEntry {
    inserted_at: Instant,
    lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GatewayStatusResponse {
    connected: bool,
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
    control_checkouts: u64,
    telemetry_checkouts: u64,
    pool_exhausted: u64,
    checkout_retries: u64,
    timeout_rejections: u64,
    telemetry_yields: u64,
    proc_cache_hits: u64,
    proc_cache_misses: u64,
    proc_cache_evictions: u64,
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
    Tail { path: String },
    Write { path: String, payload: Vec<u8> },
}

enum BrokerResponse {
    Unit,
    Lines(Vec<String>),
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
    let bounds = build_bounds();
    let shutdown = Arc::new(AtomicBool::new(false));
    let broker_metrics = Arc::new(BrokerMetrics::default());
    seed_relay_metrics_from_env(&broker_metrics);
    let broker_client =
        build_gateway_broker(pool.clone(), broker_metrics.clone(), shutdown.clone());

    let state = AppState {
        inner: Arc::new(GatewayInner {
            broker_client,
            role: config.role,
            ticket: config.ticket.clone(),
            request_auth_token: config.request_auth_token.clone(),
            status: Mutex::new(GatewayStatus::default()),
            shutdown,
            broker_timeouts: config.broker_timeouts,
            bounds,
            policy,
            broker: broker_metrics,
            proc_cache: Mutex::new(ProcReadCache::default()),
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
    mock: bool,
    allow_non_loopback_bind: bool,
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
            mock,
            allow_non_loopback_bind,
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

fn broker_response_timeout_ms(value: Option<u64>, default_ms: u64, label: &str) -> Result<u64> {
    let timeout_ms = value.unwrap_or(default_ms);
    if timeout_ms < BROKER_QUEUE_WAIT_LIMIT_MS {
        anyhow::bail!("{label} must be >= broker queue wait limit {BROKER_QUEUE_WAIT_LIMIT_MS}ms");
    }
    Ok(timeout_ms)
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
        let server = NineDoor::new();
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

fn build_bounds() -> BoundsResponse {
    BoundsResponse {
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
    }
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
                dispatch_broker_command(&pool, &metrics, command);
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
            Ok(command) => dispatch_broker_command(&pool, &metrics, command),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {}
        }
    }
    pool.shutdown();
}

fn dispatch_broker_command(pool: &SharedPool, metrics: &BrokerMetrics, command: BrokerCommand) {
    decrement_counter(metrics.wait_counter(command.kind));
    metrics
        .checkout_counter(command.kind)
        .fetch_add(1, Ordering::Relaxed);
    let result = execute_broker_request(pool, command.kind, command.request).map_err(|err| {
        if is_pool_exhausted(&err) {
            metrics.pool_exhausted.fetch_add(1, Ordering::Relaxed);
            anyhow::anyhow!(
                "gateway backpressure: broker checkout exhausted for {:?}",
                command.kind
            )
        } else {
            err
        }
    });
    let _ = command.response_tx.send(result);
}

fn execute_broker_request(
    pool: &SharedPool,
    kind: PoolKind,
    request: BrokerRequest,
) -> Result<BrokerResponse> {
    match request {
        BrokerRequest::Attach { role, ticket } => {
            pool.attach(role, ticket.as_deref())?;
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
        BrokerRequest::Tail { path } => with_pool_once(pool, kind, move |transport, session| {
            transport.tail(session, &path).map(BrokerResponse::Lines)
        }),
        BrokerRequest::Write { path, payload } => {
            with_pool_once(pool, kind, move |transport, session| {
                transport.write(session, path.as_str(), payload.as_slice())?;
                Ok(BrokerResponse::Unit)
            })
        }
    }
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
                }
                Err(err) => {
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
                    self.inner
                        .broker
                        .wait_counter(kind)
                        .fetch_add(1, Ordering::Relaxed);
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
        self.attach()?;
        self.mark_connected();
        Ok(())
    }

    fn list(&self, path: &str) -> Result<Vec<String>> {
        self.ensure_connected()?;
        if is_cacheable_list_path(path) {
            if let Some(lines) = self.read_cache_get(path) {
                return Ok(lines);
            }
            self.inner
                .broker
                .proc_cache_misses
                .fetch_add(1, Ordering::Relaxed);
        }
        let lines = match self.submit_broker(
            PoolKind::Telemetry,
            BrokerRequest::List {
                path: path.to_owned(),
            },
        )? {
            BrokerResponse::Lines(lines) => lines,
            BrokerResponse::Unit => Vec::new(),
        };
        if is_cacheable_list_path(path) {
            self.read_cache_insert(path, lines.clone());
        }
        Ok(lines)
    }

    fn read(&self, path: &str) -> Result<Vec<String>> {
        self.ensure_connected()?;
        if is_cacheable_read_path(path) {
            if let Some(lines) = self.read_cache_get(path) {
                return Ok(lines);
            }
            self.inner
                .broker
                .proc_cache_misses
                .fetch_add(1, Ordering::Relaxed);
        }
        let lines = match self.submit_broker(
            PoolKind::Telemetry,
            BrokerRequest::Read {
                path: path.to_owned(),
            },
        )? {
            BrokerResponse::Lines(lines) => lines,
            BrokerResponse::Unit => Vec::new(),
        };
        if is_cacheable_read_path(path) {
            self.read_cache_insert(path, lines.clone());
        }
        Ok(lines)
    }

    fn tail(&self, path: &str) -> Result<Vec<String>> {
        self.ensure_connected()?;
        match self.submit_broker(
            PoolKind::Telemetry,
            BrokerRequest::Tail {
                path: path.to_owned(),
            },
        )? {
            BrokerResponse::Lines(lines) => Ok(lines),
            BrokerResponse::Unit => Ok(Vec::new()),
        }
    }

    fn write(&self, path: &str, payload: &[u8]) -> Result<()> {
        self.ensure_connected()?;
        let write_path = path.to_owned();
        let payload = payload.to_vec();
        let deadline = Instant::now() + Duration::from_millis(CONTROL_WRITE_RETRY_WINDOW_MS);
        let retry_deadline_enabled = is_retryable_control_write_path(write_path.as_str());
        let mut first_retryable_error: Option<String> = None;
        let mut retry_delay = Duration::from_millis(CONTROL_WRITE_RETRY_SLEEP_MS);
        loop {
            let result = self.submit_broker(
                PoolKind::Control,
                BrokerRequest::Write {
                    path: write_path.clone(),
                    payload: payload.clone(),
                },
            );
            match result {
                Ok(_) => {
                    self.read_cache_invalidate_for_write(write_path.as_str());
                    return Ok(());
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
                    if first_retryable_error.is_none() {
                        first_retryable_error = Some(message.clone());
                    }
                    if Instant::now() >= deadline {
                        return Err(anyhow::anyhow!(final_control_write_error(
                            &first_retryable_error,
                            message.as_str()
                        )));
                    }
                    thread::sleep(retry_delay);
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

    fn read_cache_get(&self, path: &str) -> Option<Vec<String>> {
        let mut cache = self.inner.proc_cache.lock().expect("cache lock poisoned");
        let entry = cache.entries.get(path).cloned()?;
        if entry.inserted_at.elapsed() > Duration::from_millis(DEFAULT_PROC_CACHE_TTL_MS) {
            cache.entries.remove(path);
            cache.order.retain(|value| value != path);
            return None;
        }
        self.inner
            .broker
            .proc_cache_hits
            .fetch_add(1, Ordering::Relaxed);
        Some(entry.lines)
    }

    fn read_cache_insert(&self, path: &str, lines: Vec<String>) {
        let mut cache = self.inner.proc_cache.lock().expect("cache lock poisoned");
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

    fn read_cache_invalidate_for_write(&self, write_path: &str) {
        let mut cache = self.inner.proc_cache.lock().expect("cache lock poisoned");
        let Some(namespaces) = cache_invalidation_namespaces(write_path) else {
            cache.entries.clear();
            cache.order.clear();
            return;
        };
        cache
            .entries
            .retain(|key, _| !namespaces.iter().any(|ns| cache_key_in_namespace(key, ns)));
        let retained_order: Vec<String> = cache
            .order
            .iter()
            .filter(|key| cache.entries.contains_key((*key).as_str()))
            .cloned()
            .collect();
        cache.order = VecDeque::from(retained_order);
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
    let path = query.path.clone();
    let result = tokio::task::spawn_blocking(move || state.read(&path)).await;
    match result {
        Ok(Ok(lines)) => {
            let bytes = lines.join("\n").len();
            if bytes > max_bytes {
                return response_err(
                    verb,
                    &query.path,
                    format!("read exceeded max_bytes {max_bytes}"),
                    StatusCode::BAD_REQUEST,
                );
            }
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
    let path = query.path.clone();
    let result = tokio::task::spawn_blocking(move || state.tail(&path)).await;
    match result {
        Ok(Ok(lines)) => {
            let bytes = lines.join("\n").len();
            if bytes > max_bytes {
                return response_err(
                    verb,
                    &query.path,
                    format!("tail exceeded max_bytes {max_bytes}"),
                    StatusCode::BAD_REQUEST,
                );
            }
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
        Ok(Ok(())) => response_ok(verb, payload.path, Vec::new(), Some(raw_len)),
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
        "/" | "/proc" | "/queen" | "/worker" | "/gpu" | "/host"
    )
}

fn cache_key_in_namespace(key: &str, namespace: &str) -> bool {
    key == namespace || key.starts_with(&format!("{namespace}/"))
}

fn cache_invalidation_namespaces(write_path: &str) -> Option<&'static [&'static str]> {
    if write_path.starts_with("/queen/")
        || write_path == CLIENT_POLICY_CTL_PATH
        || write_path.starts_with("/actions/")
    {
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

fn is_retryable_control_write_error(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    lowered.contains("buffer-full")
        || lowered.contains("buffer full")
        || lowered.contains("session pool exhausted")
        || lowered.contains("gateway backpressure")
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
        for path in ["/", "/proc", "/queen", "/worker", "/gpu", "/host"] {
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
            cache_invalidation_namespaces("/queen/schedule/ctl"),
            Some(CACHE_INVALIDATE_CONTROL_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/policy/ctl"),
            Some(CACHE_INVALIDATE_CONTROL_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/host/docker/status"),
            Some(CACHE_INVALIDATE_HOST_NAMESPACES)
        );
        assert_eq!(
            cache_invalidation_namespaces("/gpu/bridge/status"),
            Some(CACHE_INVALIDATE_GPU_NAMESPACES)
        );
        assert_eq!(cache_invalidation_namespaces("/log/queen.log"), None);
    }

    #[test]
    fn retryable_control_write_helpers_match_expected_paths_and_errors() {
        assert!(is_retryable_control_write_path("/queen/lease/ctl"));
        assert!(is_retryable_control_write_path("/policy/ctl"));
        assert!(is_retryable_control_write_path("/actions/queue"));
        assert!(!is_retryable_control_write_path("/host/docker/status"));

        assert!(is_retryable_control_write_error(
            "ERR ECHO reason=quota detail=buffer-full path=/queen/schedule/ctl error=buffer full"
        ));
        assert!(is_retryable_control_write_error(
            "gateway backpressure: session pool checkout timed out for Control"
        ));
        assert!(!is_retryable_control_write_error(
            "ERR ECHO reason=policy detail=invalid-payload path=/queen/lease/ctl error=invalid payload"
        ));
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
            mock: true,
            allow_non_loopback_bind: false,
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
}
