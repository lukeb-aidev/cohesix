// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Implement the bounded isolated TCP console and network service state machine.
// Author: Lukas Bower

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Restricted console-network service implementation.

pub use console_network_abi as abi;

use abi::{
    CommandBatchBuilder, ExchangeKind, RuntimeInitDescriptor, SendBatchCursor,
    COMMAND_BATCH_MAX_RECORDS, COMMAND_LINE_BYTES, CONSOLE_OUTPUT_BYTES, CONSOLE_PAYLOAD_BYTES,
};
use heapless::{Deque, Vec as HeaplessVec};
use smoltcp::iface::{
    Config as InterfaceConfig, Interface, SocketHandle, SocketSet, SocketStorage,
};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp::{
    Socket as TcpSocket, SocketBuffer as TcpSocketBuffer, State as TcpState,
};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, IpListenEndpoint, Ipv4Address};

const AUTH_PREFIX: &[u8] = b"AUTH ";
const SESSION_EVENT_DEPTH: usize = 8;
const SESSION_OUTPUT_DEPTH: usize = 8;
const FRAME_PREFIX_BYTES: usize = 4;
const SESSION_INGRESS_BYTES: usize = CONSOLE_PAYLOAD_BYTES + FRAME_PREFIX_BYTES;
// One maximum receive can complete one partial command-bound oversize frame,
// contain one complete minimum-size oversize frame, and finish with one
// payload-oversize prefix. No fourth invalid-length response can fit.
const INVALID_LENGTH_OUTPUT_RESERVE: usize = 3;
const INVALID_LENGTH_FRAME: &[u8] = b"ERR FRAME reason=invalid-length";

/// Decide whether a direct NIC service loop may poll locally without a new
/// notification. A retained egress frame alone cannot justify polling after
/// the NIC reported ring backpressure; only peer rearm or independently
/// durable local work may resume it.
#[must_use]
pub const fn direct_service_repoll_required(
    quantum_exhausted: bool,
    completion_pending: bool,
    event_pending: bool,
    egress_pending: bool,
    tx_waiting_for_peer: bool,
    link_work_pending: bool,
) -> bool {
    quantum_exhausted
        || completion_pending
        || event_pending
        || link_work_pending
        || (egress_pending && !tx_waiting_for_peer)
}

/// Runtime construction or bounded service error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    /// Runtime-init validation failed.
    InvalidInit,
    /// The selected static network address is invalid.
    InvalidNetwork,
    /// The sole TCP listener could not bind.
    ListenerBind,
    /// A packet page was empty or exceeded the Ethernet bound.
    PacketBound,
    /// A console frame was malformed or exceeded the payload bound.
    ConsoleFrame,
    /// The bounded service queue was full.
    Backpressure,
    /// Root attempted output before transport authentication.
    Unauthenticated,
    /// The generation has been shut down or revoked.
    Terminal,
}

/// Result of applying one well-formed root control to its exact connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlApplyOutcome {
    /// The control belongs to the active connection and was applied.
    Applied,
    /// The control names an ended or different connection and had no effect.
    StaleConnection,
}

enum ValidatedRootControl<'a> {
    SendLine(&'a str),
    SendBatch(SendBatchCursor),
    Disconnect,
}

/// One closed material unit admitted for an isolated child scheduling iteration.
///
/// The target loop selects exactly one value after each notification gate and
/// returns to that gate after executing it. Publication units are selected
/// before retained service work, which in turn precedes newly signalled input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildTurnUnit {
    /// Publish and signal one retained packet/control completion.
    PublishCompletion,
    /// Publish and signal one retained service event.
    PublishServiceEvent,
    /// Publish and signal one retained Ethernet egress frame.
    PublishEgress,
    /// Advance one retained stack-ingress, stack-egress, or session subunit.
    PollService,
    /// Copy and ingest one newly committed Ethernet ingress frame.
    IngestPacket,
    /// Read and apply one newly committed root control record.
    ApplyControl,
    /// The notification carried no new or retained material work.
    Idle,
}

impl ChildTurnUnit {
    /// Whether this unit writes and signals one child-owned one-slot page.
    #[must_use]
    pub const fn is_publication(self) -> bool {
        matches!(
            self,
            Self::PublishCompletion | Self::PublishServiceEvent | Self::PublishEgress
        )
    }
}

/// Result of one internally bounded service-poll unit.
///
/// `Continuation` keeps the outer [`ChildTurnUnit::PollService`] retained for
/// the next scheduler iteration. After Session consumes one bounded ingress
/// chunk or commits one complete wire frame, that retention starts exactly one
/// fresh StackIngress/StackEgress/Session cycle. An empty, no-commit Session
/// returns `Complete` and permits the child scheduler to clear that retained
/// work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServicePollOutcome {
    /// A stack unit completed, or Session made bounded ingress/output progress.
    Continuation,
    /// Session completed without ingress or output progress in this unit.
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServicePollUnit {
    StackIngress,
    StackEgress,
    Session,
}

impl ServicePollUnit {
    const fn successor(self) -> Self {
        match self {
            Self::StackIngress => Self::StackEgress,
            Self::StackEgress => Self::Session,
            Self::Session => Self::StackIngress,
        }
    }
}

/// Read-only retained publication state used by the child-turn chooser.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChildTurnReadiness {
    completion: bool,
    service_event: bool,
    egress: bool,
}

impl ChildTurnReadiness {
    /// Describe the three child-owned publication queues without consuming them.
    #[must_use]
    pub const fn new(completion: bool, service_event: bool, egress: bool) -> Self {
        Self {
            completion,
            service_event,
            egress,
        }
    }
}

/// Retained chooser for one-material-unit child scheduling iterations.
///
/// Notification badges are coalescing prompts. Durable page sequences and
/// these retained booleans preserve work until a later iteration selects it.
/// Every iteration re-enters the notification gate before selecting at most
/// one unit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChildTurnScheduler {
    ingress_pending: bool,
    control_pending: bool,
    service_pending: bool,
}

impl ChildTurnScheduler {
    /// Construct an empty retained child scheduler.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ingress_pending: false,
            control_pending: false,
            service_pending: false,
        }
    }

    /// Retain ordinary work represented by one coalesced root notification.
    ///
    /// The control badge represents either a durable record or the existing
    /// root service tick. The selected control-read unit distinguishes them;
    /// a stable empty read retires that hint and requests a separate retained
    /// service-poll cycle.
    pub fn retain_notification(&mut self, packet_wake: bool, control_wake: bool) {
        if packet_wake {
            self.ingress_pending = true;
        }
        if control_wake {
            self.control_pending = true;
        }
    }

    /// Retain one bounded service-poll cycle after packet or control work.
    pub fn request_service(&mut self) {
        self.service_pending = true;
    }

    /// Select and claim at most one material unit for the current turn.
    ///
    /// Publications stay ahead of service work so shared producer slots are
    /// drained before another poll can create more output. Service work stays
    /// ahead of new input so a one-frame device cannot be overwritten.
    #[must_use]
    pub const fn take_next(&self, readiness: ChildTurnReadiness) -> ChildTurnUnit {
        if readiness.completion {
            return ChildTurnUnit::PublishCompletion;
        }
        if readiness.service_event {
            return ChildTurnUnit::PublishServiceEvent;
        }
        if readiness.egress {
            return ChildTurnUnit::PublishEgress;
        }
        if self.service_pending {
            return ChildTurnUnit::PollService;
        }
        if self.ingress_pending {
            return ChildTurnUnit::IngestPacket;
        }
        if self.control_pending {
            return ChildTurnUnit::ApplyControl;
        }
        ChildTurnUnit::Idle
    }

    /// Commit a successfully executed retained non-publication unit.
    ///
    /// A newly committed page is completed only after application succeeds. A
    /// stable page with no sequence newer than the last accepted record is an
    /// empty coalesced hint, so the target may also call this before retaining
    /// its separate service cycle; a later real record has its own signal.
    pub fn complete(&mut self, unit: ChildTurnUnit) {
        match unit {
            ChildTurnUnit::PollService => self.service_pending = false,
            ChildTurnUnit::IngestPacket => self.ingress_pending = false,
            ChildTurnUnit::ApplyControl => self.control_pending = false,
            ChildTurnUnit::PublishCompletion
            | ChildTurnUnit::PublishServiceEvent
            | ChildTurnUnit::PublishEgress
            | ChildTurnUnit::Idle => {}
        }
    }

    /// Whether any non-publication work remains retained for a later iteration.
    #[must_use]
    pub const fn retained_work_pending(&self) -> bool {
        self.ingress_pending || self.control_pending || self.service_pending
    }

    /// Whether the next iteration may progress without blocking.
    ///
    /// Internal retained work may Poll within the active MCS budget. A
    /// completion, service-event, or egress publication may Poll only with the
    /// explicit credit granted after root accepted the preceding one-slot
    /// record. Idle always blocks. Ordinary packet/control wakes and blocking
    /// Wait returns never mint publication credit.
    #[must_use]
    pub const fn local_poll_eligible(
        &self,
        publication_credit_available: bool,
        readiness: ChildTurnReadiness,
    ) -> bool {
        match self.take_next(readiness) {
            ChildTurnUnit::PublishCompletion
            | ChildTurnUnit::PublishServiceEvent
            | ChildTurnUnit::PublishEgress => publication_credit_available,
            ChildTurnUnit::PollService
            | ChildTurnUnit::IngestPacket
            | ChildTurnUnit::ApplyControl => true,
            ChildTurnUnit::Idle => false,
        }
    }
}

/// One typed event delivered to root policy over the event page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceEvent {
    kind: ExchangeKind,
    connection_id: u64,
    now_ms: u64,
    payload: HeaplessVec<u8, CONSOLE_PAYLOAD_BYTES>,
}

impl ServiceEvent {
    /// Event kind.
    #[must_use]
    pub const fn kind(&self) -> ExchangeKind {
        self.kind
    }

    /// Exact child-created connection identity.
    #[must_use]
    pub const fn connection_id(&self) -> u64 {
        self.connection_id
    }

    /// Root-supplied monotonic observation time.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Exact initialized payload bytes.
    #[must_use]
    pub fn payload_bytes(&self) -> &[u8] {
        self.payload.as_slice()
    }

