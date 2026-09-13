// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define bounded trace record/replay for Secure9P batches and ACK lines.
// Author: Lukas Bower
#![allow(clippy::module_name_repetitions)]

extern crate alloc;

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

use sha2::{Digest, Sha256};

/// Trace file magic bytes.
pub const TRACE_MAGIC: &[u8; 8] = b"COHTRACE";
/// Trace file format version.
pub const TRACE_VERSION: u8 = 1;
/// Additive canonical format for redacted console projections. Version 1 is retained.
pub const TRACE_CAPTURE_VERSION: u8 = 2;
const TRACE_HEADER_LEN: usize = 18;
const TRACE_DIGEST_LEN: usize = 32;
const CAPTURE_METADATA_LEN: usize = 174;

/// Metadata bound by the same digest as the canonical frames and acknowledgements.
/// Digests detect corruption; they do not authenticate the recording host or target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureMetadata {
    /// 1 = TCP console, 2 = REST console projection.
    pub backend: u8,
    /// 0 = complete, 1 = disconnected/error, 2 = byte bound, 3 = duration bound.
    pub completion: u8,
    /// SHA-256 of the operator-supplied target identity.
    pub target_sha256: [u8; 32],
    /// SHA-256 of the operator-supplied boot/session identity.
    pub session_sha256: [u8; 32],
    /// Expected target resolved-manifest identity; zero means unknown.
    pub manifest_sha256: [u8; 32],
    /// Expected image identity; zero means unknown.
    pub image_sha256: [u8; 32],
    /// SHA-256 of the capture policy and redaction contract.
    pub policy_sha256: [u8; 32],
    /// Capture start in Unix milliseconds.
    pub captured_unix_ms: u64,
    /// Explicit finite capture window in milliseconds.
    pub max_duration_ms: u32,
}

impl CaptureMetadata {
    fn validate(&self) -> Result<(), TraceError> {
        if !matches!(self.backend, 1 | 2) || self.completion > 3 || self.max_duration_ms == 0 {
            return Err(TraceError::InvalidLength);
        }
        Ok(())
    }
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), TraceError> {
        self.validate()?;
        out.push(self.backend);
        out.push(self.completion);
        for digest in [
            &self.target_sha256,
            &self.session_sha256,
            &self.manifest_sha256,
            &self.image_sha256,
            &self.policy_sha256,
        ] {
            out.extend_from_slice(digest);
        }
        out.extend_from_slice(&self.captured_unix_ms.to_le_bytes());
        out.extend_from_slice(&self.max_duration_ms.to_le_bytes());
        Ok(())
    }
    fn decode(data: &[u8], offset: &mut usize) -> Result<Self, TraceError> {
        let bytes = read_bytes(data, offset, CAPTURE_METADATA_LEN)?;
        let mut hashes = [[0u8; 32]; 5];
        for (i, hash) in hashes.iter_mut().enumerate() {
            hash.copy_from_slice(&bytes[2 + i * 32..2 + (i + 1) * 32]);
        }
        let mut time = [0u8; 8];
        time.copy_from_slice(&bytes[162..170]);
        let mut duration = [0u8; 4];
        duration.copy_from_slice(&bytes[170..174]);
        let metadata = Self {
            backend: bytes[0],
            completion: bytes[1],
            target_sha256: hashes[0],
            session_sha256: hashes[1],
            manifest_sha256: hashes[2],
            image_sha256: hashes[3],
            policy_sha256: hashes[4],
            captured_unix_ms: u64::from_le_bytes(time),
            max_duration_ms: u32::from_le_bytes(duration),
        };
        metadata.validate()?;
        Ok(metadata)
    }
}

/// Trace policy limits enforced during record and replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TracePolicy {
    /// Maximum encoded trace size in bytes.
    pub max_bytes: u32,
    /// Maximum frame payload size in bytes.
    pub max_frame_bytes: u32,
    /// Maximum acknowledgement line size in bytes.
    pub max_ack_bytes: u32,
}

impl TracePolicy {
    /// Construct a new trace policy.
    #[must_use]
    pub const fn new(max_bytes: u32, max_frame_bytes: u32, max_ack_bytes: u32) -> Self {
        Self {
            max_bytes,
            max_frame_bytes,
            max_ack_bytes,
        }
    }
}

/// A recorded Secure9P request/response exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFrame {
    /// Secure9P request batch bytes.
    pub request: Vec<u8>,
    /// Secure9P response batch bytes.
    pub response: Vec<u8>,
}

