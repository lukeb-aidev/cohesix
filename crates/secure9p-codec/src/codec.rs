// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Encode and decode Secure9P wire messages without std dependencies.
// Author: Lukas Bower

//! Encode/decode helpers for Secure9P wire messages.

use alloc::borrow::ToOwned;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::str;

use crate::types::*;

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

/// Encode/decode helper used by NineDoor integration tests.
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

/// Encode a request into a Secure9P wire frame using the default codec.
pub fn encode_request(req: &Request) -> Result<Vec<u8>, CodecError> {
    Codec.encode_request(req)
}

/// Encode a response into a Secure9P wire frame using the default codec.
pub fn encode_response(res: &Response) -> Result<Vec<u8>, CodecError> {
    Codec.encode_response(res)
}

/// Decode a request from a Secure9P wire frame using the default codec.
pub fn decode_request(bytes: &[u8]) -> Result<Request, CodecError> {
    Codec.decode_request(bytes)
}

/// Decode a response from a Secure9P wire frame using the default codec.
pub fn decode_response(bytes: &[u8]) -> Result<Response, CodecError> {
    Codec.decode_response(bytes)
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
    let declared = u32::from_le_bytes(bytes[..4].try_into().expect("slice length checked"));
    let actual: u32 = bytes
        .len()
        .try_into()
        .map_err(|_| CodecError::LengthMismatch {
            declared,
            actual: bytes.len(),
        })?;
    if declared != actual {
        return Err(CodecError::LengthMismatch {
            declared,
            actual: bytes.len(),
        });
    }
    let ty = MessageType::try_from(bytes[4])?;
    Ok((ty, &bytes[5..]))
}

fn read_u8(cursor: &mut Cursor<'_>) -> Result<u8, CodecError> {
    let mut buf = [0u8; 1];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(buf[0])
}

fn read_u16(cursor: &mut Cursor<'_>) -> Result<u16, CodecError> {
    let mut buf = [0u8; 2];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u32(cursor: &mut Cursor<'_>) -> Result<u32, CodecError> {
    let mut buf = [0u8; 4];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(cursor: &mut Cursor<'_>) -> Result<u64, CodecError> {
    let mut buf = [0u8; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    Ok(u64::from_le_bytes(buf))
}

fn read_string(cursor: &mut Cursor<'_>) -> Result<String, CodecError> {
    let len = read_u16(cursor)? as usize;
    let mut buf = vec![0u8; len];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| CodecError::Truncated)?;
    let text = str::from_utf8(&buf).map_err(|_| CodecError::InvalidUtf8)?;
    Ok(text.to_owned())
}

fn read_qid(cursor: &mut Cursor<'_>) -> Result<Qid, CodecError> {
    let ty = QidType::from_raw(read_u8(cursor)?);
    let version = read_u32(cursor)?;
    let path = read_u64(cursor)?;
    Ok(Qid::new(ty, version, path))
}

fn validate_component(component: &str) -> Result<(), CodecError> {
    if component.is_empty() || component.len() > 64 || component.contains('/') {
        return Err(CodecError::InvalidPath);
    }
    Ok(())
}

fn put_qid(buffer: &mut Vec<u8>, qid: &Qid) {
    buffer.push(qid.ty().into());
    buffer.extend_from_slice(&qid.version().to_le_bytes());
    buffer.extend_from_slice(&qid.path().to_le_bytes());
}

fn put_string(buffer: &mut Vec<u8>, value: &str) {
    let len: u16 = value
        .len()
        .try_into()
        .expect("string length exceeds protocol limit");
    buffer.extend_from_slice(&len.to_le_bytes());
    buffer.extend_from_slice(value.as_bytes());
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn read_exact(&mut self, out: &mut [u8]) -> Result<(), ()> {
        let end = self.pos.saturating_add(out.len());
        if end > self.buf.len() {
            return Err(());
        }
        out.copy_from_slice(&self.buf[self.pos..end]);
        self.pos = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_invalid_paths_during_encoding() {
        let codec = Codec;
        let req = Request {
            tag: 1,
            body: RequestBody::Walk {
                fid: 1,
                newfid: 2,
                wnames: vec!["invalid/component".to_string()],
            },
        };
        assert_eq!(codec.encode_request(&req), Err(CodecError::InvalidPath));
    }

    #[test]
    fn reject_invalid_paths_during_decoding() {
        let codec = Codec;
        let req = Request {
            tag: 1,
            body: RequestBody::Walk {
                fid: 1,
                newfid: 2,
                wnames: vec!["valid".to_string()],
            },
        };
        let mut frame = codec.encode_request(&req).expect("encode frame");
        // Overwrite the walk component count with an invalid value to ensure decode-side
        // validation rejects the frame without trusting the encoded payload.
        frame[15] = 9;
        frame[16] = 0;
        assert_eq!(codec.decode_request(&frame), Err(CodecError::InvalidPath));
    }

    #[test]
    fn detect_truncated_frames() {
        let codec = Codec;
        let req = Request {
            tag: 1,
            body: RequestBody::Open {
                fid: 1,
                mode: OpenMode::read_only(),
            },
        };
        let mut frame = codec.encode_request(&req).expect("encode frame");
        frame.truncate(3);
        assert_eq!(codec.decode_request(&frame), Err(CodecError::Truncated));
    }

    #[test]
    fn detect_invalid_utf8() {
        let codec = Codec;
        let response = Response {
            tag: 1,
            body: ResponseBody::Error {
                code: ErrorCode::Invalid,
                message: "invalid".to_owned(),
            },
        };
        let mut frame = codec.encode_response(&response).expect("encode frame");
        let len = frame.len();
        frame[len - 2] = 0xfe;
        frame[len - 1] = 0xff;
        assert_eq!(codec.decode_response(&frame), Err(CodecError::InvalidUtf8));
    }
}
