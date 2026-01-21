// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: SwarmUI backend primitives and bounded offline cache support.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! SwarmUI backend helpers: session management, transcripts, and cache handling.

mod cache;
mod hive;
mod transport;

pub use cache::{CacheError, SnapshotCache, SnapshotRecord};
pub use hive::{
    SwarmUiHiveAgent, SwarmUiHiveBatch, SwarmUiHiveBootstrap, SwarmUiHiveConfig,
    SwarmUiHiveEvent, SwarmUiHiveEventKind, SwarmUiHiveSnapshot,
};
pub use transport::{
    TcpTransport, TcpTransportError, TcpTransportFactory, TraceTransportFactory,
};

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use cohsh::client::{CohClient, TailEvent};
use cohsh::{
    tcp_debug_enabled, CohshPolicy, Session as CohshSession,
    TcpTransport as CohshTcpTransport, Transport as CohshTransport,
};
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::{normalize_ticket, parse_role, role_label, ConsoleVerb, RoleParseMode, TicketPolicy};
use cohesix_ticket::{Role, TicketClaims};
use serde::{Deserialize, Serialize};
use secure9p_codec::OpenMode;

mod generated;

/// Manifest-derived Secure9P maximum message size.
pub const SECURE9P_MSIZE: u32 = generated::SECURE9P_MSIZE;

const MAX_AUDIT_LOG: usize = 64;
const TELEMETRY_CACHE_PREFIX: &str = "telemetry:";
const FLEET_CACHE_KEY: &str = "fleet:ingest";
const NAMESPACE_CACHE_PREFIX: &str = "namespace:";
const HIVE_CACHE_PREFIX: &str = "hive:";

/// Session cache scope configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketScope {
    /// Cache sessions per ticket payload (role + ticket).
    PerTicket,
    /// Cache sessions per role, ignoring ticket payload.
    PerRole,
}

impl TicketScope {
    fn from_generated(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "per-role" => TicketScope::PerRole,
            _ => TicketScope::PerTicket,
        }
    }
}

/// Paths used by SwarmUI panels.
#[derive(Debug, Clone)]
pub struct SwarmUiPaths {
    /// Root path for worker telemetry.
    pub telemetry_root: String,
    /// Root path for ingest providers.
    pub proc_ingest_root: String,
    /// Root path for worker listings.
    pub worker_root: String,
    /// Namespace browser roots.
    pub namespace_roots: Vec<String>,
}

/// Cache configuration for offline snapshots.
#[derive(Debug, Clone)]
pub struct SwarmUiCacheConfig {
    /// Whether snapshot caching is enabled.
    pub enabled: bool,
    /// Maximum size allowed per snapshot file.
    pub max_bytes: usize,
    /// Snapshot time-to-live.
    pub ttl: Duration,
}

/// SwarmUI runtime configuration.
#[derive(Debug, Clone)]
pub struct SwarmUiConfig {
    /// Data directory used for snapshots and state.
    pub data_dir: PathBuf,
    /// Configured ticket scope.
    pub ticket_scope: TicketScope,
    /// Paths used by UI panels.
    pub paths: SwarmUiPaths,
    /// Cache configuration.
    pub cache: SwarmUiCacheConfig,
    /// Hive rendering defaults.
    pub hive: SwarmUiHiveConfig,
    /// Maximum trace replay size in bytes.
    pub trace_max_bytes: usize,
    /// Offline mode (disables network access).
    pub offline: bool,
}

impl SwarmUiConfig {
    /// Build a config from coh-rtc defaults and a runtime data directory.
    pub fn from_generated(data_dir: PathBuf) -> Self {
        let ticket_scope = TicketScope::from_generated(generated::SWARMUI_TICKET_SCOPE);
        let cache = SwarmUiCacheConfig {
            enabled: generated::SWARMUI_CACHE_ENABLED,
            max_bytes: generated::SWARMUI_CACHE_MAX_BYTES as usize,
            ttl: Duration::from_secs(generated::SWARMUI_CACHE_TTL_SECS),
        };
        let paths = SwarmUiPaths {
            telemetry_root: generated::SWARMUI_TELEMETRY_ROOT.to_owned(),
            proc_ingest_root: generated::SWARMUI_PROC_INGEST_ROOT.to_owned(),
            worker_root: generated::SWARMUI_WORKER_ROOT.to_owned(),
            namespace_roots: generated::SWARMUI_NAMESPACE_ROOTS
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        };
        let hive = SwarmUiHiveConfig {
            frame_cap_fps: generated::SWARMUI_HIVE_FRAME_CAP_FPS,
            step_ms: generated::SWARMUI_HIVE_STEP_MS,
            lod_zoom_out: generated::SWARMUI_HIVE_LOD_ZOOM_OUT,
            lod_zoom_in: generated::SWARMUI_HIVE_LOD_ZOOM_IN,
            lod_event_budget: generated::SWARMUI_HIVE_LOD_EVENT_BUDGET,
            snapshot_max_events: generated::SWARMUI_HIVE_SNAPSHOT_MAX_EVENTS,
        };
        Self {
            data_dir,
            ticket_scope,
            paths,
            cache,
            hive,
            trace_max_bytes: generated::SWARMUI_TRACE_MAX_BYTES as usize,
            offline: false,
        }
    }
}

/// A transcript returned to UI callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiTranscript {
    /// True when the operation succeeded.
    pub ok: bool,
    /// Lines to render in the UI transcript.
    pub lines: Vec<String>,
}

impl SwarmUiTranscript {
    fn ok(lines: Vec<String>) -> Self {
        Self { ok: true, lines }
    }

