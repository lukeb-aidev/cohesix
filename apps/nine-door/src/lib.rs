// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! NineDoor Secure9P server implementing the Milestone 2 deliverables from
//! `docs/BUILD_PLAN.md`. The implementation provides an in-process transport
//! suitable for host-side integration tests and the `cohsh` CLI while the
//! eventual seL4 runtime is constructed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::{
    Codec, CodecError, ErrorCode, FrameHeader, OpenMode, Qid, Request, RequestBody, Response,
    ResponseBody, SessionId, MAX_MSIZE, VERSION,
};
use thiserror::Error;

mod namespace;

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

/// In-process Secure9P server exposing connection handles.
#[derive(Debug, Clone)]
pub struct NineDoor {
    inner: Arc<Mutex<ServerCore>>,
    bootstrap_ticket: TicketTemplate,
}

impl NineDoor {
    /// Construct a new NineDoor server populated with the synthetic namespace.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ServerCore::new())),
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
#[derive(Debug)]
pub struct InProcessConnection {
    server: Arc<Mutex<ServerCore>>,
    codec: Codec,
    session: SessionId,
    next_tag: u16,
    negotiated_msize: u32,
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
        let response = self.transact(RequestBody::Attach {
            fid,
            afid: u32::MAX,
            uname: role_to_uname(role),
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

/// Internal server state shared between connections.
#[derive(Debug)]
struct ServerCore {
    codec: Codec,
    namespace: Namespace,
    next_session: u64,
    sessions: HashMap<SessionId, SessionState>,
}

impl ServerCore {
    fn new() -> Self {
        Self {
            codec: Codec::default(),
            namespace: Namespace::new(),
            next_session: 1,
            sessions: HashMap::new(),
        }
    }

    fn allocate_session(&mut self) -> SessionId {
        let id = SessionId::from_raw(self.next_session);
        self.next_session += 1;
        self.sessions.insert(id, SessionState::new());
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
        let namespace = &mut self.namespace;
        let sessions = &mut self.sessions;
        let state = sessions
            .get_mut(&session)
            .ok_or(NineDoorError::UnknownSession(session))?;
        match &request.body {
            RequestBody::Version { msize, version } => Self::handle_version(state, *msize, version),
            RequestBody::Attach { fid, .. } => Self::handle_attach(namespace, state, *fid),
            RequestBody::Walk {
                fid,
                newfid,
                wnames,
            } => Self::handle_walk(namespace, state, *fid, *newfid, wnames),
            RequestBody::Open { fid, mode } => Self::handle_open(namespace, state, *fid, *mode),
            RequestBody::Read { fid, offset, count } => {
                Self::handle_read(namespace, state, *fid, *offset, *count)
            }
            RequestBody::Write { fid, data, .. } => {
                Self::handle_write(namespace, state, *fid, data)
            }
            RequestBody::Clunk { fid } => Self::handle_clunk(state, *fid),
        }
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
        state.msize = Some(negotiated);
        Ok(ResponseBody::Version {
            msize: negotiated,
            version: VERSION.to_string(),
        })
    }

    fn handle_attach(
        namespace: &mut Namespace,
        state: &mut SessionState,
        fid: u32,
    ) -> Result<ResponseBody, NineDoorError> {
        if state.msize.is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "version negotiation required before attach",
            ));
        }
        if state.fids.contains_key(&fid) {
            return Err(NineDoorError::protocol(
                ErrorCode::Busy,
                format!("fid {fid} already in use"),
            ));
        }
        let qid = namespace.root_qid();
        state.fids.insert(
            fid,
            FidState {
                path: Vec::new(),
                qid,
                open_mode: None,
            },
        );
        state.attached = true;
        Ok(ResponseBody::Attach { qid })
    }

