// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Secure9P wire types and codec primitives shared across Cohesix crates,
//! aligned with `docs/ARCHITECTURE.md` §2-§3 and the policy requirements in
//! `docs/SECURE9P.md`.

use std::fmt;
use std::io::{Cursor, Read};

/// Maximum message size negotiated by Secure9P.
pub const MAX_MSIZE: u32 = 8192;

/// Protocol version string.
pub const VERSION: &str = "9P2000.L";

/// Identifier for NineDoor sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    /// Bootstrap session identifier used during early bring-up.
    pub const BOOTSTRAP: SessionId = SessionId(0);

    /// Create a new session identifier from the supplied raw value.
    #[must_use]
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    /// Access the raw session identifier value.
    #[must_use]
    pub fn into_raw(self) -> u64 {
        self.0
    }

    /// Borrow the raw session identifier value.
    #[must_use]
    pub fn session(&self) -> u64 {
        self.0
    }
}

impl From<u64> for SessionId {
    fn from(value: u64) -> Self {
        Self::from_raw(value)
    }
}

/// Lightweight representation of a 9P frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    session: SessionId,
    payload_len: u32,
}

impl FrameHeader {
    /// Construct a new frame header for the provided session and payload length.
    #[must_use]
    pub fn new(session: impl Into<SessionId>, payload_len: u32) -> Self {
        Self {
            session: session.into(),
            payload_len,
        }
    }

    /// Retrieve the associated session identifier.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// Retrieve the payload length encoded in the header.
    #[must_use]
    pub fn payload_len(&self) -> u32 {
        self.payload_len
    }
}

/// Possible errors produced while encoding or decoding Secure9P messages.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodecError {
    /// Input buffer was shorter than the declared frame length.
    #[error("truncated frame")]
    Truncated,
    /// Encountered an unknown message type.
    #[error("unsupported message type {0}")]
    Unsupported(u8),
    /// Encountered malformed UTF-8 data.
    #[error("invalid utf8 in string field")]
    InvalidUtf8,
    /// Declared message size does not match the actual payload length.
    #[error("length mismatch: declared {declared} actual {actual}")]
    LengthMismatch {
        /// Message length declared in the frame header.
        declared: u32,
        /// Actual byte length observed in the payload.
        actual: usize,
    },
    /// Detected an invalid path component or walk depth beyond the limit.
    #[error("invalid path component")]
    InvalidPath,
    /// Invalid open mode flags were provided.
    #[error("invalid open mode {0}")]
    InvalidOpenMode(u8),
}

/// 9P message opcodes relevant to the current milestone.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageType {
    Tversion = 100,
    Rversion = 101,
    Tattach = 104,
    Rattach = 105,
    Twalk = 110,
    Rwalk = 111,
    Topen = 112,
    Ropen = 113,
    Tread = 116,
    Rread = 117,
    Twrite = 118,
    Rwrite = 119,
    Tclunk = 120,
    Rclunk = 121,
    Rerror = 107,
}

impl TryFrom<u8> for MessageType {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use MessageType::*;
        Ok(match value {
            100 => Tversion,
            101 => Rversion,
            104 => Tattach,
            105 => Rattach,
            110 => Twalk,
            111 => Rwalk,
            112 => Topen,
            113 => Ropen,
            116 => Tread,
            117 => Rread,
            118 => Twrite,
            119 => Rwrite,
            120 => Tclunk,
            121 => Rclunk,
            107 => Rerror,
            other => return Err(CodecError::Unsupported(other)),
        })
    }
}

/// Qid type bitflags per the 9P2000.L specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QidType(u8);

impl QidType {
    /// Directory bit.
    pub const DIRECTORY: Self = Self(0x80);
    /// Append-only bit.
    pub const APPEND_ONLY: Self = Self(0x40);
    /// Regular file.
    pub const FILE: Self = Self(0x00);

    fn as_u8(self) -> u8 {
        self.0
    }