    fn err(lines: Vec<String>) -> Self {
        Self { ok: false, lines }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionKey {
    role: Role,
    ticket: Option<String>,
}

struct SwarmUiSession<T: cohsh_core::Secure9pTransport> {
    role: Role,
    _ticket: Option<String>,
    claims: Option<TicketClaims>,
    client: CohClient<T>,
}

/// Errors surfaced by SwarmUI backend operations.
#[derive(Debug)]
pub enum SwarmUiError {
    /// Offline mode blocks network operations.
    Offline,
    /// Ticket or role parsing failed.
    Ticket(String),
    /// Requested role is invalid.
    Role(String),
    /// Client-side role scope rejection.
    Permission(String),
    /// Invalid path or worker identifier.
    InvalidPath(String),
    /// Snapshot cache error.
    Cache(CacheError),
    /// Hive replay or snapshot errors.
    Hive(String),
    /// Generic transport error.
    Transport(String),
}

impl std::fmt::Display for SwarmUiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwarmUiError::Offline => write!(f, "offline mode prohibits network access"),
            SwarmUiError::Ticket(err) => write!(f, "{err}"),
            SwarmUiError::Role(err) => write!(f, "{err}"),
            SwarmUiError::Permission(err) => write!(f, "{err}"),
            SwarmUiError::InvalidPath(err) => write!(f, "{err}"),
            SwarmUiError::Cache(err) => write!(f, "{err}"),
            SwarmUiError::Hive(err) => write!(f, "{err}"),
            SwarmUiError::Transport(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SwarmUiError {}

impl From<CacheError> for SwarmUiError {
    fn from(err: CacheError) -> Self {
        SwarmUiError::Cache(err)
    }
}

/// Factory trait for constructing Secure9P transports.
pub trait SwarmUiTransportFactory: Send + Sync {
    /// Secure9P transport type.
    type Transport: cohsh_core::Secure9pTransport;
    /// Construct a new transport connection.
    fn connect(&self) -> Result<Self::Transport, SwarmUiError>;
}

impl SwarmUiTransportFactory for TcpTransportFactory {
    type Transport = TcpTransport;

    fn connect(&self) -> Result<Self::Transport, SwarmUiError> {
        self.build()
            .map_err(|err| SwarmUiError::Transport(err.to_string()))
    }
}

/// SwarmUI backend state and session cache.
pub struct SwarmUiBackend<F>
where
    F: SwarmUiTransportFactory,
{
    config: SwarmUiConfig,
    factory: F,
    sessions: HashMap<SessionKey, SwarmUiSession<F::Transport>>,
    hive_states: HashMap<SessionKey, hive::HiveSessionState>,
    hive_replay: Option<hive::HiveReplay>,
    audit: VecDeque<String>,
    active_tails: usize,
    cache: Option<SnapshotCache>,
}

impl<F> SwarmUiBackend<F>
where
    F: SwarmUiTransportFactory,
{
    /// Construct a new SwarmUI backend using the supplied config and transport factory.
    pub fn new(config: SwarmUiConfig, factory: F) -> Self {
        let cache = if config.cache.enabled {
            Some(SnapshotCache::new(
                config.data_dir.join("snapshots"),
                config.cache.max_bytes,
                config.cache.ttl,
            ))
        } else {
            None
        };
        Self {
            config,
            factory,
            sessions: HashMap::new(),
            hive_states: HashMap::new(),
            hive_replay: None,
            audit: VecDeque::new(),
            active_tails: 0,
            cache,
        }
    }

    /// Toggle offline mode (disables network access when enabled).
    pub fn set_offline(&mut self, offline: bool) {
        self.config.offline = offline;
    }

    /// Return a copy of the audit log buffer.
    pub fn audit_log(&self) -> Vec<String> {
        self.audit.iter().cloned().collect()
    }

    /// Return the number of active tails (used to verify no background polling).
    pub fn active_tails(&self) -> usize {
        self.active_tails
    }

    /// Attach a session for the supplied role and ticket.
    pub fn attach(&mut self, role: Role, ticket: Option<&str>) -> SwarmUiTranscript {
        if self.config.offline {
            return SwarmUiTranscript::err(vec![render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Attach.ack_label(),
                Some("reason=offline"),
            )]);
        }
        match self.ensure_session(role, ticket) {
            Ok(_) => {
                let detail = format!("role={}", role_label(role));
                SwarmUiTranscript::ok(vec![render_ack_line(
                    AckStatus::Ok,
                    ConsoleVerb::Attach.ack_label(),
                    Some(detail.as_str()),
                )])
            }
            Err(err) => {
                let detail = format!("reason={err}");
                self.record_audit(format!(
                    "audit swarmui.attach outcome=err role={} reason={err}",
                    role_label(role)
                ));
                SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Attach.ack_label(),
                    Some(detail.as_str()),
                )])
            }
        }
    }

    /// Tail telemetry for a specific worker id.
    pub fn tail_telemetry(
        &mut self,
        role: Role,
        ticket: Option<&str>,
        worker_id: &str,
    ) -> SwarmUiTranscript {
        let mut lines = Vec::new();
        let path = match telemetry_path(&self.config.paths.worker_root, worker_id) {
            Ok(path) => path,
            Err(err) => {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        };
        let cache_key = cache_key_for_path(TELEMETRY_CACHE_PREFIX, worker_id);
        if self.config.offline {
            let claims = match validate_ticket_claims(role, ticket) {
                Ok(claims) => claims,
                Err(err) => {
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Tail.ack_label(),
                        Some(format!("reason={err}").as_str()),
                    ));
                    return SwarmUiTranscript::err(lines);
                }
            };
            if let Err(err) = ensure_role_allowed(role, claims.as_ref(), &path) {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
            return self.read_cached_transcript(ConsoleVerb::Tail.ack_label(), &cache_key);
        }

        self.active_tails = self.active_tails.saturating_add(1);
        let transcript = 'tail: {
            let session = match self.session_for(role, ticket) {
                Ok(session) => session,
                Err(err) => {
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Tail.ack_label(),
                        Some(format!("reason={err}").as_str()),
                    ));
                    break 'tail SwarmUiTranscript::err(lines);
                }
            };

            if let Err(err) = ensure_role_allowed(session.role, session.claims.as_ref(), &path) {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                break 'tail SwarmUiTranscript::err(lines);
            }

            let detail = format!("path={path}");
            match session.client.tail(&path) {
                Ok(mut stream) => {
                    lines.push(render_ack_line(
                        AckStatus::Ok,
                        ConsoleVerb::Tail.ack_label(),
                        Some(detail.as_str()),
                    ));
                    while let Some(event) = stream.next() {
                        match event {
                            Ok(TailEvent::Line(line)) => lines.push(line),
                            Ok(TailEvent::End) => lines.push(END_LINE.to_owned()),
                            Err(err) => {
                                let detail = format!("path={path} reason={err}");
                                lines.push(render_ack_line(
                                    AckStatus::Err,
                                    ConsoleVerb::Tail.ack_label(),
                                    Some(detail.as_str()),
                                ));
                                break 'tail SwarmUiTranscript::err(lines);
                            }
                        }
                    }
                    break 'tail SwarmUiTranscript::ok(lines);
                }
                Err(err) => {
                    let detail = format!("path={path} reason={err}");
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Tail.ack_label(),
                        Some(detail.as_str()),
                    ));
                    break 'tail SwarmUiTranscript::err(lines);
                }
            }
        };
        self.active_tails = self.active_tails.saturating_sub(1);
        if transcript.ok {
            self.cache_transcript(&cache_key, &transcript);
        }
        transcript
    }

    /// List a namespace path (read-only).
    pub fn list_namespace(
        &mut self,
        role: Role,
        ticket: Option<&str>,
        path: &str,
    ) -> SwarmUiTranscript {
        if !self
            .config
            .paths
            .namespace_roots
            .iter()
            .any(|root| path == root)
        {
            return SwarmUiTranscript::err(vec![render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Ls.ack_label(),
                Some("reason=unsupported"),
            )]);
        }
        let cache_key = cache_key_for_path(NAMESPACE_CACHE_PREFIX, path);
        if self.config.offline {
            let claims = match validate_ticket_claims(role, ticket) {
                Ok(claims) => claims,
                Err(err) => {
                    return SwarmUiTranscript::err(vec![render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Ls.ack_label(),
                        Some(format!("reason={err}").as_str()),
                    )]);
                }
            };
            if let Err(err) = ensure_role_allowed(role, claims.as_ref(), path) {
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(format!("reason={err}").as_str()),
                )]);
            }
            return self.read_cached_transcript(ConsoleVerb::Ls.ack_label(), &cache_key);
        }
        let mut lines = Vec::new();
        let session = match self.session_for(role, ticket) {
            Ok(session) => session,
            Err(err) => {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        };
        if let Err(err) = ensure_role_allowed(session.role, session.claims.as_ref(), path) {
            lines.push(render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Ls.ack_label(),
                Some(format!("reason={err}").as_str()),
            ));
            return SwarmUiTranscript::err(lines);
        }
        match read_lines(&mut session.client, path) {
            Ok(entries) => {
                let detail = format!("path={path}");
                lines.push(render_ack_line(
                    AckStatus::Ok,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                ));
                lines.extend(entries);
                let transcript = SwarmUiTranscript::ok(lines);
                self.cache_transcript(&cache_key, &transcript);
                transcript
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                ));
                SwarmUiTranscript::err(lines)
            }
        }
    }

    /// Read ingest providers to build a fleet snapshot (text output).
    pub fn fleet_snapshot(&mut self, role: Role, ticket: Option<&str>) -> SwarmUiTranscript {
        if self.config.offline {
            let claims = match validate_ticket_claims(role, ticket) {
                Ok(claims) => claims,
                Err(err) => {
                    return SwarmUiTranscript::err(vec![render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Cat.ack_label(),
                        Some(format!("reason={err}").as_str()),
                    )]);
                }
            };
            if role != Role::Queen {
                let detail = format!("reason=permission");
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(detail.as_str()),
                )]);
            }
            if let Err(err) = ensure_role_allowed(role, claims.as_ref(), "/proc/ingest") {
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(format!("reason={err}").as_str()),
                )]);
            }
            return self.read_cached_transcript(ConsoleVerb::Cat.ack_label(), FLEET_CACHE_KEY);
        }
        let mut lines = Vec::new();
        let proc_ingest_root = self.config.paths.proc_ingest_root.clone();
        let worker_root = self.config.paths.worker_root.clone();
        let session = match self.session_for(role, ticket) {
            Ok(session) => session,
            Err(err) => {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        };
        if session.role != Role::Queen {
            lines.push(render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Cat.ack_label(),
                Some("reason=permission"),
            ));
            return SwarmUiTranscript::err(lines);
        }
        let roots = [
            "p50_ms",
            "p95_ms",
            "backpressure",
            "dropped",
            "queued",
        ];
        lines.push(render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Cat.ack_label(),
            Some("path=/proc/ingest/*"),
        ));
        for leaf in roots {
            let path = format!("{proc_ingest_root}/{leaf}");
            match read_lines(&mut session.client, &path) {
                Ok(entries) => {
                    for entry in entries {
                        lines.push(format!("{path}: {entry}"));
                    }
                }
                Err(err) => {
                    let detail = format!("path={path} reason={err}");
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Cat.ack_label(),
                        Some(detail.as_str()),
                    ));
                    return SwarmUiTranscript::err(lines);
                }
            }
        }
        match read_lines(&mut session.client, &worker_root) {
            Ok(workers) => {
                for worker in workers {
                    lines.push(format!("worker={worker}"));
                }
            }
            Err(err) => {
                let detail = format!("path={worker_root} reason={err}");
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(detail.as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        }
        let transcript = SwarmUiTranscript::ok(lines);
        self.cache_transcript(FLEET_CACHE_KEY, &transcript);
        transcript
    }

    /// Load a hive replay payload into memory.
    pub fn load_hive_replay(&mut self, payload: &[u8]) -> Result<(), SwarmUiError> {
        let replay = hive::HiveReplay::decode(payload).map_err(SwarmUiError::Hive)?;
        replay
            .snapshot()
            .validate(self.config.hive.snapshot_max_events as usize)
            .map_err(SwarmUiError::Hive)?;
        self.hive_replay = Some(replay);
        Ok(())
    }

    /// Bootstrap Live Hive with either a replay snapshot or live worker list.
    pub fn hive_bootstrap(
        &mut self,
        role: Role,
        ticket: Option<&str>,
        snapshot_key: Option<&str>,
    ) -> Result<SwarmUiHiveBootstrap, SwarmUiError> {
        if let Some(replay) = self.hive_replay.as_mut() {
            replay.reset();
            return Ok(replay.bootstrap(
                self.config.hive.clone(),
                self.config.paths.namespace_roots.clone(),
            ));
        }

        if self.config.offline {
            let key = snapshot_key.unwrap_or("demo");
            let cache_key = cache_key_for_path(HIVE_CACHE_PREFIX, key);
            let record = self.cache_read(&cache_key)?;
            let replay = hive::HiveReplay::decode(&record.payload)
                .map_err(SwarmUiError::Hive)?;
            replay
                .snapshot()
                .validate(self.config.hive.snapshot_max_events as usize)
                .map_err(SwarmUiError::Hive)?;
            let session_key = self.session_key(role, ticket);
            self.hive_states.remove(&session_key);
            let bootstrap = replay.bootstrap(
                self.config.hive.clone(),
                self.config.paths.namespace_roots.clone(),
            );
            self.hive_replay = Some(replay);
            return Ok(bootstrap);
        }

        let worker_root = self.config.paths.worker_root.clone();
        let namespace_roots = self.config.paths.namespace_roots.clone();
        let hive_config = self.config.hive.clone();
        let key = self.session_key(role, ticket);
        let subject = if role == Role::Queen {
            None
        } else {
            let claims = validate_ticket_claims(role, ticket)?;
            let subject = claims
                .as_ref()
                .and_then(|claims| claims.subject.as_deref())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    SwarmUiError::Permission("ticket subject identity missing".to_owned())
                })?;
            let path = telemetry_path(&worker_root, subject)?;
            ensure_role_allowed(role, claims.as_ref(), &path)?;
            Some(subject.to_owned())
        };

        let mut fids = Vec::new();
        if let Some(mut state) = self.hive_states.remove(&key) {
            fids = state.take_fids();
        }

        let workers = {
            let session = self.session_for(role, ticket)?;
            for fid in fids {
                let _ = session.client.clunk(fid);
            }
            if role == Role::Queen {
                let mut workers = list_workers(&mut session.client, &worker_root)?;
                workers.sort();
                workers
            } else {
                vec![subject.expect("subject already validated")]
            }
        };

        let mut agents = Vec::new();
        agents.push(SwarmUiHiveAgent {
            id: "queen".to_owned(),
            role: "queen".to_owned(),
            namespace: "/queen".to_owned(),
        });
        for worker in &workers {
            agents.push(SwarmUiHiveAgent {
                id: worker.to_owned(),
                role: "worker".to_owned(),
                namespace: format!("{}/{}", worker_root, worker),
            });
        }

        self.hive_states
            .insert(key, hive::HiveSessionState::new(workers));
        self.hive_replay = None;

        Ok(SwarmUiHiveBootstrap {
            agents,
            namespace_roots,
            hive: hive_config,
            replay: false,
        })
    }

    /// Poll Live Hive event deltas.
    pub fn hive_poll(
        &mut self,
        role: Role,
        ticket: Option<&str>,
    ) -> Result<SwarmUiHiveBatch, SwarmUiError> {
        if let Some(replay) = self.hive_replay.as_mut() {
            let max_events = self
                .config
                .hive
                .lod_event_budget
                .min(self.config.hive.snapshot_max_events) as usize;
            return Ok(replay.next_batch(
                max_events,
                self.config.hive.lod_event_budget,
            ));
        }
        if self.config.offline {
            return Err(SwarmUiError::Offline);
        }
        let worker_root = self.config.paths.worker_root.clone();
        let hive_config = self.config.hive.clone();
        let key = self.session_key(role, ticket);
        let mut state = self
            .hive_states
            .remove(&key)
            .ok_or_else(|| SwarmUiError::Hive("hive not bootstrapped".to_owned()))?;
        let ingest_result = {
            let session = self.session_for(role, ticket)?;
            state.ingest(
                &mut session.client,
                &worker_root,
                SECURE9P_MSIZE,
                &hive_config,
            )
        };
        self.hive_states.insert(key.clone(), state);
        ingest_result?;
        let state = self.hive_states.get_mut(&key).expect("hive state");
        let max_events = hive_config.lod_event_budget as usize;
        let events = state.drain(max_events);
        let backlog = state.queue_len();
        let pressure = if hive_config.lod_event_budget == 0 {
            0.0
        } else {
            backlog as f32 / hive_config.lod_event_budget as f32
        };
        Ok(SwarmUiHiveBatch {
            events,
            pressure,
            backlog,
            dropped: state.dropped(),
            done: false,
        })
    }

    /// Reset Live Hive session state and close any open telemetry cursors.
    pub fn hive_reset(
        &mut self,
        role: Role,
        ticket: Option<&str>,
    ) -> Result<(), SwarmUiError> {
        if let Some(replay) = self.hive_replay.as_mut() {
            replay.reset();
            return Ok(());
        }
        if self.config.offline {
            return Ok(());
        }
        let key = self.session_key(role, ticket);
        let mut fids = Vec::new();
        if let Some(mut state) = self.hive_states.remove(&key) {
            fids = state.take_fids();
        }
        if fids.is_empty() {
            return Ok(());
        }
        let session = self.session_for(role, ticket)?;
        for fid in fids {
            let _ = session.client.clunk(fid);
        }
        Ok(())
    }

    /// Cache a CBOR snapshot payload.
    pub fn cache_write(&mut self, key: &str, payload: &[u8]) -> Result<SnapshotRecord, SwarmUiError> {
        if self.config.offline {
            return Err(SwarmUiError::Offline);
        }
        let cache = self.cache.as_ref().ok_or_else(|| {
            SwarmUiError::Cache(CacheError::Disabled)
        })?;
        let record = cache.write(key, payload)?;
        Ok(record)
    }

    /// Read a cached CBOR snapshot payload.
    pub fn cache_read(&self, key: &str) -> Result<SnapshotRecord, SwarmUiError> {
        let cache = self.cache.as_ref().ok_or_else(|| {
            SwarmUiError::Cache(CacheError::Disabled)
        })?;
        Ok(cache.read(key)?)
    }

    fn session_for(&mut self, role: Role, ticket: Option<&str>) -> Result<&mut SwarmUiSession<F::Transport>, SwarmUiError> {
        let key = self.session_key(role, ticket);
        if !self.sessions.contains_key(&key) {
            self.ensure_session(role, ticket)?;
        }
        self.sessions
            .get_mut(&key)
            .ok_or_else(|| SwarmUiError::Transport("session unavailable".to_owned()))
    }

    fn ensure_session(&mut self, role: Role, ticket: Option<&str>) -> Result<(), SwarmUiError> {
        let key = self.session_key(role, ticket);
        let ticket_check = normalize_ticket(role, ticket, TicketPolicy::ninedoor()).map_err(|err| {
            SwarmUiError::Ticket(map_ticket_error(role, err))
        })?;
        let claims = ticket_check.claims.clone();
        let transport = self.factory.connect()?;
        let client = CohClient::connect(transport, role, ticket_check.ticket).map_err(|err| {
            SwarmUiError::Transport(err.to_string())
        })?;
        let session = SwarmUiSession {
            role,
            _ticket: ticket.map(str::to_owned),
            claims,
            client,
        };
        self.sessions.insert(key, session);
        Ok(())
    }

    fn session_key(&self, role: Role, ticket: Option<&str>) -> SessionKey {
        match self.config.ticket_scope {
            TicketScope::PerRole => SessionKey { role, ticket: None },
            TicketScope::PerTicket => SessionKey {
                role,
                ticket: ticket.map(str::to_owned),
            },
        }
    }

    fn record_audit(&mut self, entry: String) {
        log::info!("{entry}");
        if self.audit.len() >= MAX_AUDIT_LOG {
            let _ = self.audit.pop_front();
        }
        self.audit.push_back(entry);
    }

    fn cache_transcript(&mut self, key: &str, transcript: &SwarmUiTranscript) {
        if self.config.offline || !transcript.ok {
            return;
        }
        let (result, encode_err) = {
            let Some(cache) = self.cache.as_ref() else {
                return;
            };
            match serde_cbor::to_vec(transcript) {
                Ok(payload) => (Some(cache.write(key, &payload)), None),
                Err(err) => (None, Some(err.to_string())),
            }
        };
        if let Some(err) = encode_err {
            self.record_audit(format!(
                "audit swarmui.cache.encode outcome=err key={key} reason={err}"
            ));
        }
        if let Some(Err(err)) = result {
            self.record_audit(format!(
                "audit swarmui.cache.write outcome=err key={key} reason={err}"
            ));
        }
    }

    fn read_cached_transcript(&mut self, verb: &str, key: &str) -> SwarmUiTranscript {
        let read_result = {
            let Some(cache) = self.cache.as_ref() else {
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    verb,
                    Some("reason=cache-disabled"),
                )]);
            };
            cache.read(key)
        };
        let record = match read_result {
            Ok(record) => record,
            Err(err) => {
                self.record_audit(format!(
                    "audit swarmui.cache.read outcome=err key={key} reason={err}"
                ));
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    verb,
                    Some(format!("reason={err}").as_str()),
                )]);
            }
        };

        match serde_cbor::from_slice::<SwarmUiTranscript>(&record.payload) {
            Ok(transcript) => transcript,
            Err(err) => {
                let detail = format!("reason=cache-decode:{err}");
                self.record_audit(format!(
                    "audit swarmui.cache.decode outcome=err key={key} reason={err}"
                ));
                SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    verb,
                    Some(detail.as_str()),
                )])
            }
        }
    }
}

