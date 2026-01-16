// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Optional smoltcp-backed console framing helpers.
// Author: Lukas Bower

//! Optional smoltcp-backed console framing helpers.

use heapless::Vec as HeaplessVec;
use smoltcp::socket::tcp::{SendError as TcpSendError, Socket as TcpSocket};

/// Length prefix size for framed console lines.
pub const FRAME_LEN_BYTES: usize = 4;

/// Errors encountered while encoding or decoding TCP frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// Frame length was invalid or exceeded configured limits.
    InvalidLength,
    /// Frame payload exceeded the decoder capacity.
    PayloadTooLarge,
    /// TCP receive failed.
    Recv,
    /// TCP send failed.
    Send,
}

/// Frame encoder enforcing a maximum total frame length.
#[derive(Debug, Clone, Copy)]
pub struct FrameEncoder {
    max_frame_len: usize,
}

impl FrameEncoder {
    /// Create a new encoder with the supplied maximum frame length.
    #[must_use]
    pub const fn new(max_frame_len: usize) -> Self {
        Self { max_frame_len }
    }

    /// Write a single length-prefixed frame to the socket.
    pub fn write_frame(&self, socket: &mut TcpSocket<'_>, line: &str) -> Result<(), FrameError> {
        let total_len = line.len().saturating_add(FRAME_LEN_BYTES);
        if total_len < FRAME_LEN_BYTES || total_len > self.max_frame_len {
            return Err(FrameError::InvalidLength);
        }
        let len_bytes = (total_len as u32).to_le_bytes();
        match socket.send_slice(&len_bytes) {
            Ok(sent) if sent == len_bytes.len() => {}
            Ok(_) | Err(TcpSendError::InvalidState) | Err(TcpSendError::Unaddressable) => {
                return Err(FrameError::Send)
            }
        }
        match socket.send_slice(line.as_bytes()) {
            Ok(sent) if sent == line.len() => Ok(()),
            Ok(_) | Err(TcpSendError::InvalidState) | Err(TcpSendError::Unaddressable) => {
                Err(FrameError::Send)
            }
        }
    }
}

/// Frame decoder that incrementally assembles frames from TCP payload bytes.
#[derive(Debug, Clone)]
pub struct FrameDecoder<const N: usize> {
    max_frame_len: usize,
    len_buf: [u8; FRAME_LEN_BYTES],
    len_pos: usize,
    payload_len: Option<usize>,
    payload: HeaplessVec<u8, N>,
}

impl<const N: usize> FrameDecoder<N> {
    /// Create a new decoder with the supplied maximum frame length.
    #[must_use]
    pub const fn new(max_frame_len: usize) -> Self {
        Self {
            max_frame_len,
            len_buf: [0u8; FRAME_LEN_BYTES],
            len_pos: 0,
            payload_len: None,
            payload: HeaplessVec::new(),
        }
    }

    /// Attempt to read a single frame from the socket.
    pub fn read_frame(
        &mut self,
        socket: &mut TcpSocket<'_>,
    ) -> Result<Option<HeaplessVec<u8, N>>, FrameError> {
        if !socket.can_recv() {
            return Ok(None);
        }
        let mut completed: Option<HeaplessVec<u8, N>> = None;
        let mut error: Option<FrameError> = None;
        let recv_result = socket.recv(|data| {
            let mut consumed = 0usize;
            for &byte in data {
                if completed.is_some() || error.is_some() {
                    break;
                }
                consumed = consumed.saturating_add(1);
                match self.push_byte(byte) {
                    Ok(Some(frame)) => completed = Some(frame),
                    Ok(None) => {}
                    Err(err) => error = Some(err),
                }
            }
            (consumed, ())
        });
        if recv_result.is_err() || error == Some(FrameError::Recv) {
            return Err(FrameError::Recv);
        }
        if let Some(err) = error {
            return Err(err);
        }
        Ok(completed)
    }

    fn push_byte(&mut self, byte: u8) -> Result<Option<HeaplessVec<u8, N>>, FrameError> {
        if self.payload_len.is_none() {
            self.len_buf[self.len_pos] = byte;
            self.len_pos = self.len_pos.saturating_add(1);
            if self.len_pos == FRAME_LEN_BYTES {
                self.len_pos = 0;
                let total_len = u32::from_le_bytes(self.len_buf) as usize;
                if total_len < FRAME_LEN_BYTES || total_len > self.max_frame_len {
                    self.reset();
                    return Err(FrameError::InvalidLength);
                }
                let payload_len = total_len.saturating_sub(FRAME_LEN_BYTES);
                if payload_len > N {
                    self.reset();
                    return Err(FrameError::PayloadTooLarge);
                }
                self.payload_len = Some(payload_len);
                self.payload.clear();
                if payload_len == 0 {
                    let frame = self.payload.clone();
                    self.reset();
                    return Ok(Some(frame));
                }
            }
            return Ok(None);
        }

        if self.payload.push(byte).is_err() {
            self.reset();
            return Err(FrameError::PayloadTooLarge);
        }
        if let Some(expected) = self.payload_len {
            if self.payload.len() == expected {
                let frame = self.payload.clone();
                self.reset();
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }

    fn reset(&mut self) {
        self.len_pos = 0;
        self.payload_len = None;
        self.payload.clear();
    }
}
