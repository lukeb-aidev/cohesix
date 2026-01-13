// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Minimal in-kernel NineDoor bridge for console-driven control and log access.
// Author: Lukas Bower

#![cfg(feature = "kernel")]
#![allow(dead_code)]

extern crate alloc;

use crate::bootstrap::{boot_tracer, log as boot_log, BootPhase};
use crate::event::AuditSink;
use crate::generated;
use crate::log_buffer;
use crate::serial::DEFAULT_LINE_CAPACITY;
use alloc::vec::Vec;
use core::fmt::{self, Write};
use heapless::{String as HeaplessString, Vec as HeaplessVec};

const LOG_PATH: &str = "/log/queen.log";
const QUEEN_CTL_PATH: &str = "/queen/ctl";
const PROC_BOOT_PATH: &str = "/proc/boot";
const PROC_TESTS_PATH: &str = "/proc/tests";
const PROC_TESTS_QUICK_PATH: &str = "/proc/tests/selftest_quick.coh";
const PROC_TESTS_FULL_PATH: &str = "/proc/tests/selftest_full.coh";
const PROC_TESTS_NEGATIVE_PATH: &str = "/proc/tests/selftest_negative.coh";
const BOOT_HEADER: &str = "Cohesix boot: root-task online";
const MAX_STREAM_LINES: usize = log_buffer::LOG_SNAPSHOT_LINES;
const MAX_WORKERS: usize = 8;
const MAX_WORKER_ID_LEN: usize = 32;
const TELEMETRY_AUDIT_LINE: usize = 128;
const WORKER_ROOT: &str = "/worker/";
const WORKER_TELEMETRY_FILE: &str = "telemetry";

const SELFTEST_QUICK_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_quick.coh"
));
const SELFTEST_FULL_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_full.coh"
));
const SELFTEST_NEGATIVE_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_negative.coh"
));