struct SwarmUiConsoleSession {
    role: Role,
    claims: Option<TicketClaims>,
    session: CohshSession,
}

/// SwarmUI backend that speaks the Cohesix TCP console transport.
pub struct SwarmUiConsoleBackend {
    config: SwarmUiConfig,
    transport: CohshTcpTransport,
    session: Option<CohshSession>,
    session_role: Option<Role>,
    session_ticket: Option<String>,
    session_claims: Option<TicketClaims>,
    hive_states: HashMap<SessionKey, hive::ConsoleHiveSessionState>,
    hive_replay: Option<hive::HiveReplay>,
    audit: VecDeque<String>,
    active_tails: usize,
    cache: Option<SnapshotCache>,
}

impl SwarmUiConsoleBackend {
    /// Construct a new SwarmUI backend using the TCP console transport.
    pub fn new(
        config: SwarmUiConfig,
        host: impl Into<String>,
        port: u16,
        auth_token: impl Into<String>,
    ) -> Self {
        let cache = if config.cache.enabled {
            Some(SnapshotCache::new(
                config.data_dir.join("snapshots"),
                config.cache.max_bytes,
                config.cache.ttl,
            ))
        } else {
            None
        };
        let policy = CohshPolicy::from_generated();
        let transport = CohshTcpTransport::new(host.into(), port)
            .with_retry_policy(policy.retry)
            .with_heartbeat_interval(Duration::from_millis(policy.heartbeat.interval_ms))
            .with_auth_token(auth_token)
            .with_tcp_debug(tcp_debug_enabled());
        Self {
            config,
            transport,
            session: None,
            session_role: None,
            session_ticket: None,
            session_claims: None,
            hive_states: HashMap::new(),
            hive_replay: None,
            audit: VecDeque::new(),
            active_tails: 0,
            cache,
        }
    }

