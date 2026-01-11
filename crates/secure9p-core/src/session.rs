// Author: Lukas Bower
// Purpose: Provide Secure9P session tracking primitives for tag windows and queues.

//! Session tracking primitives for Secure9P servers.

use alloc::collections::{BTreeMap, BTreeSet};

/// Tag window errors returned by Secure9P tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagError {
    /// The tag is already in use.
    InUse,
    /// The tag window is full.
    WindowFull,
}

/// Queue depth errors returned by Secure9P tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// The queue is full.
    Full,
}

/// Short write retry policy for transport adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortWritePolicy {
    /// Reject short writes without retrying.
    Reject,
    /// Retry short writes with exponential back-off.
    Retry,
}

/// Default retry budget for short writes.
pub const DEFAULT_SHORT_WRITE_RETRIES: u8 = 3;
/// Default back-off base for short writes.
pub const DEFAULT_SHORT_WRITE_BACKOFF_MS: u64 = 5;

impl ShortWritePolicy {
    /// Return the back-off delay for the provided attempt.
    #[must_use]
    pub fn retry_delay_ms(self, attempt: u8) -> Option<u64> {
        match self {
            ShortWritePolicy::Reject => None,
            ShortWritePolicy::Retry => {
                if attempt >= DEFAULT_SHORT_WRITE_RETRIES {
                    return None;
                }
                let factor = 1u64
                    .checked_shl(attempt as u32)
                    .unwrap_or(u64::MAX);
                Some(DEFAULT_SHORT_WRITE_BACKOFF_MS.saturating_mul(factor))
            }
        }
    }
}

impl Default for ShortWritePolicy {
    fn default() -> Self {
        ShortWritePolicy::Reject
    }
}

/// Limits that govern Secure9P session concurrency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLimits {
    /// Maximum number of in-flight tags per session.
    pub tags_per_session: u16,
    /// Maximum number of frames allowed in a batch.
    pub batch_frames: usize,
    /// Short write retry policy.
    pub short_write_policy: ShortWritePolicy,
}

impl SessionLimits {
    /// Return the queue depth limit implied by these limits.
    #[must_use]
    pub fn queue_depth_limit(self) -> usize {
        (self.tags_per_session as usize).min(self.batch_frames.max(1))
    }
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            tags_per_session: 16,
            batch_frames: 1,
            short_write_policy: ShortWritePolicy::Reject,
        }
    }
}

/// Track the in-flight tag window for a session.
#[derive(Debug, Clone)]
pub struct TagWindow {
    max_tags: u16,
    active: BTreeSet<u16>,
}

impl TagWindow {
    /// Create a new tag window with the specified maximum.
    #[must_use]
    pub fn new(max_tags: u16) -> Self {
        Self {
            max_tags: max_tags.max(1),
            active: BTreeSet::new(),
        }
    }

    /// Attempt to reserve a tag within the window.
    pub fn reserve(&mut self, tag: u16) -> Result<(), TagError> {
        if self.active.contains(&tag) {
            return Err(TagError::InUse);
        }
        if self.active.len() >= self.max_tags as usize {
            return Err(TagError::WindowFull);
        }
        self.active.insert(tag);
        Ok(())
    }

    /// Release a tag from the window.
    pub fn release(&mut self, tag: u16) {
        self.active.remove(&tag);
    }

    /// Return the number of active tags.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Return the maximum tag count.
    #[must_use]
    pub fn max_tags(&self) -> u16 {
        self.max_tags
    }
}

/// Track the outstanding queue depth for a session.
#[derive(Debug, Clone, Copy)]
pub struct QueueDepth {
    max_depth: usize,
    current: usize,
}

impl QueueDepth {
    /// Create a new queue tracker.
    #[must_use]
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth: max_depth.max(1),
            current: 0,
        }
    }

    /// Attempt to reserve queue depth for new work.
    pub fn reserve(&mut self, count: usize) -> Result<(), QueueError> {
        if count == 0 {
            return Ok(());
        }
        if self.current.saturating_add(count) > self.max_depth {
            return Err(QueueError::Full);
        }
        self.current = self.current.saturating_add(count);
        Ok(())
    }

    /// Release queue depth for completed work.
    pub fn release(&mut self, count: usize) {
        self.current = self.current.saturating_sub(count);
    }

    /// Return the current queue depth.
    #[must_use]
    pub fn current(&self) -> usize {
        self.current
    }

    /// Return the maximum queue depth.
    #[must_use]
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }
}

/// Minimal fid table wrapper for Secure9P sessions.
#[derive(Debug, Clone)]
pub struct FidTable<T> {
    entries: BTreeMap<u32, T>,
}

impl<T> FidTable<T> {
    /// Create an empty fid table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Return true if the fid exists in the table.
    #[must_use]
    pub fn contains(&self, fid: &u32) -> bool {
        self.entries.contains_key(fid)
    }

    /// Insert a fid entry.
    pub fn insert(&mut self, fid: u32, value: T) -> Option<T> {
        self.entries.insert(fid, value)
    }

    /// Borrow an entry.
    pub fn get(&self, fid: &u32) -> Option<&T> {
        self.entries.get(fid)
    }

    /// Borrow an entry mutably.
    pub fn get_mut(&mut self, fid: &u32) -> Option<&mut T> {
        self.entries.get_mut(fid)
    }

    /// Remove an entry.
    pub fn remove(&mut self, fid: &u32) -> Option<T> {
        self.entries.remove(fid)
    }
}

impl<T> Default for FidTable<T> {
    fn default() -> Self {
        Self::new()
    }
}
