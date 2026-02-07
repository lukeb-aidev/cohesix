// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Host-only REST gateway projecting Cohesix console/file semantics.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-only REST gateway projecting Cohesix console/file semantics.

use std::env;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use cohsh::{
    CohshPolicy, PoolKind, SessionPool, TransportFactory, CLIENT_LOG_PATH,
    CLIENT_POLICY_CTL_PATH, CLIENT_QUEEN_CTL_PATH, CLIENT_QUEEN_EXPORT_CTL_PATH,
    CLIENT_QUEEN_LEASE_CTL_PATH, CLIENT_QUEEN_LIFECYCLE_CTL_PATH,
    CLIENT_QUEEN_SCHEDULE_CTL_PATH, CONTROL_EXPORT_CTL_MAX_BYTES, CONTROL_EXPORT_ENABLED,
    CONTROL_LEASE_ACTIVE_MAX_ENTRIES, CONTROL_LEASE_CTL_MAX_BYTES, CONTROL_LEASE_ENABLED,
    CONTROL_LEASE_PREEMPTIONS_MAX_ENTRIES, CONTROL_SCHEDULE_CTL_MAX_BYTES,
    CONTROL_SCHEDULE_ENABLED, CONTROL_SCHEDULE_QUEUE_MAX_ENTRIES, POLICY_CTL_MAX_BYTES,
    POLICY_ENABLED, POLICY_QUEUE_MAX_BYTES, POLICY_QUEUE_MAX_ENTRIES, PROC_LEASE_ACTIVE_BYTES,
    PROC_LEASE_PREEMPTIONS_BYTES, PROC_LEASE_SUMMARY_BYTES,
    PROC_LEASE_ACTIVE_ENABLED, PROC_LEASE_PREEMPTIONS_ENABLED, PROC_LEASE_SUMMARY_ENABLED,
    PROC_SCHEDULE_QUEUE_BYTES, PROC_SCHEDULE_SUMMARY_BYTES, PROC_SCHEDULE_QUEUE_ENABLED,
    PROC_SCHEDULE_SUMMARY_ENABLED, SECURE9P_MSIZE,
    SECURE9P_WALK_DEPTH,
};
use cohsh::{NineDoorTransport, SharedTcpTransport, TcpTransport};
use cohsh_core::{parse_role, RoleParseMode, MAX_ECHO_LEN, MAX_ID_LEN, MAX_JSON_LEN, MAX_LINE_LEN, MAX_PATH_LEN, MAX_TICKET_LEN};
use cohesix_net_constants::COHESIX_TCP_CONSOLE_PORT;
use cohesix_ticket::Role;
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
    /// TCP console auth token.
    #[arg(long, default_value = "changeme")]
    auth_token: String,
    /// Role to attach with (queen by default).
    #[arg(long, default_value = "queen")]
    role: String,
    /// Optional capability ticket payload.
    #[arg(long)]
    ticket: Option<String>,
    /// Use the in-process mock NineDoor backend.
    #[arg(long, default_value_t = false)]
    mock: bool,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<GatewayInner>,
}

struct GatewayInner {
    pool: SharedPool,
    role: Role,
    ticket: Option<String>,
    status: Mutex<GatewayStatus>,
    shutdown: AtomicBool,
    bounds: BoundsResponse,
    policy: CohshPolicy,
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

    let policy = CohshPolicy::from_generated();
    let pool = build_session_pool(&config, policy)?;
    let bounds = build_bounds();