    /// Validated UTF-8 payload when the event kind carries plain text.
    pub fn payload(&self) -> Result<&str, RuntimeError> {
        core::str::from_utf8(self.payload.as_slice()).map_err(|_| RuntimeError::ConsoleFrame)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthState {
    Inactive,
    Waiting,
    Authenticated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingOutputBatch {
    payload: HeaplessVec<u8, CONSOLE_PAYLOAD_BYTES>,
    cursor: SendBatchCursor,
}

/// Bounded transport-authentication and length-prefixed console session.
///
/// This state machine has no command parser or policy authority. It releases a
/// command event only after exact transport authentication; root then performs
/// ticket, role, namespace, and verb authorization.
pub struct TransportSession {
    auth_token: HeaplessVec<u8, { abi::AUTH_TOKEN_BYTES }>,
    auth_timeout_ms: u64,
    idle_timeout_ms: u64,
    state: AuthState,
    connection_id: u64,
    last_activity_ms: u64,
    auth_deadline_ms: u64,
    length_prefix: [u8; FRAME_PREFIX_BYTES],
    length_pos: usize,
    payload_len: Option<usize>,
    drop_remaining: usize,
    payload: HeaplessVec<u8, CONSOLE_PAYLOAD_BYTES>,
    events: Deque<ServiceEvent, SESSION_EVENT_DEPTH>,
    outbound: Deque<HeaplessVec<u8, CONSOLE_OUTPUT_BYTES>, SESSION_OUTPUT_DEPTH>,
    pending_batch: Option<PendingOutputBatch>,
    close_after_flush: bool,
    terminal: bool,
}

impl TransportSession {
    /// Construct a fail-closed session from a validated descriptor.
    pub fn new(descriptor: RuntimeInitDescriptor) -> Result<Self, RuntimeError> {
        descriptor
            .validate()
            .map_err(|_| RuntimeError::InvalidInit)?;
        let mut auth_token = HeaplessVec::new();
        auth_token
            .extend_from_slice(descriptor.auth_token())
            .map_err(|_| RuntimeError::InvalidInit)?;
        Ok(Self {
            auth_token,
            auth_timeout_ms: descriptor.auth_timeout_ms as u64,
            idle_timeout_ms: descriptor.idle_timeout_ms as u64,
            state: AuthState::Inactive,
            connection_id: 0,
            last_activity_ms: 0,
            auth_deadline_ms: 0,
            length_prefix: [0; FRAME_PREFIX_BYTES],
            length_pos: 0,
            payload_len: None,
            drop_remaining: 0,
            payload: HeaplessVec::new(),
            events: Deque::new(),
            outbound: Deque::new(),
            pending_batch: None,
            close_after_flush: false,
            terminal: false,
        })
    }

    /// Start one child-owned connection generation.
    pub fn begin(&mut self, connection_id: u64, now_ms: u64) -> Result<(), RuntimeError> {
        if self.terminal || connection_id == 0 || self.state != AuthState::Inactive {
            return Err(RuntimeError::Terminal);
        }
        self.connection_id = connection_id;
        self.state = AuthState::Waiting;
        self.last_activity_ms = now_ms;
        self.auth_deadline_ms = now_ms.saturating_add(self.auth_timeout_ms);
        self.reset_frame();
        self.outbound.clear();
        self.pending_batch = None;
        self.close_after_flush = false;
        self.push_event(ExchangeKind::Connected, now_ms, &[])
    }

    /// Maximum bytes the next receive may admit without crossing an RX proof
    /// boundary or exhausting the bounded protocol-error output reserve.
    #[must_use]
    pub fn ingress_capacity(&self) -> usize {
        if self.terminal || self.state == AuthState::Inactive {
            return 0;
        }
        if self.drop_remaining != 0 {
            return self.drop_remaining.min(SESSION_INGRESS_BYTES);
        }
        if self.pending_batch.is_some() {
            return 0;
        }
        let available_outputs = SESSION_OUTPUT_DEPTH.saturating_sub(self.outbound.len());
        match self.state {
            AuthState::Waiting | AuthState::Authenticated
                if available_outputs >= INVALID_LENGTH_OUTPUT_RESERVE =>
            {
                SESSION_INGRESS_BYTES
            }
            AuthState::Inactive | AuthState::Waiting | AuthState::Authenticated => 0,
        }
    }

    /// Consume a bounded TCP byte chunk.
    pub fn ingest(&mut self, bytes: &[u8], now_ms: u64) -> Result<(), RuntimeError> {
        if self.terminal || self.state == AuthState::Inactive {
            return Err(RuntimeError::Terminal);
        }
        if bytes.len() > self.ingress_capacity() {
            return Err(RuntimeError::Backpressure);
        }
        for byte in bytes {
            if self.drop_remaining != 0 {
                self.drop_remaining = self.drop_remaining.saturating_sub(1);
                if self.drop_remaining == 0 {
                    self.last_activity_ms = now_ms;
                    self.reset_frame();
                }
                continue;
            }
            if let Some(expected) = self.payload_len {
                self.payload
                    .push(*byte)
                    .map_err(|_| RuntimeError::ConsoleFrame)?;
                if self.payload.len() == expected {
                    self.last_activity_ms = now_ms;
                    self.finish_frame(now_ms)?;
                    self.reset_frame();
                }
                continue;
            }
            self.length_prefix[self.length_pos] = *byte;
            self.length_pos += 1;
            if self.length_pos == FRAME_PREFIX_BYTES {
                self.length_pos = 0;
                let declared = u32::from_le_bytes(self.length_prefix) as usize;
                if declared < FRAME_PREFIX_BYTES {
                    self.reject(now_ms, b"reason=invalid-length")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
                let payload_len = declared - FRAME_PREFIX_BYTES;
                if payload_len > CONSOLE_PAYLOAD_BYTES {
                    if self.state == AuthState::Authenticated {
                        self.queue_wire_payload(INVALID_LENGTH_FRAME)?;
                        self.begin_frame_drop(payload_len);
                        continue;
                    }
                    self.reject(now_ms, b"reason=invalid-length")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
                self.payload.clear();
                self.payload_len = Some(payload_len);
                if payload_len == 0 {
                    self.reject(now_ms, b"reason=invalid-length")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
            }
        }
        Ok(())
    }

    /// Queue one root-authorized line without interpreting its ACK/ERR/END body.
    pub fn queue_authorized_line(&mut self, line: &str) -> Result<(), RuntimeError> {
        if self.terminal || self.state != AuthState::Authenticated {
            return Err(RuntimeError::Unauthenticated);
        }
        if self.pending_batch.is_some() {
            return Err(RuntimeError::Backpressure);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() || trimmed.len() > CONSOLE_OUTPUT_BYTES {
            return Err(RuntimeError::ConsoleFrame);
        }
        let mut frame = HeaplessVec::new();
        frame
            .extend_from_slice(trimmed.as_bytes())
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.outbound
            .push_back(frame)
            .map_err(|_| RuntimeError::Backpressure)
    }

    fn queue_authorized_batch(
        &mut self,
        payload: &[u8],
        cursor: SendBatchCursor,
    ) -> Result<(), RuntimeError> {
        if self.terminal || self.state != AuthState::Authenticated {
            return Err(RuntimeError::Unauthenticated);
        }
        if self.pending_batch.is_some() || !self.outbound.is_empty() || cursor.is_empty() {
            return Err(RuntimeError::Backpressure);
        }
        let mut private_payload = HeaplessVec::new();
        private_payload
            .extend_from_slice(payload)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.pending_batch = Some(PendingOutputBatch {
            payload: private_payload,
            cursor,
        });
        Ok(())
    }

    fn stage_next_batch_line(&mut self) -> Result<bool, RuntimeError> {
        if self.pending_batch.is_none() || !self.outbound.is_empty() {
            return Ok(false);
        }
        let (frame, next_cursor, finished) = {
            let batch = self
                .pending_batch
                .as_ref()
                .ok_or(RuntimeError::ConsoleFrame)?;
            let mut next_cursor = batch.cursor;
            let line = next_cursor
                .next_line(batch.payload.as_slice())
                .map_err(|_| RuntimeError::ConsoleFrame)?
                .ok_or(RuntimeError::ConsoleFrame)?;
            let mut frame: HeaplessVec<u8, CONSOLE_OUTPUT_BYTES> = HeaplessVec::new();
            frame
                .extend_from_slice(line.as_bytes())
                .map_err(|_| RuntimeError::ConsoleFrame)?;
            (frame, next_cursor, next_cursor.is_empty())
        };
        self.outbound
            .push_back(frame)
            .map_err(|_| RuntimeError::Backpressure)?;
        if finished {
            self.pending_batch = None;
        } else if let Some(batch) = self.pending_batch.as_mut() {
            batch.cursor = next_cursor;
        } else {
            return Err(RuntimeError::ConsoleFrame);
        }
        Ok(true)
    }

    /// Copy one length-prefixed outgoing frame into `output`.
    pub fn pop_wire_output(&mut self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        let length = self.peek_wire_output(output)?;
        if length.is_some() {
            self.commit_wire_output()?;
        }
        Ok(length)
    }

    /// Copy, but do not consume, one length-prefixed outgoing frame.
    pub fn peek_wire_output(&self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        let Some(frame) = self.outbound.front() else {
            return Ok(None);
        };
        let total = FRAME_PREFIX_BYTES.saturating_add(frame.len());
        if output.len() < total {
            return Err(RuntimeError::Backpressure);
        }
        output[..FRAME_PREFIX_BYTES].copy_from_slice(&(total as u32).to_le_bytes());
        output[FRAME_PREFIX_BYTES..total].copy_from_slice(frame.as_slice());
        Ok(Some(total))
    }

    /// Commit the frame most recently copied with [`Self::peek_wire_output`].
    pub fn commit_wire_output(&mut self) -> Result<(), RuntimeError> {
        self.outbound
            .pop_front()
            .map(|_| ())
            .ok_or(RuntimeError::Backpressure)
    }

    /// Pop one typed event for root policy.
    pub fn pop_event(&mut self) -> Option<ServiceEvent> {
        self.events.pop_front()
    }

    /// Pop one lifecycle event or coalesce a bounded consecutive command run.
    ///
    /// Lifecycle and connection-identity changes fence batching. The returned
    /// command batch retains each command's exact observation timestamp and
    /// consumes only commands that fit the fixed exchange-page payload.
    pub fn pop_publication_event(
        &mut self,
        max_commands_per_wake: u16,
    ) -> Result<Option<ServiceEvent>, RuntimeError> {
        let Some(first) = self.events.pop_front() else {
            return Ok(None);
        };
        let limit = usize::from(max_commands_per_wake).min(COMMAND_BATCH_MAX_RECORDS);
        if first.kind != ExchangeKind::Command || limit <= 1 {
            return Ok(Some(first));
        }

        let mut storage = [0u8; CONSOLE_PAYLOAD_BYTES];
        let mut builder = CommandBatchBuilder::new(&mut storage);
        let first_command = first.payload()?;
        if !builder
            .try_push_command(first.now_ms, first_command)
            .map_err(|_| RuntimeError::ConsoleFrame)?
        {
            return Err(RuntimeError::ConsoleFrame);
        }

        while builder.record_count() < limit {
            let Some(next) = self.events.front() else {
                break;
            };
            if next.kind != ExchangeKind::Command || next.connection_id != first.connection_id {
                break;
            }
            let command = next.payload()?;
            if !builder
                .try_push_command(next.now_ms, command)
                .map_err(|_| RuntimeError::ConsoleFrame)?
            {
                break;
            }
            if self.events.pop_front().is_none() {
                return Err(RuntimeError::ConsoleFrame);
            }
        }

        if builder.record_count() == 1 {
            return Ok(Some(first));
        }

        let payload_len = builder
            .finish()
            .map_err(|_| RuntimeError::ConsoleFrame)?
            .len();
        let mut payload = HeaplessVec::new();
        payload
            .extend_from_slice(&storage[..payload_len])
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        Ok(Some(ServiceEvent {
            kind: ExchangeKind::CommandBatch,
            connection_id: first.connection_id,
            now_ms: first.now_ms,
            payload,
        }))
    }

    /// Request a graceful close after all queued wire output is copied.
    pub fn request_disconnect(&mut self) {
        self.close_after_flush = true;
    }

    /// Enforce authentication and idle deadlines.
    pub fn tick(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        if self.close_after_flush {
            return Ok(());
        }
        let timed_out = match self.state {
            AuthState::Waiting => now_ms >= self.auth_deadline_ms,
            AuthState::Authenticated => {
                now_ms.saturating_sub(self.last_activity_ms) >= self.idle_timeout_ms
            }
            AuthState::Inactive => false,
        };
        if timed_out {
            self.reject(now_ms, b"reason=timeout")?;
            self.close_after_flush = true;
        }
        Ok(())
    }

    /// Whether the socket should begin its TCP close handshake.
    #[must_use]
    pub fn close_ready(&self) -> bool {
        self.close_after_flush && self.output_queue_empty()
    }

    /// End the current TCP connection and publish the exact terminal event.
    pub fn end(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        if self.state != AuthState::Inactive {
            self.push_event(ExchangeKind::Disconnected, now_ms, b"reason=closed")?;
        }
        self.state = AuthState::Inactive;
        self.connection_id = 0;
        self.reset_frame();
        self.outbound.clear();
        self.pending_batch = None;
        self.close_after_flush = false;
        Ok(())
    }

    /// Revoke the complete generation. This state is terminal.
    pub fn revoke(&mut self) {
        self.state = AuthState::Inactive;
        self.connection_id = 0;
        self.reset_frame();
        self.events.clear();
        self.outbound.clear();
        self.pending_batch = None;
        self.close_after_flush = false;
        self.terminal = true;
    }

    /// Whether transport authentication has completed.
    #[must_use]
    pub const fn authenticated(&self) -> bool {
        matches!(self.state, AuthState::Authenticated)
    }

    /// Current child-created connection identity, when a session is live.
    #[must_use]
    pub const fn connection_id(&self) -> Option<u64> {
        if matches!(self.state, AuthState::Inactive) {
            None
        } else {
            Some(self.connection_id)
        }
    }

    /// Whether every root-authorized frame has entered the TCP send queue.
    #[must_use]
    pub fn output_queue_empty(&self) -> bool {
        self.outbound.is_empty() && self.pending_batch.is_none()
    }

    fn finish_frame(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        match self.state {
            AuthState::Waiting => self.authenticate(now_ms),
            AuthState::Authenticated => {
                if self.payload.len() > COMMAND_LINE_BYTES {
                    self.queue_wire_payload(INVALID_LENGTH_FRAME)?;
                    return Ok(());
                }
                if core::str::from_utf8(self.payload.as_slice()).is_err() {
                    self.reject(now_ms, b"reason=invalid-utf8")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
                let mut payload = HeaplessVec::new();
                payload
                    .extend_from_slice(self.payload.as_slice())
                    .map_err(|_| RuntimeError::ConsoleFrame)?;
                self.events
                    .push_back(ServiceEvent {
                        kind: ExchangeKind::Command,
                        connection_id: self.connection_id,
                        now_ms,
                        payload,
                    })
                    .map_err(|_| {
                        self.close_after_flush = true;
                        RuntimeError::Backpressure
                    })
            }
            AuthState::Inactive => Err(RuntimeError::Terminal),
        }
    }

    fn authenticate(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        let payload = self.payload.as_slice();
        let candidate = match payload.strip_prefix(AUTH_PREFIX) {
            Some(candidate) => candidate,
            None => &[],
        };
        let valid = constant_time_equal(candidate, self.auth_token.as_slice());
        if !valid {
            self.reject(now_ms, b"reason=invalid-token")?;
            return Ok(());
        }
        self.state = AuthState::Authenticated;
        self.auth_deadline_ms = 0;
        self.queue_wire_payload(b"OK AUTH")?;
        self.push_event(ExchangeKind::Authenticated, now_ms, &[])
    }

    fn reject(&mut self, now_ms: u64, reason: &[u8]) -> Result<(), RuntimeError> {
        let mut line: HeaplessVec<u8, CONSOLE_OUTPUT_BYTES> = HeaplessVec::new();
        line.extend_from_slice(b"ERR AUTH ")
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        line.extend_from_slice(reason)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.outbound
            .push_back(line)
            .map_err(|_| RuntimeError::Backpressure)?;
        self.push_event(ExchangeKind::Rejected, now_ms, reason)?;
        self.close_after_flush = true;
        Ok(())
    }

    fn queue_wire_payload(&mut self, bytes: &[u8]) -> Result<(), RuntimeError> {
        let mut frame = HeaplessVec::new();
        frame
            .extend_from_slice(bytes)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.outbound
            .push_back(frame)
            .map_err(|_| RuntimeError::Backpressure)
    }

    fn push_event(
        &mut self,
        kind: ExchangeKind,
        now_ms: u64,
        payload: &[u8],
    ) -> Result<(), RuntimeError> {
        let mut bounded = HeaplessVec::new();
        bounded
            .extend_from_slice(payload)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.events
            .push_back(ServiceEvent {
                kind,
                connection_id: self.connection_id,
                now_ms,
                payload: bounded,
            })
            .map_err(|_| RuntimeError::Backpressure)
    }

    fn reset_frame(&mut self) {
        self.length_prefix = [0; FRAME_PREFIX_BYTES];
        self.length_pos = 0;
        self.payload_len = None;
        self.drop_remaining = 0;
        self.payload.clear();
    }

    fn begin_frame_drop(&mut self, payload_len: usize) {
        self.length_prefix = [0; FRAME_PREFIX_BYTES];
        self.length_pos = 0;
        self.payload_len = None;
        self.drop_remaining = payload_len;
        self.payload.clear();
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let mut index = 0usize;
    while index < abi::AUTH_TOKEN_BYTES {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(left_byte ^ right_byte);
        index += 1;
    }
    difference == 0
}

#[derive(Clone, Copy)]
struct EthernetFrame {
    bytes: [u8; abi::ETHERNET_FRAME_BYTES],
    len: usize,
}

impl EthernetFrame {
    const fn empty() -> Self {
        Self {
            bytes: [0; abi::ETHERNET_FRAME_BYTES],
            len: 0,
        }
    }
}

/// Bounded one-packet ingress/egress device used by smoltcp inside the child.
pub struct SharedFrameDevice {
    ingress: Option<EthernetFrame>,
    egress: Option<EthernetFrame>,
    tx_overflow: u64,
}

impl SharedFrameDevice {
    /// Empty service device.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ingress: None,
            egress: None,
            tx_overflow: 0,
        }
    }

    /// Admit one copied Ethernet frame; backpressure preserves the current one.
    pub fn push_ingress(&mut self, packet: &[u8]) -> Result<(), RuntimeError> {
        if packet.is_empty() || packet.len() > abi::ETHERNET_FRAME_BYTES {
            return Err(RuntimeError::PacketBound);
        }
        if self.ingress.is_some() {
            return Err(RuntimeError::Backpressure);
        }
        let mut frame = EthernetFrame::empty();
        frame.bytes[..packet.len()].copy_from_slice(packet);
        frame.len = packet.len();
        self.ingress = Some(frame);
        Ok(())
    }

    /// Copy one smoltcp-produced Ethernet frame to root's TX page.
    pub fn pop_egress(&mut self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        let Some(frame) = self.egress else {
            return Ok(None);
        };
        if output.len() < frame.len {
            return Err(RuntimeError::PacketBound);
        }
        output[..frame.len].copy_from_slice(&frame.bytes[..frame.len]);
        self.egress = None;
        Ok(Some(frame.len))
    }

    /// Total oversized transmit requests rejected inside the child.
    #[must_use]
    pub const fn tx_overflow(&self) -> u64 {
        self.tx_overflow
    }
}

impl Default for SharedFrameDevice {
    fn default() -> Self {
        Self::new()
    }
}

/// Owned ingress token for smoltcp.
pub struct SharedRxToken(EthernetFrame);

impl RxToken for SharedRxToken {
    fn consume<R, F>(self, operation: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        operation(&self.0.bytes[..self.0.len])
    }
}

/// Borrowed egress token for smoltcp.
pub struct SharedTxToken<'a>(&'a mut SharedFrameDevice);

impl TxToken for SharedTxToken<'_> {
    fn consume<R, F>(self, len: usize, operation: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        if len > abi::ETHERNET_FRAME_BYTES {
            self.0.tx_overflow = self.0.tx_overflow.saturating_add(1);
            return operation(&mut []);
        }
        let mut frame = EthernetFrame::empty();
        frame.len = len;
        let result = operation(&mut frame.bytes[..len]);
        self.0.egress = Some(frame);
        result
    }
}

impl Device for SharedFrameDevice {
    type RxToken<'a>
        = SharedRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = SharedTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.egress.is_some() {
            return None;
        }
        let ingress = self.ingress.take()?;
        Some((SharedRxToken(ingress), SharedTxToken(self)))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        self.egress.is_none().then_some(SharedTxToken(self))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = 1500;
        capabilities
    }
}

/// Operational single-listener smoltcp service owned by the isolated child.
pub struct ConsoleNetworkService<'a> {
    interface: Interface,
    device: SharedFrameDevice,
    sockets: SocketSet<'a>,
    tcp_handle: SocketHandle,
    listener_port: u16,
    session: TransportSession,
    next_connection_id: u64,
    last_tcp_state: TcpState,
    poll_unit: ServicePollUnit,
    terminal: bool,
}

impl<'a> ConsoleNetworkService<'a> {
    /// Construct the sole static-IPv4 QEMU listener from sealed runtime state.
    pub fn new(
        descriptor: RuntimeInitDescriptor,
        tcp_rx: &'a mut [u8],
        tcp_tx: &'a mut [u8],
        socket_storage: &'a mut [SocketStorage<'a>],
    ) -> Result<Self, RuntimeError> {
        Self::new_with_mac(descriptor, descriptor.mac, tcp_rx, tcp_tx, socket_storage)
    }

    /// Construct the listener with a device-proven MAC address.
    ///
    /// Direct QEMU VirtIO ownership reads the MAC from the admitted device and
    /// verifies it against the sealed descriptor before entering this path.
    pub fn new_with_mac(
        descriptor: RuntimeInitDescriptor,
        mac: [u8; 6],
        tcp_rx: &'a mut [u8],
        tcp_tx: &'a mut [u8],
        socket_storage: &'a mut [SocketStorage<'a>],
    ) -> Result<Self, RuntimeError> {
        descriptor
            .validate()
            .map_err(|_| RuntimeError::InvalidInit)?;
        if socket_storage.len() != 1 || tcp_rx.len() < 1024 || tcp_tx.len() < 1024 {
            return Err(RuntimeError::InvalidInit);
        }
        let address = Ipv4Address::from(descriptor.ipv4);
        if address.is_unspecified() || descriptor.prefix_len == 0 {
            return Err(RuntimeError::InvalidNetwork);
        }
        let mut device = SharedFrameDevice::new();
        let mut interface_config =
            InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
        interface_config.random_seed = descriptor.generation;
        let mut interface = Interface::new(interface_config, &mut device, Instant::from_millis(0));
        let mut address_installed = false;
        interface.update_ip_addrs(|addresses| {
            addresses.clear();
            address_installed = addresses
                .push(IpCidr::new(address.into(), descriptor.prefix_len))
                .is_ok();
        });
        if !address_installed {
            return Err(RuntimeError::InvalidNetwork);
        }
        if descriptor.gateway != [0; 4] {
            interface
                .routes_mut()
                .add_default_ipv4_route(Ipv4Address::from(descriptor.gateway))
                .map_err(|_| RuntimeError::InvalidNetwork)?;
        }
        let mut sockets = SocketSet::new(socket_storage);
        let mut tcp = TcpSocket::new(TcpSocketBuffer::new(tcp_rx), TcpSocketBuffer::new(tcp_tx));
        tcp.listen(IpListenEndpoint::from(descriptor.listener_port))
            .map_err(|_| RuntimeError::ListenerBind)?;
        let tcp_handle = sockets.add(tcp);
        Ok(Self {
            interface,
            device,
            sockets,
            tcp_handle,
            listener_port: descriptor.listener_port,
            session: TransportSession::new(descriptor)?,
            next_connection_id: 1,
            last_tcp_state: TcpState::Listen,
            poll_unit: ServicePollUnit::StackIngress,
            terminal: false,
        })
    }

    /// Copy one driver-delivered frame into the child-owned smoltcp ingress.
    pub fn ingest_packet(&mut self, packet: &[u8]) -> Result<(), RuntimeError> {
        if self.terminal {
            return Err(RuntimeError::Terminal);
        }
        self.device.push_ingress(packet)
    }

    /// Whether the bounded internal device can accept one complete frame.
    #[must_use]
    pub const fn ingress_available(&self) -> bool {
        self.device.ingress.is_none()
    }

    /// Copy one smoltcp egress frame for the admitted NIC transport.
    pub fn take_packet(&mut self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        self.device.pop_egress(output)
    }

    /// Whether one child-produced Ethernet frame is retained for publication.
    #[must_use]
    pub const fn egress_pending(&self) -> bool {
        self.device.egress.is_some()
    }

    /// Apply one root-authorized control to its exact child-owned connection.
    pub fn apply_control(
        &mut self,
        connection_id: u64,
        kind: ExchangeKind,
        payload: &[u8],
    ) -> Result<ControlApplyOutcome, RuntimeError> {
        if self.terminal {
            return Err(RuntimeError::Terminal);
        }

        // Validate the complete control shape before classifying its identity.
        // A stale connection is a consumed no-op, not a way to admit malformed
        // root input. The exact-current path retains the existing session
        // authentication and queue-pressure errors below.
        if connection_id == 0 {
            return Err(RuntimeError::ConsoleFrame);
        }
        let control = match kind {
            ExchangeKind::SendLine => {
                let payload =
                    core::str::from_utf8(payload).map_err(|_| RuntimeError::ConsoleFrame)?;
                let trimmed = payload.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() || trimmed.len() > CONSOLE_OUTPUT_BYTES {
                    return Err(RuntimeError::ConsoleFrame);
                }
                ValidatedRootControl::SendLine(payload)
            }
            ExchangeKind::Disconnect => {
                if !payload.is_empty() {
                    return Err(RuntimeError::ConsoleFrame);
                }
                ValidatedRootControl::Disconnect
            }
            ExchangeKind::SendBatch => ValidatedRootControl::SendBatch(
                SendBatchCursor::validate(payload).map_err(|_| RuntimeError::ConsoleFrame)?,
            ),
            _ => return Err(RuntimeError::ConsoleFrame),
        };

        if self.session.connection_id() != Some(connection_id) {
            return Ok(ControlApplyOutcome::StaleConnection);
        }

        match control {
            ValidatedRootControl::SendLine(line) => {
                self.session.queue_authorized_line(line)?;
            }
            ValidatedRootControl::SendBatch(cursor) => {
                self.session.queue_authorized_batch(payload, cursor)?;
            }
            ValidatedRootControl::Disconnect => self.session.request_disconnect(),
        }
        Ok(ControlApplyOutcome::Applied)
    }

    /// Run one internally bounded unit of the retained service-poll cycle.
    ///
    /// The cursor successor is committed before the selected work starts, so a
    /// timeout or other terminal fault identifies the exact attempted unit.
    /// StackIngress, StackEgress, and Session therefore execute in separate
    /// scheduler iterations. The selected smoltcp entry points each have a
    /// bounded work contract, unlike `Interface::poll`. A successful
    /// complete-frame commit retains one fresh three-unit cycle; pending output
    /// without socket capacity does not.
    pub fn poll_service_unit(&mut self, now_ms: u64) -> Result<ServicePollOutcome, RuntimeError> {
        if self.terminal {
            return Err(RuntimeError::Terminal);
        }
        let unit = self.poll_unit;
        self.poll_unit = unit.successor();
        let timestamp = Instant::from_millis(now_ms.min(i64::MAX as u64) as i64);
        match unit {
            ServicePollUnit::StackIngress => {
                self.poll_stack_ingress_unit(timestamp);
                Ok(ServicePollOutcome::Continuation)
            }
            ServicePollUnit::StackEgress => {
                self.poll_stack_egress_unit(timestamp);
                Ok(ServicePollOutcome::Continuation)
            }
            ServicePollUnit::Session => {
                if self.poll_session_unit(now_ms)? {
                    Ok(ServicePollOutcome::Continuation)
                } else {
                    Ok(ServicePollOutcome::Complete)
                }
            }
        }
    }

    #[inline(never)]
    fn poll_stack_ingress_unit(&mut self, timestamp: Instant) {
        let _ = self
            .interface
            .poll_ingress_single(timestamp, &mut self.device, &mut self.sockets);
    }

    #[inline(never)]
    fn poll_stack_egress_unit(&mut self, timestamp: Instant) {
        let _ = self
            .interface
            .poll_egress(timestamp, &mut self.device, &mut self.sockets);
    }

    #[inline(never)]
    fn poll_session_unit(&mut self, now_ms: u64) -> Result<bool, RuntimeError> {
        let mut committed_wire_frame = false;
        let state = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        if state == TcpState::Established && self.session.connection_id().is_none() {
            let connection_id = self.next_connection_id;
            self.next_connection_id = self.next_connection_id.saturating_add(1).max(1);
            self.session.begin(connection_id, now_ms)?;
        }
        if state == TcpState::CloseWait {
            // The peer has closed its transmit half. Retain any output already
            // authorized for this exact generation, then actively complete the
            // server half of the close so smoltcp can reach Closed and relisten.
            // Without this transition, a normal host disconnect leaves the
            // sole listener parked in CloseWait and every replacement TCP
            // connection is reset before AUTH.
            self.session.request_disconnect();
        }

        let mut chunk = [0u8; SESSION_INGRESS_BYTES];
        let ingress_capacity = self.session.ingress_capacity().min(chunk.len());
        let received = {
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            if ingress_capacity != 0 && socket.can_recv() {
                match socket.recv_slice(&mut chunk[..ingress_capacity]) {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(RuntimeError::Terminal),
                }
            } else {
                0
            }
        };
        if received != 0 {
            match self.session.ingest(&chunk[..received], now_ms) {
                Ok(()) | Err(RuntimeError::ConsoleFrame) => {}
                Err(error) => return Err(error),
            }
        }
        self.session.tick(now_ms)?;
        let _ = self.session.stage_next_batch_line()?;

        let mut output = [0u8; CONSOLE_PAYLOAD_BYTES + FRAME_PREFIX_BYTES];
        if let Some(length) = self.session.peek_wire_output(&mut output)? {
            let (can_send, available) = {
                let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
                (
                    socket.can_send(),
                    socket.send_capacity().saturating_sub(socket.send_queue()),
                )
            };
            if can_send && available >= length {
                let sent = match self
                    .sockets
                    .get_mut::<TcpSocket>(self.tcp_handle)
                    .send_slice(&output[..length])
                {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(RuntimeError::Backpressure),
                };
                if sent != length {
                    return Err(RuntimeError::Backpressure);
                }
                self.session.commit_wire_output()?;
                committed_wire_frame = true;
            }
        }
        if self.session.close_ready() {
            self.sockets.get_mut::<TcpSocket>(self.tcp_handle).close();
        }

        let current = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        if current == TcpState::TimeWait
            || (current == TcpState::Closed && self.last_tcp_state != TcpState::Closed)
        {
            self.session.end(now_ms)?;
            if current == TcpState::TimeWait {
                // The sole listener cannot remain captive to smoltcp's close
                // timer: a replacement SYN restarts that timer and can keep
                // the service unavailable indefinitely. The previous root-
                // owned console treated TIME-WAIT as an ended application
                // generation. Preserve that boundary by discarding only the
                // completed TCP control block before rebinding the listener.
                self.sockets.get_mut::<TcpSocket>(self.tcp_handle).abort();
            }
            self.sockets
                .get_mut::<TcpSocket>(self.tcp_handle)
                .listen(IpListenEndpoint::from(self.listener_port))
                .map_err(|_| RuntimeError::ListenerBind)?;
        }
        self.last_tcp_state = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        Ok(committed_wire_frame || received != 0)
    }

    /// Pop one authenticated transport event for root policy.
    pub fn pop_event(&mut self) -> Option<ServiceEvent> {
        self.session.pop_event()
    }

    /// Pop one bounded publication event using the generated command quantum.
    pub fn pop_publication_event(
        &mut self,
        max_commands_per_wake: u16,
    ) -> Result<Option<ServiceEvent>, RuntimeError> {
        self.session.pop_publication_event(max_commands_per_wake)
    }

    /// Whether one typed service event is retained for publication.
    #[must_use]
    pub fn service_event_pending(&self) -> bool {
        !self.session.events.is_empty()
    }

    /// Exact child-created identity currently owned by the transport session.
    #[must_use]
    pub const fn active_connection_id(&self) -> Option<u64> {
        self.session.connection_id()
    }

    /// Connection whose root-authorized bytes have fully left smoltcp's send queue.
    #[must_use]
    pub fn output_drained_connection(&self) -> Option<u64> {
        let connection_id = self.session.connection_id()?;
        let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
        (self.session.output_queue_empty() && socket.send_queue() == 0).then_some(connection_id)
    }

    /// Revoke all old-generation state and stop service permanently.
    pub fn revoke(&mut self) {
        self.sockets.get_mut::<TcpSocket>(self.tcp_handle).abort();
        self.session.revoke();
        self.terminal = true;
    }

    /// Whether exactly one socket remains in LISTEN state.
    #[must_use]
    pub fn listener_ready(&self) -> bool {
        self.sockets.get::<TcpSocket>(self.tcp_handle).state() == TcpState::Listen
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::mem::size_of;

    use super::*;
    use abi::{
        RuntimeInitDescriptor, ABI_VERSION, AUTH_TOKEN_BYTES, CHILD_CSPACE_SLOTS, CHILD_WAKE_MASK,
        CHILD_WAKE_NOTIFICATION_SLOT, ETHERNET_FRAME_BYTES, FAULT_ENDPOINT_SLOT,
        PACKET_TX_WAKE_NOTIFICATION_SLOT, REQUIRED_INIT_FLAGS, ROOT_WAKE_MASK, RUNTIME_INIT_MAGIC,
        SHARED_PAGE_BYTES, SUPERVISOR_WAKE_NOTIFICATION_SLOT,
    };
    use smoltcp::phy::ChecksumCapabilities;
    use smoltcp::wire::{
        ArpOperation, ArpPacket, ArpRepr, EthernetFrame, EthernetProtocol, EthernetRepr,
        Icmpv4Packet, Icmpv4Repr, IpProtocol, Ipv4Packet, Ipv4Repr,
    };

    #[test]
    fn direct_tx_backpressure_blocks_until_peer_or_independent_work() {
        assert!(!direct_service_repoll_required(
            false, false, false, true, true, false,
        ));
        assert!(direct_service_repoll_required(
            false, false, false, true, false, false,
        ));
        assert!(direct_service_repoll_required(
            false, false, false, true, true, true,
        ));
        assert!(direct_service_repoll_required(
            false, true, false, true, true, false,
        ));
    }

    fn descriptor() -> RuntimeInitDescriptor {
        let mut token = [0; AUTH_TOKEN_BYTES];
        token[..6].copy_from_slice(b"secret");
        RuntimeInitDescriptor {
            magic: RUNTIME_INIT_MAGIC,
            abi_version: ABI_VERSION,
            descriptor_bytes: size_of::<RuntimeInitDescriptor>() as u16,
            flags: REQUIRED_INIT_FLAGS,
            reserved0: 0,
            generation: 7,
            child_wake_notification_slot: CHILD_WAKE_NOTIFICATION_SLOT,
            packet_tx_wake_notification_slot: PACKET_TX_WAKE_NOTIFICATION_SLOT,
            supervisor_wake_notification_slot: SUPERVISOR_WAKE_NOTIFICATION_SLOT,
            fault_endpoint_slot: FAULT_ENDPOINT_SLOT,
            child_cspace_slots: CHILD_CSPACE_SLOTS,
            root_wake_mask: ROOT_WAKE_MASK,
            child_wake_mask: CHILD_WAKE_MASK,
            ipc_buffer_vaddr: 0x7200_0000,
            packet_rx_vaddr: 0x7202_0000,
            packet_tx_vaddr: 0x7202_1000,
            command_vaddr: 0x7202_2000,
            event_vaddr: 0x7202_3000,
            shared_frame_bytes: SHARED_PAGE_BYTES as u32,
            ethernet_frame_bytes: ETHERNET_FRAME_BYTES as u16,
            listener_port: 31_337,
            max_packets_per_wake: 1,
            max_commands_per_wake: 1,
            max_control_inflight: 1,
            prefix_len: 24,
            auth_token_len: 6,
            reserved1: 0,
            mac: [2, 0, 0, 0, 0, 1],
            ipv4: [10, 0, 2, 15],
            gateway: [10, 0, 2, 2],
            reserved2: [0; 2],
            auth_timeout_ms: 5000,
            idle_timeout_ms: 300_000,
            timer_clock_hz: 62_500_000,
            auth_token: token,
            seal: 0,
        }
        .sealed()
    }

    fn framed(payload: &[u8]) -> std::vec::Vec<u8> {
        let mut frame = (payload.len() as u32 + 4).to_le_bytes().to_vec();
        frame.extend_from_slice(payload);
        frame
    }

    fn send_batch_payload(lines: &[&str]) -> ([u8; CONSOLE_PAYLOAD_BYTES], usize) {
        let mut payload = [0u8; CONSOLE_PAYLOAD_BYTES];
        let payload_len = {
            let mut builder = abi::SendBatchBuilder::new(&mut payload);
            for line in lines {
                assert_eq!(builder.try_push_line(line), Ok(true));
            }
            builder.finish().unwrap().len()
        };
        (payload, payload_len)
    }

    fn poll_complete_service_cycle(
        service: &mut ConsoleNetworkService<'_>,
        now_ms: u64,
    ) -> Result<usize, RuntimeError> {
        let mut committed_frames = 0usize;
        for _ in 0..=abi::SEND_BATCH_MAX_RECORDS {
            assert_eq!(service.poll_unit, ServicePollUnit::StackIngress);
            assert_eq!(
                service.poll_service_unit(now_ms)?,
                ServicePollOutcome::Continuation
            );
            assert_eq!(service.poll_unit, ServicePollUnit::StackEgress);
            assert_eq!(
                service.poll_service_unit(now_ms)?,
                ServicePollOutcome::Continuation
            );
            assert_eq!(service.poll_unit, ServicePollUnit::Session);
            let outcome = service.poll_service_unit(now_ms)?;
            assert_eq!(service.poll_unit, ServicePollUnit::StackIngress);
            if outcome == ServicePollOutcome::Complete {
                return Ok(committed_frames);
            }
            committed_frames = committed_frames.saturating_add(1);
        }
        Err(RuntimeError::Backpressure)
    }

    fn drive_network_turn(
        service: &mut ConsoleNetworkService<'_>,
        client_interface: &mut Interface,
        client_device: &mut SharedFrameDevice,
        client_sockets: &mut SocketSet<'_>,
        now_ms: u64,
    ) -> Result<(), RuntimeError> {
        let timestamp = Instant::from_millis(now_ms.min(i64::MAX as u64) as i64);
        let _ = client_interface.poll(timestamp, client_device, client_sockets);

        let mut packet = [0u8; ETHERNET_FRAME_BYTES];
        if let Some(length) = client_device.pop_egress(&mut packet)? {
            service.ingest_packet(&packet[..length])?;
        }
        let _ = poll_complete_service_cycle(service, now_ms)?;

        if let Some(length) = service.take_packet(&mut packet)? {
            client_device.push_ingress(&packet[..length])?;
        }
        let _ = client_interface.poll(timestamp, client_device, client_sockets);
        Ok(())
    }

    #[test]
    fn child_turn_scheduler_retains_priority_and_orders_input_after_service() {
        let mut scheduler = ChildTurnScheduler::new();
        scheduler.retain_notification(true, true);

        let readiness = ChildTurnReadiness::new(true, true, true);
        assert_eq!(
            scheduler.take_next(readiness),
            ChildTurnUnit::PublishCompletion
        );
        assert!(scheduler.retained_work_pending());
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::new(false, true, true)),
            ChildTurnUnit::PublishServiceEvent
        );
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::new(false, false, true)),
            ChildTurnUnit::PublishEgress
        );

        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::IngestPacket
        );
        // A stable empty/stale page retires only its coalesced input hint and
        // retains one complete service cycle. A later independently signalled
        // control remains ordered behind that cycle.
        scheduler.complete(ChildTurnUnit::IngestPacket);
        scheduler.request_service();
        scheduler.retain_notification(false, true);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::PollService
        );
        scheduler.complete(ChildTurnUnit::PollService);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::ApplyControl
        );
        scheduler.complete(ChildTurnUnit::ApplyControl);
        assert!(!scheduler.retained_work_pending());
    }

    #[test]
    fn local_poll_eligibility_gates_only_publication_and_idle() {
        let scheduler = ChildTurnScheduler::new();
        assert!(!scheduler.local_poll_eligible(false, ChildTurnReadiness::default()));
        assert!(!scheduler.local_poll_eligible(true, ChildTurnReadiness::default()));
        for readiness in [
            ChildTurnReadiness::new(true, false, false),
            ChildTurnReadiness::new(false, true, false),
            ChildTurnReadiness::new(false, false, true),
        ] {
            assert_ne!(scheduler.take_next(readiness), ChildTurnUnit::Idle);
            assert!(
                !scheduler.local_poll_eligible(false, readiness),
                "publication readiness cannot mint its own page credit"
            );
            assert!(
                scheduler.local_poll_eligible(true, readiness),
                "one explicit Observe->ACK credits the exact next publication"
            );
            assert!(scheduler.take_next(readiness).is_publication());
        }

        let mut ingress = ChildTurnScheduler::new();
        ingress.retain_notification(true, false);
        assert!(ingress.local_poll_eligible(false, ChildTurnReadiness::default()));
        assert!(ingress.local_poll_eligible(true, ChildTurnReadiness::default()));
        ingress.complete(ChildTurnUnit::IngestPacket);
        assert!(!ingress.local_poll_eligible(true, ChildTurnReadiness::default()));

        let mut control = ChildTurnScheduler::new();
        control.retain_notification(false, true);
        assert!(control.local_poll_eligible(false, ChildTurnReadiness::default()));
        assert!(control.local_poll_eligible(true, ChildTurnReadiness::default()));
        control.complete(ChildTurnUnit::ApplyControl);
        assert!(!control.local_poll_eligible(true, ChildTurnReadiness::default()));

        let mut service = ChildTurnScheduler::new();
        service.request_service();
        assert!(service.local_poll_eligible(false, ChildTurnReadiness::default()));
        assert!(service.local_poll_eligible(true, ChildTurnReadiness::default()));
        let publication = ChildTurnReadiness::new(true, true, true);
        assert!(
            !service.local_poll_eligible(false, publication),
            "a queued publication cannot proceed after earlier credit was consumed"
        );
        assert!(service.local_poll_eligible(true, publication));
        assert_eq!(
            service.take_next(publication),
            ChildTurnUnit::PublishCompletion,
            "retained-first priority spends the credit on exactly one publication"
        );
        service.complete(ChildTurnUnit::PollService);
        assert!(!service.local_poll_eligible(true, ChildTurnReadiness::default()));
    }

    #[test]
    fn observe_ack_credit_survives_internal_polls_and_serializes_pages() {
        let mut scheduler = ChildTurnScheduler::new();
        scheduler.request_service();
        let mut publication_credit_available = false;

        let all_ready = ChildTurnReadiness::new(true, true, true);
        assert!(!scheduler.local_poll_eligible(publication_credit_available, all_ready));
        // Root grants one credit only after accepting the preceding shared page.
        publication_credit_available = true;
        assert!(scheduler.local_poll_eligible(publication_credit_available, all_ready));
        assert_eq!(
            scheduler.take_next(all_ready),
            ChildTurnUnit::PublishCompletion
        );
        publication_credit_available = false;
        assert!(
            !scheduler.local_poll_eligible(publication_credit_available, all_ready),
            "one semantic publication must be observed before another event or egress publication"
        );

        // A later Observe->ACK grants exactly one new credit.
        publication_credit_available = true;
        let event_and_egress = ChildTurnReadiness::new(false, true, true);
        assert!(scheduler.local_poll_eligible(publication_credit_available, event_and_egress));
        assert_eq!(
            scheduler.take_next(event_and_egress),
            ChildTurnUnit::PublishServiceEvent
        );
        publication_credit_available = false;
        assert!(!scheduler.local_poll_eligible(publication_credit_available, event_and_egress));

        publication_credit_available = true;
        let egress = ChildTurnReadiness::new(false, false, true);
        assert!(scheduler.local_poll_eligible(publication_credit_available, egress));
        assert_eq!(scheduler.take_next(egress), ChildTurnUnit::PublishEgress);
        publication_credit_available = false;
        assert!(!scheduler.local_poll_eligible(publication_credit_available, egress));

        // Internal units remain live without credit and preserve one when it is
        // present until a later publication consumes it.
        publication_credit_available = true;
        assert!(scheduler
            .local_poll_eligible(publication_credit_available, ChildTurnReadiness::default()));
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::PollService,
            "publication preemption cannot clear the retained service cursor"
        );
        assert!(scheduler.local_poll_eligible(
            publication_credit_available,
            ChildTurnReadiness::new(false, true, false)
        ));
    }

    #[test]
    fn unrelated_wake_cannot_credit_or_overwrite_an_unobserved_page() {
        let scheduler = ChildTurnScheduler::new();
        let completion = ChildTurnReadiness::new(true, false, false);
        let mut publication_credit_available = true;

        assert!(scheduler.local_poll_eligible(publication_credit_available, completion));
        assert_eq!(
            scheduler.take_next(completion),
            ChildTurnUnit::PublishCompletion
        );
        publication_credit_available = false;

        // A packet/control wake can arrive after the entry Poll but before the
        // page commit. Consuming that wake from the following blocking Wait is
        // not evidence that root observed the page just committed.
        let unrelated_wait_returned = true;
        assert!(unrelated_wait_returned);
        assert!(!publication_credit_available);
        assert!(!scheduler.local_poll_eligible(publication_credit_available, completion));

        // Only the distinct ACK sent after root accepted the page restores the
        // single token and permits the next sequence-last publication.
        publication_credit_available = true;
        assert!(scheduler.local_poll_eligible(publication_credit_available, completion));
    }

    #[test]
    fn retained_local_cycle_rechecks_gates_between_units_and_quiesces() {
        fn admitted_unit(
            scheduler: &ChildTurnScheduler,
            urgent_badge_pending: bool,
            publication_credit_available: bool,
            readiness: ChildTurnReadiness,
        ) -> Option<ChildTurnUnit> {
            if urgent_badge_pending
                || !scheduler.local_poll_eligible(publication_credit_available, readiness)
            {
                return None;
            }
            Some(scheduler.take_next(readiness))
        }

        fn apply_service_outcome(scheduler: &mut ChildTurnScheduler, outcome: ServicePollOutcome) {
            if outcome == ServicePollOutcome::Complete {
                scheduler.complete(ChildTurnUnit::PollService);
            }
        }

        let mut scheduler = ChildTurnScheduler::new();
        scheduler.request_service();
        let no_publication = ChildTurnReadiness::default();

        // The first bounded service unit retains the outer cycle.
        assert_eq!(
            admitted_unit(&scheduler, false, false, no_publication),
            Some(ChildTurnUnit::PollService)
        );
        let mut service_units = 1usize;
        apply_service_outcome(&mut scheduler, ServicePollOutcome::Continuation);
        assert!(scheduler.retained_work_pending());

        // Loop re-entry checks urgent badges before admitting the next retained
        // unit. This is a pure ordering model, not an seL4 timing simulation.
        assert_eq!(admitted_unit(&scheduler, true, false, no_publication), None);
        assert!(scheduler.retained_work_pending());

        // Publication priority is also recomputed on loop re-entry. Retained
        // service cannot bypass an uncredited page, and one explicit credit
        // admits only that publication while leaving the service cycle intact.
        let service_event = ChildTurnReadiness::new(false, true, false);
        assert_eq!(admitted_unit(&scheduler, false, false, service_event), None);
        assert_eq!(
            admitted_unit(&scheduler, false, true, service_event),
            Some(ChildTurnUnit::PublishServiceEvent)
        );
        assert!(scheduler.retained_work_pending());

        for outcome in [
            ServicePollOutcome::Continuation,
            ServicePollOutcome::Complete,
        ] {
            assert_eq!(
                admitted_unit(&scheduler, false, false, no_publication),
                Some(ChildTurnUnit::PollService)
            );
            service_units += 1;
            apply_service_outcome(&mut scheduler, outcome);
            if outcome == ServicePollOutcome::Continuation {
                assert!(scheduler.retained_work_pending());
            }
        }

        assert_eq!(service_units, 3);
        assert!(!scheduler.retained_work_pending());
        assert_eq!(scheduler.take_next(no_publication), ChildTurnUnit::Idle);
        assert_eq!(admitted_unit(&scheduler, false, true, no_publication), None);
    }

    #[test]
    fn empty_input_hints_drive_one_service_cycle_then_wait_idle() {
        let mut scheduler = ChildTurnScheduler::new();
        scheduler.retain_notification(false, true);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::ApplyControl
        );
        // The stable control page carried no new sequence: consume this tick
        // hint before retaining its separate bounded service-poll cycle.
        scheduler.complete(ChildTurnUnit::ApplyControl);
        scheduler.request_service();
        for _ in 0..3 {
            assert!(scheduler.local_poll_eligible(false, ChildTurnReadiness::default()));
            assert_eq!(
                scheduler.take_next(ChildTurnReadiness::default()),
                ChildTurnUnit::PollService
            );
        }
        scheduler.complete(ChildTurnUnit::PollService);
        assert!(!scheduler.local_poll_eligible(true, ChildTurnReadiness::default()));
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::Idle
        );

        // A later real control has its own notification and remains lossless.
        scheduler.retain_notification(false, true);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::ApplyControl
        );
        scheduler.complete(ChildTurnUnit::ApplyControl);
        assert!(!scheduler.local_poll_eligible(true, ChildTurnReadiness::default()));

        // Packet hints obey the same empty-page rule and cannot self-Poll.
        scheduler.retain_notification(true, false);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::IngestPacket
        );
        scheduler.complete(ChildTurnUnit::IngestPacket);
        scheduler.request_service();
        for _ in 0..3 {
            assert!(scheduler.local_poll_eligible(false, ChildTurnReadiness::default()));
            assert_eq!(
                scheduler.take_next(ChildTurnReadiness::default()),
                ChildTurnUnit::PollService
            );
        }
        scheduler.complete(ChildTurnUnit::PollService);
        assert!(!scheduler.local_poll_eligible(true, ChildTurnReadiness::default()));
    }

    #[test]
    fn stack_continuations_survive_publication_between_every_phase() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let mut service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();
        let mut scheduler = ChildTurnScheduler::new();

        scheduler.request_service();
        assert_eq!(service.poll_unit, ServicePollUnit::StackIngress);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::PollService
        );
        assert_eq!(
            service.poll_service_unit(1).unwrap(),
            ServicePollOutcome::Continuation
        );
        assert_eq!(service.poll_unit, ServicePollUnit::StackEgress);

        // Coalesced packet/control hints remain durable while publication
        // priority preempts the retained StackEgress continuation.
        scheduler.retain_notification(true, true);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::new(true, false, false)),
            ChildTurnUnit::PublishCompletion
        );
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::PollService
        );
        assert_eq!(
            service.poll_service_unit(2).unwrap(),
            ServicePollOutcome::Continuation
        );
        assert_eq!(service.poll_unit, ServicePollUnit::Session);

        // Publication may also preempt between StackEgress and Session without
        // clearing the same retained outer PollService unit.
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::new(false, true, false)),
            ChildTurnUnit::PublishServiceEvent
        );
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::PollService
        );
        assert_eq!(
            service.poll_service_unit(3).unwrap(),
            ServicePollOutcome::Complete
        );
        assert_eq!(service.poll_unit, ServicePollUnit::StackIngress);
        scheduler.complete(ChildTurnUnit::PollService);
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::IngestPacket
        );
    }

    #[test]
    fn session_error_commits_internal_successor_but_retains_outer_work() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let mut service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();
        let mut scheduler = ChildTurnScheduler::new();
        scheduler.request_service();

        assert_eq!(
            service.poll_service_unit(1).unwrap(),
            ServicePollOutcome::Continuation
        );
        assert_eq!(service.poll_unit, ServicePollUnit::StackEgress);
        assert_eq!(
            service.poll_service_unit(2).unwrap(),
            ServicePollOutcome::Continuation
        );
        assert_eq!(service.poll_unit, ServicePollUnit::Session);
        service.session.state = AuthState::Authenticated;
        service.session.connection_id = 1;
        for now_ms in 0..SESSION_EVENT_DEPTH as u64 {
            service
                .session
                .push_event(ExchangeKind::Command, now_ms, b"x")
                .unwrap();
        }
        service
            .sockets
            .get_mut::<TcpSocket>(service.tcp_handle)
            .abort();

        assert_eq!(
            service.poll_service_unit(3),
            Err(RuntimeError::Backpressure)
        );
        assert_eq!(
            service.poll_unit,
            ServicePollUnit::StackIngress,
            "the faulting Session committed its successor before work"
        );
        assert_eq!(
            scheduler.take_next(ChildTurnReadiness::default()),
            ChildTurnUnit::PollService,
            "an error cannot clear the outer retained service unit"
        );
    }

    #[test]
    fn authentication_and_command_release_are_child_owned() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 10).unwrap();
        assert_eq!(session.pop_event().unwrap().kind(), ExchangeKind::Connected);
        session.ingest(&framed(b"AUTH secret"), 20).unwrap();
        assert!(session.authenticated());
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );
        let mut wire = [0u8; 64];
        let len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..len], b"OK AUTH");
        session.ingest(&framed(b"cat /proc/boot"), 30).unwrap();
        let command = session.pop_event().unwrap();
        assert_eq!(command.kind(), ExchangeKind::Command);
        assert_eq!(command.payload().unwrap(), "cat /proc/boot");
    }

    #[test]
    fn publication_event_coalesces_only_the_bounded_consecutive_command_run() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 10).unwrap();
        assert_eq!(session.pop_event().unwrap().kind(), ExchangeKind::Connected);
        session.ingest(&framed(b"AUTH secret"), 20).unwrap();
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );

        for (now_ms, command) in [
            (30, b"help".as_slice()),
            (31, b"smp"),
            (32, b"bi"),
            (33, b"caps"),
        ] {
            session.ingest(&framed(command), now_ms).unwrap();
        }
        let batch = session.pop_publication_event(3).unwrap().unwrap();
        assert_eq!(batch.kind(), ExchangeKind::CommandBatch);
        assert_eq!(batch.connection_id(), 1);
        let mut cursor = abi::CommandBatchCursor::validate(batch.payload_bytes()).unwrap();
        assert_eq!(
            cursor.next_command(batch.payload_bytes()).unwrap(),
            Some((30, "help"))
        );
        assert_eq!(
            cursor.next_command(batch.payload_bytes()).unwrap(),
            Some((31, "smp"))
        );
        assert_eq!(
            cursor.next_command(batch.payload_bytes()).unwrap(),
            Some((32, "bi"))
        );
        assert_eq!(cursor.next_command(batch.payload_bytes()), Ok(None));

        let remaining = session.pop_publication_event(8).unwrap().unwrap();
        assert_eq!(remaining.kind(), ExchangeKind::Command);
        assert_eq!(remaining.payload().unwrap(), "caps");
    }

    #[test]
    fn lifecycle_events_fence_command_publication_batches() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 10).unwrap();
        assert_eq!(session.pop_event().unwrap().kind(), ExchangeKind::Connected);
        session.ingest(&framed(b"AUTH secret"), 20).unwrap();
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );
        session.ingest(&framed(b"help"), 30).unwrap();
        session.end(31).unwrap();

        let command = session.pop_publication_event(8).unwrap().unwrap();
        assert_eq!(command.kind(), ExchangeKind::Command);
        assert_eq!(command.payload().unwrap(), "help");
        assert_eq!(
            session.pop_publication_event(8).unwrap().unwrap().kind(),
            ExchangeKind::Disconnected
        );
    }

    #[test]
    fn stale_control_cannot_mutate_an_ended_or_replacement_connection() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let mut service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();

        service.session.begin(1, 1).unwrap();
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendLine, b"pre-auth"),
            Err(RuntimeError::Unauthenticated),
            "an exact pre-authentication control remains terminal"
        );
        assert_eq!(
            service.apply_control(0, ExchangeKind::SendLine, b"zero-id"),
            Err(RuntimeError::ConsoleFrame),
            "zero is never a stale connection identity"
        );
        assert_eq!(
            service.apply_control(9, ExchangeKind::Disconnect, b"malformed"),
            Err(RuntimeError::ConsoleFrame),
            "malformed controls remain terminal even when their identity is stale"
        );

        service.session.state = AuthState::Authenticated;
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendLine, b"old-current"),
            Ok(ControlApplyOutcome::Applied)
        );
        assert!(!service.session.output_queue_empty());
        service.session.end(2).unwrap();
        assert!(service.session.output_queue_empty());
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendLine, b"after-end"),
            Ok(ControlApplyOutcome::StaleConnection)
        );
        assert!(service.session.output_queue_empty());

        service.session.begin(2, 3).unwrap();
        service.session.state = AuthState::Authenticated;
        assert_eq!(
            service.apply_control(2, ExchangeKind::SendLine, b"replacement-current"),
            Ok(ControlApplyOutcome::Applied)
        );
        let queued_before_stale = service.session.outbound.len();
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendLine, b"old-generation"),
            Ok(ControlApplyOutcome::StaleConnection)
        );
        assert_eq!(
            service.session.outbound.len(),
            queued_before_stale,
            "an old control cannot enter, clear, or reorder replacement output"
        );
        assert_eq!(
            service.session.outbound.front().unwrap().as_slice(),
            b"replacement-current"
        );
    }

    #[test]
    fn authorized_batch_is_atomic_and_stages_one_external_frame_per_session_unit() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let mut service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();
        service.session.begin(1, 1).unwrap();
        service.session.state = AuthState::Authenticated;
        let lines = [
            "ACK CAT", "body-0", "body-1", "body-2", "body-3", "body-4", "body-5", "END CAT",
        ];
        let (payload, payload_len) = send_batch_payload(&lines);

        assert_eq!(
            service.apply_control(1, ExchangeKind::SendBatch, &payload[..payload_len],),
            Ok(ControlApplyOutcome::Applied)
        );
        assert!(service.session.outbound.is_empty());
        assert_eq!(
            service
                .session
                .pending_batch
                .as_ref()
                .map(|batch| batch.cursor.remaining()),
            Some(lines.len())
        );
        service.session.request_disconnect();
        assert!(!service.session.close_ready());

        for (index, expected) in lines.iter().enumerate() {
            assert_eq!(service.session.stage_next_batch_line(), Ok(true));
            let remaining_after_stage = lines.len().saturating_sub(index + 1);
            assert_eq!(
                service
                    .session
                    .pending_batch
                    .as_ref()
                    .map(|batch| batch.cursor.remaining()),
                (remaining_after_stage != 0).then_some(remaining_after_stage)
            );
            assert_eq!(
                service.session.stage_next_batch_line(),
                Ok(false),
                "one Session unit cannot stage a second batch record"
            );

            let mut wire = [0u8; CONSOLE_OUTPUT_BYTES + FRAME_PREFIX_BYTES];
            let frame_len = service.session.pop_wire_output(&mut wire).unwrap().unwrap();
            assert_eq!(
                u32::from_le_bytes(wire[..4].try_into().unwrap()) as usize,
                frame_len
            );
            assert_eq!(&wire[4..frame_len], expected.as_bytes());
            assert_eq!(
                service.session.output_queue_empty(),
                index + 1 == lines.len()
            );
        }
        assert!(service.session.close_ready());
    }

    #[test]
    fn malformed_overlapping_stale_and_terminal_batches_preserve_state() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let mut service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();
        service.session.begin(1, 1).unwrap();
        service.session.state = AuthState::Authenticated;
        let (payload, payload_len) = send_batch_payload(&["ACK LOG", "END LOG"]);
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendBatch, &payload[..payload_len],),
            Ok(ControlApplyOutcome::Applied)
        );
        let accepted = service.session.pending_batch.clone();

        let mut malformed = payload;
        malformed[4..6].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendBatch, &malformed[..payload_len],),
            Err(RuntimeError::ConsoleFrame)
        );
        assert_eq!(service.session.pending_batch, accepted);
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendBatch, &payload[..payload_len],),
            Err(RuntimeError::Backpressure)
        );
        assert_eq!(service.session.pending_batch, accepted);
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendLine, b"legacy"),
            Err(RuntimeError::Backpressure)
        );
        assert_eq!(service.session.pending_batch, accepted);

        service.session.end(2).unwrap();
        assert!(service.session.output_queue_empty());
        service.session.begin(2, 3).unwrap();
        service.session.state = AuthState::Authenticated;
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendBatch, &payload[..payload_len],),
            Ok(ControlApplyOutcome::StaleConnection)
        );
        assert!(service.session.output_queue_empty());
        assert_eq!(
            service.apply_control(1, ExchangeKind::SendBatch, &malformed[..payload_len],),
            Err(RuntimeError::ConsoleFrame),
            "malformed input is rejected before stale identity classification"
        );
        assert!(service.session.output_queue_empty());

        assert_eq!(
            service.apply_control(2, ExchangeKind::SendBatch, &payload[..payload_len],),
            Ok(ControlApplyOutcome::Applied)
        );
        service.revoke();
        assert!(service.session.output_queue_empty());
        assert_eq!(service.session.pending_batch, None);
    }

    #[test]
    fn malformed_auth_and_partial_frame_fail_closed() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH wrong"), 1).unwrap();
        assert_eq!(session.pop_event().unwrap().kind(), ExchangeKind::Rejected);
        assert!(!session.close_ready());
        let mut wire = [0u8; 64];
        let len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..len], b"ERR AUTH reason=invalid-token");
        assert!(session.close_ready());
    }

    #[test]
    fn preauth_oversized_frame_retains_auth_rejection_and_close() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        let declared = (FRAME_PREFIX_BYTES + CONSOLE_PAYLOAD_BYTES + 1) as u32;

        assert_eq!(
            session.ingest(&declared.to_le_bytes(), 1),
            Err(RuntimeError::ConsoleFrame)
        );
        let rejected = session.pop_event().unwrap();
        assert_eq!(rejected.kind(), ExchangeKind::Rejected);
        assert_eq!(rejected.payload().unwrap(), "reason=invalid-length");
        assert!(!session.authenticated());
        assert!(!session.close_ready());

        let mut wire = [0u8; 64];
        let len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..len], b"ERR AUTH reason=invalid-length");
        assert!(session.close_ready());
    }

    #[test]
    fn authenticated_oversized_frame_is_drained_before_next_command() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(7, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );
        let mut wire = [0u8; 64];
        let auth_len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..auth_len], b"OK AUTH");

        let oversized = std::vec![b'x'; CONSOLE_PAYLOAD_BYTES + 17];
        let declared = (FRAME_PREFIX_BYTES + oversized.len()) as u32;
        session.ingest(&declared.to_le_bytes(), 2).unwrap();

        assert!(session.authenticated());
        assert_eq!(session.connection_id(), Some(7));
        assert_eq!(session.drop_remaining, oversized.len());
        assert_eq!(session.pop_event(), None);
        assert!(!session.close_ready());
        let error_len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..error_len], b"ERR FRAME reason=invalid-length");
        assert!(!session.close_ready());

        let split = 31;
        session.ingest(&oversized[..split], 3).unwrap();
        assert_eq!(session.drop_remaining, oversized.len() - split);
        assert_eq!(session.pop_event(), None);

        let mut final_fragment = oversized[split..].to_vec();
        final_fragment.extend_from_slice(&framed(b"ping"));
        let final_body_length = oversized.len() - split;
        assert_eq!(session.ingress_capacity(), final_body_length);
        assert_eq!(
            session.ingest(final_fragment.as_slice(), 4),
            Err(RuntimeError::Backpressure)
        );
        session
            .ingest(&final_fragment[..final_body_length], 4)
            .unwrap();
        session
            .ingest(&final_fragment[final_body_length..], 4)
            .unwrap();

        assert_eq!(session.drop_remaining, 0);
        assert!(session.authenticated());
        assert_eq!(session.connection_id(), Some(7));
        assert!(!session.close_ready());
        let command = session.pop_event().unwrap();
        assert_eq!(command.kind(), ExchangeKind::Command);
        assert_eq!(command.connection_id(), 7);
        assert_eq!(command.payload().unwrap(), "ping");
        assert_eq!(session.pop_event(), None);
    }

    #[test]
    fn authenticated_oversize_drain_survives_full_output_queue() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(9, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );
        let mut wire = [0u8; 64];
        let auth_len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[FRAME_PREFIX_BYTES..auth_len], b"OK AUTH");

        let prior_lines = [
            "OK PRIOR sequence=0",
            "OK PRIOR sequence=1",
            "OK PRIOR sequence=2",
            "OK PRIOR sequence=3",
            "OK PRIOR sequence=4",
            "OK PRIOR sequence=5",
            "OK PRIOR sequence=6",
            "OK PRIOR sequence=7",
        ];
        for line in prior_lines {
            session.queue_authorized_line(line).unwrap();
        }
        assert_eq!(session.outbound.len(), SESSION_OUTPUT_DEPTH);
        assert_eq!(session.ingress_capacity(), 0);

        let oversized = std::vec![b'x'; CONSOLE_PAYLOAD_BYTES + 17];
        let declared = (FRAME_PREFIX_BYTES + oversized.len()) as u32;
        assert_eq!(
            session.ingest(&declared.to_le_bytes(), 2),
            Err(RuntimeError::Backpressure)
        );
        assert_eq!(session.drop_remaining, 0);
        assert_eq!(session.length_pos, 0);
        assert_eq!(session.last_activity_ms, 1);
        assert_eq!(session.outbound.len(), SESSION_OUTPUT_DEPTH);

        for line in &prior_lines[..INVALID_LENGTH_OUTPUT_RESERVE] {
            let length = session.pop_wire_output(&mut wire).unwrap().unwrap();
            assert_eq!(&wire[FRAME_PREFIX_BYTES..length], line.as_bytes());
        }
        assert_eq!(session.ingress_capacity(), SESSION_INGRESS_BYTES);
        session.ingest(&declared.to_le_bytes(), 2).unwrap();
        assert_eq!(session.drop_remaining, oversized.len());
        assert_eq!(session.outbound.len(), SESSION_OUTPUT_DEPTH - 2);
        assert_eq!(session.pop_event(), None);
        assert!(!session.close_after_flush);

        let split = 37;
        session.ingest(&oversized[..split], 3).unwrap();
        assert_eq!(session.last_activity_ms, 1);
        let mut final_fragment = oversized[split..].to_vec();
        final_fragment.extend_from_slice(&framed(b"PING"));
        let final_body_length = oversized.len() - split;
        assert_eq!(session.ingress_capacity(), final_body_length);
        assert_eq!(
            session.ingest(final_fragment.as_slice(), 4),
            Err(RuntimeError::Backpressure)
        );
        assert_eq!(session.drop_remaining, final_body_length);
        assert_eq!(session.last_activity_ms, 1);
        session
            .ingest(&final_fragment[..final_body_length], 4)
            .unwrap();

        assert_eq!(session.drop_remaining, 0);
        assert!(session.authenticated());
        assert_eq!(session.connection_id(), Some(9));
        assert_eq!(session.last_activity_ms, 4);
        assert_eq!(session.ingress_capacity(), 0);
        assert_eq!(
            session.ingest(&final_fragment[final_body_length..], 5),
            Err(RuntimeError::Backpressure)
        );
        assert_eq!(session.last_activity_ms, 4);

        let length = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(
            &wire[FRAME_PREFIX_BYTES..length],
            prior_lines[INVALID_LENGTH_OUTPUT_RESERVE].as_bytes()
        );
        assert_eq!(session.ingress_capacity(), SESSION_INGRESS_BYTES);
        session
            .ingest(&final_fragment[final_body_length..], 5)
            .unwrap();
        let command = session.pop_event().unwrap();
        assert_eq!(command.kind(), ExchangeKind::Command);
        assert_eq!(command.connection_id(), 9);
        assert_eq!(command.payload().unwrap(), "PING");
        assert_eq!(session.pop_event(), None);
        assert!(!session.close_after_flush);

        for line in &prior_lines[INVALID_LENGTH_OUTPUT_RESERVE + 1..] {
            let length = session.pop_wire_output(&mut wire).unwrap().unwrap();
            assert_eq!(&wire[FRAME_PREFIX_BYTES..length], line.as_bytes());
        }
        let error_len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[FRAME_PREFIX_BYTES..error_len], INVALID_LENGTH_FRAME);
        assert_eq!(session.pop_wire_output(&mut wire), Ok(None));
        assert!(session.output_queue_empty());
        assert!(session.authenticated());
        assert_eq!(session.connection_id(), Some(9));
        assert!(!session.close_ready());
    }

    #[test]
    fn ingress_reserve_covers_three_invalid_lengths_in_one_maximum_chunk() {
        let mut waiting = TransportSession::new(descriptor()).unwrap();
        waiting.begin(11, 0).unwrap();
        for _ in 0..SESSION_OUTPUT_DEPTH - INVALID_LENGTH_OUTPUT_RESERVE + 1 {
            waiting.queue_wire_payload(b"queued").unwrap();
        }
        assert_eq!(waiting.ingress_capacity(), 0);
        let mut waiting_wire = [0u8; 64];
        let _ = waiting.pop_wire_output(&mut waiting_wire).unwrap();
        assert_eq!(waiting.ingress_capacity(), SESSION_INGRESS_BYTES);

        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(10, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );
        let mut wire = [0u8; 64];
        let _ = session.pop_wire_output(&mut wire).unwrap();

        let command_bound_payload = COMMAND_LINE_BYTES + 1;
        let command_bound_declared = (FRAME_PREFIX_BYTES + command_bound_payload) as u32;
        let mut partial = command_bound_declared.to_le_bytes().to_vec();
        partial.extend(std::iter::repeat_n(b'x', command_bound_payload - 1));
        session.ingest(partial.as_slice(), 2).unwrap();
        assert_eq!(session.payload.len(), command_bound_payload - 1);

        let mut maximum_chunk = std::vec![b'x'];
        maximum_chunk.extend_from_slice(&framed(&std::vec![b'y'; command_bound_payload]));
        let payload_oversize = CONSOLE_PAYLOAD_BYTES + 1;
        maximum_chunk
            .extend_from_slice(&((FRAME_PREFIX_BYTES + payload_oversize) as u32).to_le_bytes());
        assert!(maximum_chunk.len() <= SESSION_INGRESS_BYTES);
        session.ingest(maximum_chunk.as_slice(), 3).unwrap();

        assert_eq!(session.outbound.len(), INVALID_LENGTH_OUTPUT_RESERVE);
        assert_eq!(session.drop_remaining, payload_oversize);
        assert_eq!(session.pop_event(), None);
        assert!(session.authenticated());
        assert_eq!(session.connection_id(), Some(10));
        assert!(!session.close_after_flush);
        for _ in 0..INVALID_LENGTH_OUTPUT_RESERVE {
            let length = session.pop_wire_output(&mut wire).unwrap().unwrap();
            assert_eq!(&wire[FRAME_PREFIX_BYTES..length], INVALID_LENGTH_FRAME);
        }
        assert_eq!(session.pop_wire_output(&mut wire), Ok(None));
    }

    #[test]
    fn authentication_comparison_scans_the_fixed_token_bound() {
        assert!(constant_time_equal(b"secret", b"secret"));
        assert!(!constant_time_equal(b"secreu", b"secret"));
        assert!(!constant_time_equal(b"secret-extra", b"secret"));
        assert!(!constant_time_equal(b"", b"secret"));
    }

    #[test]
    fn authenticated_command_bound_matches_root_parser_capacity() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );
        let mut wire = [0u8; 64];
        let auth_len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..auth_len], b"OK AUTH");

        let oversized = [b'x'; abi::COMMAND_LINE_BYTES + 1];
        session.ingest(&framed(&oversized), 2).unwrap();
        assert!(session.authenticated());
        assert_eq!(session.connection_id(), Some(1));
        assert_eq!(session.pop_event(), None);
        assert!(!session.close_ready());
        let error_len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..error_len], INVALID_LENGTH_FRAME);

        session.ingest(&framed(b"ping"), 3).unwrap();
        let command = session.pop_event().unwrap();
        assert_eq!(command.kind(), ExchangeKind::Command);
        assert_eq!(command.connection_id(), 1);
        assert_eq!(command.payload().unwrap(), "ping");
        assert!(session.authenticated());
        assert!(!session.close_ready());
    }

    #[test]
    fn root_output_preserves_ack_err_end_bytes_and_auth_gate() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        assert_eq!(
            session.queue_authorized_line("OK CAT bytes=3"),
            Err(RuntimeError::Unauthenticated)
        );
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        let _ = session.pop_event();
        let mut discard = [0u8; 64];
        let _ = session.pop_wire_output(&mut discard).unwrap();
        for line in ["OK CAT bytes=3", "ERR CAT denied", "END CAT"] {
            session.queue_authorized_line(line).unwrap();
            let len = session.pop_wire_output(&mut discard).unwrap().unwrap();
            assert_eq!(&discard[4..len], line.as_bytes());
        }
    }

    #[test]
    fn wire_output_is_retained_until_tcp_accepts_the_complete_frame() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        let _ = session.pop_event();

        let mut first = [0u8; 64];
        let first_len = session.peek_wire_output(&mut first).unwrap().unwrap();
        let mut second = [0u8; 64];
        let second_len = session.peek_wire_output(&mut second).unwrap().unwrap();
        assert_eq!(first_len, second_len);
        assert_eq!(&first[..first_len], &second[..second_len]);
        session.commit_wire_output().unwrap();
        assert_eq!(session.peek_wire_output(&mut second).unwrap(), None);
    }

    #[test]
    fn one_packet_device_backpressures_without_overwrite() {
        let mut device = SharedFrameDevice::new();
        device.push_ingress(&[1, 2, 3]).unwrap();
        assert_eq!(device.push_ingress(&[4]), Err(RuntimeError::Backpressure));
        let timestamp = Instant::from_millis(0);
        let (rx, tx) = device.receive(timestamp).unwrap();
        assert_eq!(rx.consume(|packet| packet.to_vec()), [1, 2, 3]);
        tx.consume(3, |packet| packet.copy_from_slice(&[4, 5, 6]));
        let mut output = [0u8; 8];
        assert_eq!(device.pop_egress(&mut output).unwrap(), Some(3));
        assert_eq!(&output[..3], &[4, 5, 6]);
    }

    #[test]
    fn operational_smoltcp_service_has_exactly_one_listener() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();
        assert!(service.listener_ready());
    }

    #[test]
    fn operational_smoltcp_service_replies_to_icmp_echo_without_an_extra_socket() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let mut service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();
        let remote_mac = EthernetAddress([2, 0, 0, 0, 0, 2]);
        let remote_ip = Ipv4Address::new(10, 0, 2, 16);
        let local_mac = EthernetAddress(descriptor().mac);
        let local_ip = Ipv4Address::from(descriptor().ipv4);
        let arp = ArpRepr::EthernetIpv4 {
            operation: ArpOperation::Request,
            source_hardware_addr: remote_mac,
            source_protocol_addr: remote_ip,
            target_hardware_addr: EthernetAddress::BROADCAST,
            target_protocol_addr: local_ip,
        };
        let arp_ethernet = EthernetRepr {
            src_addr: remote_mac,
            dst_addr: EthernetAddress::BROADCAST,
            ethertype: EthernetProtocol::Arp,
        };
        let arp_len = arp_ethernet.buffer_len().saturating_add(arp.buffer_len());
        let mut arp_request = [0u8; ETHERNET_FRAME_BYTES];
        arp_ethernet.emit(&mut EthernetFrame::new_unchecked(
            &mut arp_request[..arp_len],
        ));
        arp.emit(&mut ArpPacket::new_unchecked(
            &mut arp_request[arp_ethernet.buffer_len()..arp_len],
        ));
        service.ingest_packet(&arp_request[..arp_len]).unwrap();
        assert_eq!(poll_complete_service_cycle(&mut service, 1).unwrap(), 0);
        let mut reply = [0u8; ETHERNET_FRAME_BYTES];
        assert!(service.take_packet(&mut reply).unwrap().is_some());

        let payload = [0xaa, 0x55, 0x00, 0xff];
        let ethernet = EthernetRepr {
            src_addr: remote_mac,
            dst_addr: local_mac,
            ethertype: EthernetProtocol::Ipv4,
        };
        let icmp = Icmpv4Repr::EchoRequest {
            ident: 0x1234,
            seq_no: 0xabcd,
            data: &payload,
        };
        let ipv4 = Ipv4Repr {
            src_addr: remote_ip,
            dst_addr: local_ip,
            next_header: IpProtocol::Icmp,
            payload_len: icmp.buffer_len(),
            hop_limit: 64,
        };
        let frame_len = ethernet
            .buffer_len()
            .saturating_add(ipv4.buffer_len())
            .saturating_add(icmp.buffer_len());
        let mut request = [0u8; ETHERNET_FRAME_BYTES];
        ethernet.emit(&mut EthernetFrame::new_unchecked(&mut request[..frame_len]));
        ipv4.emit(
            &mut Ipv4Packet::new_unchecked(&mut request[ethernet.buffer_len()..frame_len]),
            &ChecksumCapabilities::default(),
        );
        icmp.emit(
            &mut Icmpv4Packet::new_unchecked(
                &mut request[ethernet.buffer_len() + ipv4.buffer_len()..frame_len],
            ),
            &ChecksumCapabilities::default(),
        );

        service.ingest_packet(&request[..frame_len]).unwrap();
        assert_eq!(poll_complete_service_cycle(&mut service, 2).unwrap(), 0);

        let reply_len = service.take_packet(&mut reply).unwrap().unwrap();
        let ethernet_reply = EthernetFrame::new_checked(&reply[..reply_len]).unwrap();
        assert_eq!(ethernet_reply.src_addr(), local_mac);
        assert_eq!(ethernet_reply.dst_addr(), remote_mac);
        assert_eq!(ethernet_reply.ethertype(), EthernetProtocol::Ipv4);
        let ipv4_reply = Ipv4Packet::new_checked(ethernet_reply.payload()).unwrap();
        assert_eq!(ipv4_reply.src_addr(), local_ip);
        assert_eq!(ipv4_reply.dst_addr(), remote_ip);
        assert_eq!(ipv4_reply.next_header(), IpProtocol::Icmp);
        let icmp_reply = Icmpv4Packet::new_checked(ipv4_reply.payload()).unwrap();
        assert_eq!(
            Icmpv4Repr::parse(&icmp_reply, &ChecksumCapabilities::default()).unwrap(),
            Icmpv4Repr::EchoReply {
                ident: 0x1234,
                seq_no: 0xabcd,
                data: &payload,
            }
        );
        assert!(service.listener_ready());
    }

    #[test]
    fn buffered_oversize_body_and_quit_retain_service_until_same_connection_parse() {
        let mut server_rx = [0u8; 32 * 1024];
        let mut server_tx = [0u8; 4096];
        let mut server_storage = [SocketStorage::EMPTY];
        let mut service = ConsoleNetworkService::new(
            descriptor(),
            &mut server_rx,
            &mut server_tx,
            &mut server_storage,
        )
        .unwrap();

        let mut client_device = SharedFrameDevice::new();
        let mut client_config = InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress([
            2, 0, 0, 0, 0, 2,
        ])));
        client_config.random_seed = 17;
        let mut client_interface =
            Interface::new(client_config, &mut client_device, Instant::from_millis(0));
        client_interface.update_ip_addrs(|addresses| {
            addresses.clear();
            addresses
                .push(IpCidr::new(Ipv4Address::new(10, 0, 2, 16).into(), 24))
                .unwrap();
        });
        let mut client_rx = [0u8; 4096];
        let mut client_tx = [0u8; 16 * 1024];
        let mut client_storage = [SocketStorage::EMPTY];
        let mut client_sockets = SocketSet::new(&mut client_storage[..]);
        let client_handle = client_sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut client_rx[..]),
            TcpSocketBuffer::new(&mut client_tx[..]),
        ));
        client_sockets
            .get_mut::<TcpSocket>(client_handle)
            .connect(
                client_interface.context(),
                (Ipv4Address::new(10, 0, 2, 15), descriptor().listener_port),
                49_152,
            )
            .unwrap();

        let mut now_ms = 1u64;
        let mut auth_sent = false;
        let mut auth_response = std::vec::Vec::new();
        let expected_auth = framed(b"OK AUTH");
        let mut authenticated = false;
        for _ in 0..512 {
            drive_network_turn(
                &mut service,
                &mut client_interface,
                &mut client_device,
                &mut client_sockets,
                now_ms,
            )
            .unwrap();
            while service.pop_event().is_some() {}

            let client = client_sockets.get_mut::<TcpSocket>(client_handle);
            if !auth_sent && client.state() == TcpState::Established {
                let auth = framed(b"AUTH secret");
                assert_eq!(client.send_slice(auth.as_slice()).unwrap(), auth.len());
                auth_sent = true;
            }
            while client.can_recv() {
                let mut chunk = [0u8; 64];
                let length = client.recv_slice(&mut chunk).unwrap();
                if length == 0 {
                    break;
                }
                auth_response.extend_from_slice(&chunk[..length]);
            }

            let server = service.sockets.get::<TcpSocket>(service.tcp_handle);
            if auth_response == expected_auth
                && service.session.authenticated()
                && service.session.output_queue_empty()
                && server.state() == TcpState::Established
                && server.recv_queue() == 0
                && server.send_queue() == 0
            {
                authenticated = true;
                break;
            }
            now_ms = now_ms.saturating_add(1);
        }
        assert!(authenticated, "the in-memory peer must authenticate first");
        assert_eq!(service.session.connection_id(), Some(1));

        let oversized = std::vec![b'x'; 8 * 1024 + 65];
        let mut buffered_commands = framed(oversized.as_slice());
        buffered_commands.extend_from_slice(framed(b"quit").as_slice());
        assert!(buffered_commands.len() > 8 * 1024);
        assert_eq!(
            client_sockets
                .get_mut::<TcpSocket>(client_handle)
                .send_slice(buffered_commands.as_slice())
                .unwrap(),
            buffered_commands.len()
        );

        let mut entire_sequence_buffered = false;
        for _ in 0..512 {
            now_ms = now_ms.saturating_add(1);
            let timestamp = Instant::from_millis(now_ms as i64);
            let _ = client_interface.poll(timestamp, &mut client_device, &mut client_sockets);
            let mut packet = [0u8; ETHERNET_FRAME_BYTES];
            if let Some(length) = client_device.pop_egress(&mut packet).unwrap() {
                service.ingest_packet(&packet[..length]).unwrap();
                service.poll_stack_ingress_unit(timestamp);
            }
            service.poll_stack_egress_unit(timestamp);
            if let Some(length) = service.take_packet(&mut packet).unwrap() {
                client_device.push_ingress(&packet[..length]).unwrap();
            }
            let _ = client_interface.poll(timestamp, &mut client_device, &mut client_sockets);

            if service
                .sockets
                .get::<TcpSocket>(service.tcp_handle)
                .recv_queue()
                == buffered_commands.len()
            {
                entire_sequence_buffered = true;
                break;
            }
        }
        assert!(
            entire_sequence_buffered,
            "the complete oversized frame and following QUIT must be buffered"
        );

        let mut scheduler = ChildTurnScheduler::new();
        scheduler.request_service();
        let mut session_units = 0usize;
        let mut quit_event = None;
        for _ in 0..64 {
            assert_eq!(
                scheduler.take_next(ChildTurnReadiness::default()),
                ChildTurnUnit::PollService,
                "no fresh packet or control notification should be required"
            );
            let unit = service.poll_unit;
            let outcome = service.poll_service_unit(now_ms).unwrap();
            if unit == ServicePollUnit::Session {
                session_units = session_units.saturating_add(1);
                if let Some(event) = service.pop_event() {
                    quit_event = Some(event);
                }
                if outcome == ServicePollOutcome::Complete {
                    scheduler.complete(ChildTurnUnit::PollService);
                    break;
                }
            }
        }

        assert!(
            session_units >= 4,
            "the oversized body must span bounded reads"
        );
        assert!(!scheduler.retained_work_pending());
        let quit = quit_event.expect("the command after the oversized body must be parsed");
        assert_eq!(quit.kind(), ExchangeKind::Command);
        assert_eq!(quit.connection_id(), 1);
        assert_eq!(quit.payload().unwrap(), "quit");
        assert_eq!(service.session.connection_id(), Some(1));
        assert!(service.session.authenticated());
        assert!(!service.session.close_ready());
    }

    #[test]
    fn fin_wait_retains_unsendable_output_until_peer_close_relistens() {
        let mut server_rx = [0u8; 4096];
        let mut server_tx = [0u8; 4096];
        let mut server_storage = [SocketStorage::EMPTY];
        let mut service = ConsoleNetworkService::new(
            descriptor(),
            &mut server_rx,
            &mut server_tx,
            &mut server_storage,
        )
        .unwrap();

        let mut client_device = SharedFrameDevice::new();
        let mut client_config = InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress([
            2, 0, 0, 0, 0, 2,
        ])));
        client_config.random_seed = 9;
        let mut client_interface =
            Interface::new(client_config, &mut client_device, Instant::from_millis(0));
        client_interface.update_ip_addrs(|addresses| {
            addresses.clear();
            addresses
                .push(IpCidr::new(Ipv4Address::new(10, 0, 2, 16).into(), 24))
                .unwrap();
        });
        let mut client_rx = [0u8; 4096];
        let mut client_tx = [0u8; 4096];
        let mut replacement_rx = [0u8; 4096];
        let mut replacement_tx = [0u8; 4096];
        let mut client_storage = [SocketStorage::EMPTY; 2];
        let mut client_sockets = SocketSet::new(&mut client_storage[..]);
        let client_handle = client_sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut client_rx[..]),
            TcpSocketBuffer::new(&mut client_tx[..]),
        ));
        client_sockets
            .get_mut::<TcpSocket>(client_handle)
            .connect(
                client_interface.context(),
                (Ipv4Address::new(10, 0, 2, 15), descriptor().listener_port),
                49_152,
            )
            .unwrap();

        let mut now_ms = 1u64;
        let mut auth_sent = false;
        let mut auth_response = std::vec::Vec::new();
        let mut events = std::vec::Vec::new();
        let expected_auth = framed(b"OK AUTH");
        let mut authenticated_and_drained = false;
        for _ in 0..512 {
            drive_network_turn(
                &mut service,
                &mut client_interface,
                &mut client_device,
                &mut client_sockets,
                now_ms,
            )
            .unwrap();
            while let Some(event) = service.pop_event() {
                events.push(event);
            }

            let client = client_sockets.get_mut::<TcpSocket>(client_handle);
            if !auth_sent && client.state() == TcpState::Established {
                let auth = framed(b"AUTH secret");
                assert_eq!(client.send_slice(auth.as_slice()).unwrap(), auth.len());
                auth_sent = true;
            }
            while client.can_recv() {
                let mut chunk = [0u8; 64];
                let length = client.recv_slice(&mut chunk).unwrap();
                if length == 0 {
                    break;
                }
                auth_response.extend_from_slice(&chunk[..length]);
            }

            let server = service.sockets.get::<TcpSocket>(service.tcp_handle);
            if auth_sent
                && auth_response == expected_auth
                && service.session.authenticated()
                && service.session.output_queue_empty()
                && server.state() == TcpState::Established
                && server.send_queue() == 0
            {
                authenticated_and_drained = true;
                break;
            }
            now_ms = now_ms.saturating_add(1);
        }
        assert!(
            authenticated_and_drained,
            "the in-memory peer must authenticate and acknowledge OK AUTH"
        );
        assert_eq!(
            events
                .iter()
                .map(ServiceEvent::kind)
                .collect::<std::vec::Vec<_>>(),
            [ExchangeKind::Connected, ExchangeKind::Authenticated]
        );

        service
            .sockets
            .get_mut::<TcpSocket>(service.tcp_handle)
            .close();
        assert_eq!(
            service.sockets.get::<TcpSocket>(service.tcp_handle).state(),
            TcpState::FinWait1
        );

        let mut reached_fin_wait_2 = false;
        for _ in 0..64 {
            now_ms = now_ms.saturating_add(1);
            drive_network_turn(
                &mut service,
                &mut client_interface,
                &mut client_device,
                &mut client_sockets,
                now_ms,
            )
            .unwrap();
            while let Some(event) = service.pop_event() {
                events.push(event);
            }
            if service.sockets.get::<TcpSocket>(service.tcp_handle).state() == TcpState::FinWait2 {
                reached_fin_wait_2 = true;
                break;
            }
        }
        assert!(reached_fin_wait_2, "server never reached FIN-WAIT-2");
        assert!(!service
            .sockets
            .get::<TcpSocket>(service.tcp_handle)
            .can_send());

        service
            .apply_control(1, ExchangeKind::SendLine, b"late-output")
            .unwrap();
        assert!(!service.session.output_queue_empty());
        now_ms = now_ms.saturating_add(1);
        assert_eq!(
            poll_complete_service_cycle(&mut service, now_ms).unwrap(),
            0,
            "pending output without TCP capacity must not retain PollService"
        );
        assert_eq!(
            service.sockets.get::<TcpSocket>(service.tcp_handle).state(),
            TcpState::FinWait2
        );
        assert!(
            !service.session.output_queue_empty(),
            "unsendable output must remain retained until connection teardown"
        );

        let client = client_sockets.get_mut::<TcpSocket>(client_handle);
        assert_eq!(client.state(), TcpState::CloseWait);
        client.close();
        assert_eq!(client.state(), TcpState::LastAck);

        let replacement_handle = client_sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut replacement_rx[..]),
            TcpSocketBuffer::new(&mut replacement_tx[..]),
        ));
        client_sockets
            .get_mut::<TcpSocket>(replacement_handle)
            .connect(
                client_interface.context(),
                (Ipv4Address::new(10, 0, 2, 15), descriptor().listener_port),
                49_153,
            )
            .unwrap();

        let mut relistened = false;
        for _ in 0..64 {
            now_ms = now_ms.saturating_add(1);
            drive_network_turn(
                &mut service,
                &mut client_interface,
                &mut client_device,
                &mut client_sockets,
                now_ms,
            )
            .unwrap();
            while let Some(event) = service.pop_event() {
                events.push(event);
            }
            if service.listener_ready()
                && service.session.connection_id().is_none()
                && service.session.output_queue_empty()
            {
                relistened = true;
                break;
            }
        }
        assert!(relistened, "closed connection did not return to LISTEN");
        assert_eq!(
            events
                .iter()
                .map(ServiceEvent::kind)
                .collect::<std::vec::Vec<_>>(),
            [
                ExchangeKind::Connected,
                ExchangeKind::Authenticated,
                ExchangeKind::Disconnected,
            ]
        );
        assert!(events.iter().all(|event| event.connection_id() == 1));
        assert_eq!(events[2].payload().unwrap(), "reason=closed");

        let mut replacement_auth_sent = false;
        let mut replacement_response = std::vec::Vec::new();
        for _ in 0..512 {
            now_ms = now_ms.saturating_add(1);
            drive_network_turn(
                &mut service,
                &mut client_interface,
                &mut client_device,
                &mut client_sockets,
                now_ms,
            )
            .unwrap();
            while let Some(event) = service.pop_event() {
                events.push(event);
            }

            let replacement = client_sockets.get_mut::<TcpSocket>(replacement_handle);
            if !replacement_auth_sent && replacement.state() == TcpState::Established {
                let auth = framed(b"AUTH secret");
                assert_eq!(replacement.send_slice(auth.as_slice()).unwrap(), auth.len());
                replacement_auth_sent = true;
            }
            while replacement.can_recv() {
                let mut chunk = [0u8; 64];
                let length = replacement.recv_slice(&mut chunk).unwrap();
                if length == 0 {
                    break;
                }
                replacement_response.extend_from_slice(&chunk[..length]);
            }
            if replacement_response == expected_auth {
                break;
            }
        }
        assert_eq!(replacement_response, expected_auth);
        assert_eq!(
            events
                .iter()
                .map(ServiceEvent::kind)
                .collect::<std::vec::Vec<_>>(),
            [
                ExchangeKind::Connected,
                ExchangeKind::Authenticated,
                ExchangeKind::Disconnected,
                ExchangeKind::Connected,
                ExchangeKind::Authenticated,
            ]
        );
        assert_eq!(events[3].connection_id(), 2);
        assert_eq!(events[4].connection_id(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind() == ExchangeKind::Disconnected)
                .count(),
            1,
            "disconnect must publish once"
        );
    }

    #[test]
    fn peer_initiated_close_relistens_without_root_disconnect_control() {
        let mut server_rx = [0u8; 4096];
        let mut server_tx = [0u8; 4096];
        let mut server_storage = [SocketStorage::EMPTY];
        let mut service = ConsoleNetworkService::new(
            descriptor(),
            &mut server_rx,
            &mut server_tx,
            &mut server_storage,
        )
        .unwrap();

        let mut client_device = SharedFrameDevice::new();
        let mut client_config = InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress([
            2, 0, 0, 0, 0, 2,
        ])));
        client_config.random_seed = 11;
        let mut client_interface =
            Interface::new(client_config, &mut client_device, Instant::from_millis(0));
        client_interface.update_ip_addrs(|addresses| {
            addresses.clear();
            addresses
                .push(IpCidr::new(Ipv4Address::new(10, 0, 2, 16).into(), 24))
                .unwrap();
        });
        let mut client_rx = [0u8; 4096];
        let mut client_tx = [0u8; 4096];
        let mut client_storage = [SocketStorage::EMPTY];
        let mut client_sockets = SocketSet::new(&mut client_storage[..]);
        let client_handle = client_sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut client_rx[..]),
            TcpSocketBuffer::new(&mut client_tx[..]),
        ));
        client_sockets
            .get_mut::<TcpSocket>(client_handle)
            .connect(
                client_interface.context(),
                (Ipv4Address::new(10, 0, 2, 15), descriptor().listener_port),
                49_152,
            )
            .unwrap();

        let mut now_ms = 1u64;
        let mut auth_sent = false;
        let mut auth_response = std::vec::Vec::new();
        let expected_auth = framed(b"OK AUTH");
        let mut events = std::vec::Vec::new();
        for _ in 0..512 {
            drive_network_turn(
                &mut service,
                &mut client_interface,
                &mut client_device,
                &mut client_sockets,
                now_ms,
            )
            .unwrap();
            while let Some(event) = service.pop_event() {
                events.push(event);
            }

            let client = client_sockets.get_mut::<TcpSocket>(client_handle);
            if !auth_sent && client.state() == TcpState::Established {
                let auth = framed(b"AUTH secret");
                assert_eq!(client.send_slice(auth.as_slice()).unwrap(), auth.len());
                auth_sent = true;
            }
            while client.can_recv() {
                let mut chunk = [0u8; 64];
                let length = client.recv_slice(&mut chunk).unwrap();
                if length == 0 {
                    break;
                }
                auth_response.extend_from_slice(&chunk[..length]);
            }
            if auth_response == expected_auth
                && service.session.authenticated()
                && service.session.output_queue_empty()
                && service
                    .sockets
                    .get::<TcpSocket>(service.tcp_handle)
                    .send_queue()
                    == 0
            {
                break;
            }
            now_ms = now_ms.saturating_add(1);
        }
        assert_eq!(auth_response, expected_auth);
        assert_eq!(
            events
                .iter()
                .map(ServiceEvent::kind)
                .collect::<std::vec::Vec<_>>(),
            [ExchangeKind::Connected, ExchangeKind::Authenticated]
        );

        let client = client_sockets.get_mut::<TcpSocket>(client_handle);
        assert_eq!(client.state(), TcpState::Established);
        client.close();
        assert_eq!(client.state(), TcpState::FinWait1);

        let mut relistened = false;
        for _ in 0..128 {
            now_ms = now_ms.saturating_add(1);
            drive_network_turn(
                &mut service,
                &mut client_interface,
                &mut client_device,
                &mut client_sockets,
                now_ms,
            )
            .unwrap();
            while let Some(event) = service.pop_event() {
                events.push(event);
            }
            if service.listener_ready() && service.session.connection_id().is_none() {
                relistened = true;
                break;
            }
        }

        assert!(
            relistened,
            "a peer FIN must complete the server close and restore LISTEN"
        );
        assert_eq!(
            events
                .iter()
                .map(ServiceEvent::kind)
                .collect::<std::vec::Vec<_>>(),
            [
                ExchangeKind::Connected,
                ExchangeKind::Authenticated,
                ExchangeKind::Disconnected,
            ]
        );
        assert!(events.iter().all(|event| event.connection_id() == 1));
        assert_eq!(events[2].payload().unwrap(), "reason=closed");
    }

    #[test]
    fn operational_syn_auth_ok_advances_one_child_unit_per_turn() {
        let mut server_rx = [0u8; 4096];
        let mut server_tx = [0u8; 4096];
        let mut server_storage = [SocketStorage::EMPTY];
        let mut service = ConsoleNetworkService::new(
            descriptor(),
            &mut server_rx,
            &mut server_tx,
            &mut server_storage,
        )
        .unwrap();

        let mut client_device = SharedFrameDevice::new();
        let mut client_config = InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress([
            2, 0, 0, 0, 0, 2,
        ])));
        client_config.random_seed = 9;
        let mut client_interface =
            Interface::new(client_config, &mut client_device, Instant::from_millis(0));
        client_interface.update_ip_addrs(|addresses| {
            addresses.clear();
            addresses
                .push(IpCidr::new(Ipv4Address::new(10, 0, 2, 16).into(), 24))
                .unwrap();
        });
        let mut client_rx = [0u8; 4096];
        let mut client_tx = [0u8; 4096];
        let mut client_storage = [SocketStorage::EMPTY];
        let mut client_sockets = SocketSet::new(&mut client_storage[..]);
        let client_socket = TcpSocket::new(
            TcpSocketBuffer::new(&mut client_rx[..]),
            TcpSocketBuffer::new(&mut client_tx[..]),
        );
        let client_handle = client_sockets.add(client_socket);
        client_sockets
            .get_mut::<TcpSocket>(client_handle)
            .connect(
                client_interface.context(),
                (Ipv4Address::new(10, 0, 2, 15), descriptor().listener_port),
                49_152,
            )
            .unwrap();

        let mut scheduler = ChildTurnScheduler::new();
        let mut staged_packet: Option<std::vec::Vec<u8>> = None;
        let mut packet_signal = false;
        let mut packet_inflight = false;
        let mut units = std::vec::Vec::new();
        let mut poll_outcomes = std::vec::Vec::new();
        let mut event_creation_turns = std::vec::Vec::new();
        let mut published_events = std::vec::Vec::new();
        let mut published_egress_turns = std::vec::Vec::new();
        let mut auth_sent = false;
        let mut received = std::vec::Vec::new();
        let expected = framed(b"OK AUTH");
        let mut response_observed_turn = None;
        let mut local_poll_iterations = 0usize;
        let mut publication_credit_available = false;
        let mut credited_publication_polls = 0usize;
        let mut blocked_host_ticks = 0usize;
        let mut root_observed_publications = 0usize;
        let mut pending_publication_ack = true; // Root observed initial Ready.

        const ROOT_LOWER_UNITS_PER_SERVICE_TICK: u64 = 5;
        for turn in 1u64..=512 {
            let timestamp = Instant::from_millis(turn as i64);
            let _ = client_interface.poll(timestamp, &mut client_device, &mut client_sockets);

            if !packet_inflight {
                let mut packet = [0u8; ETHERNET_FRAME_BYTES];
                if let Some(length) = client_device.pop_egress(&mut packet).unwrap() {
                    staged_packet = Some(packet[..length].to_vec());
                    packet_signal = true;
                    packet_inflight = true;
                }
            }

            let this_packet_signal = core::mem::take(&mut packet_signal);
            let readiness_before_gate = ChildTurnReadiness::new(
                false,
                service.service_event_pending(),
                service.egress_pending(),
            );
            let local_poll_eligible =
                scheduler.local_poll_eligible(publication_credit_available, readiness_before_gate);
            // Model the root's exact five-unit lower Network cursor, whose one
            // ServiceTick is independent of whether the child happens to
            // block. V9's test fabricated a control signal on every blocking
            // boundary and masked the missing publication-credit transition.
            let control_signal = turn % ROOT_LOWER_UNITS_PER_SERVICE_TICK == 0;
            let publication_ack = core::mem::take(&mut pending_publication_ack);
            if local_poll_eligible && !this_packet_signal {
                local_poll_iterations = local_poll_iterations.saturating_add(1);
            }
            let boundary_returned =
                local_poll_eligible || this_packet_signal || control_signal || publication_ack;
            if boundary_returned {
                if publication_ack {
                    assert!(
                        !publication_credit_available,
                        "root ACKs at most one outstanding page"
                    );
                    publication_credit_available = true;
                }
                scheduler.retain_notification(this_packet_signal, control_signal);
                let readiness = ChildTurnReadiness::new(
                    false,
                    service.service_event_pending(),
                    service.egress_pending(),
                );
                let unit = scheduler.take_next(readiness);
                units.push(unit);
                match unit {
                    ChildTurnUnit::PublishCompletion => {
                        panic!("input completion watermarks never consume the event slot")
                    }
                    ChildTurnUnit::PublishServiceEvent => {
                        assert!(publication_credit_available);
                        credited_publication_polls += usize::from(local_poll_eligible);
                        let event = service.pop_event().unwrap();
                        published_events.push((turn, event.kind()));
                        publication_credit_available = false;
                        root_observed_publications = root_observed_publications.saturating_add(1);
                        pending_publication_ack = true;
                    }
                    ChildTurnUnit::PublishEgress => {
                        assert!(publication_credit_available);
                        credited_publication_polls += usize::from(local_poll_eligible);
                        let mut packet = [0u8; ETHERNET_FRAME_BYTES];
                        let length = service.take_packet(&mut packet).unwrap().unwrap();
                        client_device.push_ingress(&packet[..length]).unwrap();
                        published_egress_turns.push(turn);
                        publication_credit_available = false;
                        root_observed_publications = root_observed_publications.saturating_add(1);
                        pending_publication_ack = true;
                    }
                    ChildTurnUnit::PollService => {
                        let events_before = service.session.events.len();
                        let poll_unit = service.poll_unit;
                        let outcome = service.poll_service_unit(turn).unwrap();
                        poll_outcomes.push((turn, poll_unit, outcome));
                        if service.session.events.len() > events_before {
                            event_creation_turns.push((turn, poll_unit, outcome));
                        }
                        if outcome == ServicePollOutcome::Complete {
                            scheduler.complete(ChildTurnUnit::PollService);
                        }
                    }
                    ChildTurnUnit::IngestPacket => {
                        let packet = staged_packet.take().unwrap();
                        service.ingest_packet(packet.as_slice()).unwrap();
                        // The child publishes this exact sequence in the
                        // event-page trailer. It neither consumes publication
                        // credit nor waits for a semantic event ACK.
                        packet_inflight = false;
                        scheduler.complete(ChildTurnUnit::IngestPacket);
                        scheduler.request_service();
                    }
                    ChildTurnUnit::ApplyControl => {
                        // This is the existing root service tick: its page
                        // carries no new control record, so retire the empty
                        // hint and drive a distinct service-poll cycle.
                        scheduler.complete(ChildTurnUnit::ApplyControl);
                        scheduler.request_service();
                    }
                    ChildTurnUnit::Idle => {}
                }
            } else {
                blocked_host_ticks = blocked_host_ticks.saturating_add(1);
            }

            let _ = client_interface.poll(timestamp, &mut client_device, &mut client_sockets);
            let socket = client_sockets.get_mut::<TcpSocket>(client_handle);
            if !auth_sent && socket.state() == TcpState::Established {
                let auth = framed(b"AUTH secret");
                assert_eq!(socket.send_slice(auth.as_slice()).unwrap(), auth.len());
                auth_sent = true;
            }
            while socket.can_recv() {
                let mut chunk = [0u8; 64];
                let length = socket.recv_slice(&mut chunk).unwrap();
                if length == 0 {
                    break;
                }
                received.extend_from_slice(&chunk[..length]);
            }
            if received.len() >= expected.len() && response_observed_turn.is_none() {
                response_observed_turn = Some(turn);
            }
            if response_observed_turn.is_some_and(|observed| {
                turn > observed
                    && poll_outcomes.last().is_some_and(|(poll_turn, _, outcome)| {
                        *poll_turn == turn && *outcome == ServicePollOutcome::Complete
                    })
            }) && blocked_host_ticks > 0
            {
                break;
            }
        }

        assert!(auth_sent, "client never completed the SYN handshake");
        assert_eq!(received.as_slice(), expected.as_slice());
        let connected_turn = published_events
            .iter()
            .find_map(|(turn, kind)| (*kind == ExchangeKind::Connected).then_some(*turn))
            .expect("Connected must be published");
        let authenticated_turn = published_events
            .iter()
            .find_map(|(turn, kind)| (*kind == ExchangeKind::Authenticated).then_some(*turn))
            .expect("Authenticated must be published");
        let ok_egress_turn = *published_egress_turns.last().expect("OK AUTH needs egress");
        assert!(connected_turn < authenticated_turn);
        assert!(authenticated_turn < ok_egress_turn);
        assert!(units.contains(&ChildTurnUnit::IngestPacket));
        assert!(units.contains(&ChildTurnUnit::PollService));
        assert!(!units.contains(&ChildTurnUnit::PublishCompletion));
        assert!(units.contains(&ChildTurnUnit::PublishServiceEvent));
        assert!(units.contains(&ChildTurnUnit::PublishEgress));
        assert!(poll_outcomes.len() >= 3);
        for cycle in poll_outcomes.chunks_exact(3) {
            assert_eq!(
                cycle[0].1,
                ServicePollUnit::StackIngress,
                "each service cycle must start with one bounded ingress attempt"
            );
            assert_eq!(cycle[0].2, ServicePollOutcome::Continuation);
            assert_eq!(cycle[1].1, ServicePollUnit::StackEgress);
            assert_eq!(cycle[1].2, ServicePollOutcome::Continuation);
            assert_eq!(cycle[2].1, ServicePollUnit::Session);
            assert!(cycle[0].0 < cycle[1].0 && cycle[1].0 < cycle[2].0);
        }
        assert_eq!(poll_outcomes.len() % 3, 0);
        let session_outcomes = poll_outcomes
            .chunks_exact(3)
            .map(|cycle| cycle[2].2)
            .collect::<std::vec::Vec<_>>();
        assert_eq!(
            session_outcomes
                .iter()
                .filter(|outcome| **outcome == ServicePollOutcome::Continuation)
                .count(),
            1,
            "the one committed OK AUTH frame retains exactly one service cycle"
        );
        let committed_cycle = session_outcomes
            .iter()
            .position(|outcome| *outcome == ServicePollOutcome::Continuation)
            .unwrap();
        assert_eq!(
            session_outcomes.get(committed_cycle + 1),
            Some(&ServicePollOutcome::Complete),
            "the follow-up cycle must quiesce after its final egress attempt"
        );
        assert!(event_creation_turns.len() >= 2);
        assert!(event_creation_turns
            .iter()
            .all(|(_, unit, _)| *unit == ServicePollUnit::Session));
        assert!(event_creation_turns
            .iter()
            .any(|(_, _, outcome)| *outcome == ServicePollOutcome::Continuation));
        assert!(published_events
            .iter()
            .all(|(turn, _)| !published_egress_turns.contains(turn)));
        assert!(
            local_poll_iterations >= 3,
            "retained service work must advance without a fresh root notification"
        );
        assert!(
            credited_publication_polls > 0,
            "a retained publication must consume explicit Observe->ACK credit"
        );
        assert!(root_observed_publications >= credited_publication_polls);
        assert!(
            blocked_host_ticks > 0,
            "idle Wait must remain genuinely blocking"
        );
    }
}
