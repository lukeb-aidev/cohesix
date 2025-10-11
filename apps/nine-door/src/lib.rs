// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! NineDoor Secure9P server implementing the Milestone 3 deliverables from
//! `docs/BUILD_PLAN.md`. The implementation provides an in-process transport
//! suitable for host-side integration tests and the `cohsh` CLI while the
//! eventual seL4 runtime is constructed.

use std::collections::HashMap;
use std::fmt;
use std::str;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::{
    Codec, CodecError, ErrorCode, FrameHeader, OpenMode, Qid, Request, RequestBody, Response,
    ResponseBody, SessionId, MAX_MSIZE, VERSION,
};
use thiserror::Error;

mod control;
mod namespace;

use control::{BudgetCommand, KillCommand, QueenCommand, SpawnCommand, SpawnTarget};
use namespace::Namespace;

/// Errors surfaced by NineDoor operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NineDoorError {
    /// Indicates that no session state exists for the supplied identifier.
    #[error("unknown session {0:?}")]
    UnknownSession(SessionId),
    /// Codec failure while parsing or serialising frames.
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    /// Protocol-visible error mapped to a Secure9P error response.
    #[error("{code}: {message}")]
    Protocol {
        /// Secure9P error code propagated to clients.
        code: ErrorCode,
        /// Human-readable message accompanying the error.
        message: String,
    },
}

impl NineDoorError {
    fn protocol(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Protocol {
            code,
            message: message.into(),
        }
    }
}

/// Time source abstraction used for budget enforcement.
pub trait Clock: Send + Sync {
    /// Return the current instant.
    fn now(&self) -> Instant;
}

/// System clock implementation backed by `Instant::now`.
#[derive(Debug, Default)]
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// In-process Secure9P server exposing connection handles.
#[derive(Clone)]
pub struct NineDoor {
    inner: Arc<Mutex<ServerCore>>,
    bootstrap_ticket: TicketTemplate,
}

impl fmt::Debug for NineDoor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NineDoor")
            .field("bootstrap_ticket", &self.bootstrap_ticket)
            .finish_non_exhaustive()
    }
}

impl NineDoor {
    /// Construct a new NineDoor server populated with the synthetic namespace.
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_clock(Arc::new(SystemClock::default()))
    }

    /// Construct a server using the supplied clock (primarily for tests).
    #[must_use]
    pub fn new_with_clock(clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ServerCore::new(clock))),
            bootstrap_ticket: TicketTemplate::new(Role::Queen, BudgetSpec::unbounded()),
        }
    }

    /// Create a new in-process connection representing a Secure9P session.
    pub fn connect(&self) -> Result<InProcessConnection, NineDoorError> {
        let session = {
            let mut core = self.inner.lock().expect("poisoned nine-door lock");
            core.allocate_session()
        };
        Ok(InProcessConnection::new(self.inner.clone(), session))
    }

    /// Retrieve the negotiated frame header for the bootstrap session.
    pub fn describe_bootstrap_session(&self) -> FrameHeader {
        FrameHeader::new(SessionId::BOOTSTRAP, 0)
    }

    /// Borrow the bootstrap ticket template used for queen sessions.
    #[must_use]
    pub fn bootstrap_ticket(&self) -> &TicketTemplate {
        &self.bootstrap_ticket
    }
}

impl Default for NineDoor {
    fn default() -> Self {
        Self::new()
    }
}

/// Client-side handle used by the CLI and tests to exercise the Secure9P stack.
pub struct InProcessConnection {
    server: Arc<Mutex<ServerCore>>,
    codec: Codec,
    session: SessionId,
    next_tag: u16,
    negotiated_msize: u32,
}

impl fmt::Debug for InProcessConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcessConnection")
            .field("session", &self.session)
            .field("next_tag", &self.next_tag)
            .field("negotiated_msize", &self.negotiated_msize)
            .finish()
    }
}

impl InProcessConnection {
    fn new(server: Arc<Mutex<ServerCore>>, session: SessionId) -> Self {
        Self {
            server,
            codec: Codec::default(),
            session,
            next_tag: 1,
            negotiated_msize: MAX_MSIZE,
        }
    }

    fn next_tag(&mut self) -> u16 {
        let tag = self.next_tag;
        self.next_tag = self.next_tag.wrapping_add(1);
        tag
    }

    fn transact(&mut self, body: RequestBody) -> Result<ResponseBody, NineDoorError> {
        let tag = self.next_tag();
        let request = Request { tag, body };
        let encoded = self.codec.encode_request(&request)?;
        let response_bytes = {
            let mut core = self.server.lock().expect("poisoned nine-door lock");
            core.handle_frame(self.session, &encoded)?
        };
        let response = self.codec.decode_response(&response_bytes)?;
        debug_assert_eq!(response.tag, tag);
        match response.body {
            ResponseBody::Error { code, message } => Err(NineDoorError::Protocol { code, message }),
            other => Ok(other),
        }
    }

