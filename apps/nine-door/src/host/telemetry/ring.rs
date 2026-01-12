// Author: Lukas Bower
// Purpose: Provide bounded append-only ring storage for worker telemetry.

use secure9p_core::{append_only_read_bounds, AppendOnlyOffsetError};

/// Bounded ring buffer for telemetry payloads.
#[derive(Debug, Clone)]
pub struct TelemetryRing {
    buffer: Vec<u8>,
    capacity: usize,
    base_offset: u64,
    next_offset: u64,
}

/// Snapshot of the ring's retained offset window.
#[derive(Debug, Clone, Copy)]
pub struct RingBounds {
    /// Oldest retained offset.
    pub base_offset: u64,
    /// Next append offset.
    pub next_offset: u64,
}

/// Outcome of a ring append.
#[derive(Debug, Clone, Copy)]
pub struct RingWriteOutcome {
    /// Number of bytes written.
    pub count: u32,
    /// Bytes dropped due to wraparound.
    pub dropped_bytes: u64,
    /// New base offset after wrap.
    pub new_base: u64,
}

/// Errors raised when a ring append is invalid.
#[derive(Debug)]
pub enum RingWriteError {
    /// Payload exceeds ring capacity.
    Oversize { requested: usize, capacity: usize },
}

/// Outcome of a ring read.
#[derive(Debug)]
pub struct RingReadOutcome {
    /// Bytes returned to the caller.
    pub data: Vec<u8>,
}

/// Errors raised when a ring read is invalid.
#[derive(Debug)]
pub enum RingReadError {
    /// Requested offset is behind the ring window.
    Stale { requested: u64, available_start: u64 },
}

impl TelemetryRing {
    /// Construct a ring buffer with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            buffer: vec![0; capacity],
            capacity,
            base_offset: 0,
            next_offset: 0,
        }
    }

    /// Return the ring's configured capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Return the ring's retained offset window.
    pub fn bounds(&self) -> RingBounds {
        RingBounds {
            base_offset: self.base_offset,
            next_offset: self.next_offset,
        }
    }

    /// Append telemetry bytes, wrapping and dropping old data as needed.
    pub fn append(&mut self, data: &[u8]) -> Result<RingWriteOutcome, RingWriteError> {
        if data.is_empty() {
            return Ok(RingWriteOutcome {
                count: 0,
                dropped_bytes: 0,
                new_base: self.base_offset,
            });
        }
        if data.len() > self.capacity {
            return Err(RingWriteError::Oversize {
                requested: data.len(),
                capacity: self.capacity,
            });
        }
        let used = self.next_offset.saturating_sub(self.base_offset) as usize;
        let total_needed = used.saturating_add(data.len());
        let dropped_bytes = total_needed.saturating_sub(self.capacity) as u64;
        if dropped_bytes > 0 {
            self.base_offset = self.base_offset.saturating_add(dropped_bytes);
        }

        let start = (self.next_offset % self.capacity as u64) as usize;
        let first_len = (self.capacity - start).min(data.len());
        self.buffer[start..start + first_len].copy_from_slice(&data[..first_len]);
        if first_len < data.len() {
            let remaining = data.len() - first_len;
            self.buffer[..remaining].copy_from_slice(&data[first_len..]);
        }
        self.next_offset = self.next_offset.saturating_add(data.len() as u64);

        Ok(RingWriteOutcome {
            count: data.len() as u32,
            dropped_bytes,
            new_base: self.base_offset,
        })
    }

    /// Read telemetry bytes at the supplied offset.
    pub fn read(&self, offset: u64, count: u32) -> Result<RingReadOutcome, RingReadError> {
        let bounds = self.bounds();
        let read_bounds = append_only_read_bounds(
            offset,
            bounds.base_offset,
            bounds.next_offset,
            count,
        )
        .map_err(|err| match err {
            AppendOnlyOffsetError::Stale {
                requested,
                available_start,
            } => RingReadError::Stale {
                requested,
                available_start,
            },
            AppendOnlyOffsetError::Invalid { .. } => RingReadError::Stale {
                requested: offset,
                available_start: bounds.base_offset,
            },
        })?;
        if read_bounds.len == 0 {
            return Ok(RingReadOutcome { data: Vec::new() });
        }

        let start = (read_bounds.offset % self.capacity as u64) as usize;
        let first_len = (self.capacity - start).min(read_bounds.len);
        let mut out = Vec::with_capacity(read_bounds.len);
        out.extend_from_slice(&self.buffer[start..start + first_len]);
        if first_len < read_bounds.len {
            let remaining = read_bounds.len - first_len;
            out.extend_from_slice(&self.buffer[..remaining]);
        }
        Ok(RingReadOutcome { data: out })
    }
}
