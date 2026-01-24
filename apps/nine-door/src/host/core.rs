// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Core NineDoor Secure9P server state machine and namespace plumbing.
// Author: Lukas Bower

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
use cohesix_ticket::{BudgetSpec, Role, TicketKey, TicketToken, TicketVerb};
use gpu_bridge_host::{status_entry, GpuNamespaceSnapshot};
use log::{debug, info, trace};
use secure9p_codec::{
    Codec, ErrorCode, OpenMode, Qid, Request, RequestBody, Response, ResponseBody, SessionId,
    MAX_MSIZE, VERSION,
};
use secure9p_core::{
    FidError, QueueDepth, QueueError, SessionLimits, ShardedFidTable, TagError, TagWindow,
};
use trace_model::TraceLevel;
use worker_gpu::{GpuLease as WorkerGpuLease, JobDescriptor};

use super::control::{
    format_host_write_audit, format_policy_action_audit, format_policy_gate_allow,
    format_policy_gate_deny, host_write_target, BudgetCommand, HostWriteOutcome, HostWriteTarget,
    KillCommand, QueenCommand, SpawnCommand, SpawnTarget,
};
use super::audit::{
    AuditConfig, AuditStore, ControlOutcome, PolicyActionDecision as AuditPolicyActionDecision,
    PolicyGateDecision as AuditPolicyGateDecision, ReplayWindowError,
};
use super::CasConfig;
use super::namespace::{
    AuditNamespaceConfig, HostNamespaceConfig, Namespace, PolicyNamespaceConfig, ReplayNamespaceConfig,
    ShardLayout, SidecarKind, SidecarNamespaceConfig, SidecarScope,
};
use super::policy::{
    PolicyActionAudit, PolicyConfig, PolicyDecision, PolicyGateAllowance, PolicyGateDecision,
    PolicyGateDenial, PolicyPreflightPayloads, PolicyStore,
};
use super::namespace::UiProviderInfo;
use super::pipeline::{Pipeline, PipelineConfig, PipelineMetrics};
use super::observe::{ObserveConfig, ObserveState};
use super::replay::ReplayState;
use super::security::{CursorCheck, TicketDeny, TicketLimits, TicketUsage};
use super::telemetry::{
    TelemetryAuditLevel, TelemetryConfig, TelemetryIngestConfig, TelemetryManifestStore,
};
use super::ui::{UiProviderConfig, UiVariant, UI_MAX_READ_BYTES};
use super::{Clock, NineDoorError};

// New server core implementation and access policy are defined below.

/// Internal server state shared between connections.
pub(crate) struct ServerCore {
    codec: Codec,
    control: ControlPlane,
    next_session: u64,
    sessions: HashMap<SessionId, SessionState>,
    ticket_keys: HashMap<Role, TicketKey>,
    ticket_limits: TicketLimits,
    ticket_usage: HashMap<String, TicketUsage>,
    clock: Arc<dyn Clock>,
    limits: SessionLimits,
    pipeline: Pipeline,
    observe: ObserveState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthState {
    Start,
    WaitingVersion,
    VersionNegotiated,
    AttachRequested,
    Attached,
    Failed,
}

impl ServerCore {
    pub(crate) fn new(
        clock: Arc<dyn Clock>,
        limits: SessionLimits,
        ticket_limits: TicketLimits,
        telemetry: TelemetryConfig,
        telemetry_ingest: TelemetryIngestConfig,
        telemetry_manifest: TelemetryManifestStore,
        cas: CasConfig,
        shards: ShardLayout,
        ui: UiProviderConfig,
        host: HostNamespaceConfig,
        sidecars: SidecarNamespaceConfig,
        policy: PolicyConfig,
        audit: AuditConfig,
    ) -> Self {
        let observe = ObserveState::new(ObserveConfig::default(), ui, clock.now());
        let pipeline = Pipeline::new(PipelineConfig::from_limits(limits));
        let policy_store = PolicyStore::new(policy).expect("policy config");
        let policy_namespace = if policy_store.enabled() {
            PolicyNamespaceConfig::enabled(policy_store.rules_snapshot().to_vec())
        } else {
            PolicyNamespaceConfig::disabled()
        };
        let audit_store = AuditStore::new(audit);
        let audit_namespace = if audit_store.enabled() {
            AuditNamespaceConfig::enabled(audit_store.export_snapshot().to_vec())
        } else {
            AuditNamespaceConfig::disabled()
        };
        let replay_state = ReplayState::new(audit_store.replay_config());
        let replay_namespace = if replay_state.enabled() {
            ReplayNamespaceConfig::enabled(replay_state.status().to_vec())
        } else {
            ReplayNamespaceConfig::disabled()
        };
        let mut control = ControlPlane::new(
            telemetry,
            telemetry_ingest,
            telemetry_manifest,
            cas,
            shards,
            ui,
            host,
            sidecars,
            policy_namespace,
            policy_store,
            audit_namespace,
            audit_store,
            replay_namespace,
            replay_state,
        );
        control
            .namespace_mut()
            .install_observability(observe.config())
            .expect("install /proc observability");
        let mut core = Self {
            codec: Codec,
            control,
            next_session: 1,
            sessions: HashMap::new(),
            ticket_keys: HashMap::new(),
            ticket_limits,
            ticket_usage: HashMap::new(),
            clock,
            limits,
            pipeline,
            observe,
        }
        ;
        let _ = core.observe.update_proc_9p(&mut core.control.namespace, core.pipeline.metrics());
        let _ = core.observe.update_proc_ingest(
            &mut core.control.namespace,
            core.clock.now(),
            core.pipeline.metrics(),
        );
        let _ = core.control.refresh_policy_preflight();
        core
    }

    pub(crate) fn allocate_session(&mut self) -> SessionId {
        let id = SessionId::from_raw(self.next_session);
        self.next_session += 1;
        let now = self.clock.now();
        self.sessions
            .insert(id, SessionState::new(now, self.limits));
        id
    }

    fn refresh_proc_sessions(&mut self, current: Option<&SessionState>) -> Result<(), NineDoorError> {
        let shards = *self.control.namespace().shard_layout();
        let mut shard_counts = vec![0usize; shards.shard_count()];
        let mut worker_sessions = 0usize;
        for state in self.sessions.values() {
            if let Some(worker_id) = state.worker_id() {
                worker_sessions = worker_sessions.saturating_add(1);
                let shard = shards.worker_shard(worker_id) as usize;
                if let Some(entry) = shard_counts.get_mut(shard) {
                    *entry = entry.saturating_add(1);
                }
            }
        }
        let mut total_sessions = self.sessions.len();
        if let Some(state) = current {
            total_sessions = total_sessions.saturating_add(1);
            if let Some(worker_id) = state.worker_id() {
                worker_sessions = worker_sessions.saturating_add(1);
                let shard = shards.worker_shard(worker_id) as usize;
                if let Some(entry) = shard_counts.get_mut(shard) {
                    *entry = entry.saturating_add(1);
                }
            }
        }
        let shard_labels = shards.shard_labels();
        self.observe.update_sessions(
            self.control.namespace_mut(),
            total_sessions,
            worker_sessions,
            shards.shard_bits(),
            &shard_labels,
            &shard_counts,
        )
    }

    pub(crate) fn register_service(
        &mut self,
        service: &str,
        target: &[&str],
    ) -> Result<(), NineDoorError> {
        let path = target
            .iter()
            .map(|component| component.to_string())
            .collect();
        self.control.register_service(service, path)
    }

    pub(crate) fn install_gpu_nodes(
        &mut self,
        topology: &GpuNamespaceSnapshot,
    ) -> Result<(), NineDoorError> {
        self.control.install_gpu_nodes(topology)
    }

    pub(crate) fn register_ticket_secret(&mut self, role: Role, secret: &str) {
        self.ticket_keys
            .insert(role, TicketKey::from_secret(secret));
    }

    #[allow(dead_code)]
    pub(crate) fn handle_frame(
        &mut self,
        session: SessionId,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, NineDoorError> {
        self.handle_batch(session, request_bytes)
    }

    pub(crate) fn handle_batch(
        &mut self,
        session: SessionId,
        batch: &[u8],
    ) -> Result<Vec<u8>, NineDoorError> {
        let start = self.clock.now();
        let mut state = match self.sessions.remove(&session) {
            Some(state) => state,
            None => {
                debug!(
                    "[net-console][auth] unknown session {} while handling batch",
                    session.session()
                );
                return Err(NineDoorError::UnknownSession(session));
            }
        };
        let negotiated_msize = state.negotiated_msize();
        let mut entries = Vec::new();
        for frame in secure9p_codec::BatchIter::new(batch) {
            let frame = frame.map_err(NineDoorError::Codec)?;
            let raw = frame.bytes().to_vec();
            let request = self.codec.decode_request(&raw).map_err(|err| {
                debug!(
                    "[net-console][auth] session={} state={:?} decode error: {}",
                    session.session(),
                    state.auth_state,
                    err
                );
                err
            })?;
            entries.push((request, raw.len()));
        }
        if entries.is_empty() {
            self.sessions.insert(session, state);
            return Ok(Vec::new());
        }

        let batch_overflow = batch.len() > negotiated_msize as usize;
        let frame_overflow = entries.len() > self.limits.batch_frames;
        let mut responses = vec![None; entries.len()];
        let mut dropped = 0u64;
        let mut reserved = vec![false; entries.len()];

        for (idx, (request, frame_len)) in entries.iter().enumerate() {
            if batch_overflow {
                responses[idx] = Some(ResponseBody::Error {
                    code: ErrorCode::TooBig,
                    message: "batch exceeds negotiated msize".to_owned(),
                });
                dropped = dropped.saturating_add(1);
                continue;
            }
            if frame_overflow {
                info!(
                    "[secure9p][session={}] backpressure: batch frame limit exceeded",
                    session.session()
                );
                self.pipeline.record_backpressure();
                responses[idx] = Some(ResponseBody::Error {
                    code: ErrorCode::Busy,
                    message: "queue depth exceeded".to_owned(),
                });
                continue;
            }
            if *frame_len > negotiated_msize as usize {
                responses[idx] = Some(ResponseBody::Error {
                    code: ErrorCode::TooBig,
                    message: "frame exceeds negotiated msize".to_owned(),
                });
                dropped = dropped.saturating_add(1);
                continue;
            }
            match state.tag_window.reserve(request.tag) {
                Ok(()) => match state.queue_depth.reserve(1) {
                    Ok(()) => {
                        reserved[idx] = true;
                    }
                    Err(QueueError::Full) => {
                        state.tag_window.release(request.tag);
                        info!(
                            "[secure9p][session={}] backpressure: queue depth exceeded (tag={})",
                            session.session(),
                            request.tag
                        );
                        self.pipeline.record_backpressure();
                        responses[idx] = Some(ResponseBody::Error {
                            code: ErrorCode::Busy,
                            message: "queue depth exceeded".to_owned(),
                        });
                    }
                },
                Err(TagError::InUse) => {
                    responses[idx] = Some(ResponseBody::Error {
                        code: ErrorCode::Invalid,
                        message: "tag already in use".to_owned(),
                    });
                }
                Err(TagError::WindowFull) => {
                    info!(
                        "[secure9p][session={}] backpressure: tag window exceeded (tag={})",
                        session.session(),
                        request.tag
                    );
                    responses[idx] = Some(ResponseBody::Error {
                        code: ErrorCode::Busy,
                        message: "tag window exceeded".to_owned(),
                    });
                }
            }
            self.pipeline
                .record_queue_depth(state.queue_depth.current());
        }

        for (idx, (request, _)) in entries.iter().enumerate() {
            if reserved[idx] {
                let outcome = match self.dispatch_with_state(session, &mut state, request) {
                    Ok(body) => body,
                    Err(NineDoorError::Protocol { code, message }) => {
                        ResponseBody::Error { code, message }
                    }
                    Err(other) => {
                        state.tag_window.release(request.tag);
                        state.queue_depth.release(1);
                        self.sessions.insert(session, state);
                        return Err(other);
                    }
                };
                responses[idx] = Some(outcome);
                state.tag_window.release(request.tag);
                state.queue_depth.release(1);
            }
        }
        self.pipeline
            .record_queue_depth(state.queue_depth.current());

        self.sessions.insert(session, state);
        let mut buffer = Vec::new();
        let mut writer = io::Cursor::new(&mut buffer);
        let mut encoded = Vec::with_capacity(responses.len());
        for (idx, (request, _)) in entries.iter().enumerate() {
            let body = responses[idx].take().expect("response populated");
            let response = Response {
                tag: request.tag,
                body,
            };
            encoded.push(self.codec.encode_response(&response)?);
        }
        self.pipeline.write_batch(&mut writer, &encoded)?;
        let end = self.clock.now();
        let elapsed_ms = end
            .duration_since(start)
            .as_millis()
            .try_into()
            .unwrap_or(u32::MAX);
        self.observe.record_ingest_latency(elapsed_ms);
        self.observe.record_ingest_dropped(dropped);
        let metrics = self.pipeline.metrics();
        {
            let namespace = self.control.namespace_mut();
            let observe = &mut self.observe;
            let _ = observe.update_proc_9p(namespace, metrics);
            let _ = observe.update_proc_ingest(namespace, end, metrics);
        }
        Ok(buffer)
    }

    pub(crate) fn pipeline_metrics(&self) -> PipelineMetrics {
        self.pipeline.metrics()
    }

    #[allow(dead_code)]
    fn dispatch(
        &mut self,
        session: SessionId,
        request: &Request,
    ) -> Result<ResponseBody, NineDoorError> {
        let mut state = match self.sessions.remove(&session) {
            Some(state) => state,
            None => {
                debug!(
                    "[net-console][auth] unknown session {} while handling {:?}",
                    session.session(),
                    request.body
                );
                return Err(NineDoorError::UnknownSession(session));
            }
        };
        let result = self.dispatch_with_state(session, &mut state, request);
        self.sessions.insert(session, state);
        result
    }

    fn dispatch_with_state(
        &mut self,
        session: SessionId,
        state: &mut SessionState,
        request: &Request,
    ) -> Result<ResponseBody, NineDoorError> {
        let result = match &request.body {
            RequestBody::Version { msize, version } => {
                info!(
                    target: "nine-door",
                    "session {}: version={} msize={}",
                    session.session(),
                    version,
                    msize
                );
                debug!(
                    "[net-console][auth] session={} state={:?} recv Tversion msize={} version={}",
                    session.session(),
                    state.auth_state,
                    msize,
                    version
                );
                info!(
                    "[secure9p][session={}] received Tversion (msize={}, version={})",
                    session.session(),
                    msize,
                    version
                );
                state.auth_state = AuthState::WaitingVersion;
                let outcome = Self::handle_version(state, *msize, version);
                if outcome.is_ok() {
                    state.auth_state = AuthState::VersionNegotiated;
                    info!(
                        "[secure9p][session={}] negotiated version {} (msize={})",
                        session.session(),
                        VERSION,
                        state.negotiated_msize()
                    );
                } else {
                    state.auth_state = AuthState::Failed;
                }
                outcome
            }
            RequestBody::Attach {
                fid, uname, aname, ..
            } => {
                info!(
                    target: "nine-door",
                    "session {}: attach role={} fid={}",
                    session.session(),
                    uname,
                    fid
                );
                debug!(
                    "[net-console][auth] session={} state={:?} recv Tattach fid={} uname={}",
                    session.session(),
                    state.auth_state,
                    fid,
                    uname
                );
                info!(
                    "[secure9p][session={}] received Tattach fid={} uname={}",
                    session.session(),
                    fid,
                    uname
                );
                state.auth_state = AuthState::AttachRequested;
                let outcome = self.handle_attach(state, *fid, uname.as_str(), aname.as_str());
                if outcome.is_ok() {
                    state.auth_state = AuthState::Attached;
                    info!(
                        "[secure9p][session={}] attach accepted role={:?}",
                        session.session(),
                        state.role()
                    );
                } else {
                    state.auth_state = AuthState::Failed;
                }
                outcome
            }
            RequestBody::Walk {
                fid,
                newfid,
                wnames,
            } => {
                trace!(
                    "[net-console][auth] session={} state={:?} recv Twalk fid={} newfid={} components={}",
                    session.session(),
                    state.auth_state,
                    fid,
                    newfid,
                    wnames.len()
                );
                state.ensure_attached()?;
                if let Err(reason) = state.pre_operation(self.clock.now()) {
                    Err(self.handle_budget_failure(session, state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, state, reason))
                } else {
                    self.handle_walk(state, *fid, *newfid, wnames)
                }
            }
            RequestBody::Open { fid, mode } => {
                trace!(
                    "[net-console][auth] session={} state={:?} recv Topen fid={} mode={:?}",
                    session.session(),
                    state.auth_state,
                    fid,
                    mode
                );
                state.ensure_attached()?;
                if let Err(reason) = state.pre_operation(self.clock.now()) {
                    Err(self.handle_budget_failure(session, state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, state, reason))
                } else {
                    self.handle_open(state, *fid, *mode)
                }
            }
            RequestBody::Read { fid, offset, count } => {
                trace!(
                    "[net-console][auth] session={} state={:?} recv Tread fid={} offset={} count={}",
                    session.session(),
                    state.auth_state,
                    fid,
                    offset,
                    count
                );
                state.ensure_attached()?;
                if let Err(reason) = state.pre_operation(self.clock.now()) {
                    Err(self.handle_budget_failure(session, state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, state, reason))
                } else {
                    self.handle_read(state, *fid, *offset, *count)
                }
            }
            RequestBody::Write { fid, offset, data } => {
                trace!(
                    "[net-console][auth] session={} state={:?} recv Twrite fid={} payload_len={}",
                    session.session(),
                    state.auth_state,
                    fid,
                    data.len()
                );
                state.ensure_attached()?;
                if let Err(reason) = state.pre_operation(self.clock.now()) {
                    Err(self.handle_budget_failure(session, state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, state, reason))
                } else {
                    self.handle_write(session, state, *fid, *offset, data)
                }
            }
            RequestBody::Clunk { fid } => {
                trace!(
                    "[net-console][auth] session={} state={:?} recv Tclunk fid={}",
                    session.session(),
                    state.auth_state,
                    fid
                );
                state.ensure_attached()?;
                Self::handle_clunk(state, *fid)
            }
        };
        if let Err(ref err) = result {
            state.auth_state = AuthState::Failed;
            debug!(
                "[net-console][auth] session={} state={:?} error handling request: {}",
                session.session(),
                state.auth_state,
                err
            );
        }
        if state.attached
            && !state.first_request_logged
            && !matches!(
                request.body,
                RequestBody::Version { .. } | RequestBody::Attach { .. }
            )
        {
            info!(
                "[secure9p][session={}] first post-attach request: {:?}",
                session.session(),
                request.body
            );
            state.first_request_logged = true;
        }
        result
    }

    fn handle_version(
        state: &mut SessionState,
        requested_msize: u32,
        version: &str,
    ) -> Result<ResponseBody, NineDoorError> {
        if version != VERSION {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("unsupported version {version}"),
            ));
        }
        let negotiated = requested_msize.min(MAX_MSIZE);
        state.set_msize(negotiated);
        Ok(ResponseBody::Version {
            msize: negotiated,
            version: VERSION.to_string(),
        })
    }