    /// Negotiate Secure9P version and maximum message size.
    pub fn version(&mut self, requested_msize: u32) -> Result<u32, NineDoorError> {
        let response = self.transact(RequestBody::Version {
            msize: requested_msize,
            version: VERSION.to_string(),
        })?;
        let ResponseBody::Version { msize, version } = response else {
            unreachable!("version response must be Rversion");
        };
        if version != VERSION {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("unexpected version {version}"),
            ));
        }
        self.negotiated_msize = msize;
        Ok(msize)
    }

    /// Attach to the namespace using the supplied fid and role.
    pub fn attach(&mut self, fid: u32, role: Role) -> Result<Qid, NineDoorError> {
        self.attach_with_identity(fid, role, None)
    }

    /// Attach to the namespace providing an explicit identity string.
    pub fn attach_with_identity(
        &mut self,
        fid: u32,
        role: Role,
        identity: Option<&str>,
    ) -> Result<Qid, NineDoorError> {
        let response = self.transact(RequestBody::Attach {
            fid,
            afid: u32::MAX,
            uname: role_to_uname(role, identity),
            aname: "".to_owned(),
            n_uname: 0,
        })?;
        let ResponseBody::Attach { qid } = response else {
            unreachable!("attach response must be Rattach");
        };
        Ok(qid)
    }

    /// Walk from `fid` to `newfid` following the supplied path components.
    pub fn walk(
        &mut self,
        fid: u32,
        newfid: u32,
        path: &[String],
    ) -> Result<Vec<Qid>, NineDoorError> {
        let response = self.transact(RequestBody::Walk {
            fid,
            newfid,
            wnames: path.to_vec(),
        })?;
        let ResponseBody::Walk { qids } = response else {
            unreachable!("walk response must be Rwalk");
        };
        Ok(qids)
    }

    /// Open a fid with the specified mode.
    pub fn open(&mut self, fid: u32, mode: OpenMode) -> Result<(Qid, u32), NineDoorError> {
        let response = self.transact(RequestBody::Open { fid, mode })?;
        let ResponseBody::Open { qid, iounit } = response else {
            unreachable!("open response must be Ropen");
        };
        Ok((qid, iounit))
    }

    /// Read bytes from an opened fid.
    pub fn read(&mut self, fid: u32, offset: u64, count: u32) -> Result<Vec<u8>, NineDoorError> {
        let response = self.transact(RequestBody::Read { fid, offset, count })?;
        let ResponseBody::Read { data } = response else {
            unreachable!("read response must be Rread");
        };
        Ok(data)
    }

    /// Append bytes to an opened fid.
    pub fn write(&mut self, fid: u32, data: &[u8]) -> Result<u32, NineDoorError> {
        let response = self.transact(RequestBody::Write {
            fid,
            offset: u64::MAX,
            data: data.to_vec(),
        })?;
        let ResponseBody::Write { count } = response else {
            unreachable!("write response must be Rwrite");
        };
        Ok(count)
    }

    /// Release a fid.
    pub fn clunk(&mut self, fid: u32) -> Result<(), NineDoorError> {
        let response = self.transact(RequestBody::Clunk { fid })?;
        let ResponseBody::Clunk = response else {
            unreachable!("clunk response must be Rclunk");
        };
        Ok(())
    }

    /// Access the negotiated message size for the session.
    #[must_use]
    pub fn negotiated_msize(&self) -> u32 {
        self.negotiated_msize
    }

    /// Return the underlying session identifier.
    #[must_use]
    pub fn session_id(&self) -> SessionId {
        self.session
    }
}

// New server core implementation and access policy are defined below.

/// Internal server state shared between connections.
struct ServerCore {
    codec: Codec,
    control: ControlPlane,
    next_session: u64,
    sessions: HashMap<SessionId, SessionState>,
    clock: Arc<dyn Clock>,
}

