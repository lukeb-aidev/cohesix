// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Secure9P wire types referenced by workspace crates, aligned with
//! `docs/ARCHITECTURE.md` §2-§3.

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

impl From<u64> for SessionId {
    fn from(value: u64) -> Self {
        Self::from_raw(value)
    }
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
}
