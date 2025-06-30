// CLASSIFICATION: COMMUNITY
// Filename: secure_stream.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

extern crate alloc;
use alloc::vec::Vec;

/// Minimal trait representing readable and writable streams without `std`.
pub trait SimpleStream {
    /// Read bytes into `buf`, returning number of bytes read or an error.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()>;
    /// Write bytes from `buf`, returning number of bytes written or an error.
    fn write(&mut self, buf: &[u8]) -> Result<usize, ()>;
}

/// XOR stream wrapper applying a single-byte key.
pub struct XorStream<S> {
    inner: S,
    key: u8,
}

impl<S> XorStream<S> {
    /// Create a new `XorStream` wrapping `inner` with the provided key.
    pub fn new(inner: S, key: u8) -> Self {
        Self { inner, key }
    }
}

impl<S: SimpleStream> SimpleStream for XorStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let n = self.inner.read(buf)?;
        for b in &mut buf[..n] {
            *b ^= self.key;
        }
        Ok(n)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
        let mut temp = Vec::with_capacity(buf.len());
        temp.extend_from_slice(buf);
        for b in &mut temp {
            *b ^= self.key;
        }
        self.inner.write(&temp)
    }
}

/// Simple in-memory stream used for testing.
pub struct VecStream {
    data: Vec<u8>,
    pos: usize,
}

impl VecStream {
    /// Create a new empty `VecStream`.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            pos: 0,
        }
    }
}

impl SimpleStream for VecStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let remaining = self.data.len().saturating_sub(self.pos);
        if remaining == 0 {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(), remaining);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
        self.data.extend_from_slice(buf);
        Ok(buf.len())
    }
}