    /// Toggle offline mode (disables network access when enabled).
    pub fn set_offline(&mut self, offline: bool) {
        self.config.offline = offline;
    }

    /// Return a copy of the audit log buffer.
    pub fn audit_log(&self) -> Vec<String> {
        self.audit.iter().cloned().collect()
    }

    /// Return the number of active tails (used to verify no background polling).
    pub fn active_tails(&self) -> usize {
        self.active_tails
    }

    /// Attach a session for the supplied role and ticket.
    pub fn attach(&mut self, role: Role, ticket: Option<&str>) -> SwarmUiTranscript {
        if self.config.offline {
            return SwarmUiTranscript::err(vec![render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Attach.ack_label(),
                Some("reason=offline"),
            )]);
        }
        match self.ensure_session(role, ticket) {
            Ok(_) => {
                let detail = format!("role={}", role_label(role));
                SwarmUiTranscript::ok(vec![render_ack_line(
                    AckStatus::Ok,
                    ConsoleVerb::Attach.ack_label(),
                    Some(detail.as_str()),
                )])
            }
            Err(err) => {
                let detail = format!("reason={err}");
                self.record_audit(format!(
                    "audit swarmui.attach outcome=err role={} reason={err}",
                    role_label(role)
                ));
                SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Attach.ack_label(),
                    Some(detail.as_str()),
                )])
            }
        }
    }

    /// Tail telemetry for a specific worker id.
    pub fn tail_telemetry(
        &mut self,
        role: Role,
        ticket: Option<&str>,
        worker_id: &str,
    ) -> SwarmUiTranscript {
        let mut lines = Vec::new();
        let path = match telemetry_path(&self.config.paths.worker_root, worker_id) {
            Ok(path) => path,
            Err(err) => {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        };
        let cache_key = cache_key_for_path(TELEMETRY_CACHE_PREFIX, worker_id);
        if self.config.offline {
            let claims = match validate_ticket_claims_with_policy(role, ticket, TicketPolicy::tcp())
            {
                Ok(claims) => claims,
                Err(err) => {
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Tail.ack_label(),
                        Some(format!("reason={err}").as_str()),
                    ));
                    return SwarmUiTranscript::err(lines);
                }
            };
            if let Err(err) = ensure_role_allowed(role, claims.as_ref(), &path) {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
            return self.read_cached_transcript(ConsoleVerb::Tail.ack_label(), &cache_key);
        }

        self.active_tails = self.active_tails.saturating_add(1);
        let transcript = 'tail: {
            let session = match self.session_for(role, ticket) {
                Ok(session) => session,
                Err(err) => {
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Tail.ack_label(),
                        Some(format!("reason={err}").as_str()),
                    ));
                    break 'tail SwarmUiTranscript::err(lines);
                }
            };

            if let Err(err) = ensure_role_allowed(session.role, session.claims.as_ref(), &path) {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                break 'tail SwarmUiTranscript::err(lines);
            }

            let detail = format!("path={path}");
            match self.transport.tail(&session.session, &path) {
                Ok(payload_lines) => {
                    let _ = self.transport.drain_acknowledgements();
                    lines.push(render_ack_line(
                        AckStatus::Ok,
                        ConsoleVerb::Tail.ack_label(),
                        Some(detail.as_str()),
                    ));
                    lines.extend(payload_lines);
                    lines.push(END_LINE.to_owned());
                    break 'tail SwarmUiTranscript::ok(lines);
                }
                Err(err) => {
                    let _ = self.transport.drain_acknowledgements();
                    let detail = format!("path={path} reason={err}");
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Tail.ack_label(),
                        Some(detail.as_str()),
                    ));
                    break 'tail SwarmUiTranscript::err(lines);
                }
            }
        };
        self.active_tails = self.active_tails.saturating_sub(1);
        if transcript.ok {
            self.cache_transcript(&cache_key, &transcript);
        }
        transcript
    }

    /// List a namespace path (read-only).
    pub fn list_namespace(
        &mut self,
        role: Role,
        ticket: Option<&str>,
        path: &str,
    ) -> SwarmUiTranscript {
        if !self
            .config
            .paths
            .namespace_roots
            .iter()
            .any(|root| path == root)
        {
            return SwarmUiTranscript::err(vec![render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Ls.ack_label(),
                Some("reason=unsupported"),
            )]);
        }
        let cache_key = cache_key_for_path(NAMESPACE_CACHE_PREFIX, path);
        if self.config.offline {
            let claims =
                match validate_ticket_claims_with_policy(role, ticket, TicketPolicy::tcp()) {
                    Ok(claims) => claims,
                    Err(err) => {
                        return SwarmUiTranscript::err(vec![render_ack_line(
                            AckStatus::Err,
                            ConsoleVerb::Ls.ack_label(),
                            Some(format!("reason={err}").as_str()),
                        )]);
                    }
                };
            if let Err(err) = ensure_role_allowed(role, claims.as_ref(), path) {
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(format!("reason={err}").as_str()),
                )]);
            }
            return self.read_cached_transcript(ConsoleVerb::Ls.ack_label(), &cache_key);
        }
        let mut lines = Vec::new();
        let session = match self.session_for(role, ticket) {
            Ok(session) => session,
            Err(err) => {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        };
        if let Err(err) = ensure_role_allowed(session.role, session.claims.as_ref(), path) {
            lines.push(render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Ls.ack_label(),
                Some(format!("reason={err}").as_str()),
            ));
            return SwarmUiTranscript::err(lines);
        }
        match list_entries_console(&mut self.transport, &session.session, path) {
            Ok(entries) => {
                let detail = format!("path={path}");
                lines.push(render_ack_line(
                    AckStatus::Ok,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                ));
                lines.extend(entries);
                let transcript = SwarmUiTranscript::ok(lines);
                self.cache_transcript(&cache_key, &transcript);
                transcript
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                ));
                SwarmUiTranscript::err(lines)
            }
        }
    }

    /// Read ingest providers to build a fleet snapshot (text output).
    pub fn fleet_snapshot(&mut self, role: Role, ticket: Option<&str>) -> SwarmUiTranscript {
        if self.config.offline {
            let claims =
                match validate_ticket_claims_with_policy(role, ticket, TicketPolicy::tcp()) {
                    Ok(claims) => claims,
                    Err(err) => {
                        return SwarmUiTranscript::err(vec![render_ack_line(
                            AckStatus::Err,
                            ConsoleVerb::Cat.ack_label(),
                            Some(format!("reason={err}").as_str()),
                        )]);
                    }
                };
            if role != Role::Queen {
                let detail = format!("reason=permission");
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(detail.as_str()),
                )]);
            }
            if let Err(err) = ensure_role_allowed(role, claims.as_ref(), "/proc/ingest") {
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(format!("reason={err}").as_str()),
                )]);
            }
            return self.read_cached_transcript(ConsoleVerb::Cat.ack_label(), FLEET_CACHE_KEY);
        }
        let mut lines = Vec::new();
        let proc_ingest_root = self.config.paths.proc_ingest_root.clone();
        let worker_root = self.config.paths.worker_root.clone();
        let session = match self.session_for(role, ticket) {
            Ok(session) => session,
            Err(err) => {
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(format!("reason={err}").as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        };
        if session.role != Role::Queen {
            lines.push(render_ack_line(
                AckStatus::Err,
                ConsoleVerb::Cat.ack_label(),
                Some("reason=permission"),
            ));
            return SwarmUiTranscript::err(lines);
        }
        let roots = [
            "p50_ms",
            "p95_ms",
            "backpressure",
            "dropped",
            "queued",
        ];
        lines.push(render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Cat.ack_label(),
            Some("path=/proc/ingest/*"),
        ));
        for leaf in roots {
            let path = format!("{proc_ingest_root}/{leaf}");
            match read_lines_console(&mut self.transport, &session.session, &path) {
                Ok(entries) => {
                    for entry in entries {
                        lines.push(format!("{path}: {entry}"));
                    }
                }
                Err(err) => {
                    let detail = format!("path={path} reason={err}");
                    lines.push(render_ack_line(
                        AckStatus::Err,
                        ConsoleVerb::Cat.ack_label(),
                        Some(detail.as_str()),
                    ));
                    return SwarmUiTranscript::err(lines);
                }
            }
        }
        match list_workers_console(&mut self.transport, &session.session, &worker_root) {
            Ok(workers) => {
                for worker in workers {
                    lines.push(format!("worker={worker}"));
                }
            }
            Err(err) => {
                let detail = format!("path={worker_root} reason={err}");
                lines.push(render_ack_line(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(detail.as_str()),
                ));
                return SwarmUiTranscript::err(lines);
            }
        }
        let transcript = SwarmUiTranscript::ok(lines);
        self.cache_transcript(FLEET_CACHE_KEY, &transcript);
        transcript
    }

    /// Load a hive replay payload into memory.
    pub fn load_hive_replay(&mut self, payload: &[u8]) -> Result<(), SwarmUiError> {
        let replay = hive::HiveReplay::decode(payload).map_err(SwarmUiError::Hive)?;
        replay
            .snapshot()
            .validate(self.config.hive.snapshot_max_events as usize)
            .map_err(SwarmUiError::Hive)?;
        self.hive_replay = Some(replay);
        Ok(())
    }

    /// Bootstrap Live Hive with either a replay snapshot or live worker list.
    pub fn hive_bootstrap(
        &mut self,
        role: Role,
        ticket: Option<&str>,
        snapshot_key: Option<&str>,
    ) -> Result<SwarmUiHiveBootstrap, SwarmUiError> {
        if let Some(replay) = self.hive_replay.as_mut() {
            replay.reset();
            return Ok(replay.bootstrap(
                self.config.hive.clone(),
                self.config.paths.namespace_roots.clone(),
            ));
        }

        if self.config.offline {
            let key = snapshot_key.unwrap_or("demo");
            let cache_key = cache_key_for_path(HIVE_CACHE_PREFIX, key);
            let record = self.cache_read(&cache_key)?;
            let replay = hive::HiveReplay::decode(&record.payload)
                .map_err(SwarmUiError::Hive)?;
            replay
                .snapshot()
                .validate(self.config.hive.snapshot_max_events as usize)
                .map_err(SwarmUiError::Hive)?;
            let session_key = self.session_key(role, ticket);
            self.hive_states.remove(&session_key);
            let bootstrap = replay.bootstrap(
                self.config.hive.clone(),
                self.config.paths.namespace_roots.clone(),
            );
            self.hive_replay = Some(replay);
            return Ok(bootstrap);
        }

        let worker_root = self.config.paths.worker_root.clone();
        let namespace_roots = self.config.paths.namespace_roots.clone();
        let hive_config = self.config.hive.clone();
        let key = self.session_key(role, ticket);
        let subject = if role == Role::Queen {
            None
        } else {
            let claims = validate_ticket_claims_with_policy(role, ticket, TicketPolicy::tcp())?;
            let subject = claims
                .as_ref()
                .and_then(|claims| claims.subject.as_deref())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    SwarmUiError::Permission("ticket subject identity missing".to_owned())
                })?;
            let path = telemetry_path(&worker_root, subject)?;
            ensure_role_allowed(role, claims.as_ref(), &path)?;
            Some(subject.to_owned())
        };

        let workers = {
            let session = self.session_for(role, ticket)?;
            if role == Role::Queen {
                let mut workers =
                    list_workers_console(&mut self.transport, &session.session, &worker_root)?;
                workers.sort();
                workers
            } else {
                vec![subject.expect("subject already validated")]
            }
        };

        let mut agents = Vec::new();
        agents.push(SwarmUiHiveAgent {
            id: "queen".to_owned(),
            role: "queen".to_owned(),
            namespace: "/queen".to_owned(),
        });
        for worker in &workers {
            agents.push(SwarmUiHiveAgent {
                id: worker.to_owned(),
                role: "worker".to_owned(),
                namespace: format!("{}/{}", worker_root, worker),
            });
        }

        self.hive_states
            .insert(key, hive::ConsoleHiveSessionState::new(workers));
        self.hive_replay = None;

        Ok(SwarmUiHiveBootstrap {
            agents,
            namespace_roots,
            hive: hive_config,
            replay: false,
        })
    }

    /// Poll Live Hive event deltas.
    pub fn hive_poll(
        &mut self,
        role: Role,
        ticket: Option<&str>,
    ) -> Result<SwarmUiHiveBatch, SwarmUiError> {
        if let Some(replay) = self.hive_replay.as_mut() {
            let max_events = self
                .config
                .hive
                .lod_event_budget
                .min(self.config.hive.snapshot_max_events) as usize;
            return Ok(replay.next_batch(
                max_events,
                self.config.hive.lod_event_budget,
            ));
        }
        if self.config.offline {
            return Err(SwarmUiError::Offline);
        }
        let worker_root = self.config.paths.worker_root.clone();
        let hive_config = self.config.hive.clone();
        let key = self.session_key(role, ticket);
        let mut state = self
            .hive_states
            .remove(&key)
            .ok_or_else(|| SwarmUiError::Hive("hive not bootstrapped".to_owned()))?;
        let ingest_result = {
            let session = self.session_for(role, ticket)?;
            state.ingest(
                &mut self.transport,
                &session.session,
                &worker_root,
                &hive_config,
            )
        };
        self.hive_states.insert(key.clone(), state);
        ingest_result?;
        let state = self.hive_states.get_mut(&key).expect("hive state");
        let max_events = hive_config.lod_event_budget as usize;
        let events = state.drain(max_events);
        let backlog = state.queue_len();
        let pressure = if hive_config.lod_event_budget == 0 {
            0.0
        } else {
            backlog as f32 / hive_config.lod_event_budget as f32
        };
        Ok(SwarmUiHiveBatch {
            events,
            pressure,
            backlog,
            dropped: state.dropped(),
            done: false,
        })
    }

    /// Reset Live Hive session state and close any open telemetry cursors.
    pub fn hive_reset(
        &mut self,
        role: Role,
        ticket: Option<&str>,
    ) -> Result<(), SwarmUiError> {
        if let Some(replay) = self.hive_replay.as_mut() {
            replay.reset();
            return Ok(());
        }
        if self.config.offline {
            return Ok(());
        }
        let key = self.session_key(role, ticket);
        if let Some(mut state) = self.hive_states.remove(&key) {
            state.reset();
        }
        Ok(())
    }

    /// Cache a CBOR snapshot payload.
    pub fn cache_write(&mut self, key: &str, payload: &[u8]) -> Result<SnapshotRecord, SwarmUiError> {
        if self.config.offline {
            return Err(SwarmUiError::Offline);
        }
        let cache = self.cache.as_ref().ok_or_else(|| {
            SwarmUiError::Cache(CacheError::Disabled)
        })?;
        let record = cache.write(key, payload)?;
        Ok(record)
    }

    /// Read a cached CBOR snapshot payload.
    pub fn cache_read(&self, key: &str) -> Result<SnapshotRecord, SwarmUiError> {
        let cache = self.cache.as_ref().ok_or_else(|| {
            SwarmUiError::Cache(CacheError::Disabled)
        })?;
        Ok(cache.read(key)?)
    }

    fn session_for(
        &mut self,
        role: Role,
        ticket: Option<&str>,
    ) -> Result<SwarmUiConsoleSession, SwarmUiError> {
        self.ensure_session(role, ticket)?;
        let session = self
            .session
            .clone()
            .ok_or_else(|| SwarmUiError::Transport("session unavailable".to_owned()))?;
        Ok(SwarmUiConsoleSession {
            role,
            claims: self.session_claims.clone(),
            session,
        })
    }

    fn ensure_session(&mut self, role: Role, ticket: Option<&str>) -> Result<(), SwarmUiError> {
        let ticket_check = normalize_ticket(role, ticket, TicketPolicy::tcp())
            .map_err(|err| SwarmUiError::Ticket(map_ticket_error(role, err)))?;
        let claims = ticket_check.claims.clone();
        let ticket_payload = ticket_check.ticket.map(str::to_owned);
        let needs_attach = self.session_role != Some(role)
            || self
                .session_ticket
                .as_deref()
                .map(|value| value.trim())
                != ticket_check.ticket;
        if needs_attach {
            let session = self
                .transport
                .attach(role, ticket_check.ticket)
                .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
            let _ = self.transport.drain_acknowledgements();
            self.session = Some(session);
            self.session_role = Some(role);
            self.session_ticket = ticket_payload;
        }
        self.session_claims = claims;
        Ok(())
    }

    fn session_key(&self, role: Role, ticket: Option<&str>) -> SessionKey {
        match self.config.ticket_scope {
            TicketScope::PerRole => SessionKey { role, ticket: None },
            TicketScope::PerTicket => SessionKey {
                role,
                ticket: ticket.map(str::to_owned),
            },
        }
    }

    fn record_audit(&mut self, entry: String) {
        log::info!("{entry}");
        if self.audit.len() >= MAX_AUDIT_LOG {
            let _ = self.audit.pop_front();
        }
        self.audit.push_back(entry);
    }

    fn cache_transcript(&mut self, key: &str, transcript: &SwarmUiTranscript) {
        if self.config.offline || !transcript.ok {
            return;
        }
        let (result, encode_err) = {
            let Some(cache) = self.cache.as_ref() else {
                return;
            };
            match serde_cbor::to_vec(transcript) {
                Ok(payload) => (Some(cache.write(key, &payload)), None),
                Err(err) => (None, Some(err.to_string())),
            }
        };
        if let Some(err) = encode_err {
            self.record_audit(format!(
                "audit swarmui.cache.encode outcome=err key={key} reason={err}"
            ));
        }
        if let Some(Err(err)) = result {
            self.record_audit(format!(
                "audit swarmui.cache.write outcome=err key={key} reason={err}"
            ));
        }
    }

    fn read_cached_transcript(&mut self, verb: &str, key: &str) -> SwarmUiTranscript {
        let read_result = {
            let Some(cache) = self.cache.as_ref() else {
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    verb,
                    Some("reason=cache-disabled"),
                )]);
            };
            cache.read(key)
        };
        let record = match read_result {
            Ok(record) => record,
            Err(err) => {
                self.record_audit(format!(
                    "audit swarmui.cache.read outcome=err key={key} reason={err}"
                ));
                return SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    verb,
                    Some(format!("reason={err}").as_str()),
                )]);
            }
        };

        match serde_cbor::from_slice::<SwarmUiTranscript>(&record.payload) {
            Ok(transcript) => transcript,
            Err(err) => {
                let detail = format!("reason=cache-decode:{err}");
                self.record_audit(format!(
                    "audit swarmui.cache.decode outcome=err key={key} reason={err}"
                ));
                SwarmUiTranscript::err(vec![render_ack_line(
                    AckStatus::Err,
                    verb,
                    Some(detail.as_str()),
                )])
            }
        }
    }
}