    fn handle_attach(
        &mut self,
        state: &mut SessionState,
        fid: u32,
        uname: &str,
        ticket: &str,
    ) -> Result<ResponseBody, NineDoorError> {
        if state.msize().is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "version negotiation required before attach",
            ));
        }
        let (role, identity) = parse_role_from_uname(uname)?;
        let ticket = ticket.trim();
        let mut ticket_payload = None;
        let mut identity = identity;
        let mut budget_override = None;
        let mut ticket_claims = None;
        if !ticket.is_empty() {
            let key = self
                .ticket_keys
                .get(&role)
                .ok_or_else(|| NineDoorError::protocol(ErrorCode::Permission, "ticket rejected"))?;
            let claims = TicketToken::decode(ticket, key).map_err(|err| {
                NineDoorError::protocol(ErrorCode::Permission, format!("ticket invalid: {err}"))
            })?;
            if claims.claims().role != role {
                return Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    "ticket role mismatch",
                ));
            }
            if let Some(subject) = claims.claims().subject.as_deref() {
                match &identity {
                    Some(value) if value != subject => {
                        return Err(NineDoorError::protocol(
                            ErrorCode::Permission,
                            "ticket subject mismatch",
                        ));
                    }
                    None => {
                        identity = Some(subject.to_owned());
                    }
                    _ => {}
                }
            }
            let claims_ref = claims.claims();
            budget_override = Some(claims_ref.budget);
            ticket_claims = Some(claims_ref.clone());
            ticket_payload = Some(ticket.to_owned());
        } else if role != Role::Queen {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "ticket required for worker attach",
            ));
        }
        let now = self.clock.now();
        if let Some(claims) = ticket_claims.as_mut() {
            if let Some(ttl_s) = claims.budget.ttl_s() {
                let now_ms = unix_time_ms();
                let ttl_ms = ttl_s.saturating_mul(1_000);
                let expires_at_ms = claims.issued_at_ms.saturating_add(ttl_ms);
                if now_ms >= expires_at_ms {
                    self.record_ticket_expired(role, ticket, claims, now_ms)?;
                    self.pipeline.record_ui_deny();
                    return Err(NineDoorError::protocol(
                        ErrorCode::Permission,
                        "ticket expired",
                    ));
                }
                let remaining_ms = expires_at_ms.saturating_sub(now_ms);
                let remaining_s = remaining_ms.saturating_add(999) / 1_000;
                claims.budget = claims.budget.with_ttl(Some(remaining_s));
                budget_override = Some(claims.budget);
            }
            let usage = TicketUsage::from_claims(claims, self.ticket_limits, now).map_err(|err| {
                let _ = self.record_ticket_claim_denial(role, ticket, &err);
                NineDoorError::protocol(ErrorCode::Permission, format!("ticket invalid: {err}"))
            })?;
            if usage.has_enforcement() {
                self.ticket_usage
                    .entry(ticket.to_owned())
                    .or_insert(usage);
            }
        }
        match role {
            Role::Queen => {
                let budget = budget_override.unwrap_or_else(BudgetSpec::unbounded);
                state.configure_role(role, identity, None, None, None, budget, now);
            }
            Role::WorkerHeartbeat => {
                let worker_id = identity.clone().ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-heartbeat attach requires identity",
                    )
                })?;
                let Some(record) = self.control.worker_record(&worker_id) else {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("worker {worker_id} not found"),
                    ));
                };
                match record.kind() {
                    WorkerKind::Heartbeat => {
                        let budget = budget_override
                            .map(|override_budget| clamp_budget(record.budget(), override_budget))
                            .unwrap_or_else(|| record.budget());
                        state.configure_role(role, Some(worker_id), None, None, None, budget, now);
                    }
                    WorkerKind::Gpu(_) => {
                        return Err(NineDoorError::protocol(
                            ErrorCode::Invalid,
                            "worker-heartbeat role does not match GPU worker",
                        ));
                    }
                }
            }
            Role::WorkerGpu => {
                let worker_id = identity.clone().ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-gpu attach requires identity",
                    )
                })?;
                let Some(record) = self.control.worker_record(&worker_id) else {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("worker {worker_id} not found"),
                    ));
                };
                match record.kind() {
                    WorkerKind::Gpu(gpu) => {
                        let budget = budget_override
                            .map(|override_budget| clamp_budget(record.budget(), override_budget))
                            .unwrap_or_else(|| record.budget());
                        state.configure_role(
                            role,
                            Some(worker_id),
                            Some(gpu.lease.gpu_id.clone()),
                            None,
                            None,
                            budget,
                            now,
                        );
                    }
                    WorkerKind::Heartbeat => {
                        return Err(NineDoorError::protocol(
                            ErrorCode::Invalid,
                            "worker-gpu identity is not bound to a GPU lease",
                        ));
                    }
                }
            }
            Role::WorkerBus => {
                let scope = identity.clone().ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-bus attach requires identity",
                    )
                })?;
                if !self.control.bus_scope_exists(&scope) {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("bus scope {scope} not found"),
                    ));
                }
                let budget = budget_override.unwrap_or_else(BudgetSpec::default_heartbeat);
                state.configure_role(role, None, None, Some(scope), None, budget, now);
            }
            Role::WorkerLora => {
                let scope = identity.clone().ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-lora attach requires identity",
                    )
                })?;
                if !self.control.lora_scope_exists(&scope) {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("lora scope {scope} not found"),
                    ));
                }
                let budget = budget_override.unwrap_or_else(BudgetSpec::default_heartbeat);
                state.configure_role(role, None, None, None, Some(scope), budget, now);
            }
        }
        state.set_ticket(ticket_payload);
        let qid = self.control.namespace().root_qid();
        if let Err(err) = state.insert_fid(fid, Vec::new(), Vec::new(), qid) {
            return Err(fid_insert_error(fid, err));
        }
        state.mark_attached();
        self.refresh_proc_sessions(Some(state))?;
        Ok(ResponseBody::Attach { qid })
    }

    fn handle_walk(
        &mut self,
        state: &mut SessionState,
        fid: u32,
        newfid: u32,
        wnames: &[String],
    ) -> Result<ResponseBody, NineDoorError> {
        let existing = state.fid(fid).ok_or_else(|| {
            NineDoorError::protocol(ErrorCode::NotFound, format!("fid {fid} not found"))
        })?;
        let role = state.role();
        let worker_id_owned = state.worker_id().map(|id| id.to_owned());
        let worker_id = worker_id_owned.as_deref();
        let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
        let gpu_scope = gpu_scope_owned.as_deref();
        let bus_scope_owned = state.bus_scope().map(|scope| scope.to_owned());
        let bus_scope = bus_scope_owned.as_deref();
        let lora_scope_owned = state.lora_scope().map(|scope| scope.to_owned());
        let lora_scope = lora_scope_owned.as_deref();
        let host_mount_owned = self.control.host_mount_path().map(|path| path.to_vec());
        let host_mount = host_mount_owned.as_deref();
        let sidecar_bus_scopes = self.control.sidecar_bus_scopes().to_vec();
        let sidecar_lora_scopes = self.control.sidecar_lora_scopes().to_vec();
        if wnames.is_empty() {
            if newfid == fid {
                if state
                    .with_fid_mut(fid, |entry| {
                        entry.view_path = existing.view_path.clone();
                        entry.canonical_path = existing.canonical_path.clone();
                        entry.qid = existing.qid;
                        entry.open_mode = None;
                    })
                    .is_none()
                {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("fid {fid} not found"),
                    ));
                }
            } else if let Err(err) = state.insert_fid(
                newfid,
                existing.view_path.clone(),
                existing.canonical_path.clone(),
                existing.qid,
            ) {
                return Err(fid_insert_error(newfid, err));
            }
            return Ok(ResponseBody::Walk { qids: Vec::new() });
        }
        let mut qids = Vec::with_capacity(wnames.len());
        let shards = *self.control.namespace().shard_layout();
        let mut view_path = existing.view_path.clone();
        let mut canonical_path = existing.canonical_path.clone();
        let mut current_qid = existing.qid;
        let mut full_view_path = existing.view_path.clone();
        full_view_path.extend(wnames.iter().cloned());
        let full_resolved = state.resolve_view_path(&full_view_path);
        if let Err(err) = AccessPolicy::ensure_path(
            &shards,
            role,
            worker_id,
            gpu_scope,
            bus_scope,
            lora_scope,
            host_mount,
            &sidecar_bus_scopes,
            &sidecar_lora_scopes,
            &full_resolved,
        ) {
            self.maybe_log_sidecar_denial(&full_resolved, bus_scope, lora_scope, &err)?;
            return Err(err);
        }
        if let Some(info) = self.control.namespace().ui_provider_info(&full_resolved) {
            if !info.enabled {
                self.control.record_ui_provider_denial(
                    &full_resolved,
                    info,
                    "disabled",
                    role,
                    state.ticket(),
                    None,
                )?;
                return Err(NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("ui provider {} disabled", info.kind.label()),
                ));
            }
        }
        self.enforce_ticket_scope(
            state,
            &full_resolved,
            TicketVerb::Read,
            true,
            false,
        )?;
        for component in wnames {
            view_path.push(component.clone());
            let resolved = state.resolve_view_path(&view_path);
            if let Err(err) = AccessPolicy::ensure_path(
                &shards,
                role,
                worker_id,
                gpu_scope,
                bus_scope,
                lora_scope,
                host_mount,
                &sidecar_bus_scopes,
                &sidecar_lora_scopes,
                &resolved,
            ) {
                self.maybe_log_sidecar_denial(&resolved, bus_scope, lora_scope, &err)?;
                return Err(err);
            }
            if let Some(info) = self.control.namespace().ui_provider_info(&resolved) {
                if !info.enabled {
                    self.control.record_ui_provider_denial(
                        &resolved,
                        info,
                        "disabled",
                        role,
                        state.ticket(),
                        None,
                    )?;
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("ui provider {} disabled", info.kind.label()),
                    ));
                }
            }
            let node = self.control.namespace_mut().lookup(&resolved)?;
            current_qid = node.qid();
            qids.push(current_qid);
            canonical_path = resolved;
        }
        if newfid == fid {
            if state
                .with_fid_mut(fid, |entry| {
                    entry.view_path = view_path.clone();
                    entry.canonical_path = canonical_path.clone();
                    entry.qid = current_qid;
                    entry.open_mode = None;
                })
                .is_none()
            {
                return Err(NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("fid {fid} not found"),
                ));
            }
        } else if let Err(err) = state.insert_fid(newfid, view_path, canonical_path, current_qid) {
            return Err(fid_insert_error(newfid, err));
        }
        Ok(ResponseBody::Walk { qids })
    }

    fn handle_open(
        &mut self,
        state: &mut SessionState,
        fid: u32,
        mode: OpenMode,
    ) -> Result<ResponseBody, NineDoorError> {
        let role = state.role();
        let worker_id_owned = state.worker_id().map(|id| id.to_owned());
        let worker_id = worker_id_owned.as_deref();
        let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
        let gpu_scope = gpu_scope_owned.as_deref();
        let bus_scope_owned = state.bus_scope().map(|scope| scope.to_owned());
        let bus_scope = bus_scope_owned.as_deref();
        let lora_scope_owned = state.lora_scope().map(|scope| scope.to_owned());
        let lora_scope = lora_scope_owned.as_deref();
        let host_mount_owned = self.control.host_mount_path().map(|path| path.to_vec());
        let host_mount = host_mount_owned.as_deref();
        let sidecar_bus_scopes = self.control.sidecar_bus_scopes().to_vec();
        let sidecar_lora_scopes = self.control.sidecar_lora_scopes().to_vec();
        let ticket = state.ticket().map(str::to_owned);
        let iounit = state.negotiated_msize();
        let entry = state.fid(fid).ok_or_else(|| {
            NineDoorError::protocol(ErrorCode::NotFound, format!("fid {fid} not found"))
        })?;
        if mode.allows_read() {
            if let Some(info) = self
                .control
                .namespace()
                .ui_provider_info(&entry.canonical_path)
            {
                if !info.enabled {
                    self.control.record_ui_provider_denial(
                        &entry.canonical_path,
                        info,
                        "disabled",
                        role,
                        ticket.as_deref(),
                        None,
                    )?;
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("ui provider {} disabled", info.kind.label()),
                    ));
                }
            }
        }
        if mode.allows_write() {
            if let Some(target) = host_write_target(&entry.canonical_path, host_mount) {
                if role != Some(Role::Queen) {
                    self.control.record_host_write_audit(
                        &target,
                        HostWriteOutcome::Denied,
                        role,
                        ticket.as_deref(),
                        None,
                    )?;
                    return Err(NineDoorError::protocol(ErrorCode::Permission, "EPERM"));
                }
            }
        }
        let shards = *self.control.namespace().shard_layout();
        if let Err(err) = AccessPolicy::ensure_open(
            &shards,
            role,
            worker_id,
            gpu_scope,
            bus_scope,
            lora_scope,
            host_mount,
            &sidecar_bus_scopes,
            &sidecar_lora_scopes,
            &entry.canonical_path,
            mode,
        ) {
            self.maybe_log_sidecar_denial(&entry.canonical_path, bus_scope, lora_scope, &err)?;
            return Err(err);
        }
        if mode.allows_read() {
            self.enforce_ticket_scope(
                state,
                &entry.canonical_path,
                TicketVerb::Read,
                false,
                false,
            )?;
        }
        if mode.allows_write() {
            self.enforce_ticket_scope(
                state,
                &entry.canonical_path,
                TicketVerb::Write,
                false,
                false,
            )?;
        }
        let node = self.control.namespace_mut().lookup(&entry.canonical_path)?;
        if node.is_directory() && mode.allows_write() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "cannot write directories",
            ));
        }
        if mode.allows_write() && !node.qid().ty().is_append_only() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "fid is not append-only",
            ));
        }
        if state.with_fid_mut(fid, |entry| entry.open_mode = Some(mode)).is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("fid {fid} not found"),
            ));
        }
        let qid = node.qid();
        Ok(ResponseBody::Open { qid, iounit })
    }

    fn handle_read(
        &mut self,
        state: &mut SessionState,
        fid: u32,
        offset: u64,
        count: u32,
    ) -> Result<ResponseBody, NineDoorError> {
        let entry = state.fid(fid).ok_or_else(|| {
            NineDoorError::protocol(ErrorCode::NotFound, format!("fid {fid} not found"))
        })?;
        let mode = entry.open_mode.ok_or_else(|| {
            NineDoorError::protocol(ErrorCode::Invalid, "fid must be opened before read")
        })?;
        if !mode.allows_read() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "fid opened without read permission",
            ));
        }
        let path = entry.canonical_path.clone();
        let ui_info = self.control.namespace().ui_provider_info(&path);
        if let Some(info) = ui_info {
            if !info.enabled {
                self.control.record_ui_provider_denial(
                    &path,
                    info,
                    "disabled",
                    state.role(),
                    state.ticket(),
                    None,
                )?;
                return Err(NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("ui provider {} disabled", info.kind.label()),
                ));
            }
            if count > UI_MAX_READ_BYTES {
                self.control.record_ui_provider_denial(
                    &path,
                    info,
                    "oversize-read",
                    state.role(),
                    state.ticket(),
                    Some(count),
                )?;
                return Err(NineDoorError::protocol(
                    ErrorCode::TooBig,
                    format!(
                        "ui provider {} read exceeds {} bytes",
                        info.kind.label(),
                        UI_MAX_READ_BYTES
                    ),
                ));
            }
        }
        let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
        let gpu_scope = gpu_scope_owned.as_deref();
        let bus_scope_owned = state.bus_scope().map(|scope| scope.to_owned());
        let bus_scope = bus_scope_owned.as_deref();
        let lora_scope_owned = state.lora_scope().map(|scope| scope.to_owned());
        let lora_scope = lora_scope_owned.as_deref();
        let host_mount_owned = self.control.host_mount_path().map(|path| path.to_vec());
        let host_mount = host_mount_owned.as_deref();
        let sidecar_bus_scopes = self.control.sidecar_bus_scopes().to_vec();
        let sidecar_lora_scopes = self.control.sidecar_lora_scopes().to_vec();
        let shards = *self.control.namespace().shard_layout();
        if let Err(err) = AccessPolicy::ensure_read(
            &shards,
            state.role(),
            state.worker_id(),
            gpu_scope,
            bus_scope,
            lora_scope,
            host_mount,
            &sidecar_bus_scopes,
            &sidecar_lora_scopes,
            &path,
        ) {
            self.maybe_log_sidecar_denial(&path, bus_scope, lora_scope, &err)?;
            return Err(err);
        }
        self.enforce_ticket_scope(state, &path, TicketVerb::Read, false, true)?;
        self.enforce_ticket_bandwidth(state, &path, TicketVerb::Read, count as u64)?;
        let mut cursor_check = None;
        let mut cursor_key = None;
        if shards.worker_id_from_telemetry_path(&path).is_some() {
            let key = path.join("/");
            cursor_check =
                self.check_ticket_cursor(state, key.as_str(), &path, TicketVerb::Read, offset)?;
            cursor_key = Some(key);
        }
        let data = self
            .control
            .namespace_mut()
            .read(&path, offset, count)?;
        if let (Some(check), Some(key)) = (cursor_check, cursor_key) {
            self.record_ticket_cursor(state, key, offset, data.len(), check);
        }
        self.consume_ticket_bandwidth(state, data.len() as u64);
        if ui_info.is_some() {
            self.pipeline.record_ui_read();
        }
        Ok(ResponseBody::Read { data })
    }

    fn handle_write(
        &mut self,
        session: SessionId,
        state: &mut SessionState,
        fid: u32,
        offset: u64,
        data: &[u8],
    ) -> Result<ResponseBody, NineDoorError> {
        let role = state.role();
        let worker_id_owned = state.worker_id().map(|id| id.to_owned());
        let worker_id = worker_id_owned.as_deref();
        let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
        let gpu_scope = gpu_scope_owned.as_deref();
        let bus_scope_owned = state.bus_scope().map(|scope| scope.to_owned());
        let bus_scope = bus_scope_owned.as_deref();
        let lora_scope_owned = state.lora_scope().map(|scope| scope.to_owned());
        let lora_scope = lora_scope_owned.as_deref();
        let host_mount_owned = self.control.host_mount_path().map(|path| path.to_vec());
        let host_mount = host_mount_owned.as_deref();
        let sidecar_bus_scopes = self.control.sidecar_bus_scopes().to_vec();
        let sidecar_lora_scopes = self.control.sidecar_lora_scopes().to_vec();
        let ticket = state.ticket().map(str::to_owned);
        let path = {
            let entry = state.fid(fid).ok_or_else(|| {
                NineDoorError::protocol(ErrorCode::NotFound, format!("fid {fid} not found"))
            })?;
            let mode = entry.open_mode.ok_or_else(|| {
                NineDoorError::protocol(ErrorCode::Invalid, "fid must be opened before write")
            })?;
            if !mode.allows_write() {
                return Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    "fid opened without write permission",
                ));
            }
            entry.canonical_path.clone()
        };
        let requested_bytes = data.len() as u64;
        let policy_enabled = self.control.policy_enabled();
        let audit_enabled = self.control.audit_enabled();
        if audit_enabled && is_audit_journal_path(&path) {
            self.enforce_ticket_write_limits(state, &path, requested_bytes)?;
            let count = self
                .control
                .process_audit_journal_write(offset, data, role, ticket.as_deref())?;
            self.consume_ticket_bandwidth(state, count as u64);
            return Ok(ResponseBody::Write { count });
        }
        if audit_enabled && is_audit_decisions_path(&path) {
            self.control.record_audit_denial("audit decisions write denied")?;
            return Err(NineDoorError::protocol(ErrorCode::Permission, "EPERM"));
        }
        if audit_enabled && is_audit_export_path(&path) {
            self.control.record_audit_denial("audit export write denied")?;
            return Err(NineDoorError::protocol(ErrorCode::Permission, "EPERM"));
        }
        let replay_enabled = self.control.replay_enabled();
        if replay_enabled && is_replay_ctl_path(&path) {
            self.enforce_ticket_write_limits(state, &path, requested_bytes)?;
            let count = self
                .control
                .process_replay_ctl_write(offset, data)?;
            self.consume_ticket_bandwidth(state, count as u64);
            return Ok(ResponseBody::Write { count });
        }
        if replay_enabled && is_replay_status_path(&path) {
            self.control.record_audit_denial("replay status write denied")?;
            return Err(NineDoorError::protocol(ErrorCode::Permission, "EPERM"));
        }
        if policy_enabled && is_policy_ctl_path(&path) {
            self.enforce_ticket_write_limits(state, &path, requested_bytes)?;
            let count = self.control.process_policy_ctl_write(offset, data)?;
            self.consume_ticket_bandwidth(state, count as u64);
            return Ok(ResponseBody::Write { count });
        }
        if policy_enabled && is_actions_queue_path(&path) {
            self.enforce_ticket_write_limits(state, &path, requested_bytes)?;
            let (count, actions) = self.control.process_action_queue_write(offset, data)?;
            for action in actions {
                self.control
                    .record_policy_action_audit(&action, role, ticket.as_deref())?;
                if audit_enabled {
                    self.control.record_decision_action(&action, role, ticket.as_deref())?;
                }
            }
            self.consume_ticket_bandwidth(state, count as u64);
            return Ok(ResponseBody::Write { count });
        }
        let host_target = host_write_target(&path, host_mount);
        if let Some(target) = host_target.as_ref() {
            if role != Some(Role::Queen) {
                self.control.record_host_write_audit(
                    target,
                    HostWriteOutcome::Denied,
                    role,
                    ticket.as_deref(),
                    None,
                )?;
                if audit_enabled {
                    self.control.record_control_audit(
                        path.as_slice(),
                        data,
                        ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                        role,
                        ticket.as_deref(),
                    )?;
                }
                return Err(NineDoorError::protocol(ErrorCode::Permission, "EPERM"));
            }
        }
        let shards = *self.control.namespace().shard_layout();
        if let Err(err) = AccessPolicy::ensure_write(
            &shards,
            role,
            worker_id,
            gpu_scope,
            bus_scope,
            lora_scope,
            host_mount,
            &sidecar_bus_scopes,
            &sidecar_lora_scopes,
            &path,
        ) {
            self.maybe_log_sidecar_denial(&path, bus_scope, lora_scope, &err)?;
            return Err(err);
        }
        self.enforce_ticket_write_limits(state, &path, requested_bytes)?;
        if policy_enabled {
            let decision = self.control.consume_policy_gate(&path)?;
            match decision {
                PolicyGateDecision::Allowed(allowance) => {
                    if matches!(allowance, PolicyGateAllowance::Action { .. }) {
                        self.control
                            .record_policy_gate_audit(&path, &allowance, role, ticket.as_deref())?;
                    }
                    if audit_enabled {
                        self.control
                            .record_decision_gate(&path, &allowance, role, ticket.as_deref())?;
                    }
                }
                PolicyGateDecision::Denied(denial) => {
                    self.control.record_policy_gate_denial(
                        &path,
                        &denial,
                        role,
                        ticket.as_deref(),
                    )?;
                    if audit_enabled {
                        self.control
                            .record_decision_gate_denial(&path, &denial, role, ticket.as_deref())?;
                        if is_queen_ctl_path(&path) || host_target.is_some() {
                            self.control.record_control_audit(
                                path.as_slice(),
                                data,
                                ControlOutcome::err(ErrorCode::Permission, "EPERM"),
                                role,
                                ticket.as_deref(),
                            )?;
                        }
                    }
                    return Err(NineDoorError::protocol(ErrorCode::Permission, "EPERM"));
                }
            }
        }
        let telemetry_write = worker_id
            .map(|id| is_worker_telemetry_path(&shards, &path, id))
            .unwrap_or(false);
        if telemetry_write {
            if let Err(reason) = state.consume_tick() {
                return Err(self.handle_budget_failure(session, state, reason));
            }
        }
        if let (Some(worker), Some(scope)) = (worker_id, gpu_scope) {
            if is_gpu_job_path(&path, scope) {
                let count = self.control.process_gpu_job(worker, scope, data)?;
                self.consume_ticket_bandwidth(state, count as u64);
                return Ok(ResponseBody::Write { count });
            }
        }
        if is_queen_ctl_path(&path) {
            let events = self
                .control
                .process_queen_write(data, role, ticket.as_deref())?;
            let role = state.role();
            let worker_id_owned = state.worker_id().map(|id| id.to_owned());
            let worker_id = worker_id_owned.as_deref();
            let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
            let gpu_scope = gpu_scope_owned.as_deref();
            for event in &events {
                match event {
                    QueenEvent::Bound { target, mount } | QueenEvent::Mounted { target, mount } => {
                        state.apply_mount(
                            &shards,
                            role,
                            worker_id,
                            gpu_scope,
                            bus_scope,
                            lora_scope,
                            host_mount,
                            &sidecar_bus_scopes,
                            &sidecar_lora_scopes,
                            target,
                            mount,
                        )?;
                    }
                    _ => {}
                }
            }
            self.process_queen_events(events, session)?;
            self.consume_ticket_bandwidth(state, data.len() as u64);
            Ok(ResponseBody::Write {
                count: data.len() as u32,
            })
        } else if let Some(target) = host_target {
            let count = self
                .control
                .namespace_mut()
                .write_append(&path, offset, data)?;
            self.control.record_host_write_audit(
                &target,
                HostWriteOutcome::Allowed,
                role,
                ticket.as_deref(),
                Some(count),
            )?;
            if audit_enabled {
                self.control.record_control_audit(
                    path.as_slice(),
                    data,
                    ControlOutcome::ok(),
                    role,
                    ticket.as_deref(),
                )?;
            }
            self.consume_ticket_bandwidth(state, count as u64);
            Ok(ResponseBody::Write { count })
        } else {
            let count = self
                .control
                .namespace_mut()
                .write_append(&path, offset, data)?;
            self.consume_ticket_bandwidth(state, count as u64);
            Ok(ResponseBody::Write { count })
        }
    }

    fn handle_clunk(state: &mut SessionState, fid: u32) -> Result<ResponseBody, NineDoorError> {
        if state.remove_fid(fid).is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::Closed,
                format!("fid {fid} already closed"),
            ));
        }
        Ok(ResponseBody::Clunk)
    }

    fn process_queen_events(
        &mut self,
        events: Vec<QueenEvent>,
        current_session: SessionId,
    ) -> Result<(), NineDoorError> {
        for event in events {
            match event {
                QueenEvent::Spawned(worker_id) => {
                    let _ = worker_id;
                }
                QueenEvent::Killed(worker_id) => {
                    self.revoke_worker_sessions(
                        &worker_id,
                        "killed by queen",
                        Some(current_session),
                    );
                    self.control.log_event(
                        "queen",
                        TraceLevel::Warn,
                        Some(&worker_id),
                        &format!("revoked {worker_id}: killed by queen"),
                    )?;
                }
                QueenEvent::BudgetUpdated => {}
                QueenEvent::Bound { .. } => {}
                QueenEvent::Mounted { .. } => {}
            }
        }
        Ok(())
    }

    fn revoke_worker_sessions(&mut self, worker_id: &str, reason: &str, skip: Option<SessionId>) {
        for (session_id, state) in &mut self.sessions {
            if skip == Some(*session_id) {
                continue;
            }
            if state.matches_worker(worker_id) {
                state.mark_revoked(reason.to_string());
            }
        }
    }

    fn handle_budget_failure(
        &mut self,
        session: SessionId,
        state: &mut SessionState,
        reason: String,
    ) -> NineDoorError {
        state.mark_revoked(reason.clone());
        if let Some(worker_id) = state.worker_id().map(ToOwned::to_owned) {
            if self
                .control
                .revoke_worker(&worker_id, &reason)
                .unwrap_or(false)
            {
                self.revoke_worker_sessions(&worker_id, &reason, Some(session));
            }
        }
        NineDoorError::protocol(ErrorCode::Closed, reason)
    }

    fn maybe_log_sidecar_denial(
        &mut self,
        path: &[String],
        bus_scope: Option<&str>,
        lora_scope: Option<&str>,
        err: &NineDoorError,
    ) -> Result<(), NineDoorError> {
        let NineDoorError::Protocol { code, .. } = err else {
            return Ok(());
        };
        if *code != ErrorCode::Permission {
            return Ok(());
        }
        let Some(kind) = self.control.namespace().sidecar_kind_for_path(path) else {
            return Ok(());
        };
        let scope = match kind {
            SidecarKind::Bus => bus_scope,
            SidecarKind::Lora => lora_scope,
        }
        .unwrap_or("none");
        let message = format!("sidecar-deny kind={} scope={}", kind.as_str(), scope);
        self.control
            .log_event("sidecar", TraceLevel::Warn, None, &message)?;
        Ok(())
    }

    fn record_ticket_claim_denial(
        &mut self,
        role: Role,
        ticket: &str,
        err: &dyn std::fmt::Display,
    ) -> Result<(), NineDoorError> {
        self.pipeline.record_ui_deny();
        let role_label = role_label(role);
        let message = format!(
            "ui-ticket outcome=deny reason=invalid-claims role={} ticket={} detail={err}",
            role_label, ticket
        );
        self.control.log_event("ui", TraceLevel::Warn, None, &message)
    }

    fn record_ticket_expired(
        &mut self,
        role: Role,
        ticket: &str,
        claims: &cohesix_ticket::TicketClaims,
        now_ms: u64,
    ) -> Result<(), NineDoorError> {
        let role_label = role_label(role);
        let ttl_s = claims.budget.ttl_s().unwrap_or(0);
        let message = format!(
            "ui-ticket outcome=deny reason=expired role={} ticket={} issued_at_ms={} ttl_s={} now_ms={}",
            role_label,
            ticket,
            claims.issued_at_ms,
            ttl_s,
            now_ms
        );
        self.control.log_event("ui", TraceLevel::Warn, None, &message)
    }

    fn record_ticket_denial(
        &mut self,
        path: &[String],
        verb: TicketVerb,
        denial: TicketDeny,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        self.pipeline.record_ui_deny();
        let role_label = match role {
            Some(role) => role_label(role),
            None => "unauthenticated",
        };
        let path_label = if path.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", path.join("/"))
        };
        let verb_label = ticket_verb_label(verb);
        let mut message = format!(
            "ui-ticket outcome=deny reason={} role={} ticket={} path={} verb={}",
            ticket_deny_reason(denial),
            role_label,
            ticket.unwrap_or("none"),
            path_label,
            verb_label
        );
        match denial {
            TicketDeny::Scope => {}
            TicketDeny::Rate { limit_per_s } => {
                message.push_str(&format!(" limit_per_s={limit_per_s} window_ms=1000"));
            }
            TicketDeny::Bandwidth {
                limit_bytes,
                remaining_bytes,
                requested_bytes,
            } => {
                message.push_str(&format!(
                    " limit_bytes={limit_bytes} remaining_bytes={remaining_bytes} requested_bytes={requested_bytes}"
                ));
            }
            TicketDeny::CursorResume { limit } => {
                message.push_str(&format!(" limit={limit}"));
            }
            TicketDeny::CursorAdvance { limit } => {
                message.push_str(&format!(" limit={limit}"));
            }
        }
        self.control.log_event("ui", TraceLevel::Warn, None, &message)
    }

    fn enforce_ticket_scope(
        &mut self,
        state: &SessionState,
        path: &[String],
        verb: TicketVerb,
        allow_ancestor: bool,
        check_rate: bool,
    ) -> Result<(), NineDoorError> {
        let Some(ticket) = state.ticket() else {
            return Ok(());
        };
        let Some(usage) = self.ticket_usage.get_mut(ticket) else {
            return Ok(());
        };
        if !usage.has_enforcement() {
            return Ok(());
        }
        let outcome = if check_rate {
            usage.check_scope(path, verb, allow_ancestor, self.clock.now())
        } else {
            usage.check_scope_no_rate(path, verb, allow_ancestor)
        };
        if let Err(denial) = outcome {
            self.record_ticket_denial(path, verb, denial, state.role(), Some(ticket))?;
            return Err(ticket_denial_error(denial));
        }
        Ok(())
    }

    fn enforce_ticket_bandwidth(
        &mut self,
        state: &SessionState,
        path: &[String],
        verb: TicketVerb,
        requested_bytes: u64,
    ) -> Result<(), NineDoorError> {
        let Some(ticket) = state.ticket() else {
            return Ok(());
        };
        let Some(usage) = self.ticket_usage.get_mut(ticket) else {
            return Ok(());
        };
        if !usage.has_enforcement() {
            return Ok(());
        }
        if let Err(denial) = usage.check_bandwidth(requested_bytes) {
            self.record_ticket_denial(path, verb, denial, state.role(), Some(ticket))?;
            return Err(ticket_denial_error(denial));
        }
        Ok(())
    }

    fn enforce_ticket_write_limits(
        &mut self,
        state: &SessionState,
        path: &[String],
        requested_bytes: u64,
    ) -> Result<(), NineDoorError> {
        self.enforce_ticket_scope(state, path, TicketVerb::Write, false, true)?;
        self.enforce_ticket_bandwidth(state, path, TicketVerb::Write, requested_bytes)?;
        Ok(())
    }

    fn check_ticket_cursor(
        &mut self,
        state: &SessionState,
        path_key: &str,
        path: &[String],
        verb: TicketVerb,
        offset: u64,
    ) -> Result<Option<CursorCheck>, NineDoorError> {
        let Some(ticket) = state.ticket() else {
            return Ok(None);
        };
        let Some(usage) = self.ticket_usage.get_mut(ticket) else {
            return Ok(None);
        };
        if !usage.has_enforcement() {
            return Ok(None);
        }
        match usage.check_cursor(path_key, offset) {
            Ok(check) => Ok(Some(check)),
            Err(denial) => {
                self.record_ticket_denial(path, verb, denial, state.role(), Some(ticket))?;
                Err(ticket_denial_error(denial))
            }
        }
    }

    fn consume_ticket_bandwidth(&mut self, state: &SessionState, consumed: u64) {
        let Some(ticket) = state.ticket() else {
            return;
        };
        let Some(usage) = self.ticket_usage.get_mut(ticket) else {
            return;
        };
        usage.consume_bandwidth(consumed);
    }

    fn record_ticket_cursor(
        &mut self,
        state: &SessionState,
        path_key: String,
        offset: u64,
        len: usize,
        check: CursorCheck,
    ) {
        let Some(ticket) = state.ticket() else {
            return;
        };
        let Some(usage) = self.ticket_usage.get_mut(ticket) else {
            return;
        };
        usage.record_cursor(path_key, offset, len, check);
    }
}

