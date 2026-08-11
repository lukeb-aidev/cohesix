// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide bounded transport and namespace-service ABI primitives for Secure9P users.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![no_std]

//! Bounded Secure9P transport adapters shared across Cohesix host and VM components.
//!
//! The types in this crate deliberately contain no sockets or target-specific
//! authority.  Host streams and seL4 shared-frame endpoints use the same frame,
//! queue, cancellation, and namespace preparation state machines.

#[cfg(test)]
extern crate std;

use core::fmt;
use core::str;

use heapless::Deque;

/// Internal namespace-service ABI version implemented by Milestone 26e.
pub const NAMESPACE_SERVICE_ABI_VERSION: u16 = 1;
/// seL4 IPC label for one namespace preparation request.
pub const NAMESPACE_REQUEST_LABEL: u64 = 0x004e_4401;
/// seL4 IPC label for one successfully prepared response.
pub const NAMESPACE_PREPARED_LABEL: u64 = 0x004e_4402;
/// seL4 IPC label for one typed rejected response.
pub const NAMESPACE_REJECTED_LABEL: u64 = 0x004e_4403;
/// Maximum path bytes carried through the namespace-service boundary.
pub const NAMESPACE_PATH_MAX: usize = 256;
/// Maximum control payload bytes carried through the namespace-service boundary.
pub const NAMESPACE_PAYLOAD_MAX: usize = 4096;
/// Maximum number of path components admitted by Secure9P.
pub const NAMESPACE_COMPONENT_MAX: usize = 8;
/// Encoded size of a namespace request or response header.
pub const NAMESPACE_HEADER_BYTES: usize = 32;
/// Maximum encoded namespace request or response bytes.
pub const NAMESPACE_OPERATION_FRAME_BYTES: usize =
    NAMESPACE_HEADER_BYTES + NAMESPACE_PATH_MAX + NAMESPACE_PAYLOAD_MAX;
/// Size of each request and response mapping in the isolated service ABI.
/// Two pages leave the maximum typed operation bounded without coupling the
/// ABI to a variable mapping size.
pub const NAMESPACE_SHARED_FRAME_BYTES: usize = 8192;
/// Runtime-init descriptor version for the isolated namespace child.
pub const NAMESPACE_RUNTIME_INIT_VERSION: u16 = 1;
/// Fixed pointer-free namespace runtime-init descriptor size.
pub const NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES: usize = 72;
/// Fixed child CSpace slot holding the service receive endpoint.
pub const NAMESPACE_SERVICE_ENDPOINT_SLOT: u64 = 2;
/// Fixed child CSpace slot holding its single-owner MCS Reply object.
pub const NAMESPACE_SERVICE_REPLY_SLOT: u64 = 3;
/// Canonical seL4 capability-rights word for a read-only cap.
pub const SEL4_RIGHTS_READ: u8 = 0b0010;
/// Canonical seL4 capability-rights word for a write-only cap.
pub const SEL4_RIGHTS_WRITE: u8 = 0b0001;
/// Canonical seL4 capability-rights word for a read-write cap.
pub const SEL4_RIGHTS_READ_WRITE: u8 = SEL4_RIGHTS_READ | SEL4_RIGHTS_WRITE;
/// Exact root service-call cap rights: Write + GrantReply, with no Grant or Read.
pub const NAMESPACE_ROOT_CALL_RIGHTS: u8 = 0b1001;
/// Exact child service-receive cap rights: Read only.
pub const NAMESPACE_CHILD_RECEIVE_RIGHTS: u8 = SEL4_RIGHTS_READ;
/// Minimum encoded 9P frame size (size, type, and tag).
pub const MIN_9P_FRAME_BYTES: usize = 7;
/// Maximum Secure9P message size mandated by the protocol contract.
pub const SECURE9P_MAX_MSIZE: usize = 8192;
/// Bounded encoded response size accepted by the compatibility host writer.
/// The current codec interprets a full-size read count before adding its fixed
/// response envelope; Milestone 26e preserves that fixture behavior.
pub const COMPAT_ENCODED_RESPONSE_MAX: usize = SECURE9P_MAX_MSIZE + 64;

/// State shared by bounded request and response transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    /// New work is accepted.
    Open,
    /// No new work is accepted, but already queued work may drain.
    Closing,
    /// The transport is closed and contains no queued work.
    Closed,
    /// The backing authority was revoked and no further observation is valid.
    Revoked,
}

/// Errors returned by bounded transport and namespace-service operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// A configured bound was zero or exceeded the protocol maximum.
    InvalidLimits,
    /// A frame declared fewer than the minimum 9P header bytes.
    InvalidFrameLength,
    /// A frame exceeded the configured bound.
    FrameTooLarge,
    /// The peer closed while a partial frame was buffered.
    PartialFrame,
    /// The bounded queue has no free slot.
    QueueFull,
    /// New work was attempted after close began.
    Closed,
    /// The transport generation was revoked.
    Revoked,
    /// A token did not identify queued work in the current generation.
    UnknownRequest,
    /// A supplied sequence or generation was zero or stale.
    StaleIdentity,
    /// A destination buffer was too small.
    BufferTooSmall,
    /// A namespace request used an unknown operation or invalid flags.
    InvalidOperation,
    /// A namespace path was malformed or exceeded its component bound.
    InvalidPath,
    /// A namespace payload was malformed or exceeded its bound.
    InvalidPayload,
    /// The namespace request used an unsupported ABI version or layout.
    InvalidAbi,
    /// The configured short-write retry budget was exhausted.
    ShortWriteExhausted,
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLimits => "invalid transport limits",
            Self::InvalidFrameLength => "invalid 9P frame length",
            Self::FrameTooLarge => "9P frame exceeds transport bound",
            Self::PartialFrame => "transport closed with a partial frame",
            Self::QueueFull => "transport queue full",
            Self::Closed => "transport closed",
            Self::Revoked => "transport revoked",
            Self::UnknownRequest => "unknown transport request",
            Self::StaleIdentity => "stale transport identity",
            Self::BufferTooSmall => "transport buffer too small",
            Self::InvalidOperation => "invalid namespace operation",
            Self::InvalidPath => "invalid namespace path",
            Self::InvalidPayload => "invalid namespace payload",
            Self::InvalidAbi => "invalid namespace-service ABI",
            Self::ShortWriteExhausted => "short-write retry budget exhausted",
        })
    }
}

impl TransportError {
    /// Encode a stable nonzero error value for the internal seL4 reply.
    #[must_use]
    pub const fn wire_code(self) -> u64 {
        match self {
            Self::InvalidLimits => 1,
            Self::InvalidFrameLength => 2,
            Self::FrameTooLarge => 3,
            Self::PartialFrame => 4,
            Self::QueueFull => 5,
            Self::Closed => 6,
            Self::Revoked => 7,
            Self::UnknownRequest => 8,
            Self::StaleIdentity => 9,
            Self::BufferTooSmall => 10,
            Self::InvalidOperation => 11,
            Self::InvalidPath => 12,
            Self::InvalidPayload => 13,
            Self::InvalidAbi => 14,
            Self::ShortWriteExhausted => 15,
        }
    }

