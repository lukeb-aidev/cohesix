// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Implement the bounded isolated NineDoor namespace parser state machine.
// Author: Lukas Bower
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Pure-Rust isolated namespace parser used by the target NineDoor child.
//!
//! This crate prepares typed operations only. It has no Queen policy, namespace
//! mutation authority, TCP listener, SchedControl cap, or root CSpace access.

use secure9p_transport::{
    prepare_namespace_operation, NamespaceRequestHeader, PreparedNamespaceOperation, RequestToken,
    TransportError, NAMESPACE_OPERATION_FRAME_BYTES, NAMESPACE_PATH_MAX, NAMESPACE_PAYLOAD_MAX,
    NAMESPACE_PREPARED_LABEL, NAMESPACE_REJECTED_LABEL, NAMESPACE_REQUEST_LABEL,
    NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES, NAMESPACE_RUNTIME_INIT_VERSION,
    NAMESPACE_SERVICE_ENDPOINT_SLOT, NAMESPACE_SERVICE_REPLY_SLOT, NAMESPACE_SHARED_FRAME_BYTES,
};

pub use secure9p_transport::NamespaceRuntimeInitDescriptor as RuntimeInitDescriptor;

/// Runtime-init descriptor version.
pub const RUNTIME_INIT_VERSION: u16 = NAMESPACE_RUNTIME_INIT_VERSION;
/// Fixed pointer-free runtime-init descriptor size.
pub const RUNTIME_INIT_DESCRIPTOR_BYTES: usize = NAMESPACE_RUNTIME_INIT_DESCRIPTOR_BYTES;
/// IPC label used for a namespace preparation request.
pub const REQUEST_LABEL: u64 = NAMESPACE_REQUEST_LABEL;
/// IPC label used for a successful prepared response.
pub const PREPARED_LABEL: u64 = NAMESPACE_PREPARED_LABEL;
/// IPC label used for a rejected request.
pub const REJECTED_LABEL: u64 = NAMESPACE_REJECTED_LABEL;
/// Fixed child CSpace slot holding the root-call endpoint.
pub const SERVICE_ENDPOINT_SLOT: u64 = NAMESPACE_SERVICE_ENDPOINT_SLOT;
/// Fixed child CSpace slot holding the MCS Reply object.
pub const SERVICE_REPLY_SLOT: u64 = NAMESPACE_SERVICE_REPLY_SLOT;
/// Maximum shared request or response frame bytes.
pub const SERVICE_FRAME_BYTES: usize = NAMESPACE_OPERATION_FRAME_BYTES;
/// One mapped page pair spans two 4 KiB pages per shared frame.
pub const SERVICE_SHARED_FRAME_BYTES: usize = NAMESPACE_SHARED_FRAME_BYTES;

/// Parser runtime state for one immutable supervisor generation.
#[derive(Debug)]
pub struct NamespaceRuntime {
    generation: u64,
    last_sequence: u64,
    cancelled_sequence: Option<u64>,
    revoked: bool,
}

impl NamespaceRuntime {
    /// Construct runtime state for a nonzero generation.
    pub const fn new(generation: u64) -> Result<Self, TransportError> {
        if generation == 0 {
            return Err(TransportError::StaleIdentity);
        }
        Ok(Self {
            generation,
            last_sequence: 0,
            cancelled_sequence: None,
            revoked: false,
        })
    }

    /// Prepare one exact request from the mapped shared request frame.
    pub fn prepare(
        &mut self,
        header: NamespaceRequestHeader,
        bytes: &[u8],
    ) -> Result<PreparedNamespaceOperation, TransportError> {
        if self.revoked {
            return Err(TransportError::Revoked);
        }
        let token = header.token()?;
        if token.generation != self.generation || token.sequence <= self.last_sequence {
            return Err(TransportError::StaleIdentity);
        }
        self.last_sequence = token.sequence;
        if self.cancelled_sequence == Some(token.sequence) {
            self.cancelled_sequence = None;
            return Err(TransportError::UnknownRequest);
        }
        let prepared = prepare_namespace_operation::<NAMESPACE_PATH_MAX, NAMESPACE_PAYLOAD_MAX>(
            header, bytes,
        )?;
        prepared.validate_identity(token)?;
        Ok(prepared)
    }

    /// Cancel one not-yet-prepared request in this generation.
    pub fn cancel(&mut self, token: RequestToken) -> Result<(), TransportError> {
        if self.revoked {
            return Err(TransportError::Revoked);
        }
        if token.generation != self.generation || token.sequence <= self.last_sequence {
            return Err(TransportError::StaleIdentity);
        }
        self.cancelled_sequence = Some(token.sequence);
        Ok(())
    }