struct ControlPlane {
    namespace: Namespace,
    workers: HashMap<String, WorkerRecord>,
    next_worker_id: u64,
    default_budget: BudgetSpec,
    services: HashMap<String, Vec<String>>,
    gpu_nodes: HashSet<String>,
    active_leases: HashMap<String, String>,
    policy: PolicyStore,
    audit: AuditStore,
    replay: ReplayState,
}

impl ControlPlane {
    fn new(
        telemetry: TelemetryConfig,
        telemetry_ingest: TelemetryIngestConfig,
        telemetry_manifest: TelemetryManifestStore,
        cas: CasConfig,
        shards: ShardLayout,
        ui: UiProviderConfig,
        host: HostNamespaceConfig,
        sidecars: SidecarNamespaceConfig,
        policy_namespace: PolicyNamespaceConfig,
        policy: PolicyStore,
        audit_namespace: AuditNamespaceConfig,
        audit: AuditStore,
        replay_namespace: ReplayNamespaceConfig,
        replay: ReplayState,
    ) -> Self {
        Self {
            namespace: Namespace::new_with_telemetry_manifest_host_policy(
                telemetry,
                telemetry_ingest,
                telemetry_manifest,
                cas,
                shards,
                ui,
                host,
                sidecars,
                policy_namespace,
                audit_namespace,
                replay_namespace,
            ),
            workers: HashMap::new(),
            next_worker_id: 1,
            default_budget: BudgetSpec::default_heartbeat(),
            services: HashMap::new(),
            gpu_nodes: HashSet::new(),
            active_leases: HashMap::new(),
            policy,
            audit,
            replay,
        }
    }

    fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    fn namespace_mut(&mut self) -> &mut Namespace {
        &mut self.namespace
    }

    fn host_mount_path(&self) -> Option<&[String]> {
        self.namespace.host_mount_path()
    }

    fn sidecar_bus_scopes(&self) -> &[SidecarScope] {
        self.namespace.sidecar_bus_scopes()
    }

    fn sidecar_lora_scopes(&self) -> &[SidecarScope] {
        self.namespace.sidecar_lora_scopes()
    }

    fn bus_scope_exists(&self, scope: &str) -> bool {
        self.namespace.bus_scope_exists(scope)
    }

    fn lora_scope_exists(&self, scope: &str) -> bool {
        self.namespace.lora_scope_exists(scope)
    }

    fn policy_enabled(&self) -> bool {
        self.policy.enabled()
    }

    fn audit_enabled(&self) -> bool {
        self.audit.enabled()
    }

    fn replay_enabled(&self) -> bool {
        self.replay.enabled()
    }

    fn worker_record(&self, worker_id: &str) -> Option<&WorkerRecord> {
        self.workers.get(worker_id)
    }

    fn register_service(
        &mut self,
        service: &str,
        target: Vec<String>,
    ) -> Result<(), NineDoorError> {
        self.namespace.lookup(&target)?;
        self.services.insert(service.to_owned(), target);
        Ok(())
    }