    /// Decode one internal seL4 reply error value.
    pub const fn from_wire_code(code: u64) -> Result<Self, Self> {
        match code {
            1 => Ok(Self::InvalidLimits),
            2 => Ok(Self::InvalidFrameLength),
            3 => Ok(Self::FrameTooLarge),
            4 => Ok(Self::PartialFrame),
            5 => Ok(Self::QueueFull),
            6 => Ok(Self::Closed),
            7 => Ok(Self::Revoked),
            8 => Ok(Self::UnknownRequest),
            9 => Ok(Self::StaleIdentity),
            10 => Ok(Self::BufferTooSmall),
            11 => Ok(Self::InvalidOperation),
            12 => Ok(Self::InvalidPath),
            13 => Ok(Self::InvalidPayload),
            14 => Ok(Self::InvalidAbi),
            15 => Ok(Self::ShortWriteExhausted),
            _ => Err(Self::InvalidAbi),
        }
    }
}

/// Sealed, pointer-free runtime-init descriptor supplied to the isolated
/// namespace child. The addresses name mappings in the child VSpace; they are
/// not pointers in any message or shared-frame record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct NamespaceRuntimeInitDescriptor {
    /// Descriptor version.
    pub version: u16,
    /// Descriptor size for layout validation.
    pub descriptor_bytes: u16,
    /// Rights installed on the child's receive endpoint cap.
    pub endpoint_cap_rights: u8,
    /// Rights installed on the child's request-frame mapping.
    pub request_frame_rights: u8,
    /// Rights installed on the child's response-frame mapping.
    pub response_frame_rights: u8,
    /// Reserved rights/layout byte; must be zero.
    pub reserved_rights: u8,
    /// Exact supervisor generation admitted for this child.
    pub generation: u64,
    /// Request shared-frame virtual address in the child VSpace.
    pub request_frame_vaddr: u64,
    /// Response shared-frame virtual address in the child VSpace.
    pub response_frame_vaddr: u64,
    /// Size of each mapped frame window.
    pub frame_bytes: u32,
    /// Expected badge on root calls.
    pub request_badge: u32,
    /// CSpace slot containing the service endpoint.
    pub endpoint_cptr: u64,
    /// CSpace slot containing the single-owner MCS Reply object.
    pub reply_cptr: u64,
    /// Reserved for a future ABI; must be zero.
    pub reserved: [u64; 2],
}

const _: [(); NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES] =
    [(); core::mem::size_of::<NamespaceRuntimeInitDescriptor>()];

impl NamespaceRuntimeInitDescriptor {
    /// Validate exact layout, generation, mappings, cap rights, badge, and
    /// fixed child CSpace slots.
    #[must_use]
    pub fn valid(self) -> bool {
        self.version == NAMESPACE_RUNTIME_INIT_VERSION
            && self.descriptor_bytes as usize == NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES
            && self.generation != 0
            && self.request_frame_vaddr != 0
            && self.response_frame_vaddr != 0
            && self.request_frame_vaddr != self.response_frame_vaddr
            && self.request_frame_vaddr as usize & 4095 == 0
            && self.response_frame_vaddr as usize & 4095 == 0
            && self.frame_bytes as usize == NAMESPACE_SHARED_FRAME_BYTES
            && self.request_badge != 0
            && self.endpoint_cap_rights == NAMESPACE_CHILD_RECEIVE_RIGHTS
            && self.request_frame_rights == SEL4_RIGHTS_READ
            && self.response_frame_rights == SEL4_RIGHTS_READ_WRITE
            && self.reserved_rights == 0
            && self.endpoint_cptr == NAMESPACE_SERVICE_ENDPOINT_SLOT
            && self.reply_cptr == NAMESPACE_SERVICE_REPLY_SLOT
            && self.reserved[0] == 0
            && self.reserved[1] == 0
    }

    /// Encode the fixed descriptor in native target layout without exposing a
    /// pointer or relying on an unbounded serializer.
    pub fn encode(self, output: &mut [u8]) -> Result<(), TransportError> {
        if !self.valid() || output.len() < NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES {
            return Err(TransportError::InvalidAbi);
        }
        let output = &mut output[..NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES];
        output.fill(0);
        output[0..2].copy_from_slice(&self.version.to_le_bytes());
        output[2..4].copy_from_slice(&self.descriptor_bytes.to_le_bytes());
        output[4] = self.endpoint_cap_rights;
        output[5] = self.request_frame_rights;
        output[6] = self.response_frame_rights;
        output[7] = self.reserved_rights;
        output[8..16].copy_from_slice(&self.generation.to_le_bytes());
        output[16..24].copy_from_slice(&self.request_frame_vaddr.to_le_bytes());
        output[24..32].copy_from_slice(&self.response_frame_vaddr.to_le_bytes());
        output[32..36].copy_from_slice(&self.frame_bytes.to_le_bytes());
        output[36..40].copy_from_slice(&self.request_badge.to_le_bytes());
        output[40..48].copy_from_slice(&self.endpoint_cptr.to_le_bytes());
        output[48..56].copy_from_slice(&self.reply_cptr.to_le_bytes());
        output[56..64].copy_from_slice(&self.reserved[0].to_le_bytes());
        output[64..72].copy_from_slice(&self.reserved[1].to_le_bytes());
        Ok(())
    }
}

/// Bounds applied to a stream-frame accumulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimits {
    /// Maximum accepted wire-frame bytes.
    pub max_frame_bytes: usize,
}

impl FrameLimits {
    /// Validate and construct frame limits.
    pub const fn new(max_frame_bytes: usize) -> Result<Self, TransportError> {
        if max_frame_bytes < MIN_9P_FRAME_BYTES || max_frame_bytes > SECURE9P_MAX_MSIZE {
            return Err(TransportError::InvalidLimits);
        }
        Ok(Self { max_frame_bytes })
    }
}

/// Result of ingesting one stream fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameProgress {
    /// More bytes are required before a complete frame is available.
    Pending {
        /// Bytes consumed from the supplied fragment.
        consumed: usize,
        /// Additional bytes currently required.
        needed: usize,
    },
    /// A complete frame is ready to be copied and released.
    Complete {
        /// Bytes consumed from the supplied fragment. Any remainder belongs to
        /// the next frame and must be submitted after [`FrameAccumulator::take`].
        consumed: usize,
        /// Complete frame length.
        frame_len: usize,
    },
}

/// Fixed-capacity partial-frame accumulator for byte-stream adapters.
#[derive(Debug)]
pub struct FrameAccumulator<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    len: usize,
    expected: Option<usize>,
    limits: FrameLimits,
    state: TransportState,
}