/// Minimal NineDoor bridge used by the seL4 build until the full Secure9P server is ported.
#[derive(Debug)]
pub struct NineDoorBridge {
    attached: bool,
    next_worker_id: u32,
    telemetry: generated::TelemetryConfig,
    workers: HeaplessVec<WorkerTelemetry, MAX_WORKERS>,
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
            next_worker_id: 1,
            telemetry: generated::telemetry_config(),
            workers: HeaplessVec::new(),
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
        self.handle_queen_ctl(payload)
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
        self.remove_worker(identifier)
    }

    /// Append a payload line to an append-only file.
    pub fn echo(&mut self, path: &str, payload: &str) -> Result<(), NineDoorBridgeError> {
        if payload.contains('\n') || payload.contains('\r') {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        if path == LOG_PATH {
            log_buffer::append_user_line(payload);
            log_buffer::append_log_line(payload);
            return Ok(());
        }
        if path == QUEEN_CTL_PATH {
            return self.handle_queen_ctl(payload);
        }
        if let Some(worker_id) = parse_worker_telemetry_path(path) {
            return self.append_worker_telemetry(worker_id, payload.as_bytes());
        }
        Err(NineDoorBridgeError::InvalidPath)
    }

    /// Read file contents as line-oriented output.
    pub fn cat(
        &self,
        path: &str,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        if path == LOG_PATH {
            return Ok(log_buffer::snapshot_lines::<
                DEFAULT_LINE_CAPACITY,
                MAX_STREAM_LINES,
            >());
        }
        if path == PROC_BOOT_PATH {
            return boot_lines();
        }
        if path == PROC_TESTS_QUICK_PATH {
            return script_lines(SELFTEST_QUICK_SCRIPT);
        }
        if path == PROC_TESTS_FULL_PATH {
            return script_lines(SELFTEST_FULL_SCRIPT);
        }
        if path == PROC_TESTS_NEGATIVE_PATH {
            return script_lines(SELFTEST_NEGATIVE_SCRIPT);
        }
        Err(NineDoorBridgeError::InvalidPath)
    }

    /// List directory entries (not yet supported by the shim bridge).
    pub fn list(
        &self,
        path: &str,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        if path == "/worker" {
            return self.list_workers();
        }
        let entries = match path {
            "/" => &["gpu", "kmesg", "log", "proc", "queen", "trace", "worker"][..],
            "/log" => &["queen.log"][..],
            "/proc" => &["boot", "tests"][..],
            "/proc/tests" => &[
                "selftest_quick.coh",
                "selftest_full.coh",
                "selftest_negative.coh",
            ][..],
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

    fn list_workers(
        &self,
    ) -> Result<
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
        NineDoorBridgeError,
    > {
        let mut output = HeaplessVec::new();
        for worker in self.workers.iter() {
            let mut line = HeaplessString::new();
            line.push_str(worker.id.as_str())
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
            output
                .push(line)
                .map_err(|_| NineDoorBridgeError::BufferFull)?;
        }
        Ok(output)
    }

    fn handle_queen_ctl(&mut self, payload: &str) -> Result<(), NineDoorBridgeError> {
        let command = parse_queen_ctl(payload)?;
        match command {
            QueenCtlCommand::Spawn(target) => self.spawn_worker(target),
            QueenCtlCommand::Kill(worker_id) => self.remove_worker(worker_id),
        }
    }

    fn spawn_worker(&mut self, target: SpawnTarget) -> Result<(), NineDoorBridgeError> {
        let mut id = HeaplessString::<MAX_WORKER_ID_LEN>::new();
        let worker_id = self.next_worker_id;
        write!(id, "worker-{worker_id}").map_err(|_| NineDoorBridgeError::BufferFull)?;
        if self.workers.is_full() {
            return Err(NineDoorBridgeError::BufferFull);
        }
        self.next_worker_id = self.next_worker_id.saturating_add(1);
        let ring = TelemetryRing::new(self.telemetry.ring_bytes_per_worker as usize);
        self.workers
            .push(WorkerTelemetry { id, ring, target })
            .map_err(|_| NineDoorBridgeError::BufferFull)?;
        Ok(())
    }

    fn remove_worker(&mut self, worker_id: &str) -> Result<(), NineDoorBridgeError> {
        let position = self
            .workers
            .iter()
            .position(|worker| worker.id.as_str() == worker_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        let _ = self.workers.swap_remove(position);
        Ok(())
    }

    fn append_worker_telemetry(
        &mut self,
        worker_id: &str,
        payload: &[u8],
    ) -> Result<(), NineDoorBridgeError> {
        let worker = self
            .workers
            .iter_mut()
            .find(|worker| worker.id.as_str() == worker_id)
            .ok_or(NineDoorBridgeError::InvalidPath)?;
        if matches!(self.telemetry.frame_schema, generated::TelemetryFrameSchema::CborV1) {
            return Err(NineDoorBridgeError::InvalidPayload);
        }
        match worker.ring.append(payload) {
            Ok(outcome) => {
                if outcome.dropped_bytes > 0 {
                    log_telemetry_wrap(outcome.dropped_bytes, outcome.new_base);
                }
                Ok(())
            }
            Err(RingWriteError::Oversize {
                requested,
                capacity,
            }) => {
                log_telemetry_quota_reject(requested, capacity);
                Err(NineDoorBridgeError::InvalidPayload)
            }
        }
    }
}

#[derive(Debug)]
struct WorkerTelemetry {
    id: HeaplessString<MAX_WORKER_ID_LEN>,
    ring: TelemetryRing,
    target: SpawnTarget,
}

#[derive(Debug, Clone, Copy)]
enum SpawnTarget {
    Heartbeat,
    Gpu,
}

#[derive(Debug)]
enum QueenCtlCommand<'a> {
    Spawn(SpawnTarget),
    Kill(&'a str),
}

#[derive(Debug, Clone, Copy)]
struct RingWriteOutcome {
    count: u32,
    dropped_bytes: u64,
    new_base: u64,
}

#[derive(Debug)]
enum RingWriteError {
    Oversize { requested: usize, capacity: usize },
}

#[derive(Debug)]
struct TelemetryRing {
    buffer: Vec<u8>,
    capacity: usize,
    base_offset: u64,
    next_offset: u64,
}

impl TelemetryRing {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let mut buffer = Vec::with_capacity(capacity);
        buffer.resize(capacity, 0);
        Self {
            buffer,
            capacity,
            base_offset: 0,
            next_offset: 0,
        }
    }

    fn append(&mut self, data: &[u8]) -> Result<RingWriteOutcome, RingWriteError> {
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
}

fn parse_worker_telemetry_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix(WORKER_ROOT)?;
    let (worker_id, leaf) = rest.split_once('/')?;
    if worker_id.is_empty() || leaf != WORKER_TELEMETRY_FILE {
        return None;
    }
    Some(worker_id)
}

fn parse_queen_ctl(payload: &str) -> Result<QueenCtlCommand<'_>, NineDoorBridgeError> {
    if let Some(target) = parse_json_string_field(payload, "spawn") {
        let target = match target {
            "heartbeat" => SpawnTarget::Heartbeat,
            "gpu" => SpawnTarget::Gpu,
            _ => return Err(NineDoorBridgeError::InvalidPayload),
        };
        return Ok(QueenCtlCommand::Spawn(target));
    }
    if let Some(worker_id) = parse_json_string_field(payload, "kill") {
        return Ok(QueenCtlCommand::Kill(worker_id));
    }
    Err(NineDoorBridgeError::InvalidPayload)
}

fn parse_json_string_field<'a>(input: &'a str, key: &str) -> Option<&'a str> {
    let mut cursor = 0usize;
    while let Some(found) = input[cursor..].find(key) {
        let index = cursor + found;
        let before = index.checked_sub(1)?;
        let after = index + key.len();
        let bytes = input.as_bytes();
        if bytes.get(before) != Some(&b'"') || bytes.get(after) != Some(&b'"') {
            cursor = after;
            continue;
        }
        let mut rest = &input[after + 1..];
        let colon = rest.find(':')?;
        rest = rest[colon + 1..].trim_start();
        if !rest.starts_with('"') {
            return None;
        }
        rest = &rest[1..];
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    None
}