    fn resolve_service(&self, service: &str) -> Option<Vec<String>> {
        self.services.get(service).cloned()
    }

    fn install_gpu_nodes(&mut self, topology: &GpuNamespaceSnapshot) -> Result<(), NineDoorError> {
        for node in &topology.nodes {
            self.namespace.set_gpu_node(
                &node.id,
                node.info_payload.as_bytes(),
                node.ctl_payload.as_bytes(),
                node.status_payload.as_bytes(),
            )?;
            self.gpu_nodes.insert(node.id.clone());
        }
        self.namespace.set_gpu_models(&topology.models)?;
        self.namespace
            .set_gpu_telemetry_schema(&topology.telemetry_schema)?;
        Ok(())
    }

    fn process_queen_write(
        &mut self,
        data: &[u8],
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<Vec<QueenEvent>, NineDoorError> {
        let ctl_path = vec!["queen".to_owned(), "ctl".to_owned()];
        self.namespace.write_append(&ctl_path, u64::MAX, data)?;
        let text = str::from_utf8(data).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("queen command must be UTF-8: {err}"),
            )
        })?;
        let mut events = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let command = match QueenCommand::parse(trimmed) {
                Ok(command) => command,
                Err(err) => {
                    self.record_control_audit(
                        ctl_path.as_slice(),
                        trimmed.as_bytes(),
                        ControlOutcome::from_error(&err),
                        role,
                        ticket,
                    )?;
                    return Err(err);
                }
            };
            let outcome = match command {
                QueenCommand::Spawn(spec) => {
                    let result = self.spawn_worker(&spec).map(|worker_id| {
                        events.push(QueenEvent::Spawned(worker_id));
                    });
                    result
                }
                QueenCommand::Kill(KillCommand { kill }) => {
                    let result = self.kill_worker(&kill).map(|()| {
                        events.push(QueenEvent::Killed(kill));
                    });
                    result
                }
                QueenCommand::Budget(payload) => {
                    let result = self.update_default_budget(&payload).map(|()| {
                        events.push(QueenEvent::BudgetUpdated);
                    });
                    result
                }
                QueenCommand::Bind(command) => {
                    let result = command.into_parts().and_then(|(from_raw, to_raw, source, mount)| {
                        if mount.is_empty() {
                            return Err(NineDoorError::protocol(
                                ErrorCode::Invalid,
                                "bind target must not be root",
                            ));
                        }
                        self.namespace.lookup(&source)?;
                        self.log_event(
                            "queen",
                            TraceLevel::Info,
                            None,
                            &format!("bound {from_raw} -> {to_raw}"),
                        )?;
                        events.push(QueenEvent::Bound {
                            target: source,
                            mount,
                        });
                        Ok(())
                    });
                    result
                }
                QueenCommand::Mount(command) => {
                    let result = command.into_parts().and_then(|(service, at_raw, mount)| {
                        if mount.is_empty() {
                            return Err(NineDoorError::protocol(
                                ErrorCode::Invalid,
                                "mount point must not be root",
                            ));
                        }
                        let Some(target) = self.resolve_service(&service) else {
                            return Err(NineDoorError::protocol(
                                ErrorCode::NotFound,
                                format!("service {service} not registered"),
                            ));
                        };
                        self.log_event(
                            "queen",
                            TraceLevel::Info,
                            None,
                            &format!("mounted {service} at {at_raw}"),
                        )?;
                        events.push(QueenEvent::Mounted { target, mount });
                        Ok(())
                    });
                    result
                }
            };
            if let Err(err) = outcome {
                self.record_control_audit(
                    ctl_path.as_slice(),
                    trimmed.as_bytes(),
                    ControlOutcome::from_error(&err),
                    role,
                    ticket,
                )?;
                return Err(err);
            }
            self.record_control_audit(
                ctl_path.as_slice(),
                trimmed.as_bytes(),
                ControlOutcome::ok(),
                role,
                ticket,
            )?;
        }
        Ok(events)
    }

    fn process_policy_ctl_write(
        &mut self,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        let outcome = self.policy.append_policy_ctl(offset, data)?;
        self.namespace
            .set_policy_ctl_payload(self.policy.ctl_log())?;
        Ok(outcome.count)
    }

    fn process_action_queue_write(
        &mut self,
        offset: u64,
        data: &[u8],
    ) -> Result<(u32, Vec<PolicyActionAudit>), NineDoorError> {
        let outcome = self.policy.append_action_queue(offset, data)?;
        self.namespace
            .set_action_queue_payload(self.policy.queue_log())?;
        for action in &outcome.appended {
            if let Some(payload) = self.policy.action_status_payload(&action.id) {
                self.namespace
                    .set_action_status_payload(&action.id, &payload)?;
            }
        }
        self.refresh_policy_preflight()?;
        Ok((outcome.count, outcome.appended))
    }

    fn process_audit_journal_write(
        &mut self,
        offset: u64,
        data: &[u8],
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<u32, NineDoorError> {
        let outcome = match self.audit.append_manual_journal(offset, data) {
            Ok(outcome) => outcome,
            Err(err) => {
                let message = format!("audit journal append rejected: {err}");
                self.log_event("audit", TraceLevel::Warn, None, &message)?;
                return Err(err);
            }
        };
        self.namespace
            .set_audit_journal_payload(&self.audit.journal_payload())?;
        self.namespace
            .set_audit_export_payload(self.audit.export_snapshot())?;
        if outcome.dropped_bytes > 0 {
            let message = format!(
                "audit journal truncation dropped_bytes={} new_base={}",
                outcome.dropped_bytes, outcome.new_base
            );
            self.namespace
                .emit_audit_notice(TelemetryAuditLevel::Warn, message.clone())?;
            self.log_event("audit", TraceLevel::Warn, None, &message)?;
        }
        if let Some(role) = role {
            let message = format!(
                "audit journal append role={} ticket={} bytes={}",
                role_label(role),
                ticket.unwrap_or("none"),
                outcome.count
            );
            self.log_event("audit", TraceLevel::Info, None, &message)?;
        }
        Ok(outcome.count)
    }

    fn process_replay_ctl_write(&mut self, offset: u64, data: &[u8]) -> Result<u32, NineDoorError> {
        if !self.replay.enabled() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "replay disabled",
            ));
        }
        let command = match self.replay.append_ctl(offset, data) {
            Ok(command) => command,
            Err(err) => {
                let message = format!("replay control rejected: {err}");
                self.log_event("audit", TraceLevel::Warn, None, &message)?;
                return Err(err);
            }
        };
        self.namespace
            .set_replay_ctl_payload(self.replay.ctl_log())?;
        let summary = match self
            .audit
            .replay_summary(command.from, self.audit.replay_config().max_entries())
        {
            Ok(summary) => summary,
            Err(err) => {
                let (code, message) = match err {
                    ReplayWindowError::Stale {
                        requested,
                        available_start,
                    } => (
                        ErrorCode::Invalid,
                        format!(
                            "replay cursor stale requested={} window_start={}",
                            requested, available_start
                        ),
                    ),
                    ReplayWindowError::Future {
                        requested,
                        available_end,
                    } => (
                        ErrorCode::Invalid,
                        format!(
                            "replay cursor beyond window requested={} window_end={}",
                            requested, available_end
                        ),
                    ),
                    ReplayWindowError::TooManyEntries { requested, max } => (
                        ErrorCode::TooBig,
                        format!("replay exceeds max entries {} > {}", requested, max),
                    ),
                };
                self.replay.set_status_err(&message)?;
                self.namespace
                    .set_replay_status_payload(self.replay.status())?;
                self.log_event("audit", TraceLevel::Warn, None, &message)?;
                return Err(NineDoorError::protocol(code, message));
            }
        };
        self.replay.set_status_ok(&summary)?;
        self.namespace
            .set_replay_status_payload(self.replay.status())?;
        let message = format!(
            "replay ok from={} to={} entries={} match=true",
            summary.from, summary.to, summary.entries
        );
        self.log_event("audit", TraceLevel::Info, None, &message)?;
        Ok(data.len() as u32)
    }

    fn record_control_audit(
        &mut self,
        path: &[String],
        data: &[u8],
        outcome: ControlOutcome,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        if !self.audit.enabled() {
            return Ok(());
        }
        let path_label = format!("/{}", path.join("/"));
        let role_label = role.map(role_label);
        let outcome = self.audit.record_control(
            path_label.as_str(),
            data,
            outcome,
            role_label,
            ticket,
        )?;
        self.namespace
            .set_audit_journal_payload(&self.audit.journal_payload())?;
        self.namespace
            .set_audit_export_payload(self.audit.export_snapshot())?;
        if outcome.dropped_bytes > 0 {
            let message = format!(
                "audit journal truncation dropped_bytes={} new_base={}",
                outcome.dropped_bytes, outcome.new_base
            );
            self.namespace
                .emit_audit_notice(TelemetryAuditLevel::Warn, message.clone())?;
            self.log_event("audit", TraceLevel::Warn, None, &message)?;
        }
        Ok(())
    }

    fn record_decision_action(
        &mut self,
        action: &PolicyActionAudit,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        if !self.audit.enabled() {
            return Ok(());
        }
        let decision = AuditPolicyActionDecision {
            id: action.id.as_str(),
            decision: policy_decision_label(action.decision),
            target: action.target.as_str(),
        };
        let outcome = self
            .audit
            .record_decision_action(&decision, role.map(role_label), ticket)?;
        self.namespace
            .set_audit_decisions_payload(&self.audit.decisions_payload())?;
        self.namespace
            .set_audit_export_payload(self.audit.export_snapshot())?;
        if outcome.dropped_bytes > 0 {
            let message = format!(
                "audit decisions truncation dropped_bytes={} new_base={}",
                outcome.dropped_bytes, outcome.new_base
            );
            self.namespace
                .emit_audit_notice(TelemetryAuditLevel::Warn, message.clone())?;
            self.log_event("audit", TraceLevel::Warn, None, &message)?;
        }
        Ok(())
    }

    fn record_decision_gate(
        &mut self,
        path: &[String],
        allowance: &PolicyGateAllowance,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        if !self.audit.enabled() {
            return Ok(());
        }
        let path_label = format!("/{}", path.join("/"));
        let decision = match allowance {
            PolicyGateAllowance::Action { id, target } => AuditPolicyGateDecision {
                outcome: "allow",
                id: Some(id.as_str()),
                target: Some(target.as_str()),
                path: path_label.as_str(),
            },
            PolicyGateAllowance::Ungated | PolicyGateAllowance::NotRequired => return Ok(()),
        };
        let outcome = self
            .audit
            .record_decision_gate(&decision, role.map(role_label), ticket)?;
        self.namespace
            .set_audit_decisions_payload(&self.audit.decisions_payload())?;
        self.namespace
            .set_audit_export_payload(self.audit.export_snapshot())?;
        if outcome.dropped_bytes > 0 {
            let message = format!(
                "audit decisions truncation dropped_bytes={} new_base={}",
                outcome.dropped_bytes, outcome.new_base
            );
            self.namespace
                .emit_audit_notice(TelemetryAuditLevel::Warn, message.clone())?;
            self.log_event("audit", TraceLevel::Warn, None, &message)?;
        }
        Ok(())
    }

    fn record_decision_gate_denial(
        &mut self,
        path: &[String],
        denial: &PolicyGateDenial,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        if !self.audit.enabled() {
            return Ok(());
        }
        let path_label = format!("/{}", path.join("/"));
        let decision = match denial {
            PolicyGateDenial::Missing => AuditPolicyGateDecision {
                outcome: "deny",
                id: None,
                target: None,
                path: path_label.as_str(),
            },
            PolicyGateDenial::Action { id, target } => AuditPolicyGateDecision {
                outcome: "deny",
                id: Some(id.as_str()),
                target: Some(target.as_str()),
                path: path_label.as_str(),
            },
        };
        let outcome = self
            .audit
            .record_decision_gate(&decision, role.map(role_label), ticket)?;
        self.namespace
            .set_audit_decisions_payload(&self.audit.decisions_payload())?;
        self.namespace
            .set_audit_export_payload(self.audit.export_snapshot())?;
        if outcome.dropped_bytes > 0 {
            let message = format!(
                "audit decisions truncation dropped_bytes={} new_base={}",
                outcome.dropped_bytes, outcome.new_base
            );
            self.namespace
                .emit_audit_notice(TelemetryAuditLevel::Warn, message.clone())?;
            self.log_event("audit", TraceLevel::Warn, None, &message)?;
        }
        Ok(())
    }

    fn record_audit_denial(&mut self, message: &str) -> Result<(), NineDoorError> {
        self.log_event("audit", TraceLevel::Warn, None, message)
    }

    fn consume_policy_gate(
        &mut self,
        path: &[String],
    ) -> Result<PolicyGateDecision, NineDoorError> {
        let decision = self.policy.consume_gate(path);
        match &decision {
            PolicyGateDecision::Allowed(allowance) => {
                if let PolicyGateAllowance::Action { id, .. } = allowance {
                    if let Some(payload) = self.policy.action_status_payload(id) {
                        self.namespace.set_action_status_payload(id, &payload)?;
                    }
                }
            }
            PolicyGateDecision::Denied(PolicyGateDenial::Action { id, .. }) => {
                if let Some(payload) = self.policy.action_status_payload(id) {
                    self.namespace.set_action_status_payload(id, &payload)?;
                }
            }
            PolicyGateDecision::Denied(PolicyGateDenial::Missing) => {}
        }
        self.refresh_policy_preflight()?;
        Ok(decision)
    }

    fn process_gpu_job(
        &mut self,
        worker_id: &str,
        gpu_id: &str,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        let text = str::from_utf8(data).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("gpu job descriptor must be UTF-8: {err}"),
            )
        })?;
        let mut descriptors = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let descriptor: JobDescriptor = serde_json::from_str(trimmed).map_err(|err| {
                NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("invalid gpu job descriptor: {err}"),
                )
            })?;
            descriptor.validate().map_err(|err| {
                NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("gpu job validation failed: {err}"),
                )
            })?;
            descriptors.push(descriptor);
        }
        let job_path = vec!["gpu".to_owned(), gpu_id.to_owned(), "job".to_owned()];
        let count = self.namespace.write_append(&job_path, u64::MAX, data)?;
        let telemetry_path = vec![
            "worker".to_owned(),
            worker_id.to_owned(),
            "telemetry".to_owned(),
        ];
        for descriptor in descriptors {
            let job_id = descriptor.job.as_str();
            let queued = status_entry(job_id, "QUEUED", "accepted");
            let running = status_entry(job_id, "RUNNING", "scheduled");
            let ok = status_entry(job_id, "OK", "completed");
            for status in [queued, running, ok] {
                let mut line = status.into_bytes();
                line.push(b'\n');
                self.namespace.append_gpu_status(gpu_id, &line)?;
            }
            let telemetry = format!(
                "{{\"job\":\"{}\",\"state\":\"RUNNING\",\"detail\":\"scheduled\"}}\n",
                job_id
            );
            self.namespace
                .write_append(&telemetry_path, u64::MAX, telemetry.as_bytes())?;
            let telemetry_done = format!(
                "{{\"job\":\"{}\",\"state\":\"OK\",\"detail\":\"completed\"}}\n",
                job_id
            );
            self.namespace
                .write_append(&telemetry_path, u64::MAX, telemetry_done.as_bytes())?;
        }
        Ok(count)
    }

    fn spawn_worker(&mut self, spec: &SpawnCommand) -> Result<String, NineDoorError> {
        let defaults = match spec.spawn {
            SpawnTarget::Heartbeat => self.default_budget,
            SpawnTarget::Gpu => BudgetSpec::default_gpu(),
        };
        let budget = spec.budget_spec(defaults)?;
        let worker_id = format!("worker-{}", self.next_worker_id);
        self.next_worker_id += 1;
        self.namespace.create_worker(&worker_id)?;
        let record = match spec.spawn {
            SpawnTarget::Heartbeat => {
                self.log_event(
                    "worker",
                    TraceLevel::Info,
                    Some(&worker_id),
                    &format!(
                        "spawned {worker_id} ticks={} ttl={} ops={}",
                        format_budget_value(budget.ticks()),
                        format_budget_value(budget.ttl_s()),
                        format_budget_value(budget.ops())
                    ),
                )?;
                WorkerRecord::heartbeat(budget)
            }
            SpawnTarget::Gpu => {
                let lease_fields = spec.gpu_lease().expect("gpu spawn must include lease");
                if !self.gpu_nodes.contains(&lease_fields.gpu_id) {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("gpu {} not registered", lease_fields.gpu_id),
                    ));
                }
                if self.active_leases.contains_key(&lease_fields.gpu_id) {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Busy,
                        format!("gpu {} already leased", lease_fields.gpu_id),
                    ));
                }
                let lease = WorkerGpuLease::new(
                    lease_fields.gpu_id.clone(),
                    lease_fields.mem_mb,
                    lease_fields.streams,
                    lease_fields.ttl_s,
                    lease_fields.priority,
                    worker_id.clone(),
                )
                .map_err(|err| {
                    NineDoorError::protocol(ErrorCode::Invalid, format!("invalid gpu lease: {err}"))
                })?;
                self.active_leases
                    .insert(lease.gpu_id.clone(), worker_id.clone());
                let ctl_path = vec!["gpu".to_owned(), lease.gpu_id.clone(), "ctl".to_owned()];
                let message = format!(
                    "LEASE {} mem={} streams={} priority={}\n",
                    worker_id, lease.mem_mb, lease.streams, lease.priority
                )
                .into_bytes();
                self.namespace.write_append(&ctl_path, u64::MAX, &message)?;
                self.log_event(
                    "worker",
                    TraceLevel::Info,
                    Some(&worker_id),
                    &format!(
                        "spawned {worker_id} gpu={} ttl={} streams={}",
                        lease.gpu_id, lease.ttl_s, lease.streams
                    ),
                )?;
                WorkerRecord::gpu(budget, lease)
            }
        };
        self.workers.insert(worker_id.clone(), record);
        Ok(worker_id)
    }

    fn kill_worker(&mut self, worker_id: &str) -> Result<(), NineDoorError> {
        let Some(record) = self.workers.remove(worker_id) else {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("worker {worker_id} not found"),
            ));
        };
        self.release_gpu_for_worker(worker_id, &record, "killed by queen")?;
        self.namespace.remove_worker(worker_id)?;
        self.log_event(
            "queen",
            TraceLevel::Info,
            Some(worker_id),
            &format!("killed {worker_id}"),
        )?;
        Ok(())
    }

    fn update_default_budget(&mut self, payload: &BudgetCommand) -> Result<(), NineDoorError> {
        self.default_budget = payload.apply(self.default_budget);
        let budget = self.default_budget;
        self.log_event(
            "queen",
            TraceLevel::Info,
            None,
            &format!(
                "updated default budget ttl={} ops={} ticks={}",
                format_budget_value(budget.ttl_s()),
                format_budget_value(budget.ops()),
                format_budget_value(budget.ticks())
            ),
        )?;
        Ok(())
    }

    fn revoke_worker(&mut self, worker_id: &str, reason: &str) -> Result<bool, NineDoorError> {
        let Some(record) = self.workers.remove(worker_id) else {
            return Ok(false);
        };
        self.release_gpu_for_worker(worker_id, &record, reason)?;
        if let Err(err) = self.namespace.remove_worker(worker_id) {
            if let NineDoorError::Protocol { code, .. } = &err {
                if *code != ErrorCode::NotFound {
                    return Err(err);
                }
            }
        }
        self.log_event(
            "queen",
            TraceLevel::Info,
            Some(worker_id),
            &format!("revoked {worker_id}: {reason}"),
        )?;
        Ok(true)
    }

    fn log_event(
        &mut self,
        category: &str,
        level: TraceLevel,
        task: Option<&str>,
        message: &str,
    ) -> Result<(), NineDoorError> {
        self.namespace
            .tracefs_mut()
            .record(level, category, task, message);
        self.append_queen_log(message)
    }

    fn record_ui_provider_denial(
        &mut self,
        path: &[String],
        info: UiProviderInfo,
        reason: &str,
        role: Option<Role>,
        ticket: Option<&str>,
        count: Option<u32>,
    ) -> Result<(), NineDoorError> {
        let role_label = match role {
            Some(role) => role_label(role),
            None => "unauthenticated",
        };
        let path_label = if path.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", path.join("/"))
        };
        let variant = match info.variant {
            UiVariant::Text => "text",
            UiVariant::Cbor => "cbor",
        };
        let mut message = format!(
            "ui-provider outcome=deny reason={} provider={} variant={} role={} ticket={} path={}",
            reason,
            info.kind.label(),
            variant,
            role_label,
            ticket.unwrap_or("none"),
            path_label
        );
        if let Some(count) = count {
            message.push_str(&format!(" count={count}"));
        }
        self.log_event("ui", TraceLevel::Warn, None, &message)
    }

    fn refresh_policy_preflight(&mut self) -> Result<(), NineDoorError> {
        if !self.policy.enabled() {
            return Ok(());
        }
        let req = self.policy.preflight_req_payloads()?;
        self.apply_policy_preflight(req, true)?;
        let diff = self.policy.preflight_diff_payloads()?;
        self.apply_policy_preflight(diff, false)?;
        Ok(())
    }

    fn apply_policy_preflight(
        &mut self,
        payloads: PolicyPreflightPayloads,
        is_req: bool,
    ) -> Result<(), NineDoorError> {
        let text_result = if is_req {
            self.namespace
                .set_policy_preflight_req_payload(&payloads.text)
        } else {
            self.namespace
                .set_policy_preflight_diff_payload(&payloads.text)
        };
        self.ignore_not_found(text_result)?;
        let cbor_result = if is_req {
            self.namespace
                .set_policy_preflight_req_cbor_payload(&payloads.cbor)
        } else {
            self.namespace
                .set_policy_preflight_diff_cbor_payload(&payloads.cbor)
        };
        self.ignore_not_found(cbor_result)?;
        Ok(())
    }

    fn ignore_not_found(&self, result: Result<(), NineDoorError>) -> Result<(), NineDoorError> {
        if let Err(err) = result {
            if let NineDoorError::Protocol { code, .. } = &err {
                if *code == ErrorCode::NotFound {
                    return Ok(());
                }
            }
            return Err(err);
        }
        Ok(())
    }

    fn record_host_write_audit(
        &mut self,
        target: &HostWriteTarget<'_>,
        outcome: HostWriteOutcome,
        role: Option<Role>,
        ticket: Option<&str>,
        bytes: Option<u32>,
    ) -> Result<(), NineDoorError> {
        let message = format_host_write_audit(target, outcome, role, ticket, bytes);
        let level = match outcome {
            HostWriteOutcome::Allowed => TraceLevel::Info,
            HostWriteOutcome::Denied => TraceLevel::Warn,
        };
        self.log_event("host", level, None, &message)
    }

    fn record_policy_action_audit(
        &mut self,
        action: &PolicyActionAudit,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        let message = format_policy_action_audit(action, role, ticket);
        self.log_event("policy", TraceLevel::Info, None, &message)
    }

    fn record_policy_gate_audit(
        &mut self,
        path: &[String],
        allowance: &PolicyGateAllowance,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        if let Some(message) = format_policy_gate_allow(path, allowance, role, ticket) {
            self.log_event("policy", TraceLevel::Info, None, &message)?;
        }
        Ok(())
    }

    fn record_policy_gate_denial(
        &mut self,
        path: &[String],
        denial: &PolicyGateDenial,
        role: Option<Role>,
        ticket: Option<&str>,
    ) -> Result<(), NineDoorError> {
        let message = format_policy_gate_deny(path, denial, role, ticket);
        self.log_event("policy", TraceLevel::Warn, None, &message)
    }

    fn append_queen_log(&mut self, message: &str) -> Result<(), NineDoorError> {
        let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
        let mut line = message.as_bytes().to_vec();
        line.push(b'\n');
        self.namespace.write_append(&log_path, u64::MAX, &line)?;
        Ok(())
    }

    fn release_gpu_for_worker(
        &mut self,
        worker_id: &str,
        record: &WorkerRecord,
        reason: &str,
    ) -> Result<(), NineDoorError> {
        if let WorkerKind::Gpu(gpu) = record.kind() {
            self.active_leases.remove(&gpu.lease.gpu_id);
            let ctl_path = vec!["gpu".to_owned(), gpu.lease.gpu_id.clone(), "ctl".to_owned()];
            let ctl_line = format!("RELEASE {worker_id} {reason}\n");
            self.namespace
                .write_append(&ctl_path, u64::MAX, ctl_line.as_bytes())?;
            let status = status_entry(worker_id, "LEASE-ENDED", reason);
            let mut status_bytes = status.into_bytes();
            status_bytes.push(b'\n');
            self.namespace
                .append_gpu_status(&gpu.lease.gpu_id, &status_bytes)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct WorkerRecord {
    budget: BudgetSpec,
    kind: WorkerKind,
}

impl WorkerRecord {
    fn heartbeat(budget: BudgetSpec) -> Self {
        Self {
            budget,
            kind: WorkerKind::Heartbeat,
        }
    }

    fn gpu(budget: BudgetSpec, lease: WorkerGpuLease) -> Self {
        Self {
            budget,
            kind: WorkerKind::Gpu(GpuWorkerRecord { lease }),
        }
    }

    fn kind(&self) -> &WorkerKind {
        &self.kind
    }

    fn budget(&self) -> BudgetSpec {
        self.budget
    }
}

#[derive(Clone)]
enum WorkerKind {
    Heartbeat,
    Gpu(GpuWorkerRecord),
}

#[derive(Clone)]
struct GpuWorkerRecord {
    lease: WorkerGpuLease,
}

enum QueenEvent {
    Spawned(String),
    Killed(String),
    BudgetUpdated,
    Bound {
        target: Vec<String>,
        mount: Vec<String>,
    },
    Mounted {
        target: Vec<String>,
        mount: Vec<String>,
    },
}

/// Tracks per-session state including budget counters.
struct SessionState {
    msize: Option<u32>,
    attached: bool,
    fids: ShardedFidTable<FidState>,
    role: Option<Role>,
    worker_id: Option<String>,
    gpu_scope: Option<String>,
    bus_scope: Option<String>,
    lora_scope: Option<String>,
    ticket: Option<String>,
    budget: BudgetState,
    mounts: MountTable,
    auth_state: AuthState,
    first_request_logged: bool,
    tag_window: TagWindow,
    queue_depth: QueueDepth,
}

impl SessionState {
    fn new(now: Instant, limits: SessionLimits) -> Self {
        Self {
            msize: None,
            attached: false,
            fids: ShardedFidTable::default(),
            role: None,
            worker_id: None,
            gpu_scope: None,
            bus_scope: None,
            lora_scope: None,
            ticket: None,
            budget: BudgetState::new(BudgetSpec::unbounded(), now),
            mounts: MountTable::default(),
            auth_state: AuthState::Start,
            first_request_logged: false,
            tag_window: TagWindow::new(limits.tags_per_session),
            queue_depth: QueueDepth::new(limits.queue_depth_limit()),
        }
    }

    fn set_msize(&mut self, msize: u32) {
        self.msize = Some(msize);
    }

    fn msize(&self) -> Option<u32> {
        self.msize
    }

    fn negotiated_msize(&self) -> u32 {
        self.msize.unwrap_or(MAX_MSIZE)
    }

    fn mark_attached(&mut self) {
        self.attached = true;
    }

    fn ensure_attached(&self) -> Result<(), NineDoorError> {
        if self.attached {
            Ok(())
        } else {
            Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "attach required before operation",
            ))
        }
    }

    fn insert_fid(
        &mut self,
        fid: u32,
        view_path: Vec<String>,
        canonical_path: Vec<String>,
        qid: Qid,
    ) -> Result<(), FidError> {
        self.fids.insert(
            fid,
            FidState {
                view_path,
                canonical_path,
                qid,
                open_mode: None,
            },
        )
    }

    fn fid(&self, fid: u32) -> Option<FidState> {
        self.fids.get(fid)
    }

    fn with_fid_mut<R>(&self, fid: u32, f: impl FnOnce(&mut FidState) -> R) -> Option<R> {
        self.fids.with_entry_mut(fid, f)
    }

    fn remove_fid(&mut self, fid: u32) -> Option<FidState> {
        self.fids.remove(fid)
    }

    fn configure_role(
        &mut self,
        role: Role,
        identity: Option<String>,
        gpu_scope: Option<String>,
        bus_scope: Option<String>,
        lora_scope: Option<String>,
        budget: BudgetSpec,
        now: Instant,
    ) {
        self.role = Some(role);
        self.worker_id = identity;
        self.gpu_scope = gpu_scope;
        self.bus_scope = bus_scope;
        self.lora_scope = lora_scope;
        self.budget = BudgetState::new(budget, now);
    }

    fn set_ticket(&mut self, ticket: Option<String>) {
        self.ticket = ticket;
    }

    fn role(&self) -> Option<Role> {
        self.role
    }

    fn worker_id(&self) -> Option<&str> {
        self.worker_id.as_deref()
    }

    fn gpu_scope(&self) -> Option<&str> {
        self.gpu_scope.as_deref()
    }

    fn bus_scope(&self) -> Option<&str> {
        self.bus_scope.as_deref()
    }

    fn lora_scope(&self) -> Option<&str> {
        self.lora_scope.as_deref()
    }

    fn ticket(&self) -> Option<&str> {
        self.ticket.as_deref()
    }

    fn matches_worker(&self, worker_id: &str) -> bool {
        self.worker_id.as_deref() == Some(worker_id)
    }

    fn pre_operation(&mut self, now: Instant) -> Result<(), String> {
        let verdict = self.budget.check(now);
        self.budget.evaluate(verdict)
    }

    fn consume_operation(&mut self) -> Result<(), String> {
        let verdict = self.budget.consume_op();
        self.budget.evaluate(verdict)
    }

    fn consume_tick(&mut self) -> Result<(), String> {
        let verdict = self.budget.consume_tick();
        self.budget.evaluate(verdict)
    }

    fn mark_revoked(&mut self, reason: String) {
        self.budget.revoke(reason);
    }

    fn apply_mount(
        &mut self,
        shards: &ShardLayout,
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        bus_scope: Option<&str>,
        lora_scope: Option<&str>,
        host_mount: Option<&[String]>,
        sidecar_bus_scopes: &[SidecarScope],
        sidecar_lora_scopes: &[SidecarScope],
        target: &[String],
        mount: &[String],
    ) -> Result<(), NineDoorError> {
        AccessPolicy::ensure_path(
            shards,
            role,
            worker_id,
            gpu_scope,
            bus_scope,
            lora_scope,
            host_mount,
            sidecar_bus_scopes,
            sidecar_lora_scopes,
            target,
        )?;
        self.mounts.bind(target.to_vec(), mount.to_vec())
    }

    fn resolve_view_path(&self, view_path: &[String]) -> Vec<String> {
        self.mounts.resolve(view_path)
    }
}