impl<const CAPACITY: usize> FrameAccumulator<CAPACITY> {
    /// Construct an empty accumulator.
    pub const fn new(limits: FrameLimits) -> Result<Self, TransportError> {
        if limits.max_frame_bytes > CAPACITY {
            return Err(TransportError::InvalidLimits);
        }
        Ok(Self {
            bytes: [0; CAPACITY],
            len: 0,
            expected: None,
            limits,
            state: TransportState::Open,
        })
    }

    /// Return the current transport state.
    #[must_use]
    pub const fn state(&self) -> TransportState {
        self.state
    }

    /// Return the number of buffered bytes.
    #[must_use]
    pub const fn buffered_len(&self) -> usize {
        self.len
    }

    /// Ingest as much of one fragment as belongs to the current frame.
    pub fn push(&mut self, fragment: &[u8]) -> Result<FrameProgress, TransportError> {
        match self.state {
            TransportState::Open => {}
            TransportState::Closing | TransportState::Closed => return Err(TransportError::Closed),
            TransportState::Revoked => return Err(TransportError::Revoked),
        }
        if self.expected.is_some_and(|expected| self.len == expected) {
            return Err(TransportError::QueueFull);
        }

        let mut consumed = 0usize;
        while consumed < fragment.len() {
            let target = self.expected.unwrap_or(4);
            if self.len == target {
                if self.expected.is_none() {
                    let declared = u32::from_le_bytes([
                        self.bytes[0],
                        self.bytes[1],
                        self.bytes[2],
                        self.bytes[3],
                    ]) as usize;
                    if declared < MIN_9P_FRAME_BYTES {
                        self.clear_buffer();
                        return Err(TransportError::InvalidFrameLength);
                    }
                    if declared > self.limits.max_frame_bytes || declared > CAPACITY {
                        self.clear_buffer();
                        return Err(TransportError::FrameTooLarge);
                    }
                    self.expected = Some(declared);
                    if self.len == declared {
                        break;
                    }
                    continue;
                }
                break;
            }
            let remaining = target.saturating_sub(self.len);
            let available = fragment.len().saturating_sub(consumed);
            let copied = remaining.min(available);
            self.bytes[self.len..self.len + copied]
                .copy_from_slice(&fragment[consumed..consumed + copied]);
            self.len += copied;
            consumed += copied;
        }

        if self.expected.is_none() && self.len == 4 {
            let declared =
                u32::from_le_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]])
                    as usize;
            if declared < MIN_9P_FRAME_BYTES {
                self.clear_buffer();
                return Err(TransportError::InvalidFrameLength);
            }
            if declared > self.limits.max_frame_bytes || declared > CAPACITY {
                self.clear_buffer();
                return Err(TransportError::FrameTooLarge);
            }
            self.expected = Some(declared);
        }

        let expected = self.expected.unwrap_or(4);
        if self.len == expected && self.expected.is_some() {
            Ok(FrameProgress::Complete {
                consumed,
                frame_len: expected,
            })
        } else {
            Ok(FrameProgress::Pending {
                consumed,
                needed: expected.saturating_sub(self.len),
            })
        }
    }

    /// Copy the complete frame into `output` and admit the next frame.
    pub fn take(&mut self, output: &mut [u8]) -> Result<usize, TransportError> {
        let Some(expected) = self.expected else {
            return Err(TransportError::PartialFrame);
        };
        if self.len != expected {
            return Err(TransportError::PartialFrame);
        }
        if output.len() < expected {
            return Err(TransportError::BufferTooSmall);
        }
        output[..expected].copy_from_slice(&self.bytes[..expected]);
        self.clear_buffer();
        Ok(expected)
    }

    /// Stop accepting new fragments and report whether the stream ended cleanly.
    pub fn close(&mut self) -> Result<(), TransportError> {
        if self.state == TransportState::Revoked {
            return Err(TransportError::Revoked);
        }
        self.state = TransportState::Closed;
        if self.len != 0 {
            self.clear_buffer();
            return Err(TransportError::PartialFrame);
        }
        Ok(())
    }

    /// Revoke the backing authority and discard all buffered bytes.
    pub fn revoke(&mut self) {
        self.clear_buffer();
        self.state = TransportState::Revoked;
    }

    fn clear_buffer(&mut self) {
        self.bytes[..self.len].fill(0);
        self.len = 0;
        self.expected = None;
    }
}

/// Stable identity for a queued transport operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestToken {
    /// Monotonic sequence within one generation.
    pub sequence: u64,
    /// Authority generation that owns the request.
    pub generation: u64,
}

impl RequestToken {
    /// Validate and construct a request token.
    pub const fn new(sequence: u64, generation: u64) -> Result<Self, TransportError> {
        if sequence == 0 || generation == 0 {
            return Err(TransportError::StaleIdentity);
        }
        Ok(Self {
            sequence,
            generation,
        })
    }
}

/// One fixed-capacity queued frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedFrame<const FRAME_BYTES: usize> {
    token: RequestToken,
    len: usize,
    bytes: [u8; FRAME_BYTES],
    cancelled: bool,
}

impl<const FRAME_BYTES: usize> QueuedFrame<FRAME_BYTES> {
    /// Return the request token.
    #[must_use]
    pub const fn token(&self) -> RequestToken {
        self.token
    }

    /// Borrow the complete frame bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Return whether cancellation won before dequeue.
    #[must_use]
    pub const fn cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Bounded FIFO shared by host in-process and seL4 shared-frame adapters.
#[derive(Debug)]
pub struct BoundedFrameQueue<const DEPTH: usize, const FRAME_BYTES: usize> {
    generation: u64,
    last_sequence: u64,
    state: TransportState,
    queue: Deque<QueuedFrame<FRAME_BYTES>, DEPTH>,
}

impl<const DEPTH: usize, const FRAME_BYTES: usize> BoundedFrameQueue<DEPTH, FRAME_BYTES> {
    /// Construct an open queue for one nonzero authority generation.
    pub fn new(generation: u64) -> Result<Self, TransportError> {
        if generation == 0
            || DEPTH == 0
            || FRAME_BYTES < MIN_9P_FRAME_BYTES
            || FRAME_BYTES > SECURE9P_MAX_MSIZE
        {
            return Err(TransportError::InvalidLimits);
        }
        Ok(Self {
            generation,
            last_sequence: 0,
            state: TransportState::Open,
            queue: Deque::new(),
        })
    }

    /// Return the current transport state.
    #[must_use]
    pub const fn state(&self) -> TransportState {
        self.state
    }

    /// Return the number of queued requests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Return true when no requests are queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Submit one complete wire frame.
    pub fn submit(&mut self, token: RequestToken, frame: &[u8]) -> Result<(), TransportError> {
        match self.state {
            TransportState::Open => {}
            TransportState::Closing | TransportState::Closed => return Err(TransportError::Closed),
            TransportState::Revoked => return Err(TransportError::Revoked),
        }
        if token.generation != self.generation || token.sequence <= self.last_sequence {
            return Err(TransportError::StaleIdentity);
        }
        validate_complete_frame(frame, FRAME_BYTES)?;
        let mut bytes = [0u8; FRAME_BYTES];
        bytes[..frame.len()].copy_from_slice(frame);
        self.queue
            .push_back(QueuedFrame {
                token,
                len: frame.len(),
                bytes,
                cancelled: false,
            })
            .map_err(|_| TransportError::QueueFull)?;
        self.last_sequence = token.sequence;
        Ok(())
    }

