// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide batched frame iterators for Secure9P transports.
// Author: Lukas Bower

//! Batched frame iterators for Secure9P transports.

use crate::CodecError;

/// Slice of a single Secure9P frame within a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchFrame<'a> {
    bytes: &'a [u8],
}

impl<'a> BatchFrame<'a> {
    /// Return the raw bytes for the frame.
    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Return the declared frame length.
    #[must_use]
    pub fn declared_len(&self) -> u32 {
        u32::from_le_bytes(
            self.bytes
                .get(..4)
                .expect("batch frame length already validated")
                .try_into()
                .expect("length slice checked"),
        )
    }
}

/// Iterator over length-prefixed Secure9P frames.
#[derive(Debug, Clone)]
pub struct BatchIter<'a> {
    buffer: &'a [u8],
    offset: usize,
    max_frame: Option<u32>,
}

impl<'a> BatchIter<'a> {
    /// Create a new batch iterator without a maximum frame bound.
    #[must_use]
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            offset: 0,
            max_frame: None,
        }
    }

    /// Create a new batch iterator enforcing a maximum frame size.
    #[must_use]
    pub fn with_max_frame(buffer: &'a [u8], max_frame: u32) -> Self {
        Self {
            buffer,
            offset: 0,
            max_frame: Some(max_frame),
        }
    }
}

impl<'a> Iterator for BatchIter<'a> {
    type Item = Result<BatchFrame<'a>, CodecError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.buffer.len() {
            return None;
        }
        if self.buffer.len() - self.offset < 4 {
            return Some(Err(CodecError::Truncated));
        }
        let declared = u32::from_le_bytes(
            self.buffer[self.offset..self.offset + 4]
                .try_into()
                .expect("slice length checked"),
        );
        if let Some(max) = self.max_frame {
            if declared > max {
                return Some(Err(CodecError::FrameTooLarge {
                    declared,
                    max,
                }));
            }
        }
        let declared_usize: usize = match declared.try_into() {
            Ok(value) => value,
            Err(_) => {
                return Some(Err(CodecError::LengthMismatch {
                    declared,
                    actual: self.buffer.len(),
                }))
            }
        };
        if declared < 5 {
            return Some(Err(CodecError::LengthMismatch {
                declared,
                actual: self.buffer.len(),
            }));
        }
        let end = self.offset + declared_usize;
        if end > self.buffer.len() {
            return Some(Err(CodecError::Truncated));
        }
        let frame = BatchFrame {
            bytes: &self.buffer[self.offset..end],
        };
        self.offset = end;
        Some(Ok(frame))
    }
}