struct BudgetState {
    ticks_remaining: Option<u64>,
    ops_remaining: Option<u64>,
    deadline: Option<Instant>,
    revoked: Option<String>,
}

impl BudgetState {
    fn new(spec: BudgetSpec, now: Instant) -> Self {
        Self {
            ticks_remaining: spec.ticks(),
            ops_remaining: spec.ops(),
            deadline: spec.ttl_s().map(|ttl| now + Duration::from_secs(ttl)),
            revoked: None,
        }
    }

    fn check(&mut self, now: Instant) -> BudgetVerdict {
        if let Some(reason) = self.revoked.clone() {
            return BudgetVerdict::Revoked(reason);
        }
        if let Some(deadline) = self.deadline {
            if now >= deadline {
                let reason = "ticket ttl expired".to_owned();
                self.revoked = Some(reason.clone());
                return BudgetVerdict::Revoked(reason);
            }
        }
        BudgetVerdict::Active
    }

    fn consume_op(&mut self) -> BudgetVerdict {
        if let Some(reason) = self.revoked.clone() {
            return BudgetVerdict::Revoked(reason);
        }
        if let Some(ops) = &mut self.ops_remaining {
            if *ops == 0 {
                let reason = "operation budget exhausted".to_owned();
                self.revoked = Some(reason.clone());
                return BudgetVerdict::Revoked(reason);
            }
            *ops -= 1;
        }
        BudgetVerdict::Active
    }