fn log_telemetry_wrap(dropped_bytes: u64, new_base: u64) {
    let mut line = HeaplessString::<TELEMETRY_AUDIT_LINE>::new();
    let _ = write!(
        line,
        "telemetry ring wrap dropped_bytes={} new_base={}",
        dropped_bytes, new_base
    );
    // Keep critical telemetry audits visible in /log/queen.log summaries.
    log_buffer::append_log_line(line.as_str());
    log_buffer::append_user_line(line.as_str());
}

fn log_telemetry_quota_reject(requested: usize, capacity: usize) {
    let mut line = HeaplessString::<TELEMETRY_AUDIT_LINE>::new();
    let _ = write!(
        line,
        "telemetry quota reject bytes={} quota={}",
        requested, capacity
    );
    // Keep critical telemetry audits visible in /log/queen.log summaries.
    log_buffer::append_log_line(line.as_str());
    log_buffer::append_user_line(line.as_str());
}

fn boot_lines(
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    push_boot_line(&mut output, BOOT_HEADER)?;
    // Keep the shim output concise so console ack summaries remain within bounds.
    for line in generated::initial_audit_lines() {
        if line.starts_with("manifest.schema=")
            || line.starts_with("manifest.profile=")
            || line.starts_with("manifest.sha256=")
            || line.starts_with("telemetry.")
            || line.starts_with("event_pump.")
        {
            push_boot_line(&mut output, line)?;
        }
    }
    Ok(output)
}

fn script_lines(
    script: &str,
) -> Result<HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>, NineDoorBridgeError>
{
    let mut output = HeaplessVec::new();
    for line in script.lines() {
        push_boot_line(&mut output, line)?;
    }
    Ok(output)
}

fn push_boot_line(
    output: &mut HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, MAX_STREAM_LINES>,
    line: &str,
) -> Result<(), NineDoorBridgeError> {
    let mut entry: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
    entry
        .push_str(line)
        .map_err(|_| NineDoorBridgeError::BufferFull)?;
    output
        .push(entry)
        .map_err(|_| NineDoorBridgeError::BufferFull)
}

fn truncate(input: &str, limit: usize) -> &str {
    if input.len() <= limit {
        input
    } else {
        &input[..limit]
    }
}