    /// Mark queued work cancelled. Cancellation is durable and observed at dequeue.
    pub fn cancel(&mut self, token: RequestToken) -> Result<(), TransportError> {
        if self.state == TransportState::Revoked {
            return Err(TransportError::Revoked);
        }
        if token.generation != self.generation {
            return Err(TransportError::StaleIdentity);
        }
        let entry = self
            .queue
            .iter_mut()
            .find(|entry| entry.token == token)
            .ok_or(TransportError::UnknownRequest)?;
        entry.cancelled = true;
        Ok(())
    }

    /// Dequeue the oldest request or cancellation result.
    pub fn pop(&mut self) -> Option<QueuedFrame<FRAME_BYTES>> {
        let item = self.queue.pop_front();
        if item.is_none() && self.state == TransportState::Closing {
            self.state = TransportState::Closed;
        }
        item
    }

    /// Stop accepting new work while allowing already queued work to drain.
    pub fn close(&mut self) -> Result<(), TransportError> {
        if self.state == TransportState::Revoked {
            return Err(TransportError::Revoked);
        }
        self.state = if self.queue.is_empty() {
            TransportState::Closed
        } else {
            TransportState::Closing
        };
        Ok(())
    }

    /// Revoke this generation and discard queued bytes.
    pub fn revoke(&mut self) {
        while let Some(mut entry) = self.queue.pop_front() {
            entry.bytes[..entry.len].fill(0);
        }
        self.state = TransportState::Revoked;
    }
}

fn validate_complete_frame(frame: &[u8], bound: usize) -> Result<(), TransportError> {
    if frame.len() < MIN_9P_FRAME_BYTES {
        return Err(TransportError::InvalidFrameLength);
    }
    if frame.len() > bound || frame.len() > SECURE9P_MAX_MSIZE {
        return Err(TransportError::FrameTooLarge);
    }
    let declared = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if declared < MIN_9P_FRAME_BYTES {
        return Err(TransportError::InvalidFrameLength);
    }
    if declared != frame.len() {
        return Err(TransportError::PartialFrame);
    }
    Ok(())
}

/// Outcome of accounting for one stream write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteProgress {
    /// The complete response frame was written.
    Complete,
    /// More bytes should be written immediately.
    Continue,
    /// A bounded retry is permitted after the specified delay.
    RetryAfter {
        /// Delay before the next attempt.
        delay_ms: u64,
    },
}

/// Transport-neutral short-write handling policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteRetryPolicy {
    /// Reject the first short write.
    Reject,
    /// Permit the fixed three-attempt exponential retry schedule.
    Retry,
}

impl WriteRetryPolicy {
    const fn retry_delay_ms(self, attempt: u8) -> Option<u64> {
        match self {
            Self::Reject => None,
            Self::Retry if attempt < 3 => Some(5u64.saturating_mul(1u64 << attempt)),
            Self::Retry => None,
        }
    }
}

/// Transport-neutral short-write accounting state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialWrite {
    total: usize,
    offset: usize,
    retry_attempt: u8,
    short_writes: u64,
    retries: u64,
    policy: WriteRetryPolicy,
}

impl PartialWrite {
    /// Construct write accounting for one nonempty frame.
    pub const fn new(total: usize, policy: WriteRetryPolicy) -> Result<Self, TransportError> {
        if total == 0 || total > COMPAT_ENCODED_RESPONSE_MAX {
            return Err(TransportError::InvalidLimits);
        }
        Ok(Self {
            total,
            offset: 0,
            retry_attempt: 0,
            short_writes: 0,
            retries: 0,
            policy,
        })
    }

    /// Return the unwritten offset.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Return the observed short-write count.
    #[must_use]
    pub const fn short_writes(&self) -> u64 {
        self.short_writes
    }

    /// Return the bounded retry count.
    #[must_use]
    pub const fn retries(&self) -> u64 {
        self.retries
    }

    /// Account for the byte count returned by one underlying write.
    pub fn advance(&mut self, written: usize) -> Result<WriteProgress, TransportError> {
        let remaining = self.total.saturating_sub(self.offset);
        if written > remaining {
            return Err(TransportError::InvalidFrameLength);
        }
        self.offset = self.offset.saturating_add(written);
        if self.offset == self.total {
            return Ok(WriteProgress::Complete);
        }
        if written == remaining {
            return Ok(WriteProgress::Continue);
        }
        self.short_writes = self.short_writes.saturating_add(1);
        let Some(delay_ms) = self.policy.retry_delay_ms(self.retry_attempt) else {
            return Err(TransportError::ShortWriteExhausted);
        };
        self.retry_attempt = self.retry_attempt.saturating_add(1);
        self.retries = self.retries.saturating_add(1);
        Ok(WriteProgress::RetryAfter { delay_ms })
    }
}

/// Namespace operations admitted across the isolated parser boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum NamespaceOpcode {
    /// Establish or refresh one authenticated session projection.
    Attach = 1,
    /// Prepare a bounded log or telemetry tail.
    Tail = 2,
    /// Prepare a Queen Worker-spawn control operation.
    Spawn = 3,
    /// Prepare a Queen Worker-kill control operation.
    Kill = 4,
    /// Prepare one bounded append operation.
    Echo = 5,
    /// Prepare one bounded read operation.
    Cat = 6,
    /// Prepare one bounded directory projection.
    List = 7,
    /// Prepare a bounded log-stream request.
    Log = 8,
}

impl TryFrom<u16> for NamespaceOpcode {
    type Error = TransportError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Attach),
            2 => Ok(Self::Tail),
            3 => Ok(Self::Spawn),
            4 => Ok(Self::Kill),
            5 => Ok(Self::Echo),
            6 => Ok(Self::Cat),
            7 => Ok(Self::List),
            8 => Ok(Self::Log),
            _ => Err(TransportError::InvalidOperation),
        }
    }
}

/// Pointer-free namespace request descriptor. Variable bytes live in the
/// separately bounded shared request frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct NamespaceRequestHeader {
    /// ABI version.
    pub version: u16,
    /// Header size used for layout mismatch detection.
    pub header_bytes: u16,
    /// One [`NamespaceOpcode`] discriminant.
    pub opcode: u16,
    /// Reserved flags; must be zero in version 1.
    pub flags: u16,
    /// Monotonic operation sequence.
    pub sequence: u64,
    /// Supervisor-owned child generation.
    pub generation: u64,
    /// Path bytes in the shared frame.
    pub path_len: u16,
    /// Payload bytes immediately following the path.
    pub payload_len: u16,
    /// Reserved for a future ABI; must be zero.
    pub reserved: u32,
}

