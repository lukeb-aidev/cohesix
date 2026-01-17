// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a minimal CBOR encoder for UI provider snapshots.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Errors emitted by the CBOR writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CborError {
    /// Output exceeded the configured maximum length.
    TooLarge,
}

/// Minimal CBOR writer for unsigned scalars, text, bytes, maps, and arrays.
#[derive(Debug, Default)]
pub(crate) struct CborWriter {
    buffer: Vec<u8>,
    max_len: usize,
}

impl CborWriter {
    /// Create a new writer with a hard maximum length.
    pub(crate) fn new(max_len: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_len,
        }
    }

    /// Return the encoded bytes.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// Encode a CBOR map with a fixed length.
    pub(crate) fn map(&mut self, len: usize) -> Result<(), CborError> {
        self.write_type_and_len(5, len as u64)
    }

    /// Encode a CBOR array with a fixed length.
    pub(crate) fn array(&mut self, len: usize) -> Result<(), CborError> {
        self.write_type_and_len(4, len as u64)
    }

    /// Encode a CBOR text string.
    pub(crate) fn text(&mut self, value: &str) -> Result<(), CborError> {
        self.write_type_and_len(3, value.len() as u64)?;
        self.push(value.as_bytes())
    }

    /// Encode a CBOR byte string.
    pub(crate) fn bytes(&mut self, value: &[u8]) -> Result<(), CborError> {
        self.write_type_and_len(2, value.len() as u64)?;
        self.push(value)
    }

    /// Encode a CBOR unsigned integer.
    pub(crate) fn unsigned(&mut self, value: u64) -> Result<(), CborError> {
        self.write_type_and_len(0, value)
    }

    /// Encode a CBOR boolean.
    #[allow(dead_code)]
    pub(crate) fn boolean(&mut self, value: bool) -> Result<(), CborError> {
        let byte = if value { 0xf5 } else { 0xf4 };
        self.push_u8(byte)
    }

    /// Encode a CBOR null.
    pub(crate) fn null(&mut self) -> Result<(), CborError> {
        self.push_u8(0xf6)
    }

    fn write_type_and_len(&mut self, major: u8, len: u64) -> Result<(), CborError> {
        let (info, extra) = if len <= 23 {
            (len as u8, None)
        } else if len <= u8::MAX as u64 {
            (24, Some(len.to_be_bytes()[7..8].to_vec()))
        } else if len <= u16::MAX as u64 {
            (25, Some((len as u16).to_be_bytes().to_vec()))
        } else if len <= u32::MAX as u64 {
            (26, Some((len as u32).to_be_bytes().to_vec()))
        } else {
            (27, Some(len.to_be_bytes().to_vec()))
        };
        self.push_u8((major << 5) | info)?;
        if let Some(bytes) = extra {
            self.push(&bytes)?;
        }
        Ok(())
    }

    fn push_u8(&mut self, value: u8) -> Result<(), CborError> {
        self.push(&[value])
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), CborError> {
        if self.buffer.len().saturating_add(bytes.len()) > self.max_len {
            return Err(CborError::TooLarge);
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }
}