/// Parse a role label into a Cohesix role.
pub fn parse_role_label(input: &str) -> Result<Role, SwarmUiError> {
    parse_role(input, RoleParseMode::AllowWorkerAlias)
        .ok_or_else(|| SwarmUiError::Role(format!("unknown role '{input}'")))
}

fn telemetry_path(root: &str, worker_id: &str) -> Result<String, SwarmUiError> {
    let trimmed = worker_id.trim();
    if trimmed.is_empty() || trimmed.contains('/') || trimmed == "." || trimmed == ".." {
        return Err(SwarmUiError::InvalidPath(format!(
            "invalid worker id '{worker_id}'"
        )));
    }
    Ok(format!("{root}/{trimmed}/telemetry"))
}

fn cache_key_for_path(prefix: &str, path: &str) -> String {
    let trimmed = path.trim().trim_start_matches('/');
    let safe = trimmed.replace('/', ".");
    format!("{prefix}{safe}")
}

fn render_ack_line(status: AckStatus, verb: &str, detail: Option<&str>) -> String {
    let ack = AckLine { status, verb, detail };
    let mut line = String::new();
    render_ack(&mut line, &ack).expect("render ack");
    line
}

fn read_lines<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
) -> Result<Vec<String>, SwarmUiError> {
    let fid = client
        .open(path, OpenMode::read_only())
        .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
    let mut offset = 0u64;
    let mut buffer = Vec::new();
    let msize = SECURE9P_MSIZE;
    loop {
        let chunk = client
            .read(fid, offset, msize)
            .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
        if chunk.is_empty() {
            break;
        }
        offset = offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| SwarmUiError::Transport("offset overflow".to_owned()))?;
        buffer.extend_from_slice(&chunk);
        if chunk.len() < msize as usize {
            break;
        }
    }
    let _ = client.clunk(fid);
    let text = String::from_utf8(buffer)
        .map_err(|_| SwarmUiError::Transport("payload is not valid UTF-8".to_owned()))?;
    Ok(text.lines().map(|line| line.to_owned()).collect())
}