const _: [(); NAMESPACE_HEADER_BYTES] = [(); core::mem::size_of::<NamespaceRequestHeader>()];

impl NamespaceRequestHeader {
    /// Construct a version-1 request descriptor.
    pub fn new(
        opcode: NamespaceOpcode,
        token: RequestToken,
        path_len: usize,
        payload_len: usize,
    ) -> Result<Self, TransportError> {
        if path_len > NAMESPACE_PATH_MAX || payload_len > NAMESPACE_PAYLOAD_MAX {
            return Err(TransportError::InvalidLimits);
        }
        Ok(Self {
            version: NAMESPACE_SERVICE_ABI_VERSION,
            header_bytes: core::mem::size_of::<Self>() as u16,
            opcode: opcode as u16,
            flags: 0,
            sequence: token.sequence,
            generation: token.generation,
            path_len: path_len as u16,
            payload_len: payload_len as u16,
            reserved: 0,
        })
    }

    /// Return the validated request token.
    pub const fn token(self) -> Result<RequestToken, TransportError> {
        RequestToken::new(self.sequence, self.generation)
    }

    /// Decode one little-endian shared-frame header.
    pub fn decode(bytes: &[u8]) -> Result<Self, TransportError> {
        if bytes.len() < NAMESPACE_HEADER_BYTES {
            return Err(TransportError::PartialFrame);
        }
        let header = Self {
            version: read_u16(bytes, 0),
            header_bytes: read_u16(bytes, 2),
            opcode: read_u16(bytes, 4),
            flags: read_u16(bytes, 6),
            sequence: read_u64(bytes, 8),
            generation: read_u64(bytes, 16),
            path_len: read_u16(bytes, 24),
            payload_len: read_u16(bytes, 26),
            reserved: read_u32(bytes, 28),
        };
        if header.header_bytes as usize != NAMESPACE_HEADER_BYTES {
            return Err(TransportError::InvalidAbi);
        }
        Ok(header)
    }

    /// Encode one little-endian shared-frame header.
    pub fn encode(self, output: &mut [u8]) -> Result<(), TransportError> {
        if output.len() < NAMESPACE_HEADER_BYTES {
            return Err(TransportError::BufferTooSmall);
        }
        write_u16(output, 0, self.version);
        write_u16(output, 2, self.header_bytes);
        write_u16(output, 4, self.opcode);
        write_u16(output, 6, self.flags);
        write_u64(output, 8, self.sequence);
        write_u64(output, 16, self.generation);
        write_u16(output, 24, self.path_len);
        write_u16(output, 26, self.payload_len);
        write_u32(output, 28, self.reserved);
        Ok(())
    }
}

/// Result status returned by the isolated namespace parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum NamespaceStatus {
    /// Root may apply the typed prepared operation after policy evaluation.
    Prepared = 0,
    /// The request was rejected as malformed.
    Invalid = 1,
    /// Cancellation won before preparation was published.
    Cancelled = 2,
    /// The service queue was full.
    Busy = 3,
    /// The child generation was revoked.
    Revoked = 4,
}

impl TryFrom<u16> for NamespaceStatus {
    type Error = TransportError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Prepared),
            1 => Ok(Self::Invalid),
            2 => Ok(Self::Cancelled),
            3 => Ok(Self::Busy),
            4 => Ok(Self::Revoked),
            _ => Err(TransportError::InvalidAbi),
        }
    }
}

/// Pointer-free response descriptor published sequence-last by the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct NamespaceResponseHeader {
    /// ABI version.
    pub version: u16,
    /// Header size used for layout mismatch detection.
    pub header_bytes: u16,
    /// Echoed operation discriminant.
    pub opcode: u16,
    /// Typed preparation result.
    pub status: NamespaceStatus,
    /// Echoed generation.
    pub generation: u64,
    /// Prepared path length.
    pub path_len: u16,
    /// Prepared payload length.
    pub payload_len: u16,
    /// Reserved for a future ABI; must be zero.
    pub reserved: u32,
    /// Sequence is committed last; zero means no durable response.
    pub sequence: u64,
}

const _: [(); NAMESPACE_HEADER_BYTES] = [(); core::mem::size_of::<NamespaceResponseHeader>()];

impl NamespaceResponseHeader {
    /// Decode one little-endian response header from a shared frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, TransportError> {
        if bytes.len() < NAMESPACE_HEADER_BYTES {
            return Err(TransportError::PartialFrame);
        }
        let header = Self {
            version: read_u16(bytes, 0),
            header_bytes: read_u16(bytes, 2),
            opcode: read_u16(bytes, 4),
            status: NamespaceStatus::try_from(read_u16(bytes, 6))?,
            generation: read_u64(bytes, 8),
            path_len: read_u16(bytes, 16),
            payload_len: read_u16(bytes, 18),
            reserved: read_u32(bytes, 20),
            sequence: read_u64(bytes, 24),
        };
        if header.version != NAMESPACE_SERVICE_ABI_VERSION
            || header.header_bytes as usize != NAMESPACE_HEADER_BYTES
            || header.reserved != 0
        {
            return Err(TransportError::InvalidAbi);
        }
        Ok(header)
    }
}

/// Borrowed typed view of one prepared child response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamespaceResponseView<'a> {
    header: NamespaceResponseHeader,
    opcode: NamespaceOpcode,
    path: &'a str,
    payload: &'a str,
}

impl<'a> NamespaceResponseView<'a> {
    /// Return the validated pointer-free response header.
    #[must_use]
    pub const fn header(self) -> NamespaceResponseHeader {
        self.header
    }

    /// Return the validated operation kind.
    #[must_use]
    pub const fn opcode(self) -> NamespaceOpcode {
        self.opcode
    }

    /// Return the validated prepared path.
    #[must_use]
    pub const fn path(self) -> &'a str {
        self.path
    }

    /// Return the validated prepared payload.
    #[must_use]
    pub const fn payload(self) -> &'a str {
        self.payload
    }
}

/// Borrowed typed view of one validated untrusted request frame.
///
/// This process-local view is not an ABI record; the pointer-free
/// [`NamespaceRequestHeader`] remains the shared-frame authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamespaceRequestView<'a> {
    header: NamespaceRequestHeader,
    opcode: NamespaceOpcode,
    path: &'a str,
    payload: &'a str,
}

impl<'a> NamespaceRequestView<'a> {
    /// Return the validated pointer-free header.
    #[must_use]
    pub const fn header(self) -> NamespaceRequestHeader {
        self.header
    }

    /// Return the validated operation kind.
    #[must_use]
    pub const fn opcode(self) -> NamespaceOpcode {
        self.opcode
    }

    /// Return the validated namespace path.
    #[must_use]
    pub const fn path(self) -> &'a str {
        self.path
    }

    /// Return the validated control payload.
    #[must_use]
    pub const fn payload(self) -> &'a str {
        self.payload
    }
}

