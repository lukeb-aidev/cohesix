// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide Secure9P session tracking primitives for tag windows and queues.
// Author: Lukas Bower

//! Session tracking primitives for Secure9P servers.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use spin::Mutex;

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

/// Fid table errors returned by sharded tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FidError {
    /// The fid is already active.
    InUse,
    /// The fid was clunked and may not be reused.
    Retired,
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
                let factor = 1u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
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

/// Default number of shards used by [`ShardedFidTable`].
pub const DEFAULT_FID_SHARDS: usize = 16;

#[derive(Debug)]
struct FidShard<T> {
    active: BTreeMap<u32, T>,
    retired: BTreeSet<u32>,
}

impl<T> FidShard<T> {
    fn new() -> Self {
        Self {
            active: BTreeMap::new(),
            retired: BTreeSet::new(),
        }
    }
}

/// Sharded fid table with per-shard locking and clunk retirement.
#[derive(Debug)]
pub struct ShardedFidTable<T> {
    shards: Vec<Mutex<FidShard<T>>>,
}

impl<T> ShardedFidTable<T> {
    /// Create a sharded fid table with the requested number of shards.
    #[must_use]
    pub fn new(shard_count: usize) -> Self {
        let count = shard_count.max(1);
        let mut shards = Vec::with_capacity(count);
        for _ in 0..count {
            shards.push(Mutex::new(FidShard::new()));
        }
        Self { shards }
    }

    /// Return true if the fid is active or has been clunked.
    #[must_use]
    pub fn contains(&self, fid: u32) -> bool {
        let shard = self.shard_for(fid);
        let guard = self.shards[shard].lock();
        guard.active.contains_key(&fid) || guard.retired.contains(&fid)
    }

    /// Insert a fid entry, rejecting reuse after clunk.
    pub fn insert(&self, fid: u32, value: T) -> Result<(), FidError> {
        let shard = self.shard_for(fid);
        let mut guard = self.shards[shard].lock();
        if guard.active.contains_key(&fid) {
            return Err(FidError::InUse);
        }
        if guard.retired.contains(&fid) {
            return Err(FidError::Retired);
        }
        guard.active.insert(fid, value);
        Ok(())
    }

    /// Borrow an entry by cloning it.
    pub fn get(&self, fid: u32) -> Option<T>
    where
        T: Clone,
    {
        let shard = self.shard_for(fid);
        let guard = self.shards[shard].lock();
        guard.active.get(&fid).cloned()
    }

    /// Apply a read-only function to an entry.
    pub fn with_entry<R>(&self, fid: u32, f: impl FnOnce(&T) -> R) -> Option<R> {
        let shard = self.shard_for(fid);
        let guard = self.shards[shard].lock();
        guard.active.get(&fid).map(f)
    }

    /// Apply a mutable function to an entry.
    pub fn with_entry_mut<R>(&self, fid: u32, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let shard = self.shard_for(fid);
        let mut guard = self.shards[shard].lock();
        guard.active.get_mut(&fid).map(f)
    }

    /// Remove an entry and retire the fid.
    pub fn remove(&self, fid: u32) -> Option<T> {
        let shard = self.shard_for(fid);
        let mut guard = self.shards[shard].lock();
        let entry = guard.active.remove(&fid);
        if entry.is_some() {
            guard.retired.insert(fid);
        }
        entry
    }

    fn shard_for(&self, fid: u32) -> usize {
        let idx = fid as usize;
        idx % self.shards.len()
    }
}

impl<T> Default for ShardedFidTable<T> {
    fn default() -> Self {
        Self::new(DEFAULT_FID_SHARDS)
    }
}