fn read_lines_console<T: CohshTransport>(
    transport: &mut T,
    session: &CohshSession,
    path: &str,
) -> Result<Vec<String>, SwarmUiError> {
    let lines = transport
        .read(session, path)
        .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
    let _ = transport.drain_acknowledgements();
    Ok(lines)
}

fn list_entries_console<T: CohshTransport>(
    transport: &mut T,
    session: &CohshSession,
    path: &str,
) -> Result<Vec<String>, SwarmUiError> {
    let entries = transport
        .list(session, path)
        .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
    let _ = transport.drain_acknowledgements();
    Ok(entries)
}

fn list_workers<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    worker_root: &str,
) -> Result<Vec<String>, SwarmUiError> {
    let entries = read_lines(client, worker_root)?;
    let mut workers = Vec::new();
    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        workers.push(trimmed.to_owned());
    }
    Ok(workers)
}

fn list_workers_console<T: CohshTransport>(
    transport: &mut T,
    session: &CohshSession,
    worker_root: &str,
) -> Result<Vec<String>, SwarmUiError> {
    let entries = list_entries_console(transport, session, worker_root)?;
    let mut workers = Vec::new();
    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        workers.push(trimmed.to_owned());
    }
    Ok(workers)
}

fn ensure_role_allowed(
    role: Role,
    claims: Option<&TicketClaims>,
    path: &str,
) -> Result<(), SwarmUiError> {
    if role == Role::Queen {
        return Ok(());
    }
    let subject = claims
        .and_then(|claims| claims.subject.as_deref())
        .filter(|value| !value.is_empty());
    let Some(subject) = subject else {
        return Err(SwarmUiError::Permission(
            "ticket subject identity missing".to_owned(),
        ));
    };
    let allowed = path == "/proc/boot"
        || path == "/log/queen.log"
        || match path.trim_start_matches('/').split('/').collect::<Vec<_>>().as_slice() {
            ["worker", worker_id, "telemetry"] => *worker_id == subject,
            _ => false,
        };
    if allowed {
        Ok(())
    } else {
        Err(SwarmUiError::Permission(format!(
            "role {:?} cannot access {path}",
            role
        )))
    }
}