/// Decoded trace log containing Secure9P frames and acknowledgement lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceLog {
    /// Present only for version-2 console transcript captures. Authentication and
    /// request payloads are omitted; frames retain ordered redacted response lines.
    pub capture: Option<CaptureMetadata>,
    /// Ordered Secure9P request/response frames.
    pub frames: Vec<TraceFrame>,
    /// Ordered acknowledgement lines emitted by the client.
    pub ack_lines: Vec<String>,
}

impl TraceLog {
    /// Encode the trace log to bytes with hash validation.
    pub fn encode(&self, policy: TracePolicy) -> Result<Vec<u8>, TraceError> {
        let mut total = TRACE_HEADER_LEN
            .checked_add(TRACE_DIGEST_LEN)
            .ok_or(TraceError::LengthOverflow)?;
        let max_bytes = policy.max_bytes as usize;
        let max_frame = policy.max_frame_bytes as usize;
        let max_ack = policy.max_ack_bytes as usize;
        if let Some(capture) = &self.capture {
            capture.validate()?;
            total = total
                .checked_add(CAPTURE_METADATA_LEN)
                .ok_or(TraceError::LengthOverflow)?;
            if self.frames.iter().any(|frame| {
                core::str::from_utf8(&frame.request).is_err()
                    || core::str::from_utf8(&frame.response).is_err()
            }) {
                return Err(TraceError::InvalidLength);
            }
        }

        for frame in &self.frames {
            let request_len = frame.request.len();
            let response_len = frame.response.len();
            if request_len > max_frame || response_len > max_frame {
                return Err(TraceError::FrameTooLarge {
                    len: request_len.max(response_len),
                    max: policy.max_frame_bytes,
                });
            }
            total = total
                .checked_add(4 + request_len + 4 + response_len)
                .ok_or(TraceError::LengthOverflow)?;
        }
        for line in &self.ack_lines {
            let len = line.len();
            if len > max_ack {
                return Err(TraceError::AckTooLarge {
                    len,
                    max: policy.max_ack_bytes,
                });
            }
            total = total
                .checked_add(4 + len)
                .ok_or(TraceError::LengthOverflow)?;
        }
        if total > max_bytes {
            return Err(TraceError::TraceTooLarge {
                size: total,
                max: policy.max_bytes,
            });
        }

        let frame_count =
            u32::try_from(self.frames.len()).map_err(|_| TraceError::LengthOverflow)?;
        let ack_count =
            u32::try_from(self.ack_lines.len()).map_err(|_| TraceError::LengthOverflow)?;

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(TRACE_MAGIC);
        out.push(if self.capture.is_some() {
            TRACE_CAPTURE_VERSION
        } else {
            TRACE_VERSION
        });
        out.push(0);
        out.extend_from_slice(&frame_count.to_le_bytes());
        out.extend_from_slice(&ack_count.to_le_bytes());
        if let Some(capture) = &self.capture {
            capture.encode(&mut out)?;
        }

        for frame in &self.frames {
            let request_len = frame.request.len() as u32;
            let response_len = frame.response.len() as u32;
            out.extend_from_slice(&request_len.to_le_bytes());
            out.extend_from_slice(&frame.request);
            out.extend_from_slice(&response_len.to_le_bytes());
            out.extend_from_slice(&frame.response);
        }
        for line in &self.ack_lines {
            let bytes = line.as_bytes();
            let len = bytes.len() as u32;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(bytes);
        }

        let digest = Sha256::digest(&out);
        out.extend_from_slice(&digest);
        Ok(out)
    }