    fn handle_walk(
        namespace: &mut Namespace,
        state: &mut SessionState,
        fid: u32,
        newfid: u32,
        wnames: &[String],
    ) -> Result<ResponseBody, NineDoorError> {
        if !state.attached {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "attach required before walk",
            ));
        }
        let Some(existing) = state.fids.get(&fid) else {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("fid {fid} not found"),
            ));
        };
        let (path, qids) = namespace.walk(&existing.path, wnames)?;
        let qid = qids.last().copied().unwrap_or(existing.qid);
        state.fids.insert(
            newfid,
            FidState {
                path,
                qid,
                open_mode: None,
            },
        );
        Ok(ResponseBody::Walk { qids })
    }

    fn handle_open(
        namespace: &mut Namespace,
        state: &mut SessionState,
        fid: u32,
        mode: OpenMode,
    ) -> Result<ResponseBody, NineDoorError> {
        let Some(entry) = state.fids.get_mut(&fid) else {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("fid {fid} not found"),
            ));
        };
        let node = namespace.lookup(&entry.path)?;
        if node.is_directory() {
            if mode.allows_write() {
                return Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    "cannot write directories",
                ));
            }
        } else if mode.allows_write() && !node.qid().ty().is_append_only() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "fid is not append-only",
            ));
        }
        if !mode.allows_read() && !mode.allows_write() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "open requires read or write access",
            ));
        }
        entry.open_mode = Some(mode);
        Ok(ResponseBody::Open {
            qid: node.qid(),
            iounit: state.msize.unwrap_or(MAX_MSIZE),
        })
    }

    fn handle_read(
        namespace: &mut Namespace,
        state: &mut SessionState,
        fid: u32,
        offset: u64,
        count: u32,
    ) -> Result<ResponseBody, NineDoorError> {
        let Some(entry) = state.fids.get(&fid) else {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("fid {fid} not found"),
            ));
        };
        let Some(mode) = entry.open_mode else {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "fid must be opened before read",
            ));
        };
        if !mode.allows_read() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "fid opened without read permission",
            ));
        }
        let data = namespace.read(&entry.path, offset, count)?;
        Ok(ResponseBody::Read { data })
    }

    fn handle_write(
        namespace: &mut Namespace,
        state: &mut SessionState,
        fid: u32,
        data: &[u8],
    ) -> Result<ResponseBody, NineDoorError> {
        let Some(entry) = state.fids.get(&fid) else {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("fid {fid} not found"),
            ));
        };
        let Some(mode) = entry.open_mode else {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "fid must be opened before write",
            ));
        };
        if !mode.allows_write() {
            return Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "fid opened without write permission",
            ));
        }
        let count = namespace.write_append(&entry.path, data)?;
        Ok(ResponseBody::Write { count })
    }

    fn handle_clunk(state: &mut SessionState, fid: u32) -> Result<ResponseBody, NineDoorError> {
        if state.fids.remove(&fid).is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::Closed,
                format!("fid {fid} already closed"),
            ));
        }
        Ok(ResponseBody::Clunk)
    }
}

/// Tracks per-session state.
#[derive(Debug)]
struct SessionState {
    msize: Option<u32>,
    attached: bool,
    fids: HashMap<u32, FidState>,
}

impl SessionState {
    fn new() -> Self {
        Self {
            msize: None,
            attached: false,
            fids: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct FidState {
    path: Vec<String>,
    qid: Qid,
    open_mode: Option<OpenMode>,
}

fn role_to_uname(role: Role) -> String {
    match role {
        Role::Queen => "queen".to_owned(),
        Role::WorkerHeartbeat => "worker-heartbeat".to_owned(),
        Role::WorkerGpu => "worker-gpu".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secure9p_wire::OpenMode;

    #[test]
    fn double_clunk_returns_closed() {
        let server = NineDoor::new();
        let mut client = server.connect().unwrap();
        client.version(MAX_MSIZE).unwrap();
        client.attach(1, Role::Queen).unwrap();
        client.clunk(1).unwrap();
        let err = client.clunk(1).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Closed,
                ..
            }
        ));
    }

    #[test]
    fn write_to_proc_boot_is_rejected() {
        let server = NineDoor::new();
        let mut client = server.connect().unwrap();
        client.version(MAX_MSIZE).unwrap();
        client.attach(1, Role::Queen).unwrap();
        let path = vec!["proc".to_owned(), "boot".to_owned()];
        client.walk(1, 2, &path).unwrap();
        let err = client.open(2, OpenMode::write_append()).unwrap_err();
        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Permission,
                ..
            }
        ));
    }
}
