// CLASSIFICATION: COMMUNITY
// Filename: protocol.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// Core 9P protocol definitions for Cohesix.
/// Defines message types, tags, and stub handlers for 9P wire operations.

/// 9P message types.
#[derive(Debug)]
#[allow(dead_code)]
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
#[allow(dead_code)]
pub type Tag = u16;

/// Very small parser for demo purposes. Expects the first byte to encode the
/// message type and ignores any payload.
pub fn parse_message(buf: &[u8]) -> P9Message {
    match buf.first().copied() {
        Some(0x6f) => P9Message::Tversion,
        Some(0x70) => P9Message::Rversion,
        Some(0x01) => P9Message::Tattach,
        Some(0x02) => P9Message::Rattach,
        Some(0x03) => P9Message::Twalk,
        Some(0x04) => P9Message::Rwalk,
        Some(0x05) => P9Message::Topen,
        Some(0x06) => P9Message::Ropen,
        Some(0x07) => P9Message::Tread,
        Some(0x08) => P9Message::Rread,
        Some(0x09) => P9Message::Twrite,
        Some(0x0a) => P9Message::Rwrite,
        Some(0x0b) => P9Message::Tclunk,
        Some(0x0c) => P9Message::Rclunk,
        Some(0x0d) => P9Message::Tstat,
        Some(0x0e) => P9Message::Rstat,
        Some(other) => P9Message::Unknown(other),
        None => P9Message::Unknown(0xff),
    }
}

/// Serialize a message using the same tiny demo format where the first byte is
/// the variant discriminator.
pub fn serialize_message(msg: &P9Message) -> Vec<u8> {
    let tag = match msg {
        P9Message::Tversion => 0x6f,
        P9Message::Rversion => 0x70,
        P9Message::Tattach => 0x01,
        P9Message::Rattach => 0x02,
        P9Message::Twalk => 0x03,
        P9Message::Rwalk => 0x04,
        P9Message::Topen => 0x05,
        P9Message::Ropen => 0x06,
        P9Message::Tread => 0x07,
        P9Message::Rread => 0x08,
        P9Message::Twrite => 0x09,
        P9Message::Rwrite => 0x0a,
        P9Message::Tclunk => 0x0b,
        P9Message::Rclunk => 0x0c,
        P9Message::Tstat => 0x0d,
        P9Message::Rstat => 0x0e,
        P9Message::Unknown(v) => *v,
        _ => 0xff,
    };
    vec![tag]
}

