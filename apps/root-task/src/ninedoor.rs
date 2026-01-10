// Author: Lukas Bower
// Purpose: Minimal in-kernel NineDoor bridge for console-driven control and log access.

#![cfg(feature = "kernel")]
#![allow(dead_code)]

use crate::bootstrap::{boot_tracer, log as boot_log, BootPhase};
use crate::event::AuditSink;
use crate::log_buffer;
use crate::serial::DEFAULT_LINE_CAPACITY;
use core::fmt::{self, Write};
use heapless::{String as HeaplessString, Vec as HeaplessVec};

const LOG_PATH: &str = "/log/queen.log";
const MAX_STREAM_LINES: usize = log_buffer::LOG_SNAPSHOT_LINES;

/// Minimal NineDoor bridge used by the seL4 build until the full Secure9P server is ported.
#[derive(Debug)]
pub struct NineDoorBridge {
    attached: bool,
}

/// Errors surfaced by [`NineDoorBridge`] operations.
#[derive(Debug)]
pub enum NineDoorBridgeError {
    /// Command was not recognised by the shim bridge.
    Unsupported(&'static str),
    /// Host failed to acknowledge the attach handshake in time.
    AttachTimeout,
    /// Path was not recognised by the shim bridge.
    InvalidPath,
    /// Buffer capacity was exceeded while appending or formatting output.
    BufferFull,
    /// Payload contained invalid bytes or formatting.
    InvalidPayload,
}

impl fmt::Display for NineDoorBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(cmd) => write!(f, "unsupported command: {cmd}"),
            Self::AttachTimeout => write!(f, "attach handshake timed out"),
            Self::InvalidPath => write!(f, "invalid path"),
            Self::BufferFull => write!(f, "buffer full"),
            Self::InvalidPayload => write!(f, "invalid payload"),
        }
    }
}

impl NineDoorBridge {
    /// Construct a new bridge instance.
    #[must_use]
    pub fn new() -> Self {
        #[cfg(feature = "kernel")]
        {
            boot_log::notify_bridge_created();
        }
        Self {
            attached: false,
        }
    }

    /// Reset per-session state after a console disconnect.
    pub fn reset_session(&mut self) {
        self.attached = false;
    }

    /// Returns `true` when the bridge has successfully attached to the host.
    #[must_use]
    pub fn attached(&self) -> bool {
        self.attached
    }

    /// Handle an `attach` request received from the console.
    pub fn attach(
        &mut self,
        role: &str,
        ticket: Option<&str>,
        audit: &mut dyn AuditSink,
    ) -> Result<(), NineDoorBridgeError> {
        let ticket_repr = ticket.unwrap_or("<none>");
        let mut message = HeaplessString::<128>::new();
        if write!(
            message,
            "nine-door: attach role={role} ticket={ticket_repr}"
        )
        .is_err()
        {
            // Truncated audit line is acceptable.
        }
        audit.info(message.as_str());
        if self.attached {
            return Ok(());
        }
        #[cfg(feature = "kernel")]
        {
            boot_log::notify_bridge_attached();
            if boot_log::bridge_disabled() || boot_log::ep_only_active() {
                self.attached = true;
                boot_tracer().advance(BootPhase::EPAttachOk);
                return Ok(());
            }
            return Err(NineDoorBridgeError::AttachTimeout);
        }
        #[cfg(not(feature = "kernel"))]
        {
            Ok(())
        }
    }

    /// Handle a `tail` request.
    pub fn tail(
        &mut self,
        path: &str,
        audit: &mut dyn AuditSink,
    ) -> Result<(), NineDoorBridgeError> {
        let mut message = HeaplessString::<128>::new();
        if write!(message, "nine-door: tail {path}").is_err() {
            // Truncated audit line is acceptable.
        }
        audit.info(message.as_str());
        Ok(())
    }

    /// Handle a log stream request.
    pub fn log_stream(&mut self, audit: &mut dyn AuditSink) -> Result<(), NineDoorBridgeError> {
        audit.info("nine-door: log stream requested");
        Ok(())
    }

    /// Handle a spawn request.
    pub fn spawn(
        &mut self,
        payload: &str,
        audit: &mut dyn AuditSink,
    ) -> Result<(), NineDoorBridgeError> {
        let mut message = HeaplessString::<128>::new();
        if write!(
            message,
            "nine-door: spawn payload={}...",
            truncate(payload, 64)
        )
        .is_err()
        {
            // Truncated audit line is acceptable.
        }
        audit.info(message.as_str());
        Ok(())
    }

    /// Handle a kill request.
    pub fn kill(
        &mut self,
        identifier: &str,
        audit: &mut dyn AuditSink,
    ) -> Result<(), NineDoorBridgeError> {
        let mut message = HeaplessString::<128>::new();
        if write!(message, "nine-door: kill {identifier}").is_err() {
            // Truncated audit line is acceptable.
        }
        audit.info(message.as_str());
        Ok(())
    }

    /// Append a payload line to an append-only file.
    pub fn echo(&mut self, path: &str, payload: &str) -> Result<(), NineDoorBridgeError> {
        if path != LOG_PATH {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        if payload.contains('\n') || payload.contains('\r') {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        log_buffer::append_log_line(payload);
        Ok(())
    }

    /// Read file contents as line-oriented output.
    pub fn cat(
        &self,
        path: &str,
    ) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
    {
        if path != LOG_PATH {
            return Err(NineDoorBridgeError::InvalidPath);
        }
        Ok(log_buffer::snapshot_lines::<DEFAULT_LINE_CAPACITY, MAX_STREAM_LINES>())
    }

    /// List directory entries (not yet supported by the shim bridge).
    pub fn list(
        &self,
        path: &str,
    ) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
    {
        let entries = match path {
            "/" => &[
                "gpu",
                "kmesg",
                "log",
                "proc",
                "queen",
                "trace",
                "worker",
            ][..],
            "/log" => &["queen.log"][..],
            "/proc" => &["boot"][..],
            "/queen" => &["ctl"][..],
            "/trace" => &["ctl", "events"][..],
            "/worker" | "/gpu" => &[][..],
            _ => return Err(NineDoorBridgeError::InvalidPath),
        };
        let mut output = HeaplessVec::new();
        for entry in entries {
            let mut line = HeaplessString::new();
            line.push_str(entry)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
            output
                .push(line)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        Ok(output)
    }
}

fn truncate(input: &str, limit: usize) -> &str {
    if input.len() <= limit {
        input
    } else {
        &input[..limit]
    }
}