impl ServerCore {
    fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            codec: Codec::default(),
            control: ControlPlane::new(),
            next_session: 1,
            sessions: HashMap::new(),
            clock,
        }
    }

    fn allocate_session(&mut self) -> SessionId {
        let id = SessionId::from_raw(self.next_session);
        self.next_session += 1;
        let now = self.clock.now();
        self.sessions.insert(id, SessionState::new(now));
        id
    }

    fn handle_frame(
        &mut self,
        session: SessionId,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, NineDoorError> {
        let request = self.codec.decode_request(request_bytes)?;
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
        let mut state = self
            .sessions
            .remove(&session)
            .ok_or(NineDoorError::UnknownSession(session))?;
        let result = match &request.body {
            RequestBody::Version { msize, version } => {
                Self::handle_version(&mut state, *msize, version)
            }
            RequestBody::Attach { fid, uname, .. } => {
                self.handle_attach(&mut state, *fid, uname.as_str())
            }
            RequestBody::Walk {
                fid,
                newfid,
                wnames,
            } => {
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
                state.ensure_attached()?;
                Self::handle_clunk(&mut state, *fid)
            }
        };
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
                state.configure_role(role, identity, BudgetSpec::unbounded(), now);
            }
            Role::WorkerHeartbeat => {
                let worker_id = identity.clone().ok_or_else(|| {
                    NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "worker-heartbeat attach requires identity",
                    )
                })?;
                let Some(spec) = self.control.worker_budget(&worker_id) else {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        format!("worker {worker_id} not found"),
                    ));
                };
                state.configure_role(role, Some(worker_id), spec, now);
            }
            Role::WorkerGpu => {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    "worker-gpu role unsupported in this milestone",
                ));
            }
        }
        let qid = self.control.namespace().root_qid();
        state.insert_fid(fid, Vec::new(), qid);
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
        AccessPolicy::ensure_walk(state.role(), state.worker_id(), &existing.path, wnames)?;
        let (path, qids) = self.control.namespace_mut().walk(&existing.path, wnames)?;
        let qid = qids.last().copied().unwrap_or(existing.qid);
        state.insert_fid(newfid, path, qid);
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
        let iounit = state.negotiated_msize();
        let qid = {
            let entry = state.fid_mut(fid).ok_or_else(|| {
                NineDoorError::protocol(ErrorCode::NotFound, format!("fid {fid} not found"))
            })?;
            AccessPolicy::ensure_open(role, worker_id, &entry.path, mode)?;
            let node = self.control.namespace().lookup(&entry.path)?;
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
        AccessPolicy::ensure_read(state.role(), state.worker_id(), &entry.path)?;
        let data = self.control.namespace().read(&entry.path, offset, count)?;
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
            entry.path.clone()
        };
        AccessPolicy::ensure_write(role, worker_id, &path)?;
        let telemetry_write = worker_id
            .map(|id| is_worker_telemetry_path(&path, id))
            .unwrap_or(false);
        if telemetry_write {
            if let Err(reason) = state.consume_tick() {
                return Err(self.handle_budget_failure(session, state, reason));
            }
        }
        if is_queen_ctl_path(&path) {
            let events = self.control.process_queen_write(data)?;
            self.process_queen_events(events, session);
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

    fn process_queen_events(&mut self, events: Vec<QueenEvent>, current_session: SessionId) {
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
                }
                QueenEvent::BudgetUpdated => {}
            }
        }
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
}

impl ControlPlane {
    fn new() -> Self {
        Self {
            namespace: Namespace::new(),
            workers: HashMap::new(),
            next_worker_id: 1,
            default_budget: BudgetSpec::default_heartbeat(),
        }
    }

    fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    fn namespace_mut(&mut self) -> &mut Namespace {
        &mut self.namespace
    }