    /// Check whether the Qid represents a directory.
    #[must_use]
    pub fn is_directory(self) -> bool {
        self.0 & Self::DIRECTORY.0 != 0
    }

    /// Check whether the Qid represents an append-only node.
    #[must_use]
    pub fn is_append_only(self) -> bool {
        self.0 & Self::APPEND_ONLY.0 != 0
    }
}

impl From<QidType> for u8 {
    fn from(value: QidType) -> Self {
        value.as_u8()
    }
}

/// 9P Qid descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Qid {
    ty: QidType,
    version: u32,
    path: u64,
}

impl Qid {
    /// Construct a new Qid.
    #[must_use]
    pub fn new(ty: QidType, version: u32, path: u64) -> Self {
        Self { ty, version, path }
    }

    /// Return the Qid type flags.
    #[must_use]
    pub fn ty(&self) -> QidType {
        self.ty
    }

    /// Return the Qid version field.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Return the Qid path field.
    #[must_use]
    pub fn path(&self) -> u64 {
        self.path
    }
}

/// Base open mode encoded in the low bits of the open mode field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpenModeBase {
    /// Open for reading.
    ReadOnly = 0,
    /// Open for writing.
    WriteOnly = 1,
    /// Open for reading and writing.
    ReadWrite = 2,
    /// Execute traversal.
    Execute = 3,
}

/// 9P open mode flags as a structured representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenMode {
    base: OpenModeBase,
    truncate: bool,
    append: bool,
}

impl OpenMode {
    /// Construct a read-only mode descriptor.
    #[must_use]
    pub fn read_only() -> Self {
        Self {
            base: OpenModeBase::ReadOnly,
            truncate: false,
            append: false,
        }
    }

    /// Construct a write-only append descriptor.
    #[must_use]
    pub fn write_append() -> Self {
        Self {
            base: OpenModeBase::WriteOnly,
            truncate: false,
            append: true,
        }
    }

    fn from_bits(value: u8) -> Result<Self, CodecError> {
        let base = match value & 0x03 {
            0 => OpenModeBase::ReadOnly,
            1 => OpenModeBase::WriteOnly,
            2 => OpenModeBase::ReadWrite,
            3 => OpenModeBase::Execute,
            _ => return Err(CodecError::InvalidOpenMode(value)),
        };
        Ok(Self {
            base,
            truncate: value & 0x10 != 0,
            append: value & 0x80 != 0,
        })
    }

    /// Determine if the mode permits reading.
    #[must_use]
    pub fn allows_read(self) -> bool {
        matches!(
            self.base,
            OpenModeBase::ReadOnly | OpenModeBase::ReadWrite | OpenModeBase::Execute
        )
    }

    /// Determine if the mode permits writing.
    #[must_use]
    pub fn allows_write(self) -> bool {
        matches!(self.base, OpenModeBase::WriteOnly | OpenModeBase::ReadWrite) || self.append
    }

    /// Check whether append-only behaviour is requested.
    #[must_use]
    pub fn is_append(self) -> bool {
        self.append
    }

    /// Expose the raw flag representation used on the wire.
    #[must_use]
    pub fn raw(self) -> u8 {
        let mut bits = self.base as u8;
        if self.truncate {
            bits |= 0x10;
        }
        if self.append {
            bits |= 0x80;
        }
        bits
    }
}

impl From<OpenMode> for u8 {
    fn from(value: OpenMode) -> Self {
        value.raw()
    }
}

/// Secure9P error codes surfaced to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Permission denied for the requested operation.
    Permission,
    /// Requested node was not found.
    NotFound,
    /// Requested resource is busy.
    Busy,
    /// Input data was invalid.
    Invalid,
    /// Frame exceeded negotiated size.
    TooBig,
    /// Fid was already closed.
    Closed,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::Permission => "Permission",
            Self::NotFound => "NotFound",
            Self::Busy => "Busy",
            Self::Invalid => "Invalid",
            Self::TooBig => "TooBig",
            Self::Closed => "Closed",
        };
        write!(f, "{code}")
    }
}

