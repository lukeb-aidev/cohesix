// Author: Lukas Bower
// Purpose: Core NineDoor Secure9P server state machine and namespace plumbing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
use cohesix_ticket::{BudgetSpec, Role};
use gpu_bridge_host::{status_entry, GpuNamespaceSnapshot};
use log::{debug, info, trace};
use secure9p_wire::{
    Codec, ErrorCode, OpenMode, Qid, Request, RequestBody, Response, ResponseBody, SessionId,
    MAX_MSIZE, VERSION,
};
use trace_model::TraceLevel;
use worker_gpu::{GpuLease as WorkerGpuLease, JobDescriptor};

use super::control::{BudgetCommand, KillCommand, QueenCommand, SpawnCommand, SpawnTarget};
use super::namespace::Namespace;
use super::{Clock, NineDoorError};

// New server core implementation and access policy are defined below.

/// Internal server state shared between connections.
pub(crate) struct ServerCore {
    codec: Codec,
    control: ControlPlane,
    next_session: u64,
    sessions: HashMap<SessionId, SessionState>,
    clock: Arc<dyn Clock>,
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
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            codec: Codec,
            control: ControlPlane::new(),
            next_session: 1,
            sessions: HashMap::new(),
            clock,
        }
    }

    pub(crate) fn allocate_session(&mut self) -> SessionId {
        let id = SessionId::from_raw(self.next_session);
        self.next_session += 1;
        let now = self.clock.now();
        self.sessions.insert(id, SessionState::new(now));
        id
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

    pub(crate) fn handle_frame(
        &mut self,
        session: SessionId,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, NineDoorError> {
        let request = self.codec.decode_request(request_bytes).map_err(|err| {
            let state = self
                .sessions
                .get(&session)
                .map(|entry| entry.auth_state)
                .unwrap_or(AuthState::Failed);
            debug!(
                "[net-console][auth] session={} state={:?} decode error: {}",
                session.session(),
                state,
                err
            );
            err
        })?;
        let response_body = match self.dispatch(session, &request) {
            Ok(body) => body,
            Err(NineDoorError::Protocol { code, message }) => ResponseBody::Error { code, message },
            Err(other) => return Err(other),
        };
        let response = Response {
            tag: request.tag,
            body: response_body,
        };
        Ok(self.codec.encode_response(&response)?)
    }

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
                let outcome = Self::handle_version(&mut state, *msize, version);
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
            RequestBody::Attach { fid, uname, .. } => {
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
                let outcome = self.handle_attach(&mut state, *fid, uname.as_str());
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
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else {
                    self.handle_walk(&mut state, *fid, *newfid, wnames)
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
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else {
                    self.handle_open(&mut state, *fid, *mode)
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
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else {
                    self.handle_read(&mut state, *fid, *offset, *count)
                }
            }
            RequestBody::Write { fid, data, .. } => {
                trace!(
                    "[net-console][auth] session={} state={:?} recv Twrite fid={} payload_len={}",
                    session.session(),
                    state.auth_state,
                    fid,
                    data.len()
                );
                state.ensure_attached()?;
                if let Err(reason) = state.pre_operation(self.clock.now()) {
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else if let Err(reason) = state.consume_operation() {
                    Err(self.handle_budget_failure(session, &mut state, reason))
                } else {
                    self.handle_write(session, &mut state, *fid, data)
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
                Self::handle_clunk(&mut state, *fid)
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
        self.sessions.insert(session, state);
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
    ) -> Result<ResponseBody, NineDoorError> {
        if state.msize().is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "version negotiation required before attach",
            ));
        }
        if state.has_fid(fid) {
            return Err(NineDoorError::protocol(
                ErrorCode::Busy,
                format!("fid {fid} already in use"),
            ));
        }
        let (role, identity) = parse_role_from_uname(uname)?;
        let now = self.clock.now();
        match role {
            Role::Queen => {
                state.configure_role(role, identity, None, BudgetSpec::unbounded(), now);
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
                        state.configure_role(role, Some(worker_id), None, record.budget(), now);
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
                        state.configure_role(
                            role,
                            Some(worker_id),
                            Some(gpu.lease.gpu_id.clone()),
                            record.budget(),
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
        }
        let qid = self.control.namespace().root_qid();
        state.insert_fid(fid, Vec::new(), Vec::new(), qid);
        state.mark_attached();
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
        if wnames.is_empty() {
            state.insert_fid(
                newfid,
                existing.view_path.clone(),
                existing.canonical_path.clone(),
                existing.qid,
            );
            return Ok(ResponseBody::Walk { qids: Vec::new() });
        }
        let mut qids = Vec::with_capacity(wnames.len());
        let mut view_path = existing.view_path.clone();
        let mut canonical_path = existing.canonical_path.clone();
        let mut current_qid = existing.qid;
        for component in wnames {
            view_path.push(component.clone());
            let resolved = state.resolve_view_path(&view_path);
            AccessPolicy::ensure_path(role, worker_id, gpu_scope, &resolved)?;
            let node = self.control.namespace().lookup(&resolved)?;
            current_qid = node.qid();
            qids.push(current_qid);
            canonical_path = resolved;
        }
        state.insert_fid(newfid, view_path, canonical_path, current_qid);
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
        let iounit = state.negotiated_msize();
        let qid = {
            let entry = state.fid_mut(fid).ok_or_else(|| {
                NineDoorError::protocol(ErrorCode::NotFound, format!("fid {fid} not found"))
            })?;
            AccessPolicy::ensure_open(role, worker_id, gpu_scope, &entry.canonical_path, mode)?;
            let node = self.control.namespace().lookup(&entry.canonical_path)?;
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
            entry.open_mode = Some(mode);
            node.qid()
        };
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
        let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
        let gpu_scope = gpu_scope_owned.as_deref();
        AccessPolicy::ensure_read(
            state.role(),
            state.worker_id(),
            gpu_scope,
            &entry.canonical_path,
        )?;
        let data = self
            .control
            .namespace()
            .read(&entry.canonical_path, offset, count)?;
        Ok(ResponseBody::Read { data })
    }

    fn handle_write(
        &mut self,
        session: SessionId,
        state: &mut SessionState,
        fid: u32,
        data: &[u8],
    ) -> Result<ResponseBody, NineDoorError> {
        let role = state.role();
        let worker_id_owned = state.worker_id().map(|id| id.to_owned());
        let worker_id = worker_id_owned.as_deref();
        let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
        let gpu_scope = gpu_scope_owned.as_deref();
        let path = {
            let entry = state.fid_mut(fid).ok_or_else(|| {
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
        AccessPolicy::ensure_write(role, worker_id, gpu_scope, &path)?;
        let telemetry_write = worker_id
            .map(|id| is_worker_telemetry_path(&path, id))
            .unwrap_or(false);
        if telemetry_write {
            if let Err(reason) = state.consume_tick() {
                return Err(self.handle_budget_failure(session, state, reason));
            }
        }
        if let (Some(worker), Some(scope)) = (worker_id, gpu_scope) {
            if is_gpu_job_path(&path, scope) {
                let count = self.control.process_gpu_job(worker, scope, data)?;
                return Ok(ResponseBody::Write { count });
            }
        }
        if is_queen_ctl_path(&path) {
            let events = self.control.process_queen_write(data)?;
            let role = state.role();
            let worker_id_owned = state.worker_id().map(|id| id.to_owned());
            let worker_id = worker_id_owned.as_deref();
            let gpu_scope_owned = state.gpu_scope().map(|scope| scope.to_owned());
            let gpu_scope = gpu_scope_owned.as_deref();
            for event in &events {
                match event {
                    QueenEvent::Bound { target, mount } | QueenEvent::Mounted { target, mount } => {
                        state.apply_mount(role, worker_id, gpu_scope, target, mount)?;
                    }
                    _ => {}
                }
            }
            self.process_queen_events(events, session)?;
            Ok(ResponseBody::Write {
                count: data.len() as u32,
            })
        } else {
            let count = self.control.namespace_mut().write_append(&path, data)?;
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
}

struct ControlPlane {
    namespace: Namespace,
    workers: HashMap<String, WorkerRecord>,
    next_worker_id: u64,
    default_budget: BudgetSpec,
    services: HashMap<String, Vec<String>>,
    gpu_nodes: HashSet<String>,
    active_leases: HashMap<String, String>,
}

impl ControlPlane {
    fn new() -> Self {
        Self {
            namespace: Namespace::new(),
            workers: HashMap::new(),
            next_worker_id: 1,
            default_budget: BudgetSpec::default_heartbeat(),
            services: HashMap::new(),
            gpu_nodes: HashSet::new(),
            active_leases: HashMap::new(),
        }
    }

    fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    fn namespace_mut(&mut self) -> &mut Namespace {
        &mut self.namespace
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

    fn process_queen_write(&mut self, data: &[u8]) -> Result<Vec<QueenEvent>, NineDoorError> {
        let ctl_path = vec!["queen".to_owned(), "ctl".to_owned()];
        self.namespace.write_append(&ctl_path, data)?;
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
            let command = QueenCommand::parse(trimmed)?;
            match command {
                QueenCommand::Spawn(spec) => {
                    let worker_id = self.spawn_worker(&spec)?;
                    events.push(QueenEvent::Spawned(worker_id));
                }
                QueenCommand::Kill(KillCommand { kill }) => {
                    self.kill_worker(&kill)?;
                    events.push(QueenEvent::Killed(kill));
                }
                QueenCommand::Budget(payload) => {
                    self.update_default_budget(&payload)?;
                    events.push(QueenEvent::BudgetUpdated);
                }
                QueenCommand::Bind(command) => {
                    let (from_raw, to_raw, source, mount) = command.into_parts()?;
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
                }
                QueenCommand::Mount(command) => {
                    let (service, at_raw, mount) = command.into_parts()?;
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
                }
            }
        }
        Ok(events)
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
        let count = self.namespace.write_append(&job_path, data)?;
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
                .write_append(&telemetry_path, telemetry.as_bytes())?;
            let telemetry_done = format!(
                "{{\"job\":\"{}\",\"state\":\"OK\",\"detail\":\"completed\"}}\n",
                job_id
            );
            self.namespace
                .write_append(&telemetry_path, telemetry_done.as_bytes())?;
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
                self.namespace.write_append(&ctl_path, &message)?;
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

    fn append_queen_log(&mut self, message: &str) -> Result<(), NineDoorError> {
        let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
        let mut line = message.as_bytes().to_vec();
        line.push(b'\n');
        self.namespace.write_append(&log_path, &line)?;
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
                .write_append(&ctl_path, ctl_line.as_bytes())?;
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
    fids: HashMap<u32, FidState>,
    role: Option<Role>,
    worker_id: Option<String>,
    gpu_scope: Option<String>,
    budget: BudgetState,
    mounts: MountTable,
    auth_state: AuthState,
    first_request_logged: bool,
}

impl SessionState {
    fn new(now: Instant) -> Self {
        Self {
            msize: None,
            attached: false,
            fids: HashMap::new(),
            role: None,
            worker_id: None,
            gpu_scope: None,
            budget: BudgetState::new(BudgetSpec::unbounded(), now),
            mounts: MountTable::default(),
            auth_state: AuthState::Start,
            first_request_logged: false,
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

    fn has_fid(&self, fid: u32) -> bool {
        self.fids.contains_key(&fid)
    }

    fn insert_fid(
        &mut self,
        fid: u32,
        view_path: Vec<String>,
        canonical_path: Vec<String>,
        qid: Qid,
    ) {
        self.fids.insert(
            fid,
            FidState {
                view_path,
                canonical_path,
                qid,
                open_mode: None,
            },
        );
    }

    fn fid(&self, fid: u32) -> Option<&FidState> {
        self.fids.get(&fid)
    }

    fn fid_mut(&mut self, fid: u32) -> Option<&mut FidState> {
        self.fids.get_mut(&fid)
    }

    fn remove_fid(&mut self, fid: u32) -> Option<FidState> {
        self.fids.remove(&fid)
    }

    fn configure_role(
        &mut self,
        role: Role,
        identity: Option<String>,
        gpu_scope: Option<String>,
        budget: BudgetSpec,
        now: Instant,
    ) {
        self.role = Some(role);
        self.worker_id = identity;
        self.gpu_scope = gpu_scope;
        self.budget = BudgetState::new(budget, now);
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
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        target: &[String],
        mount: &[String],
    ) -> Result<(), NineDoorError> {
        AccessPolicy::ensure_path(role, worker_id, gpu_scope, target)?;
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
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        path: &[String],
        mode: OpenMode,
    ) -> Result<(), NineDoorError> {
        Self::ensure_path(role, worker_id, gpu_scope, path)?;
        if mode.allows_write() {
            Self::ensure_write(role, worker_id, gpu_scope, path)?;
        }
        if mode.allows_read() {
            Self::ensure_read(role, worker_id, gpu_scope, path)?;
        }
        Ok(())
    }

    fn ensure_read(
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        path: &[String],
    ) -> Result<(), NineDoorError> {
        match role {
            Some(Role::Queen) => Ok(()),
            Some(Role::WorkerHeartbeat) => {
                if worker_allowed_path(worker_id, path) {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerGpu) => {
                if worker_allowed_path(worker_id, path) || gpu_allowed_read(gpu_scope, path) {
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
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        path: &[String],
    ) -> Result<(), NineDoorError> {
        match role {
            Some(Role::Queen) => Ok(()),
            Some(Role::WorkerHeartbeat) => {
                if worker_allowed_write(worker_id, path) {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerGpu) => {
                if worker_allowed_write(worker_id, path) || gpu_allowed_write(gpu_scope, path) {
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
        role: Option<Role>,
        worker_id: Option<&str>,
        gpu_scope: Option<&str>,
        path: &[String],
    ) -> Result<(), NineDoorError> {
        match role {
            Some(Role::Queen) => Ok(()),
            Some(Role::WorkerHeartbeat) => {
                if worker_allowed_prefix(worker_id, path) {
                    Ok(())
                } else {
                    Err(Self::permission_denied(path))
                }
            }
            Some(Role::WorkerGpu) => {
                if worker_allowed_prefix(worker_id, path) || gpu_allowed_prefix(gpu_scope, path) {
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

fn worker_allowed_prefix(worker_id: Option<&str>, path: &[String]) -> bool {
    match worker_id {
        None => false,
        Some(id) => match path.len() {
            0 => true,
            1 => matches!(path[0].as_str(), "proc" | "log" | "worker"),
            2 => match (path[0].as_str(), path[1].as_str()) {
                ("proc", "boot") => true,
                ("log", "queen.log") => true,
                ("worker", other) => other == id,
                _ => false,
            },
            3 => path[0] == "worker" && path[1] == id && path[2] == "telemetry",
            _ => false,
        },
    }
}

fn worker_allowed_path(worker_id: Option<&str>, path: &[String]) -> bool {
    if !worker_allowed_prefix(worker_id, path) {
        return false;
    }
    match path {
        [] => true,
        [single] => single != "worker",
        [first, second] if first == "worker" => second != "self",
        _ => true,
    }
}

fn worker_allowed_write(worker_id: Option<&str>, path: &[String]) -> bool {
    match worker_id {
        Some(id) => matches!(path, [first, second, third]
            if first == "worker" && second == id && third == "telemetry"),
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

fn format_budget_value(value: Option<u64>) -> String {
    value.map_or_else(|| "∞".to_owned(), |v| v.to_string())
}

fn is_worker_telemetry_path(path: &[String], worker_id: &str) -> bool {
    matches!(path, [first, second, third]
        if first == "worker" && second == worker_id && third == "telemetry")
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InProcessConnection, NineDoor};
    use secure9p_wire::OpenMode;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn queen_spawn_creates_worker_directory() {
        let server = NineDoor::new();
        let mut queen = attach_queen(&server);
        write_queen_command(&mut queen, "{\"spawn\":\"heartbeat\",\"ticks\":3}\n");
        let worker_path = vec![
            "worker".to_owned(),
            "worker-1".to_owned(),
            "telemetry".to_owned(),
        ];
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
        let worker_path = vec![
            "worker".to_owned(),
            "worker-1".to_owned(),
            "telemetry".to_owned(),
        ];
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
        let worker_one_path = vec![
            "worker".to_owned(),
            "worker-1".to_owned(),
            "telemetry".to_owned(),
        ];
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
        let telemetry = vec![
            "worker".to_owned(),
            "worker-1".to_owned(),
            "telemetry".to_owned(),
        ];
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
        let telemetry = vec![
            "worker".to_owned(),
            "worker-1".to_owned(),
            "telemetry".to_owned(),
        ];
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
        let mut client = server.connect().unwrap();
        client.version(MAX_MSIZE).unwrap();
        client
            .attach_with_identity(1, Role::WorkerHeartbeat, Some(id))
            .unwrap();
        client
    }

    fn write_queen_command(client: &mut InProcessConnection, payload: &str) {
        let path = vec!["queen".to_owned(), "ctl".to_owned()];
        client.walk(1, 2, &path).unwrap();
        client.open(2, OpenMode::write_append()).unwrap();
        client.write(2, payload.as_bytes()).unwrap();
        client.clunk(2).unwrap();
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
