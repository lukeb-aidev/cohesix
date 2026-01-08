// Author: Lukas Bower
// Purpose: Define Secure9P wire types and constants shared across components.
#![allow(clippy::module_name_repetitions)]

//! Secure9P data model definitions shared across codec backends.

use core::fmt;

use alloc::string::String;
use alloc::vec::Vec;

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

    pub(crate) fn from_raw(value: u8) -> Self {
        Self(value)
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

    pub(crate) fn from_bits(value: u8) -> Result<Self, CodecError> {
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
        /// Path components supplied by the client.
        wnames: Vec<String>,
    },
    /// `Topen` opens a fid for subsequent I/O operations.
    Open {
        /// Fid to open.
        fid: u32,
        /// Requested open mode.
        mode: OpenMode,
    },
    /// `Tread` reads a range of bytes from a fid.
    Read {
        /// Fid to read from.
        fid: u32,
        /// Offset into the file.
        offset: u64,
        /// Number of bytes requested.
        count: u32,
    },
    /// `Twrite` writes bytes to a fid.
    Write {
        /// Fid to write to.
        fid: u32,
        /// Offset within the file.
        offset: u64,
        /// Payload bytes supplied by the client.
        data: Vec<u8>,
    },
    /// `Tclunk` closes a fid.
    Clunk {
        /// Fid identifier to close.
        fid: u32,
    },
}

/// Response variants surfaced to clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseBody {
    /// Response to `Tversion` carrying the negotiated size and version.
    Version {
        /// Negotiated maximum message size.
        msize: u32,
        /// Protocol version string.
        version: String,
    },
    /// Response to `Tattach` containing the root Qid.
    Attach {
        /// Qid associated with the session root.
        qid: Qid,
    },
    /// Response to `Twalk` containing the traversed Qids.
    Walk {
        /// Qids encountered during the walk.
        qids: Vec<Qid>,
    },
    /// Response to `Topen` containing the opened Qid and I/O unit size.
    Open {
        /// Qid associated with the opened fid.
        qid: Qid,
        /// Maximum I/O payload size.
        iounit: u32,
    },
    /// Response to `Tread` containing the payload bytes.
    Read {
        /// Data payload read from the fid.
        data: Vec<u8>,
    },
    /// Response to `Twrite` containing the write count.
    Write {
        /// Number of bytes written.
        count: u32,
    },
    /// Response to `Tclunk` acknowledging the closure.
    Clunk,
    /// Error response containing a Secure9P error code and message.
    Error {
        /// Secure9P error code propagated to the client.
        code: ErrorCode,
        /// Human-readable message describing the error.
        message: String,
    },
}