/// Request envelope containing a tag and message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Request identifier, echoed back by responses.
    pub tag: u16,
    /// The concrete request payload.
    pub body: RequestBody,
}

/// Response envelope containing a tag and message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Response identifier (mirrors the request tag).
    pub tag: u16,
    /// The concrete response payload.
    pub body: ResponseBody,
}

/// Request variants supported by the current milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestBody {
    /// `Tversion` negotiates the message size and version string.
    Version {
        /// Requested maximum message size.
        msize: u32,
        /// Protocol version string supplied by the client.
        version: String,
    },
    /// `Tattach` binds a fid to the session root.
    Attach {
        /// Fid identifier associated with the session root.
        fid: u32,
        /// Authentication fid (unused in Secure9P).
        afid: u32,
        /// User name string provided by the client.
        uname: String,
        /// Attachment name (namespace selector) supplied by the client.
        aname: String,
        /// Numeric user identifier.
        n_uname: u32,
    },
    /// `Twalk` traverses the namespace to produce a new fid.
    Walk {
        /// Source fid for the walk operation.
        fid: u32,
        /// Destination fid receiving the walk result.
        newfid: u32,
        /// Path components to traverse.
        wnames: Vec<String>,
    },
    /// `Topen` opens an existing fid.
    Open {
        /// Fid to open.
        fid: u32,
        /// Requested open mode flags.
        mode: OpenMode,
    },
    /// `Tread` reads bytes from an opened fid.
    Read {
        /// Fid to read from.
        fid: u32,
        /// Byte offset provided by the client.
        offset: u64,
        /// Number of bytes requested by the client.
        count: u32,
    },
    /// `Twrite` appends bytes to an opened fid.
    Write {
        /// Fid to write to.
        fid: u32,
        /// Byte offset provided by the client (ignored for append-only files).
        offset: u64,
        /// Payload to append to the file.
        data: Vec<u8>,
    },
    /// `Tclunk` closes and releases a fid.
    Clunk {
        /// Fid to release.
        fid: u32,
    },
}

/// Response variants supported by the current milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseBody {
    /// `Rversion` returning negotiated parameters.
    Version {
        /// Negotiated message size.
        msize: u32,
        /// Protocol version accepted by the server.
        version: String,
    },
    /// `Rattach` acknowledging the attach with a root Qid.
    Attach {
        /// Qid associated with the attached fid.
        qid: Qid,
    },
    /// `Rwalk` returning the resulting Qids.
    Walk {
        /// Qids for each traversed path component.
        qids: Vec<Qid>,
    },
    /// `Ropen` confirming an opened fid.
    Open {
        /// Qid describing the opened node.
        qid: Qid,
        /// Server-selected I/O unit size.
        iounit: u32,
    },
    /// `Rread` returning the requested bytes.
    Read {
        /// Bytes read from the fid.
        data: Vec<u8>,
    },
    /// `Rwrite` returning the committed byte count.
    Write {
        /// Number of bytes accepted by the server.
        count: u32,
    },
    /// `Rclunk` acknowledging fid release.
    Clunk,
    /// `Rerror` describing the failure condition.
    Error {
        /// Error code describing the failure class.
        code: ErrorCode,
        /// Human-readable error message supplied by the server.
        message: String,
    },
}

/// Codec responsible for encoding and decoding Secure9P messages.
#[derive(Debug, Default, Clone, Copy)]
pub struct Codec;