    /// Decode a trace log from bytes, enforcing size limits and hash validation.
    pub fn decode(bytes: &[u8], policy: TracePolicy) -> Result<Self, TraceError> {
        let max_bytes = policy.max_bytes as usize;
        if bytes.len() > max_bytes {
            return Err(TraceError::TraceTooLarge {
                size: bytes.len(),
                max: policy.max_bytes,
            });
        }
        if bytes.len() < TRACE_HEADER_LEN + TRACE_DIGEST_LEN {
            return Err(TraceError::Truncated);
        }
        let payload_len = bytes.len() - TRACE_DIGEST_LEN;
        let (payload, digest) = bytes.split_at(payload_len);
        let expected = Sha256::digest(payload);
        if expected.as_slice() != digest {
            return Err(TraceError::HashMismatch);
        }

        if payload.len() < TRACE_HEADER_LEN {
            return Err(TraceError::Truncated);
        }
        if &payload[0..8] != TRACE_MAGIC {
            return Err(TraceError::BadMagic);
        }
        let version = payload[8];
        if version != TRACE_VERSION && version != TRACE_CAPTURE_VERSION {
            return Err(TraceError::UnsupportedVersion(version));
        }
        let frame_count = u32::from_le_bytes(payload[10..14].try_into().expect("frame count"));
        let ack_count = u32::from_le_bytes(payload[14..18].try_into().expect("ack count"));
        let frame_count = usize::try_from(frame_count).map_err(|_| TraceError::LengthOverflow)?;
        let ack_count = usize::try_from(ack_count).map_err(|_| TraceError::LengthOverflow)?;

        let mut offset = TRACE_HEADER_LEN;
        if payload[9] != 0 {
            return Err(TraceError::InvalidLength);
        }
        let capture = if version == TRACE_CAPTURE_VERSION {
            Some(CaptureMetadata::decode(payload, &mut offset)?)
        } else {
            None
        };
        let max_frame = policy.max_frame_bytes as usize;
        let max_ack = policy.max_ack_bytes as usize;

        // Reject hostile counts before allocating from peer-controlled integers.
        if frame_count > payload.len().saturating_sub(offset) / 8
            || ack_count > payload.len().saturating_sub(offset) / 4
        {
            return Err(TraceError::InvalidLength);
        }
        let mut frames = Vec::new();
        for _ in 0..frame_count {
            let request_len = read_u32(payload, &mut offset)?;
            let request_len =
                usize::try_from(request_len).map_err(|_| TraceError::LengthOverflow)?;
            if request_len > max_frame {
                return Err(TraceError::FrameTooLarge {
                    len: request_len,
                    max: policy.max_frame_bytes,
                });
            }
            let request = read_bytes(payload, &mut offset, request_len)?;
            let response_len = read_u32(payload, &mut offset)?;
            let response_len =
                usize::try_from(response_len).map_err(|_| TraceError::LengthOverflow)?;
            if response_len > max_frame {
                return Err(TraceError::FrameTooLarge {
                    len: response_len,
                    max: policy.max_frame_bytes,
                });
            }
            let response = read_bytes(payload, &mut offset, response_len)?;
            if capture.is_some()
                && (core::str::from_utf8(&request).is_err()
                    || core::str::from_utf8(&response).is_err())
            {
                return Err(TraceError::InvalidLength);
            }
            frames.push(TraceFrame { request, response });
        }

        let mut ack_lines = Vec::new();
        for _ in 0..ack_count {
            let ack_len = read_u32(payload, &mut offset)?;
            let ack_len = usize::try_from(ack_len).map_err(|_| TraceError::LengthOverflow)?;
            if ack_len > max_ack {
                return Err(TraceError::AckTooLarge {
                    len: ack_len,
                    max: policy.max_ack_bytes,
                });
            }
            let ack_bytes = read_bytes(payload, &mut offset, ack_len)?;
            let ack = String::from_utf8(ack_bytes).map_err(|_| TraceError::InvalidUtf8)?;
            ack_lines.push(ack);
        }

        if offset != payload.len() {
            return Err(TraceError::InvalidLength);
        }

        Ok(Self {
            capture,
            frames,
            ack_lines,
        })
    }
}

/// Builder used to record trace frames and acknowledgement lines.
#[derive(Debug)]
pub struct TraceLogBuilder {
    capture: Option<CaptureMetadata>,
    policy: TracePolicy,
    frames: Vec<TraceFrame>,
    ack_lines: Vec<String>,
    total_bytes: usize,
}

/// Shared trace builder handle.
pub type TraceLogBuilderRef = Rc<RefCell<TraceLogBuilder>>;

impl TraceLogBuilder {
    /// Create a new trace builder with enforced limits.
    #[must_use]
    pub fn new(policy: TracePolicy) -> Self {
        Self {
            capture: None,
            policy,
            frames: Vec::new(),
            ack_lines: Vec::new(),
            total_bytes: TRACE_HEADER_LEN + TRACE_DIGEST_LEN,
        }
    }

    /// Use the same bounded builder for an identity-bound live console capture.
    pub fn capture(policy: TracePolicy, metadata: CaptureMetadata) -> Result<Self, TraceError> {
        metadata.validate()?;
        let mut builder = Self::new(policy);
        builder.total_bytes += CAPTURE_METADATA_LEN;
        if builder.total_bytes > policy.max_bytes as usize {
            return Err(TraceError::TraceTooLarge {
                size: builder.total_bytes,
                max: policy.max_bytes,
            });
        }
        builder.capture = Some(metadata);
        Ok(builder)
    }

