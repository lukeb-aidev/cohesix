// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Public NineDoor Secure9P server interface and in-process transport helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! NineDoor Secure9P server implementing the Milestone 3 deliverables from
//! `docs/BUILD_PLAN.md`. The implementation provides an in-process transport
//! suitable for host-side integration tests and the `cohsh` CLI while the
//! eventual seL4 runtime is constructed.

use std::fmt;
use std::io;
use std::str;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims};
use gpu_bridge_host::GpuNamespaceSnapshot;
use secure9p_codec::{
    Codec, CodecError, ErrorCode, FrameHeader, OpenMode, Qid, Request, RequestBody, ResponseBody,
    SessionId, MAX_MSIZE, VERSION,
};
use secure9p_core::SessionLimits;
use thiserror::Error;

mod control;
mod cas;
mod audit;
mod core;
mod namespace;
mod observe;
mod pipeline;
mod policy;
mod replay;
mod telemetry;
mod tracefs;

use self::core::{role_to_uname, ServerCore};
pub use self::cas::CasConfig;
pub use self::namespace::{
    HostNamespaceConfig, HostProvider, ShardLayout, SidecarBusAdapterConfig, SidecarBusConfig,
    SidecarLoraAdapterConfig, SidecarLoraConfig, SidecarNamespaceConfig,
};
pub use self::audit::{AuditConfig, AuditLimits, ReplayConfig};
pub use self::policy::{PolicyConfig, PolicyDecision, PolicyLimits, PolicyRuleSpec};
pub use self::observe::{ObserveConfig, ProcIngestConfig, Proc9pConfig};
pub use self::pipeline::{Pipeline, PipelineConfig, PipelineMetrics};
pub use self::telemetry::{
    TelemetryConfig, TelemetryCursorConfig, TelemetryFrameSchema, TelemetryManifestStore,
};

