// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide Secure9P client transport primitives for host tooling.
// Author: Lukas Bower
#![allow(clippy::module_name_repetitions)]

//! Secure9P client transport helpers shared by Cohesix host tooling.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
use cohesix_ticket::Role;
use secure9p_codec::{
    Codec, CodecError, ErrorCode, OpenMode, Qid, Request, RequestBody, ResponseBody, VERSION,
};

/// Transport abstraction that exchanges Secure9P frame batches.
pub trait Secure9pTransport {
    /// Error type returned by the transport backend.
    type Error: fmt::Display;
    /// Send a batch of request frames and return the raw response batch.
    fn exchange(&mut self, batch: &[u8]) -> Result<Vec<u8>, Self::Error>;
}

/// Errors surfaced by the Secure9P client.
#[derive(Debug, PartialEq, Eq)]
pub enum ClientError<E> {
    /// Transport-level failure.
    Transport(E),
    /// Codec failure encoding or decoding frames.
    Codec(CodecError),
    /// Protocol-visible error response.
    Protocol {
        /// Secure9P error code.
        code: ErrorCode,
        /// Error message supplied by the server.
        message: String,
    },
    /// Unexpected response tag returned by the transport.
    UnexpectedTag {
        /// Expected response tag.
        expected: u16,
        /// Actual response tag.
        got: u16,
    },
    /// Response body does not match the expected variant.
    UnexpectedResponse(&'static str),
}

impl<E> From<CodecError> for ClientError<E> {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

impl<E: fmt::Display> fmt::Display for ClientError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Codec(err) => write!(f, "codec error: {err}"),
            Self::Protocol { code, message } => write!(f, "{code}: {message}"),
            Self::UnexpectedTag { expected, got } => {
                write!(f, "unexpected response tag {got} (expected {expected})")
            }
            Self::UnexpectedResponse(label) => write!(f, "unexpected response {label}"),
        }
    }
}

/// Minimal Secure9P client used by host tooling.
pub struct Secure9pClient<T> {
    transport: T,
    codec: Codec,
    next_tag: u16,
    negotiated_msize: u32,
}

impl<T> Secure9pClient<T> {
    /// Return the negotiated maximum message size.
    #[must_use]
    pub fn negotiated_msize(&self) -> u32 {
        self.negotiated_msize
    }
}

