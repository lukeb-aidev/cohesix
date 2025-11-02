// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{BTreeSet, VecDeque};
use std::str;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use trace_model::{TraceEvent, TraceLevel};

use crate::NineDoorError;
use secure9p_wire::ErrorCode;

const DEFAULT_RING_CAPACITY: usize = 256;

#[derive(Debug)]
struct StoredEvent {
    event: TraceEvent,
    line: String,
}

/// Synthetic trace filesystem backing `/trace/*` providers.
#[derive(Debug)]
pub struct TraceFs {
    ring: VecDeque<StoredEvent>,
    capacity: usize,
    filter: TraceFilter,
    ctl_log: Vec<u8>,
}

impl TraceFs {
    /// Construct a new trace filesystem with the default capacity.
    pub fn new() -> Self {
        let mut fs = Self {
            ring: VecDeque::new(),
            capacity: DEFAULT_RING_CAPACITY,
            filter: TraceFilter::default(),
            ctl_log: Vec::new(),
        };
        fs.record(TraceLevel::Info, "boot", None, "tracefs initialised");
        fs
    }

    /// Record a new trace event if it passes the current filter configuration.
    pub fn record(&mut self, level: TraceLevel, category: &str, task: Option<&str>, message: &str) {
        if !self.filter.allows(level, category) {
            return;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let sequence = self
            .ring
            .back()
            .map(|event| event.event.sequence() + 1)
            .unwrap_or(1);
        let event = TraceEvent::new(sequence, timestamp, level, category, task, message);
        if let Ok(json) = event.to_json_line() {
            if self.ring.len() == self.capacity {
                self.ring.pop_front();
            }
            self.ring.push_back(StoredEvent {
                event,
                line: format!("{json}\n"),
            });
        }
    }

    /// Render the combined trace stream as a byte vector respecting the supplied offset and count.
    pub fn read_events(&self, offset: u64, count: u32) -> Vec<u8> {
        self.read_filtered(offset, count, |_entry| true)
    }

    /// Render the kernel message stream, derived from events tagged with the `kmesg` category.
    pub fn read_kmesg(&self, offset: u64, count: u32) -> Vec<u8> {
        self.read_filtered(offset, count, |entry| entry.event.category() == "kmesg")
    }

    /// Render the per-task trace stream for the supplied identifier.
    pub fn read_task(&self, task: &str, offset: u64, count: u32) -> Vec<u8> {
        self.read_filtered(offset, count, |entry| entry.event.task() == Some(task))
    }

    /// Read the accumulated control log contents.
    pub fn read_ctl(&self, offset: u64, count: u32) -> Vec<u8> {
        slice_with_offset(&self.ctl_log, offset, count)
    }

    /// Process a control payload, updating filters and appending to the control log.
    pub fn write_ctl(&mut self, data: &[u8]) -> Result<u32, NineDoorError> {
        self.ctl_log.extend_from_slice(data);
        let text = str::from_utf8(data).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("trace control must be UTF-8: {err}"),
            )
        })?;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let command: TraceCtlCommand = serde_json::from_str(trimmed).map_err(|err| {
                NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("invalid trace control command: {err}"),
                )
            })?;
            self.filter.update(&command)?;
        }
        Ok(data.len() as u32)
    }

    fn read_filtered<F>(&self, offset: u64, count: u32, predicate: F) -> Vec<u8>
    where
        F: Fn(&StoredEvent) -> bool,
    {
        let mut buffer = String::new();
        for entry in self.ring.iter().filter(|entry| predicate(entry)) {
            buffer.push_str(&entry.line);
        }
        slice_with_offset(buffer.as_bytes(), offset, count)
    }
}

#[derive(Debug)]
struct TraceFilter {
    level: TraceLevel,
    categories: Option<BTreeSet<String>>,
}

impl TraceFilter {
    fn allows(&self, level: TraceLevel, category: &str) -> bool {
        if level.priority() > self.level.priority() {
            return false;
        }
        match &self.categories {
            Some(set) => set.contains(category),
            None => true,
        }
    }

    fn update(&mut self, command: &TraceCtlCommand) -> Result<(), NineDoorError> {
        if let Some(ref set) = command.set.cats {
            if set.is_empty() {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    "category filter must not be empty",
                ));
            }
        }
        if let Some(level) = command.set.level.as_ref() {
            let parsed = TraceLevel::from_str(level).map_err(|_| {
                NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("unknown trace level '{level}'"),
                )
            })?;
            self.level = parsed;
        }
        if let Some(categories) = command.set.cats.as_ref() {
            let mut set = BTreeSet::new();
            for category in categories {
                set.insert(category.clone());
            }
            self.categories = Some(set);
        }
        if command.set.cats.is_none() {
            self.categories = None;
        }
        Ok(())
    }
}

impl Default for TraceFilter {
    fn default() -> Self {
        Self {
            level: TraceLevel::Info,
            categories: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TraceCtlCommand {
    #[serde(rename = "set")]
    set: TraceCtlSet,
}

#[derive(Debug, Deserialize)]
struct TraceCtlSet {
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    cats: Option<Vec<String>>,
}

fn slice_with_offset(data: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = offset as usize;
    if start >= data.len() {
        return Vec::new();
    }
    let end = start.saturating_add(count as usize).min(data.len());
    data[start..end].to_vec()
}