/// Errors surfaced by NineDoor operations.
#[derive(Debug, Error)]
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
    /// Pipeline write failure or retry exhaustion.
    #[error("pipeline error: {0}")]
    Pipeline(#[from] io::Error),
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
    bootstrap_ticket: TicketClaims,
    telemetry_manifest: TelemetryManifestStore,
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
        Self::new_with_shard_layout(ShardLayout::default())
    }

    /// Construct a new NineDoor server with an explicit shard layout.
    #[must_use]
    pub fn new_with_shard_layout(shards: ShardLayout) -> Self {
        Self::new_with_limits_telemetry_host_policy_shards(
            Arc::new(SystemClock),
            SessionLimits::default(),
            TelemetryConfig::default(),
            TelemetryManifestStore::default(),
            CasConfig::disabled(),
            shards,
            HostNamespaceConfig::disabled(),
            SidecarNamespaceConfig::disabled(),
            PolicyConfig::disabled(),
            AuditConfig::disabled(),
        )
    }

    /// Construct a new NineDoor server with host namespace configuration.
    #[must_use]
    pub fn new_with_host_config(host: HostNamespaceConfig) -> Self {
        Self::new_with_limits_telemetry_host_policy_shards(
            Arc::new(SystemClock),
            SessionLimits::default(),
            TelemetryConfig::default(),
            TelemetryManifestStore::default(),
            CasConfig::disabled(),
            ShardLayout::default(),
            host,
            SidecarNamespaceConfig::disabled(),
            PolicyConfig::disabled(),
            AuditConfig::disabled(),
        )
    }

    /// Construct a new NineDoor server with sidecar namespace configuration.
    #[must_use]
    pub fn new_with_sidecar_config(sidecars: SidecarNamespaceConfig) -> Self {
        Self::new_with_limits_telemetry_host_policy_shards(
            Arc::new(SystemClock),
            SessionLimits::default(),
            TelemetryConfig::default(),
            TelemetryManifestStore::default(),
            CasConfig::disabled(),
            ShardLayout::default(),
            HostNamespaceConfig::disabled(),
            sidecars,
            PolicyConfig::disabled(),
            AuditConfig::disabled(),
        )
    }

    /// Construct a new NineDoor server with host and policy configuration.
    #[must_use]
    pub fn new_with_host_and_policy_config(
        host: HostNamespaceConfig,
        policy: PolicyConfig,
    ) -> Self {
        Self::new_with_host_policy_audit_config(host, policy, AuditConfig::disabled())
    }

    /// Construct a new NineDoor server with host, policy, and audit configuration.
    #[must_use]
    pub fn new_with_host_policy_audit_config(
        host: HostNamespaceConfig,
        policy: PolicyConfig,
        audit: AuditConfig,
    ) -> Self {
        Self::new_with_limits_telemetry_host_policy_shards(
            Arc::new(SystemClock),
            SessionLimits::default(),
            TelemetryConfig::default(),
            TelemetryManifestStore::default(),
            CasConfig::disabled(),
            ShardLayout::default(),
            host,
            SidecarNamespaceConfig::disabled(),
            policy,
            audit,
        )
    }

    /// Construct a server using the supplied clock (primarily for tests).
    #[must_use]
    pub fn new_with_clock(clock: Arc<dyn Clock>) -> Self {
        Self::new_with_limits_and_telemetry(
            clock,
            SessionLimits::default(),
            TelemetryConfig::default(),
        )
    }

    /// Construct a server using the supplied clock and session limits.
    #[must_use]
    pub fn new_with_limits(clock: Arc<dyn Clock>, limits: SessionLimits) -> Self {
        Self::new_with_limits_and_telemetry(clock, limits, TelemetryConfig::default())
    }

    /// Construct a server using explicit telemetry configuration.
    #[must_use]
    pub fn new_with_limits_and_telemetry(
        clock: Arc<dyn Clock>,
        limits: SessionLimits,
        telemetry: TelemetryConfig,
    ) -> Self {
        Self::new_with_limits_and_telemetry_manifest(
            clock,
            limits,
            telemetry,
            TelemetryManifestStore::default(),
        )
    }

    /// Construct a server using explicit telemetry configuration and manifest store.
    #[must_use]
    pub fn new_with_limits_and_telemetry_manifest(
        clock: Arc<dyn Clock>,
        limits: SessionLimits,
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
    ) -> Self {
        Self::new_with_limits_telemetry_and_host(
            clock,
            limits,
            telemetry,
            telemetry_manifest,
            HostNamespaceConfig::disabled(),
        )
    }

    /// Construct a new NineDoor server with CAS enabled and default limits.
    #[must_use]
    pub fn new_with_cas_config(cas: CasConfig) -> Self {
        Self::new_with_limits_telemetry_host_policy_shards(
            Arc::new(SystemClock),
            SessionLimits::default(),
            TelemetryConfig::default(),
            TelemetryManifestStore::default(),
            cas,
            ShardLayout::default(),
            HostNamespaceConfig::disabled(),
            SidecarNamespaceConfig::disabled(),
            PolicyConfig::disabled(),
            AuditConfig::disabled(),
        )
    }

    fn new_with_limits_telemetry_and_host(
        clock: Arc<dyn Clock>,
        limits: SessionLimits,
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
        host: HostNamespaceConfig,
    ) -> Self {
        Self::new_with_limits_telemetry_host_policy_shards(
            clock,
            limits,
            telemetry,
            telemetry_manifest,
            CasConfig::disabled(),
            ShardLayout::default(),
            host,
            SidecarNamespaceConfig::disabled(),
            PolicyConfig::disabled(),
            AuditConfig::disabled(),
        )
    }

    fn new_with_limits_telemetry_host_policy_shards(
        clock: Arc<dyn Clock>,
        limits: SessionLimits,
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
        cas: CasConfig,
        shards: ShardLayout,
        host: HostNamespaceConfig,
        sidecars: SidecarNamespaceConfig,
        policy: PolicyConfig,
        audit: AuditConfig,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ServerCore::new(
                clock,
                limits,
                telemetry,
                telemetry_manifest.clone(),
                cas,
                shards,
                host,
                sidecars,
                policy,
                audit,
            ))),
            bootstrap_ticket: TicketClaims::new(
                Role::Queen,
                BudgetSpec::unbounded(),
                None,
                MountSpec::empty(),
                0,
            ),
            telemetry_manifest,
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
    pub fn bootstrap_ticket(&self) -> &TicketClaims {
        &self.bootstrap_ticket
    }

    /// Register a named service namespace that queen sessions may mount.
    pub fn register_service(&self, service: &str, target: &[&str]) -> Result<(), NineDoorError> {
        let mut core = self.inner.lock().expect("poisoned nine-door lock");
        core.register_service(service, target)
    }

    /// Register a shared secret used to validate attach tickets for the role.
    pub fn register_ticket_secret(&self, role: Role, secret: &str) {
        let mut core = self.inner.lock().expect("poisoned nine-door lock");
        core.register_ticket_secret(role, secret);
    }

    /// Install GPU namespace nodes discovered by the host bridge.
    pub fn install_gpu_nodes(&self, topology: &GpuNamespaceSnapshot) -> Result<(), NineDoorError> {
        let mut core = self.inner.lock().expect("poisoned nine-door lock");
        core.install_gpu_nodes(topology)
    }

    /// Fetch current Secure9P pipeline metrics.
    #[must_use]
    pub fn pipeline_metrics(&self) -> PipelineMetrics {
        let core = self.inner.lock().expect("poisoned nine-door lock");
        core.pipeline_metrics()
    }

    /// Retrieve the telemetry manifest store for reboot resumption.
    #[must_use]
    pub fn telemetry_manifest(&self) -> TelemetryManifestStore {
        self.telemetry_manifest.clone()
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
            codec: Codec,
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
        let response_bytes = self.exchange_batch(&encoded)?;
        let response = self.codec.decode_response(&response_bytes)?;
        debug_assert_eq!(response.tag, tag);
        match response.body {
            ResponseBody::Error { code, message } => Err(NineDoorError::Protocol { code, message }),
            other => Ok(other),
        }
    }

    /// Exchange a raw batch of Secure9P frames with the server.
    pub fn exchange_batch(&mut self, batch: &[u8]) -> Result<Vec<u8>, NineDoorError> {
        let mut core = self.server.lock().expect("poisoned nine-door lock");
        core.handle_batch(self.session, batch)
    }

    /// Send a batch of request bodies and return decoded responses.
    pub fn transact_batch(
        &mut self,
        bodies: &[RequestBody],
    ) -> Result<Vec<ResponseBody>, NineDoorError> {
        let mut batch = Vec::new();
        for body in bodies {
            let tag = self.next_tag();
            let request = Request {
                tag,
                body: body.clone(),
            };
            let frame = self.codec.encode_request(&request)?;
            batch.extend_from_slice(&frame);
        }
        let response_bytes = self.exchange_batch(&batch)?;
        let mut responses = Vec::new();
        for frame in secure9p_codec::BatchIter::new(&response_bytes) {
            let frame = frame?;
            let response = self.codec.decode_response(frame.bytes())?;
            responses.push(response.body);
        }
        Ok(responses)
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
        self.attach_with_identity(fid, role, None, None)
    }

    /// Attach to the namespace providing an explicit identity string.
    pub fn attach_with_identity(
        &mut self,
        fid: u32,
        role: Role,
        identity: Option<&str>,
        ticket: Option<&str>,
    ) -> Result<Qid, NineDoorError> {
        let response = self.transact(RequestBody::Attach {
            fid,
            afid: u32::MAX,
            uname: role_to_uname(role, identity)?,
            aname: ticket.unwrap_or("").to_owned(),
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