    fn consume_tick(&mut self) -> BudgetVerdict {
        if let Some(reason) = self.revoked.clone() {
            return BudgetVerdict::Revoked(reason);
        }
        if let Some(ticks) = &mut self.ticks_remaining {
            if *ticks == 0 {
                let reason = "tick budget exhausted".to_owned();
                self.revoked = Some(reason.clone());
                return BudgetVerdict::Revoked(reason);
            }
            *ticks -= 1;
        }
        BudgetVerdict::Active
    }

    fn revoke(&mut self, reason: String) {
        self.revoked = Some(reason);
    }

    fn evaluate(&mut self, verdict: BudgetVerdict) -> Result<(), String> {
        match verdict {
            BudgetVerdict::Active => Ok(()),
            BudgetVerdict::Revoked(reason) => {
                self.revoked = Some(reason.clone());
                Err(reason)
            }
        }
    }
}

enum BudgetVerdict {
    Active,
    Revoked(String),
}

#[derive(Debug, Clone)]
struct FidState {
    view_path: Vec<String>,
    canonical_path: Vec<String>,
    qid: Qid,
    open_mode: Option<OpenMode>,
}

#[derive(Debug, Default, Clone)]
struct MountTable {
    entries: Vec<MountEntry>,
}

#[derive(Debug, Clone)]
struct MountEntry {
    target: Vec<String>,
    mount: Vec<String>,
}

impl MountTable {
    fn bind(&mut self, target: Vec<String>, mount: Vec<String>) -> Result<(), NineDoorError> {
        if mount.is_empty() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "mount point must not be root",
            ));
        }
        self.entries.retain(|entry| entry.mount != mount);
        self.entries.push(MountEntry { target, mount });
        self.entries
            .sort_by(|a, b| b.mount.len().cmp(&a.mount.len()));
        Ok(())
    }

    fn resolve(&self, view_path: &[String]) -> Vec<String> {
        for entry in &self.entries {
            if view_path.starts_with(entry.mount.as_slice()) {
                let mut resolved = entry.target.clone();
                resolved.extend_from_slice(&view_path[entry.mount.len()..]);
                return resolved;
            }
        }
        view_path.to_vec()
    }
}

struct AccessPolicy;

impl AccessPolicy {
    fn ensure_open(
        shards: &ShardLayout,
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        bus_scope: Option<&str>,
        lora_scope: Option<&str>,
        host_mount: Option<&[String]>,
        sidecar_bus_scopes: &[SidecarScope],
        sidecar_lora_scopes: &[SidecarScope],
        path: &[String],
        mode: OpenMode,
    ) -> Result<(), NineDoorError> {
        Self::ensure_path(
            shards,
            role,
            worker_id,
            gpu_scope,
            bus_scope,
            lora_scope,
            host_mount,
            sidecar_bus_scopes,
            sidecar_lora_scopes,
            path,
        )?;
        if mode.allows_write() {
            Self::ensure_write(
                shards,
                role,
                worker_id,
                gpu_scope,
                bus_scope,
                lora_scope,
                host_mount,
                sidecar_bus_scopes,
                sidecar_lora_scopes,
                path,
            )?;
        }
        if mode.allows_read() {
            Self::ensure_read(
                shards,
                role,
                worker_id,
                gpu_scope,
                bus_scope,
                lora_scope,
                host_mount,
                sidecar_bus_scopes,
                sidecar_lora_scopes,
                path,
            )?;
        }
        Ok(())
    }

    fn ensure_read(
        shards: &ShardLayout,
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        bus_scope: Option<&str>,
        lora_scope: Option<&str>,
        host_mount: Option<&[String]>,
        sidecar_bus_scopes: &[SidecarScope],
        sidecar_lora_scopes: &[SidecarScope],
        path: &[String],
    ) -> Result<(), NineDoorError> {
        match role {
            Some(Role::Queen) => Ok(()),
            Some(Role::WorkerHeartbeat) => {
                if host_allowed_path(host_mount, path)
                    || worker_allowed_path(shards, worker_id, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerGpu) => {
                if host_allowed_path(host_mount, path)
                    || worker_allowed_path(shards, worker_id, path)
                    || gpu_allowed_read(gpu_scope, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerBus) => {
                if host_allowed_path(host_mount, path)
                    || worker_common_path(path)
                    || sidecar_allowed_path(sidecar_bus_scopes, bus_scope, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerLora) => {
                if host_allowed_path(host_mount, path)
                    || worker_common_path(path)
                    || sidecar_allowed_path(sidecar_lora_scopes, lora_scope, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            None => Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "attach required before operation",
            )),
        }
    }

    fn ensure_write(
        shards: &ShardLayout,
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        bus_scope: Option<&str>,
        lora_scope: Option<&str>,
        host_mount: Option<&[String]>,
        sidecar_bus_scopes: &[SidecarScope],
        sidecar_lora_scopes: &[SidecarScope],
        path: &[String],
    ) -> Result<(), NineDoorError> {
        match role {
            Some(Role::Queen) => Ok(()),
            Some(Role::WorkerHeartbeat) => {
                if host_allowed_path(host_mount, path) {
                    Err(Self::permission_denied(path))
                } else if worker_allowed_write(shards, worker_id, path) {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerGpu) => {
                if host_allowed_path(host_mount, path) {
                    Err(Self::permission_denied(path))
                } else if worker_allowed_write(shards, worker_id, path)
                    || gpu_allowed_write(gpu_scope, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerBus) => {
                if host_allowed_path(host_mount, path) {
                    Err(Self::permission_denied(path))
                } else if sidecar_allowed_path(sidecar_bus_scopes, bus_scope, path) {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerLora) => {
                if host_allowed_path(host_mount, path) {
                    Err(Self::permission_denied(path))
                } else if sidecar_allowed_path(sidecar_lora_scopes, lora_scope, path) {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            None => Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "attach required before operation",
            )),
        }
    }

    fn ensure_path(
        shards: &ShardLayout,
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        bus_scope: Option<&str>,
        lora_scope: Option<&str>,
        host_mount: Option<&[String]>,
        sidecar_bus_scopes: &[SidecarScope],
        sidecar_lora_scopes: &[SidecarScope],
        path: &[String],
    ) -> Result<(), NineDoorError> {
        match role {
            Some(Role::Queen) => Ok(()),
            Some(Role::WorkerHeartbeat) => {
                if host_allowed_prefix(host_mount, path)
                    || worker_allowed_prefix(shards, worker_id, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerGpu) => {
                if host_allowed_prefix(host_mount, path)
                    || worker_allowed_prefix(shards, worker_id, path)
                    || gpu_allowed_prefix(gpu_scope, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerBus) => {
                if host_allowed_prefix(host_mount, path)
                    || worker_common_prefix(path)
                    || sidecar_allowed_prefix(sidecar_bus_scopes, bus_scope, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerLora) => {
                if host_allowed_prefix(host_mount, path)
                    || worker_common_prefix(path)
                    || sidecar_allowed_prefix(sidecar_lora_scopes, lora_scope, path)
                {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            None => Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "attach required before operation",
            )),
        }
    }

    fn permission_denied(path: &[String]) -> NineDoorError {
        NineDoorError::protocol(
            ErrorCode::Permission,
            format!("access to /{} denied", path.join("/")),
        )
    }
}

fn worker_allowed_prefix(shards: &ShardLayout, worker_id: Option<&str>, path: &[String]) -> bool {
    let Some(id) = worker_id else {
        return false;
    };
    if path.is_empty() {
        return true;
    }
    if worker_common_prefix(path) {
        return true;
    }
    if shards.is_enabled() {
        let shard_label = shards.worker_shard_label(id);
        let shard_label = shard_label.as_str();
        match path {
            [first] if first == "shard" => return true,
            [first, shard] if first == "shard" => return shard == shard_label,
            [first, shard, second] if first == "shard" && second == "worker" => {
                return shard == shard_label
            }
            [first, shard, second, worker] if first == "shard" && second == "worker" => {
                return shard == shard_label && worker == id
            }
            [first, shard, second, worker, leaf]
                if first == "shard" && second == "worker" && leaf == "telemetry" =>
            {
                return shard == shard_label && worker == id
            }
            _ => {}
        }
        if shards.legacy_worker_alias_enabled() {
            match path {
                [first] if first == "worker" => return true,
                [first, worker] if first == "worker" => return worker == id,
                [first, worker, leaf] if first == "worker" && leaf == "telemetry" => {
                    return worker == id
                }
                _ => {}
            }
        }
        return false;
    }
    match path {
        [first] if first == "worker" => true,
        [first, worker] if first == "worker" => worker == id,
        [first, worker, leaf] if first == "worker" && leaf == "telemetry" => worker == id,
        _ => false,
    }
}

fn worker_common_prefix(path: &[String]) -> bool {
    match path {
        [first] if first == "proc" || first == "log" => true,
        [first, second] if first == "proc" && second == "boot" => true,
        [first, second] if first == "log" && second == "queen.log" => true,
        _ => false,
    }
}

fn worker_common_path(path: &[String]) -> bool {
    worker_common_prefix(path)
}

fn host_allowed_prefix(host_mount: Option<&[String]>, path: &[String]) -> bool {
    host_mount.map_or(false, |mount| path.starts_with(mount))
}

fn host_allowed_path(host_mount: Option<&[String]>, path: &[String]) -> bool {
    host_allowed_prefix(host_mount, path)
}

fn worker_allowed_path(shards: &ShardLayout, worker_id: Option<&str>, path: &[String]) -> bool {
    if !worker_allowed_prefix(shards, worker_id, path) {
        return false;
    }
    match path {
        [] => true,
        [first] if first == "worker" => false,
        [first, second] if first == "worker" && second == "self" => false,
        [first] if shards.is_enabled() && first == "shard" => false,
        [first, _] if shards.is_enabled() && first == "shard" => false,
        [first, _, second] if shards.is_enabled() && first == "shard" && second == "worker" => {
            false
        }
        _ => true,
    }
}

fn worker_allowed_write(shards: &ShardLayout, worker_id: Option<&str>, path: &[String]) -> bool {
    match worker_id {
        Some(id) => shards.is_worker_telemetry_path(path, id),
        None => false,
    }
}

fn gpu_allowed_prefix(gpu_scope: Option<&str>, path: &[String]) -> bool {
    match (gpu_scope, path) {
        (Some(_), [single]) => single == "gpu",
        (Some(scope), [first, second]) => first == "gpu" && second == scope,
        (Some(scope), [first, second, ..]) => first == "gpu" && second == scope,
        _ => false,
    }
}

fn gpu_allowed_read(gpu_scope: Option<&str>, path: &[String]) -> bool {
    match gpu_scope {
        Some(scope) => {
            is_gpu_info_path(path, scope)
                || is_gpu_status_path(path, scope)
                || is_gpu_ctl_path(path, scope)
                || is_gpu_job_path(path, scope)
        }
        None => false,
    }
}

fn gpu_allowed_write(gpu_scope: Option<&str>, path: &[String]) -> bool {
    match gpu_scope {
        Some(scope) => is_gpu_job_path(path, scope),
        None => false,
    }
}

fn sidecar_scope_for<'a>(
    scopes: &'a [SidecarScope],
    scope: Option<&str>,
) -> Option<&'a SidecarScope> {
    let scope = scope?;
    scopes.iter().find(|entry| entry.scope() == scope)
}

fn sidecar_allowed_prefix(scopes: &[SidecarScope], scope: Option<&str>, path: &[String]) -> bool {
    sidecar_scope_for(scopes, scope)
        .map(|entry| entry.matches_prefix(path))
        .unwrap_or(false)
}

fn sidecar_allowed_path(scopes: &[SidecarScope], scope: Option<&str>, path: &[String]) -> bool {
    sidecar_scope_for(scopes, scope)
        .map(|entry| entry.contains_path(path))
        .unwrap_or(false)
}

fn format_budget_value(value: Option<u64>) -> String {
    value.map_or_else(|| "∞".to_owned(), |v| v.to_string())
}

fn clamp_budget(record: BudgetSpec, override_budget: BudgetSpec) -> BudgetSpec {
    BudgetSpec::unbounded()
        .with_ticks(min_budget_field(record.ticks(), override_budget.ticks()))
        .with_ops(min_budget_field(record.ops(), override_budget.ops()))
        .with_ttl(min_budget_field(record.ttl_s(), override_budget.ttl_s()))
}

fn min_budget_field(record: Option<u64>, override_budget: Option<u64>) -> Option<u64> {
    match (record, override_budget) {
        (Some(record), Some(override_budget)) => Some(record.min(override_budget)),
        (Some(record), None) => Some(record),
        (None, Some(override_budget)) => Some(override_budget),
        (None, None) => None,
    }
}

fn fid_insert_error(fid: u32, err: FidError) -> NineDoorError {
    match err {
        FidError::InUse => NineDoorError::protocol(
            ErrorCode::Busy,
            format!("fid {fid} already in use"),
        ),
        FidError::Retired => NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("fid {fid} was clunked"),
        ),
    }
}

fn is_worker_telemetry_path(shards: &ShardLayout, path: &[String], worker_id: &str) -> bool {
    shards.is_worker_telemetry_path(path, worker_id)
}

fn is_gpu_job_path(path: &[String], scope: &str) -> bool {
    matches!(path, [first, second, third]
        if first == "gpu" && second == scope && third == "job")
}

fn is_gpu_status_path(path: &[String], scope: &str) -> bool {
    matches!(path, [first, second, third]
        if first == "gpu" && second == scope && third == "status")
}

fn is_gpu_info_path(path: &[String], scope: &str) -> bool {
    matches!(path, [first, second, third]
        if first == "gpu" && second == scope && third == "info")
}

fn is_gpu_ctl_path(path: &[String], scope: &str) -> bool {
    matches!(path, [first, second, third]
        if first == "gpu" && second == scope && third == "ctl")
}

fn is_queen_ctl_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "queen" && second == "ctl")
}

fn is_policy_ctl_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "policy" && second == "ctl")
}

fn is_actions_queue_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "actions" && second == "queue")
}

