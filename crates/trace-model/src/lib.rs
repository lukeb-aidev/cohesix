// Author: Lukas Bower
//! Fixed-capacity trace data structures shared by kernel and host components.
#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use core::fmt::{self, Write};
use core::str::FromStr;
use heapless::String as HeaplessString;

/// Maximum length of trace category strings.
pub const CATEGORY_CAPACITY: usize = 16;
/// Maximum length of task identifiers recorded in trace events.
pub const TASK_CAPACITY: usize = 32;
/// Maximum length of trace messages stored in the ring buffer.
pub const MESSAGE_CAPACITY: usize = 192;
/// Maximum encoded length of a JSONL trace line.
pub const JSON_LINE_CAPACITY: usize = 256;

/// Fixed-capacity heapless string used for categories.
pub type CategoryString = HeaplessString<CATEGORY_CAPACITY>;
/// Fixed-capacity heapless string used for task identifiers.
pub type TaskString = HeaplessString<TASK_CAPACITY>;
/// Fixed-capacity heapless string used for trace messages.
pub type MessageString = HeaplessString<MESSAGE_CAPACITY>;
/// Heapless buffer used when serialising a trace event to JSONL.
pub type JsonLine = HeaplessString<JSON_LINE_CAPACITY>;

/// Trace severity levels exposed by the trace facade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub enum TraceLevel {
    /// Error level events describe fatal failures.
    Error,
    /// Warning level events describe recoverable issues.
    Warn,
    /// Informational events document expected progress.
    Info,
    /// Debug events surface verbose implementation details.
    Debug,
    /// Trace events capture extremely chatty diagnostics.
    Trace,
}

impl TraceLevel {
    /// Return the string representation used in JSON output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    /// Return the priority associated with the level.
    /// Lower values indicate more severe messages.
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warn => 1,
            Self::Info => 2,
            Self::Debug => 3,
            Self::Trace => 4,
        }
    }
}

impl FromStr for TraceLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            _ => Err(()),
        }
    }
}

/// Errors encountered while rendering trace events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    /// JSON output exceeded the fixed-capacity buffer.
    JsonOverflow,
}

/// Trace event captured by the trace ring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    sequence: u64,
    timestamp_ms: u64,
    level: TraceLevel,
    category: CategoryString,
    task: Option<TaskString>,
    message: MessageString,
}

impl TraceEvent {
    /// Construct a new trace event, truncating fields that exceed the configured limits.
    #[must_use]
    pub fn new(
        sequence: u64,
        timestamp_ms: u64,
        level: TraceLevel,
        category: &str,
        task: Option<&str>,
        message: &str,
    ) -> Self {
        let mut cat = CategoryString::new();
        push_truncated(&mut cat, category);
        let task_string = task.map(|value| {
            let mut task_buf = TaskString::new();
            push_truncated(&mut task_buf, value);
            task_buf
        });
        let mut msg = MessageString::new();
        push_truncated(&mut msg, message);
        Self {
            sequence,
            timestamp_ms,
            level,
            category: cat,
            task: task_string,
            message: msg,
        }
    }

    /// Return the monotonically increasing sequence identifier.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Return the timestamp recorded for the event in milliseconds.
    #[must_use]
    pub const fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }

    /// Return the trace level associated with the event.
    #[must_use]
    pub const fn level(&self) -> TraceLevel {
        self.level
    }

    /// Return the category string associated with the event.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }

    /// Return the optional task identifier associated with the event.
    #[must_use]
    pub fn task(&self) -> Option<&str> {
        self.task.as_deref()
    }

    /// Return the trace message string.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Serialise the event to JSONL form.
    pub fn to_json_line(&self) -> Result<JsonLine, TraceError> {
        let mut line = JsonLine::new();
        push_fmt(
            &mut line,
            format_args!(
                "{{\"seq\":{},\"ts_ms\":{},\"level\":\"{}\",\"cat\":\"",
                self.sequence,
                self.timestamp_ms,
                self.level.as_str()
            ),
        )?;
        append_json_string(&mut line, self.category())?;
        push_str(&mut line, "\"")?;
        if let Some(task) = self.task() {
            push_str(&mut line, ",\"task\":\"")?;
            append_json_string(&mut line, task)?;
            push_str(&mut line, "\"")?;
        }
        push_str(&mut line, ",\"msg\":\"")?;
        append_json_string(&mut line, self.message())?;
        push_str(&mut line, "\"}")?;
        Ok(line)
    }
}

fn push_truncated<const N: usize>(target: &mut HeaplessString<N>, value: &str) {
    for ch in value.chars() {
        if target.push(ch).is_err() {
            break;
        }
    }
}

fn push_fmt(buffer: &mut JsonLine, args: fmt::Arguments<'_>) -> Result<(), TraceError> {
    buffer.write_fmt(args).map_err(|_| TraceError::JsonOverflow)
}

fn push_str(buffer: &mut JsonLine, value: &str) -> Result<(), TraceError> {
    buffer.push_str(value).map_err(|_| TraceError::JsonOverflow)
}

fn push_char(buffer: &mut JsonLine, ch: char) -> Result<(), TraceError> {
    buffer.push(ch).map_err(|_| TraceError::JsonOverflow)
}

fn append_json_string(buffer: &mut JsonLine, value: &str) -> Result<(), TraceError> {
    for ch in value.chars() {
        match ch {
            '"' => {
                push_str(buffer, "\\\"")?;
            }
            '\\' => {
                push_str(buffer, "\\\\")?;
            }
            '\n' => {
                push_str(buffer, "\\n")?;
            }
            '\r' => {
                push_str(buffer, "\\r")?;
            }
            '\t' => {
                push_str(buffer, "\\t")?;
            }
            other if other.is_control() => {
                push_str(buffer, "\\u")?;
                push_fmt(buffer, format_args!("{:04x}", other as u32))?;
            }
            other => {
                push_char(buffer, other)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_event_json_round_trip() {
        let event = TraceEvent::new(1, 42, TraceLevel::Info, "boot", Some("worker-1"), "ready");
        let rendered = event.to_json_line().expect("json rendering");
        assert!(rendered.as_str().contains("\"seq\":1"));
        assert!(rendered.as_str().contains("\"task\":\"worker-1\""));
        assert!(rendered.as_str().contains("\"msg\":\"ready\""));
    }
}