impl Codec {
    /// Encode a request into its wire representation.
    pub fn encode_request(&self, request: &Request) -> Result<Vec<u8>, CodecError> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&request.tag.to_le_bytes());
        match &request.body {
            RequestBody::Version { msize, version } => {
                payload.extend_from_slice(&msize.to_le_bytes());
                put_string(&mut payload, version);
                Ok(finish(MessageType::Tversion, &payload))
            }
            RequestBody::Attach {
                fid,
                afid,
                uname,
                aname,
                n_uname,
            } => {
                payload.extend_from_slice(&fid.to_le_bytes());
                payload.extend_from_slice(&afid.to_le_bytes());
                put_string(&mut payload, uname);
                put_string(&mut payload, aname);
                payload.extend_from_slice(&n_uname.to_le_bytes());
                Ok(finish(MessageType::Tattach, &payload))
            }
            RequestBody::Walk {
                fid,
                newfid,
                wnames,
            } => {
                payload.extend_from_slice(&fid.to_le_bytes());
                payload.extend_from_slice(&newfid.to_le_bytes());
                let count: u16 = wnames
                    .len()
                    .try_into()
                    .map_err(|_| CodecError::InvalidPath)?;
                if count as usize > 8 {
                    return Err(CodecError::InvalidPath);
                }
                payload.extend_from_slice(&count.to_le_bytes());
                for name in wnames {
                    validate_component(name)?;
                    put_string(&mut payload, name);
                }
                Ok(finish(MessageType::Twalk, &payload))
            }
            RequestBody::Open { fid, mode } => {
                payload.extend_from_slice(&fid.to_le_bytes());
                payload.push((*mode).into());
                Ok(finish(MessageType::Topen, &payload))
            }
            RequestBody::Read { fid, offset, count } => {
                payload.extend_from_slice(&fid.to_le_bytes());
                payload.extend_from_slice(&offset.to_le_bytes());
                payload.extend_from_slice(&count.to_le_bytes());
                Ok(finish(MessageType::Tread, &payload))
            }
            RequestBody::Write { fid, offset, data } => {
                payload.extend_from_slice(&fid.to_le_bytes());
                payload.extend_from_slice(&offset.to_le_bytes());
                let count: u32 = data
                    .len()
                    .try_into()
                    .map_err(|_| CodecError::LengthMismatch {
                        declared: u32::MAX,
                        actual: data.len(),
                    })?;
                payload.extend_from_slice(&count.to_le_bytes());
                payload.extend_from_slice(data);
                Ok(finish(MessageType::Twrite, &payload))
            }
            RequestBody::Clunk { fid } => {
                payload.extend_from_slice(&fid.to_le_bytes());
                Ok(finish(MessageType::Tclunk, &payload))
            }
        }
    }

    /// Encode a response into its wire representation.
    pub fn encode_response(&self, response: &Response) -> Result<Vec<u8>, CodecError> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&response.tag.to_le_bytes());
        match &response.body {
            ResponseBody::Version { msize, version } => {
                payload.extend_from_slice(&msize.to_le_bytes());
                put_string(&mut payload, version);
                Ok(finish(MessageType::Rversion, &payload))
            }
            ResponseBody::Attach { qid } => {
                put_qid(&mut payload, qid);
                Ok(finish(MessageType::Rattach, &payload))
            }
            ResponseBody::Walk { qids } => {
                let count: u16 = qids.len().try_into().map_err(|_| CodecError::InvalidPath)?;
                payload.extend_from_slice(&count.to_le_bytes());
                for qid in qids {
                    put_qid(&mut payload, qid);
                }
                Ok(finish(MessageType::Rwalk, &payload))
            }
            ResponseBody::Open { qid, iounit } => {
                put_qid(&mut payload, qid);
                payload.extend_from_slice(&iounit.to_le_bytes());
                Ok(finish(MessageType::Ropen, &payload))
            }
            ResponseBody::Read { data } => {
                let count: u32 = data
                    .len()
                    .try_into()
                    .map_err(|_| CodecError::LengthMismatch {
                        declared: u32::MAX,
                        actual: data.len(),
                    })?;
                payload.extend_from_slice(&count.to_le_bytes());
                payload.extend_from_slice(data);
                Ok(finish(MessageType::Rread, &payload))
            }
            ResponseBody::Write { count } => {
                payload.extend_from_slice(&count.to_le_bytes());
                Ok(finish(MessageType::Rwrite, &payload))
            }
            ResponseBody::Clunk => Ok(finish(MessageType::Rclunk, &payload)),
            ResponseBody::Error { code, message } => {
                put_string(&mut payload, &code.to_string());
                put_string(&mut payload, message);
                Ok(finish(MessageType::Rerror, &payload))
            }
        }
    }

    /// Decode a request from the wire representation.
    pub fn decode_request(&self, bytes: &[u8]) -> Result<Request, CodecError> {
        let (ty, payload) = decode_message(bytes)?;
        let mut cursor = Cursor::new(payload);
        let tag = read_u16(&mut cursor)?;
        let body = match ty {
            MessageType::Tversion => {
                let msize = read_u32(&mut cursor)?;
                let version = read_string(&mut cursor)?;
                RequestBody::Version { msize, version }
            }
            MessageType::Tattach => {
                let fid = read_u32(&mut cursor)?;
                let afid = read_u32(&mut cursor)?;
                let uname = read_string(&mut cursor)?;
                let aname = read_string(&mut cursor)?;
                let n_uname = read_u32(&mut cursor)?;
                RequestBody::Attach {
                    fid,
                    afid,
                    uname,
                    aname,
                    n_uname,
                }
            }
            MessageType::Twalk => {
                let fid = read_u32(&mut cursor)?;
                let newfid = read_u32(&mut cursor)?;
                let nwname = read_u16(&mut cursor)? as usize;
                if nwname > 8 {
                    return Err(CodecError::InvalidPath);
                }
                let mut wnames = Vec::with_capacity(nwname);
                for _ in 0..nwname {
                    let name = read_string(&mut cursor)?;
                    validate_component(&name)?;
                    wnames.push(name);
                }
                RequestBody::Walk {
                    fid,
                    newfid,
                    wnames,
                }
            }
            MessageType::Topen => {
                let fid = read_u32(&mut cursor)?;
                let raw_mode = read_u8(&mut cursor)?;
                let mode = OpenMode::from_bits(raw_mode)?;
                RequestBody::Open { fid, mode }
            }
            MessageType::Tread => {
                let fid = read_u32(&mut cursor)?;
                let offset = read_u64(&mut cursor)?;
                let count = read_u32(&mut cursor)?;
                RequestBody::Read { fid, offset, count }
            }
            MessageType::Twrite => {
                let fid = read_u32(&mut cursor)?;
                let offset = read_u64(&mut cursor)?;
                let count = read_u32(&mut cursor)? as usize;
                let mut data = vec![0u8; count];
                cursor
                    .read_exact(&mut data)
                    .map_err(|_| CodecError::Truncated)?;
                RequestBody::Write { fid, offset, data }
            }
            MessageType::Tclunk => {
                let fid = read_u32(&mut cursor)?;
                RequestBody::Clunk { fid }
            }
            other => return Err(CodecError::Unsupported(other as u8)),
        };
        Ok(Request { tag, body })
    }

    /// Decode a response from the wire representation.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<Response, CodecError> {
        let (ty, payload) = decode_message(bytes)?;
        let mut cursor = Cursor::new(payload);
        let tag = read_u16(&mut cursor)?;
        let body = match ty {
            MessageType::Rversion => {
                let msize = read_u32(&mut cursor)?;
                let version = read_string(&mut cursor)?;
                ResponseBody::Version { msize, version }
            }
            MessageType::Rattach => {
                let qid = read_qid(&mut cursor)?;
                ResponseBody::Attach { qid }
            }
            MessageType::Rwalk => {
                let count = read_u16(&mut cursor)? as usize;
                let mut qids = Vec::with_capacity(count);
                for _ in 0..count {
                    qids.push(read_qid(&mut cursor)?);
                }
                ResponseBody::Walk { qids }
            }
            MessageType::Ropen => {
                let qid = read_qid(&mut cursor)?;
                let iounit = read_u32(&mut cursor)?;
                ResponseBody::Open { qid, iounit }
            }
            MessageType::Rread => {
                let count = read_u32(&mut cursor)? as usize;
                let mut data = vec![0u8; count];
                cursor
                    .read_exact(&mut data)
                    .map_err(|_| CodecError::Truncated)?;
                ResponseBody::Read { data }
            }
            MessageType::Rwrite => {
                let count = read_u32(&mut cursor)?;
                ResponseBody::Write { count }
            }
            MessageType::Rclunk => ResponseBody::Clunk,
            MessageType::Rerror => {
                let code_str = read_string(&mut cursor)?;
                let message = read_string(&mut cursor)?;
                let code = match code_str.as_str() {
                    "Permission" => ErrorCode::Permission,
                    "NotFound" => ErrorCode::NotFound,
                    "Busy" => ErrorCode::Busy,
                    "Invalid" => ErrorCode::Invalid,
                    "TooBig" => ErrorCode::TooBig,
                    "Closed" => ErrorCode::Closed,
                    _ => return Err(CodecError::InvalidUtf8),
                };
                ResponseBody::Error { code, message }
            }
            other => return Err(CodecError::Unsupported(other as u8)),
        };
        Ok(Response { tag, body })
    }
}