/// Fixed-capacity prepared namespace operation returned to root policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedNamespaceOperation<
    const PATH_BYTES: usize = NAMESPACE_PATH_MAX,
    const PAYLOAD_BYTES: usize = NAMESPACE_PAYLOAD_MAX,
> {
    header: NamespaceResponseHeader,
    path: [u8; PATH_BYTES],
    payload: [u8; PAYLOAD_BYTES],
}

impl<const PATH_BYTES: usize, const PAYLOAD_BYTES: usize>
    PreparedNamespaceOperation<PATH_BYTES, PAYLOAD_BYTES>
{
    /// Return the durable response header.
    #[must_use]
    pub const fn header(&self) -> NamespaceResponseHeader {
        self.header
    }

    /// Return the validated operation.
    pub fn opcode(&self) -> Result<NamespaceOpcode, TransportError> {
        NamespaceOpcode::try_from(self.header.opcode)
    }

    /// Borrow the validated absolute namespace path.
    pub fn path(&self) -> Result<&str, TransportError> {
        str::from_utf8(&self.path[..self.header.path_len as usize])
            .map_err(|_| TransportError::InvalidPath)
    }

    /// Borrow the validated UTF-8 control payload.
    pub fn payload(&self) -> Result<&str, TransportError> {
        str::from_utf8(&self.payload[..self.header.payload_len as usize])
            .map_err(|_| TransportError::InvalidPayload)
    }

    /// Validate that this response belongs to the exact outstanding request.
    pub fn validate_identity(&self, token: RequestToken) -> Result<(), TransportError> {
        if self.header.version != NAMESPACE_SERVICE_ABI_VERSION
            || self.header.header_bytes as usize != core::mem::size_of::<NamespaceResponseHeader>()
            || self.header.reserved != 0
        {
            return Err(TransportError::InvalidAbi);
        }
        if self.header.status != NamespaceStatus::Prepared
            || self.header.sequence != token.sequence
            || self.header.generation != token.generation
        {
            return Err(TransportError::StaleIdentity);
        }
        Ok(())
    }

    /// Encode the prepared response header, path, and payload into a bounded
    /// shared frame. Callers publish the frame before sending the IPC reply.
    pub fn encode(&self, output: &mut [u8]) -> Result<usize, TransportError> {
        let path_len = self.header.path_len as usize;
        let payload_len = self.header.payload_len as usize;
        let total = NAMESPACE_HEADER_BYTES
            .saturating_add(path_len)
            .saturating_add(payload_len);
        if output.len() < total {
            return Err(TransportError::BufferTooSmall);
        }
        let header = &mut output[..NAMESPACE_HEADER_BYTES];
        write_u16(header, 0, self.header.version);
        write_u16(header, 2, self.header.header_bytes);
        write_u16(header, 4, self.header.opcode);
        write_u16(header, 6, self.header.status as u16);
        write_u64(header, 8, self.header.generation);
        write_u16(header, 16, self.header.path_len);
        write_u16(header, 18, self.header.payload_len);
        write_u32(header, 20, self.header.reserved);
        write_u64(header, 24, self.header.sequence);
        output[NAMESPACE_HEADER_BYTES..NAMESPACE_HEADER_BYTES + path_len]
            .copy_from_slice(&self.path[..path_len]);
        output[NAMESPACE_HEADER_BYTES + path_len..total]
            .copy_from_slice(&self.payload[..payload_len]);
        Ok(total)
    }
}

/// Validate untrusted shared-frame bytes and produce one typed operation for
/// root-owned policy and mutation.
pub fn prepare_namespace_operation<const PATH_BYTES: usize, const PAYLOAD_BYTES: usize>(
    request: NamespaceRequestHeader,
    shared_frame: &[u8],
) -> Result<PreparedNamespaceOperation<PATH_BYTES, PAYLOAD_BYTES>, TransportError> {
    let validated = validate_namespace_request(request, shared_frame)?;
    let token = validated.header.token()?;
    let opcode = validated.opcode;
    let path_bytes = validated.path.as_bytes();
    let payload_bytes = validated.payload.as_bytes();
    let path_len = path_bytes.len();
    let payload_len = payload_bytes.len();
    if path_len > PATH_BYTES || payload_len > PAYLOAD_BYTES {
        return Err(TransportError::InvalidLimits);
    }

    let mut prepared_path = [0u8; PATH_BYTES];
    prepared_path[..path_len].copy_from_slice(path_bytes);
    let mut prepared_payload = [0u8; PAYLOAD_BYTES];
    prepared_payload[..payload_len].copy_from_slice(payload_bytes);
    let mut prepared = PreparedNamespaceOperation {
        header: NamespaceResponseHeader {
            version: NAMESPACE_SERVICE_ABI_VERSION,
            header_bytes: core::mem::size_of::<NamespaceResponseHeader>() as u16,
            opcode: opcode as u16,
            status: NamespaceStatus::Prepared,
            generation: token.generation,
            path_len: request.path_len,
            payload_len: request.payload_len,
            reserved: 0,
            sequence: 0,
        },
        path: prepared_path,
        payload: prepared_payload,
    };
    // The data fields are fully initialized before sequence is made nonzero.
    prepared.header.sequence = token.sequence;
    Ok(prepared)
}

/// Validate one shared namespace request without copying its bounded variable
/// bytes. This is used by host fixtures and root's typed response adapter; the
/// target child additionally copies the result into its response frame.
pub fn validate_namespace_request(
    request: NamespaceRequestHeader,
    shared_frame: &[u8],
) -> Result<NamespaceRequestView<'_>, TransportError> {
    let path_len = request.path_len as usize;
    let payload_len = request.payload_len as usize;
    if path_len.saturating_add(payload_len) != shared_frame.len() {
        return Err(TransportError::InvalidLimits);
    }
    let path_bytes = &shared_frame[..path_len];
    let payload_bytes = &shared_frame[path_len..];
    let path = str::from_utf8(path_bytes).map_err(|_| TransportError::InvalidPath)?;
    let payload = str::from_utf8(payload_bytes).map_err(|_| TransportError::InvalidPayload)?;
    validate_namespace_parts(request, path, payload)
}

/// Validate already separated path and payload slices against the pointer-free
/// request descriptor. This avoids a temporary concatenation in in-process
/// fixtures while preserving the exact child ABI checks.
pub fn validate_namespace_parts<'a>(
    request: NamespaceRequestHeader,
    path: &'a str,
    payload: &'a str,
) -> Result<NamespaceRequestView<'a>, TransportError> {
    if request.version != NAMESPACE_SERVICE_ABI_VERSION
        || request.header_bytes as usize != core::mem::size_of::<NamespaceRequestHeader>()
        || request.flags != 0
        || request.reserved != 0
    {
        return Err(TransportError::InvalidAbi);
    }
    request.token()?;
    let opcode = NamespaceOpcode::try_from(request.opcode)?;
    let path_len = request.path_len as usize;
    let payload_len = request.payload_len as usize;
    if path_len > NAMESPACE_PATH_MAX
        || payload_len > NAMESPACE_PAYLOAD_MAX
        || path_len != path.len()
        || payload_len != payload.len()
    {
        return Err(TransportError::InvalidLimits);
    }
    validate_namespace_fields(opcode, path, payload)?;

    Ok(NamespaceRequestView {
        header: request,
        opcode,
        path,
        payload,
    })
}