    /// Create a shared trace builder handle.
    #[must_use]
    pub fn shared(policy: TracePolicy) -> TraceLogBuilderRef {
        Rc::new(RefCell::new(Self::new(policy)))
    }

    /// Record a Secure9P request/response exchange.
    pub fn record_frame(&mut self, request: &[u8], response: &[u8]) -> Result<(), TraceError> {
        self.record_frame_inner(request, response)?;
        self.frames.push(TraceFrame {
            request: request.to_vec(),
            response: response.to_vec(),
        });
        Ok(())
    }

    /// Record an acknowledgement line.
    pub fn record_ack(&mut self, line: &str) -> Result<(), TraceError> {
        let len = line.len();
        if len > self.policy.max_ack_bytes as usize {
            return Err(TraceError::AckTooLarge {
                len,
                max: self.policy.max_ack_bytes,
            });
        }
        self.total_bytes = self
            .total_bytes
            .checked_add(4 + len)
            .ok_or(TraceError::LengthOverflow)?;
        if self.total_bytes > self.policy.max_bytes as usize {
            return Err(TraceError::TraceTooLarge {
                size: self.total_bytes,
                max: self.policy.max_bytes,
            });
        }
        self.ack_lines.push(line.to_string());
        Ok(())
    }

    /// Atomically admit one console line and its optional acknowledgement entry.
    pub fn record_console_line(
        &mut self,
        line: &str,
        acknowledgement: bool,
    ) -> Result<(), TraceError> {
        let added = 8usize
            .checked_add(line.len())
            .and_then(|n| {
                if acknowledgement {
                    n.checked_add(4 + line.len())
                } else {
                    Some(n)
                }
            })
            .ok_or(TraceError::LengthOverflow)?;
        if self
            .total_bytes
            .checked_add(added)
            .ok_or(TraceError::LengthOverflow)?
            > self.policy.max_bytes as usize
        {
            return Err(TraceError::TraceTooLarge {
                size: self.total_bytes.saturating_add(added),
                max: self.policy.max_bytes,
            });
        }
        if acknowledgement && line.len() > self.policy.max_ack_bytes as usize {
            return Err(TraceError::AckTooLarge {
                len: line.len(),
                max: self.policy.max_ack_bytes,
            });
        }
        self.record_frame(&[], line.as_bytes())?;
        if acknowledgement {
            self.record_ack(line)?;
        }
        Ok(())
    }

    /// Snapshot the recorded trace log.
    #[must_use]
    pub fn snapshot(&self) -> TraceLog {
        TraceLog {
            capture: self.capture.clone(),
            frames: self.frames.clone(),
            ack_lines: self.ack_lines.clone(),
        }
    }

    fn record_frame_inner(&mut self, request: &[u8], response: &[u8]) -> Result<(), TraceError> {
        let max_frame = self.policy.max_frame_bytes as usize;
        let request_len = request.len();
        let response_len = response.len();
        if request_len > max_frame || response_len > max_frame {
            return Err(TraceError::FrameTooLarge {
                len: request_len.max(response_len),
                max: self.policy.max_frame_bytes,
            });
        }
        self.total_bytes = self
            .total_bytes
            .checked_add(4 + request_len + 4 + response_len)
            .ok_or(TraceError::LengthOverflow)?;
        if self.total_bytes > self.policy.max_bytes as usize {
            return Err(TraceError::TraceTooLarge {
                size: self.total_bytes,
                max: self.policy.max_bytes,
            });
        }
        Ok(())
    }
}

