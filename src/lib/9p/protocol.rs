// CLASSIFICATION: COMMUNITY
// Filename: protocol.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Core 9P protocol definitions for Cohesix.
//! Defines message types, tags, and stub handlers for 9P wire operations.

/// 9P message types.
#[derive(Debug)]
pub enum P9Message {
    Tversion,
    Rversion,
    Tauth,
    Rauth,
    Tattach,
    Rattach,
    Tflush,
    Rflush,
    Twalk,
    Rwalk,
    Topen,
    Ropen,
    Tread,
    Rread,
    Twrite,
    Rwrite,
    Tclunk,
    Rclunk,
    Tremove,
    Rremove,
    Tstat,
    Rstat,
    Twstat,
    Rwstat,
    Unknown(u8),
}

/// 9P tag used to match request/response pairs.
pub type Tag = u16;

/// Stub: Parse a raw 9P message into a typed enum.
pub fn parse_message(_buf: &[u8]) -> P9Message {
    // TODO(cohesix): Implement 9P wire format parsing
    P9Message::Unknown(0xff)
}

/// Stub: Serialize a 9P message into bytes.
pub fn serialize_message(_msg: &P9Message) -> Vec<u8> {
    // TODO(cohesix): Implement 9P wire format serialization
    vec![]
}