    let state = AppState {
        inner: Arc::new(GatewayInner {
            pool,
            role: config.role,
            ticket: config.ticket.clone(),
            status: Mutex::new(GatewayStatus::default()),
            shutdown: AtomicBool::new(false),
            bounds,
            policy,
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
    role: Role,
    ticket: Option<String>,
    mock: bool,
}

impl GatewayConfig {
    fn from_cli(cli: Cli) -> Result<Self> {
        let mut mock = cli.mock;
        if !mock {
            if let Ok(value) = env::var("HIVE_GATEWAY_MOCK") {
                let trimmed = value.trim();
                if !trimmed.is_empty()
                    && !matches!(trimmed, "0" | "false" | "off" | "no")
                {
                    mock = true;
                }
            }
        }
        let bind = env_override(cli.bind, "127.0.0.1:8080", "HIVE_GATEWAY_BIND");
        let tcp_host = env_override(cli.tcp_host, "127.0.0.1", "COH_TCP_HOST");
        let tcp_port = env_override_u16(cli.tcp_port, COHESIX_TCP_CONSOLE_PORT, "COH_TCP_PORT");
        let mut auth_token = env_override(cli.auth_token, "changeme", "COH_AUTH_TOKEN");
        if auth_token == "changeme" {
            auth_token = env_override(auth_token, "changeme", "COHSH_AUTH_TOKEN");
        }
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
        Ok(Self {
            bind,
            tcp_host,
            tcp_port,
            auth_token,
            role,
            ticket,
            mock,
        })
    }
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

fn build_session_pool(config: &GatewayConfig, policy: CohshPolicy) -> Result<SharedPool> {
    if config.mock {
        let server = NineDoor::new();
        let factory: Arc<dyn TransportFactory> = Arc::new(move || {
            Ok(Box::new(NineDoorTransport::new(server.clone())) as Box<dyn cohsh::Transport + Send>)
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
        Ok(Box::new(SharedTcpTransport::new(inner.clone())) as Box<dyn cohsh::Transport + Send>)
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
    fn attach(&self) -> Result<()> {
        self.inner
            .pool
            .attach(self.inner.role, self.inner.ticket.as_deref())
    }

    fn ping(&self) -> Result<()> {
        let mut lease = self.inner.pool.checkout(PoolKind::Control)?;
        let session = lease.session().clone();
        let _ = lease
            .transport_mut()
            .ping(&session)
            .context("ping failed")?;
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
        let mut lease = self.inner.pool.checkout(PoolKind::Telemetry)?;
        let session = lease.session().clone();
        lease.transport_mut().list(&session, path)
    }

    fn read(&self, path: &str) -> Result<Vec<String>> {
        self.ensure_connected()?;
        let mut lease = self.inner.pool.checkout(PoolKind::Telemetry)?;
        let session = lease.session().clone();
        lease.transport_mut().read(&session, path)
    }

    fn tail(&self, path: &str) -> Result<Vec<String>> {
        self.ensure_connected()?;
        let mut lease = self.inner.pool.checkout(PoolKind::Telemetry)?;
        let session = lease.session().clone();
        lease.transport_mut().tail(&session, path)
    }

    fn write(&self, path: &str, payload: &[u8]) -> Result<()> {
        self.ensure_connected()?;
        let mut lease = self.inner.pool.checkout(PoolKind::Control)?;
        let session = lease.session().clone();
        lease.transport_mut().write(&session, path, payload)
    }

    fn bounds(&self) -> BoundsResponse {
        self.inner.bounds.clone()
    }
}

async fn meta_bounds(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    Json(state.bounds())
}

async fn openapi_yaml() -> impl axum::response::IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "application/yaml")], OPENAPI_YAML)
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
    Json(payload): Json<EchoRequest>,
) -> impl axum::response::IntoResponse {
    handle_echo(state, payload).await
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
        Ok(Err(err)) => response_err(verb, &path, err.to_string(), StatusCode::SERVICE_UNAVAILABLE),
        Err(err) => response_err(verb, &path, err.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
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
            let bytes = lines.join("\n").as_bytes().len();
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
        Ok(Err(err)) => response_err(verb, &query.path, err.to_string(), StatusCode::SERVICE_UNAVAILABLE),
        Err(err) => response_err(verb, &query.path, err.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
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
            let bytes = lines.join("\n").as_bytes().len();
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
        Ok(Err(err)) => response_err(verb, &query.path, err.to_string(), StatusCode::SERVICE_UNAVAILABLE),
        Err(err) => response_err(verb, &query.path, err.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
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
    let raw_len = raw_line.as_bytes().len();
    let trimmed = match normalise_payload(&raw_line, &payload.path) {
        Ok(value) => value,
        Err(err) => return response_err(verb, &payload.path, err, StatusCode::BAD_REQUEST),
    };
    if let Some(limit) = max_ctl_bytes(&payload.path, &state.bounds()) {
        if trimmed.as_bytes().len() > limit {
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
        Ok(Err(err)) => response_err(verb, &payload.path, err.to_string(), StatusCode::SERVICE_UNAVAILABLE),
        Err(err) => response_err(verb, &payload.path, err.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("path is required".to_owned());
    }
    if !path.starts_with('/') {
        return Err("path must be absolute".to_owned());
    }
    if path.as_bytes().len() > MAX_PATH_LEN {
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
        return Err(format!(
            "path exceeds max depth {}",
            SECURE9P_WALK_DEPTH
        ));
    }
    Ok(())
}

fn normalise_payload(raw: &str, path: &str) -> Result<String, String> {
    let trimmed = raw.trim_end_matches('\n');
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("payload must be a single line".to_owned());
    }
    let len = trimmed.as_bytes().len();
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
        PROC_SCHEDULE_SUMMARY_PATH => Some(bounds.observability.proc_schedule.summary_bytes as usize),
        PROC_SCHEDULE_QUEUE_PATH => Some(bounds.observability.proc_schedule.queue_bytes as usize),
        PROC_LEASE_SUMMARY_PATH => Some(bounds.observability.proc_lease.summary_bytes as usize),
        PROC_LEASE_ACTIVE_PATH => Some(bounds.observability.proc_lease.active_bytes as usize),
        PROC_LEASE_PREEMPTIONS_PATH => Some(bounds.observability.proc_lease.preemptions_bytes as usize),
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