impl<T> Secure9pClient<T>
where
    T: Secure9pTransport,
{
    /// Create a new Secure9P client backed by the supplied transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            codec: Codec,
            next_tag: 1,
            negotiated_msize: secure9p_codec::MAX_MSIZE,
        }
    }

    /// Negotiate Secure9P version and maximum message size.
    pub fn version(&mut self, requested_msize: u32) -> Result<u32, ClientError<T::Error>> {
        let response = self.transact(RequestBody::Version {
            msize: requested_msize,
            version: VERSION.to_string(),
        })?;
        let ResponseBody::Version { msize, version } = response else {
            return Err(ClientError::UnexpectedResponse("Rversion"));
        };
        if version != VERSION {
            return Err(ClientError::Protocol {
                code: ErrorCode::Invalid,
                message: format!("unexpected version {version}"),
            });
        }
        self.negotiated_msize = msize;
        Ok(msize)
    }

    /// Attach to the namespace using the supplied fid and role.
    pub fn attach(
        &mut self,
        fid: u32,
        role: Role,
        identity: Option<&str>,
        ticket: Option<&str>,
    ) -> Result<Qid, ClientError<T::Error>> {
        let uname = role_to_uname(role, identity)?;
        let response = self.transact(RequestBody::Attach {
            fid,
            afid: u32::MAX,
            uname,
            aname: ticket.unwrap_or("").to_string(),
            n_uname: 0,
        })?;
        let ResponseBody::Attach { qid } = response else {
            return Err(ClientError::UnexpectedResponse("Rattach"));
        };
        Ok(qid)
    }

    /// Walk from `fid` to `newfid` following the supplied path components.
    pub fn walk(
        &mut self,
        fid: u32,
        newfid: u32,
        path: &[String],
    ) -> Result<Vec<Qid>, ClientError<T::Error>> {
        let response = self.transact(RequestBody::Walk {
            fid,
            newfid,
            wnames: path.to_vec(),
        })?;
        let ResponseBody::Walk { qids } = response else {
            return Err(ClientError::UnexpectedResponse("Rwalk"));
        };
        Ok(qids)
    }

    /// Open the fid using the provided mode.
    pub fn open(
        &mut self,
        fid: u32,
        mode: OpenMode,
    ) -> Result<(Qid, u32), ClientError<T::Error>> {
        let response = self.transact(RequestBody::Open { fid, mode })?;
        let ResponseBody::Open { qid, iounit } = response else {
            return Err(ClientError::UnexpectedResponse("Ropen"));
        };
        Ok((qid, iounit))
    }

    /// Read data from the fid at the supplied offset.
    pub fn read(
        &mut self,
        fid: u32,
        offset: u64,
        count: u32,
    ) -> Result<Vec<u8>, ClientError<T::Error>> {
        let response = self.transact(RequestBody::Read { fid, offset, count })?;
        let ResponseBody::Read { data } = response else {
            return Err(ClientError::UnexpectedResponse("Rread"));
        };
        Ok(data)
    }

    /// Write data to the fid at the supplied offset.
    pub fn write(
        &mut self,
        fid: u32,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, ClientError<T::Error>> {
        let response = self.transact(RequestBody::Write {
            fid,
            offset,
            data: data.to_vec(),
        })?;
        let ResponseBody::Write { count } = response else {
            return Err(ClientError::UnexpectedResponse("Rwrite"));
        };
        Ok(count)
    }

    /// Clunk the supplied fid.
    pub fn clunk(&mut self, fid: u32) -> Result<(), ClientError<T::Error>> {
        let response = self.transact(RequestBody::Clunk { fid })?;
        let ResponseBody::Clunk = response else {
            return Err(ClientError::UnexpectedResponse("Rclunk"));
        };
        Ok(())
    }

    fn next_tag(&mut self) -> u16 {
        let tag = self.next_tag;
        self.next_tag = self.next_tag.wrapping_add(1);
        tag
    }

    fn transact(
        &mut self,
        body: RequestBody,
    ) -> Result<ResponseBody, ClientError<T::Error>> {
        let tag = self.next_tag();
        let request = Request { tag, body };
        let encoded = self.codec.encode_request(&request)?;
        let response_bytes = self
            .transport
            .exchange(&encoded)
            .map_err(ClientError::Transport)?;
        let response = self.codec.decode_response(&response_bytes)?;
        if response.tag != tag {
            return Err(ClientError::UnexpectedTag {
                expected: tag,
                got: response.tag,
            });
        }
        match response.body {
            ResponseBody::Error { code, message } => Err(ClientError::Protocol { code, message }),
            other => Ok(other),
        }
    }
}

fn role_to_uname<E>(role: Role, identity: Option<&str>) -> Result<String, ClientError<E>> {
    match role {
        Role::Queen => Ok(proto_role_label(ProtoRole::Queen).to_string()),
        Role::WorkerHeartbeat => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| ClientError::Protocol {
                    code: ErrorCode::Invalid,
                    message: "worker-heartbeat attach requires identity".to_string(),
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::Worker)))
        }
        Role::WorkerGpu => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| ClientError::Protocol {
                    code: ErrorCode::Invalid,
                    message: "worker-gpu attach requires identity".to_string(),
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::GpuWorker)))
        }
        Role::WorkerBus => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| ClientError::Protocol {
                    code: ErrorCode::Invalid,
                    message: "worker-bus attach requires identity".to_string(),
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::BusWorker)))
        }
        Role::WorkerLora => {
            let id = identity
                .and_then(|value| (!value.is_empty()).then_some(value))
                .ok_or_else(|| ClientError::Protocol {
                    code: ErrorCode::Invalid,
                    message: "worker-lora attach requires identity".to_string(),
                })?;
            Ok(format!("{}:{id}", proto_role_label(ProtoRole::LoraWorker)))
        }
    }
}