/// Decode and independently validate one exact child response before root
/// policy or authoritative mutation consumes it.
pub fn validate_namespace_response(
    response_frame: &[u8],
    token: RequestToken,
    expected_opcode: NamespaceOpcode,
) -> Result<NamespaceResponseView<'_>, TransportError> {
    let header = NamespaceResponseHeader::decode(response_frame)?;
    if header.status != NamespaceStatus::Prepared
        || header.sequence != token.sequence
        || header.generation != token.generation
        || header.opcode != expected_opcode as u16
    {
        return Err(TransportError::StaleIdentity);
    }
    let path_len = header.path_len as usize;
    let payload_len = header.payload_len as usize;
    let expected_len = NAMESPACE_HEADER_BYTES
        .saturating_add(path_len)
        .saturating_add(payload_len);
    if path_len > NAMESPACE_PATH_MAX
        || payload_len > NAMESPACE_PAYLOAD_MAX
        || response_frame.len() != expected_len
    {
        return Err(TransportError::InvalidLimits);
    }
    let path_bytes = &response_frame[NAMESPACE_HEADER_BYTES..NAMESPACE_HEADER_BYTES + path_len];
    let payload_bytes = &response_frame[NAMESPACE_HEADER_BYTES + path_len..];
    let path = str::from_utf8(path_bytes).map_err(|_| TransportError::InvalidPath)?;
    let payload = str::from_utf8(payload_bytes).map_err(|_| TransportError::InvalidPayload)?;
    validate_namespace_fields(expected_opcode, path, payload)?;
    Ok(NamespaceResponseView {
        header,
        opcode: expected_opcode,
        path,
        payload,
    })
}

fn validate_namespace_fields(
    opcode: NamespaceOpcode,
    path: &str,
    payload: &str,
) -> Result<(), TransportError> {
    let path_required = matches!(
        opcode,
        NamespaceOpcode::Tail
            | NamespaceOpcode::Echo
            | NamespaceOpcode::Cat
            | NamespaceOpcode::List
    );
    if path_required {
        validate_namespace_path(path)?;
    } else if !path.is_empty() {
        return Err(TransportError::InvalidPath);
    }

    match opcode {
        NamespaceOpcode::Attach
        | NamespaceOpcode::Spawn
        | NamespaceOpcode::Kill
        | NamespaceOpcode::Echo => {
            if payload.is_empty() || payload.contains(['\n', '\r', '\0']) {
                return Err(TransportError::InvalidPayload);
            }
        }
        NamespaceOpcode::Tail
        | NamespaceOpcode::Cat
        | NamespaceOpcode::List
        | NamespaceOpcode::Log => {
            if !payload.is_empty() {
                return Err(TransportError::InvalidPayload);
            }
        }
    }
    Ok(())
}