    fn worker_budget(&self, worker_id: &str) -> Option<BudgetSpec> {
        self.workers.get(worker_id).map(|record| record.budget)
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
            }
        }
        Ok(events)
    }

    fn spawn_worker(&mut self, spec: &SpawnCommand) -> Result<String, NineDoorError> {
        let SpawnCommand { spawn, .. } = spec;
        match spawn {
            SpawnTarget::Heartbeat => {}
        }
        let worker_id = format!("worker-{}", self.next_worker_id);
        self.next_worker_id += 1;
        let budget = spec.budget_spec(self.default_budget);
        self.namespace.create_worker(&worker_id)?;
        self.workers
            .insert(worker_id.clone(), WorkerRecord { budget });
        self.log_event(&format!(
            "spawned {worker_id} ticks={} ttl={} ops={}",
            format_budget_value(budget.ticks()),
            format_budget_value(budget.ttl_s()),
            format_budget_value(budget.ops())
        ))?;
        Ok(worker_id)
    }

    fn kill_worker(&mut self, worker_id: &str) -> Result<(), NineDoorError> {
        if self.workers.remove(worker_id).is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("worker {worker_id} not found"),
            ));
        }
        self.namespace.remove_worker(worker_id)?;
        self.log_event(&format!("killed {worker_id}"))?;
        Ok(())
    }

    fn update_default_budget(&mut self, payload: &BudgetCommand) -> Result<(), NineDoorError> {
        self.default_budget = payload.apply(self.default_budget);
        let budget = self.default_budget;
        self.log_event(&format!(
            "updated default budget ttl={} ops={} ticks={}",
            format_budget_value(budget.ttl_s()),
            format_budget_value(budget.ops()),
            format_budget_value(budget.ticks())
        ))?;
        Ok(())
    }

    fn revoke_worker(&mut self, worker_id: &str, reason: &str) -> Result<bool, NineDoorError> {
        let Some(_record) = self.workers.remove(worker_id) else {
            return Ok(false);
        };
        if let Err(err) = self.namespace.remove_worker(worker_id) {
            if let NineDoorError::Protocol { code, .. } = &err {
                if *code != ErrorCode::NotFound {
                    return Err(err);
                }
            }
        }
        self.log_event(&format!("revoked {worker_id}: {reason}"))?;
        Ok(true)
    }

    fn log_event(&mut self, message: &str) -> Result<(), NineDoorError> {
        let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
        let mut line = message.as_bytes().to_vec();
        line.push(b'\n');
        self.namespace.write_append(&log_path, &line)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct WorkerRecord {
    budget: BudgetSpec,
}

enum QueenEvent {
    Spawned(String),
    Killed(String),
    BudgetUpdated,
}

/// Tracks per-session state including budget counters.
struct SessionState {
    msize: Option<u32>,
    attached: bool,
    fids: HashMap<u32, FidState>,
    role: Option<Role>,
    worker_id: Option<String>,
    budget: BudgetState,
}

impl SessionState {
    fn new(now: Instant) -> Self {
        Self {
            msize: None,
            attached: false,
            fids: HashMap::new(),
            role: None,
            worker_id: None,
            budget: BudgetState::new(BudgetSpec::unbounded(), now),
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

    fn insert_fid(&mut self, fid: u32, path: Vec<String>, qid: Qid) {
        self.fids.insert(
            fid,
            FidState {
                path,
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
        budget: BudgetSpec,
        now: Instant,
    ) {
        self.role = Some(role);
        self.worker_id = identity;
        self.budget = BudgetState::new(budget, now);
    }

    fn role(&self) -> Option<Role> {
        self.role
    }

    fn worker_id(&self) -> Option<&str> {
        self.worker_id.as_deref()
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
    path: Vec<String>,
    qid: Qid,
    open_mode: Option<OpenMode>,
}

struct AccessPolicy;

impl AccessPolicy {
    fn ensure_walk(
        role: Option<Role>,
        worker_id: Option<&str>,
        start: &[String],
        components: &[String],
    ) -> Result<(), NineDoorError> {
        let mut path = start.to_vec();
        for component in components {
            path.push(component.clone());
            Self::ensure_path(role, worker_id, &path)?;
        }
        Ok(())
    }

    fn ensure_open(
        role: Option<Role>,
        worker_id: Option<&str>,
        path: &[String],
        mode: OpenMode,
    ) -> Result<(), NineDoorError> {
        Self::ensure_path(role, worker_id, path)?;
        if mode.allows_write() {
            Self::ensure_write(role, worker_id, path)?;
        }
        if mode.allows_read() {
            Self::ensure_read(role, worker_id, path)?;
        }
        Ok(())
    }

    fn ensure_read(
        role: Option<Role>,
        worker_id: Option<&str>,
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
            Some(Role::WorkerGpu) => Err(Self::permission_denied(path)),
            None => Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "attach required before operation",
            )),
        }
    }

    fn ensure_write(
        role: Option<Role>,
        worker_id: Option<&str>,
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
            Some(Role::WorkerGpu) => Err(Self::permission_denied(path)),
            None => Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "attach required before operation",
            )),
        }
    }

    fn ensure_path(
        role: Option<Role>,
        worker_id: Option<&str>,
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
            Some(Role::WorkerGpu) => Err(Self::permission_denied(path)),
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

fn format_budget_value(value: Option<u64>) -> String {
    value.map_or_else(|| "∞".to_owned(), |v| v.to_string())
}

fn is_worker_telemetry_path(path: &[String], worker_id: &str) -> bool {
    matches!(path, [first, second, third]
        if first == "worker" && second == worker_id && third == "telemetry")
}

fn is_queen_ctl_path(path: &[String]) -> bool {
    matches!(path, [first, second] if first == "queen" && second == "ctl")
}

fn parse_role_from_uname(uname: &str) -> Result<(Role, Option<String>), NineDoorError> {
    if uname == "queen" {
        return Ok((Role::Queen, None));
    }
    if let Some(rest) = uname.strip_prefix("worker-heartbeat:") {
        if rest.is_empty() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "worker-heartbeat identity cannot be empty",
            ));
        }
        return Ok((Role::WorkerHeartbeat, Some(rest.to_owned())));
    }
    Err(NineDoorError::protocol(
        ErrorCode::Invalid,
        format!("unknown role string '{uname}'"),
    ))
}

fn role_to_uname(role: Role, identity: Option<&str>) -> String {
    match role {
        Role::Queen => "queen".to_owned(),
        Role::WorkerHeartbeat => {
            let id = identity.expect("worker heartbeat requires identity");
            format!("worker-heartbeat:{id}")
        }
        Role::WorkerGpu => "worker-gpu".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