/// Errors encountered when recording or replaying trace data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceError {
    /// Trace file magic did not match.
    BadMagic,
    /// Trace format version unsupported.
    UnsupportedVersion(u8),
    /// Trace length values overflowed.
    LengthOverflow,
    /// Trace file truncated or incomplete.
    Truncated,
    /// Trace file contains an invalid length value.
    InvalidLength,
    /// Trace digest mismatch.
    HashMismatch,
    /// Trace frame exceeds size limits.
    FrameTooLarge {
        /// Observed frame size in bytes.
        len: usize,
        /// Maximum allowed frame size.
        max: u32,
    },
    /// Trace acknowledgement exceeds size limits.
    AckTooLarge {
        /// Observed acknowledgement line length in bytes.
        len: usize,
        /// Maximum allowed acknowledgement length.
        max: u32,
    },
    /// Trace exceeds maximum allowed size.
    TraceTooLarge {
        /// Observed total trace size in bytes.
        size: usize,
        /// Maximum allowed trace size.
        max: u32,
    },
    /// Trace payload contains invalid UTF-8.
    InvalidUtf8,
    /// Replay trace exhausted before completing a request.
    ReplayExhausted,
    /// Replay request did not match recorded frame.
    RequestMismatch {
        /// Frame index where the request mismatch occurred.
        index: usize,
    },
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceError::BadMagic => write!(f, "trace magic mismatch"),
            TraceError::UnsupportedVersion(version) => {
                write!(f, "unsupported trace version {version}")
            }
            TraceError::LengthOverflow => write!(f, "trace length overflow"),
            TraceError::Truncated => write!(f, "trace data truncated"),
            TraceError::InvalidLength => write!(f, "trace length invalid"),
            TraceError::HashMismatch => write!(f, "trace hash mismatch"),
            TraceError::FrameTooLarge { len, max } => {
                write!(f, "trace frame size {len} exceeds max {max}")
            }
            TraceError::AckTooLarge { len, max } => {
                write!(f, "trace ack size {len} exceeds max {max}")
            }
            TraceError::TraceTooLarge { size, max } => {
                write!(f, "trace size {size} exceeds max {max}")
            }
            TraceError::InvalidUtf8 => write!(f, "trace ack line invalid UTF-8"),
            TraceError::ReplayExhausted => write!(f, "trace replay exhausted"),
            TraceError::RequestMismatch { index } => {
                write!(f, "trace request mismatch at index {index}")
            }
        }
    }
}

impl core::error::Error for TraceError {}

/// Secure9P transport wrapper that records trace frames.
pub struct TraceTransportRecorder<T> {
    inner: T,
    log: TraceLogBuilderRef,
}

impl<T> TraceTransportRecorder<T> {
    /// Wrap an existing transport with a shared trace builder.
    #[must_use]
    pub fn new(inner: T, log: TraceLogBuilderRef) -> Self {
        Self { inner, log }
    }

    /// Return a cloned handle to the shared trace builder.
    #[must_use]
    pub fn log(&self) -> TraceLogBuilderRef {
        Rc::clone(&self.log)
    }
}

/// Errors produced by the trace recorder transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceTransportError<E> {
    /// Underlying transport error.
    Transport(E),
    /// Trace recording error.
    Trace(TraceError),
}

impl<E: fmt::Display> fmt::Display for TraceTransportError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceTransportError::Transport(err) => write!(f, "transport error: {err}"),
            TraceTransportError::Trace(err) => write!(f, "trace error: {err}"),
        }
    }
}

impl<T: crate::secure9p::Secure9pTransport> crate::secure9p::Secure9pTransport
    for TraceTransportRecorder<T>
{
    type Error = TraceTransportError<T::Error>;

    fn exchange(&mut self, batch: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let response = self
            .inner
            .exchange(batch)
            .map_err(TraceTransportError::Transport)?;
        self.log
            .borrow_mut()
            .record_frame(batch, &response)
            .map_err(TraceTransportError::Trace)?;
        Ok(response)
    }
}

/// Secure9P transport that replays recorded trace frames.
pub struct TraceReplayTransport {
    frames: Vec<TraceFrame>,
    index: usize,
}

impl TraceReplayTransport {
    /// Create a new replay transport from recorded frames.
    #[must_use]
    pub fn new(frames: Vec<TraceFrame>) -> Self {
        Self { frames, index: 0 }
    }
}

impl crate::secure9p::Secure9pTransport for TraceReplayTransport {
    type Error = TraceError;

    fn exchange(&mut self, batch: &[u8]) -> Result<Vec<u8>, Self::Error> {
        if self.index >= self.frames.len() {
            return Err(TraceError::ReplayExhausted);
        }
        let frame = &self.frames[self.index];
        if batch != frame.request.as_slice() {
            return Err(TraceError::RequestMismatch { index: self.index });
        }
        self.index = self.index.saturating_add(1);
        Ok(frame.response.clone())
    }
}

fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32, TraceError> {
    if *offset + 4 > data.len() {
        return Err(TraceError::Truncated);
    }
    let value = u32::from_le_bytes(
        data[*offset..*offset + 4]
            .try_into()
            .expect("length checked"),
    );
    *offset += 4;
    Ok(value)
}

fn read_bytes(data: &[u8], offset: &mut usize, len: usize) -> Result<Vec<u8>, TraceError> {
    if *offset + len > data.len() {
        return Err(TraceError::Truncated);
    }
    let slice = data[*offset..*offset + len].to_vec();
    *offset += len;
    Ok(slice)
}