fn map_ticket_error(role: Role, err: cohsh_core::TicketError) -> String {
    match err {
        cohsh_core::TicketError::Missing => format!(
            "role {:?} requires a capability ticket containing an identity",
            role
        ),
        cohsh_core::TicketError::TooLong(max) => format!("ticket payload exceeds {max} bytes"),
        cohsh_core::TicketError::Invalid(inner) => format!("invalid ticket: {inner}"),
        cohsh_core::TicketError::RoleMismatch { expected, found } => format!(
            "ticket role {:?} does not match requested role {:?}",
            found, expected
        ),
        cohsh_core::TicketError::MissingSubject => {
            format!("ticket is missing required subject identity for role {:?}", role)
        }
    }
}

fn validate_ticket_claims(
    role: Role,
    ticket: Option<&str>,
) -> Result<Option<TicketClaims>, SwarmUiError> {
    validate_ticket_claims_with_policy(role, ticket, TicketPolicy::ninedoor())
}

fn validate_ticket_claims_with_policy(
    role: Role,
    ticket: Option<&str>,
    policy: TicketPolicy,
) -> Result<Option<TicketClaims>, SwarmUiError> {
    normalize_ticket(role, ticket, policy)
        .map(|check| check.claims)
        .map_err(|err| SwarmUiError::Ticket(map_ticket_error(role, err)))
}