fn is_audit_journal_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "audit" && second == "journal")
}

fn is_audit_decisions_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "audit" && second == "decisions")
}

fn is_audit_export_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "audit" && second == "export")
}

fn is_replay_ctl_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "replay" && second == "ctl")
}

fn is_replay_status_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "replay" && second == "status")
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::Queen => "queen",
        Role::WorkerHeartbeat => "worker-heartbeat",
        Role::WorkerGpu => "worker-gpu",
        Role::WorkerBus => "worker-bus",
        Role::WorkerLora => "worker-lora",
    }
}

fn policy_decision_label(decision: PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Approve => "approve",
        PolicyDecision::Deny => "deny",
    }
}

fn ticket_denial_error(denial: TicketDeny) -> NineDoorError {
    match denial {
        TicketDeny::Scope => NineDoorError::protocol(ErrorCode::Permission, "EPERM"),
        TicketDeny::Rate { .. }
        | TicketDeny::Bandwidth { .. }
        | TicketDeny::CursorResume { .. }
        | TicketDeny::CursorAdvance { .. } => NineDoorError::protocol(ErrorCode::TooBig, "ELIMIT"),
    }
}

fn ticket_deny_reason(denial: TicketDeny) -> &'static str {
    match denial {
        TicketDeny::Scope => "scope",
        TicketDeny::Rate { .. } => "rate",
        TicketDeny::Bandwidth { .. } => "bandwidth",
        TicketDeny::CursorResume { .. } => "cursor-resume",
        TicketDeny::CursorAdvance { .. } => "cursor-advance",
    }
}

fn ticket_verb_label(verb: TicketVerb) -> &'static str {
    match verb {
        TicketVerb::Read => "read",
        TicketVerb::Write => "write",
        TicketVerb::ReadWrite => "read-write",
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn parse_role_from_uname(uname: &str) -> Result<(Role, Option<String>), NineDoorError> {
    if uname == proto_role_label(ProtoRole::Queen) {
        return Ok((Role::Queen, None));
    }
    if let Some(rest) = uname
        .strip_prefix(proto_role_label(ProtoRole::Worker))
        .and_then(|value| value.strip_prefix(':'))
    {
        if rest.is_empty() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "worker-heartbeat identity cannot be empty",
            ));
        }
        return Ok((Role::WorkerHeartbeat, Some(rest.to_owned())));
    }
    if let Some(rest) = uname
        .strip_prefix(proto_role_label(ProtoRole::GpuWorker))
        .and_then(|value| value.strip_prefix(':'))
    {
        if rest.is_empty() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "worker-gpu identity cannot be empty",
            ));
        }
        return Ok((Role::WorkerGpu, Some(rest.to_owned())));
    }
    if let Some(rest) = uname
        .strip_prefix(proto_role_label(ProtoRole::BusWorker))
        .and_then(|value| value.strip_prefix(':'))
    {
        if rest.is_empty() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "worker-bus identity cannot be empty",
            ));
        }
        return Ok((Role::WorkerBus, Some(rest.to_owned())));
    }
    if let Some(rest) = uname
        .strip_prefix(proto_role_label(ProtoRole::LoraWorker))
        .and_then(|value| value.strip_prefix(':'))
    {
        if rest.is_empty() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "worker-lora identity cannot be empty",
            ));
        }
        return Ok((Role::WorkerLora, Some(rest.to_owned())));
    }
    Err(NineDoorError::protocol(
        ErrorCode::Invalid,
        format!("unknown role string '{uname}'"),
    ))
}

pub(crate) fn role_to_uname(role: Role, identity: Option<&str>) -> Result<String, NineDoorError> {
    match role {
        Role::Queen => Ok(proto_role_label(ProtoRole::Queen).to_owned()),
        Role::WorkerHeartbeat => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-heartbeat attach requires identity",
                    )
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::Worker)))
        }
        Role::WorkerGpu => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-gpu attach requires identity",
                    )
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::GpuWorker)))
        }
        Role::WorkerBus => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-bus attach requires identity",
                    )
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::BusWorker)))
        }
        Role::WorkerLora => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-lora attach requires identity",
                    )
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::LoraWorker)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InProcessConnection, NineDoor};
    use cohesix_ticket::{BudgetSpec, MountSpec, TicketClaims, TicketIssuer};
    use secure9p_codec::OpenMode;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn queen_spawn_creates_worker_directory() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":3}\n");
        let worker_path = worker_telemetry_path("worker-1");
        queen.walk(1, 3, &worker_path).unwrap();
        queen.open(3, OpenMode::read_only()).unwrap();
        let data = queen.read(3, 0, 128).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn worker_attach_without_identity_returns_error() {
        let server = NineDoor::new();
        let mut worker = server.connect().unwrap();
        worker.version(MAX_MSIZE).unwrap();
        let err = worker
            .attach(1, Role::WorkerHeartbeat)
            .expect_err("worker attach without identity should fail");
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Invalid,
                ref message,
            } if message.contains("requires identity")
        ));
    }

    #[test]
    fn queen_kill_removes_worker_directory() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":5}\n");
        write_queen_command(&mut queen, "{\"kill\":\"worker-1\"}\n");
        let worker_path = worker_telemetry_path("worker-1");
        let err = queen.walk(1, 4, &worker_path).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::NotFound,
                ..
            }
        ));
    }

    #[test]
    fn clunking_unknown_fid_reports_closed() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        let err = queen.clunk(42).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Closed,
                ..
            }
        ));
    }

    #[test]
    fn read_requires_open() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        let boot_path = vec!["proc".to_owned(), "boot".to_owned()];
        queen.walk(1, 2, &boot_path).unwrap();
        let err = queen.read(2, 0, 16).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Invalid,
                ..
            }
        ));
    }

    #[test]
    fn worker_isolation_prevents_queen_access() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":5}\n");
        let mut worker = attach_worker(&server, "worker-1");
        let queen_path = vec!["queen".to_owned(), "ctl".to_owned()];
        let err = worker.walk(1, 2, &queen_path).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Permission,
                ..
            }
        ));
    }

    #[test]
    fn worker_cannot_write_queen_log() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":5}\n");

        let mut worker = attach_worker(&server, "worker-1");
        let queen_log = vec!["log".to_owned(), "queen.log".to_owned()];
        worker.walk(1, 2, &queen_log).unwrap();
        let err = worker.open(2, OpenMode::write_append()).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Permission,
                ..
            }
        ));
    }

    #[test]
    fn worker_cannot_read_other_worker_namespace() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":5}\n");
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":5}\n");

        let mut worker_two = attach_worker(&server, "worker-2");
        let worker_one_path = worker_telemetry_path("worker-1");
        let err = worker_two.walk(1, 3, &worker_one_path).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Permission,
                ..
            }
        ));
    }

    #[test]
    fn tick_budget_revokes_worker() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":1}\n");
        let mut worker = attach_worker(&server, "worker-1");
        let telemetry = worker_telemetry_path("worker-1");
        worker.walk(1, 2, &telemetry).unwrap();
        worker.open(2, OpenMode::write_append()).unwrap();
        worker.write(2, b"heartbeat 1\n").unwrap();
        let err = worker.write(2, b"heartbeat 2\n").unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Closed,
                ..
            }
        ));
        queen
            .walk(1, 3, &vec!["log".into(), "queen.log".into()])
            .unwrap();
        queen.open(3, OpenMode::read_only()).unwrap();
        let log = String::from_utf8(queen.read(3, 0, 1024).unwrap()).unwrap();
        assert!(log.contains("revoked worker-1: tick budget exhausted"));
    }

    #[test]
    fn ttl_budget_revokes_after_deadline() {
        let clock = Arc::new(TestClock::new());
        let server = NineDoor::new_with_clock(clock.clone());
        let mut queen = attach_queen(&server);
        write_queen_command(
            &mut queen,
            "{\"budget\":{\"ttl_s\":1}}\n{\"spawn\":\"heartbeat\",\"ticks\":5}\n",
        );
        let mut worker = attach_worker(&server, "worker-1");
        let telemetry = worker_telemetry_path("worker-1");
        worker.walk(1, 2, &telemetry).unwrap();
        clock.advance(Duration::from_secs(2));
        let err = worker.open(2, OpenMode::write_append()).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Closed,
                ..
            }
        ));
    }

    fn attach_queen(server: &NineDoor) -> InProcessConnection {
        let mut client = server.connect().unwrap();
        client.version(MAX_MSIZE).unwrap();
        client.attach(1, Role::Queen).unwrap();
        client
    }

    fn attach_worker(server: &NineDoor, id: &str) -> InProcessConnection {
        server.register_ticket_secret(Role::WorkerHeartbeat, "worker-secret");
        let issuer = TicketIssuer::new("worker-secret");
        let claims = TicketClaims::new(
            Role::WorkerHeartbeat,
            BudgetSpec::default_heartbeat(),
            Some(id.to_owned()),
            MountSpec::empty(),
            unix_time_ms(),
        );
        let token = issuer.issue(claims).unwrap().encode().unwrap();
        let mut client = server.connect().unwrap();
        client.version(MAX_MSIZE).unwrap();
        client
            .attach_with_identity(1, Role::WorkerHeartbeat, Some(id), Some(token.as_str()))
            .unwrap();
        client
    }

    fn worker_telemetry_path(worker_id: &str) -> Vec<String> {
        ShardLayout::default().worker_telemetry_path(worker_id)
    }

    fn write_queen_command(client: &mut InProcessConnection, payload: &str) {
        static NEXT_FID: AtomicU32 = AtomicU32::new(100);
        let path = vec!["queen".to_owned(), "ctl".to_owned()];
        let fid = NEXT_FID.fetch_add(1, Ordering::Relaxed);
        client.walk(1, fid, &path).unwrap();
        client.open(fid, OpenMode::write_append()).unwrap();
        client.write(fid, payload.as_bytes()).unwrap();
        client.clunk(fid).unwrap();
    }

    #[derive(Debug)]
    struct TestClock {
        now: Mutex<Instant>,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                now: Mutex::new(Instant::now()),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut guard = self.now.lock().unwrap();
            *guard = *guard + duration;
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> Instant {
            *self.now.lock().unwrap()
        }
    }
}