    /// Revoke the child generation. Revocation is terminal for this instance.
    pub fn revoke(&mut self) {
        self.cancelled_sequence = None;
        self.revoked = true;
    }

    /// Return the most recently consumed request sequence.
    #[must_use]
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secure9p_transport::{
        NamespaceOpcode, NAMESPACE_CHILD_RECEIVE_RIGHTS, NAMESPACE_ROOT_CALL_RIGHTS,
        SEL4_RIGHTS_READ, SEL4_RIGHTS_READ_WRITE,
    };
    use std::vec::Vec;

    extern crate std;

    fn request(
        opcode: NamespaceOpcode,
        token: RequestToken,
        path: &str,
        payload: &str,
    ) -> (NamespaceRequestHeader, Vec<u8>) {
        let header = NamespaceRequestHeader::new(opcode, token, path.len(), payload.len()).unwrap();
        let mut bytes = path.as_bytes().to_vec();
        bytes.extend_from_slice(payload.as_bytes());
        (header, bytes)
    }

    #[test]
    fn descriptor_rejects_aliasing_and_broad_cap_slots() {
        let valid = RuntimeInitDescriptor {
            version: RUNTIME_INIT_VERSION,
            descriptor_bytes: core::mem::size_of::<RuntimeInitDescriptor>() as u16,
            endpoint_cap_rights: NAMESPACE_CHILD_RECEIVE_RIGHTS,
            request_frame_rights: SEL4_RIGHTS_READ,
            response_frame_rights: SEL4_RIGHTS_READ_WRITE,
            reserved_rights: 0,
            generation: 7,
            request_frame_vaddr: 0x1000,
            response_frame_vaddr: 0x3000,
            frame_bytes: SERVICE_SHARED_FRAME_BYTES as u32,
            request_badge: 1,
            endpoint_cptr: SERVICE_ENDPOINT_SLOT,
            reply_cptr: SERVICE_REPLY_SLOT,
            reserved: [0; 2],
        };
        assert!(valid.valid());
        assert!(!RuntimeInitDescriptor {
            response_frame_vaddr: valid.request_frame_vaddr,
            ..valid
        }
        .valid());
        assert!(!RuntimeInitDescriptor {
            endpoint_cptr: 9,
            ..valid
        }
        .valid());
        assert!(!RuntimeInitDescriptor {
            endpoint_cap_rights: NAMESPACE_ROOT_CALL_RIGHTS,
            ..valid
        }
        .valid());
    }

    #[test]
    fn prepare_returns_typed_operation_and_rejects_replay() {
        let token = RequestToken::new(1, 7).unwrap();
        let (header, bytes) = request(
            NamespaceOpcode::Echo,
            token,
            "/queen/ctl",
            r#"{"spawn":"heart"}"#,
        );
        let mut runtime = NamespaceRuntime::new(7).unwrap();
        let prepared = runtime.prepare(header, &bytes).unwrap();
        assert_eq!(prepared.path().unwrap(), "/queen/ctl");
        assert_eq!(
            runtime.prepare(header, &bytes),
            Err(TransportError::StaleIdentity)
        );
    }

    #[test]
    fn malformed_path_is_contained_and_sequence_is_retired() {
        let token = RequestToken::new(1, 7).unwrap();
        let (header, bytes) = request(NamespaceOpcode::Cat, token, "/proc/../queen", "");
        let mut runtime = NamespaceRuntime::new(7).unwrap();
        assert_eq!(
            runtime.prepare(header, &bytes),
            Err(TransportError::InvalidPath)
        );
        assert_eq!(runtime.last_sequence(), 1);
        assert_eq!(
            runtime.prepare(header, &bytes),
            Err(TransportError::StaleIdentity)
        );
    }

    #[test]
    fn cancellation_and_revoke_fail_closed() {
        let mut runtime = NamespaceRuntime::new(7).unwrap();
        let token = RequestToken::new(1, 7).unwrap();
        runtime.cancel(token).unwrap();
        let (header, bytes) = request(NamespaceOpcode::Cat, token, "/proc/boot", "");
        assert_eq!(
            runtime.prepare(header, &bytes),
            Err(TransportError::UnknownRequest)
        );
        runtime.revoke();
        let token = RequestToken::new(2, 7).unwrap();
        let (header, bytes) = request(NamespaceOpcode::Cat, token, "/proc/boot", "");
        assert_eq!(
            runtime.prepare(header, &bytes),
            Err(TransportError::Revoked)
        );
    }
}