fn validate_namespace_path(path: &str) -> Result<(), TransportError> {
    if !path.starts_with('/') || path.len() > NAMESPACE_PATH_MAX || path.contains('\0') {
        return Err(TransportError::InvalidPath);
    }
    if path != "/" && (path.ends_with('/') || path.contains("//")) {
        return Err(TransportError::InvalidPath);
    }
    let mut components = 0usize;
    for component in path.split('/').filter(|component| !component.is_empty()) {
        if component == "." || component == ".." {
            return Err(TransportError::InvalidPath);
        }
        components = components.saturating_add(1);
        if components > NAMESPACE_COMPONENT_MAX {
            return Err(TransportError::InvalidPath);
        }
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;

    fn wire_frame(payload: &[u8]) -> std::vec::Vec<u8> {
        let len = MIN_9P_FRAME_BYTES + payload.len();
        let mut frame = vec![0u8; len];
        frame[..4].copy_from_slice(&(len as u32).to_le_bytes());
        frame[4] = 100;
        frame[5..7].copy_from_slice(&7u16.to_le_bytes());
        frame[7..].copy_from_slice(payload);
        frame
    }

    #[test]
    fn partial_frames_complete_without_consuming_next_frame() {
        let limits = FrameLimits::new(64).unwrap();
        let mut accumulator = FrameAccumulator::<64>::new(limits).unwrap();
        let frame = wire_frame(b"abcd");
        assert_eq!(
            accumulator.push(&frame[..2]).unwrap(),
            FrameProgress::Pending {
                consumed: 2,
                needed: 2
            }
        );
        assert!(matches!(
            accumulator.push(&frame[2..6]).unwrap(),
            FrameProgress::Pending { consumed: 4, .. }
        ));
        let mut joined = frame[6..].to_vec();
        joined.extend_from_slice(&wire_frame(b"next"));
        assert_eq!(
            accumulator.push(&joined).unwrap(),
            FrameProgress::Complete {
                consumed: frame.len() - 6,
                frame_len: frame.len()
            }
        );
        let mut output = [0u8; 64];
        let len = accumulator.take(&mut output).unwrap();
        assert_eq!(&output[..len], frame.as_slice());
    }

    #[test]
    fn close_rejects_partial_frame_and_revoke_is_terminal() {
        let mut accumulator = FrameAccumulator::<64>::new(FrameLimits::new(64).unwrap()).unwrap();
        accumulator.push(&[9, 0, 0]).unwrap();
        assert_eq!(accumulator.close(), Err(TransportError::PartialFrame));
        assert_eq!(accumulator.push(&[0]), Err(TransportError::Closed));

        let mut accumulator = FrameAccumulator::<64>::new(FrameLimits::new(64).unwrap()).unwrap();
        accumulator.push(&[9, 0]).unwrap();
        accumulator.revoke();
        assert_eq!(accumulator.buffered_len(), 0);
        assert_eq!(accumulator.push(&[0]), Err(TransportError::Revoked));
    }

    #[test]
    fn over_limit_frame_fails_before_payload_is_copied() {
        let mut accumulator = FrameAccumulator::<32>::new(FrameLimits::new(32).unwrap()).unwrap();
        assert_eq!(
            accumulator.push(&64u32.to_le_bytes()),
            Err(TransportError::FrameTooLarge)
        );
        assert_eq!(accumulator.buffered_len(), 0);
    }

    #[test]
    fn bounded_queue_reports_backpressure_cancel_close_and_revoke() {
        let frame = wire_frame(&[]);
        let mut queue = BoundedFrameQueue::<2, 64>::new(9).unwrap();
        let first = RequestToken::new(1, 9).unwrap();
        let second = RequestToken::new(2, 9).unwrap();
        queue.submit(first, &frame).unwrap();
        queue.submit(second, &frame).unwrap();
        assert_eq!(
            queue.submit(RequestToken::new(3, 9).unwrap(), &frame),
            Err(TransportError::QueueFull)
        );
        queue.cancel(second).unwrap();
        queue.close().unwrap();
        assert_eq!(queue.state(), TransportState::Closing);
        assert!(!queue.pop().unwrap().cancelled());
        assert!(queue.pop().unwrap().cancelled());
        assert!(queue.pop().is_none());
        assert_eq!(queue.state(), TransportState::Closed);

        let mut queue = BoundedFrameQueue::<2, 64>::new(10).unwrap();
        queue
            .submit(RequestToken::new(1, 10).unwrap(), &frame)
            .unwrap();
        queue.revoke();
        assert!(queue.is_empty());
        assert_eq!(queue.state(), TransportState::Revoked);
        assert_eq!(
            queue.cancel(RequestToken::new(1, 10).unwrap()),
            Err(TransportError::Revoked)
        );
    }

    #[test]
    fn short_writes_use_shared_bounded_retry_policy() {
        let mut write = PartialWrite::new(8, WriteRetryPolicy::Retry).unwrap();
        assert_eq!(
            write.advance(0).unwrap(),
            WriteProgress::RetryAfter { delay_ms: 5 }
        );
        assert_eq!(
            write.advance(2).unwrap(),
            WriteProgress::RetryAfter { delay_ms: 10 }
        );
        assert_eq!(write.advance(6).unwrap(), WriteProgress::Complete);
        assert_eq!(write.short_writes(), 2);
        assert_eq!(write.retries(), 2);
    }

    #[test]
    fn namespace_preparation_is_bounded_typed_and_generation_stamped() {
        let token = RequestToken::new(4, 3).unwrap();
        let path = b"/queen/ctl";
        let payload = br#"{"spawn":"heart"}"#;
        let header =
            NamespaceRequestHeader::new(NamespaceOpcode::Echo, token, path.len(), payload.len())
                .unwrap();
        let mut frame = path.to_vec();
        frame.extend_from_slice(payload);
        let prepared = prepare_namespace_operation::<256, 4096>(header, &frame).unwrap();
        prepared.validate_identity(token).unwrap();
        assert_eq!(prepared.opcode().unwrap(), NamespaceOpcode::Echo);
        assert_eq!(prepared.path().unwrap(), "/queen/ctl");
        assert_eq!(prepared.payload().unwrap(), r#"{"spawn":"heart"}"#);
        assert_eq!(core::mem::size_of::<NamespaceRequestHeader>(), 32);
        assert_eq!(core::mem::size_of::<NamespaceResponseHeader>(), 32);
        let mut response = [0u8; 512];
        let response_len = prepared.encode(&mut response).unwrap();
        let response =
            validate_namespace_response(&response[..response_len], token, NamespaceOpcode::Echo)
                .unwrap();
        assert_eq!(response.path(), "/queen/ctl");
        assert_eq!(response.payload(), r#"{"spawn":"heart"}"#);
    }

    #[test]
    fn namespace_preparation_rejects_traversal_overdepth_and_stale_identity() {
        let token = RequestToken::new(1, 2).unwrap();
        for path in [
            "/queen/../ctl",
            "/a/b/c/d/e/f/g/h/i",
            "queen/ctl",
            "/queen//ctl",
            "/queen/ctl/",
        ] {
            let header =
                NamespaceRequestHeader::new(NamespaceOpcode::Cat, token, path.len(), 0).unwrap();
            assert_eq!(
                prepare_namespace_operation::<256, 4096>(header, path.as_bytes()),
                Err(TransportError::InvalidPath)
            );
        }
        let path = b"/queen/ctl";
        let header =
            NamespaceRequestHeader::new(NamespaceOpcode::Cat, token, path.len(), 0).unwrap();
        let prepared = prepare_namespace_operation::<256, 4096>(header, path).unwrap();
        assert_eq!(
            prepared.validate_identity(RequestToken::new(2, 2).unwrap()),
            Err(TransportError::StaleIdentity)
        );
    }

    #[test]
    fn runtime_descriptor_and_error_wire_contract_are_exact() {
        let descriptor = NamespaceRuntimeInitDescriptor {
            version: NAMESPACE_RUNTIME_INIT_VERSION,
            descriptor_bytes: NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES as u16,
            endpoint_cap_rights: NAMESPACE_CHILD_RECEIVE_RIGHTS,
            request_frame_rights: SEL4_RIGHTS_READ,
            response_frame_rights: SEL4_RIGHTS_READ_WRITE,
            reserved_rights: 0,
            generation: 7,
            request_frame_vaddr: 0x1000,
            response_frame_vaddr: 0x3000,
            frame_bytes: NAMESPACE_SHARED_FRAME_BYTES as u32,
            request_badge: 11,
            endpoint_cptr: NAMESPACE_SERVICE_ENDPOINT_SLOT,
            reply_cptr: NAMESPACE_SERVICE_REPLY_SLOT,
            reserved: [0; 2],
        };
        assert!(descriptor.valid());
        assert!(!NamespaceRuntimeInitDescriptor {
            endpoint_cap_rights: NAMESPACE_ROOT_CALL_RIGHTS,
            ..descriptor
        }
        .valid());
        assert_eq!(
            core::mem::size_of::<NamespaceRuntimeInitDescriptor>(),
            NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES
        );
        let mut encoded = [0xff; NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES];
        descriptor.encode(&mut encoded).unwrap();
        assert_eq!(read_u16(&encoded, 0), NAMESPACE_RUNTIME_INIT_VERSION);
        assert_eq!(read_u64(&encoded, 8), descriptor.generation);
        assert_eq!(read_u64(&encoded, 16), descriptor.request_frame_vaddr);
        assert_eq!(read_u64(&encoded, 24), descriptor.response_frame_vaddr);
        assert_eq!(read_u32(&encoded, 36), descriptor.request_badge);
        assert_eq!(read_u64(&encoded, 40), NAMESPACE_SERVICE_ENDPOINT_SLOT);
        assert_eq!(read_u64(&encoded, 48), NAMESPACE_SERVICE_REPLY_SLOT);

        for error in [
            TransportError::InvalidLimits,
            TransportError::InvalidFrameLength,
            TransportError::FrameTooLarge,
            TransportError::PartialFrame,
            TransportError::QueueFull,
            TransportError::Closed,
            TransportError::Revoked,
            TransportError::UnknownRequest,
            TransportError::StaleIdentity,
            TransportError::BufferTooSmall,
            TransportError::InvalidOperation,
            TransportError::InvalidPath,
            TransportError::InvalidPayload,
            TransportError::InvalidAbi,
            TransportError::ShortWriteExhausted,
        ] {
            assert_eq!(TransportError::from_wire_code(error.wire_code()), Ok(error));
        }
        assert_eq!(
            TransportError::from_wire_code(0),
            Err(TransportError::InvalidAbi)
        );
    }
}