fn finish(ty: MessageType, payload: &[u8]) -> Vec<u8> {
    let size = payload
        .len()
        .checked_add(5)
        .expect("payload length overflow");
    let mut buffer = Vec::with_capacity(size);
    buffer.extend_from_slice(&(size as u32).to_le_bytes());
    buffer.push(ty as u8);
    buffer.extend_from_slice(payload);
    buffer
}

fn decode_message(bytes: &[u8]) -> Result<(MessageType, &[u8]), CodecError> {
    if bytes.len() < 5 {
        return Err(CodecError::Truncated);
    }
    let declared = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if declared as usize != bytes.len() {
        return Err(CodecError::LengthMismatch {
            declared,
            actual: bytes.len(),
        });
    }
    let ty = MessageType::try_from(bytes[4])?;
    Ok((ty, &bytes[5..]))
}

fn put_string(buffer: &mut Vec<u8>, value: &str) {
    let len = value.len() as u16;
    buffer.extend_from_slice(&len.to_le_bytes());
    buffer.extend_from_slice(value.as_bytes());
}

fn put_qid(buffer: &mut Vec<u8>, qid: &Qid) {
    buffer.push(qid.ty.as_u8());
    buffer.extend_from_slice(&qid.version.to_le_bytes());
    buffer.extend_from_slice(&qid.path.to_le_bytes());
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, CodecError> {
    let mut buf = [0u8; 1];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(buf[0])
}

fn read_u16(cursor: &mut Cursor<&[u8]>) -> Result<u16, CodecError> {
    let mut buf = [0u8; 2];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, CodecError> {
    let mut buf = [0u8; 4];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, CodecError> {
    let mut buf = [0u8; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(u64::from_le_bytes(buf))
}

fn read_string(cursor: &mut Cursor<&[u8]>) -> Result<String, CodecError> {
    let len = read_u16(cursor)? as usize;
    let mut buf = vec![0u8; len];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    String::from_utf8(buf).map_err(|_| CodecError::InvalidUtf8)
}

fn read_qid(cursor: &mut Cursor<&[u8]>) -> Result<Qid, CodecError> {
    let ty = QidType(read_u8(cursor)?);
    let version = read_u32(cursor)?;
    let path = read_u64(cursor)?;
    Ok(Qid { ty, version, path })
}

fn validate_component(component: &str) -> Result<(), CodecError> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.as_bytes().contains(&0)
    {
        return Err(CodecError::InvalidPath);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_round_trips() {
        let header = FrameHeader::new(SessionId::from_raw(4), 128);
        assert_eq!(header.session().into_raw(), 4);
        assert_eq!(header.payload_len(), 128);
    }

    #[test]
    fn encode_decode_version_round_trip() {
        let codec = Codec;
        let request = Request {
            tag: 1,
            body: RequestBody::Version {
                msize: MAX_MSIZE,
                version: VERSION.to_string(),
            },
        };
        let encoded = codec.encode_request(&request).unwrap();
        let decoded = codec.decode_request(&encoded).unwrap();
        assert_eq!(request, decoded);
    }

    #[test]
    fn decode_rejects_length_mismatch() {
        let codec = Codec;
        let request = Request {
            tag: 2,
            body: RequestBody::Clunk { fid: 1 },
        };
        let mut encoded = codec.encode_request(&request).unwrap();
        encoded[0] = 0;
        encoded[1] = 0;
        encoded[2] = 0;
        encoded[3] = 0;
        assert_eq!(
            codec.decode_request(&encoded),
            Err(CodecError::LengthMismatch {
                declared: 0,
                actual: encoded.len(),
            })
        );
    }

    #[test]
    fn decode_rejects_invalid_component() {
        let codec = Codec;
        let mut frame = Vec::new();
        frame.extend_from_slice(&[0u8; 4]);
        frame.push(110); // Twalk opcode
        frame.extend_from_slice(&7u16.to_le_bytes());
        frame.extend_from_slice(&1u32.to_le_bytes());
        frame.extend_from_slice(&2u32.to_le_bytes());
        frame.extend_from_slice(&1u16.to_le_bytes());
        frame.extend_from_slice(&2u16.to_le_bytes());
        frame.extend_from_slice(b"..");
        let size = frame.len() as u32;
        frame[0..4].copy_from_slice(&size.to_le_bytes());
        assert_eq!(codec.decode_request(&frame), Err(CodecError::InvalidPath));
    }

    #[test]
    fn decode_request_reports_truncated_payload() {
        let codec = Codec;
        let mut frame = Vec::new();
        frame.extend_from_slice(&[0u8; 4]);
        frame.push(110); // Twalk opcode
        frame.extend_from_slice(&1u16.to_le_bytes()); // tag
        frame.extend_from_slice(&1u32.to_le_bytes()); // fid
        frame.extend_from_slice(&2u32.to_le_bytes()); // newfid
        frame.extend_from_slice(&1u16.to_le_bytes()); // one path component
        frame.extend_from_slice(&5u16.to_le_bytes()); // declared length 5
        frame.extend_from_slice(b"abc"); // missing two bytes
        let size = frame.len() as u32;
        frame[0..4].copy_from_slice(&size.to_le_bytes());
        assert_eq!(codec.decode_request(&frame), Err(CodecError::Truncated));
    }

    #[test]
    fn decode_response_rejects_unknown_error_code() {
        let codec = Codec;
        let response = Response {
            tag: 41,
            body: ResponseBody::Error {
                code: ErrorCode::Permission,
                message: "not used".to_owned(),
            },
        };
        let mut encoded = codec.encode_response(&response).unwrap();
        // Overwrite the error code string with a value not recognised by the decoder.
        // Layout: size (4) | type (1) | tag (2) | strlen (2) | str bytes | ...
        // After encoding, replace the code with "Strange".
        let code_len_offset = 5 + 2; // skip size/type/tag
        encoded[code_len_offset..code_len_offset + 2].copy_from_slice(&(7u16).to_le_bytes());
        encoded.splice(
            (code_len_offset + 2)..(code_len_offset + 2 + "Permission".len()),
            b"Strange".to_vec(),
        );
        // Update outer length to match modified payload.
        let declared = encoded.len() as u32;
        encoded[0..4].copy_from_slice(&declared.to_le_bytes());
        assert_eq!(
            codec.decode_response(&encoded),
            Err(CodecError::InvalidUtf8)
        );
    }
}
